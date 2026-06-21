use tiny_skia::Rect;
use tiny_skia::Pixmap;
use crate::types::Placement;
use std::path::PathBuf;

pub fn make_rect(a: (f64, f64), b: (f64, f64)) -> Option<Rect> {
    let x = a.0.min(b.0) as f32;
    let y = a.1.min(b.1) as f32;
    let w = (a.0 - b.0).abs() as f32;
    let h = (a.1 - b.1).abs() as f32;
    if w < 1.0 || h < 1.0 { return None; }
    Rect::from_xywh(x, y, w, h)
}

pub fn global_point_to_local(
    placements: &[Placement],
    global: (f64, f64),
    fallback_idx: usize,
    fallback_local: (f64, f64),
) -> (usize, f64, f64) {
    let (gx, gy) = global;
    placements
        .iter()
        .enumerate()
        .find_map(|(idx, p)| {
            let (px, py) = p.position;
            let (w, h) = p.size;
            let inside = gx >= px as f64 && gx < (px + w) as f64
                && gy >= py as f64 && gy < (py + h) as f64;
            inside.then(|| (idx, gx - px as f64, gy - py as f64))
        })
        .unwrap_or((fallback_idx, fallback_local.0, fallback_local.1))
}



pub fn encode_png(pixmap: &Pixmap) -> Vec<u8> {
    use image::codecs::png::{PngEncoder, CompressionType, FilterType};
    use image::ImageEncoder;

    let mut png_bytes = Vec::new();
    let mut rgba = pixmap.data().to_vec();
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let encoder = PngEncoder::new_with_quality(
        &mut png_bytes,
        CompressionType::Fast,
        FilterType::Adaptive,
    );
    encoder.write_image(
        &rgba,
        pixmap.width(),
        pixmap.height(),
        image::ExtendedColorType::Rgba8,
    ).unwrap();

    png_bytes
}

pub fn save_to_file(png_data: &[u8]) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let now = chrono::Local::now();

    let dir = dirs::picture_dir()
        .unwrap_or_else(|| PathBuf::from("~/Pictures"))             // hardcoded TOFIX
        .join("screenshots")
        .join(now.format("%Y-%m").to_string());
    
    std::fs::create_dir_all(&dir)?;

    let filename = now.format("%Y-%m-%d_%H-%M.png").to_string();    // hardcoded TOFIX
    let path = dir.join(filename);

    std::fs::write(&path, png_data)?;
    Ok(path)

}

// to render necessary monitors 
pub fn get_overlapping_monitors(selection: &Rect, placements: &[crate::types::Placement]) -> u32 {
    let mut mask = 0u32;
    for (i, p) in placements.iter().enumerate() {
        let mx = p.position.0 as f32;
        let my = p.position.1 as f32;
        let overlaps = selection.left()  < mx + p.size.0 as f32
                    && selection.right() > mx
                    && selection.top()   < my + p.size.1 as f32
                    && selection.bottom()> my;
        if overlaps { mask |= 1 << i; }
    }
    mask
}