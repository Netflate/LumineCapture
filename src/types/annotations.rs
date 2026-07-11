use crate::editor::EditorState;
use crate::tools::text::update_text_bbox_inline;
use crate::types::{SelectionHandle, SignedRect};
use crate::utils::{apply_handle_drag, hit_test_rect_handle};
use tiny_skia::{Color, Rect};

pub const HANDLE_PAD: f64 = 20.0;
pub const SHADOW_COLOR: (u8, u8, u8, u8) = (0, 0, 0, 130);
pub const SHADOW_WIDTH_BONUS: f32 = 4.0;

#[derive(Clone)]
pub enum AnnotationShape {
    NumeratedArrow {
        start: (f32, f32),
        end: (f32, f32),
        number: u32,
    },
    Arrow {
        start: (f32, f32),
        end: (f32, f32),
    },
    Rectangle {
        start: (f32, f32),
        end: (f32, f32),
    },
    Circle {
        start: (f32, f32),
        end: (f32, f32),
    },
    Line {
        start: (f32, f32),
        end: (f32, f32),
    },
    Pen {
        points: Vec<(f32, f32)>,
    },
    Text {
        start: (f32, f32),
        content: String,
        font_size: f32,
    },
}

pub struct AnnDragState {
    pub handle: SelectionHandle,
    pub start_global: (f64, f64),
    pub prev_global: (f64, f64),
    pub orig: Annotation, // snapshot
}

impl AnnotationShape {
    pub fn start_point(&self) -> (f32, f32) {
        match self {
            AnnotationShape::NumeratedArrow { start, .. } => *start,
            AnnotationShape::Arrow { start, .. } => *start,
            AnnotationShape::Rectangle { start, .. } => *start,
            AnnotationShape::Circle { start, .. } => *start,
            AnnotationShape::Line { start, .. } => *start,
            AnnotationShape::Pen { points } => points.first().copied().unwrap_or((0.0, 0.0)),
            AnnotationShape::Text { start, .. } => *start,
        }
    }
}

#[derive(Clone)]
pub struct Annotation {
    pub id: u64,
    pub shape: AnnotationShape,
    pub color: Color,
    pub stroke_width: f32,
    pub bbox: Rect,
}

impl Annotation {
    pub fn update_bbox(&mut self) {
        match &self.shape {
            AnnotationShape::NumeratedArrow { start, end, .. }
            | AnnotationShape::Arrow { start, end }
            | AnnotationShape::Rectangle { start, end }
            | AnnotationShape::Circle { start, end }
            | AnnotationShape::Line { start, end } => {
                self.bbox = Rect::from_ltrb(
                    start.0.min(end.0),
                    start.1.min(end.1),
                    start.0.max(end.0),
                    start.1.max(end.1),
                )
                .unwrap();
            }

            AnnotationShape::Pen { points } => {
                let min_x = points.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
                let min_y = points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
                let max_x = points.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max);
                let max_y = points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);

                self.bbox = Rect::from_ltrb(min_x, min_y, max_x, max_y).unwrap();
            }

