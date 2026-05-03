use crate::backend::{ScreenOverlay, initialize_capture, initialize_overlay};
use crate::types::{DamageRect, EditMode, EditorState, MagnifierState, MonitorFrame, MouseButton, OverlayEvent, Placement, PointerState, SelectionEdges, SelectionHandle, SelectionState, HANDLE_RADIUS, MAG_FRAME_INTERVAL};
use tiny_skia::{Pixmap, PixmapPaint, Transform, Rect};
use crate::renderer::{self, apply_handle_drag};
use crate::utils::{make_rect, global_selection_to_local, global_point_to_local, hit_test_selection, point_in_monitor, selection_edges_for_monitor, selection_handle_points};
use std::time::{Instant};


pub async fn make_screenshot (
    wayland_conn: Option<wayland_client::Connection>,
) -> Result<(), Box<dyn std::error::Error>> {
    let t0 = std::time::Instant::now();                               
    
    let conn = wayland_conn.unwrap();
    let capture = initialize_capture();
    let mut overlay = initialize_overlay(conn);
    let screenshots = capture.capture_frame().await?;
    println!("after capturing {}ms", t0.elapsed().as_millis());               
    

    let base_pixmaps: Vec<Pixmap> = build_base_pixmap(&screenshots.frames);
    let (canvas, dimmed) = build_layers(&base_pixmaps);
    let placements = build_placements(&screenshots.frames); 

    let mut editor_state = EditorState {
        base: base_pixmaps,
        canvas: canvas,
        dimmed : dimmed,
        mode: EditMode::Selection,
        selection: SelectionState::default(),
        placements : placements,
        drag_start: None,
        pointer: PointerState::default(),
        magnifier: None,
        prev_magnifier: None,
        last_mag_update: None,
        mouse_down_left: false,
    };

    println!("after saving base screenshot {}ms", t0.elapsed().as_millis());                
    let outputs = overlay.present(&editor_state.placements)?.to_vec();
    
    initial_paint(&mut editor_state, &mut overlay)?;
    println!("after initialising overlay and showing it {}ms", t0.elapsed().as_millis());                


    let mut dirty_mask : u32 = 0 ;
    let mut selection_dirty = false;

    loop {
        let ev = overlay.next_event()?;
        match ev {
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
                match button {
                    MouseButton::Left => {
                        editor_state.mouse_down_left = pressed;
                        if pressed {
                            let handle = editor_state.selection.zone.as_ref()
                                .map(|sel| hit_test_selection(sel, editor_state.pointer.global))
                                .unwrap_or(SelectionHandle::None);

                            if handle != SelectionHandle::None {
                                if let Some(sel) = editor_state.selection.zone.as_ref() {
                                    editor_state.selection.set_drag(
                                        handle,
                                        Some(editor_state.pointer.global),
                                        Some(*sel),
                                    );
                                    editor_state.drag_start = None;
                                }
                            } else {
                                editor_state.selection.set_drag(SelectionHandle::None, None, None);
                                editor_state.drag_start = Some(editor_state.pointer.global);
                            }
                        } else {
                            editor_state.drag_start = None;
                            editor_state.selection.set_drag(SelectionHandle::None, None, None);
                        }
                    }
                    _ => {}
                }
            }
        }

        if dirty_mask != 0 {
            for i in 0..outputs.len() {
                if is_dirty(dirty_mask, i) {                    
                    let is_mag_monitor = editor_state.magnifier
                        .as_ref()
                        .map_or(false, |m| m.monitor_idx == i);

                    let (local_sel, prev_local, edges) = selection_render_info(
                        &editor_state.selection.zone,
                        &editor_state.selection.prev_zone,
                        &editor_state.placements[i],
                    );
                    let dirty_rect = monitor_dirty_rect(
                        selection_dirty,
                        &local_sel,
                        &prev_local,
                        &editor_state.placements[i],
                        &editor_state.magnifier,
                        &editor_state.prev_magnifier,
                        i,
                    );
                    let damage: Option<DamageRect> = dirty_rect
                        .as_ref()
                        .and_then(|r| renderer::rect_bounds(r, editor_state.base[i].width(), editor_state.base[i].height()));

                    renderer::render_frame(
                        &mut editor_state.canvas[i],
                        &editor_state.base[i],
                        &mut editor_state.dimmed[i],
                        &local_sel,
                        &prev_local,
                        dirty_rect.as_ref(),
                        edges.as_ref(),
                        selection_dirty,
                        &editor_state.magnifier,
                        is_mag_monitor,
                    );

                    overlay.update_frame(i, editor_state.canvas[i].data(), damage)?;

                }
            }
            selection_dirty = false;
            dirty_mask = 0;
        }
    }
    Ok(())
}





