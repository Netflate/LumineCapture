use crate::backend::{ScreenOverlay, initialize_capture, initialize_clipboard, initialize_overlay};
use crate::editor::EditorState;
use crate::profiler::Profiler;
use crate::renderer::{self};
use crate::tools::selection::{global_selection_to_local, selection_edges_for_monitor};
use crate::tools::{
    Tool, dispatch_button, dispatch_deactivate, dispatch_key, dispatch_move, dispatch_text,
};
use crate::types::toolbar::{
    TOOLBAR_HEIGHT, TOOLBAR_OFFSET, TOOLBAR_PADDING, Toolbar, ToolbarAction, ToolbarAnimation,
    ToolbarButton, ToolbarItem, ToolbarSide,
};
use crate::types::{
    DamageRect, MAG_FRAME_INTERVAL, MagnifierState, MonitorFrame, MouseButton, OverlayEvent,
    Placement, PointerState, SelectionEdges, SelectionState, SpecialKey, Output, icons
};
use crate::utils::{encode_png, get_overlapping_monitors, global_point_to_local, save_to_file};
use cosmic_text::{FontSystem, SwashCache};
use std::collections::HashMap;
use std::time::Instant;
use tiny_skia::{Pixmap, PixmapPaint, Rect, Transform};
use usvg::Tree;

// ************************* //
//      ENTRY POINT          //
// ************************* //

