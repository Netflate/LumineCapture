mod actions;
mod init;
mod input;
mod panels;

use crate::backend::notify::{self, Notice};
use crate::backend::{initialize_capture, initialize_clipboard, initialize_overlay};
use crate::backend::ScreenOverlay;
use crate::editor::{EditorState, Layers, OcrState, TextState};
use crate::editor::dirty::{apply_damage_rects, is_dirty, mark_all_dirty, mark_dirty};
use crate::profiler::Profiler;
use crate::renderer;
use crate::theme::anim;
use crate::tools::Tool;
use crate::tools::selection::{global_selection_to_local, selection_edges_for_monitor};
use crate::ui::panel::{AnimatedPanel, panel_to_draw, tick_panel_animation};
use crate::types::{DamageRect, Finish, OverlayEvent, Placement, SelectionEdges};
use crate::ui::panel::UiPanel;
use crate::utils::{encode_png, get_full_workspace_rect, save_to_file};

use cosmic_text::{FontSystem, SwashCache};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tiny_skia::Rect;

/// download progress updates each tick, but if there is no animation
/// then manually after each 50ms
const DOWNLOAD_POLL: Duration = Duration::from_millis(50);

const POINTER_GRACE: Duration = Duration::from_millis(100);

// ************************* //
//      ENTRY POINT          //
// ************************* //

pub async fn make_screenshot(
    conn: wayland_client::Connection,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut prof = Profiler::new();

    let (mut editor_state, mut overlay) = start_capture(conn, &mut prof).await?;

    init::initial_paint(&mut editor_state, &mut overlay, &mut prof)?;
    prof.dump();

    run_overlay(&mut editor_state, overlay)?;

    editor_state.ocr.models.shutdown();
    finish_capture(&mut editor_state).await
}

/// Shows the overlay and builds the editor at the same time. The overlay renders
/// on its own thread while screens are captured, running both slow setup steps in parallel.
async fn start_capture(
    conn: wayland_client::Connection,
    prof: &mut Profiler,
) -> Result<(EditorState, Box<dyn ScreenOverlay>), Box<dyn std::error::Error>> {
    let icons_handle = std::thread::spawn(init::load_icons_cache);
    let text_handle = std::thread::spawn(|| (SwashCache::new(), FontSystem::new()));

    let mut overlay = initialize_overlay(conn.clone())?;
    prof.mark("overlay init");

    let outputs = overlay.discovered_outputs().to_vec();
    let present_handle = std::thread::spawn(move || {
        let t = std::time::Instant::now();
        let res = overlay.present().map(|_| ()).map_err(|e| e.to_string());
        (overlay, res, t.elapsed())
    });

    let capture = initialize_capture(&conn);
    let screenshots = capture.capture_frame(&outputs).await?;
    prof.mark("capture");

    let kept: Vec<usize> = screenshots.frames.iter().map(|f| f.output).collect();
    if kept.is_empty() {
        return Err("no monitor was captured".into());
    }
    let shrunk = kept.len() < outputs.len();
    let outputs: Vec<_> = kept.iter().map(|&i| outputs[i].clone()).collect();

    let captures = init::build_captures(&screenshots.frames)?;
    let placements = init::build_placements(&outputs);
    let (canvas, dimmed, annotations) = init::build_layers(&placements);
    prof.mark("base_pixmaps + layers + placements");

    drop(screenshots);

    let (swash_cache, font_system) = text_handle.join().expect("Failed to join text thread");
    let icons_cache = icons_handle.join().expect("Failed to join icons thread");

    let editor_state = EditorState::new(
        Layers {
            captures,
            canvas,
            dimmed,
            annotations,
        },
        placements,
        icons_cache,
        TextState {
            font_system,
            swash_cache,
            editors: HashMap::new(),
            editing: None,
        },
        build_ocr_state(),
    );
    prof.mark("editor_state built");

    let (mut overlay, present_res, present_dt) = present_handle
        .join()
        .map_err(|_| "present thread panicked")?;
    present_res.map_err(|msg| -> Box<dyn std::error::Error> { msg.into() })?;
    if shrunk {
        overlay.retain_outputs(&kept)?;
    }
    for (i, capture) in editor_state.captures.iter().enumerate() {
        let pixmap = &capture.pixmap;
        overlay.set_background(i, pixmap.data(), pixmap.width(), pixmap.height())?;
    }
    prof.mark("backgrounds uploaded");
    prof.mark("present() joined");
    prof.mark_external("  ^ present_dt (thread-internal duration)", present_dt);

    Ok((editor_state, overlay))
}

