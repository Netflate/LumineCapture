//! Elements that break predictable pixel matching
//! fixed UI overlays (docks, tooltips, sticky bars, shadows, translucent plaques)
//! as well as self-changing content like videos.
//!
//! they don't shift predictably with the page,
//! and isolating them is the core job of this module.
use crate::profile::{BAND_COUNT, band_columns};
use shot_image::Image;

/// Columns per cell. Masks are per row of pixels and per cell of columns.
pub(crate) const CELL: usize = 8;
/// How far a mask grows around what was found, in cells and rows
/// because of things like soft shadows, rounded corners and the flat padding
/// of a plaque differ from the page by too little to be found.
const GROW_X: usize = 2;
const GROW_Y: usize = 16;
/// Tolerance for pixel differences. Since an overlay can have semi-transparent pixels,
/// we still need to distinguish it while accepting a normal variance in pixel color.
const TOLERANCE: u32 = 8;
/// Threshold for unstable/self-changing cells (videos, smooth zoom, fractional scaling...).
/// If more than 25% of the compared cells change on their own, pixel-exact scrolling
/// is deemed unreliable, and the algorithm falls back to trusting only completely static elements.
const RESTLESS_SHARE: f32 = 0.25;
/// Frames a cell stays covered after it was last found so. The flat inside of a float is
/// never found on its own and lives on this; a cell found by mistake fades away.
const LIFE: u8 = 6;

#[inline]
pub(crate) fn near(a: u32, b: u32) -> bool {
    let close = |s: u32| ((a >> s) & 255).abs_diff((b >> s) & 255) <= TOLERANCE;
    a == b || (close(16) && close(8) && close(0))
}

pub(crate) fn same(a: &[u32], b: &[u32]) -> bool {
    a == b || (a.len() == b.len() && a.iter().zip(b).all(|(&x, &y)| near(x, y)))
}

pub(crate) fn detailed(cell: &[u32]) -> bool {
    cell.iter().any(|&p| !near(p, cell[0]))
}

pub(crate) fn cells(width: i32) -> usize {
    (width.max(0) as usize).div_ceil(CELL)
}

pub(crate) fn span(cx: usize, width: usize) -> std::ops::Range<usize> {
    cx * CELL..((cx + 1) * CELL).min(width)
}

/// Cells of each band of the signature, to tell which bands a covered cell spoils.
#[derive(Clone)]
pub(crate) struct Bands([(usize, usize); BAND_COUNT]);

impl Bands {
    pub fn new(width: i32) -> Self {
        Self(std::array::from_fn(|g| {
            let (first, last) = band_columns(width, g);
            (first.max(0) as usize / CELL, last.max(0) as usize / CELL)
        }))
    }

    /// Bit `g` set when a cell `hit` says yes to lies in band `g`.
    pub fn of(&self, hit: impl Fn(usize) -> bool) -> u8 {
        let mut bits = 0;
        for (g, &(first, last)) in self.0.iter().enumerate() {
            if (first..=last).any(&hit) {
                bits |= 1 << g;
            }
        }
        bits
    }
}

/// How sure a mask is that a cell is covered: a float `FOUND` right there in this frame,
/// `NEAR` one, or from an earlier frame, as sure as the frames it has left.
const FOUND: u8 = LIFE + 2;
pub(crate) const NEAR: u8 = LIFE + 1;

/// Screen cells covered by something that doesn't scroll with the page, and how sure that is.
#[derive(Clone, Default)]
pub(crate) struct Mask {
    pub cells: usize,
    pub rows: usize,
    /// frames a cell found to float has left to stay so; what is near it is covered too,
    /// but only for as long as it lasts, or the margins of a few frames would add up
    life: Vec<u8>,
    sure: Vec<u8>,
    /// cells of this frame that changed on their own since the last one, a video in the page
    /// as much as a float over it, trusted or not: they can't tell where the frame goes
    changing: Vec<bool>,
}