pub async fn make_screenshot(
    wayland_: Option<wayland_client::Connection>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut prof = Profiler::new();

    let icons_handle = std::thread::spawn(load_icons_cache);
    let text_handle = std::thread::spawn(|| (SwashCache::new(), FontSystem::new()));

    let conn = wayland_.unwrap();

    let mut overlay = initialize_overlay(conn.clone())?;
    prof.mark("overlay init");

    let outputs = overlay.discovered_outputs().to_vec();
    // we don't work with capture workspace functions, instead of use capture on each screen
    // so we need to get output list first of all 
    let present_handle = std::thread::spawn(move || {
        let t = std::time::Instant::now();
        let res = overlay.present().map(|_| ()).map_err(|e| e.to_string());
        (overlay, res, t.elapsed())
    });

    let capture = initialize_capture( );
    let screenshots = capture.capture_frame(&outputs).await?;
    prof.mark("capture");

    let clipboard = initialize_clipboard(conn);

    let base_pixmaps: Vec<Pixmap> = build_base_pixmap(&screenshots.frames);
    let (canvas, dimmed, annotations_layer) = build_layers(&base_pixmaps);
    let placements = build_placements(&outputs);
    prof.mark("base_pixmaps + layers + placements");

    drop(screenshots);
    // 4 may 2026 : ~75mb memory usage while screenshoting on kde linux with 2 hd monitors
    // not ideal, could be resolved with rendering in shm itself

    let (swash_cache, font_system) = text_handle.join().expect("Failed to join text thread");
    let icon_cache = icons_handle.join().expect("Failed to join icons thread");
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
        icon_cache,
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
    };
    prof.mark("editor_state built");

    let (mut overlay, present_res, present_dt) = present_handle
        .join()
        .map_err(|_| "present thread panicked")?;
    present_res.map_err(|msg| -> Box<dyn std::error::Error> { msg.into() })?;
    prof.mark("present() joined");
    prof.mark_external("  ^ present_dt (thread-internal duration)", present_dt);

    initial_paint(&mut editor_state, &mut overlay, &mut prof)?;
    prof.dump();

    let mut dirty_mask: u32 = 0;
    let mut selection_dirty = false;

    // todo: Remove hardcoded settings, will be saved later in a config-like structure
    let mut save_to_clipboard = false;
    let _save_as_file = true;

    loop {
        // rendering happens only if x event happens
        // if we have some kind of animation we need to force rendering every 16 ms
        // to create a 60 fps animation
        let opacity_animating = {
            let tb = &editor_state.toolbar;
            let target_opacity = if tb.interferes { 0.1 } else { 1.0 };
            (tb.opacity - target_opacity).abs() > 0.001
        };
        let timeout = if editor_state.toolbar.anim.is_some() || opacity_animating {
            16
        } else {
            -1
        };

        let ev = overlay.next_event(timeout)?;
        match ev {
            OverlayEvent::Tick => {}
            OverlayEvent::EscapePressed => break,
            OverlayEvent::PointerMove { monitor_idx, x, y } => {
                handle_pointer_move(
                    &mut editor_state,
                    monitor_idx,
                    x,
                    y,
                    &mut dirty_mask,
                    &mut selection_dirty,
                );
            }
            OverlayEvent::PointerButton { button, pressed } => {
                handle_pointer_button(&mut editor_state, button, pressed, &mut dirty_mask);
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
                handle_text_input(&mut editor_state, ch, &mut dirty_mask);
            }
            OverlayEvent::KeyPress(key) => {
                handle_key_press(&mut editor_state, key, &mut dirty_mask);
            }
            OverlayEvent::ModifiersChanged { ctrl, shift } => {
                editor_state.mod_ctrl = ctrl;
                editor_state.mod_shift = shift;
            }
        }

        tick_toolbar_anim(&mut editor_state, &mut dirty_mask);

        if dirty_mask != 0 {
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
                    let dirty_rect = editor_state.monitor_dirty_rect(i, selection_dirty);
                    let damage: Option<DamageRect> = dirty_rect.as_ref().and_then(|r| {
                        renderer::rect_bounds(
                            r,
                            editor_state.base[i].width(),
                            editor_state.base[i].height(),
                        )
                    });

                    if i == editor_state.toolbar.monitor_idx
                        && !editor_state.toolbar.dirty
                        && let Some(dirty) = dirty_rect.as_ref()
                    {
                        let tb = &editor_state.toolbar;
                        let tb_rect = Rect::from_xywh(
                            tb.position.0,
                            tb.position.1,
                            tb.size.0,
                            TOOLBAR_HEIGHT,
                        );
                        if let Some(tb_r) = tb_rect {
                            let intersects = dirty.left() < tb_r.right()
                                && dirty.right() > tb_r.left()
                                && dirty.top() < tb_r.bottom()
                                && dirty.bottom() > tb_r.top();
                            if intersects {
                                editor_state.toolbar.dirty = true;
                            }
                        }
                    }

                    let toolbar =
                        if i == editor_state.toolbar.monitor_idx && editor_state.toolbar.dirty {
                            Some(&mut editor_state.toolbar)
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
                        selection_edges: edges.as_ref(),
                        selection_dirty,
                        magnifier: editor_state.magnifier.as_ref(),
                        is_mag_monitor,
                        toolbar,
                        icons_cache: &editor_state.icon_cache,
                        annotations_layer: &editor_state.annotations_layer[i],
                        offset,
                        annotations_layer_empty: false,

                    });

                    overlay.stage_frame(i, editor_state.canvas[i].data(), damage)?;
                }
            }
            overlay.flush()?;
            selection_dirty = false;
            editor_state.toolbar.dirty = false;
            dirty_mask = 0;
            editor_state.prev_pending = editor_state.pending.clone();
            editor_state.annotations_dirty = false;
        }
    }

    if save_to_clipboard {
        let final_result = render_final(&mut editor_state);
        // it doesn't make sense, but while this program in wip
        // it will have one option - save to clipboard AND file
        let _path = save_to_file(&final_result);
        clipboard.copy_image_to_clipboard(final_result)?;
    }

    Ok(())
}
// ************************* //
//      INITIALIZATION       //
// ************************* //