fn build_ocr_state() -> OcrState {
    let models = crate::ocr::models::OcrModels::load();
    let settings = crate::ocr::settings::EngineSettings::load();
    let mut runtime = crate::ocr::OcrRuntime::new(crate::ocr::daemon::resolve_mode(&settings));
    if let Some(files) = models.active().and_then(|idx| models.files(idx)) {
        runtime.load(files);
    }

    OcrState {
        runtime,
        models,
        view: crate::ocr::OcrView::default(),
        redrag: false,
        redrag_from: None,
        await_region: false,
        scan_started: None,
    }
}

/// Runs the overlay until the user completes or cancels the selection.
/// By the time this returns, the overlay is already closed and capture is complete.
fn run_overlay(
    editor_state: &mut EditorState,
    mut overlay: Box<dyn ScreenOverlay>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut dirty_mask: u32 = 0;
    let mut annotations_were_hidden = false;
    let mut pointer_wait = Some(Instant::now() + POINTER_GRACE);

    loop {
        let mut timeout = poll_timeout(editor_state);
        if let Some(deadline) = pointer_wait {
            let left = deadline.saturating_duration_since(Instant::now());
            timeout = Some(timeout.map_or(left, |t| t.min(left)));
        }
        poll_background_work(editor_state, &mut dirty_mask);

        match overlay.next_event(timeout)? {
            OverlayEvent::EscapePressed => break,
            ev => {
                match ev {
                    OverlayEvent::PointerMove { .. } => pointer_wait = None,
                    OverlayEvent::Focus { monitor_idx } if pointer_wait.is_some() => {
                        editor_state.input.pointer.monitor_idx = monitor_idx;
                    }
                    _ => {}
                }
                handle_event(editor_state, ev, &mut dirty_mask);
            }
        }

        if pointer_wait.is_some_and(|deadline| Instant::now() >= deadline) {
            pointer_wait = None;
            panels::toolbar::update_toolbar(editor_state, &mut dirty_mask);
            apply_damage_rects(editor_state, &mut dirty_mask);
        }

        if editor_state.finish.is_some() || editor_state.cancel {
            break;
        }

        overlay.set_cursor(input::compute_cursor(editor_state));
        tick_panels(editor_state, &mut dirty_mask);

        let annotations_hidden = editor_state.selected_tool == Tool::Ocr;
        if annotations_hidden != annotations_were_hidden {
            annotations_were_hidden = annotations_hidden;
            damage_hidden_annotations(editor_state, &mut dirty_mask);
        }

        if dirty_mask != 0 {
            render_dirty_monitors(
                editor_state,
                overlay.as_mut(),
                dirty_mask,
                annotations_hidden,
            )?;
            clear_frame_state(editor_state, &mut dirty_mask);
        }
    }

    Ok(())
}

/// system can't sleep waiting only for events, since we have animation and 
/// download beat and etc
fn poll_timeout(editor_state: &EditorState) -> Option<Duration> {
    let is_animating = editor_state.toolbar.is_animating()
        || editor_state.color_popover.is_animating()
        || editor_state.model_popover.is_animating()
        || editor_state.toasts.is_animating();
    let stepper_holding = editor_state.settings_panel.arrow_held.is_some();
    let ocr_working = editor_state.ocr.runtime.needs_poll();

    if is_animating || stepper_holding || ocr_working {
        Some(anim::FRAME)
    } else if editor_state.ocr.models.is_downloading() {
        Some(DOWNLOAD_POLL)
    } else {
        None
    }
}

fn poll_background_work(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    if let Some(result) = editor_state.ocr.runtime.poll() {
        crate::tools::ocr::finish_ocr(editor_state, result, dirty_mask);
        panels::settings::update_settings_panel(editor_state, dirty_mask);
    }
    panels::model::tick_model_downloads(editor_state, dirty_mask);
    if editor_state.ocr.runtime.is_busy() {
        crate::tools::ocr::tick_scan_badge(editor_state, dirty_mask);
    }
}

