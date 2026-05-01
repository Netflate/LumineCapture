use crate::backend::{initialize_capture, initialize_overlay};
use crate::types::{EditMode, EditorState, OverlayEvent, Placement, MagnifierState};
use tiny_skia::{Pixmap, PixmapPaint, Transform};
use crate::renderer;

pub async fn make_screenshot (
    wayland_conn: Option<wayland_client::Connection>,
) -> Result<(), Box<dyn std::error::Error>> {
    let t0 = std::time::Instant::now();                               
    
    let conn = wayland_conn.unwrap();
    let capture = initialize_capture();
    let mut overlay = initialize_overlay(conn);
    let screenshots = capture.capture_frame().await?;
    println!("after capturing {}ms", t0.elapsed().as_millis());               
    

    let base_pixmaps: Vec<Pixmap> = screenshots
        .frames
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
        .collect();
    
    let canvas = base_pixmaps.iter()
        .map(|p| Pixmap::new(p.width(), p.height()).unwrap())
        .collect();
    
    let dimmed = base_pixmaps.iter()
        .map(|p| Pixmap::new(p.width(), p.height()).unwrap())
        .collect();

    let mut editor_state = EditorState {
        base: base_pixmaps,
        canvas: canvas,
        dimmed : dimmed,
        mode: EditMode::Selection,
        selection: None,
        pointer: (0, 0.0, 0.0),
        mouse_down: false,
        magnifier: None,
    };
    
    let placements: Vec<Placement> = screenshots.frames.iter()
    .map(|stream| Placement {
        position: stream.info.position.unwrap_or((0, 0)),
        size: stream.info.size.unwrap_or((0, 0)),
    })
    .collect();


    println!("after saving base screenshot {}ms", t0.elapsed().as_millis());                
    let outputs = overlay.present(&placements)?.to_vec();
    // Initial paint: draw and upload a frame for each monitor once
    for monitor_idx in 0..editor_state.base.len() {
        editor_state.pointer.0 = monitor_idx;
        renderer::render_frame(
            &mut editor_state.canvas[monitor_idx],
            &editor_state.base[monitor_idx],
            &mut editor_state.dimmed[monitor_idx],
            &editor_state.selection,
            true,
            &editor_state.magnifier,
            &outputs,
            false, 
        );
        overlay.update_frame(monitor_idx, editor_state.canvas[monitor_idx].data())?;
    }
    println!("after initialising overlay and showing it {}ms", t0.elapsed().as_millis());                

    let mut dirty_mask : u32 = 0 ;
        
    loop {
        let ev = overlay.next_event()?;
        match ev {
            OverlayEvent::EscapePressed => break,
            OverlayEvent::PointerMove { monitor_idx, x, y } => {
                editor_state.pointer = (monitor_idx, x, y);

                if let Some(ref mag) = editor_state.magnifier {
                    if mag.monitor_idx != monitor_idx {
                        dirty_mask |= 1 << mag.monitor_idx;
                    }
                }
                match editor_state.magnifier {
                    None => {
                        editor_state.magnifier = Some(MagnifierState {
                            monitor_idx,
                            pos: (x, y),
                        });
                    }
                    Some(ref mut mag) => {
                        mag.monitor_idx = monitor_idx;
                        mag.pos = (x, y);
                    }
                }

                dirty_mask |= 1 << monitor_idx;
            }
        }

        if dirty_mask != 0 {
            for i in 0..outputs.len() {
                if (dirty_mask & (1 << i)) != 0 {
                    let t0 = std::time::Instant::now();
                    
                    let is_mag_monitor = editor_state.magnifier
                    .as_ref()
                    .map_or(false, |m| m.monitor_idx == i);
                
                renderer::render_frame(
                    &mut editor_state.canvas[i],
                    &editor_state.base[i],
                    &mut editor_state.dimmed[i],
                    &editor_state.selection,
                    false,
                    &editor_state.magnifier,
                    &outputs,
                    is_mag_monitor,
                );
                println!("render {}: {}ms", i, t0.elapsed().as_millis());
                overlay.update_frame(i, editor_state.canvas[i].data())?;
                println!("render + output {}: {}ms", i, t0.elapsed().as_millis());
            }
        }
            dirty_mask = 0;
        }
    }
    Ok(())
}