            AnnotationShape::Text { .. } => {} // nothing, text will use its own function to update bbox
        }
    }

    pub fn last_segment_bbox(&self) -> Rect {
        match &self.shape {
            // to render only new pixels from pen, insted of rendering the whole rectangle
            AnnotationShape::Pen { points } if points.len() >= 2 => {
                let from = points[points.len() - 2];
                let to = points[points.len() - 1];

                Rect::from_ltrb(
                    from.0.min(to.0),
                    from.1.min(to.1),
                    from.0.max(to.0),
                    from.1.max(to.1),
                )
                .unwrap()
            }
            _ => self.bbox,
        }
    }

    pub fn translate_mut(&mut self, dx: f32, dy: f32) {
        match &mut self.shape {
            AnnotationShape::NumeratedArrow { start, end, .. }
            | AnnotationShape::Arrow { start, end }
            | AnnotationShape::Rectangle { start, end }
            | AnnotationShape::Circle { start, end }
            | AnnotationShape::Line { start, end } => {
                start.0 += dx;
                start.1 += dy;
                end.0 += dx;
                end.1 += dy;
            }
            AnnotationShape::Pen { points } => {
                for p in points.iter_mut() {
                    p.0 += dx;
                    p.1 += dy;
                }
            }
            AnnotationShape::Text { start, .. } => {
                start.0 += dx;
                start.1 += dy;
            }
        }
        // NOTE: text bbox size depends on font metrics (not available here)
        // while for moving, size is unchanged, only coordinates changes
        // full recalc bbox is done in update_text_bbox(), in tools/text.rs
        match &self.shape {
            AnnotationShape::Text { .. } => {
                self.bbox = Rect::from_ltrb(
                    self.bbox.left() + dx,
                    self.bbox.top() + dy,
                    self.bbox.right() + dx,
                    self.bbox.bottom() + dy,
                )
                .unwrap_or(self.bbox);
            }
            _ => self.update_bbox(),
        }
    }

    pub fn translate(&self, dx: f32, dy: f32) -> Annotation {
        let mut result = self.clone();
        result.translate_mut(dx, dy);
        result
    }

    pub fn resize_to_bbox(&self, new_bbox: SignedRect) -> Annotation {
        let orig = self.bbox;

        let sx = if orig.width() > 0.0 {
            new_bbox.width() / orig.width()
        } else {
            1.0
        };
        let sy = if orig.height() > 0.0 {
            new_bbox.height() / orig.height()
        } else {
            1.0
        };

        let remap = |p: (f32, f32)| -> (f32, f32) {
            (
                new_bbox.left + (p.0 - orig.left()) * sx,
                new_bbox.top + (p.1 - orig.top()) * sy,
            )
        };

        let shape = match &self.shape {
            AnnotationShape::NumeratedArrow { start, end, number } => {
                AnnotationShape::NumeratedArrow {
                    start: remap(*start),
                    end: remap(*end),
                    number: *number,
                }
            }
            AnnotationShape::Arrow { start, end } => AnnotationShape::Arrow {
                start: remap(*start),
                end: remap(*end),
            },
            AnnotationShape::Rectangle { start, end } => AnnotationShape::Rectangle {
                start: remap(*start),
                end: remap(*end),
            },
            AnnotationShape::Circle { start, end } => AnnotationShape::Circle {
                start: remap(*start),
                end: remap(*end),
            },
            AnnotationShape::Line { start, end } => AnnotationShape::Line {
                start: remap(*start),
                end: remap(*end),
            },
            AnnotationShape::Pen { points } => AnnotationShape::Pen {
                points: points.iter().map(|p| remap(*p)).collect(),
            },
            AnnotationShape::Text {
                start,
                content,
                font_size,
            } => {
                // NOTE: bbox after this is stale, must call update_text_bbox() afterwards
                // because text dimensions require FontSystem
                // font_size scales by the axis with larger relative change
                // not the best possible implementation, but meh
                let scale = sx.abs().max(sy.abs());
                AnnotationShape::Text {
                    start: remap(*start),
                    content: content.clone(),
                    font_size: (*font_size * scale).clamp(6.0, 200.0),
                }
            }
        };

        let mut result = Annotation {
            shape,
            ..self.clone()
        };
        result.update_bbox();
        result
    }

    pub fn initial_hit_test(&self, coordinates: (f64, f64)) -> bool {
        // TODO: separate for pen
        let (x, y) = (coordinates.0 as f32, coordinates.1 as f32);
        let pad = HANDLE_PAD as f32;

        x >= self.bbox.left() - pad
            && x <= self.bbox.right() + pad
            && y >= self.bbox.top() - pad
            && y <= self.bbox.bottom() + pad
    }

    pub fn damage_bbox(&self, is_selected: bool) -> Rect {
        if is_selected {
            let pad = HANDLE_PAD as f32; // if its selected we need to add handlers padding
            Rect::from_ltrb(
                self.bbox.left() - pad,
                self.bbox.top() - pad,
                self.bbox.right() + pad,
                self.bbox.bottom() + pad,
            )
            .unwrap_or(self.bbox)
        } else {
            self.bbox
        }
    }
}

// to not repeat the same code in tools/pick.rs, and tools/text.rs
// (they both can select and drag, tho text does that only with text)
// utils.rs или editor/drag.rs

pub fn begin_drag_for_annotation(state: &mut EditorState, idx: usize) {
    let ann = &state.annotations[idx];
    let out_pad = (HANDLE_PAD / 2.0) as f32;
    let bbox = ann.bbox;

    let visual_bbox = Rect::from_ltrb(
        bbox.left() - out_pad,
        bbox.top() - out_pad,
        bbox.right() + out_pad,
        bbox.bottom() + out_pad,
    )
    .unwrap_or(bbox);

    let handle = hit_test_rect_handle(&visual_bbox, state.pointer.global);

    state.ann_drag = Some(AnnDragState {
        handle,
        start_global: state.pointer.global,
        prev_global: state.pointer.global,
        orig: ann.clone(),
    });
}

pub fn commit_drag_if_changed(state: &mut EditorState) {
    // mouse up > commit to undo only if something actually changed
    if let Some(drag) = &state.ann_drag
        && let Some(idx) = state.selected_annotation
    {
        let actually_changed = !matches!(drag.handle, SelectionHandle::None)
            && state.annotations[idx].bbox != drag.orig.bbox;

        if actually_changed {
            // annotations[idx] already has the new position from on_move
            // we reconstruct the pre-drag snapshot using drag.orig
            let pre_drag: Vec<_> = state
                .annotations
                .iter()
                .enumerate()
                .map(|(i, ann)| {
                    if i == idx {
                        drag.orig.clone()
                    } else {
                        ann.clone()
                    }
                })
                .collect();
            state.undo_stack.push(pre_drag);
            state.redo_stack.clear();
        }
    }
    state.ann_drag = None;
}

