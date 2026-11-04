use crate::comparison::{Skip, frame_diff};
use crate::fixed_regions::Observation;
use crate::overlay::{self, Bands, Mask};
use crate::profile::{BAND_COUNT, Sampler, Signature, compute, exact_rows, moving_columns};
use crate::session::Stitcher;
use crate::types::{LineMatch, NO_MATCH, overhang};
use crate::{Axis, Edge, Status, StitchResult};
use shot_image::{Image, ImageError};

const MIN_CHECKED: usize = 32;
const MIN_EXACT_ROWS: i32 = 8;

#[derive(Clone, Copy)]
struct Placement {
    position: i32,
    offset: i32,
    confidence: f32,
    line: Option<LineMatch>,
}

impl Stitcher {
    pub fn push(&mut self, raw_frame: Image) -> Result<StitchResult, ImageError> {
        let fixed = self.fixed.clone();
        let motion = self.motion.clone();
        let result = self.push_inner(raw_frame);
        if result.is_err() {
            self.fixed = fixed;
            self.motion = motion;
        }
        result
    }

    fn recover(&self, frame: &Image, cols: &Signature, predicted: i32) -> Option<Placement> {
        let (position, confidence) = self.find_edge(cols, predicted);
        if confidence <= self.config.accept_diff {
            return Some(Placement {
                position,
                offset: position - self.anchor,
                confidence,
                line: None,
            });
        }
        let line = self.find_line_run(frame, predicted);
        if line.diff > self.config.accept_diff || line.rows < self.config.min_overlap {
            return None;
        }
        Some(Placement {
            position: line.position,
            offset: line.position - self.anchor,
            confidence: line.diff,
            line: Some(line),
        })
    }

