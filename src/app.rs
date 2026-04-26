use crate::backend::{initialize_capture, initialize_overlay};
use crate::types::{EditMode, EditorState, OverlayEvent, Placement};
use tiny_skia::Pixmap;
use crate::renderer;

pub async fn make_screenshot (
    wayland_conn: Option<wayland_client::Connection>,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = wayland_conn.unwrap();
    let capture = initialize_capture();
    let mut overlay = initialize_overlay(conn);

    let screenshots = capture.capture_frame().await?;

    let base_pixmaps: Vec<Pixmap> = screenshots.frames.iter().map(|f| {
        let mut p = Pixmap::new(f.info.size.unwrap().0 as u32, f.info.size.unwrap().1 as u32)
            .expect("Failed to create Pixmap for monitor");
        p.data_mut().copy_from_slice(&f.pixels);
        p
    }).collect();


    let mut editor_state = EditorState {
        base: base_pixmaps,
        mode: EditMode::Selection,
        selection: None,
        pointer: (0, 0.0, 0.0),
        mouse_down: false,
    };

    let placements: Vec<Placement> = screenshots.frames.iter()
        .map(|stream| Placement {
            position: stream.info.position.unwrap_or((0, 0)),
            size: stream.info.size.unwrap_or((0, 0)),
        })
        .collect();

    let outputs = overlay.present(&placements)?.to_vec();

    // Initial paint: draw and upload a frame for each monitor once.
    for monitor_idx in 0..editor_state.base.len() {
        editor_state.pointer.0 = monitor_idx;
        let (pixels, _, _) = renderer::render_frame(&editor_state, &outputs);
        overlay.update_frame(monitor_idx, &pixels)?;
    }

    loop {
        let ev = overlay.next_event()?;
        match ev {
            OverlayEvent::EscapePressed => break,
            OverlayEvent::PointerMove { monitor_idx, x, y } => {
                if monitor_idx >= editor_state.base.len() {
                    continue;
                }

                editor_state.pointer = (monitor_idx, x, y);

                let t0 = std::time::Instant::now();
                let (pixels, _, _) = renderer::render_frame(&editor_state, &outputs);
                println!("render monitor {}: {}ms", monitor_idx, t0.elapsed().as_millis());
                overlay.update_frame(monitor_idx, &pixels)?;
            }
        }
    }

    Ok(())
}