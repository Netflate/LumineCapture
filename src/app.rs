mod actions;
mod delay;
mod finish;
mod init;
mod input;
mod instant;
mod ocr;
mod panels;
mod scroll;

use crate::backend::ScreenOverlay;
use crate::backend::{FRAME_FALLBACK, initialize_capture, initialize_overlay};
use crate::editor::dirty::{apply_damage_rects, is_dirty, mark_all_dirty, mark_dirty};
use crate::editor::{EditorState, Layers, OcrState, TextState};
use crate::profiler::Profiler;
use crate::renderer;
use crate::theme::anim;
use crate::tools::Tool;
use crate::tools::selection::{global_selection_to_local, selection_edges_for_monitor};
use crate::types::{DamageRect, Mode, Outputs, OverlayEvent, Placement, SelectionEdges};
use crate::ui::panel::UiPanel;
use crate::ui::panel::{AnimatedPanel, panel_to_draw, tick_panel_animation};
use crate::utils::{encode_png, get_full_workspace_rect};

use cosmic_text::SwashCache;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tiny_skia::{IntRect, Rect};

/// download progress updates each tick, but if there is no animation
/// then manually after each 50ms
const DOWNLOAD_POLL: Duration = Duration::from_millis(50);

const POINTER_GRACE: Duration = Duration::from_millis(100);

// ************************* //
//      ENTRY POINT          //
// ************************* //

/// Launch options from the command line.
#[derive(Debug, Clone, Default)]
pub struct Launch {
    pub mode: Mode,
    pub one_monitor: bool,
    pub to: Option<Outputs>,
    pub output: Option<PathBuf>,
    pub speed: bool,
    pub shot_at: Option<Instant>,
}

impl Launch {
    fn outputs(&self) -> Outputs {
        let mut outputs = self.to.unwrap_or(crate::config::get().general.to);
        outputs.save |= self.output.is_some();
        if self.mode == Mode::Ocr {
            outputs.pin = false;
            outputs.save = false;
        } else {
            outputs.primary = false;
        }
        outputs
    }

    fn warn_ignored(&self) {
        let to = self.to.unwrap_or_default();
        if self.mode != Mode::Ocr {
            if to.primary {
                log::warn!("r (primary selection) works only with -r ocr, ignoring it");
            }
            return;
        }
        if to.pin || to.save {
            log::warn!("p and s don't apply to -r ocr, ignoring them");
        }
        if self.output.is_some() {
            log::warn!("--output has no effect with -r ocr, ignoring it, use stdout");
        }
    }
}

pub async fn run(
    conn: wayland_client::Connection,
    launch: Launch,
) -> Result<(), Box<dyn std::error::Error>> {
    launch.warn_ignored();
    if launch.mode != Mode::Editor && launch.outputs().is_empty() {
        return Err("nothing to do with the shot: pass --to or set general.to".into());
    }
    if launch.mode == Mode::Ocr && !crate::ocr::models::OcrModels::load().active_installed() {
        return Err(
            "no OCR model is installed: open the editor, pick the OCR tool and download one".into(),
        );
    }
    match launch.mode {
        Mode::Editor | Mode::Region | Mode::Ocr | Mode::Scroll => {
            make_screenshot(conn, launch).await
        }
        Mode::Full => instant::full(conn, launch).await,
        Mode::Window => instant::window(conn, launch).await,
    }
}

async fn make_screenshot(
    conn: wayland_client::Connection,
    launch: Launch,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut prof = Profiler::new(launch.speed);

    // scrolling capture needs its own connection to grab frames
    let scroll_conn = conn.clone();
    let (mut editor_state, mut overlay) = start_capture(conn, &launch, &mut prof).await?;
    editor_state.to = launch.outputs();
    editor_state.toolbar.to = editor_state.to;
    editor_state.output = launch.output.clone();
    editor_state.mode = launch.mode;
    if editor_state.region() {
        editor_state.ui_hidden = true;
        editor_state.toolbar.start_hidden();
    }
    // when launched with -S, let the user select a region first; scrolling takes over once ready
    if launch.mode == Mode::Scroll {
        editor_state.toasts.show(
            crate::ui::toast::ToastKind::ScrollNeedsRegion,
            &mut editor_state.text.font_system,
        );
    }
    if launch.mode == Mode::Ocr {
        // preparing the ocr engine immediately after launch,
        // by default, in the editor, it does that only after choosing ocr
        editor_state.ocr.runtime.prepare();
    }

    init::initial_paint(&mut editor_state, &mut overlay, &mut prof)?;
    prof.dump();

    let scroll_mode = run_overlay(&mut editor_state, &mut overlay)?;
    // (saving what user chose)
    crate::editor::saved::save(
        &editor_state.tool_settings,
        &editor_state.color_popover.history,
        &editor_state.color_popover.initial_history,
        !editor_state.magnifier.hidden,
    );

    editor_state.ocr.models.shutdown();
    if launch.mode == Mode::Ocr {
        ocr::finish(&mut editor_state).await
    } else if scroll_mode {
        finish::finish_scroll(&scroll_conn, &mut editor_state, &mut overlay, &launch).await
    } else {
        finish::finish_capture(&mut editor_state).await
    }
}

