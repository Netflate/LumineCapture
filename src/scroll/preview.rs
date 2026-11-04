// Downsampled thumbnails for the scrolling capture preview panel.

use shot_image::ImageView;
use tiny_skia::{Pixmap, PremultipliedColorU8};

/// Scales `image` down to `target_w` wide, preserving aspect ratio.
pub fn thumbnail(image: ImageView, transposed: bool, target_w: u32) -> Option<Pixmap> {
    let (w, h) = if transposed {
        (image.height(), image.width())
    } else {
        (image.width(), image.height())
    };
    if w <= 0 || h <= 0 || target_w == 0 {
        return None;
    }
    let scale = target_w as f32 / w as f32;
    let target_h = ((h as f32 * scale).round() as u32).max(1);
    let mut pixmap = Pixmap::new(target_w, target_h)?;
    let dst = pixmap.pixels_mut();
    let at = |x: i32, y: i32| {
        if transposed {
            image.row(x)[y as usize]
        } else {
            image.row(y)[x as usize]
        }
    };

    for y in 0..target_h {
        let sy = ((y as f32 / scale) as i32).clamp(0, h - 1);
        for x in 0..target_w {
            let sx = ((x as f32 / scale) as i32).clamp(0, w - 1);
            let pixel = at(sx, sy);
            let r = ((pixel >> 16) & 255) as u8;
            let g = ((pixel >> 8) & 255) as u8;
            let b = (pixel & 255) as u8;
            dst[(y * target_w + x) as usize] = PremultipliedColorU8::from_rgba(r, g, b, 255)
                .unwrap_or(PremultipliedColorU8::TRANSPARENT);
        }
    }

    Some(pixmap)
}
