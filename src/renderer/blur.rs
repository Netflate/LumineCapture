use tiny_skia::{IntRect, Pixmap, Rect};

use super::paste;

const PASSES: usize = 3;

/// blur radius or size of pixelated cell
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    pub size: f32,
    pub pixelate: bool,
}

// the stronger the blur, the more we can downscale
fn factor(style: Style) -> (i32, usize) {
    if style.pixelate {
        return ((style.size.round() as i32).max(2), 0);
    }
    let k = ((style.size / 3.0).round() as i32).max(2);
    (k, (style.size / k as f32).round() as usize)
}

fn reach(k: i32, r: usize) -> i32 {
    ((PASSES * r) as i32 + 2) * k
}

// pixels outside the rect affect the result too, `+ k` is room for the grid alignment
pub fn margin(style: Style) -> f32 {
    let (k, r) = factor(style);
    (reach(k, r) + k) as f32
}

pub fn draw(
    canvas: &mut Pixmap,
    rect: Rect,
    style: Style,
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
                Blurred::new(canvas, local, style, phase).and_then(|b| b.crop(local))
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
    pixelate: bool,
}

impl Blurred {
    pub fn new(src: &Pixmap, rect: IntRect, style: Style, phase: (i32, i32)) -> Option<Self> {
        let (k, r) = factor(style);
        let m = reach(k, r);
        let align = |v: i32, p: i32| (v.max(0) + p).div_euclid(k) * k - p;
        let ax = align(rect.x() - m, phase.0);
        let ay = align(rect.y() - m, phase.1);
        let right = (rect.right() + m).min(src.width() as i32);
        let bottom = (rect.bottom() + m).min(src.height() as i32);
        if right <= ax.max(0) || bottom <= ay.max(0) {
            return None;
        }

        let mut low = downsample(src, (ax, ay, right, bottom), k)?;
        let (lw, lh) = (low.width() as usize, low.height() as usize);
        smooth(low.data_mut(), lw, lh, r);
        Some(Self {
            low,
            k,
            at: (ax, ay),
            pixelate: style.pixelate,
        })
    }

    pub fn crop(&self, rect: IntRect) -> Option<Pixmap> {
        upscale(
            &self.low,
            self.k,
            (self.at.0 - rect.x(), self.at.1 - rect.y()),
            rect.width(),
            rect.height(),
            self.pixelate,
        )
    }
}

fn downsample(
    src: &Pixmap,
    (ax, ay, right, bottom): (i32, i32, i32, i32),
    k: i32,
) -> Option<Pixmap> {
    let lw = ((right - ax) as usize).div_ceil(k as usize);
    let lh = ((bottom - ay) as usize).div_ceil(k as usize);
    let mut low = Pixmap::new(lw as u32, lh as u32)?;
    let stride = src.width() as usize;
    let pixels = src.data().as_chunks::<4>().0;
    let out = low.data_mut().as_chunks_mut::<4>().0;
    let x0 = ax.max(0);
    let first = (ax + k - x0) as usize;
    let mut acc = vec![[0u32; 4]; lw];
    for (ly, out_row) in out.chunks_exact_mut(lw).enumerate() {
        acc.fill([0; 4]);
        let top = ay + ly as i32 * k;
        let rows = top.max(0)..(top + k).min(bottom);
        let count = rows.len() as i32;
        for y in rows {
            let row = &pixels[y as usize * stride + x0 as usize..][..(right - x0) as usize];
            let (head, tail) = row.split_at(first.min(row.len()));
            let blocks = std::iter::once(head).chain(tail.chunks(k as usize));
            for (a, block) in acc.iter_mut().zip(blocks) {
                for p in block {
                    for (s, v) in a.iter_mut().zip(p) {
                        *s += *v as u32;
                    }
                }
            }
        }
        for (lx, (o, a)) in out_row.iter_mut().zip(&acc).enumerate() {
            let left = ax + lx as i32 * k;
            let n = (((left + k).min(right) - left.max(0)) * count) as u32;
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
fn taps(len: u32, k: i32, offset: i32, n: u32, nearest: bool) -> Vec<(usize, usize, u32)> {
    let last = n as i32 - 1;
    (0..len as i32)
        .map(|p| {
            let u = if nearest {
                (p - offset).div_euclid(k) * 256
            } else {
                ((2 * (p - offset) + 1 - k) * 256).div_euclid(2 * k)
            };
            let i = u.div_euclid(256);
            (
                i.clamp(0, last) as usize,
                (i + 1).clamp(0, last) as usize,
                u.rem_euclid(256) as u32,
            )
        })
        .collect()
}

fn upscale(
    low: &Pixmap,
    k: i32,
    offset: (i32, i32),
    w: u32,
    h: u32,
    nearest: bool,
) -> Option<Pixmap> {
    let mut out = Pixmap::new(w, h)?;
    let src = low.data();
    let src_len = low.width() as usize * 4;
    let xs = taps(w, k, offset.0, low.width(), nearest);
    let ys = taps(h, k, offset.1, low.height(), nearest);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn noise(w: u32, h: u32) -> Pixmap {
        let mut pixmap = Pixmap::new(w, h).unwrap();
        let mut seed = 0x2545_f491_u32;
        for px in pixmap.data_mut().chunks_exact_mut(4) {
            for c in &mut px[..3] {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                *c = seed as u8;
            }
            px[3] = 255;
        }
        pixmap
    }

    /// we need this test since every cache relies on this: piece blurred on a small source must
    /// exactly match the same piece of the whole image, otherwise pieces show visible seams where they meet
    #[test]
    fn piece_matches_whole() {
        let image = noise(320, 200);
        let whole = IntRect::from_xywh(0, 0, 320, 200).unwrap();
        let phase = (7, 3);
        let styles = [1.0, 4.0, 12.0, 40.0]
            .into_iter()
            .flat_map(|size| [false, true].map(|pixelate| Style { size, pixelate }));
        for style in styles {
            let full = Blurred::new(&image, whole, style, phase).unwrap();
            for (x, y, w, h) in [(70, 40, 90, 60), (0, 0, 50, 50), (270, 150, 50, 50)] {
                let rect = IntRect::from_xywh(x, y, w, h).unwrap();
                let margin = margin(style) as i32;
                let region = rect.make_outset(margin, margin).unwrap();
                let region = region.intersect(&whole).unwrap();
                let src = image.clone_rect(region).unwrap();
                let local = rect.translate(-region.x(), -region.y()).unwrap();
                let part_phase = (phase.0 + region.x(), phase.1 + region.y());
                let part = Blurred::new(&src, local, style, part_phase).unwrap();
                assert!(
                    full.crop(rect).unwrap().data() == part.crop(local).unwrap().data(),
                    "{style:?}, rect {rect:?}"
                );
            }
        }
    }
}
