use crate::backend::{initialize_capture, initialize_overlay};

pub async fn make_screenshot(
    wayland_conn: Option<wayland_client::Connection>,
) -> Result<(), Box<dyn std::error::Error>> {
    let started_at = std::time::Instant::now();
    let conn = wayland_conn.unwrap();

    let capture = initialize_capture();
    let overlay = initialize_overlay(conn);

    let screenshot = capture.capture_frame().await?;
    println!("after capturing: {}ms", started_at.elapsed().as_millis());

    overlay.show_screenshot(screenshot)?;

    

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
