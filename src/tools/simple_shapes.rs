use crate::tools::ToolBehavior;
use crate::types::{EditorState, MouseButton};
use crate::types::annotations::{Annotation, AnnotationShape};
use tiny_skia::Color;

pub struct SimpleShapeTool {
    pub make_shape: fn((f32, f32), (f32, f32)) -> AnnotationShape,
    pub color: Color,
    pub stroke_width: f32,
}

impl ToolBehavior for SimpleShapeTool {
    fn on_button(&self, state: &mut EditorState, _button: MouseButton, pressed: bool, dirty_mask: &mut u32) {
        let pos = (state.pointer.global.0 as f32, state.pointer.global.1 as f32);
        if pressed {
            let ann = Annotation {
                id: state.next_id,
                shape: (self.make_shape)(pos, pos),
                color: self.color,
                stroke_width: self.stroke_width,
            };
            ann.mark_dirty(&state.placements, dirty_mask);
            state.pending = Some(ann.clone());
            state.prev_pending = Some(ann);
        } else if let Some(ann) = state.pending.take() {
            state.next_id += 1;
            ann.mark_dirty(&state.placements, dirty_mask);
            state.annotations.push(ann);
            state.prev_pending = None;
        }
    }

    fn on_move(&self, state: &mut EditorState, _global: (f64, f64), _sel_dirty: &mut bool, dirty_mask: &mut u32) {
        if state.pending.is_none() { return; }
        let pos = (state.pointer.global.0 as f32, state.pointer.global.1 as f32);
        if let Some(ann) = state.pending.as_mut() {
            let start = ann.shape.start_point();
            ann.shape = (self.make_shape)(start, pos);
        }
        if let Some(ann) = &state.pending {
            ann.mark_dirty(&state.placements, dirty_mask);
        }
    }
}