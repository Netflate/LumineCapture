use tiny_skia::{Color, FilterQuality, Pixmap, PixmapPaint, Transform};

use super::view::View;

const SHARP_FROM: f32 = 2.0;

pub struct Picture {
    /// Halved copies generated on demand.
    ///
    /// Pure bilinear scaling skips rows when zoomed far out, causing text in long
    /// captures to shimmer during movement. The full-size version remains with
    /// the editor's captures.
    levels: Vec<Pixmap>,
    opaque: bool,
    /// Canvas state (zoom, pan, and size) from the last paint, panning shifts
    /// the old frame and renders only newly exposed areas
    shown: Option<Shown>,
}

#[derive(Clone, Copy)]
pub(super) struct Shown {
    pub(super) zoom: f32,
    pub(super) pan: (f32, f32),
    pub(super) size: (u32, u32),
}

impl Picture {
    pub fn new(full: &Pixmap) -> Self {
        Self {
            levels: Vec::new(),
            opaque: full.pixels().iter().all(|p| p.alpha() == 255),
            shown: None,
        }
    }

    pub fn draw(&mut self, full: &Pixmap, canvas: &mut Pixmap, view: &View, backdrop: Color) {
        let size = (canvas.width(), canvas.height());
        let moved = self
            .shown
            .filter(|shown| shown.zoom == view.zoom && shown.size == size)
            .and_then(|shown| whole_shift(shown.pan, view.pan, size));
        self.shown = Some(Shown {
            zoom: view.zoom,
            pan: view.pan,
            size,
        });
        let Some((dx, dy)) = moved else {
            self.paint(full, canvas, view.pan, view.zoom, backdrop);
            return;
        };
        crate::renderer::shift_pixels(canvas, dx, dy);
        for (x, y, w, h) in crate::renderer::exposed_strips(size, dx, dy) {
            let Some(mut part) = Pixmap::new(w, h) else {
                continue;
            };
            let pan = (view.pan.0 - x as f32, view.pan.1 - y as f32);
            self.paint(full, &mut part, pan, view.zoom, backdrop);
            crate::renderer::paste(canvas, &part, x as i32, y as i32);
        }
    }

    fn paint(
        &mut self,
        full: &Pixmap,
        canvas: &mut Pixmap,
        pan: (f32, f32),
        zoom: f32,
        backdrop: Color,
    ) {
        canvas.fill(backdrop);
        // whole pixels at 100% and from SHARP_FROM up
        // in between nearest makes columns of uneven width, so it's smoothed like zooming out
        let (x, y) = (pan.0.round(), pan.1.round());
        if zoom == 1.0 && self.opaque {
            crate::renderer::paste(canvas, full, x as i32, y as i32);
            return;
        }
        if zoom >= SHARP_FROM {
            draw(canvas, full, zoom, zoom, (x, y), FilterQuality::Nearest);
            return;
        }

        let (w, h) = (full.width(), full.height());
        let level = self.level(full, (1.0 / zoom).log2().floor().max(0.0) as usize);
        let sx = zoom * w as f32 / level.width() as f32;
        let sy = zoom * h as f32 / level.height() as f32;
        draw(canvas, level, sx, sy, pan, FilterQuality::Bilinear);
    }

    fn level<'a>(&'a mut self, full: &'a Pixmap, wanted: usize) -> &'a Pixmap {
        while self.levels.len() < wanted {
            let Some(half) = halve(self.levels.last().unwrap_or(full)) else {
                break;
            };
            self.levels.push(half);
        }
        match wanted.min(self.levels.len()) {
            0 => full,
            n => &self.levels[n - 1],
        }
    }
}

fn draw(
    canvas: &mut Pixmap,
    pixmap: &Pixmap,
    sx: f32,
    sy: f32,
    (x, y): (f32, f32),
    quality: FilterQuality,
) {
    let paint = PixmapPaint {
        quality,
        ..PixmapPaint::default()
    };
    canvas.draw_pixmap(
        0,
        0,
        pixmap.as_ref(),
        &paint,
        Transform::from_row(sx, 0.0, 0.0, sy, x, y),
        None,
    );
}

pub(super) fn whole_shift(
    from: (f32, f32),
    to: (f32, f32),
    (w, h): (u32, u32),
) -> Option<(i32, i32)> {
    let whole = |d: f32| ((d - d.round()).abs() < 1e-3).then_some(d.round() as i32);
    let (dx, dy) = (whole(to.0 - from.0)?, whole(to.1 - from.1)?);
    (dx.unsigned_abs() < w && dy.unsigned_abs() < h).then_some((dx, dy))
}

fn halve(src: &Pixmap) -> Option<Pixmap> {
    let (w, h) = (src.width() as usize, src.height() as usize);
    if w < 2 && h < 2 {
        return None;
    }
    let (hw, hh) = (w.div_ceil(2), h.div_ceil(2));
    let mut out = Pixmap::new(hw as u32, hh as u32)?;
    let data = src.data();
    let row = |y: usize| &data[y.min(h - 1) * w * 4..][..w * 4];
    for (y, dst) in out.data_mut().chunks_exact_mut(hw * 4).enumerate() {
        halve_rows(row(2 * y), row(2 * y + 1), dst);
    }
    Some(out)
}