/// Shows the overlay and builds the editor at the same time. The overlay renders
/// on its own thread while screens are captured, running both slow setup steps in parallel.
async fn start_capture(
    conn: wayland_client::Connection,
    launch: &Launch,
    prof: &mut Profiler,
) -> Result<(EditorState, Box<dyn ScreenOverlay>), Box<dyn std::error::Error>> {
    let icons_handle = std::thread::spawn(init::load_icons_cache);
    let system_fonts = std::thread::spawn(crate::fonts::system);

    let mut overlay = initialize_overlay(conn.clone())?;
    prof.mark("overlay init");

    let all = overlay.discovered_outputs().to_vec();
    let hidpi = overlay.hidpi();
    let delayed = launch.shot_at.is_some();
    // with a delay the shot can't happen before the deadline anyway, so the backend is
    // created and prepared in advance (on KDE that's connecting to the session bus).
    // that time is part of the delay, not added to it.
    // without a delay it stays after the present spawn, so present starts sooner
    let mut early = None;
    if delayed {
        let capture = early.insert(initialize_capture(&conn));
        overlay.prefault();
        prof.mark("pool prefaulted");
        delay::wait(launch.shot_at, &**capture, prof).await?;
    }

    let one = launch.one_monitor;
    let targets: Vec<usize> = match one.then(|| overlay.active_output()) {
        Some(Some(idx)) => vec![idx],
        Some(None) => {
            log::warn!("Can't tell which monitor is active, capturing all of them");
            (0..all.len()).collect()
        }
        None => (0..all.len()).collect(),
    };
    let outputs: Vec<_> = targets.iter().map(|&i| all[i].clone()).collect();
    if one {
        prof.mark("active output probed");
    }
    let layers_handle = {
        let placements = init::build_placements(&outputs, hidpi);
        std::thread::spawn(move || init::build_layers(&placements))
    };
    // with delay window is created only after the shot, since at least
    // kwin puts a new window into the taskbar and pins it to the current
    // virtual desktop right at creation, while doing it before capturing
    // doesn't really gain anything, less than 1ms, and might be unstable
    let (shot_tx, shot_rx) = std::sync::mpsc::channel::<()>();
    let present_handle = std::thread::spawn(move || {
        if delayed {
            let _ = shot_rx.recv();
        }
        let t = std::time::Instant::now();
        let res = overlay.present(&targets).map_err(|e| e.to_string());
        (overlay, res, t.elapsed())
    });

    let capture = early.unwrap_or_else(|| initialize_capture(&conn));
    let screenshots = capture.capture_frame(&outputs).await?;
    drop(shot_tx);
    prof.mark("capture");

    let kept: Vec<usize> = screenshots.frames.iter().map(|f| f.output).collect();
    if kept.is_empty() {
        return Err("no monitor was captured".into());
    }
    let shrunk = kept.len() < outputs.len();
    let outputs: Vec<_> = kept.iter().map(|&i| outputs[i].clone()).collect();

    let captures = init::build_captures(screenshots.frames)?;
    let placements = init::build_placements(&outputs, hidpi);
    let layers = layers_handle.join().expect("Failed to join layers thread");
    let (canvas, dimmed, annotations) = if kept.iter().copied().eq(0..kept.len()) && !shrunk {
        layers
    } else {
        init::build_layers(&placements)
    };
    prof.mark("base_pixmaps + layers + placements");

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
            font_system: crate::fonts::bundled(),
            system_fonts: Some(system_fonts),
            swash_cache: SwashCache::new(),
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
    overlay: &mut Box<dyn ScreenOverlay>,
) -> Result<bool, Box<dyn std::error::Error>> {
    let mut dirty_mask: u32 = 0;
    let mut pacing = Pacing::new(editor_state.captures.len());
    let mut annotations_were_hidden = false;
    // wayland on COSMIC unlike KDE doesn't immediately send pointer event
    let mut pointer_wait = Some(Instant::now() + POINTER_GRACE);

    loop {
        let mut timeout = poll_timeout(editor_state);
        if let Some(deadline) = pointer_wait {
            let left = deadline.saturating_duration_since(Instant::now());
            timeout = Some(timeout.map_or(left, |t| t.min(left)));
        }
        // wake up by the fallback even if no other event ever comes
        let waiting = (0..editor_state.captures.len())
            .any(|i| (is_dirty(dirty_mask, i) || pacing.owes_to(i)) && !overlay.ready(i));
        if waiting {
            timeout = Some(timeout.map_or(FRAME_FALLBACK, |t| t.min(FRAME_FALLBACK)));
        }
        poll_background_work(editor_state, &mut dirty_mask);

        match overlay.next_event(timeout)? {
            OverlayEvent::EscapePressed => break,
            ev => {
                match ev {
                    OverlayEvent::PointerMove { .. } => pointer_wait = None,
                    // initiailly toolbar didn't appear as magnifier untill the first pointer event
                    // once again not a conscious chocie, thought it was enough since on kde it always
                    // send pointer event even without moving the pointer if overlay is under it
                    // but cosmic doesn't, so toolbar now appears when the keyboard is focused, which makes
                    // sense even more than pointer
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

        // hand off to the scrolling session once a region is selected and mouse is released
        if editor_state.mode == Mode::Scroll
            && editor_state.selection.zone.is_some()
            && !editor_state.input.mouse_down
        {
            return Ok(true);
        }

        overlay.set_cursor(input::compute_cursor(editor_state));
        tick_panels(editor_state, &mut dirty_mask);

        let annotations_hidden = editor_state.selected_tool == Tool::Ocr;
        if annotations_hidden != annotations_were_hidden {
            annotations_were_hidden = annotations_hidden;
            damage_hidden_annotations(editor_state, &mut dirty_mask);
        }

        // pass only when someone can show it
        let drawable = (0..editor_state.captures.len())
            .any(|i| (is_dirty(dirty_mask, i) || pacing.owes_to(i)) && overlay.ready(i));
        if drawable {
            render_dirty_monitors(
                editor_state,
                overlay.as_mut(),
                dirty_mask,
                annotations_hidden,
                &mut pacing,
            )?;
            clear_frame_state(editor_state, &mut dirty_mask, &pacing);
        }
    }

    Ok(false)
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
        Some(anim::frame())
    } else if editor_state.ocr.models.is_downloading() {
        Some(DOWNLOAD_POLL)
    } else {
        None
    }
}

fn poll_background_work(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    if let Some(fonts) = editor_state.text.system_fonts.take_if(|f| f.is_finished())
        && let Ok(db) = fonts.join()
    {
        crate::fonts::add_system(&mut editor_state.text.font_system, db);
    }
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
        OverlayEvent::Tick
        | OverlayEvent::EscapePressed
        | OverlayEvent::Focus { .. }
        | OverlayEvent::FrameDone { .. } => {}
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
            input::handle_modifiers(editor_state, ctrl, shift, dirty_mask);
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
    if has_annotations && let Some(workspace) = get_full_workspace_rect(&editor_state.placements) {
        editor_state
            .damage_rects
            .push(crate::editor::DamageZone::Global(workspace));
        mark_all_dirty(dirty_mask, editor_state.placements.len());
    }
}

/// what has to be redrawed
#[derive(Clone, Copy)]
enum Redraw {
    Area(Rect),
    Whole,
}

impl Redraw {
    fn join(self, other: Redraw) -> Redraw {
        match (self, other) {
            (Redraw::Area(a), Redraw::Area(b)) => a.join(&b).map_or(Redraw::Whole, Redraw::Area),
            _ => Redraw::Whole,
        }
    }

    fn area(self) -> Option<Rect> {
        match self {
            Redraw::Area(rect) => Some(rect),
            Redraw::Whole => None,
        }
    }
}

/// every monitor draws on its own frame callbacks
/// until then the damage piles up in `owed`
struct Pacing {
    owed: Vec<Option<Redraw>>,
    dimmed_for: Vec<Option<Rect>>,
}

impl Pacing {
    fn new(monitors: usize) -> Self {
        Self {
            owed: vec![None; monitors],
            dimmed_for: vec![None; monitors],
        }
    }

    fn owes_to(&self, monitor_idx: usize) -> bool {
        self.owed[monitor_idx].is_some()
    }
}

/// What every monitor of one frame draws from.
struct Frame {
    active_text_id: Option<u64>,
    scan_badge: Option<(Rect, f32)>,
    current_color: tiny_skia::Color,
    outline_color: tiny_skia::Color,
    annotations_hidden: bool,
}

fn render_dirty_monitors(
    editor_state: &mut EditorState,
    overlay: &mut dyn ScreenOverlay,
    mut dirty_mask: u32,
    annotations_hidden: bool,
    pacing: &mut Pacing,
) -> Result<(), Box<dyn std::error::Error>> {
    let frame = Frame {
        active_text_id: editor_state.text.editing.as_ref().map(|e| e.annotation_id),
        scan_badge: editor_state.ocr.scan_started.and_then(|started| {
            Some((
                editor_state.ocr.view.region()?,
                started.elapsed().as_secs_f32(),
            ))
        }),
        current_color: panels::settings::current_color(editor_state),
        outline_color: panels::settings::current_outline_color(editor_state),
        annotations_hidden,
    };
    editor_state.refresh_blurs(&mut dirty_mask);

    for i in 0..editor_state.captures.len() {
        if is_dirty(dirty_mask, i) {
            rebuild_layer(editor_state, i, &frame);
            let area = editor_state
                .monitor_dirty_rect(i)
                .map_or(Redraw::Whole, Redraw::Area);
            pacing.owed[i] = Some(pacing.owed[i].map_or(area, |owed| owed.join(area)));
        }
        let Some(owed) = pacing.owed[i] else {
            continue;
        };
        // the layer is rebuilt right away, as its damage will be cleared afterwards
        // (only the composite and the commit wait for the frame callback)
        if !overlay.ready(i) {
            continue;
        }
        pacing.owed[i] = None;
        let damage = render_monitor(
            editor_state,
            i,
            &frame,
            owed.area(),
            &mut pacing.dimmed_for[i],
        );
        overlay.stage_frame(i, editor_state.canvas[i].data(), damage)?;
    }
    overlay.flush()?;

    Ok(())
}

fn rebuild_layer(editor_state: &mut EditorState, i: usize, frame: &Frame) {
    let layer_dirty = editor_state.monitor_layer_dirty_rect(i);
    if layer_dirty.is_none() && !editor_state.annotations_dirty {
        return;
    }
    let placement = &editor_state.placements[i];
    renderer::rebuild_annotations_layer(
        &mut renderer::Painter::new(&mut editor_state.annotations_layer[i])
            .with_scale(placement.scale)
            .with_text(
                &mut editor_state.text.font_system,
                &mut editor_state.text.swash_cache,
            )
            .with_origin(placement.offset()),
        &editor_state.annotations,
        &mut editor_state.text.editors,
        &editor_state.blur.cache,
        frame.active_text_id,
        layer_dirty,
    );
}

/// Draws one monitor's canvas and returns the region that changed
fn render_monitor(
    editor_state: &mut EditorState,
    i: usize,
    frame: &Frame,
    dirty_rect: Option<Rect>,
    dimmed_for: &mut Option<Rect>,
) -> Option<DamageRect> {
    // No loupe while reading text: it sits right where the
    // pointer is selecting and hides the line under it.
    let picking = editor_state.picking();
    let is_mag_monitor = editor_state.selected_tool != Tool::Ocr
        && editor_state.magnifier_shown()
        && editor_state
            .magnifier
            .current
            .as_ref()
            .is_some_and(|m| m.monitor_idx == i);

    // to not redraw the whole dim layer on every frame, only the changed part is redrawn;
    // that's why each monitor has to remember its own previous selection,
    // to compare the new one against it
    //
    // (a monitor may skip frames, so the shared prev_zone can be outdated for it)
    let zone = editor_state.selection.zone;
    let (local_sel, prev_local, edges) =
        selection_render_info(&zone, dimmed_for, &editor_state.placements[i]);
    let selection_dirty = *dimmed_for != zone;
    *dimmed_for = zone;

    let offset = editor_state.placements[i].offset();
    let scale = editor_state.placements[i].scale;

    let damage = dirty_rect.as_ref().and_then(|r| {
        renderer::rect_bounds(
            &renderer::scale_rect(r, scale)?,
            editor_state.canvas[i].width(),
            editor_state.canvas[i].height(),
        )
    });

    let dragged_index = editor_state.dragged_index();
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
        selection_dirty,
        selection_edges: edges.as_ref(),
        magnifier: editor_state.magnifier.current.as_ref(),
        mag_label: picking,
        is_mag_monitor,
        toolbar,
        settings_panel,
        current_color: frame.current_color,
        outline_color: frame.outline_color,
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
        pending_pen_baked: editor_state.pending_pen_baked,
        is_pending_selected: editor_state.ann_drag.is_some(),
        dragged_index,
        selected_annotation: editor_state.selected_annotation.filter(|_| !hidden),
        annotations: &editor_state.annotations,
        blurs: &editor_state.blur.cache,
        text: renderer::TextContext {
            font_system: &mut editor_state.text.font_system,
            swash_cache: &mut editor_state.text.swash_cache,
            editors: &mut editor_state.text.editors,
        },
        active_text_id: frame.active_text_id,

        ocr_view: (editor_state.selected_tool == Tool::Ocr && editor_state.ocr.view.is_active())
            .then_some(&editor_state.ocr.view),
        ocr_scan: frame.scan_badge,
        monitor_idx: i,
        toasts: &editor_state.toasts,
        size: (
            editor_state.placements[i].size.0 as f32,
            editor_state.placements[i].size.1 as f32,
        ),
        scale,
    });

    damage
}

