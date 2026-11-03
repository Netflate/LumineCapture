use crate::profile::{BAND_COUNT, Signature, Weights, bottom_ignore, top_ignore};
use crate::types::NO_MATCH;

#[derive(Clone, Copy)]
pub(crate) struct Window {
    pub minimum: i32,
    pub top: i32,
    pub bottom: i32,
}

impl Window {
    fn crop(self, length: i32, height: i32) -> bool {
        length >= self.minimum + top_ignore(height) + bottom_ignore(height)
    }

    fn usable(self, y: i32, height: i32, crop: bool) -> bool {
        y >= self.top
            && y < height - self.bottom
            && (!crop || (y >= top_ignore(height) && y < height - bottom_ignore(height)))
    }
}

/// A row-indexed bitmask indicating which bands to skip during comparison
/// One bit per band. Rows past the end of the slice default to skipping nothing.
///
/// required to skip in comparison stuff like floating overlays
#[derive(Clone, Copy, Default)]
pub(crate) struct Skip<'a>(pub &'a [u8]);

impl Skip<'_> {
    fn at(self, y: i32) -> u8 {
        self.0.get(y as usize).copied().unwrap_or(0)
    }
}

fn textured(cols: &Signature, y: usize, weights: &Weights) -> bool {
    y > 0
        && weights
            .iter()
            .enumerate()
            .any(|(g, &weight)| weight > 0.0 && (cols[y][g] - cols[y - 1][g]).abs() > 1.0)
}

#[derive(Default)]
struct Sums {
    sums: [f32; BAND_COUNT],
    counts: [i32; BAND_COUNT],
    rows: i32,
    textured: i32,
}

impl Sums {
    fn add(&mut self, a: &[f32; BAND_COUNT], b: &[f32; BAND_COUNT], skip: u8, weights: &Weights) {
        let mut used = false;
        for g in 0..BAND_COUNT {
            if skip >> g & 1 == 0 && weights[g] > 0.0 {
                self.sums[g] += (a[g] - b[g]).abs();
                self.counts[g] += 1;
                used = true;
            }
        }
        self.rows += i32::from(used);
    }

    fn finish(&self, weights: &Weights, minimum: i32) -> f32 {
        if self.rows < minimum || self.textured < 8 {
            return NO_MATCH;
        }
        let enough = (self.rows / 4).max(8);
        let (mut sum, mut total, mut worst, mut worst_weight, mut active) = (0.0, 0.0, 0.0, 0.0, 0);
        for (g, &weight) in weights.iter().enumerate() {
            if weight <= 0.0 || self.counts[g] < enough {
                continue;
            }
            let value = weight * self.sums[g] / self.counts[g] as f32;
            sum += value;
            total += weight;
            active += 1;
            if value > worst {
                worst = value;
                worst_weight = weight;
            }
        }
        if active >= 4 {
            sum -= worst;
            total -= worst_weight;
        }
        if total <= 0.0 { NO_MATCH } else { sum / total }
    }
}

pub(crate) fn frame_diff(
    first: &Signature,
    second: &Signature,
    offset: i32,
    window: Window,
    weights: &Weights,
    (skip, first_skip): (Skip, Skip),
) -> f32 {
    let (h1, h2) = (first.len() as i32, second.len() as i32);
    let length = if offset >= 0 {
        (h1 - offset).min(h2)
    } else {
        h1.min(h2 + offset)
    };
    if h1 == 0 || h2 == 0 || length < window.minimum {
        return NO_MATCH;
    }
    let crop = window.crop(length, h1.min(h2));
    let (s1, s2) = if offset >= 0 {
        (offset, 0)
    } else {
        (0, -offset)
    };
    let mut sums = Sums::default();
    for i in 0..length {
        let (y1, y2) = (s1 + i, s2 + i);
        if !window.usable(y1, h1, crop) || !window.usable(y2, h2, crop) {
            continue;
        }
        sums.add(
            &first[y1 as usize],
            &second[y2 as usize],
            skip.at(y1) | skip.at(y2) | first_skip.at(y1),
            weights,
        );
        if sums.textured < 8 && textured(second, y2 as usize, weights) {
            sums.textured += 1;
        }
    }
    let minimum = if crop {
        24.max(window.minimum / 2)
    } else {
        window.minimum
    };
    sums.finish(weights, minimum)
}

pub(crate) fn overlap_diff(
    full: &Signature,
    frame: &Signature,
    position: i32,
    window: Window,
    excluded: (i32, i32),
    weights: &Weights,
    (skip, tainted): (Skip, Skip),
) -> (f32, i32) {
    let (full_h, frame_h) = (full.len() as i32, frame.len() as i32);
    let top = excluded.0.clamp(0, full_h);
    let bottom = excluded.1.clamp(0, full_h - top);
    let start = top.max(position);
    let end = (full_h - bottom).min(position + frame_h);
    let length = (end - start).max(0);
    if length < window.minimum {
        return (NO_MATCH, length);
    }
    let crop = window.crop(length, frame_h);
    let mut sums = Sums::default();
    for row in 0..length {
        let y = start - position + row;
        if !window.usable(y, frame_h, crop) {
            continue;
        }
        let at = start + row;
        sums.add(
            &full[at as usize],
            &frame[y as usize],
            skip.at(y) | tainted.at(at),
            weights,
        );
        if sums.textured < 8 && textured(frame, y as usize, weights) {
            sums.textured += 1;
        }
    }
    let minimum = if crop {
        24.max(window.minimum / 2)
    } else {
        window.minimum
    };
    (sums.finish(weights, minimum), length)
}
