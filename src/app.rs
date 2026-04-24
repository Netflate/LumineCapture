use crate::backend::{initialize_capture, initialize_overlay};
use crate::types::{OverlayEvent, EditMode, EditorState, Placement};
use tiny_skia::{Pixmap};
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

    let (pixels, w, h) = renderer::render_frame(&editor_state);
    overlay.present(w, h, &placements)?;
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
            let (pixels, w, h) = renderer::render_frame(&editor_state);
            println!("render: {}ms", t0.elapsed().as_millis());
            let t1 = std::time::Instant::now();
            overlay.update_frame(&pixels)?;
            println!("update: {}ms", t1.elapsed().as_millis());
        }
    }

    Ok(())
}


// old implementation of copying screneshot to wayland clipboard, as an early reference will be here

    // let rgba_pixels: Vec<u8> = frame.pixels
    //     .chunks_exact(4)
    //     .flat_map(|p| [p[2], p[1], p[0], p[3]])
    //     .collect();

    // let mut png_bytes: Vec<u8> = Vec::new();
    // let encoder = PngEncoder::new_with_quality(
    //     &mut png_bytes,
    //     CompressionType::Fast, // Fast / Default / Best
    //     FilterType::Adaptive,
    // );
    // encoder.write_image(
    //     &rgba_pixels,
    //     frame.width,
    //     frame.height,
    //     image::ExtendedColorType::Rgba8,
    // )?;

    // println!("after png encode: {}ms", t0.elapsed().as_millis());
    // if let Some(conn) = wayland_conn {
    //     clipboard::copy_image_to_clipboard(png_bytes, conn)?;
    // }
    // println!("after copying: {}ms", t0.elapsed().as_millis());
    // println!("Done");
