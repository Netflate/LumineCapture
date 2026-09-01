// Selection over recognized text.
//
// The selection is represented as two points: start and end (line + character position).
// Text between them includes full middle lines and partial outer lines.
// 
// Uses `nearest_line` for click/hover detection so mouse clicks and hover effects 
// always target the exact same line.

use tiny_skia::Rect;

use super::layout::{self, Block};
use super::OcrLine;

/// Vertical slack before a drag is allowed to leave its anchor block.
const VERTICAL_SLACK: f32 = 10.0;

/// Pointer distance at which a line still counts as hit.
const HIT_SLACK: f32 = 3.0;

/// Character offset `ch` (0..=char count) within `line`, ordered by reading
/// position then offset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Caret {
    line: usize,
    ch: usize,
}

/// Block chrome for the renderer.
pub struct BlockOverlay {
    pub bounds: Rect,
    pub hovered: bool,
    /// Only multi-line blocks get a box; a lone line gets an underline.
    pub multiline: bool,
}

/// A line's selected span, for the renderer.
pub struct LineSelection {
    /// Global x of span's left and right edges.
    pub x: (f32, f32),
    /// Vertical extent (line's own top/bottom).
    pub y: (f32, f32),
}

/// Recognized lines, their blocks, and the selection over them. Lives in
/// `EditorState` while the OCR tool holds a result; cleared when the tool is
/// left or a new recognition starts. All coordinates are global.
#[derive(Default)]
pub struct OcrView {
    pub lines: Vec<OcrLine>,
    blocks: Vec<Block>,
    /// line index -> block index
    block_of: Vec<usize>,
    /// reading position -> line index
    order: Vec<usize>,
    /// line index -> reading position
    rank: Vec<usize>,
    /// (anchor, focus), not normalized; `ordered()` sorts them.
    sel: Option<(Caret, Caret)>,
    /// Block the current drag began in (confines a sideways drag).
    anchor_block: Option<usize>,
    /// Line under the cursor.
    hovered: Option<usize>,
}

impl OcrView {
    /// Group lines into blocks and work out the reading order.
    pub fn set_lines(&mut self, lines: Vec<OcrLine>) {
        let blocks = layout::group_blocks(&lines);
        let n = lines.len();

        let mut block_of = vec![0usize; n];
        let mut order = Vec::with_capacity(n);
        for (bi, block) in blocks.iter().enumerate() {
            for &li in &block.lines {
                block_of[li] = bi;
                order.push(li);
            }
        }
        let mut rank = vec![0usize; n];
        for (pos, &li) in order.iter().enumerate() {
            rank[li] = pos;
        }

        self.lines = lines;
        self.blocks = blocks;
        self.block_of = block_of;
        self.order = order;
        self.rank = rank;
        self.sel = None;
        self.anchor_block = None;
        self.hovered = None;
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn is_active(&self) -> bool {
        !self.lines.is_empty()
    }

    // ── hit testing ──────────────────────────────────────────────────────────

    /// Line under a global point, if close enough to count as hit.
    pub fn line_at(&self, p: (f64, f64)) -> Option<usize> {
        let (line, distance) = self.nearest_line(p.0 as f32, p.1 as f32, false)?;
        (distance <= HIT_SLACK).then_some(line)
    }

    /// Finds the nearest line and its distance (0 if inside).
    ///
    /// If `confine` is set, skips lines outside the anchor block while the mouse 
    /// is vertically aligned with it.
    ///
    /// In case of a tie, picks the line closer to the anchor in reading order.
    fn nearest_line(&self, x: f32, y: f32, confine: bool) -> Option<(usize, f32)> {
        let level_with_block = confine
            && self
                .anchor_block
                .and_then(|bi| self.blocks.get(bi))
                .is_some_and(|b| {
                    y >= b.bounds.top() - VERTICAL_SLACK && y <= b.bounds.bottom() + VERTICAL_SLACK
                });

        let anchor_rank = self.sel.map(|(a, _)| self.rank[a.line]).unwrap_or(0);
        let mut best = None;
        let mut best_key = (f32::MAX, usize::MAX);
        for (i, line) in self.lines.iter().enumerate() {
            if level_with_block && Some(self.block_of[i]) != self.anchor_block {
                continue;
            }
            let key = (
                layout::point_rect_dist(x, y, &line.bounds),
                self.rank[i].abs_diff(anchor_rank),
            );
            if key < best_key {
                best_key = key;
                best = Some(i);
            }
        }
        best.map(|i| (i, best_key.0))
    }

    /// Nearest line, then nearest character boundary within it.
    fn caret_at(&self, p: (f64, f64), confine: bool) -> Option<Caret> {
        let (x, y) = (p.0 as f32, p.1 as f32);
        let (line, _) = self.nearest_line(x, y, confine)?;
        Some(Caret {
            line,
            ch: nearest_boundary(&self.lines[line].char_x, x),
        })
    }

    // ── what the renderer draws ──────────────────────────────────────────────

    /// Block chrome, in block order.
    pub fn block_overlays(&self) -> impl Iterator<Item = BlockOverlay> + '_ {
        let hovered_block = self.hovered.map(|l| self.block_of[l]);
        self.blocks.iter().enumerate().map(move |(i, b)| BlockOverlay {
            bounds: b.bounds,
            hovered: Some(i) == hovered_block,
            multiline: b.lines.len() > 1,
        })
    }

    /// Whether line `i` sits in a boxed (multi-line) block.
    pub fn line_boxed(&self, i: usize) -> bool {
        self.block_of
            .get(i)
            .and_then(|&bi| self.blocks.get(bi))
            .is_some_and(|b| b.lines.len() > 1)
    }

    pub fn is_hovered(&self, i: usize) -> bool {
        self.hovered == Some(i)
    }

