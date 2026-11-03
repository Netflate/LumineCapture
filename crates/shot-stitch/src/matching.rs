use crate::profile::Signature;
use crate::session::Stitcher;
use crate::types::{Edge, NO_MATCH, overhang};

/// Steps of the coarse search looked at closer
const REFINED: usize = 6;

struct Candidates {
    best: (i32, f32),
    good: (i32, f32),
    distance: i32,
}

impl Candidates {
    fn visit(&mut self, pos: i32, diff: f32, prediction: i32, accept: f32) {
        if diff < self.best.1 {
            self.best = (pos, diff);
        }
        let distance = (pos - prediction).abs();
        if diff <= accept
            && (distance < self.distance || (distance == self.distance && diff < self.good.1))
        {
            self.good = (pos, diff);
            self.distance = distance;
        }
    }
}

impl Stitcher {
    pub(crate) fn find_known(&self, cols: &Signature, prediction: i32) -> (i32, f32) {
        let maximum = self.long.height() - cols.len() as i32;
        if maximum < 0 || cols.is_empty() {
            return (0, NO_MATCH);
        }
        let inside = prediction.clamp(0, maximum);
        let diff = self.known_diff(cols, inside, (0, 0)).0;
        if diff <= self.config.accept_diff {
            return (inside, diff);
        }
        let mut candidates = Candidates {
            best: (inside, NO_MATCH),
            good: (inside, NO_MATCH),
            distance: i32::MAX,
        };
        let mut coarse = Vec::new();
        for pos in [prediction, self.anchor, 0, maximum]
            .into_iter()
            .chain((0..=maximum).step_by(8))
            .chain(std::iter::once(maximum))
        {
            let pos = pos.clamp(0, maximum);
            let diff = self.known_diff(cols, pos, (0, 0)).0;
            candidates.visit(pos, diff, inside, self.config.accept_diff);
            coarse.push((pos, diff));
        }
        coarse.sort_by(|a, b| a.1.total_cmp(&b.1));
        let mut around: Vec<i32> = Vec::new();
        for (pos, _) in coarse {
            if around.len() == REFINED {
                break;
            }
            if around.iter().all(|&a| (a - pos).abs() > 8) {
                around.push(pos);
            }
        }
        for at in around {
            for pos in (at - 8).max(0)..=(at + 8).min(maximum) {
                candidates.visit(
                    pos,
                    self.known_diff(cols, pos, (0, 0)).0,
                    inside,
                    self.config.accept_diff,
                );
            }
        }
        let chosen = if candidates.good.1 <= self.config.accept_diff {
            candidates.good
        } else {
            candidates.best
        };
        // the place nearest to the prediction that fits, but where it fits best: the rows
        // next to it fit almost as well on a smooth page
        let mut bottom = chosen;
        for pos in (chosen.0 - 8).max(0)..=(chosen.0 + 8).min(maximum) {
            let diff = self.known_diff(cols, pos, (0, 0)).0;
            if diff < bottom.1 {
                bottom = (pos, diff);
            }
        }
        bottom
    }

    fn visit_edge(&self, cols: &Signature, pos: i32, best: &mut (i32, f32)) {
        let height = cols.len() as i32;
        let (amount, edge) = overhang(pos, height, self.long.height());
        if amount <= 0 || !self.reachable(pos, height) {
            return;
        }
        let excluded = if edge == Edge::Start {
            (self.fixed.top(), 0)
        } else {
            (0, self.fixed.bottom())
        };
        let (diff, length) = self.known_diff(cols, pos, excluded);
        if length >= self.config.min_overlap && diff < best.1 {
            *best = (pos, diff);
        }
    }

    pub(crate) fn find_edge(&self, cols: &Signature, prediction: i32) -> (i32, f32) {
        let (full, height) = (self.long.height(), cols.len() as i32);
        if full <= 0 || height <= 0 {
            return (0, NO_MATCH);
        }
        let minimum = self.config.min_overlap - height;
        let maximum = full - self.config.min_overlap;
        if minimum > maximum {
            return (0, NO_MATCH);
        }
        let mut best = (prediction.clamp(minimum, maximum), NO_MATCH);
        for pos in [prediction, self.anchor, self.anchor + self.last_offset] {
            self.visit_edge(cols, pos.clamp(minimum, maximum), &mut best);
        }
        let ranges = [
            (minimum.max(full - height + 1), maximum),
            (minimum, maximum.min(-1)),
            (
                (prediction - 160).clamp(minimum, maximum),
                (prediction + 160).clamp(minimum, maximum),
            ),
        ];
        for (start, end) in ranges {
            if start > end {
                continue;
            }
            for pos in (start..=end).step_by(8).chain(std::iter::once(end)) {
                self.visit_edge(cols, pos, &mut best);
            }
        }
        if best.1 < NO_MATCH {
            for pos in minimum.max(best.0 - 8)..=maximum.min(best.0 + 8) {
                self.visit_edge(cols, pos, &mut best);
            }
        }
        best
    }
}
