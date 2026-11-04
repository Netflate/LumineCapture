use crate::overlay::{Bands, Mask, NEAR, cells, detailed as detailed_cell, same, span};
use crate::profile::{Sampler, Signature};
use shot_image::buffer::allocate;
use shot_image::{Image, ImageError, ImageView, MAX_PIXELS};

/// A cell confirmed by `VOTES` distinct screen rows.
const VERIFIED: u16 = u16::MAX;
/// Marks a cell copied from under a float; the count stores mask certainty.
const FLOATED: u16 = 1 << 15;
/// Stores the net vote count for an unverified, uncovered cell.
const COUNT: u16 = 0b1111 << SHIFT;
const SHIFT: u16 = 11;
const VOTES: u16 = 3;
/// Screen row last observed; repeated views from the same row do not vote.
const ROW: u16 = (1 << SHIFT) - 1;

/// State of an uncovered cell copied from row `y`.
fn state(y: usize) -> u16 {
    covered_state(y, 0)
}

/// State of a cell copied from row `y` under a float.
fn covered_state(y: usize, sure: u8) -> u16 {
    let row = (y & ROW as usize) as u16;
    if sure == 0 {
        row | 1 << SHIFT
    } else {
        // Keep this distinct from `VERIFIED`.
        row | u16::from(sure.min(14)) << SHIFT | FLOATED
    }
}

fn votes(state: u16) -> u16 {
    (state & COUNT) >> SHIFT
}

fn seen(state: u16) -> u16 {
    state & ROW
}

/// Whether the end frame should supply this cell instead of the stitched image.
fn from_end(state: u16, sure: Option<u8>) -> bool {
    if state == VERIFIED {
        false
    } else if state & FLOATED != 0 {
        true
    } else {
        votes(state) < 2 && sure.is_some_and(|sure| sure < NEAR)
    }
}

/// Frame and optional coverage certainty stored at one image end.
#[derive(Clone)]
struct End {
    top: i32,
    frame: Image,
    sure: Option<Vec<u8>>,
}

/// Cell counts returned by `LongImage::fit`.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Fit {
    pub detailed: usize,
    pub checked: usize,
    pub alike: usize,
}

#[derive(Default)]
pub(crate) struct LongImage {
    pixels: Vec<u32>,
    width: i32,
    top: usize,
    used: usize,
    pub cols: Signature,
    /// State of each cell (`VERIFIED` / `FLOATED`).
    states: Vec<u16>,
    /// Signature bands tainted by cells copied from under floats.
    pub tainted: Vec<u8>,
    cells: usize,
    bands: Option<Bands>,
    /// Latest frames at each end, used to restore content obscured by floating UI.
    ends: [Option<End>; 2],
    /// End frame to draw last when they overlap.
    later: usize,
    pub revision: u64,
}

impl LongImage {
    pub fn reset(&mut self, seed: &Image, cols: Signature) -> Result<(), ImageError> {
        let mut pixels = allocate(seed.pixels().len())?;
        pixels.copy_from_slice(seed.pixels());
        let cells = cells(seed.width());
        let rows = seed.height() as usize;
        let mut states = Vec::new();
        states
            .try_reserve_exact(rows * cells)
            .map_err(|_| ImageError::Allocation)?;
        for y in 0..rows {
            states.extend(std::iter::repeat_n(state(y), cells));
        }
        *self = Self {
            pixels,
            width: seed.width(),
            used: rows,
            cols,
            states,
            tainted: vec![0; rows],
            cells,
            bands: Some(Bands::new(seed.width())),
            ends: [0, 1].map(|_| {
                Some(End {
                    top: 0,
                    frame: seed.clone(),
                    sure: None,
                })
            }),
            later: 1,
            revision: self.revision.wrapping_add(1),
            ..Self::default()
        };
        Ok(())
    }

