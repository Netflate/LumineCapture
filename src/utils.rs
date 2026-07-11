use crate::types::{HANDLE_PAD, Placement, SelectionHandle, SignedRect};
use std::path::PathBuf;
use tiny_skia::Pixmap;
use tiny_skia::Rect;

pub fn make_rect(a: (f64, f64), b: (f64, f64)) -> Option<Rect> {
    let x = a.0.min(b.0) as f32;
    let y = a.1.min(b.1) as f32;
    let w = (a.0 - b.0).abs() as f32;
    let h = (a.1 - b.1).abs() as f32;
    if w < 1.0 || h < 1.0 {
        return None;
    }
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
            let inside =
                gx >= px as f64 && gx < (px + w) as f64 && gy >= py as f64 && gy < (py + h) as f64;
            inside.then_some((idx, gx - px as f64, gy - py as f64))
        })
        .unwrap_or((fallback_idx, fallback_local.0, fallback_local.1))
}

pub fn encode_png(pixmap: &Pixmap) -> Vec<u8> {
    use image::ImageEncoder;
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};

    let mut png_bytes = Vec::new();
    let mut rgba = pixmap.data().to_vec();
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    let encoder =
        PngEncoder::new_with_quality(&mut png_bytes, CompressionType::Fast, FilterType::Adaptive);
    encoder
        .write_image(
            &rgba,
            pixmap.width(),
            pixmap.height(),
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();

    png_bytes
}

pub fn save_to_file(png_data: &[u8]) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let now = chrono::Local::now();

    let dir = dirs::picture_dir()
        .unwrap_or_else(|| PathBuf::from("~/Pictures")) // hardcoded TOFIX
        .join("screenshots")
        .join(now.format("%Y-%m").to_string());

    std::fs::create_dir_all(&dir)?;

    let filename = now.format("%Y-%m-%d_%H-%M.png").to_string(); // hardcoded TOFIX
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
        let overlaps = selection.left() < mx + p.size.0 as f32
            && selection.right() > mx
            && selection.top() < my + p.size.1 as f32
            && selection.bottom() > my;
        if overlaps {
            mask |= 1 << i;
        }
    }
    mask
}

// used for selecting/resizing annotations or selection
pub fn hit_test_rect_handle(sel: &Rect, pos: (f64, f64)) -> SelectionHandle {
    let (x, y) = pos;
    let (l, r, t, b) = (
        sel.left() as f64,
        sel.right() as f64,
        sel.top() as f64,
        sel.bottom() as f64,
    );
    let w = sel.width() as f64;
    let h = sel.height() as f64;

    let corner_w = (w * 0.30).clamp(8.0, 40.0).min(w * 0.5);
    let corner_h = (h * 0.30).clamp(8.0, 40.0).min(h * 0.5);

    let half_pad = HANDLE_PAD / 2.0;

    // Top-Left
    let in_tl_horizontal = (y - t).abs() <= half_pad && x >= l - half_pad && x <= l + corner_w;
    let in_tl_vertical = (x - l).abs() <= half_pad && y >= t - half_pad && y <= t + corner_h;
    if in_tl_horizontal || in_tl_vertical {
        return SelectionHandle::TopLeft;
    }

    // Top-Right
    let in_tr_horizontal = (y - t).abs() <= half_pad && x >= r - corner_w && x <= r + half_pad;
    let in_tr_vertical = (x - r).abs() <= half_pad && y >= t - half_pad && y <= t + corner_h;
    if in_tr_horizontal || in_tr_vertical {
        return SelectionHandle::TopRight;
    }

    // Bottom-Left
    let in_bl_horizontal = (y - b).abs() <= half_pad && x >= l - half_pad && x <= l + corner_w;
    let in_bl_vertical = (x - l).abs() <= half_pad && y >= b - corner_h && y <= b + half_pad;
    if in_bl_horizontal || in_bl_vertical {
        return SelectionHandle::BottomLeft;
    }

    // Bottom-Right
    let in_br_horizontal = (y - b).abs() <= half_pad && x >= r - corner_w && x <= r + half_pad;
    let in_br_vertical = (x - r).abs() <= half_pad && y >= b - corner_h && y <= b + half_pad;
    if in_br_horizontal || in_br_vertical {
        return SelectionHandle::BottomRight;
    }

    // Top
    if (y - t).abs() <= half_pad && x > l + corner_w && x < r - corner_w {
        return SelectionHandle::Top;
    }
    // Bottom
    if (y - b).abs() <= half_pad && x > l + corner_w && x < r - corner_w {
        return SelectionHandle::Bottom;
    }
    // Left
    if (x - l).abs() <= half_pad && y > t + corner_h && y < b - corner_h {
        return SelectionHandle::Left;
    }
    // Right
    if (x - r).abs() <= half_pad && y > t + corner_h && y < b - corner_h {
        return SelectionHandle::Right;
    }

    if x >= l + half_pad && x <= r - half_pad && y >= t + half_pad && y <= b - half_pad {
        return SelectionHandle::Move;
    }

    SelectionHandle::None
}

pub fn apply_handle_drag(orig: &Rect, handle: SelectionHandle, delta: (f64, f64)) -> SignedRect {
    let (dx, dy) = (delta.0 as f32, delta.1 as f32);
    let (mut l, mut r, mut t, mut b) = (orig.left(), orig.right(), orig.top(), orig.bottom());
    match handle {
        SelectionHandle::TopLeft => {
            l += dx;
            t += dy;
        }
        SelectionHandle::Top => {
            t += dy;
        }
        SelectionHandle::TopRight => {
            r += dx;
            t += dy;
        }
        SelectionHandle::Left => {
            l += dx;
        }
        SelectionHandle::Right => {
            r += dx;
        }
        SelectionHandle::BottomLeft => {
            l += dx;
            b += dy;
        }
        SelectionHandle::Bottom => {
            b += dy;
        }
        SelectionHandle::BottomRight => {
            r += dx;
            b += dy;
        }
        SelectionHandle::Move => {
            l += dx;
            r += dx;
            t += dy;
            b += dy;
        }
        SelectionHandle::None => {}
    }
    SignedRect {
        left: l,
        top: t,
        right: r,
        bottom: b,
    }
}