fn handle_event(editor_state: &mut EditorState, ev: OverlayEvent, dirty_mask: &mut u32) {
    match ev {
        OverlayEvent::Tick | OverlayEvent::EscapePressed | OverlayEvent::Focus { .. } => {}
        OverlayEvent::PointerMove { monitor_idx, x, y } => {
            input::handle_pointer_move(editor_state, monitor_idx, x, y, dirty_mask);
        }
        OverlayEvent::PointerButton { button, pressed } => {
            input::handle_pointer_button(editor_state, button, pressed, dirty_mask);
        }
        OverlayEvent::Key { chord, text } => {
            input::handle_key(editor_state, chord, text, dirty_mask);
        }
        OverlayEvent::ModifiersChanged { ctrl, shift } => {
            editor_state.input.ctrl = ctrl;
            editor_state.input.shift = shift;
        }
        OverlayEvent::Scroll { delta_x, delta_y } => {
            input::handle_scroll(editor_state, delta_x, delta_y, dirty_mask);
        }
    }
}

/// tick animation
fn tick_panels(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    tick_panel_animation(
        &mut editor_state.toolbar,
        &mut editor_state.damage_rects,
        dirty_mask,
    );
    tick_panel_animation(
        &mut editor_state.color_popover,
        &mut editor_state.damage_rects,
        dirty_mask,
    );
    tick_panel_animation(
        &mut editor_state.model_popover,
        &mut editor_state.damage_rects,
        dirty_mask,
    );
    panels::settings::tick_stepper_arrow_hold(editor_state, dirty_mask);

    let toast_place = {
        let idx = editor_state.input.pointer.monitor_idx;
        let placement = &editor_state.placements[idx];
        crate::ui::toast::ToastPlace {
            monitor_idx: idx,
            size: (placement.size.0 as f32, placement.size.1 as f32),
            focus: editor_state
                .selection
                .zone
                .as_ref()
                .and_then(|zone| global_selection_to_local(zone, placement)),
        }
    };
    editor_state
        .toasts
        .tick(toast_place, &mut editor_state.damage_rects, dirty_mask);

    if editor_state.toolbar.is_animating()
        || editor_state.color_popover.is_animating()
        || editor_state.model_popover.is_animating()
    {
        if editor_state.settings_panel.visible {
            panels::settings::update_settings_panel(editor_state, dirty_mask);
        }
        if editor_state.color_popover.is_visible() {
            panels::color::update_color_popover(editor_state, dirty_mask);
        }
        if editor_state.model_popover.is_visible() {
            panels::model::update_model_popover(editor_state, dirty_mask);
        }
    }
}

// ocr tool hides annotations, so need to mark diryt everything
fn damage_hidden_annotations(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let has_annotations =
        !editor_state.annotations.is_empty() || editor_state.pending_pen_baked != 0;
    if has_annotations
        && let Some(workspace) = get_full_workspace_rect(&editor_state.placements)
    {
        editor_state
            .damage_rects
            .push(crate::editor::DamageZone::Global(workspace));
        mark_all_dirty(dirty_mask, editor_state.placements.len());
    }
}

/// What every monitor of one frame draws from.
struct Frame {
    selection_dirty: bool,
    active_text_id: Option<u64>,
    scan_badge: Option<(Rect, f32)>,
    current_color: tiny_skia::Color,
    annotations_hidden: bool,
}

fn render_dirty_monitors(
    editor_state: &mut EditorState,
    overlay: &mut dyn ScreenOverlay,
    dirty_mask: u32,
    annotations_hidden: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let frame = Frame {
        selection_dirty: editor_state.selection.zone != editor_state.selection.prev_zone,
        active_text_id: editor_state.text.editing.as_ref().map(|e| e.annotation_id),
        scan_badge: editor_state.ocr.scan_started.and_then(|started| {
            Some((
                editor_state.ocr.view.region()?,
                started.elapsed().as_secs_f32(),
            ))
        }),
        current_color: panels::settings::current_color(editor_state),
        annotations_hidden,
    };

    for i in 0..editor_state.captures.len() {
        if !is_dirty(dirty_mask, i) {
            continue;
        }
        let damage = render_monitor(editor_state, i, &frame);
        overlay.stage_frame(i, editor_state.canvas[i].data(), damage)?;
    }
    overlay.flush()?;

    Ok(())
}