fn build_base_pixmap(frames: &Vec<MonitorFrame>) -> Vec<Pixmap> {
    frames
        .iter()
        .enumerate()
        .map(|(monitor_idx, f)| {
            let (src_w, src_h) = (f.pw_width, f.pw_height);
            let mut src_pixmap =
                Pixmap::new(src_w, src_h).expect("Failed to create source Pixmap for monitor");

            let row_bytes = (src_w as usize) * 4;
            let src_stride = f.pw_stride as usize;
            let dst = src_pixmap.data_mut();

            if src_stride < row_bytes {
                panic!(
                    "Invalid stride for monitor {}: stride={} row_bytes={}",
                    monitor_idx, src_stride, row_bytes
                );
            }

            let needed = src_stride * (src_h as usize);
            let src = f.pixels.get(..needed).unwrap_or_else(|| {
                panic!(
                    "Not enough pixel data for monitor {}: have={} need={}",
                    monitor_idx,
                    f.pixels.len(),
                    needed
                )
            });

            for row in 0..(src_h as usize) {
                let src_off = row * src_stride;
                let dst_off = row * row_bytes;
                dst[dst_off..dst_off + row_bytes]
                    .copy_from_slice(&src[src_off..src_off + row_bytes]);
            }

            let (logical_w_i32, logical_h_i32) =
                f.info.size.unwrap_or((src_w as i32, src_h as i32));
            let logical_w = logical_w_i32.max(1) as u32;
            let logical_h = logical_h_i32.max(1) as u32;

            if logical_w == src_w && logical_h == src_h {
                return src_pixmap;
            }

            let mut logical_pixmap = Pixmap::new(logical_w, logical_h)
                .expect("Failed to create logical Pixmap for monitor");
            let sx = logical_w as f32 / src_w as f32;
            let sy = logical_h as f32 / src_h as f32;
            logical_pixmap.draw_pixmap(
                0,
                0,
                src_pixmap.as_ref(),
                &PixmapPaint::default(),
                Transform::from_row(sx, 0.0, 0.0, sy, 0.0, 0.0),
                None,
            );
            logical_pixmap
        })
        .collect()
}

fn build_layers(base_pixmaps: &[Pixmap]) -> (Vec<Pixmap>, Vec<Pixmap>, Vec<Pixmap>) {
    let len = base_pixmaps.len();
    let mut canvases = Vec::with_capacity(len);
    let mut dimmed_layers = Vec::with_capacity(len);
    let mut annotation_layers = Vec::with_capacity(len);

    for p in base_pixmaps {
        let w = p.width();
        let h = p.height();

        canvases.push(Pixmap::new(w, h).expect("Failed to create canvas"));
        dimmed_layers.push(Pixmap::new(w, h).expect("Failed to create dimmed"));
        annotation_layers.push(Pixmap::new(w, h).expect("Failed to create annotations"));
    }

    (canvases, dimmed_layers, annotation_layers)
}

fn build_placements(outputs: &[Output]) -> Vec<Placement> {
    outputs.iter().map(|o| Placement {
        position: o.info.logical_position.unwrap_or(o.info.location),
        size: o.info.logical_size.unwrap_or_else(|| {
            o.info.modes.iter().find(|m| m.current)
                .map(|m| m.dimensions)
                .unwrap_or((0, 0))
        }),
    }).collect()
}

fn load_icons_cache() -> HashMap<ToolbarButton, Tree> {
    let mut cache = HashMap::new();
    let opt = usvg::Options::default();

    for item in crate::types::toolbar::TOOLBAR_ITEMS {
        let ToolbarItem::Button(button) = item else {
            continue;
        };
        let (svg_str, _) = icons::get_svg(button);
        let tree =
            Tree::from_str(svg_str, &opt).expect("Critical: Failed to parse embedded SVG icon");
        cache.insert(*button, tree);
    }
    cache
}

