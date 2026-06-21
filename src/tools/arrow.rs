use tiny_skia::Color;
use crate::tools::ToolBehavior;
use crate::types::{EditorState, MouseButton};
use crate::types::annotations::{annotation_bounding_box, Annotation, AnnotationShape};
use crate::utils::get_overlapping_monitors;

pub struct ArrowTool;

impl ToolBehavior for ArrowTool {
    fn on_button(&self, state: &mut EditorState, _button: MouseButton, pressed: bool, dirty_mask: &mut u32) {
        let pos = (state.pointer.global.0 as f32, state.pointer.global.1 as f32);
        if pressed {
            let ann = Annotation {
                id: state.next_id,
                shape: AnnotationShape::Arrow { start: pos, end: pos },
                color: Color::from_rgba8(255, 0, 0, 255),
                stroke_width: 2.0,
            };
            mark_annotation_dirty(&ann, &state.placements, dirty_mask);
            state.pending = Some(ann.clone());
            state.prev_pending = Some(ann);
        } else if let Some(ann) = state.pending.take() {
            state.next_id += 1;
            mark_annotation_dirty(&ann, &state.placements, dirty_mask);
            state.annotations.push(ann);
            state.prev_pending = None;
        }
    }

    fn on_move(&self, state: &mut EditorState, _global: (f64, f64), _sel_dirty: &mut bool, dirty_mask: &mut u32) {
        if state.pending.is_none() { return; }

        // marking the old one dirty to delete what has left behind in next render
        if let Some(prev) = &state.prev_pending {
            mark_annotation_dirty(prev, &state.placements, dirty_mask);
        }

        // updating
        let pos = (state.pointer.global.0 as f32, state.pointer.global.1 as f32);
        if let Some(ann) = state.pending.as_mut() {
            ann.shape = AnnotationShape::Arrow {
                start: match ann.shape { AnnotationShape::Arrow { start, .. } => start, _ => pos },
                end: pos,
            };
        }

        // marking the new one dirty to render it
        // and putting it in the prev_pending for future on_move
        if let Some(ann) = &state.pending {
            mark_annotation_dirty(ann, &state.placements, dirty_mask);
        }
        state.prev_pending = state.pending.clone();
    }
}

fn mark_annotation_dirty(ann: &Annotation, placements: &[crate::types::Placement], dirty_mask: &mut u32) {
    if let Some(bbox) = annotation_bounding_box(ann) {
        *dirty_mask |= get_overlapping_monitors(&bbox, placements);
    }
}