pub fn apply_annotation_drag(state: &mut EditorState, global: (f64, f64)) {
    let (handle, prev_global, start_global) = match &state.ann_drag {
        Some(drag) => (drag.handle, drag.prev_global, drag.start_global),
        None => return,
    };

    if matches!(handle, SelectionHandle::None) {
        return;
    }

    let Some(idx) = state.selected_annotation else {
        return;
    };

    state
        .damage_rects
        .push(state.annotations[idx].damage_bbox(true));

    match handle {
        SelectionHandle::Move => {
            // move: incremental delta from prev_global, no clone needed
            let dx = (global.0 - prev_global.0) as f32;
            let dy = (global.1 - prev_global.1) as f32;
            state.annotations[idx].translate_mut(dx, dy);
        }
        _ => {
            if matches!(state.annotations[idx].shape, AnnotationShape::Text { .. }) {
                // text resize: incremental from prev_global
                // using separate function since text scales font_size, not coordinates
                apply_text_resize_incremental(
                    &mut state.annotations[idx],
                    handle,
                    prev_global,
                    global,
                );
                let ann_id = state.annotations[idx].id;
                let editor = state.text_editors.get_mut(&ann_id);
                if let Some(ed) = editor {
                    update_text_bbox_inline(
                        &mut state.annotations[idx],
                        ed,
                        &mut state.font_system,
                    );
                }
            } else {
                // shape resize: always from orig + total delta to avoid accumulated error
                let total_dx = (global.0 - start_global.0) as f32;
                let total_dy = (global.1 - start_global.1) as f32;
                let orig = state.ann_drag.as_ref().unwrap().orig.clone();
                apply_shape_resize_from_orig(
                    &mut state.annotations[idx],
                    &orig,
                    handle,
                    total_dx,
                    total_dy,
                );
            }
        }
    }

    state
        .damage_rects
        .push(state.annotations[idx].damage_bbox(true));
    state.annotations_dirty = true;

    if let Some(drag) = state.ann_drag.as_mut() {
        drag.prev_global = global;
    }
}

// resizing function for non-text annotations
// uses total delta from orig to avoid accumulated floating point error
fn apply_shape_resize_from_orig(
    ann: &mut Annotation,
    orig: &Annotation,
    handle: SelectionHandle,
    total_dx: f32,
    total_dy: f32,
) {
    let out_pad = (HANDLE_PAD / 2.0) as f32;

    let visual_bbox = Rect::from_ltrb(
        orig.bbox.left() - out_pad,
        orig.bbox.top() - out_pad,
        orig.bbox.right() + out_pad,
        orig.bbox.bottom() + out_pad,
    )
    .unwrap_or(orig.bbox);

    let new_visual_bbox =
        apply_handle_drag(&visual_bbox, handle, (total_dx as f64, total_dy as f64));

    let clean_bbox = SignedRect {
        left: new_visual_bbox.left + out_pad,
        top: new_visual_bbox.top + out_pad,
        right: new_visual_bbox.right - out_pad,
        bottom: new_visual_bbox.bottom - out_pad,
    };

    *ann = orig.resize_to_bbox(clean_bbox);
}

// resizing function for text annotations
// incremental from prev_global since font_size can't be derived from total delta alone
fn apply_text_resize_incremental(
    ann: &mut Annotation,
    handle: SelectionHandle,
    prev: (f64, f64),
    global: (f64, f64),
) {
    let AnnotationShape::Text {
        font_size, start, ..
    } = &mut ann.shape
    else {
        return;
    };
    let bbox = ann.bbox;

    let (anchor_x, anchor_y) = match handle {
        SelectionHandle::TopLeft => (bbox.right(), bbox.bottom()),
        SelectionHandle::TopRight => (bbox.left(), bbox.bottom()),
        SelectionHandle::BottomLeft => (bbox.right(), bbox.top()),
        SelectionHandle::BottomRight => (bbox.left(), bbox.top()),
        _ => return,
    };

    let prev_dx = prev.0 as f32 - anchor_x;
    let prev_dy = prev.1 as f32 - anchor_y;
    let new_dx = global.0 as f32 - anchor_x;
    let new_dy = global.1 as f32 - anchor_y;

    let prev_dist = (prev_dx.powi(2) + prev_dy.powi(2)).sqrt().max(1.0);
    let new_dist = (new_dx.powi(2) + new_dy.powi(2)).sqrt();

    let dot = prev_dx * new_dx + prev_dy * new_dy;
    let scale = if dot <= 0.0 {
        0.0
    } else {
        new_dist / prev_dist
    };

    let new_font_size = (*font_size * scale).clamp(6.0, 300.0);
    let applied_scale = new_font_size / *font_size;

    start.0 = anchor_x + (start.0 - anchor_x) * applied_scale;
    start.1 = anchor_y + (start.1 - anchor_y) * applied_scale;
    *font_size = new_font_size;
    // bbox updated in apply_annotation_drag via update_text_bbox
}