fn initial_paint(
    editor_state: &mut EditorState,
    overlay: &mut Box<dyn ScreenOverlay>,
    prof: &mut Profiler,
) -> Result<(), Box<dyn std::error::Error>> {
    let n = editor_state.base.len();

    let EditorState {
        base,
        canvas,
        dimmed,
        annotations_layer,
        placements,
        icon_cache,
        magnifier,
        selection,
        ..
    } = editor_state;

    let sel_zone = &selection.zone;
    let prev_zone = &selection.prev_zone;
    let icon_cache_ref = &*icon_cache;
    let magnifier_ref = &*magnifier;

    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(n);

        for (i, (((base_i, canvas_i), dimmed_i), ann_i)) in base
            .iter()
            .zip(canvas.iter_mut())
            .zip(dimmed.iter_mut())
            .zip(annotations_layer.iter_mut())
            .enumerate()
        {
            let placement = &placements[i];

            handles.push(scope.spawn(move || {
                let (local_sel, prev_local, edges) =
                    selection_render_info(sel_zone, prev_zone, placement);

                renderer::init_dimming(dimmed_i, base_i, &local_sel);

                renderer::render_frame(&mut renderer::RenderRequest {
                    canvas: canvas_i,
                    base: base_i,
                    dimmed: dimmed_i,
                    selection: local_sel.as_ref(),
                    prev_selection: prev_local.as_ref(),
                    dirty_rect: None,
                    selection_edges: edges.as_ref(),
                    selection_dirty: false,
                    magnifier: magnifier_ref.as_ref(),
                    is_mag_monitor: false,
                    toolbar: None,
                    icons_cache: icon_cache_ref,
                    offset: (0.0, 0.0),
                    annotations_layer: ann_i,
                    annotations_layer_empty: true,
                });
            }));
        }

        for h in handles {
            h.join().expect("initial_paint render thread panicked");
        }
    });

    prof.mark(&format!("dimming+render for {n} monitors (parallel)"));

    for i in 0..n {
        overlay.stage_frame(i, editor_state.canvas[i].data(), None)?;
    }
    overlay.flush()?;
    prof.mark("frames staged + flushed");
    Ok(())
}

// ************************* //
//      INPUT HANDLING       //
// ************************* //

fn handle_pointer_move(
    editor_state: &mut EditorState,
    monitor_idx: usize,
    x: f64,
    y: f64,
    dirty_mask: &mut u32,
    selection_dirty: &mut bool,
) {
    let global = (
        editor_state.placements[monitor_idx].position.0 as f64 + x,
        editor_state.placements[monitor_idx].position.1 as f64 + y,
    );
    let (current_monitor_idx, local_x, local_y) =
        global_point_to_local(&editor_state.placements, global, monitor_idx, (x, y));
    update_pointer(
        editor_state,
        current_monitor_idx,
        (local_x, local_y),
        global,
    );
    update_magnifier(editor_state, dirty_mask);
    dispatch_move(
        editor_state.selected_tool,
        editor_state,
        global,
        selection_dirty,
        dirty_mask,
    );
    apply_damage_rects(editor_state, dirty_mask);
    update_toolbar(editor_state, dirty_mask);
}

fn handle_pointer_button(
    editor_state: &mut EditorState,
    button: MouseButton,
    pressed: bool,
    dirty_mask: &mut u32,
) {
    if matches!(button, MouseButton::Left)
        && pressed
        && let Some(tb_button) = toolbar_hit_test(&editor_state.toolbar, editor_state.pointer.local)
    {
        editor_state.toolbar.dirty = true;
        mark_dirty(dirty_mask, editor_state.toolbar.monitor_idx);

        if let Some(ToolbarItem::Button(button)) = editor_state.toolbar.items.get(tb_button) {
            match button {
                ToolbarButton::Tool(tool) => {
                    dispatch_deactivate(editor_state.selected_tool, editor_state, dirty_mask);
                    editor_state.selected_tool = *tool;
                    editor_state.toolbar.selected = Some(tb_button);
                }
                ToolbarButton::Action(ToolbarAction::SideChange) => {
                    editor_state.toolbar.current_side = match editor_state.toolbar.current_side {
                        ToolbarSide::Top => ToolbarSide::Bottom,
                        ToolbarSide::Bottom => ToolbarSide::Top,
                    };
                }
            }
            editor_state.toolbar.dirty = true;
            update_toolbar(editor_state, dirty_mask);
        }
        return;
    }

    dispatch_button(
        editor_state.selected_tool,
        editor_state,
        button,
        pressed,
        dirty_mask,
    );
    apply_damage_rects(editor_state, dirty_mask);

    if matches!(button, MouseButton::Left) && !pressed {
        update_toolbar(editor_state, dirty_mask);
    }
}

fn update_pointer(
    editor_state: &mut EditorState,
    monitor_idx: usize,
    local: (f64, f64),
    global: (f64, f64),
) {
    editor_state.pointer = PointerState::new(monitor_idx, local, global);
}

