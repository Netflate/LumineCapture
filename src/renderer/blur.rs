use tiny_skia::{IntRect, Pixmap, Rect};

const PASSES: usize = 2;

// the stronger the blur, the more we can downscale
fn factor(radius: f32) -> (i32, usize) {
    let k = ((radius / 3.0).round() as i32).max(2);
    (k, (radius / k as f32).round() as usize)
}

fn reach(k: i32, r: usize) -> i32 {
    ((PASSES * r) as i32 + 2) * k
}

// pixels outside the rect affect the result too, `+ k` is room for the grid alignment
pub fn margin(radius: f32) -> f32 {
    let (k, r) = factor(radius);
    (reach(k, r) + k) as f32
}

pub fn draw(
    canvas: &mut Pixmap,
    rect: Rect,
    radius: f32,
    cached: Option<&Pixmap>,
    offset: (f32, f32),
) {
    let Some(local) = Rect::from_xywh(
        rect.left() - offset.0,
        rect.top() - offset.1,
        rect.width(),
        rect.height(),
    )
    .and_then(|r| r.round_out()) else {
        return;
    };
    match cached {
        Some(pixmap) => paste(canvas, pixmap, local.x(), local.y()),
        None => {
            let phase = (offset.0 as i32, offset.1 as i32);
            if let Some(pixmap) =
                Blurred::new(canvas, local, radius, phase).and_then(|b| b.crop(local))
            {
                paste(canvas, &pixmap, local.x(), local.y());
            }
        }
    }
}

pub struct Blurred {
    low: Pixmap,
    k: i32,
    at: (i32, i32),
}

impl Blurred {
    pub fn new(src: &Pixmap, rect: IntRect, radius: f32, phase: (i32, i32)) -> Option<Self> {
        let (k, r) = factor(radius);
        let m = reach(k, r);
        let align = |v: i32, p: i32| (v + p).div_euclid(k) * k - p;
        let ax = align(rect.x() - m, phase.0).max(0);
        let ay = align(rect.y() - m, phase.1).max(0);
        let aw = (rect.right() + m).min(src.width() as i32) - ax;
        let ah = (rect.bottom() + m).min(src.height() as i32) - ay;
        if aw <= 0 || ah <= 0 {
            return None;
        }

        let area = (ax as usize, ay as usize, aw as usize, ah as usize);
        let mut low = downsample(src, area, k as usize)?;
        let (lw, lh) = (low.width() as usize, low.height() as usize);
        smooth(low.data_mut(), lw, lh, r);
        Some(Self {
            low,
            k,
            at: (ax, ay),
        })
    }

    pub fn crop(&self, rect: IntRect) -> Option<Pixmap> {
        upscale(
            &self.low,
            self.k,
            (self.at.0 - rect.x(), self.at.1 - rect.y()),
            rect.width(),
            rect.height(),
        )
    }
}

// since pixels are opaque, there is no need in blending
// and copying rows is much faster than draw_pixmap
fn paste(canvas: &mut Pixmap, pixmap: &Pixmap, x: i32, y: i32) {
    let (cw, ch) = (canvas.width() as i32, canvas.height() as i32);
    let (x0, x1) = (x.max(0), (x + pixmap.width() as i32).min(cw));
    if x1 <= x0 {
        return;
    }
    let len = (x1 - x0) as usize * 4;
    let src_len = pixmap.width() as usize * 4;
    let skip = (x0 - x) as usize * 4;
    for row in y.max(0)..(y + pixmap.height() as i32).min(ch) {
        let src = &pixmap.data()[(row - y) as usize * src_len + skip..][..len];
        let at = (row * cw + x0) as usize * 4;
        canvas.data_mut()[at..at + len].copy_from_slice(src);
    }
}

fn downsample(
    src: &Pixmap,
    (ax, ay, aw, ah): (usize, usize, usize, usize),
    k: usize,
) -> Option<Pixmap> {
    let (lw, lh) = (aw.div_ceil(k), ah.div_ceil(k));
    let mut low = Pixmap::new(lw as u32, lh as u32)?;
    let stride = src.width() as usize;
    let pixels = src.data().as_chunks::<4>().0;
    let out = low.data_mut().as_chunks_mut::<4>().0;
    let mut acc = vec![[0u32; 4]; lw];
    for (ly, out_row) in out.chunks_exact_mut(lw).enumerate() {
        acc.fill([0; 4]);
        let rows = (ay + ly * k)..(ay + (ly + 1) * k).min(ay + ah);
        let count = rows.len();
        for y in rows {
            let row = &pixels[y * stride + ax..][..aw];
            for (a, block) in acc.iter_mut().zip(row.chunks(k)) {
                for p in block {
                    for (s, v) in a.iter_mut().zip(p) {
                        *s += *v as u32;
                    }
                }
            }
        }
        for (lx, (o, a)) in out_row.iter_mut().zip(&acc).enumerate() {
            let n = (k.min(aw - lx * k) * count) as u32;
            *o = a.map(|s| ((s + n / 2) / n) as u8);
        }
    }
    Some(low)
}

