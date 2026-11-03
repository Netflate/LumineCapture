use crate::overlay::near;
use crate::types::NO_MATCH;
use shot_image::{Image, ImageError, MAX_PIXELS};

pub const BAND_COUNT: usize = 8;
pub type Signature = Vec<[f32; BAND_COUNT]>;
pub type Weights = [f32; BAND_COUNT];
pub const ALL_BANDS: Weights = [1.0; BAND_COUNT];
pub const ROW_MAX_DIFF: f32 = 2.0;

pub fn band_columns(width: i32, band: usize) -> (i32, i32) {
    if width <= 0 {
        return (0, 0);
    }
    let span = (0.96_f32 - 0.04_f32) / BAND_COUNT as f32;
    let start = 0.04 + span * band as f32;
    let first = (width as f32 * start).round() as i32;
    let last = (width as f32 * (start + span * 0.95)).round() as i32;
    (
        first.clamp(0, width - 1),
        last.clamp(first.clamp(0, width - 1), width - 1),
    )
}

pub fn compute(frame: &Image) -> Result<Signature, ImageError> {
    let rows = frame.height() as usize;
    if (frame.width() as usize + BAND_COUNT)
        .checked_mul(rows)
        .is_none_or(|count| count > MAX_PIXELS)
    {
        return Err(ImageError::Dimensions);
    }
    let mut result = Vec::new();
    result
        .try_reserve_exact(rows)
        .map_err(|_| ImageError::Allocation)?;
    let sampler = Sampler::new(frame.width());
    result.extend((0..rows).map(|y| sampler.profile(frame.row(y as i32))));
    Ok(result)
}

/// points of a row that make its signature, so one row can be redone alone.
pub(crate) struct Sampler {
    bands: [(i32, i32, f32); BAND_COUNT],
    width: i32,
}

impl Sampler {
    pub fn new(width: i32) -> Self {
        let bands = std::array::from_fn(|band| {
            let (start, end) = band_columns(width, band);
            let count = (end - start + 1).clamp(1, 9);
            let step = if count > 1 {
                (end - start) as f32 / (count - 1) as f32
            } else {
                0.0
            };
            (start, count, step)
        });
        Self { bands, width }
    }

    pub fn profile(&self, row: &[u32]) -> [f32; BAND_COUNT] {
        self.bands.map(|(start, count, step)| {
            let mut sum = 0.0;
            for i in 0..count {
                let x = ((start as f32 + i as f32 * step).round() as i32).clamp(0, self.width - 1);
                let pixel = row[x as usize];
                sum += 0.299 * ((pixel >> 16) & 255) as f32
                    + 0.587 * ((pixel >> 8) & 255) as f32
                    + 0.114 * (pixel & 255) as f32;
            }
            sum / count as f32
        })
    }
}

fn scaled_ignore(height: i32, ratio: f32) -> i32 {
    if height < 80 {
        0
    } else {
        (height / 4).min(16.max((height as f32 * ratio) as i32))
    }
}

pub fn top_ignore(height: i32) -> i32 {
    scaled_ignore(height, 0.10)
}

pub fn bottom_ignore(height: i32) -> i32 {
    scaled_ignore(height, 0.08)
}

pub fn side_ignore(width: i32) -> i32 {
    if width <= 0 {
        0
    } else {
        50.max(width / 20)
            .min(width / 3)
            .min(((width - 1) / 2).max(0))
    }
}

pub fn row_diff(a: &[u32], b: &[u32], start: i32, width: i32) -> f32 {
    if start < 0
        || width <= 0
        || (i64::from(start) + i64::from(width)) as usize > a.len().min(b.len())
    {
        return NO_MATCH;
    }
    let mut sum = 0.0;
    let mut count = 0;
    for x in (start as usize..(start + width) as usize).step_by((width / 256).max(1) as usize) {
        for shift in [16, 8, 0] {
            sum += (((a[x] >> shift) & 255) as i32 - ((b[x] >> shift) & 255) as i32).abs() as f32;
            count += 1;
        }
    }
    if count > 0 {
        sum / count as f32
    } else {
        NO_MATCH
    }
}

/// Pixels of a row `exact_rows` looks at
const SAMPLES: usize = 256;
/// Fewer columns than this that changed tell too little to consider
const MIN_MOVING: usize = 16;
/// Changes of color along a row it takes to tell where the row is
const MIN_EDGES: usize = 8;

/// Selects columns that actually changed between frames for `exact_rows` to compare,
/// ignoring static side panels, docks, and repeating UI patterns (like chat lists)
/// that could cause false matches during a scroll.
pub(crate) fn moving_columns(previous: &Image, current: &Image) -> Vec<usize> {
    let (w, h) = (current.width(), current.height());
    let side = side_ignore(w).max(0) as usize;
    let all = side..(w.max(0) as usize).saturating_sub(side);
    let even = |columns: &[usize]| -> Vec<usize> {
        let step = columns.len().div_ceil(SAMPLES).max(1);
        columns.iter().step_by(step).copied().collect()
    };
    let every: Vec<usize> = all.collect();
    if previous.width() != w || previous.height() != h {
        return even(&every);
    }
    let candidates = even(&every);
    let mut moved = vec![false; candidates.len()];
    for y in (0..h).step_by(4) {
        let (before, now) = (previous.row(y), current.row(y));
        for (moved, &x) in moved.iter_mut().zip(&candidates) {
            *moved |= !near(before[x], now[x]);
        }
    }
    let moving: Vec<usize> = candidates
        .iter()
        .zip(&moved)
        .filter(|(_, moved)| **moved)
        .map(|(&x, _)| x)
        .collect();
    if moving.len() < MIN_MOVING {
        candidates
    } else {
        moving
    }
}

/// Whether row `b` shows what row `a` does, pixel for pixel give or take what a translucent
/// window lets through, at `columns`. `None` when `b` has too little drawn there to tell
/// where it is (see `MIN_EDGES`).
pub(crate) fn rows_alike(a: &[u32], b: &[u32], columns: &[usize]) -> Option<bool> {
    if columns.iter().any(|&x| x >= a.len().min(b.len())) {
        return None;
    }
    let edges = columns
        .windows(2)
        .filter(|pair| !near(b[pair[0]], b[pair[1]]))
        .count();
    if edges < MIN_EDGES {
        return None;
    }
    Some(columns.iter().all(|&x| near(a[x], b[x])))
}

/// Counts how many alternating rows in `current` match the corresponding rows in `previous`
/// shifted by `offset` across the given `columns`.
///
/// At the correct scroll offset, a vast majority of the page rows will match perfectly;
/// at an incorrect offset, matches will be sparse or absent.
pub(crate) fn exact_rows(previous: &Image, current: &Image, offset: i32, columns: &[usize]) -> i32 {
    let (w, h) = (current.width(), current.height());
    if previous.width() != w || previous.height() != h {
        return 0;
    }
    (0..h)
        .step_by(2)
        .filter(|&y| (0..h).contains(&(y + offset)))
        .filter(|&y| rows_alike(previous.row(y + offset), current.row(y), columns) == Some(true))
        .count() as i32
}
