use crate::editor::{DamageZone, EditorState};
use crate::tools::text::update_text_bbox_inline;
use crate::types::{SelectionHandle, SignedRect};
use crate::utils::{apply_handle_drag, hit_test_rect_handle};
use crate::theme::shadow;
use tiny_skia::{Color, Rect};


/// Arrow head size for a stroke over a shaft of `len`. The renderer and the
/// bbox must agree on it, otherwise the damage rect clips the drawn head.
pub fn arrow_head(stroke_width: f32, len: f32) -> (f32, f32) {
    let arrow = &crate::config::get().annotations.arrow;
    let head_len = (stroke_width * arrow.head_length)
        .max(arrow.head_min)
        .min(len * arrow.head_max);
    (head_len, head_len * arrow.head_width)
}

#[derive(Clone, PartialEq, Debug)]
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
        filled: bool,
    },
    Circle {
        start: (f32, f32),
        end: (f32, f32),
        filled: bool,
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
        bold: bool,
        italic: bool,
    },
}

pub struct AnnDragState {
    pub handle: SelectionHandle,
    pub start_global: (f64, f64),
    pub prev_global: (f64, f64),
    pub orig: Annotation, // snapshot
    pub orig_index: usize,
    // ^ index of the annotation in the baked list before dragging
    // since dragging removes the annotation from baked and it becomes pending one
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

#[derive(Clone, PartialEq, Debug)]
pub struct Annotation {
    pub id: u64,
    pub shape: AnnotationShape,
    pub color: Color,
    pub shadow_color: Color,
    pub stroke_width: f32,
    pub bbox: Rect,
}

impl Annotation {
    pub fn scaled(&self, s: f32) -> Annotation {
        let p = |(x, y): (f32, f32)| (x * s, y * s);
        let shape = match &self.shape {
            AnnotationShape::NumeratedArrow { start, end, number } => {
                AnnotationShape::NumeratedArrow {
                    start: p(*start),
                    end: p(*end),
                    number: *number,
                }
            }
            AnnotationShape::Arrow { start, end } => AnnotationShape::Arrow {
                start: p(*start),
                end: p(*end),
            },
            AnnotationShape::Rectangle { start, end, filled } => AnnotationShape::Rectangle {
                start: p(*start),
                end: p(*end),
                filled: *filled,
            },
            AnnotationShape::Circle { start, end, filled } => AnnotationShape::Circle {
                start: p(*start),
                end: p(*end),
                filled: *filled,
            },
            AnnotationShape::Line { start, end } => AnnotationShape::Line {
                start: p(*start),
                end: p(*end),
            },
            AnnotationShape::Pen { points } => AnnotationShape::Pen {
                points: points.iter().map(|&q| p(q)).collect(),
            },
            AnnotationShape::Text {
                start,
                content,
                font_size,
                bold,
                italic,
            } => AnnotationShape::Text {
                start: p(*start),
                content: content.clone(),
                font_size: font_size * s,
                bold: *bold,
                italic: *italic,
            },
        };
        let bbox = Rect::from_ltrb(
            self.bbox.left() * s,
            self.bbox.top() * s,
            self.bbox.right() * s,
            self.bbox.bottom() * s,
        )
        .unwrap_or(self.bbox);

        Annotation {
            id: self.id,
            shape,
            color: self.color,
            shadow_color: self.shadow_color,
            stroke_width: self.stroke_width * s,
            bbox,
        }
    }

    pub fn update_bbox(&mut self) {
        match &self.shape {
            AnnotationShape::Rectangle { start, end, .. }
            | AnnotationShape::Circle { start, end, .. }
            | AnnotationShape::Line { start, end } => {
                let pad = self.stroke_width / 2.0 + shadow::width_bonus() / 2.0;
                self.bbox = Rect::from_ltrb(
                    start.0.min(end.0) - pad,
                    start.1.min(end.1) - pad,
                    start.0.max(end.0) + pad,
                    start.1.max(end.1) + pad,
                )
                .unwrap();
            }
            AnnotationShape::Arrow { start, end } => {
                let pad = self.stroke_width / 2.0 + shadow::width_bonus() / 2.0;
                let dx = end.0 - start.0;
                let dy = end.1 - start.1;
                let len = (dx * dx + dy * dy).sqrt().max(1.0);
                let (head_len, head_width) = arrow_head(self.stroke_width, len);

                let ux = dx / len;
                let uy = dy / len;
                let px = -uy;
                let py = ux;

                let base = (end.0 - ux * head_len, end.1 - uy * head_len);
                let b1 = (base.0 + px * head_width, base.1 + py * head_width);
                let b2 = (base.0 - px * head_width, base.1 - py * head_width);

                let xs = [start.0, end.0, b1.0, b2.0];
                let ys = [start.1, end.1, b1.1, b2.1];

                self.bbox = Rect::from_ltrb(
                    xs.iter().cloned().fold(f32::INFINITY, f32::min) - pad,
                    ys.iter().cloned().fold(f32::INFINITY, f32::min) - pad,
                    xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max) + pad,
                    ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max) + pad,
                )
                .unwrap();
            }