fn update_magnifier(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let now = Instant::now();
    if let Some(last) = editor_state.last_mag_update
        && now.duration_since(last) < MAG_FRAME_INTERVAL
    {
        return;
    }
    editor_state.last_mag_update = Some(now);

    let monitor_idx = editor_state.pointer.monitor_idx;
    let local = editor_state.pointer.local;

    if let Some(mag) = editor_state.magnifier.as_ref() {
        if mag.monitor_idx != monitor_idx {
            mark_dirty(dirty_mask, mag.monitor_idx);
        }
        editor_state.prev_magnifier = Some(MagnifierState {
            monitor_idx: mag.monitor_idx,
            pos: mag.pos,
        });
    } else {
        editor_state.prev_magnifier = None;
    }

    editor_state.magnifier = Some(MagnifierState {
        monitor_idx,
        pos: local,
    });
    mark_dirty(dirty_mask, monitor_idx);
}

// ************************* //
//         TOOLBAR           //
// ************************* //

fn update_toolbar(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let monitor_idx = editor_state.pointer.monitor_idx;
    let placement = &editor_state.placements[monitor_idx];
    let (pos_x, pos_y) = toolbar_position(
        editor_state.toolbar.current_side,
        placement,
        editor_state.toolbar.toolbar_width(),
    );
    let from_y = match editor_state.toolbar.current_side {
        ToolbarSide::Top => -TOOLBAR_HEIGHT,
        ToolbarSide::Bottom => placement.size.1 as f32,
    };
    let interferes = toolbar_interferes(editor_state);
    let pointer_local = editor_state.pointer.local;

    let tb = &mut editor_state.toolbar;
    if tb.interferes != interferes {
        tb.dirty = true;
        mark_dirty(dirty_mask, tb.monitor_idx);
    }
    tb.interferes = interferes;

    if tb.monitor_idx != monitor_idx || tb.position != (pos_x, pos_y) {
        tb.anim = Some(ToolbarAnimation {
            start: Instant::now(),
            duration_ms: 200,
            from_y,
            to_y: pos_y,
        });
        tb.render_y = from_y;
        tb.prev_position = tb.position;
        tb.prev_monitor_idx = tb.monitor_idx;
        mark_dirty(dirty_mask, tb.monitor_idx);
        if tb.monitor_idx != monitor_idx {
            mark_dirty(dirty_mask, monitor_idx);
        }
        tb.dirty = true;
        tb.position = (pos_x, pos_y);
        tb.monitor_idx = monitor_idx;
    }

    let button = toolbar_hit_test(tb, pointer_local);
    if button != tb.hovered {
        tb.hovered = button;
        tb.dirty = true;
        mark_dirty(dirty_mask, tb.monitor_idx);
    }
}

fn tick_toolbar_anim(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let tb = &mut editor_state.toolbar;

    // smooth opacity animation
    let now = Instant::now();
    let dt = tb
        .last_tick
        .map(|t| now.duration_since(t).as_secs_f32())
        .unwrap_or(0.016);
    tb.last_tick = Some(now);

    let opacity_per_sec = 5.0;
    let target_opacity = if tb.interferes { 0.1 } else { 1.0 };
    if (tb.opacity - target_opacity).abs() > 0.001 {
        let delta = opacity_per_sec * dt;
        if tb.opacity < target_opacity {
            tb.opacity = (tb.opacity + delta).min(target_opacity);
        } else {
            tb.opacity = (tb.opacity - delta).max(target_opacity);
        }
        tb.dirty = true;
        mark_dirty(dirty_mask, tb.monitor_idx);
    }

    // ease out animation
    let Some(anim) = &tb.anim else { return };

    let t = (anim.start.elapsed().as_millis() as f32 / anim.duration_ms as f32).clamp(0.0, 1.0);
    let t_eased = 1.0 - (1.0 - t).powi(3);

    tb.render_y = anim.from_y + (anim.to_y - anim.from_y) * t_eased;
    tb.dirty = true;
    mark_dirty(dirty_mask, tb.monitor_idx);

    if t >= 1.0 {
        tb.render_y = anim.to_y;
        tb.anim = None;
    }
}

