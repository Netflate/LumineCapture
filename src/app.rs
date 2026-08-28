mod init;
mod input;
mod settings_logic;
mod toolbar_logic;
mod color_popover;

use crate::backend::{initialize_capture, initialize_clipboard, initialize_overlay};
use crate::editor::EditorState;
use crate::editor::dirty::is_dirty;
use crate::profiler::Profiler;
use crate::renderer;
use crate::tools::Tool;
use crate::tools::selection::{global_selection_to_local, selection_edges_for_monitor};
use crate::types::toolbar::Toolbar;
use crate::types::{
    DamageRect, UiPanel, OverlayEvent, Placement, PointerState, SelectionEdges,
    SelectionState, SettingsPanel, ToolSettings, ColorPickerPopover
};
use crate::types::click::DoubleClickTracker;
use crate::utils::{encode_png, get_full_workspace_rect, get_overlapping_monitors, save_to_file};
use crate::types::panel::{tick_panel_animation, AnimatedPanel};

use cosmic_text::{FontSystem, SwashCache};
use std::collections::HashMap;
use tiny_skia::{Pixmap, PixmapPaint, Rect, Transform};

// ************************* //
//      ENTRY POINT          //
// ************************* //

pub async fn make_screenshot(
    wayland_: Option<wayland_client::Connection>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut prof = Profiler::new();

    let icons_handle = std::thread::spawn(init::load_icons_cache);
    let text_handle = std::thread::spawn(|| (SwashCache::new(), FontSystem::new()));

    let conn = wayland_.unwrap();

    let mut overlay = initialize_overlay(conn.clone())?;
    prof.mark("overlay init");

    let outputs = overlay.discovered_outputs().to_vec();
    let present_handle = std::thread::spawn(move || {
        let t = std::time::Instant::now();
        let res = overlay.present().map(|_| ()).map_err(|e| e.to_string());
        (overlay, res, t.elapsed())
    });

    let capture = initialize_capture();
    let screenshots = capture.capture_frame(&outputs).await?;
    prof.mark("capture");

    let clipboard = initialize_clipboard(conn);

    let base_pixmaps: Vec<Pixmap> = init::build_base_pixmap(&screenshots.frames);
    let (canvas, dimmed, annotations_layer) = init::build_layers(&base_pixmaps);
    let placements = init::build_placements(&outputs);
    prof.mark("base_pixmaps + layers + placements");

    drop(screenshots);

    let (swash_cache, font_system) = text_handle.join().expect("Failed to join text thread");
    let icons_cache = icons_handle.join().expect("Failed to join icons thread");
    let mut editor_state = EditorState {
        base: base_pixmaps,
        canvas,
        dimmed,
        selected_tool: Tool::Selection,
        tool_active: false,
        selection: SelectionState::default(),
        placements,
        drag_start: None,
        pointer: PointerState::default(),
        magnifier: None,
        prev_magnifier: None,
        last_mag_update: None,
        mouse_down_left: false,
        toolbar: Toolbar::new(),
        settings_panel: SettingsPanel::new(),
        tool_settings: ToolSettings::default(),
        color_popover: ColorPickerPopover::new(),
        icons_cache,
        annotations: Vec::new(),
        pending: None,
        next_id: 0,
        prev_pending: None,
        undo_stack: Vec::new(),
        redo_stack: Vec::new(),
        damage_rects: Vec::new(),
        selected_annotation: None,
        ann_drag: None,

        annotations_layer,
        annotations_dirty: false,
        font_system,
        swash_cache,
        text_editors: HashMap::new(),
        text_editing: None,

        mod_ctrl: false,
        mod_shift: false,

        click_tracker: DoubleClickTracker::new(),
    };
    prof.mark("editor_state built");

    let (mut overlay, present_res, present_dt) = present_handle
        .join()
        .map_err(|_| "present thread panicked")?;
    present_res.map_err(|msg| -> Box<dyn std::error::Error> { msg.into() })?;
    prof.mark("present() joined");
    prof.mark_external("  ^ present_dt (thread-internal duration)", present_dt);

    init::initial_paint(&mut editor_state, &mut overlay, &mut prof)?;
    prof.dump();

    let mut dirty_mask: u32 = 0;

    let mut save_to_clipboard = false;
    let _save_as_file = true;

    loop {
        let is_animating =  editor_state.toolbar.is_animating() || editor_state.color_popover.is_animating();
        let stepper_holding = editor_state.settings_panel.arrow_held.is_some();
        let timeout = if is_animating || stepper_holding { 16 } else { -1 };

        let ev = overlay.next_event(timeout)?;
        match ev {
            OverlayEvent::Tick => {}
            OverlayEvent::EscapePressed => break,
            OverlayEvent::PointerMove { monitor_idx, x, y } => {
                input::handle_pointer_move(&mut editor_state, monitor_idx, x, y, &mut dirty_mask);
            }
            OverlayEvent::PointerButton { button, pressed } => {
                input::handle_pointer_button(&mut editor_state, button, pressed, &mut dirty_mask);
            }
            OverlayEvent::Undo => {
                editor_state.undo(&mut dirty_mask);
            }
            OverlayEvent::Redo => {
                editor_state.redo(&mut dirty_mask);
            }
            OverlayEvent::SaveToClipboard => {
                drop(overlay);
                save_to_clipboard = true;
                break;
            }
            OverlayEvent::TextInput(ch) => {
                input::handle_text_input(&mut editor_state, ch, &mut dirty_mask);
            }
            OverlayEvent::KeyPress(key) => {
                input::handle_key_press(&mut editor_state, key, &mut dirty_mask);
            }
            OverlayEvent::ModifiersChanged { ctrl, shift } => {
                editor_state.mod_ctrl = ctrl;
                editor_state.mod_shift = shift;
            }
        }

        tick_panel_animation(&mut editor_state.toolbar, &mut editor_state.damage_rects, &mut dirty_mask);
        tick_panel_animation(&mut editor_state.color_popover, &mut editor_state.damage_rects, &mut dirty_mask);
        settings_logic::tick_stepper_arrow_hold(&mut editor_state, &mut dirty_mask);
        
        if editor_state.toolbar.is_animating() {
            if editor_state.settings_panel.visible {
                settings_logic::update_settings_panel(&mut editor_state, &mut dirty_mask);
            }
            if editor_state.color_popover.open {
                color_popover::update_color_popover(&mut editor_state, &mut dirty_mask);
            }
        }
        
        if dirty_mask != 0 {
            let selection_dirty = editor_state.selection.zone != editor_state.selection.prev_zone;

            if editor_state.annotations_dirty {
                let active_text_id = editor_state.text_editing.as_ref().map(|e| e.annotation_id);

                for i in 0..editor_state.base.len() {
                    let offset = (
                        editor_state.placements[i].position.0 as f32,
                        editor_state.placements[i].position.1 as f32,
                    );
                    renderer::rebuild_annotations_layer(
                        &mut editor_state.annotations_layer[i],
                        &editor_state.annotations,
                        editor_state.pending.as_ref(),
                        editor_state.selected_annotation,
                        offset,
                        &mut editor_state.font_system,
                        &mut editor_state.swash_cache,
                        &mut editor_state.text_editors,
                        active_text_id,
                    );
                }
            }

            for i in 0..editor_state.base.len() {
                if is_dirty(dirty_mask, i) {
                    let is_mag_monitor = editor_state
                        .magnifier
                        .as_ref()
                        .is_some_and(|m| m.monitor_idx == i);

                    let (local_sel, prev_local, edges) = selection_render_info(
                        &editor_state.selection.zone,
                        &editor_state.selection.prev_zone,
                        &editor_state.placements[i],
                    );

                    let dirty_rect = editor_state.monitor_dirty_rect(i);
                    let damage: Option<DamageRect> = dirty_rect.as_ref().and_then(|r| {
                        renderer::rect_bounds(r, editor_state.base[i].width(), editor_state.base[i].height())
                    });

                    if i == editor_state.toolbar.monitor_idx
                        && !editor_state.toolbar.dirty
                        && let Some(dirty) = dirty_rect.as_ref()
                        && let Some(tb_r) = editor_state.toolbar.rect()
                    {
                        let intersects = dirty.left() < tb_r.right() && dirty.right() > tb_r.left()
                            && dirty.top() < tb_r.bottom() && dirty.bottom() > tb_r.top();
                        if intersects {
                            editor_state.toolbar.dirty = true;
                        }
                    }

                    let toolbar = if i == editor_state.toolbar.monitor_idx && editor_state.toolbar.dirty {
                        Some(&mut editor_state.toolbar)
                    } else { None };

                    if i == editor_state.settings_panel.monitor_idx
                        && editor_state.settings_panel.visible
                        && !editor_state.settings_panel.dirty
                        && let Some(dirty) = dirty_rect.as_ref()
                        && let Some(sp_r) = editor_state.settings_panel.rect()
                    {
                        let intersects = dirty.left() < sp_r.right() && dirty.right() > sp_r.left()
                            && dirty.top() < sp_r.bottom() && dirty.bottom() > sp_r.top();
                        if intersects {
                            editor_state.settings_panel.dirty = true;
                        }
                    }

                    let settings_panel = if i == editor_state.settings_panel.monitor_idx
                        && editor_state.settings_panel.visible
                        && editor_state.settings_panel.dirty
                    {
                        Some(&mut editor_state.settings_panel)
                    } else {
                        None
                    };

                    if i == editor_state.color_popover.monitor_idx
                        && editor_state.color_popover.open
                        && !editor_state.color_popover.dirty
                        && let Some(dirty) = dirty_rect.as_ref()
                        && let Some(cp_r) = editor_state.color_popover.rect()
                    {
                        let intersects = dirty.left() < cp_r.right() && dirty.right() > cp_r.left()
                            && dirty.top() < cp_r.bottom() && dirty.bottom() > cp_r.top();
                        if intersects {
                            editor_state.color_popover.dirty = true;
                        }
                    }

                    let color_picker = if i==editor_state.color_popover.monitor_idx && editor_state.color_popover.dirty 
                    && editor_state.color_popover.open {
                        Some(&mut editor_state.color_popover)
                    } else {
                        None
                    };

                    let offset = (
                        editor_state.placements[i].position.0 as f32,
                        editor_state.placements[i].position.1 as f32,
                    );

                    renderer::render_frame(&mut renderer::RenderRequest {
                        canvas: &mut editor_state.canvas[i],
                        base: &editor_state.base[i],
                        dimmed: &mut editor_state.dimmed[i],
                        selection: local_sel.as_ref(),
                        prev_selection: prev_local.as_ref(),
                        dirty_rect: dirty_rect.as_ref(),
                        selection_dirty,
                        selection_edges: edges.as_ref(),
                        magnifier: editor_state.magnifier.as_ref(),
                        is_mag_monitor,
                        toolbar,
                        settings_panel,
                        color_picker,
                        icons_cache: &editor_state.icons_cache,
                        annotations_layer: &editor_state.annotations_layer[i],
                        offset,
                        annotations_layer_empty: false,
                        font_system: Some(&mut editor_state.font_system),
                        swash_cache: Some(&mut editor_state.swash_cache),
                    });

                    overlay.stage_frame(i, editor_state.canvas[i].data(), damage)?;
                }
            }
            overlay.flush()?;

            editor_state.selection.prev_zone = editor_state.selection.zone;
            editor_state.toolbar.dirty = false;
            dirty_mask = 0;
            editor_state.prev_pending = editor_state.pending.clone();
            editor_state.annotations_dirty = false;
            editor_state.damage_rects.clear();
            editor_state.settings_panel.dirty = false;
            editor_state.color_popover.dirty = false;
        }
    }

    if save_to_clipboard {
        let final_result = render_final(&mut editor_state);
        let _path = save_to_file(&final_result);
        clipboard.copy_image_to_clipboard(final_result)?;
    }

    Ok(())
}