/// Draws one monitor's canvas and returns the region that changed
fn render_monitor(editor_state: &mut EditorState, i: usize, frame: &Frame) -> Option<DamageRect> {
    // No loupe while reading text: it sits right where the
    // pointer is selecting and hides the line under it.
    let picking = editor_state.picking();
    let is_mag_monitor = editor_state.selected_tool != Tool::Ocr
        && editor_state
            .magnifier
            .current
            .as_ref()
            .is_some_and(|m| m.monitor_idx == i);

    let (local_sel, prev_local, edges) = selection_render_info(
        &editor_state.selection.zone,
        &editor_state.selection.prev_zone,
        &editor_state.placements[i],
    );

    let dirty_rect = editor_state.monitor_dirty_rect(i);
    let layer_dirty = editor_state.monitor_layer_dirty_rect(i);
    let offset = editor_state.placements[i].offset();

    if layer_dirty.is_some() || editor_state.annotations_dirty {
        renderer::rebuild_annotations_layer(
            &mut editor_state.annotations_layer[i],
            &editor_state.annotations,
            offset,
            &mut editor_state.text.font_system,
            &mut editor_state.text.swash_cache,
            &mut editor_state.text.editors,
            frame.active_text_id,
            layer_dirty,
        );
    }

    let damage = dirty_rect.as_ref().and_then(|r| {
        renderer::rect_bounds(
            r,
            editor_state.canvas[i].width(),
            editor_state.canvas[i].height(),
        )
    });

    let dirty = dirty_rect.as_ref();
    let toolbar = panel_to_draw(&mut editor_state.toolbar, i, dirty);
    let settings_panel = panel_to_draw(&mut editor_state.settings_panel, i, dirty);
    let color_picker = panel_to_draw(&mut editor_state.color_popover, i, dirty);
    let model_list = panel_to_draw(&mut editor_state.model_popover, i, dirty);

    let hidden = frame.annotations_hidden;

    renderer::render_frame(&mut renderer::RenderRequest {
        canvas: &mut editor_state.canvas[i],
        capture: &editor_state.captures[i],
        dimmed: &mut editor_state.dimmed[i],
        selection: local_sel.as_ref(),
        prev_selection: prev_local.as_ref(),
        dirty_rect: dirty_rect.as_ref(),
        selection_dirty: frame.selection_dirty,
        selection_edges: edges.as_ref(),
        magnifier: editor_state.magnifier.current.as_ref(),
        mag_label: picking,
        is_mag_monitor,
        toolbar,
        settings_panel,
        current_color: frame.current_color,
        color_picker,
        model_popover: model_list,
        icons_cache: &editor_state.icons_cache,
        annotations_layer: &editor_state.annotations_layer[i],
        offset,
        // Nothing is ever drawn into the layer without also
        // landing in `annotations` or bumping the baked-pen
        // counter, so this is exactly "the layer is blank" -
        // and it skips a full-canvas composite on every whole
        // frame.
        annotations_layer_empty: hidden
            || (editor_state.annotations.is_empty() && editor_state.pending_pen_baked == 0),
        pending: editor_state.pending.as_ref().filter(|_| !hidden),
        is_pending_selected: editor_state.ann_drag.is_some(),
        selected_annotation: editor_state.selected_annotation.filter(|_| !hidden),
        annotations: &editor_state.annotations,
        text: Some(renderer::TextContext {
            font_system: &mut editor_state.text.font_system,
            swash_cache: &mut editor_state.text.swash_cache,
            editors: &mut editor_state.text.editors,
        }),
        active_text_id: frame.active_text_id,

        ocr_view: (editor_state.selected_tool == Tool::Ocr && editor_state.ocr.view.is_active())
            .then_some(&editor_state.ocr.view),
        ocr_scan: frame.scan_badge,
        monitor_idx: i,
        toasts: &editor_state.toasts,
    });

    damage
}

/// Everything the next frame starts from a clean slate.
fn clear_frame_state(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    editor_state.selection.prev_zone = editor_state.selection.zone;
    editor_state.prev_pending = editor_state.pending.clone();
    editor_state.annotations_dirty = false;
    editor_state.damage_rects.clear();
    editor_state.layer_damage_rects.clear();
    editor_state.toolbar.dirty = false;
    editor_state.settings_panel.dirty = false;
    editor_state.color_popover.dirty = false;
    editor_state.model_popover.dirty = false;
    *dirty_mask = 0;
}