    fn adopt(&mut self, frame: Image, cols: Signature, position: i32, offset: i32) -> StitchResult {
        let height = frame.height();
        let mask = self.floats(&frame, offset);
        self.take_in(&frame, position, mask.as_ref(), 0..0);
        self.long.ended(&frame, position, mask.as_ref());
        self.anchor = position;
        self.last_offset = offset;
        self.pending_edge = Edge::None;
        self.remember(frame, cols, mask);
        StitchResult {
            status: Status::NoProgress,
            position,
            frame_length: height,
            ..StitchResult::default()
        }
    }
    fn floats(&mut self, frame: &Image, offset: i32) -> Option<Mask> {
        let mut mask = match self.early.take() {
            Some((at, mask)) if at == offset => mask,
            _ => {
                let last = self.last_frame.as_ref()?;
                let (top, bottom) = if self.fixed.known() {
                    (self.fixed.top(), self.fixed.bottom())
                } else {
                    crate::fixed_regions::bars(last, frame)
                };
                let bars = (top.max(0) as usize, bottom.max(0) as usize);
                overlay::find(last, frame, offset, self.mask.as_ref(), bars)
            }
        };
        let height = frame.height().max(0) as usize;
        mask.cover_rows(0..self.fixed.top().max(0) as usize);
        mask.cover_rows(height.saturating_sub(self.fixed.bottom().max(0) as usize)..height);
        Some(mask)
    }
    fn agrees(
        &self,
        frame: &Image,
        top: i32,
        mask: Option<&Mask>,
        fresh: std::ops::Range<i32>,
    ) -> bool {
        if self.backed == Some(top - self.anchor) {
            return true;
        }
        let fit = self.long.fit(frame, top, self.mask.as_ref(), mask, fresh);
        if fit.checked >= MIN_CHECKED {
            // Require a clear pixel-level majority.
            fit.alike * 3 >= fit.checked * 2 && fit.alike * 2 >= fit.detailed
        } else {
            fit.detailed < MIN_CHECKED
        }
    }
    fn look_everywhere(&mut self, frame: Image, cols: Signature) -> Option<StitchResult> {
        if self.long.height() < frame.height() {
            return None;
        }
        self.screen_skip(&cols);
        self.long_skip.clone_from(&self.long.tainted);
        self.early = None;
        self.backed = None;
        let places = self.known_places(&cols);
        let position = places
            .into_iter()
            .map(|(position, _)| position)
            .find(|&position| self.fits_at(&frame, position))?;
        let offset = position - self.anchor;
        if let Some(last) = &self.last_frame {
            self.fixed.observe(last, &frame, offset);
        }
        Some(self.adopt(frame, cols, position, offset))
    }
    fn fits_at(&mut self, frame: &Image, position: i32) -> bool {
        let offset = position - self.anchor;
        let mask = self.floats(frame, offset);
        let fits = self.agrees(frame, position, mask.as_ref(), 0..0);
        self.early = mask.map(|mask| (offset, mask));
        fits
    }
    fn take_in(
        &mut self,
        frame: &Image,
        top: i32,
        mask: Option<&Mask>,
        fresh: std::ops::Range<i32>,
    ) {
        if let Some(mask) = mask {
            self.long
                .observe(frame, top, mask, &Sampler::new(frame.width()), fresh);
        }
    }
    fn pick_offset(&self, frame: &Image, cols: &Signature, columns: &[usize]) -> (i32, f32, i32) {
        let candidates = self.find_offsets(cols);
        let best = candidates[0];
        let Some(last) = self.last_frame.as_ref() else {
            return (best.0, best.1, 0);
        };
        if best.1 >= NO_MATCH {
            return (best.0, best.1, 0);
        }
        let bound = (best.1 * 3.0).max(self.config.accept_diff);
        candidates
            .iter()
            .filter(|candidate| candidate.1 <= bound)
            .map(|&candidate| (exact_rows(last, frame, candidate.0, columns), candidate))
            .max_by(|a, b| a.0.cmp(&b.0).then(b.1.1.total_cmp(&a.1.1)))
            .map_or((best.0, best.1, 0), |(rows, (offset, diff))| {
                (offset, diff, rows)
            })
    }
    fn leave_out_floats(
        &mut self,
        frame: &Image,
        cols: &Signature,
        offset: i32,
        confidence: f32,
    ) -> f32 {
        let Some(mask) = self.floats(frame, offset) else {
            return confidence;
        };
        if mask.share() > 0.5 {
            return confidence;
        }
        let bands = Bands::new(frame.width());
        let changing = mask.changing_band_rows(&bands);
        let rows: Vec<u8> = mask
            .band_rows(&bands)
            .iter()
            .zip(&changing)
            .map(|(floats, changing)| floats | changing)
            .collect();
        for (skip, bits) in self.skip.iter_mut().zip(&rows) {
            *skip |= bits;
        }
        for (y, bits) in rows.iter().enumerate() {
            let at = self.anchor + y as i32;
            if let Some(skip) = usize::try_from(at)
                .ok()
                .and_then(|at| self.long_skip.get_mut(at))
            {
                *skip |= bits;
            }
        }
        self.early = Some((offset, mask));
        if confidence <= self.config.accept_diff {
            return confidence;
        }
        let window = self.window();
        confidence.min(frame_diff(
            &self.last_cols,
            cols,
            offset,
            window,
            &self.motion.weights,
            (Skip(&self.skip), Skip(&self.skip_before)),
        ))
    }
    fn screen_skip(&mut self, cols: &Signature) {
        self.skip.clear();
        self.skip_before.clear();
        if self.last_cols.len() != cols.len() {
            return;
        }
        if let Some(mask) = self.mask.as_ref().filter(|mask| mask.rows == cols.len()) {
            self.skip_before = mask.changing_band_rows(&Bands::new(self.long.width()));
        }
        let floated = self
            .mask
            .as_ref()
            .filter(|mask| mask.rows == cols.len())
            .map(|mask| mask.band_rows(&Bands::new(self.long.width())));
        self.skip.extend(
            cols.iter()
                .zip(&self.last_cols)
                .enumerate()
                .map(|(y, (now, before))| {
                    let mut bits = floated.as_ref().map_or(0, |floated| floated[y]);
                    for g in 0..BAND_COUNT {
                        if (now[g] - before[g]).abs() < 0.5 {
                            bits |= 1 << g;
                        }
                    }
                    bits
                }),
        );
    }
    fn unmoved(&mut self, height: usize) {
        self.skip.clear();
        if let Some(mask) = self.mask.as_ref().filter(|mask| mask.rows == height) {
            self.skip = mask.band_rows(&Bands::new(self.long.width()));
        }
    }
    fn observe_fixed(&mut self, frame: &Image, offset: i32) -> Observation {
        self.last_frame
            .as_ref()
            .map(|last| self.fixed.observe(last, frame, offset))
            .unwrap_or_default()
    }
    fn trims(&self, observation: Observation, position: i32, height: i32, full: i32) -> (i32, i32) {
        let (amount, edge) = overhang(position, height, full);
        if amount <= 0 {
            return (0, 0);
        }
        if edge == Edge::End {
            let footer = if observation.valid {
                observation.footer
            } else {
                self.fixed.bottom()
            };
            let maximum = (full - 1)
                .min(height / 3)
                .min(full / 3)
                .min(full - position)
                .max(0);
            (0, footer.clamp(0, maximum))
        } else {
            let header = if observation.valid {
                observation.header
            } else {
                self.fixed.top()
            };
            (header.clamp(0, (height - amount).min(full - 1).max(0)), 0)
        }
    }

