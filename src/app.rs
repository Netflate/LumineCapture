use crate::backend::{ScreenOverlay, initialize_capture, initialize_overlay};
use crate::types::{EditMode, EditorState, MagnifierState, MonitorFrame, MouseButton, OverlayEvent, Placement, PointerState};
use tiny_skia::{Pixmap, PixmapPaint, Transform};
use crate::renderer;
use crate::utils::{make_rect, global_selection_to_local, global_point_to_local};

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
        selection: None,
        placements : placements,
        drag_start: None,
        pointer: PointerState::default(),
        magnifier: None,
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
                            editor_state.drag_start = Some(editor_state.pointer.global);
                        } else {
                            editor_state.drag_start = None;
                        }
                    }
                    _ => {}
                }
            }
        }

        if dirty_mask != 0 {
            for i in 0..outputs.len() {
                if is_dirty(dirty_mask, i) {
                    let t0 = std::time::Instant::now();
                    
                    let is_mag_monitor = editor_state.magnifier
                        .as_ref()
                        .map_or(false, |m| m.monitor_idx == i);
                
                    let local_sel = editor_state.selection.as_ref()
                        .and_then(|s| global_selection_to_local(s, &editor_state.placements[i]));

                    renderer::render_frame(
                        &mut editor_state.canvas[i],
                        &editor_state.base[i],
                        &mut editor_state.dimmed[i],
                        &local_sel,
                        selection_dirty,
                        &editor_state.magnifier,
                        is_mag_monitor,
                    );

                    println!("render {}: {}ms", i, t0.elapsed().as_millis());
                    overlay.update_frame(i, editor_state.canvas[i].data())?;
                    println!("render + output {}: {}ms", i, t0.elapsed().as_millis());

                    println!("rendering monitor {}: is_mag={}, mag={:?}, sel_dirty={}",i, is_mag_monitor, editor_state.magnifier, selection_dirty);

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
        renderer::render_frame(
            &mut editor_state.canvas[monitor_idx],
            &editor_state.base[monitor_idx],
            &mut editor_state.dimmed[monitor_idx],
            &editor_state.selection,
            true,
            &editor_state.magnifier,
            false, 
        );
        overlay.update_frame(monitor_idx, editor_state.canvas[monitor_idx].data())?;
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
    mark_dirty(dirty_mask, current_monitor_idx);
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
    if let Some(mag) = editor_state.magnifier.as_ref() {
        if mag.monitor_idx != monitor_idx {
            mark_dirty(dirty_mask, mag.monitor_idx);
        }
    }

    editor_state.magnifier = Some(MagnifierState {
        monitor_idx,
        pos: local,
    });
}

fn update_selection(
    editor_state: &mut EditorState,
    global: (f64, f64),
    selection_dirty: &mut bool,
    dirty_mask: &mut u32,
) {
    if !editor_state.mouse_down_left || editor_state.mode != EditMode::Selection {
        return;
    }

    if let Some(start) = editor_state.drag_start {
        editor_state.selection = make_rect(start, global);
        *selection_dirty = true;
        mark_all_dirty(dirty_mask, editor_state.base.len());
    }
}

fn mark_dirty(mask: &mut u32, idx: usize) {
    *mask |= 1 << idx;
}

fn mark_all_dirty(mask: &mut u32, count: usize) {
    for idx in 0..count {
        mark_dirty(mask, idx);
    }
}

fn is_dirty(mask: u32, idx: usize) -> bool {
    (mask & (1 << idx)) != 0
}