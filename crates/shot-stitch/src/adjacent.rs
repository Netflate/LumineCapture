use crate::comparison::{Skip, Window, frame_diff};
use crate::profile::{Signature, Weights};
use crate::session::Stitcher;
use crate::types::NO_MATCH;

struct Search<'a> {
    previous: &'a Signature,
    current: &'a Signature,
    weights: &'a Weights,
    window: Window,
    skips: (Skip<'a>, Skip<'a>),
    /// rank of the best few offsets apart from each other
    top: Vec<(i32, f32)>,
}

/// Distinct offsets kept for a closer look.
const CANDIDATES: usize = 6;

impl Search<'_> {
    fn probe(&mut self, offset: i32) {
        let diff = frame_diff(
            self.previous,
            self.current,
            offset,
            self.window,
            self.weights,
            self.skips,
        );
        if diff >= NO_MATCH {
            return;
        }
        if let Some(near) = self.top.iter_mut().find(|t| (t.0 - offset).abs() <= 6) {
            if diff < near.1 {
                *near = (offset, diff);
            }
        } else if self.top.len() < CANDIDATES {
            self.top.push((offset, diff));
        } else if let Some(worst) = self.top.iter_mut().max_by(|a, b| a.1.total_cmp(&b.1))
            && diff < worst.1
        {
            *worst = (offset, diff);
        }
    }

    fn ranked(mut self) -> Vec<(i32, f32)> {
        self.top.sort_by(|a, b| a.1.total_cmp(&b.1));
        if self.top.is_empty() {
            self.top.push((0, NO_MATCH));
        }
        self.top
    }
}

fn offsets(maximum: i32, prediction: i32) -> impl Iterator<Item = i32> {
    let prediction = prediction.clamp(-maximum, maximum);
    std::iter::once(prediction).chain((1..=maximum * 2).flat_map(move |delta| {
        [prediction + delta, prediction - delta]
            .into_iter()
            .filter(move |&x| (-maximum..=maximum).contains(&x))
    }))
}

impl Stitcher {
    pub(crate) fn find_offsets(&self, cols: &Signature) -> Vec<(i32, f32)> {
        if self.last_cols.is_empty() || cols.is_empty() {
            return vec![(0, NO_MATCH)];
        }
        let maximum = (self.last_cols.len() as i32 - self.config.min_overlap).max(0);
        let mut search = Search {
            previous: &self.last_cols,
            current: cols,
            weights: &self.motion.weights,
            window: self.window(),
            skips: (Skip(&self.skip), Skip(&self.skip_before)),
            top: Vec::new(),
        };
        let predicted = self.last_offset.clamp(-maximum, maximum);
        for offset in offsets(maximum, predicted).take_while(|o| (o - predicted).abs() <= 128) {
            search.probe(offset);
        }
        for offset in (-maximum..=maximum).step_by(3) {
            if (offset - predicted).abs() > 128 {
                search.probe(offset);
            }
        }
        let coarse: Vec<i32> = search.top.iter().map(|t| t.0).collect();
        for offset in coarse {
            for candidate in (-maximum).max(offset - 2)..=maximum.min(offset + 2) {
                search.probe(candidate);
            }
        }
        search.ranked()
    }
}

#[cfg(test)]
mod tests {
    use super::offsets;

    #[test]
    fn prediction_covers_opposite_edge() {
        let mut values: Vec<_> = offsets(5, 4).collect();
        assert_eq!(values[0], 4);
        values.sort_unstable();
        assert_eq!(values, (-5..=5).collect::<Vec<_>>());
    }
}
