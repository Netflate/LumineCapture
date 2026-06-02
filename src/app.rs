use crate::backend::{ScreenOverlay, initialize_capture, initialize_clipboard, initialize_overlay};
use crate::types::{DamageRect, EditMode, EditorState, MagnifierState, MonitorFrame, MouseButton, OverlayEvent, Placement, PointerState, SelectionEdges, SelectionHandle, SelectionState, ToolbarState, ToolbarSide, HANDLE_RADIUS, MAG_FRAME_INTERVAL, TOOLBAR_HEIGHT, TOOLBAR_OFFSET, TOOLBAR_WIDTH};
use tiny_skia::{Pixmap, PixmapPaint, Transform, Rect};
use crate::renderer::{self, apply_handle_drag};
use crate::utils::{make_rect, global_selection_to_local, global_point_to_local, hit_test_selection, point_in_monitor, selection_edges_for_monitor, selection_handle_points, encode_png, save_to_file};
use std::time::{Instant};


pub async fn make_screenshot (
    wayland_conn: Option<wayland_client::Connection>,
) -> Result<(), Box<dyn std::error::Error>> {
    let t0 = std::time::Instant::now();                               
    
    let conn = wayland_conn.unwrap();
    let capture = initialize_capture();
    let mut overlay = initialize_overlay(conn.clone());
    let screenshots = capture.capture_frame().await?;
    let clipboard = initialize_clipboard(conn);
    
    println!("after capturing {}ms", t0.elapsed().as_millis());               
    
    
    let base_pixmaps: Vec<Pixmap> = build_base_pixmap(&screenshots.frames);
    println!("after initialising base_pixmaps, which are original frames in Vec<Pixmap> {}ms", t0.elapsed().as_millis());                
    let (canvas, dimmed) = build_layers(&base_pixmaps);
    println!("after initialising dimmed canvas, same size dimmed frames {}ms", t0.elapsed().as_millis());                
    let placements = build_placements(&screenshots.frames); 


    drop(screenshots); 
    // 4 may 2026 : ~75mb memory usage while screenshoting on kde linux with 2 hd monitors
    // not ideal, could be resolved with rendering in shm itself 


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
        toolbar: None, 
    };

    println!("after saving base screenshot {}ms", t0.elapsed().as_millis());                
    let outputs = overlay.present(&editor_state.placements)?.to_vec();
    
    initial_paint(&mut editor_state, &mut overlay)?;
    println!("after initialising overlay and showing it {}ms", t0.elapsed().as_millis());                


    let mut dirty_mask : u32 = 0 ;
    let mut selection_dirty = false;

    let mut save_to_clipboard = false;
    let _save_as_file = true;                    // hardcoded
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
            OverlayEvent::SaveToClipboard => {
                drop(overlay);
                save_to_clipboard = true;
                break
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
                    let dirty_rect = editor_state.monitor_dirty_rect(i, selection_dirty);
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
                        &editor_state.toolbar,
                    );

                    overlay.update_frame(i, editor_state.canvas[i].data(), damage)?;

                }
            }
            selection_dirty = false;
            dirty_mask = 0;
        }
    }
    if save_to_clipboard {
        let final_result = render_final(&editor_state);
        // it doesn't make sense, but while this program in wip 
        // it will have one option - save to clipboard AND file

        let _path = save_to_file(&final_result);
        clipboard.copy_image_to_clipboard(final_result)?;   
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
        size: stream
            .info
            .size
            .unwrap_or((stream.pw_width as i32, stream.pw_height as i32)),
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
            &editor_state.toolbar,
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
        &editor_state.placements, global, monitor_idx,(x, y)
    );
    update_pointer(editor_state, current_monitor_idx, (local_x, local_y), global);


    update_magnifier(editor_state, dirty_mask);
    update_selection(editor_state, global, selection_dirty, dirty_mask);
    update_toolbar(editor_state, dirty_mask);
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
    dirty_mask: &mut u32,
) {
    let now = Instant::now();
    if let Some(last) = editor_state.last_mag_update {
        if now.duration_since(last) < MAG_FRAME_INTERVAL {
            return;
        }
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


fn render_final(editor_state: &EditorState) -> Vec<u8> {
    let sel = match editor_state.selection.zone {
        Some(s) => s,
        None => return vec![],
    };

    // choosing monitors within the selection zone 
    let mask = monitors_for_selection(&sel, &editor_state.placements);

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
        if (mask & (1 << i)) == 0 { continue; }

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

    encode_png(&out)
}




impl EditorState {
    pub fn monitor_dirty_rect(
        &self,
        monitor_idx: usize,
        selection_dirty: bool,
    ) -> Option<Rect> {
        let mut dirty: Option<Rect> = None;
        let placement = &self.placements[monitor_idx];
        
        if selection_dirty {
            let selection_pad = (HANDLE_RADIUS as f32).max(4.0);
            
            let local_sel = self.selection.zone
                .as_ref()
                .and_then(|sel| global_selection_to_local(sel, placement));
                
            let prev_local = self.selection.prev_zone
                .as_ref()
                .and_then(|sel| global_selection_to_local(sel, placement));

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
            
            let mut add_mag_dirty = |mag_state: &Option<MagnifierState>| {
                if let Some(mag) = mag_state.as_ref().filter(|m| m.monitor_idx == monitor_idx) {
                    let rect = renderer::magnifier_rect((mag.pos.0 as f32, mag.pos.1 as f32), mw, mh);
                    if let Some(r) = expand_rect(&rect, mag_pad) {
                        dirty = union_rect(dirty, Some(r));
                    }
                }
            };

            add_mag_dirty(&self.magnifier);
            add_mag_dirty(&self.prev_magnifier);
        }
        if let Some(tb) = &self.toolbar {
            if tb.monitor_idx == monitor_idx {
                if let Some(r) = Rect::from_xywh(tb.position.0, tb.position.1, TOOLBAR_WIDTH, TOOLBAR_HEIGHT) {
                    dirty = union_rect(dirty, Some(r));
                }
            }
        }
        
        dirty
    }
}




// Toolbar stuff 
fn update_toolbar(
    editor_state: &mut EditorState, 
    dirty_mask: &mut u32) 
{
    let (side, monitor_idx, pos_x, pos_y) = toolbar_placement(editor_state);

    if let Some(ref old_tb) = editor_state.toolbar {
        if old_tb.monitor_idx != monitor_idx || old_tb.position != (pos_x, pos_y) {
            mark_dirty(dirty_mask, old_tb.monitor_idx);
        }
    }

    mark_dirty(dirty_mask, monitor_idx);

    editor_state.toolbar = Some(ToolbarState {
        position: (pos_x, pos_y),
        size: (TOOLBAR_WIDTH,TOOLBAR_WIDTH),
        monitor_idx,
        current_side: side,
        transparent: false,
    });
}



fn toolbar_placement(editor_state: &EditorState) -> (ToolbarSide, usize, f32, f32) {
    let monitor_idx = editor_state.pointer.monitor_idx;
    let placement = &editor_state.placements[monitor_idx];
    let mon_w = placement.size.0 as f32;
    let mon_h = placement.size.1 as f32;


    let x = (mon_w - TOOLBAR_WIDTH) / 2.0;

    let local_sel = editor_state.selection.zone
        .as_ref()
        .and_then(|sel| global_selection_to_local(sel, placement));

    let top_rect    = (x, TOOLBAR_OFFSET,              x + TOOLBAR_WIDTH, TOOLBAR_OFFSET + TOOLBAR_HEIGHT);
    let bottom_rect = (x, mon_h - TOOLBAR_OFFSET - TOOLBAR_HEIGHT, x + TOOLBAR_WIDTH, mon_h - TOOLBAR_OFFSET);

    let overlaps = |tb: (f32, f32, f32, f32), sel: &Rect| -> bool {
        tb.0 < sel.right() && tb.2 > sel.left() &&
        tb.1 < sel.bottom() && tb.3 > sel.top()
    };

    match local_sel {
        None => (ToolbarSide::Top, monitor_idx, x, TOOLBAR_OFFSET),
        Some(sel) => {
            if !overlaps(top_rect, &sel) {
                (ToolbarSide::Top, monitor_idx, x, TOOLBAR_OFFSET)
            } else if !overlaps(bottom_rect, &sel) {
                (ToolbarSide::Bottom, monitor_idx, x, mon_h - TOOLBAR_OFFSET - TOOLBAR_HEIGHT)
            } else {
                // оба перекрыты — сверху всё равно
                (ToolbarSide::Top, monitor_idx, x, TOOLBAR_OFFSET)
            }
        }
    }
}