// ************************* //
//      RENDER HELPERS       //
// ************************* //

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

fn render_final(editor_state: &mut EditorState) -> Vec<u8> {
    let sel = match editor_state.selection.zone {
        Some(s) => s,
        None => match get_full_workspace_rect(&editor_state.placements) {
            Some(r) => r,
            None => return vec![],
        },
    };

    let mask = get_overlapping_monitors(&sel, &editor_state.placements);

    let sel_left = sel.left().floor() as i32;
    let sel_top = sel.top().floor() as i32;
    let sel_right = sel.right().ceil() as i32;
    let sel_bottom = sel.bottom().ceil() as i32;
    let sel_w = (sel_right - sel_left).max(0) as u32;
    let sel_h = (sel_bottom - sel_top).max(0) as u32;
    if sel_w == 0 || sel_h == 0 {
        return vec![];
    }

    let mut out = Pixmap::new(sel_w, sel_h).unwrap();
    for (i, placement) in editor_state.placements.iter().enumerate() {
        if (mask & (1 << i)) == 0 {
            continue;
        }
        let dst_x = placement.position.0 - sel_left;
        let dst_y = placement.position.1 - sel_top;
        out.draw_pixmap(
            dst_x,
            dst_y,
            editor_state.base[i].as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }

    let offset = (sel_left as f32, sel_top as f32);
    for ann in &editor_state.annotations {
        renderer::draw_annotation(
            &mut out,
            ann,
            offset,
            false,
            &mut editor_state.font_system,
            &mut editor_state.swash_cache,
            &mut editor_state.text_editors,
            None,
        );
    }

    encode_png(&out)
}