impl Mask {
    pub fn empty(width: i32, height: i32) -> Self {
        let (cells, rows) = (cells(width), height.max(0) as usize);
        Self {
            cells,
            rows,
            life: vec![0; cells * rows],
            sure: vec![0; cells * rows],
            changing: vec![false; cells * rows],
        }
    }

    /// Whether the cell changed on its own since the last frame, as far as this frame knows.
    pub fn changing(&self, y: usize, cx: usize) -> bool {
        self.changing[y * self.cells + cx]
    }

    /// 0 when not covered.
    pub fn sure(&self, y: usize, cx: usize) -> u8 {
        self.sure[y * self.cells + cx]
    }

    /// `sure` of every cell, row after row.
    pub fn sure_cells(&self) -> &[u8] {
        &self.sure
    }

    pub fn row(&self, y: usize) -> impl Fn(usize) -> bool + '_ {
        let row = &self.sure[y * self.cells..(y + 1) * self.cells];
        move |cx| row[cx] > 0
    }

    /// Share of the screen covered.
    pub fn share(&self) -> f32 {
        self.sure.iter().filter(|&&sure| sure > 0).count() as f32 / self.sure.len().max(1) as f32
    }

    fn fits(&self, image: &Image) -> bool {
        self.cells == cells(image.width()) && self.rows == image.height() as usize
    }

    /// Whole rows covered, like a sticky header or footer.
    pub fn cover_rows(&mut self, rows: std::ops::Range<usize>) {
        let rows = rows.start.min(self.rows)..rows.end.min(self.rows);
        self.sure[rows.start * self.cells..rows.end * self.cells].fill(FOUND);
    }

    /// Bands of every row where something changed on its own.
    pub fn changing_band_rows(&self, bands: &Bands) -> Vec<u8> {
        (0..self.rows)
            .map(|y| bands.of(|cx| self.changing(y, cx)))
            .collect()
    }

    pub fn band_rows(&self, bands: &Bands) -> Vec<u8> {
        (0..self.rows).map(|y| bands.of(self.row(y))).collect()
    }
}

// What is known of a cell of `current` at screen row `y`, as bits.
/// `previous` shows the page of this cell at `y + offset`, on screen,
const AHEAD: u8 = 1;
/// and it shows something else there.
const AHEAD_DIFFERS: u8 = 1 << 1;
/// What `previous` shows at `y` is in `current` at `y - offset`,
const BEHIND: u8 = 1 << 2;
/// and it shows something else there.
const BEHIND_DIFFERS: u8 = 1 << 3;
/// `previous` shows the same at `y`: it stayed put on screen.
const STILL: u8 = 1 << 4;
/// Not one flat color.
const DETAILED: u8 = 1 << 5;