/// Everything the next frame starts from a clean slate.
fn clear_frame_state(editor_state: &mut EditorState, dirty_mask: &mut u32, pacing: &Pacing) {
    editor_state.selection.prev_zone = editor_state.selection.zone;
    editor_state.prev_pending = editor_state.pending.clone();
    // set per pointer move it could point at a spot never drawn (several moves per frame)
    editor_state.magnifier.prev = editor_state.magnifier.current;
    editor_state.annotations_dirty = false;
    editor_state.damage_rects.clear();
    editor_state.layer_damage_rects.clear();
    let drawn = |monitor_idx: usize| !pacing.owes_to(monitor_idx);
    if drawn(editor_state.toolbar.monitor_idx) {
        editor_state.toolbar.dirty = false;
    }
    if drawn(editor_state.settings_panel.monitor_idx) {
        editor_state.settings_panel.dirty = false;
    }
    if drawn(editor_state.color_popover.monitor_idx) {
        editor_state.color_popover.dirty = false;
    }
    if drawn(editor_state.model_popover.monitor_idx) {
        editor_state.model_popover.dirty = false;
    }
    *dirty_mask = 0;
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

    // checks is it for outline or text color
    if crate::editor::edits::active_annotation_idx(editor_state).is_some() {
        let color = panels::settings::swatch_color(editor_state, editor_state.color_popover.target);
        editor_state.color_popover.select_color(color);
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
pub(crate) fn render_final(editor_state: &mut EditorState) -> Option<(Vec<u8>, (i32, i32), f32)> {
    let sel = match editor_state.selection.zone {
        Some(s) => s,
        None => get_full_workspace_rect(&editor_state.placements)?,
    };

    let scale = renderer::capture_scale(&editor_state.captures, &editor_state.placements, sel);
    let area = editor_state.blur_area(sel, scale);
    let (mut out, mut origin, _) = renderer::composite(
        &editor_state.captures,
        &editor_state.placements,
        area,
        Some(scale),
    )?;

    let offset = (origin.0 as f32 * scale, origin.1 as f32 * scale);
    let mut scaled_editors = HashMap::new();
    // empty on purpose: `out` already has the screenshot and annotations below
    let in_place = HashMap::new();
    for ann in &editor_state.annotations {
        let scaled;
        let (ann, editors) = if scale == 1.0 {
            (ann, &mut editor_state.text.editors)
        } else {
            scaled = ann.scaled(scale);
            (&scaled, &mut scaled_editors)
        };
        renderer::draw_annotation(
            &mut renderer::Painter::new(&mut out)
                .with_text(
                    &mut editor_state.text.font_system,
                    &mut editor_state.text.swash_cache,
                )
                .with_origin(offset),
            ann,
            false,
            editors,
            &in_place,
            None,
        );
    }

    if area != sel
        && let Some(px) = sel.round_out()
        && let Some(cut) = IntRect::from_xywh(
            ((px.x() - origin.0) as f32 * scale).round() as i32,
            ((px.y() - origin.1) as f32 * scale).round() as i32,
            (px.width() as f32 * scale).round() as u32,
            (px.height() as f32 * scale).round() as u32,
        )
    {
        out = out.clone_rect(cut)?;
        origin = (px.x(), px.y());
    }

    Some((encode_png(&out), origin, scale))
}