fn toolbar_interferes(editor_state: &EditorState) -> bool {
    let monitor_idx = editor_state.pointer.monitor_idx;
    let placement = &editor_state.placements[monitor_idx];
    let mon_w = placement.size.0 as f32;
    let mon_h = placement.size.1 as f32;
    let tb = &editor_state.toolbar;
    let tb_width = tb.size.0;
    let x = (mon_w - tb_width) / 2.0;

    let active_rect = match tb.current_side {
        ToolbarSide::Top => (
            x,
            TOOLBAR_OFFSET,
            x + tb_width,
            TOOLBAR_OFFSET + TOOLBAR_HEIGHT,
        ),
        ToolbarSide::Bottom => (
            x,
            mon_h - TOOLBAR_OFFSET - TOOLBAR_HEIGHT,
            x + tb_width,
            mon_h - TOOLBAR_OFFSET,
        ),
    };

    let Some(sel) = editor_state
        .selection
        .zone
        .as_ref()
        .and_then(|sel| global_selection_to_local(sel, placement))
    else {
        return false;
    };

    active_rect.0 < sel.right()
        && active_rect.2 > sel.left()
        && active_rect.1 < sel.bottom()
        && active_rect.3 > sel.top()
        && editor_state.tool_active // if selection isn't active toolbar won't be transparent
}

fn toolbar_position(side: ToolbarSide, placement: &Placement, tb_width: f32) -> (f32, f32) {
    let mon_w = placement.size.0 as f32;
    let mon_h = placement.size.1 as f32;
    let x = (mon_w - tb_width) / 2.0;
    let y = match side {
        ToolbarSide::Top => TOOLBAR_OFFSET,
        ToolbarSide::Bottom => mon_h - TOOLBAR_OFFSET - TOOLBAR_HEIGHT,
    };
    (x, y)
}

pub fn toolbar_hit_test(toolbar: &Toolbar, local: (f64, f64)) -> Option<usize> {
    let (px, py) = (local.0 as f32, local.1 as f32);
    let (tb_x, tb_y) = toolbar.position;
    let (_, tb_h) = toolbar.size;

    if py < tb_y || py > tb_y + tb_h {
        return None;
    }

    let mut current_x = tb_x + TOOLBAR_PADDING;
    for (idx, item) in toolbar.items.iter().enumerate() {
        let item_w = item.size();
        let item_right = current_x + item_w;
        if px >= current_x && px <= item_right {
            return match item {
                ToolbarItem::Button(_) => Some(idx),
                ToolbarItem::Seperator => None,
            };
        }
        current_x += item_w + item.trailing_padding();
    }
    None
}

// ************************* //
//      RENDER HELPERS       //
// ************************* //

fn selection_render_info(
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
        None => return vec![],
    };

    // based on selection choosing monitors for render
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

    // re-drawing annotations
    // maybe in the future it would be better to make a separate layer
    // instead of redrawing them
    // or for some specific perfomance eating tools as blur
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

// ************************* //
//           UTILS           //
// ************************* //

pub fn mark_dirty(mask: &mut u32, idx: usize) {
    *mask |= 1 << idx;
}

fn is_dirty(mask: u32, idx: usize) -> bool {
    (mask & (1 << idx)) != 0
}

// some parts of the code still identify dirty monitors themselves
// that will be fixed
fn apply_damage_rects(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    for rect in &editor_state.damage_rects {
        let mask = get_overlapping_monitors(rect, &editor_state.placements);
        *dirty_mask |= mask;
    }
}
// text

fn handle_text_input(editor_state: &mut EditorState, ch: char, dirty_mask: &mut u32) {
    dispatch_text(editor_state.selected_tool, editor_state, ch, dirty_mask);
    apply_damage_rects(editor_state, dirty_mask);
}

fn handle_key_press(editor_state: &mut EditorState, key: SpecialKey, dirty_mask: &mut u32) {
    dispatch_key(editor_state.selected_tool, editor_state, key, dirty_mask);
    apply_damage_rects(editor_state, dirty_mask);
}
