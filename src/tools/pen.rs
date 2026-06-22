use crate::tools::ToolBehavior;
use crate::types::{EditorState, MouseButton};
use crate::types::annotations::{Annotation, AnnotationShape};
use crate::utils::get_overlapping_monitors;
use crate::types::Placement;
use tiny_skia::{Color, Rect};

pub struct SimpleShapeTool {
    pub make_shape: fn((f32, f32), (f32, f32)) -> AnnotationShape,
    pub color: Color,
    pub stroke_width: f32,
}


pub struct PenTool;

impl ToolBehavior for PenTool {
    fn on_button(&self, state: &mut EditorState, _button: MouseButton, pressed: bool, dirty_mask: &mut u32) {
        let pos = (state.pointer.global.0 as f32, state.pointer.global.1 as f32);
        if pressed {
            let ann = Annotation {
                id: state.next_id,
                shape: AnnotationShape::Pen { points: vec![pos] },
                color: Color::from_rgba8(255, 0, 0, 255),
                stroke_width: 2.0,
            };
            ann.mark_dirty(&state.placements, dirty_mask);
            state.pending = Some(ann);
        } else if let Some(ann) = state.pending.take() {
            state.next_id += 1;
            ann.mark_dirty(&state.placements, dirty_mask);
            state.annotations.push(ann);
            state.prev_pending = None;
        }
    }

    fn on_move(&self, state: &mut EditorState, _global: (f64, f64), _sel_dirty: &mut bool, dirty_mask: &mut u32) {
        let pos = (state.pointer.global.0 as f32, state.pointer.global.1 as f32);
        if let Some(ann) = state.pending.as_mut() {
            if let AnnotationShape::Pen { points } = &mut ann.shape {
                if let Some(&last) = points.last() {
                    mark_segment_dirty(last, pos, &state.placements, ann.stroke_width, dirty_mask);
                }
                points.push(pos);
            }
        }
    }
}

fn mark_segment_dirty(from: (f32, f32), to: (f32, f32), placements: &[Placement], stroke_width: f32, dirty_mask: &mut u32) {
    let pad = stroke_width * 2.0 + 2.0;
    if let Some(bbox) = Rect::from_ltrb(
        from.0.min(to.0) - pad,
        from.1.min(to.1) - pad,
        from.0.max(to.0) + pad,
        from.1.max(to.1) + pad,
    ) {
        *dirty_mask |= get_overlapping_monitors(&bbox, placements);
    }
}