// Window decoration drawing for the editor window.
// Usually we don't handle this, but His Majesty GNOME finds drawing decorations
// a little too exhausting, so unfortunately we have to do it ourselves
//
// The compositor tells us whether decorations are server-side or client-side,
// and from what I tested, on Hyprland, KDE and niri it works fine: their decorations don't get mixed up with ours
pub const MARGIN: u32 = 24;
pub const RADIUS: f32 = 15.0;
pub const GRAB: u32 = 12;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Decor {
    pub margin_px: u32,
    pub radius_px: f32,
    pub scale: f32,
    pub active: bool,
    pub dark: bool,
}

impl Decor {
    pub fn new(scale: f32, active: bool, dark: bool) -> Self {
        Self {
            margin_px: (MARGIN as f32 * scale).round() as u32,
            radius_px: RADIUS * scale,
            scale,
            active,
            dark,
        }
    }
}

const FOCUSED: &[f32] = &[
    0.22, 0.153, 0.133, 0.11, 0.094, 0.078, 0.067, 0.055, 0.047, 0.039, 0.031, 0.027, 0.02, 0.016,
    0.012, 0.012, 0.008, 0.004, 0.004, 0.004, 0.004,
];
const INACTIVE: &[f32] = &[
    0.114, 0.059, 0.055, 0.051, 0.043, 0.035, 0.031, 0.024, 0.02, 0.016, 0.012, 0.008, 0.004,
    0.004, 0.004,
];

pub fn shadow_alpha(distance: f32, active: bool) -> f32 {
    let table = if active { FOCUSED } else { INACTIVE };
    if !distance.is_finite() || distance <= 0.0 || distance >= table.len() as f32 + 1.0 {
        return 0.0;
    }
    if distance < 1.0 {
        return table[0];
    }
    let at = distance - 1.0;
    let index = at.floor() as usize;
    let next = table.get(index + 1).copied().unwrap_or(0.0);
    table[index] + (next - table[index]) * at.fract()
}

fn sdf(px: f32, py: f32, content: (u32, u32), decor: &Decor) -> f32 {
    let (w, h) = (content.0 as f32, content.1 as f32);
    let x = px - decor.margin_px as f32;
    let y = py - decor.margin_px as f32;
    let qx = (x - w * 0.5).abs() - (w * 0.5 - decor.radius_px);
    let qy = (y - h * 0.5).abs() - (h * 0.5 - decor.radius_px);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - decor.radius_px
}

pub fn buffer_size(content_px: (u32, u32), decor: &Decor) -> (u32, u32) {
    (
        content_px.0 + decor.margin_px * 2,
        content_px.1 + decor.margin_px * 2,
    )
}

pub fn paint_shadow(dst: &mut [u8], size: (u32, u32), content: (u32, u32), decor: &Decor) {
    let (bw, bh) = size;
    let m = decor.margin_px;
    for y in 0..bh {
        let ranges = if y >= m && y < m + content.1 {
            [(0, m), (m + content.0, bw)]
        } else {
            [(0, bw), (bw, bw)]
        };
        for (start, end) in ranges {
            for x in start..end {
                let distance = sdf(x as f32 + 0.5, y as f32 + 0.5, content, decor);
                let alpha = if distance > 0.0 {
                    shadow_alpha(distance / decor.scale, decor.active)
                } else {
                    0.0
                };
                let i = ((y * bw + x) * 4) as usize;
                dst[i..i + 4].copy_from_slice(&[0, 0, 0, (alpha * 255.0).round() as u8]);
            }
        }
    }
}