/// Identifies floating ui elements and changing content (like docks, tooltips, or videos)
/// by comparing the current frame with the previous one at the given scroll offset.
///
/// Core logic:
/// - If a pixel block doesn't match where it should be according to the scroll offset,
///   it's either a fixed overlay or self-changing content (like a video).
/// - Carries over mask history from previous frames so transparent plaques fading elements
///   don't instantly "blink" out of existence.
/// - Handles sticky top/bottom bars (`bars`) where the page goes underneath them.
pub(crate) fn find(
    previous: &Image,
    current: &Image,
    offset: i32,
    earlier: Option<&Mask>,
    bars: (usize, usize),
) -> Mask {
    let (w, h) = (current.width() as usize, current.height() as usize);
    let mut mask = Mask::empty(current.width(), current.height());
    if previous.width() != current.width() || previous.height() != current.height() {
        return mask;
    }
    let earlier = earlier.filter(|earlier| earlier.fits(current));
    if offset == 0 {
        // nothing moved, nothing to learn
        return earlier.cloned().map_or(mask, |mut earlier| {
            earlier.changing.fill(false);
            earlier
        });
    }
    let cw = mask.cells;
    let page = bars.0.min(h) as i64..h.saturating_sub(bars.1) as i64;
    let at = |y: i64| page.contains(&y).then_some(y as usize);
    let mut known = vec![0u8; cw * h];
    for y in 0..h {
        let (before, now) = (previous.row(y as i32), current.row(y as i32));
        let ahead = at(y as i64 + offset as i64);
        let behind = at(y as i64 - offset as i64);
        // most rows move with the page as a whole, or stay as they were
        let ahead_same = ahead.is_some_and(|a| previous.row(a as i32) == now);
        let behind_same = behind.is_some_and(|b| current.row(b as i32) == before);
        let still_row = before == now;
        for cx in 0..cw {
            let range = span(cx, w);
            let cell = &now[range.clone()];
            let mut bits = 0;
            if detailed(cell) {
                bits |= DETAILED;
            }
            if still_row || same(&before[range.clone()], cell) {
                bits |= STILL;
            }
            if let Some(a) = ahead {
                bits |= AHEAD;
                if !ahead_same && !same(&previous.row(a as i32)[range.clone()], cell) {
                    bits |= AHEAD_DIFFERS;
                }
            }
            if let Some(b) = behind {
                bits |= BEHIND;
                if !behind_same && !same(&current.row(b as i32)[range.clone()], &before[range]) {
                    bits |= BEHIND_DIFFERS;
                }
            }
            known[y * cw + cx] = bits;
        }
    }
    let has = |bits: u8, flag: u8| bits & flag != 0;
    // a vertical border or a table line stays put under scrolling as well, but it matches
    // where its page went too; a float doesn't
    let moves = |bits: u8| {
        (has(bits, AHEAD) && !has(bits, AHEAD_DIFFERS))
            || (has(bits, BEHIND) && !has(bits, BEHIND_DIFFERS))
    };
    let stays = |bits: u8| has(bits, STILL) && has(bits, DETAILED) && !moves(bits);
    // (0 = page, 1 = stays put, 2 = changes on its own)
    let mut found = vec![0u8; cw * h];
    let (mut compared, mut restless) = (0usize, 0usize);
    for y in 0..h {
        for cx in 0..cw {
            let c = y * cw + cx;
            let bits = known[c];
            // a pair of spots that differ holds a float at one of them: at this one if the
            // other shows nothing of the kind on its own
            let ahead = at(y as i64 + offset as i64).map_or(0, |a| known[a * cw + cx]);
            let behind = at(y as i64 - offset as i64).map_or(0, |b| known[b * cw + cx]);
            let differs_ahead =
                has(bits, AHEAD_DIFFERS) && !(stays(ahead) || has(ahead, AHEAD_DIFFERS));
            let differs_behind =
                has(bits, BEHIND_DIFFERS) && !(stays(behind) || has(behind, BEHIND_DIFFERS));
            let float = if has(bits, AHEAD) {
                let own = stays(bits) || has(bits, BEHIND_DIFFERS);
                has(bits, AHEAD_DIFFERS) && (own || differs_ahead)
            } else {
                // just revealed: what stayed put, or what the last frame had here and not
                // where its page went
                stays(bits) || differs_behind
            };
            if float {
                found[c] = if has(bits, STILL) { 1 } else { 2 };
            }
            mask.changing[c] = float && !has(bits, STILL);
            // the share is of cells checked against where their page went
            if has(bits, AHEAD) {
                compared += 1;
                restless += usize::from(float && !has(bits, STILL));
            }
        }
    }
    let trust_restless = (restless as f32) <= compared as f32 * RESTLESS_SHARE;
    let seeds: Vec<bool> = found
        .iter()
        .map(|&f| f == 1 || (f == 2 && trust_restless))
        .collect();
    for (c, life) in mask.life.iter_mut().enumerate() {
        *life = if seeds[c] {
            LIFE
        } else {
            earlier.map_or(0, |earlier| earlier.life[c].saturating_sub(1))
        };
    }
    let lingering: Vec<bool> = mask.life.iter().map(|&life| life > 0).collect();
    let (mut near, mut near_any) = (vec![false; cw * h], vec![false; cw * h]);
    grow(&seeds, cw, h, &mut near);
    grow(&lingering, cw, h, &mut near_any);
    for (c, sure) in mask.sure.iter_mut().enumerate() {
        *sure = if seeds[c] {
            FOUND
        } else if near[c] {
            NEAR
        } else if lingering[c] {
            mask.life[c]
        } else {
            u8::from(near_any[c])
        };
    }
    mask
}