    /// Selected span on line `i`, or `None` if untouched. Middle lines give
    /// their whole width, the first and last a partial span.
    pub fn line_selection(&self, i: usize) -> Option<LineSelection> {
        let (lo, hi) = self.span(self.sel, i)?;
        let line = &self.lines[i];
        Some(LineSelection {
            x: (
                line.char_x.get(lo).copied().unwrap_or(line.bounds.left()),
                line.char_x.get(hi).copied().unwrap_or(line.bounds.right()),
            ),
            y: (line.bounds.top(), line.bounds.bottom()),
        })
    }

    /// Bounding box of everything recognized. Interaction damages this whole
    /// area so the region is re-blitted from the dimmed layer before the
    /// overlay is redrawn and the translucent fills can't stack up.
    pub fn bounds(&self) -> Option<Rect> {
        self.blocks
            .iter()
            .fold(None, |acc, b| layout::union_rect(acc, b.bounds))
    }

    // ── changing the selection ───────────────────────────────────────────────

    /// Move the hover highlight. Returns whether anything changed.
    pub fn set_hovered(&mut self, line: Option<usize>) -> bool {
        if self.hovered == line {
            return false;
        }
        self.hovered = line;
        true
    }

    /// Start a drag selection at the caret nearest the pointer.
    pub fn begin_drag(&mut self, pointer: (f64, f64)) {
        let Some(caret) = self.caret_at(pointer, false) else {
            return;
        };
        self.anchor_block = self.block_of.get(caret.line).copied();
        self.sel = Some((caret, caret));
    }

    /// Expands the selection to the nearest line position under the mouse.
    ///
    /// While dragging horizontally inside the current block, only that block's lines 
    /// can be selected (preventing accidental column grabs).
    ///
    /// Once the mouse moves past the top or bottom of the block, selection flows 
    /// normally in reading order.
    pub fn extend_drag(&mut self, pointer: (f64, f64)) -> bool {
        let Some((anchor, _)) = self.sel else {
            return false;
        };
        let Some(focus) = self.caret_at(pointer, true) else {
            return false;
        };
        let next = Some((anchor, focus));
        if self.sel == next {
            return false;
        }
        self.sel = next;
        true
    }

    /// Select the whole block a line belongs to (double-click).
    pub fn select_block(&mut self, line: usize) -> bool {
        let Some(&bi) = self.block_of.get(line) else {
            return false;
        };
        let block = &self.blocks[bi];
        let (Some(&first), Some(&last)) = (block.lines.first(), block.lines.last()) else {
            return false;
        };
        self.anchor_block = Some(bi);
        self.set_span(first, last)
    }

    pub fn select_all(&mut self) -> bool {
        self.anchor_block = None;
        let (Some(&first), Some(&last)) = (self.order.first(), self.order.last()) else {
            return false;
        };
        self.set_span(first, last)
    }

    pub fn deselect(&mut self) -> bool {
        self.anchor_block = None;
        let changed = self.sel.is_some();
        self.sel = None;
        changed
    }

    /// Select from the start of `first` to the end of `last`.
    fn set_span(&mut self, first: usize, last: usize) -> bool {
        let next = Some((
            Caret {
                line: first,
                ch: 0,
            },
            Caret {
                line: last,
                ch: self.lines[last].char_count(),
            },
        ));
        let changed = self.sel != next;
        self.sel = next;
        changed
    }

    // ── reading the selection out ────────────────────────────────────────────

    /// Selected text in reading order, blocks separated by a blank line.
    /// With nothing selected, every line in full.
    pub fn text_to_copy(&self) -> String {
        let sel = self.sel.filter(|&(a, b)| a != b);
        let mut out = String::new();
        let mut prev_block: Option<usize> = None;
        for &li in &self.order {
            let segment: String = if sel.is_none() {
                self.lines[li].text.clone()
            } else {
                let Some((lo, hi)) = self.span(sel, li) else {
                    continue;
                };
                self.lines[li].text.chars().skip(lo).take(hi - lo).collect()
            };
            let block = self.block_of[li];
            match prev_block {
                Some(pb) if pb == block => out.push('\n'),
                Some(_) => out.push_str("\n\n"),
                None => {}
            }
            out.push_str(&segment);
            prev_block = Some(block);
        }
        out
    }

    /// Half-open character range of line `i` covered by `sel`. The only place
    /// a caret pair is interpreted, so drawing and copying stay in step.
    fn span(&self, sel: Option<(Caret, Caret)>, i: usize) -> Option<(usize, usize)> {
        let (a, b) = self.ordered(sel)?;
        let r = *self.rank.get(i)?;
        if r < self.rank[a.line] || r > self.rank[b.line] {
            return None;
        }
        let lo = if i == a.line { a.ch } else { 0 };
        let hi = if i == b.line {
            b.ch
        } else {
            self.lines[i].char_count()
        };
        (lo < hi).then_some((lo, hi))
    }

    /// Normalized so the first caret precedes the second.
    fn ordered(&self, sel: Option<(Caret, Caret)>) -> Option<(Caret, Caret)> {
        let (a, b) = sel?;
        if self.caret_key(a) <= self.caret_key(b) {
            Some((a, b))
        } else {
            Some((b, a))
        }
    }

    fn caret_key(&self, c: Caret) -> (usize, usize) {
        (self.rank.get(c.line).copied().unwrap_or(0), c.ch)
    }
}

/// Index of the character boundary in `char_x` closest to `x`.
fn nearest_boundary(char_x: &[f32], x: f32) -> usize {
    let mut best = 0;
    let mut best_d = f32::MAX;
    for (k, &bx) in char_x.iter().enumerate() {
        let d = (x - bx).abs();
        if d < best_d {
            best_d = d;
            best = k;
        }
    }
    best
}