            AnnotationShape::NumeratedArrow { start, end, .. } => {
                let circle_radius = self.stroke_width * crate::config::get().annotations.numbered.circle;
                let xs = [start.0 - circle_radius, start.0 + circle_radius, end.0];
                let ys = [start.1 - circle_radius, start.1 + circle_radius, end.1];

                self.bbox = Rect::from_ltrb(
                    xs.iter().cloned().fold(f32::INFINITY, f32::min),
                    ys.iter().cloned().fold(f32::INFINITY, f32::min),
                    xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max),
                    ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max),
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

    pub fn pen_active_tail_bbox(&self) -> Rect {
        let pad = crate::renderer::visual_pad(self.stroke_width);
        match &self.shape {
            AnnotationShape::Pen { points } if points.len() >= 3 => {
                let p_prev = points[points.len() - 2];
                let p_last = points[points.len() - 1];
                let mid = ((p_prev.0 + p_last.0) / 2.0, (p_prev.1 + p_last.1) / 2.0);
                Rect::from_ltrb(
                    mid.0.min(p_last.0) - pad,
                    mid.1.min(p_last.1) - pad,
                    mid.0.max(p_last.0) + pad,
                    mid.1.max(p_last.1) + pad,
                )
                .unwrap_or(self.bbox)
            }
            AnnotationShape::Pen { points } if points.len() == 2 => {
                let p0 = points[0];
                let p1 = points[1];
                Rect::from_ltrb(
                    p0.0.min(p1.0) - pad,
                    p0.1.min(p1.1) - pad,
                    p0.0.max(p1.0) + pad,
                    p0.1.max(p1.1) + pad,
                )
                .unwrap_or(self.bbox)
            }
            AnnotationShape::Pen { points } if points.len() == 1 => {
                let p0 = points[0];
                Rect::from_ltrb(
                    p0.0 - pad,
                    p0.1 - pad,
                    p0.0 + pad,
                    p0.1 + pad,
                )
                .unwrap_or(self.bbox)
            }
            _ => self.damage_bbox(false),
        }
    }

