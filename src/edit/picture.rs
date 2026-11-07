use tiny_skia::{Color, FilterQuality, Pixmap, PixmapPaint, Transform};

use super::view::View;

const SHARP_FROM: f32 = 2.0;

pub struct Picture {
    levels: Vec<Pixmap>,
    opaque: bool,
    /// Canvas state (zoom, pan, and size) from the last paint, panning shifts
    /// the old frame and renders only newly exposed areas
    shown: Option<Shown>,
}

#[derive(Clone, Copy)]
struct Shown {
    zoom: f32,
    pan: (f32, f32),
    size: (u32, u32),
}

impl Picture {
    pub fn new(pixmap: Pixmap) -> Self {
        let opaque = pixmap.pixels().iter().all(|p| p.alpha() == 255);
        Self {
            levels: vec![pixmap],
            opaque,
            shown: None,
        }
    }

    pub fn size(&self) -> (u32, u32) {
        let full = &self.levels[0];
        (full.width(), full.height())
    }

    pub fn draw(&mut self, canvas: &mut Pixmap, view: &View, backdrop: Color) {
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
            self.paint(canvas, view.pan, view.zoom, backdrop);
            return;
        };
        shift(canvas, dx, dy);
        for (x, y, w, h) in exposed(size, dx, dy) {
            let Some(mut part) = Pixmap::new(w, h) else {
                continue;
            };
            let pan = (view.pan.0 - x as f32, view.pan.1 - y as f32);
            self.paint(&mut part, pan, view.zoom, backdrop);
            crate::renderer::paste(canvas, &part, x as i32, y as i32);
        }
    }

    fn paint(&mut self, canvas: &mut Pixmap, pan: (f32, f32), zoom: f32, backdrop: Color) {
        canvas.fill(backdrop);
        // whole pixels at 100% and from SHARP_FROM up
        // in between nearest makes columns of uneven width, so it's smoothed like zooming out
        let (x, y) = (pan.0.round(), pan.1.round());
        if zoom == 1.0 && self.opaque {
            crate::renderer::paste(canvas, &self.levels[0], x as i32, y as i32);
            return;
        }
        if zoom >= SHARP_FROM {
            let full = &self.levels[0];
            draw(canvas, full, zoom, zoom, (x, y), FilterQuality::Nearest);
            return;
        }

        let (w, h) = self.size();
        let level = self.level((1.0 / zoom).log2().floor().max(0.0) as usize);
        let sx = zoom * w as f32 / level.width() as f32;
        let sy = zoom * h as f32 / level.height() as f32;
        draw(canvas, level, sx, sy, pan, FilterQuality::Bilinear);
    }

    fn level(&mut self, wanted: usize) -> &Pixmap {
        while self.levels.len() <= wanted {
            let Some(half) = self.levels.last().and_then(halve) else {
                break;
            };
            self.levels.push(half);
        }
        &self.levels[wanted.min(self.levels.len() - 1)]
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

fn whole_shift(from: (f32, f32), to: (f32, f32), (w, h): (u32, u32)) -> Option<(i32, i32)> {
    let whole = |d: f32| ((d - d.round()).abs() < 1e-3).then_some(d.round() as i32);
    let (dx, dy) = (whole(to.0 - from.0)?, whole(to.1 - from.1)?);
    (dx.unsigned_abs() < w && dy.unsigned_abs() < h).then_some((dx, dy))
}

fn shift(canvas: &mut Pixmap, dx: i32, dy: i32) {
    let offset = (dy as isize * canvas.width() as isize + dx as isize) * 4;
    let data = canvas.data_mut();
    let skip = offset.unsigned_abs();
    if offset > 0 {
        data.copy_within(..data.len() - skip, skip);
    } else if offset < 0 {
        data.copy_within(skip.., 0);
    }
}

fn exposed((w, h): (u32, u32), dx: i32, dy: i32) -> Vec<(u32, u32, u32, u32)> {
    let mut rects = Vec::new();
    match dy.signum() {
        1 => rects.push((0, 0, w, dy as u32)),
        -1 => rects.push((0, h - dy.unsigned_abs(), w, dy.unsigned_abs())),
        _ => {}
    }
    match dx.signum() {
        1 => rects.push((0, 0, dx as u32, h)),
        -1 => rects.push((w - dx.unsigned_abs(), 0, dx.unsigned_abs(), h)),
        _ => {}
    }
    rects
}

fn halve(src: &Pixmap) -> Option<Pixmap> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
            let mut picture = Picture::new(image.clone());
            let mut view = View::new((300, 200));
            view.resize((120, 90), 1.0);
            view.zoom_to(zoom, (17.0, 9.0));
            let mut canvas = Pixmap::new(120, 90).unwrap();
            picture.draw(&mut canvas, &view, backdrop);
            for step in [
                (-7.0, -5.0),
                (3.0, 11.0),
                (2.5, -1.5),
                (-40.0, 0.0),
                (0.0, -30.0),
            ] {
                view.pan_by(step);
                picture.draw(&mut canvas, &view, backdrop);
                let mut full = Pixmap::new(120, 90).unwrap();
                Picture::new(image.clone()).draw(&mut full, &view, backdrop);
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
        let mut picture = Picture::new(Pixmap::new(4, 4).unwrap());
        assert_eq!(picture.level(10).width(), 1);
        assert_eq!(picture.levels.len(), 3);
    }
}