fn smooth(buf: &mut [u8], w: usize, h: usize, r: usize) {
    if r == 0 {
        return;
    }
    let mut tmp = vec![0u8; buf.len()];
    for _ in 0..PASSES {
        horizontal(buf, &mut tmp, w, r);
        vertical(&tmp, buf, w, h, r);
    }
}

fn horizontal(src: &[u8], dst: &mut [u8], w: usize, r: usize) {
    let mul = 65536 / (2 * r + 1) as u32;
    let (lo, hi) = (r + 1, w.saturating_sub(r + 1));
    for (src_row, dst_row) in src.chunks_exact(w * 4).zip(dst.chunks_exact_mut(w * 4)) {
        let px = |i: usize| {
            let p = &src_row[i * 4..i * 4 + 4];
            [p[0], p[1], p[2], p[3]].map(u32::from)
        };
        let at = |i: isize| px(i.clamp(0, w as isize - 1) as usize);

        let mut sum = [0u32; 4];
        for i in -(r as isize)..=r as isize {
            sum = slide(sum, at(i), [0; 4]);
        }
        for (x, out) in dst_row.chunks_exact_mut(4).enumerate() {
            for (o, s) in out.iter_mut().zip(sum) {
                *o = ((s * mul + 32768) >> 16) as u8;
            }
            let (next, prev) = if (lo..hi).contains(&x) {
                (px(x + r + 1), px(x - r))
            } else {
                (at((x + r + 1) as isize), at(x as isize - r as isize))
            };
            sum = slide(sum, next, prev);
        }
    }
}

fn slide(sum: [u32; 4], next: [u32; 4], prev: [u32; 4]) -> [u32; 4] {
    [0, 1, 2, 3].map(|c| sum[c] + next[c] - prev[c])
}

fn vertical(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize) {
    let mul = 65536 / (2 * r + 1) as u32;
    let len = w * 4;
    let row = |y: isize| &src[y.clamp(0, h as isize - 1) as usize * len..][..len];
    let r = r as isize;

    let mut sums = vec![0u32; len];
    for y in -r..=r {
        for (s, &v) in sums.iter_mut().zip(row(y)) {
            *s += v as u32;
        }
    }
    for (y, dst_row) in dst.chunks_exact_mut(len).enumerate() {
        for (o, &s) in dst_row.iter_mut().zip(&sums) {
            *o = ((s * mul + 32768) >> 16) as u8;
        }
        let y = y as isize;
        for ((s, &next), &prev) in sums.iter_mut().zip(row(y + r + 1)).zip(row(y - r)) {
            *s = *s + next as u32 - prev as u32;
        }
    }
}

// maps a pixel center to the center of a downscaled cell
fn taps(len: u32, k: i32, offset: i32, n: u32) -> Vec<(usize, usize, u32)> {
    let last = n as i32 - 1;
    (0..len as i32)
        .map(|p| {
            let u = ((2 * (p - offset) + 1 - k) * 256).div_euclid(2 * k);
            let i = u.div_euclid(256);
            (
                i.clamp(0, last) as usize,
                (i + 1).clamp(0, last) as usize,
                u.rem_euclid(256) as u32,
            )
        })
        .collect()
}

fn upscale(low: &Pixmap, k: i32, offset: (i32, i32), w: u32, h: u32) -> Option<Pixmap> {
    let mut out = Pixmap::new(w, h)?;
    let src = low.data();
    let src_len = low.width() as usize * 4;
    let xs = taps(w, k, offset.0, low.width());
    let ys = taps(h, k, offset.1, low.height());
    let stretch = |j: usize, row: &mut Vec<u16>| {
        row.clear();
        let line = &src[j * src_len..][..src_len];
        for &(a, b, t) in &xs {
            for c in 0..4 {
                let v = line[a * 4 + c] as u32 * (256 - t) + line[b * 4 + c] as u32 * t;
                row.push((v >> 4) as u16);
            }
        }
    };

    let (mut top, mut bottom) = (Vec::new(), Vec::new());
    let (mut top_j, mut bottom_j) = (usize::MAX, usize::MAX);
    let len = w as usize * 4;
    for (&(j0, j1, t), dst_row) in ys.iter().zip(out.data_mut().chunks_exact_mut(len)) {
        if j0 != top_j {
            if j0 == bottom_j {
                std::mem::swap(&mut top, &mut bottom);
                bottom_j = usize::MAX;
            } else {
                stretch(j0, &mut top);
            }
            top_j = j0;
        }
        if j1 != bottom_j {
            stretch(j1, &mut bottom);
            bottom_j = j1;
        }
        for (o, (&a, &b)) in dst_row.iter_mut().zip(top.iter().zip(&bottom)) {
            *o = ((a as u32 * (256 - t) + b as u32 * t + 2048) >> 12) as u8;
        }
    }
    Some(out)
}
