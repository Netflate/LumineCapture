// Selection over recognized text.
//
// The selection is represented as two points: start and end (line + character position).
// Text between them includes full middle lines and partial outer lines.
// 
// Uses `nearest_line` for click/hover detection so mouse clicks and hover effects 
// always target the exact same line.

use tiny_skia::Rect;

use super::OcrLine;
use super::layout::{self, Block};

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

/// One plate for the renderer to draw. A block is the unit a double-click
/// selects, so it is also the unit that gets a box - a line that ended up in no
/// group of its own is a block of one and still gets one.
pub struct BlockPlate {
    pub bounds: Rect,
    pub hovered: bool,
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
    /// The scanned area (global). Shaded as a whole so it is obvious what was
    /// read - and, when no selection was drawn, which monitor it came from.
    region: Option<Rect>,
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
    /// Remember the area a scan covers. Set when recognition starts, so the
    /// progress overlay and the finished result shade the same rectangle.
    pub fn set_region(&mut self, region: Rect) {
        self.region = Some(region);
    }

    pub fn region(&self) -> Option<Rect> {
        self.region
    }

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

    pub fn block_plates(&self) -> impl Iterator<Item = BlockPlate> + '_ {
        let hovered_block = self.hovered.map(|line| self.block_of[line]);
        self.blocks.iter().enumerate().map(move |(i, block)| BlockPlate {
            bounds: block.bounds,
            hovered: Some(i) == hovered_block,
        })
    }

    /// Bounds of the block under the pointer, the area a hover change repaints.
    fn hover_bounds(&self) -> Option<Rect> {
        let line = self.hovered?;
        let &bi = self.block_of.get(line)?;
        self.blocks.get(bi).map(|b| b.bounds)
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

    /// Everything the overlay covers: the shaded region plus any text that
    /// spilled outside it. Used when the whole overlay has to be repainted -
    /// results arriving, the tool being left.
    pub fn bounds(&self) -> Option<Rect> {
        self.blocks
            .iter()
            .map(|b| b.bounds)
            .chain(self.region)
            .fold(None, layout::union_rect)
    }

    // ── changing the selection ───────────────────────────────────────────────

    // Every mutator reports the area that has to be repainted rather than a
    // bare `changed` flag: the overlay is translucent, so the caller has to
    // restore exactly that area from the dim layer before it is drawn again.
    // Repainting the whole result instead would mean filling the entire
    // scanned region on every mouse move.

    /// Move the hover highlight. Returns the area to repaint.
    pub fn set_hovered(&mut self, line: Option<usize>) -> Option<Rect> {
        if self.hovered == line {
            return None;
        }
        let old = self.hover_bounds();
        self.hovered = line;
        union(old, self.hover_bounds())
    }

    /// Start a drag selection at the caret nearest the pointer.
    pub fn begin_drag(&mut self, pointer: (f64, f64)) -> Option<Rect> {
        let caret = self.caret_at(pointer, false)?;
        let old = self.sel_bounds(self.sel);
        self.anchor_block = self.block_of.get(caret.line).copied();
        self.sel = Some((caret, caret));
        union(old, self.sel_bounds(self.sel))
    }

    /// Expands the selection to the nearest line position under the mouse.
    ///
    /// While dragging horizontally inside the current block, only that block's lines 
    /// can be selected (preventing accidental column grabs).
    ///
    /// Once the mouse moves past the top or bottom of the block, selection flows 
    /// normally in reading order.
    pub fn extend_drag(&mut self, pointer: (f64, f64)) -> Option<Rect> {
        let (anchor, _) = self.sel?;
        let focus = self.caret_at(pointer, true)?;
        let next = Some((anchor, focus));
        if self.sel == next {
            return None;
        }
        let old = self.sel_bounds(self.sel);
        self.sel = next;
        union(old, self.sel_bounds(self.sel))
    }

    /// Select the whole block a line belongs to (double-click).
    pub fn select_block(&mut self, line: usize) -> Option<Rect> {
        let &bi = self.block_of.get(line)?;
        let block = &self.blocks[bi];
        let (Some(&first), Some(&last)) = (block.lines.first(), block.lines.last()) else {
            return None;
        };
        self.anchor_block = Some(bi);
        self.set_span(first, last)
    }

    pub fn select_all(&mut self) -> Option<Rect> {
        self.anchor_block = None;
        let (Some(&first), Some(&last)) = (self.order.first(), self.order.last()) else {
            return None;
        };
        self.set_span(first, last)
    }

    pub fn deselect(&mut self) -> Option<Rect> {
        self.anchor_block = None;
        let old = self.sel_bounds(self.sel);
        self.sel = None;
        old
    }

    /// Select from the start of `first` to the end of `last`.
    fn set_span(&mut self, first: usize, last: usize) -> Option<Rect> {
        let next = Some((
            Caret { line: first, ch: 0 },
            Caret {
                line: last,
                ch: self.lines[last].char_count(),
            },
        ));
        if self.sel == next {
            return None;
        }
        let old = self.sel_bounds(self.sel);
        self.sel = next;
        union(old, self.sel_bounds(next))
    }

    /// Bounds of every line a caret pair touches.
    fn sel_bounds(&self, sel: Option<(Caret, Caret)>) -> Option<Rect> {
        let (a, b) = self.ordered(sel)?;
        let (lo, hi) = (self.rank[a.line], self.rank[b.line]);
        self.order[lo..=hi]
            .iter()
            .fold(None, |acc, &li| layout::union_rect(acc, self.lines[li].bounds))
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

fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => layout::union_rect(Some(a), b),
        (some, None) | (None, some) => some,
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