/// Expands detected mask seeds horizontally and vertically by `GROW_X` and `GROW_Y`
/// to catch soft shadows, rounded corners, and anti-aliased edges that differ
/// too little to be found directly.
fn grow(seeds: &[bool], cw: usize, h: usize, out: &mut [bool]) {
    let mut wide = vec![false; cw * h];
    for y in 0..h {
        let row = &seeds[y * cw..(y + 1) * cw];
        for cx in (0..cw).filter(|&cx| row[cx]) {
            wide[y * cw + cx.saturating_sub(GROW_X)..y * cw + (cx + GROW_X + 1).min(cw)].fill(true);
        }
    }
    // vertical growth with a running count over a window of rows
    for cx in 0..cw {
        let mut count = 0usize;
        for y in 0..h + GROW_Y {
            if y < h && wide[y * cw + cx] {
                count += 1;
            }
            if y > 2 * GROW_Y && wide[(y - 2 * GROW_Y - 1) * cw + cx] {
                count -= 1;
            }
            if y >= GROW_Y {
                out[(y - GROW_Y) * cw + cx] = count > 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(origin: i32, width: i32, height: i32) -> Vec<u32> {
        (origin..origin + height)
            .flat_map(|y| {
                (0..width).map(move |x| {
                    0xff000000
                        | ((y as u32).wrapping_mul(2_654_435_761) ^ (x as u32 * 977)) & 0xffffff
                })
            })
            .collect()
    }

    fn image(pixels: Vec<u32>) -> Image {
        Image::from_pixels(64, 120, pixels).unwrap()
    }

    #[test]
    fn opaque_and_translucent_floats_are_found_and_the_page_is_not() {
        let (before, mut after) = (page(0, 64, 120), page(20, 64, 120));
        // an opaque button and a translucent plaque over the moving page
        for y in 40..50 {
            for x in 8..24 {
                after[y * 64 + x] = 0xff3a6ea5;
            }
            for x in 40..56 {
                after[y * 64 + x] = (after[y * 64 + x] >> 1) & 0x7f7f7f | 0xff000000;
            }
        }
        let mut before = before;
        for y in 40..50 {
            for x in 8..24 {
                before[y * 64 + x] = 0xff3a6ea5;
            }
        }
        let mask = find(&image(before), &image(after), 20, None, (0, 0));
        assert!((mask.sure(45, 1) > 0) && (mask.sure(45, 2) > 0));
        assert!((mask.sure(45, 5) > 0) && (mask.sure(45, 6) > 0));
        // far from both, the page is the page
        assert!(!(mask.sure(80, 4) > 0));
        assert!(!(mask.sure(2, 4) > 0));
    }

    #[test]
    fn the_page_just_shown_under_a_flat_header_is_the_page() {
        // scrolled up by 30 under a header of 20 flat rows: the page right under it pairs
        // with the header of the last frame2
        let frame = |origin: i32| {
            let mut pixels = page(origin, 64, 120);
            pixels[..20 * 64].fill(0xff202020);
            image(pixels)
        };
        let mask = find(&frame(100), &frame(70), -30, None, (20, 0));
        // the header stays put over the page, and what is near it is covered as well
        for y in 20 + GROW_Y..120 {
            for cx in 0..8 {
                assert_eq!(mask.sure(y, cx), 0, "row {y}, cell {cx}");
            }
        }
    }
}