fn build_base_pixmap(frames: &Vec<MonitorFrame>) -> Vec<Pixmap> {
    frames
        .iter()
        .enumerate()
        .map(|(monitor_idx, f)| {
            let (src_w, src_h) = (f.pw_width, f.pw_height);
            let mut src_pixmap = Pixmap::new(src_w, src_h)
                .expect("Failed to create source Pixmap for monitor");

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
            let src = f
                .pixels
                .get(..needed)
                .unwrap_or_else(|| panic!(
                    "Not enough pixel data for monitor {}: have={} need={}",
                    monitor_idx,
                    f.pixels.len(),
                    needed
                ));

            for row in 0..(src_h as usize) {
                let src_off = row * src_stride;
                let dst_off = row * row_bytes;
                dst[dst_off..dst_off + row_bytes]
                    .copy_from_slice(&src[src_off..src_off + row_bytes]);
            }

            let (logical_w_i32, logical_h_i32) = f
                .info
                .size
                .unwrap_or((src_w as i32, src_h as i32));
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

fn build_layers(base_pixmaps: &[Pixmap]) -> (Vec<Pixmap>, Vec<Pixmap>) {
    base_pixmaps
        .iter()
        .map(|p| {
            let w = p.width();
            let h = p.height();

            let canvas = Pixmap::new(w, h).expect("Failed to create canvas Pixmap");
            let dimmed = Pixmap::new(w, h).expect("Failed to create dimmed Pixmap");

            (canvas, dimmed)
        })
        .unzip() 
}



fn build_placements(frames: &Vec<MonitorFrame>) -> Vec<Placement> {
    frames.iter()
    .map(|stream| Placement {
        position: stream.info.position.unwrap_or((0, 0)),
        size: stream.info.size.unwrap_or((0, 0)),
    })
    .collect()
}


fn initial_paint(
    editor_state: &mut EditorState,
    overlay: &mut Box<dyn ScreenOverlay>,
) -> Result<(), Box<dyn std::error::Error>>
{
    for monitor_idx in 0..editor_state.base.len() {
        let (local_sel, prev_local, edges) = selection_render_info(
            &editor_state.selection.zone,
            &editor_state.selection.prev_zone,
            &editor_state.placements[monitor_idx],
        );
        renderer::init_dimming(
            &mut editor_state.dimmed[monitor_idx],
            &editor_state.base[monitor_idx],
            &local_sel,
        );
        renderer::render_frame(
            &mut editor_state.canvas[monitor_idx],
            &editor_state.base[monitor_idx],
            &mut editor_state.dimmed[monitor_idx],
            &local_sel,
            &prev_local,
            None,
            edges.as_ref(),
            false,
            &editor_state.magnifier,
            false, 
        );
        overlay.update_frame(monitor_idx, editor_state.canvas[monitor_idx].data(), None)?;
    }
    Ok(())
}



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

    let (current_monitor_idx, local_x, local_y) = global_point_to_local(
        &editor_state.placements,
        global,
        monitor_idx,
        (x, y),
    );

    update_pointer(editor_state, current_monitor_idx, (local_x, local_y), global);
    update_magnifier(editor_state, current_monitor_idx, (local_x, local_y), dirty_mask);
    update_selection(editor_state, global, selection_dirty, dirty_mask);
}

fn update_pointer(
    editor_state: &mut EditorState,
    monitor_idx: usize,
    local: (f64, f64),
    global: (f64, f64),
) {
    editor_state.pointer = PointerState::new(monitor_idx, local, global);
}

fn update_magnifier(
    editor_state: &mut EditorState,
    monitor_idx: usize,
    local: (f64, f64),
    dirty_mask: &mut u32,
) {
    let now = Instant::now();
    if let Some(last) = editor_state.last_mag_update {
        if now.duration_since(last) < MAG_FRAME_INTERVAL {
            return;
        }
    }
    editor_state.last_mag_update = Some(now);

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


fn mark_dirty(mask: &mut u32, idx: usize) {
    *mask |= 1 << idx;
}


fn is_dirty(mask: u32, idx: usize) -> bool {
    (mask & (1 << idx)) != 0
}

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
    let mut handles = Vec::new();

    if let (Some(sel), Some(_)) = (selection.as_ref(), local_sel.as_ref()) {
        edges = Some(selection_edges_for_monitor(sel, placement));
        let (mx, my) = (placement.position.0 as f32, placement.position.1 as f32);
        for (hx, hy) in selection_handle_points(sel) {
            if point_in_monitor((hx, hy), placement) {
                handles.push((hx - mx, hy - my));
            }
        }
    }

    (local_sel, prev_local, edges)
}

fn monitor_dirty_rect(
    selection_dirty: bool,
    local_sel: &Option<Rect>,
    prev_local: &Option<Rect>,
    placement: &Placement,
    magnifier: &Option<MagnifierState>,
    prev_magnifier: &Option<MagnifierState>,
    monitor_idx: usize,
) -> Option<Rect> {
    let mut dirty: Option<Rect> = None;
    let selection_pad = (HANDLE_RADIUS as f32).max(4.0);

    if selection_dirty {
        if let Some(r) = local_sel.as_ref().and_then(|sel| expand_rect(sel, selection_pad)) {
            dirty = union_rect(dirty, Some(r));
        }
        if let Some(r) = prev_local.as_ref().and_then(|sel| expand_rect(sel, selection_pad)) {
            dirty = union_rect(dirty, Some(r));
        }
    }

    let (mw, mh) = (placement.size.0 as f32, placement.size.1 as f32);
    if mw > 0.0 && mh > 0.0 {
        let mag_pad = 2.0;
        if let Some(mag) = magnifier.as_ref().filter(|m| m.monitor_idx == monitor_idx) {
            let rect = renderer::magnifier_rect((mag.pos.0 as f32, mag.pos.1 as f32), mw, mh);
            if let Some(r) = expand_rect(&rect, mag_pad) {
                dirty = union_rect(dirty, Some(r));
            }
        }
        if let Some(mag) = prev_magnifier.as_ref().filter(|m| m.monitor_idx == monitor_idx) {
            let rect = renderer::magnifier_rect((mag.pos.0 as f32, mag.pos.1 as f32), mw, mh);
            if let Some(r) = expand_rect(&rect, mag_pad) {
                dirty = union_rect(dirty, Some(r));
            }
        }
    }

    dirty
}

fn expand_rect(rect: &Rect, pad: f32) -> Option<Rect> {
    Rect::from_ltrb(
        rect.left() - pad,
        rect.top() - pad,
        rect.right() + pad,
        rect.bottom() + pad,
    )
}

fn union_rect(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (None, None) => None,
        (Some(r), None) | (None, Some(r)) => Some(r),
        (Some(r1), Some(r2)) => Rect::from_ltrb(
            r1.left().min(r2.left()),
            r1.top().min(r2.top()),
            r1.right().max(r2.right()),
            r1.bottom().max(r2.bottom()),
        ),
    }
}


impl SelectionState {
    fn set_drag(&mut self, handle: SelectionHandle, origin: Option<(f64, f64)>, zone: Option<Rect>) {
        self.active_handle = handle;
        self.drag_origin = origin;
        self.selection_at_drag_start = zone;
    }
}


fn update_selection(
    editor_state: &mut EditorState,
    global: (f64, f64),
    selection_dirty: &mut bool,
    dirty_mask: &mut u32,
) {
    let old_sel = editor_state.selection.zone;

    if editor_state.mouse_down_left && editor_state.selection.active_handle != SelectionHandle::None {
        if let (Some(drag_origin), Some(sel_start)) = (
            editor_state.selection.drag_origin,
            editor_state.selection.selection_at_drag_start.as_ref(),
        ) {
            let delta = (global.0 - drag_origin.0, global.1 - drag_origin.1);
            editor_state.selection.zone = apply_handle_drag(
                sel_start,
                editor_state.selection.active_handle,
                delta,
            );
            editor_state.selection.prev_zone = old_sel;
            apply_selection_dirty(old_sel, editor_state.selection.zone, &editor_state.placements, dirty_mask, selection_dirty);
        }
        return;
    }

    if !editor_state.mouse_down_left || editor_state.mode != EditMode::Selection {
        return;
    }

    if let Some(start) = editor_state.drag_start {
        editor_state.selection.zone = make_rect(start, global);
        editor_state.selection.prev_zone = old_sel;
        apply_selection_dirty(old_sel, editor_state.selection.zone, &editor_state.placements, dirty_mask, selection_dirty);
    }
}

fn apply_selection_dirty(
    old_sel: Option<Rect>,
    new_sel: Option<Rect>,
    placements: &[Placement],
    dirty_mask: &mut u32,
    selection_dirty: &mut bool,
) {
    *selection_dirty = true;
    if let Some(sel) = old_sel {
        *dirty_mask |= monitors_for_selection(&sel, placements);
    }
    if let Some(sel) = new_sel {
        *dirty_mask |= monitors_for_selection(&sel, placements);
    }
}

fn monitors_for_selection(selection: &Rect, placements: &[Placement]) -> u32 {
    let mut mask = 0u32;
    for (i, p) in placements.iter().enumerate() {
        let mx = p.position.0 as f32;
        let my = p.position.1 as f32;
        let mw = p.size.0 as f32;
        let mh = p.size.1 as f32;

        let overlaps = selection.left()   < mx + mw
                    && selection.right()  > mx
                    && selection.top()    < my + mh
                    && selection.bottom() > my;

        if overlaps {
            mask |= 1 << i;
        }
    }
    mask
}