fn halve_rows(a: &[u8], b: &[u8], dst: &mut [u8]) {
    const EVEN: u32 = 0x00ff_00ff;
    let px = |c: &[u8]| u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
    let avg = |p: [u32; 4]| {
        let even = p.iter().map(|v| v & EVEN).sum::<u32>() + 0x0002_0002;
        let odd = p.iter().map(|v| (v >> 8) & EVEN).sum::<u32>() + 0x0002_0002;
        ((even >> 2) & EVEN) | (((odd >> 2) & EVEN) << 8)
    };
    let ((a, a_odd), (b, b_odd)) = (a.as_chunks::<8>(), b.as_chunks::<8>());
    let (out, _) = dst.as_chunks_mut::<4>();
    for ((a, b), d) in a.iter().zip(b).zip(&mut *out) {
        *d = avg([px(&a[..4]), px(&a[4..]), px(&b[..4]), px(&b[4..])]).to_le_bytes();
    }
    if !a_odd.is_empty() {
        let (p, q) = (px(a_odd), px(b_odd));
        out[a.len()] = avg([p, p, q, q]).to_le_bytes();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn halve_reference(src: &Pixmap) -> Option<Pixmap> {
        let (w, h) = (src.width() as usize, src.height() as usize);
        if w < 2 && h < 2 {
            return None;
        }
        let (hw, hh) = (w.div_ceil(2), h.div_ceil(2));
        let mut out = Pixmap::new(hw as u32, hh as u32)?;
        let data = src.data();
        let dst = out.data_mut();
        for y in 0..hh {
            let (r0, r1) = (2 * y * w, (2 * y + 1).min(h - 1) * w);
            for x in 0..hw {
                let (c0, c1) = (2 * x, (2 * x + 1).min(w - 1));
                let at = (y * hw + x) * 4;
                for ch in 0..4 {
                    let sum = data[(r0 + c0) * 4 + ch] as u32
                        + data[(r0 + c1) * 4 + ch] as u32
                        + data[(r1 + c0) * 4 + ch] as u32
                        + data[(r1 + c1) * 4 + ch] as u32;
                    dst[at + ch] = ((sum + 2) / 4) as u8;
                }
            }
        }
        Some(out)
    }

    fn noise(w: u32, h: u32, seed: u64) -> Pixmap {
        let mut pixmap = Pixmap::new(w, h).unwrap();
        let mut state = seed;
        for byte in pixmap.data_mut() {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            *byte = (state >> 56) as u8;
        }
        pixmap
    }

    #[test]
    fn halving_matches_the_reference() {
        let sizes = [
            (1, 1),
            (1, 7),
            (7, 1),
            (2, 2),
            (3, 5),
            (5, 3),
            (64, 64),
            (1001, 37),
        ];
        for (i, (w, h)) in sizes.into_iter().enumerate() {
            let src = noise(w, h, i as u64 + 1);
            let (new, old) = (halve(&src), halve_reference(&src));
            assert_eq!(new.is_none(), old.is_none(), "{w}x{h}");
            if let (Some(new), Some(old)) = (new, old) {
                assert_eq!((new.width(), new.height()), (old.width(), old.height()));
                assert!(new.data() == old.data(), "{w}x{h}");
            }
        }
    }

    #[test]
    fn halving_averages_and_keeps_odd_edges() {
        let mut src = Pixmap::new(3, 1).unwrap();
        let px = |v: u8| tiny_skia::PremultipliedColorU8::from_rgba(v, v, v, 255).unwrap();
        src.pixels_mut().copy_from_slice(&[px(0), px(100), px(200)]);
        let half = halve(&src).unwrap();
        assert_eq!((half.width(), half.height()), (2, 1));
        assert_eq!(half.pixels()[0].red(), 50);
        assert_eq!(half.pixels()[1].red(), 200);
    }

    #[test]
    fn panning_by_moving_the_frame_matches_a_full_paint() {
        let mut image = Pixmap::new(300, 200).unwrap();
        for (i, px) in image.pixels_mut().iter_mut().enumerate() {
            let v = (i * 37 % 251) as u8;
            *px = tiny_skia::PremultipliedColorU8::from_rgba(v, v / 2, 255 - v, 255).unwrap();
        }
        let backdrop = Color::from_rgba8(20, 20, 20, 255);
        for zoom in [0.3, 0.8, 1.0, 1.5, 3.0] {
            let mut picture = Picture::new(&image);
            let mut view = View::new((300, 200));
            view.resize((120, 90), 1.0);
            view.zoom_to(zoom, (17.0, 9.0));
            let mut canvas = Pixmap::new(120, 90).unwrap();
            picture.draw(&image, &mut canvas, &view, backdrop);
            for step in [
                (-7.0, -5.0),
                (3.0, 11.0),
                (2.5, -1.5),
                (-40.0, 0.0),
                (0.0, -30.0),
            ] {
                view.pan_by(step);
                picture.draw(&image, &mut canvas, &view, backdrop);
                let mut full = Pixmap::new(120, 90).unwrap();
                Picture::new(&image).draw(&image, &mut full, &view, backdrop);
                let off = canvas
                    .data()
                    .iter()
                    .zip(full.data())
                    .filter(|(a, b)| a.abs_diff(**b) > 1)
                    .count();
                assert_eq!(off, 0, "zoom {zoom}, step {step:?}");
            }
        }
    }

    #[test]
    fn levels_stop_at_one_pixel() {
        let full = Pixmap::new(4, 4).unwrap();
        let mut picture = Picture::new(&full);
        assert_eq!(picture.level(&full, 0).width(), 4);
        assert_eq!(picture.level(&full, 10).width(), 1);
        assert_eq!(picture.levels.len(), 2);
    }
}