    /// Records `frame` when it reaches either end of the stitched image.
    pub fn ended(&mut self, frame: &Image, top: i32, mask: Option<&Mask>) {
        if frame.width() != self.width {
            return;
        }
        let rows = frame.height().max(0) as usize;
        let sure = mask
            .filter(|mask| mask.cells == self.cells && mask.rows == rows)
            .map(|mask| mask.sure_cells().to_vec());
        let (first, last) = (top <= 0, top + frame.height() >= self.used as i32);
        if !first && !last {
            return;
        }
        for (side, at) in [first, last].into_iter().enumerate() {
            if at {
                self.ends[side] = Some(End {
                    top,
                    frame: frame.clone(),
                    sure: sure.clone(),
                });
                self.later = side;
            }
        }
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.used as i32
    }

    pub fn row(&self, y: i32) -> &[u32] {
        assert!(y >= 0 && (y as usize) < self.used);
        let start = (self.top + y as usize) * self.width as usize;
        &self.pixels[start..start + self.width as usize]
    }

    fn row_mut(&mut self, y: usize) -> &mut [u32] {
        let width = self.width as usize;
        let start = (self.top + y) * width;
        &mut self.pixels[start..start + width]
    }

    /// Borrows the stitched pixels.
    pub fn view(&self) -> Option<ImageView<'_>> {
        let width = self.width as usize;
        let start = self.top * width;
        ImageView::new(
            &self.pixels[start..start + self.used * width],
            self.width,
            self.height(),
            width,
        )
        .ok()
    }

    /// Builds the stitched image, restoring content from the end frames.
    pub fn image(&self) -> Result<Image, ImageError> {
        let width = self.width as usize;
        let mut data = Vec::new();
        data.try_reserve_exact(self.used * width)
            .map_err(|_| ImageError::Allocation)?;
        let start = self.top * width;
        data.extend_from_slice(&self.pixels[start..start + self.used * width]);
        let later = self.later.min(1);
        for end in [&self.ends[1 - later], &self.ends[later]]
            .into_iter()
            .flatten()
        {
            let rows = end.top.max(0)..(end.top + end.frame.height()).min(self.used as i32);
            for l in rows {
                let y = (l - end.top) as usize;
                let (now, l) = (end.frame.row(y as i32), l as usize);
                let row = &mut data[l * width..(l + 1) * width];
                for cx in 0..self.cells {
                    let sure = end.sure.as_ref().map(|sure| sure[y * self.cells + cx]);
                    if from_end(self.states[l * self.cells + cx], sure) {
                        let range = span(cx, width);
                        row[range.clone()].copy_from_slice(&now[range]);
                    }
                }
            }
        }
        Image::from_pixels(self.width, self.height(), data)
    }

    /// Reserves space; bottom growth avoids copying the existing image.
    fn ensure_room(&mut self, grow_top: usize, grow_bottom: usize) -> Result<(), ImageError> {
        let width = self.width as usize;
        let target = self
            .used
            .checked_add(grow_top)
            .and_then(|n| n.checked_add(grow_bottom))
            .ok_or(ImageError::Dimensions)?;
        let limit = MAX_PIXELS / (width + crate::profile::BAND_COUNT);
        if target > limit {
            return Err(ImageError::Dimensions);
        }
        if self.top < grow_top {
            let reserve = grow_top + (target / 8).max(256);
            let mut grown = Vec::new();
            grown
                .try_reserve_exact((reserve + self.used + grow_bottom) * width)
                .map_err(|_| ImageError::Allocation)?;
            grown.resize(reserve * width, 0);
            let start = self.top * width;
            grown.extend_from_slice(&self.pixels[start..start + self.used * width]);
            self.pixels = grown;
            self.top = reserve;
        }
        let needed = self.top + self.used + grow_bottom;
        if needed * width > self.pixels.capacity() {
            let rows = (needed + needed / 2)
                .max(needed + 256)
                .min(self.top + limit);
            self.pixels
                .try_reserve_exact(rows * width - self.pixels.len())
                .map_err(|_| ImageError::Allocation)?;
        }
        Ok(())
    }

    /// Returns cell states and signature taint for frame row `y`.
    fn fresh(&self, mask: Option<&Mask>, y: usize) -> (impl Iterator<Item = u16>, u8) {
        let covered = mask.filter(|mask| y < mask.rows && mask.cells == self.cells);
        let tainted = match (covered, &self.bands) {
            (Some(mask), Some(bands)) => bands.of(mask.row(y)),
            _ => 0,
        };
        let states =
            (0..self.cells).map(move |cx| covered_state(y, covered.map_or(0, |m| m.sure(y, cx))));
        (states, tainted)
    }

    pub fn append(
        &mut self,
        frame: &Image,
        cols: &Signature,
        rows: i32,
        trim: i32,
        mask: Option<&Mask>,
    ) -> Result<(), ImageError> {
        let rows = rows.clamp(0, frame.height().min(cols.len() as i32)) as usize;
        let trim = trim.clamp(0, (self.used as i32 - 1).max(0)) as usize;
        self.cols
            .try_reserve(rows)
            .map_err(|_| ImageError::Allocation)?;
        self.states
            .try_reserve(rows * self.cells)
            .map_err(|_| ImageError::Allocation)?;
        self.tainted
            .try_reserve(rows)
            .map_err(|_| ImageError::Allocation)?;
        self.ensure_room(0, rows.saturating_sub(trim))?;
        self.used -= trim;
        self.cols.truncate(self.used);
        self.states.truncate(self.used * self.cells);
        self.tainted.truncate(self.used);
        self.pixels
            .truncate((self.top + self.used) * self.width as usize);
        for i in 0..rows {
            let y = frame.height() as usize - rows + i;
            self.pixels.extend_from_slice(frame.row(y as i32));
            self.cols.push(cols[y]);
            let (states, tainted) = self.fresh(mask, y);
            let states: Vec<u16> = states.collect();
            self.states.extend(states);
            self.tainted.push(tainted);
        }
        self.used += rows;
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    pub fn prepend(
        &mut self,
        frame: &Image,
        cols: &Signature,
        rows: i32,
        trim: i32,
        mask: Option<&Mask>,
    ) -> Result<(), ImageError> {
        let rows = rows.clamp(0, frame.height().min(cols.len() as i32)) as usize;
        let trim = trim.clamp(0, (self.used as i32 - 1).max(0)) as usize;
        let kept = self.used - trim;
        let mut merged = Vec::new();
        merged
            .try_reserve_exact(rows + kept)
            .map_err(|_| ImageError::Allocation)?;
        merged.extend_from_slice(&cols[..rows]);
        merged.extend_from_slice(&self.cols[trim..]);
        let mut states = Vec::new();
        states
            .try_reserve_exact((rows + kept) * self.cells)
            .map_err(|_| ImageError::Allocation)?;
        let mut tainted = Vec::new();
        tainted
            .try_reserve_exact(rows + kept)
            .map_err(|_| ImageError::Allocation)?;
        for y in 0..rows {
            let (row, taint) = self.fresh(mask, y);
            states.extend(row);
            tainted.push(taint);
        }
        states.extend_from_slice(&self.states[trim * self.cells..]);
        tainted.extend_from_slice(&self.tainted[trim..]);
        self.ensure_room(rows.saturating_sub(trim), 0)?;
        self.top += trim;
        self.used -= trim;
        let width = self.width as usize;
        for i in 0..rows {
            let start = (self.top - rows + i) * width;
            self.pixels[start..start + width].copy_from_slice(frame.row(i as i32));
        }
        self.top -= rows;
        self.used += rows;
        for end in self.ends.iter_mut().flatten() {
            end.top += (rows - trim) as i32;
        }
        self.cols = merged;
        self.states = states;
        self.tainted = tainted;
        self.revision = self.revision.wrapping_add(1);
        Ok(())
    }

    /// Compares detailed cells against stored rows, skipping known or newly copied regions.
    pub fn fit(
        &self,
        frame: &Image,
        top: i32,
        known: Option<&Mask>,
        here: Option<&Mask>,
        fresh: std::ops::Range<i32>,
    ) -> Fit {
        let cw = self.cells;
        let width = self.width as usize;
        let rows = frame.height() as usize;
        let fits = |mask: &&Mask| mask.cells == cw && mask.rows == rows;
        let (known, here) = (known.filter(fits), here.filter(fits));
        let mut fit = Fit::default();
        if frame.width() != self.width {
            return fit;
        }
        for y in (0..frame.height()).step_by(2) {
            let l = top + y;
            if l < 0 || l >= self.used as i32 || fresh.contains(&l) {
                continue;
            }
            let (stored, now) = (self.row(l), frame.row(y));
            let y = y as usize;
            for cx in 0..cw {
                let range = span(cx, width);
                let cell = &now[range.clone()];
                let s = self.states[l as usize * cw + cx];
                if !detailed_cell(cell)
                    || known.is_some_and(|mask| mask.sure(y, cx) > 0)
                    || (s != VERIFIED && s & FLOATED != 0)
                {
                    continue;
                }
                fit.detailed += 1;
                if here.is_some_and(|mask| mask.sure(y, cx) > 0 || mask.changing(y, cx)) {
                    continue;
                }
                fit.checked += 1;
                fit.alike += usize::from(same(&stored[range], cell));
            }
        }
        fit
    }

    /// Updates stored cells from a frame, using uncovered views to confirm or replace them.
    pub fn observe(
        &mut self,
        frame: &Image,
        top: i32,
        mask: &Mask,
        sampler: &Sampler,
        fresh: std::ops::Range<i32>,
    ) {
        let cw = self.cells;
        if frame.width() != self.width || mask.cells != cw || mask.rows != frame.height() as usize {
            return;
        }
        let width = self.width as usize;
        let mut changed = false;
        let mut touched_rows = Vec::new();
        for l in top.max(0)..(top + frame.height()).min(self.used as i32) {
            if fresh.contains(&l) {
                continue;
            }
            let (y, l) = ((l - top) as usize, l as usize);
            let covered: Vec<bool> = (0..cw).map(mask.row(y)).collect();
            let now = frame.row(y as i32);
            let mut written = false;
            let mut retagged = false;
            for (cx, &covered) in covered.iter().enumerate() {
                let s = self.states[l * cw + cx];
                if s == VERIFIED {
                    continue;
                }
                let floated = s & FLOATED != 0;
                let range = span(cx, width);
                let start = (self.top + l) * width;
                let stored = &self.pixels[start + range.start..start + range.end];
                let view = &now[range.clone()];
                if covered {
                    // Prefer the less-covered view.
                    let sure = mask.sure(y, cx);
                    if floated && u16::from(sure) < votes(s) {
                        if stored != view {
                            self.row_mut(l)[range].copy_from_slice(view);
                            written = true;
                        }
                        self.states[l * cw + cx] = covered_state(y, sure);
                    }
                    continue;
                }
                let elsewhere = seen(s) != seen(state(y));
                let agrees = same(stored, view);
                let next = if floated {
                    // Uncovered views replace copies from under floats.
                    if stored != view {
                        self.row_mut(l)[range].copy_from_slice(view);
                        written = true;
                    }
                    state(y)
                } else if agrees && !elsewhere {
                    continue;
                } else if agrees {
                    if votes(s) + 1 >= VOTES {
                        // Prefer the clean view to a faint shadow edge.
                        if stored != view {
                            self.row_mut(l)[range].copy_from_slice(view);
                            written = true;
                        }
                        VERIFIED
                    } else {
                        seen(state(y)) | (votes(s) + 1) << SHIFT
                    }
                } else if votes(s) > 1 {
                    s & !COUNT | (votes(s) - 1) << SHIFT
                } else {
                    self.row_mut(l)[range].copy_from_slice(view);
                    written = true;
                    state(y)
                };
                retagged |= (next & FLOATED) != (s & FLOATED);
                self.states[l * cw + cx] = next;
            }
            if written {
                self.cols[l] = sampler.profile(self.row(l as i32));
                changed = true;
            }
            if written || retagged {
                touched_rows.push(l);
            }
        }
        if let Some(bands) = &self.bands {
            for l in touched_rows {
                let states = &self.states[l * cw..(l + 1) * cw];
                self.tainted[l] =
                    bands.of(|cx| states[cx] != VERIFIED && states[cx] & FLOATED != 0);
            }
        }
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
    }
}