    pub fn translate_mut(&mut self, dx: f32, dy: f32) {
        match &mut self.shape {
            AnnotationShape::NumeratedArrow { start, end, .. }
            | AnnotationShape::Arrow { start, end }
            | AnnotationShape::Rectangle { start, end, .. }
            | AnnotationShape::Circle { start, end, .. }
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
            AnnotationShape::Rectangle { start, end, filled } => AnnotationShape::Rectangle {
                start: remap(*start),
                end: remap(*end),
                filled: *filled,
            },
            AnnotationShape::Circle { start, end, filled } => AnnotationShape::Circle {
                start: remap(*start),
                end: remap(*end),
                filled: *filled,
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
                bold,
                italic,
            } => {
                // NOTE: bbox after this is stale, must call update_text_bbox() afterwards
                // because text dimensions require FontSystem
                // font_size scales by the axis with larger relative change
                // not the best possible implementation, but meh
                let scale = sx.abs().max(sy.abs());
                let text = &crate::config::get().annotations.text;
                AnnotationShape::Text {
                    start: remap(*start),
                    content: content.clone(),
                    font_size: (*font_size * scale).clamp(text.resize_min, text.resize_max),
                    bold: *bold,
                    italic: *italic,
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
        let pad = crate::config::get().input.handle_hit_width;

        x >= self.bbox.left() - pad
            && x <= self.bbox.right() + pad
            && y >= self.bbox.top() - pad
            && y <= self.bbox.bottom() + pad
    }

    pub fn damage_bbox(&self, is_selected: bool) -> Rect {
        let pad = if is_selected {
            crate::renderer::selection_chrome_pad()
                .max(crate::renderer::visual_pad(self.stroke_width))
        } else {
            crate::renderer::visual_pad(self.stroke_width)
        };
        Rect::from_ltrb(
            self.bbox.left() - pad,
            self.bbox.top() - pad,
            self.bbox.right() + pad,
            self.bbox.bottom() + pad,
        )
        .unwrap_or(self.bbox)
    }
}

// to not repeat the same code in tools/pick.rs, and tools/text.rs
// (they both can select and drag, tho text does that only with text)
// utils.rs or editor/drag.rs

pub fn handle_hit_test_for_annotation(ann: &Annotation, pos: (f64, f64)) -> SelectionHandle {
    let out_pad = crate::config::get().input.handle_hit_width / 2.0;
    let bbox = ann.bbox;

    let visual_bbox = Rect::from_ltrb(
        bbox.left() - out_pad,
        bbox.top() - out_pad,
        bbox.right() + out_pad,
        bbox.bottom() + out_pad,
    )
    .unwrap_or(bbox);

    hit_test_rect_handle(&visual_bbox, pos)
}

pub fn begin_drag_for_annotation(state: &mut EditorState, idx: usize) {
    let ann = &state.annotations[idx];
    let handle = handle_hit_test_for_annotation(ann, state.input.pointer.global);

    state.ann_drag = Some(AnnDragState {
        handle,
        start_global: state.input.pointer.global,
        prev_global: state.input.pointer.global,
        orig: ann.clone(),
        orig_index: idx,
    });
}

pub fn commit_drag_if_changed(state: &mut EditorState) {
    if let Some(drag) = state.ann_drag.take()
        && let Some(ann) = state.pending.take() {
            let actually_changed =
                !matches!(drag.handle, SelectionHandle::None) && ann != drag.orig;

            let insert_idx = drag.orig_index.min(state.annotations.len());
            if actually_changed {
                let mut pre_drag = state.annotations.clone();
                pre_drag.insert(insert_idx, drag.orig);
                state.undo_stack.push(pre_drag);
                state.redo_stack.clear();
            }

            state.bake_annotation(&ann);
            state
                .damage_rects
                .push(DamageZone::Global(ann.damage_bbox(true)));
            state.annotations.insert(insert_idx, ann);
            state.prev_pending = None;
            state.selected_annotation = Some(insert_idx);
        }
}

pub fn apply_annotation_drag(state: &mut EditorState, global: (f64, f64)) {
    let (handle, prev_global, start_global, orig_index) = match &state.ann_drag {
        Some(drag) => (
            drag.handle,
            drag.prev_global,
            drag.start_global,
            drag.orig_index,
        ),
        None => return,
    };

    if matches!(handle, SelectionHandle::None) {
        return;
    }

    // Move the annotation out of baked list while dragging.
    // The dragged annotation is considered as `pending`,
    // and the old baked pixels are removed. This avoids
    // rebuilding the entire annotation layer on every mouse move.
    if state.pending.is_none() {
        if orig_index < state.annotations.len() {
            let ann = state.annotations.remove(orig_index);
            state.layer_damage_rects.push(ann.damage_bbox(false));
            state
                .damage_rects
                .push(DamageZone::Global(ann.damage_bbox(true)));
            state.pending = Some(ann.clone());
            state.prev_pending = Some(ann);
            state.selected_annotation = None;
        } else {
            return;
        }
    }

    let Some(ann) = state.pending.as_mut() else {
        return;
    };

    state
        .damage_rects
        .push(DamageZone::Global(ann.damage_bbox(true)));

    match handle {
        SelectionHandle::Move => {
            // move: incremental delta from prev_global, no clone needed
            let dx = (global.0 - prev_global.0) as f32;
            let dy = (global.1 - prev_global.1) as f32;
            ann.translate_mut(dx, dy);
        }
        _ => {
            if matches!(ann.shape, AnnotationShape::Text { .. }) {
                // text resize: incremental from prev_global
                // using separate function since text scales font_size, not coordinates
                apply_text_resize_incremental(ann, handle, prev_global, global);
                let editor = crate::tools::text::ensure_text_editor(
                    ann,
                    &mut state.text.editors,
                    &mut state.text.font_system,
                );
                update_text_bbox_inline(ann, editor, &mut state.text.font_system);
            } else {
                // shape resize: always from orig + total delta to avoid accumulated error
                let total_dx = (global.0 - start_global.0) as f32;
                let total_dy = (global.1 - start_global.1) as f32;
                let orig = state.ann_drag.as_ref().unwrap().orig.clone();
                apply_shape_resize_from_orig(ann, &orig, handle, total_dx, total_dy);
            }
        }
    }

    state
        .damage_rects
        .push(DamageZone::Global(ann.damage_bbox(true)));

    if let Some(drag) = state.ann_drag.as_mut() {
        drag.prev_global = global;
    }
}

pub fn rebuild_annotation(state: &mut EditorState, idx: usize) {
    let Some(ann) = state.annotations.get(idx) else {
        return;
    };
    state.layer_damage_rects.push(ann.damage_bbox(false));
    state
        .damage_rects
        .push(DamageZone::Global(ann.damage_bbox(true)));

    if matches!(state.annotations[idx].shape, AnnotationShape::Text { .. }) {
        let editor = crate::tools::text::ensure_text_editor(
            &state.annotations[idx],
            &mut state.text.editors,
            &mut state.text.font_system,
        );
        update_text_bbox_inline(&mut state.annotations[idx], editor, &mut state.text.font_system);
    } else {
        state.annotations[idx].update_bbox();
    }

    state
        .layer_damage_rects
        .push(state.annotations[idx].damage_bbox(false));
    state
        .damage_rects
        .push(DamageZone::Global(state.annotations[idx].damage_bbox(true)));
    state.annotations_dirty = true;
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
    let out_pad = crate::config::get().input.handle_hit_width / 2.0;

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

    let text = &crate::config::get().annotations.text;
    let new_font_size = (*font_size * scale).clamp(text.resize_min, text.resize_max);
    let applied_scale = new_font_size / *font_size;

    start.0 = anchor_x + (start.0 - anchor_x) * applied_scale;
    start.1 = anchor_y + (start.1 - anchor_y) * applied_scale;
    *font_size = new_font_size;
    // bbox updated in apply_annotation_drag via update_text_bbox
}