    fn score_edge(
        &self,
        cols: &Signature,
        placement: Placement,
        trim: (i32, i32),
    ) -> (i32, Edge, f32, i32) {
        let (amount, edge) = overhang(
            placement.position,
            cols.len() as i32,
            self.long.height() - trim.1,
        );
        if amount <= 0 {
            return (amount, edge, 0.0, 0);
        }
        if let Some(line) = placement.line.filter(|line| line.edge == edge) {
            return (amount, edge, placement.confidence, line.rows);
        }
        let excluded = if edge == Edge::Start {
            (trim.0, 0)
        } else {
            (0, trim.1)
        };
        let (diff, length) = self.known_diff(cols, placement.position, excluded);
        (amount, edge, diff, length)
    }

    fn push_inner(&mut self, raw: Image) -> Result<StitchResult, ImageError> {
        let frame = if self.axis == Axis::Horizontal {
            raw.transpose()?
        } else {
            raw
        };
        let cols = compute(&frame)?;
        let settled = cols == self.previous_cols;
        self.previous_cols.clone_from(&cols);
        let min_append = if settled { 1 } else { self.config.min_append };
        if self.long.height() == 0 {
            return self.initialize(frame, cols);
        }
        if frame.width() != self.long.width() {
            return Ok(StitchResult::default());
        }
        let height = frame.height();
        let full = self.long.height();
        // Ignore unchanged frames.
        if cols == self.last_cols {
            self.pending_edge = Edge::None;
            return Ok(StitchResult {
                status: Status::NoProgress,
                position: self.anchor,
                frame_length: height,
                ..StitchResult::default()
            });
        }
        self.motion.observe(&self.last_cols, &cols);
        self.screen_skip(&cols);
        self.long_skip.clone_from(&self.long.tainted);
        self.early = None;
        self.backed = None;
        let columns = self
            .last_frame
            .as_ref()
            .map_or_else(Vec::new, |last| moving_columns(last, &frame));
        let (mut offset, mut confidence, exact) = self.pick_offset(&frame, &cols, &columns);
        self.backed = (exact >= MIN_EXACT_ROWS).then_some(offset);
        if confidence < NO_MATCH {
            confidence = self.leave_out_floats(&frame, &cols, offset, confidence);
        }
        if confidence > self.config.accept_diff {
            let still = frame_diff(
                &self.last_cols,
                &cols,
                0,
                self.window(),
                &self.motion.weights,
                (Skip::default(), Skip::default()),
            );
            let still_exact = self
                .last_frame
                .as_ref()
                .map_or(0, |last| exact_rows(last, &frame, 0, &columns));
            let unmoved = offset == 0 || still_exact > exact;
            if still <= self.config.accept_diff && unmoved {
                (offset, confidence) = (0, still);
                self.backed = (still_exact >= MIN_EXACT_ROWS).then_some(0);
            }
        }
        if offset == 0 {
            self.unmoved(cols.len());
        }
        let predicted = self.anchor + offset;
        let mut placement = Placement {
            position: predicted,
            offset,
            confidence,
            line: None,
        };
        let near_edge = self.anchor <= self.config.min_append
            || self.anchor + height >= full - self.config.min_append;
        let mut observation = self.observe_fixed(&frame, offset);
        let disagrees = confidence <= self.config.accept_diff && full >= height && {
            let (top, bottom) = self.trims(observation, predicted, height, full);
            self.known_diff(&cols, predicted, (top, bottom)).0 > self.config.accept_diff
        };
        if confidence > self.config.accept_diff || disagrees {
            if let Some(recovered) = near_edge
                .then(|| self.recover(&frame, &cols, predicted))
                .flatten()
            {
                placement = recovered;
            } else {
                if full >= height {
                    let known = self.find_known(&cols, predicted);
                    if known.1 <= self.config.accept_diff && self.fits_at(&frame, known.0) {
                        let applied = known.0 - self.anchor;
                        if let Some(last) = &self.last_frame {
                            self.fixed.observe(last, &frame, applied);
                        }
                        return Ok(self.adopt(frame, cols, known.0, applied));
                    }
                }
                let Some(recovered) = self.recover(&frame, &cols, predicted) else {
                    return Ok(self.look_everywhere(frame, cols).unwrap_or_default());
                };
                placement = recovered;
            }
        }
        if placement.offset != offset {
            observation = self.observe_fixed(&frame, placement.offset);
        }
        let (mut top_trim, mut bottom_trim) = match placement.line {
            Some(line) if line.edge == Edge::Start => (line.trim, 0),
            Some(line) => (0, line.trim),
            None => self.trims(observation, placement.position, height, full),
        };
        let (mut amount, mut edge, mut edge_diff, mut overlap) =
            self.score_edge(&cols, placement, (top_trim, bottom_trim));
        let switching =
            self.growth_edge != Edge::None && edge != Edge::None && edge != self.growth_edge;
        let boundary = (edge == Edge::Start
            && self.anchor <= self.config.min_append
            && placement.position <= 0)
            || (edge == Edge::End
                && self.anchor + height >= full - self.config.min_append
                && placement.position + height >= full - bottom_trim);
        let trusted_switch = switching
            && boundary
            && placement.confidence <= self.config.accept_diff
            && placement.offset.abs() >= self.config.min_append
            && overlap >= self.config.min_overlap;
        if amount > 0
            && full >= height
            && !trusted_switch
            && (edge_diff > self.config.accept_diff || overlap < self.config.min_overlap)
        {
            let known = self.find_known(&cols, predicted);
            if known.1 <= self.config.accept_diff && self.fits_at(&frame, known.0) {
                placement.position = known.0;
                placement.offset = known.0 - self.anchor;
                placement.line = None;
                top_trim = 0;
                bottom_trim = 0;
                (amount, edge) = overhang(placement.position, height, full);
                edge_diff = 0.0;
                overlap = height;
            }
        }
        let delta = if edge == Edge::End {
            amount - bottom_trim
        } else {
            amount
        };
        // Reject unstable growth before appending.
        if delta >= min_append {
            if self.pending_edge != Edge::None && edge != Edge::None && edge != self.pending_edge {
                self.pending_edge = Edge::None;
                return Ok(StitchResult::default());
            }
            let changing =
                self.growth_edge != Edge::None && edge != Edge::None && edge != self.growth_edge;
            let edge_valid =
                edge_diff <= self.config.accept_diff && overlap >= self.config.min_overlap;
            let at_end = if edge == Edge::Start {
                placement.position <= 0
            } else {
                placement.position + height >= full - bottom_trim
            };
            if (changing && !trusted_switch && (!edge_valid || !at_end))
                || (!edge_valid && !trusted_switch)
            {
                return Ok(self.look_everywhere(frame, cols).unwrap_or_default());
            }
            let mask = self.floats(&frame, placement.offset);
            // Verify the overlap before committing new rows.
            if !self.agrees(&frame, placement.position, mask.as_ref(), 0..0) {
                return Ok(self.look_everywhere(frame, cols).unwrap_or_default());
            }
            let (top, fresh) = if edge == Edge::End {
                self.long
                    .append(&frame, &cols, amount, bottom_trim, mask.as_ref())?;
                self.anchor = placement.position;
                self.best_bottom_trim = self.best_bottom_trim.max(bottom_trim);
                let end = self.long.height();
                (end - height, end - amount.clamp(0, height)..end)
            } else {
                top_trim = top_trim.clamp(0, (height - amount).min(full - 1).max(0));
                self.long
                    .prepend(&frame, &cols, amount + top_trim, top_trim, mask.as_ref())?;
                self.anchor = placement.position + amount;
                self.best_top_trim = self.best_top_trim.max(top_trim);
                (0, 0..(amount + top_trim).clamp(0, height))
            };
            self.take_in(&frame, top, mask.as_ref(), fresh);
            self.long.ended(&frame, top, mask.as_ref());
            self.axis_locked = true;
            self.pending_edge = Edge::None;
            if edge != Edge::None {
                self.growth_edge = edge;
            }
            self.remember(frame, cols, mask);
            self.last_offset = placement.offset;
            self.stats.frame_count += 1;
            self.stats.total_height = self.long.height();
            self.stats.last_append = delta;
            return Ok(StitchResult {
                status: Status::Appended,
                added: delta,
                edge,
                position: self.anchor,
                frame_length: height,
            });
        }
        if amount == 0 {
            if !self.fits_at(&frame, placement.position) {
                return Ok(self.look_everywhere(frame, cols).unwrap_or_default());
            }
            return Ok(self.adopt(frame, cols, placement.position, placement.offset));
        }
        self.pending_edge = edge;
        Ok(StitchResult {
            status: Status::NoProgress,
            edge,
            position: placement.position,
            frame_length: height,
            ..StitchResult::default()
        })
    }
}