fn paint_pixel(dst: &mut [u8], i: usize, distance: f32, decor: &Decor) {
    let bw = 1.0_f32.max(decor.scale.round());
    let band = if decor.dark {
        (distance + bw + 0.5).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let cov = (0.5 - distance).clamp(0.0, 1.0);
    let shadow = shadow_alpha(distance.max(0.0) / decor.scale, decor.active) * 255.0;
    let over = 0.07 * band;
    for c in 0..3 {
        let content = 255.0 * over + dst[i + c] as f32 * (1.0 - over);
        dst[i + c] = (content * cov).round() as u8;
    }
    let alpha = 255.0 * over + dst[i + 3] as f32 * (1.0 - over);
    dst[i + 3] = (alpha * cov + shadow * (1.0 - cov)).round() as u8;
}

pub fn compose_edges(
    dst: &mut [u8],
    size: (u32, u32),
    content: (u32, u32),
    decor: &Decor,
    rect: (u32, u32, u32, u32),
) {
    let (bw, _) = size;
    let (w, h) = content;
    let m = decor.margin_px;
    let corner = decor.radius_px.ceil() as u32 + 1;
    let border = (decor.scale.round() as u32).max(1);
    let x0 = rect.0.max(m);
    let y0 = rect.1.max(m);
    let x1 = rect.0.saturating_add(rect.2).min(m + w);
    let y1 = rect.1.saturating_add(rect.3).min(m + h);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let mut visit = |x: u32, y: u32| {
        let dist = sdf(x as f32 + 0.5, y as f32 + 0.5, content, decor);
        let is_corner = (x - m < corner || m + w - 1 - x < corner)
            && (y - m < corner || m + h - 1 - y < corner);
        if is_corner || dist >= -(border as f32) && dist <= 0.5 {
            let i = ((y * bw + x) * 4) as usize;
            paint_pixel(dst, i, dist, decor);
        }
    };
    for y in y0..y1 {
        let edge_row = y - m < border || m + h - 1 - y < border;
        if edge_row {
            for x in x0..x1 {
                visit(x, y);
            }
        } else {
            let strip = if y - m < corner || m + h - 1 - y < corner {
                corner
            } else {
                border
            };
            let left_end = (m + strip).min(x1);
            for x in x0.max(m)..left_end {
                visit(x, y);
            }
            let right_start = (m + w).saturating_sub(strip).max(x0).max(left_end);
            for x in right_start..x1 {
                visit(x, y);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn margins_are_exact_for_common_scales() {
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
            let decor = Decor::new(scale, true, false);
            assert_eq!(decor.margin_px as f32, MARGIN as f32 * scale);
        }
    }

    #[test]
    fn compose_rounding_and_border() {
        let decor = Decor::new(1.0, true, true);
        let content = (80, 60);
        let size = buffer_size(content, &decor);
        let mut pixels = vec![0; (size.0 * size.1 * 4) as usize];
        for y in decor.margin_px..decor.margin_px + content.1 {
            for x in decor.margin_px..decor.margin_px + content.0 {
                let i = ((y * size.0 + x) * 4) as usize;
                pixels[i..i + 4].copy_from_slice(&[38, 34, 34, 255]);
            }
        }
        compose_edges(&mut pixels, size, content, &decor, (0, 0, size.0, size.1));
        let at = |x: u32, y: u32, p: &[u8]| {
            let i = ((y * size.0 + x) * 4) as usize;
            [p[i], p[i + 1], p[i + 2], p[i + 3]]
        };
        assert!(at(decor.margin_px, decor.margin_px, &pixels)[3] < 80);
        assert_eq!(
            at(decor.margin_px + 40, decor.margin_px + 30, &pixels),
            [38, 34, 34, 255]
        );
        let top = at(decor.margin_px + 40, decor.margin_px, &pixels);
        assert!((top[0] as i16 - 53).abs() <= 2);
        assert!((top[1] as i16 - 50).abs() <= 2);
        assert!((top[2] as i16 - 50).abs() <= 2);
        assert_eq!(
            at(decor.margin_px + 40, decor.margin_px + 1, &pixels),
            [38, 34, 34, 255]
        );

        let light = Decor {
            dark: false,
            ..decor
        };
        let mut untouched = vec![0; (size.0 * size.1 * 4) as usize];
        for y in decor.margin_px..decor.margin_px + content.1 {
            for x in decor.margin_px..decor.margin_px + content.0 {
                let i = ((y * size.0 + x) * 4) as usize;
                untouched[i..i + 4].copy_from_slice(&[38, 34, 34, 255]);
            }
        }
        compose_edges(
            &mut untouched,
            size,
            content,
            &light,
            (0, 0, size.0, size.1),
        );
        assert_eq!(
            at(decor.margin_px + 40, decor.margin_px, &untouched),
            [38, 34, 34, 255]
        );
    }

    #[test]
    fn composing_quadrants_matches_a_single_full_pass() {
        let decor = Decor::new(1.0, true, true);
        let content = (80, 60);
        let size = buffer_size(content, &decor);
        let fresh = vec![96; (size.0 * size.1 * 4) as usize];
        let mut whole = fresh.clone();
        compose_edges(&mut whole, size, content, &decor, (0, 0, size.0, size.1));

        let mut split = fresh;
        let (mx, my) = (size.0 / 2, size.1 / 2);
        for rect in [
            (0, 0, mx, my),
            (mx, 0, size.0 - mx, my),
            (0, my, mx, size.1 - my),
            (mx, my, size.0 - mx, size.1 - my),
        ] {
            compose_edges(&mut split, size, content, &decor, rect);
        }
        assert_eq!(whole, split);
    }
}
