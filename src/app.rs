use crate::backend::{initialize_capture, initialize_overlay};
use crate::types::{OverlayEvent, EditMode, EditorState, Placement};
use tiny_skia::Pixmap;
use crate::renderer;

pub async fn make_screenshot(
    wayland_conn: Option<wayland_client::Connection>,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = wayland_conn.unwrap();
    let capture = initialize_capture();
    let mut overlay = initialize_overlay(conn);

    let screenshot = capture.capture_frame().await?;
    let base_pixmap = {
        let mut p = Pixmap::new(screenshot.frame.width, screenshot.frame.height).unwrap();
        p.data_mut().copy_from_slice(&screenshot.frame.pixels);
        p
    };

    let mut editor_state = EditorState {
        base: base_pixmap,
        mode: EditMode::Selection,
        selection: None,
        pointer: (0.0, 0.0),
        mouse_down: false,
    };

    let placements: Vec<Placement> = screenshot.streams.iter()
        .map(|stream| Placement {
            position: stream.position.unwrap_or((0, 0)),
            size: stream.size.unwrap_or((0, 0)),
        })
        .collect();

    let (pixels, w, h) = renderer::render_frame(&editor_state, &[]);
    let outputs = overlay.present(w, h, &placements)?.to_vec();
    for o in &outputs {
        println!("output: x={} y={} w={} h={}", o.x, o.y, o.width, o.height);
    }

    overlay.update_frame(&pixels);

    loop {
        let ev = overlay.next_event()?;
        let mut dirty = false;

        match ev {
            OverlayEvent::EscapePressed => break,
            OverlayEvent::PointerMove { x, y } => {
                editor_state.pointer = (x, y);
                dirty = true;
            }
        }

        if dirty {
            let t0 = std::time::Instant::now();
            let (pixels, w, h) = renderer::render_frame(&editor_state, &outputs);
            println!("render: {}ms", t0.elapsed().as_millis());
            overlay.update_frame(&pixels)?;
        }
    }

    Ok(())
}