/// Saves, copies or pins the finished capture and tells the user about it.
async fn finish_capture(editor_state: &mut EditorState) -> Result<(), Box<dyn std::error::Error>> {
    let Some(finish) = editor_state.finish else {
        return Ok(());
    };
    let Some((png, _)) = render_final(editor_state, finish != Finish::Pin) else {
        return Ok(());
    };

    if finish == Finish::Pin
        && let Err(e) = crate::backend::wayland::pin::spawn(&png, 1.0)
    {
        notify::send(Notice::PinFailed(e.to_string())).await;
    }
    let saved = if finish == Finish::Save || crate::config::get().general.save_always {
        let native = if finish == Finish::Pin {
            render_final(editor_state, true).map(|(png, _)| png)
        } else {
            None
        };
        match save_to_file(native.as_deref().unwrap_or(&png)) {
            Ok(path) => Some(path),
            Err(e) => {
                notify::send(Notice::SaveFailed(e.to_string())).await;
                None
            }
        }
    } else {
        None
    };
    match finish {
        Finish::Save => {
            if let Some(path) = saved {
                notify::send(Notice::Saved(path)).await;
            }
        }
        Finish::Copy => match initialize_clipboard().copy_image_to_clipboard(png) {
            Ok(()) => notify::send(Notice::Copied(saved)).await,
            Err(e) => notify::send(Notice::CopyFailed(e.to_string())).await,
        },
        Finish::Pin => {}
    }

    Ok(())
}

// ************************* //
//      RENDER HELPERS       //
// ************************* //



/// Undo and redo can change what the panels show, so refresh is needed
fn refresh_panels_after_history(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    panels::settings::update_settings_panel(editor_state, dirty_mask);
    editor_state.settings_panel.dirty = true;
    let sp_mon = editor_state.settings_panel.monitor_idx;
    if let Some(rect) = editor_state.settings_panel.rect() {
        editor_state.damage_local(sp_mon, rect);
    }
    mark_dirty(dirty_mask, sp_mon);

    if !editor_state.color_popover.open {
        return;
    }

    if let Some(ann_idx) = crate::editor::edits::active_annotation_idx(editor_state)
        && let Some(ann) = editor_state.annotations.get(ann_idx)
    {
        editor_state.color_popover.select_color(ann.color);
    }
    panels::color::update_color_popover(editor_state, dirty_mask);
    editor_state.color_popover.dirty = true;
    let cp_mon = editor_state.color_popover.monitor_idx;
    if let Some(rect) = editor_state.color_popover.rect() {
        editor_state.damage_local(cp_mon, rect);
    }
    mark_dirty(dirty_mask, cp_mon);
}

pub fn selection_render_info(
    selection: &Option<Rect>,
    prev_selection: &Option<Rect>,
    placement: &Placement,
) -> (Option<Rect>, Option<Rect>, Option<SelectionEdges>) {
    let local_sel = selection
        .as_ref()
        .and_then(|sel| global_selection_to_local(sel, placement));
    let prev_local = prev_selection
        .as_ref()
        .and_then(|sel| global_selection_to_local(sel, placement));

    let mut edges = None;
    if let (Some(sel), Some(_)) = (selection.as_ref(), local_sel.as_ref()) {
        edges = Some(selection_edges_for_monitor(sel, placement));
    }
    (local_sel, prev_local, edges)
}

/// Returns the final PNG and the global position of its top-left corner.
/// position is required for the pin feature ^^^      
fn render_final(
    editor_state: &mut EditorState,
    native: bool,
) -> Option<(Vec<u8>, (i32, i32))> {
    let sel = match editor_state.selection.zone {
        Some(s) => s,
        None => get_full_workspace_rect(&editor_state.placements)?,
    };

    let (mut out, origin, scale) = renderer::composite(
        &editor_state.captures,
        &editor_state.placements,
        sel,
        (!native).then_some(1.0),
    )?;

    let offset = (origin.0 as f32 * scale, origin.1 as f32 * scale);
    let mut scaled_editors = HashMap::new();
    for ann in &editor_state.annotations {
        let scaled;
        let (ann, editors) = if scale == 1.0 {
            (ann, &mut editor_state.text.editors)
        } else {
            scaled = ann.scaled(scale);
            (&scaled, &mut scaled_editors)
        };
        renderer::draw_annotation(
            &mut out,
            ann,
            offset,
            false,
            &mut editor_state.text.font_system,
            &mut editor_state.text.swash_cache,
            editors,
            None,
        );
    }

    Some((encode_png(&out), origin))
}
