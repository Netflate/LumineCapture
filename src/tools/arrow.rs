// mark all dirty - temporary solution for first implementation
use tiny_skia::Color;

use crate::tools::ToolBehavior;
use crate::types::{EditorState, MouseButton};
use crate::types::annotations::{Annotation, AnnotationShape};
use crate::app::mark_dirty;
pub struct ArrowTool;


impl ToolBehavior for ArrowTool {
    fn on_button(&self, state: &mut EditorState, _button: MouseButton, pressed: bool, dirty_mask: &mut u32) {
        let pos = (state.pointer.global.0 as f32, state.pointer.global.1 as f32);
        if pressed {
            state.pending = Some(Annotation {
                id: state.next_id,
                shape: AnnotationShape::Arrow { start: pos, end: pos },
                color: Color::from_rgba8(255, 0, 0, 255),
                stroke_width: 2.0,
            });
        } else if let Some(ann) = state.pending.take() {
            state.next_id += 1;
            state.annotations.push(ann);
        }
        mark_all_dirty(state, dirty_mask);
    }

    fn on_move(&self, state: &mut EditorState, _global: (f64, f64), _sel_dirty: &mut bool, dirty_mask: &mut u32) {
        if let Some(ann) = state.pending.as_mut() {
            let pos = (state.pointer.global.0 as f32, state.pointer.global.1 as f32);
            ann.shape = AnnotationShape::Arrow { 
                start: match ann.shape { AnnotationShape::Arrow { start, .. } => start, _ => pos },
                end: pos 
            };
            mark_all_dirty(state, dirty_mask);
        }
    }
}


fn mark_all_dirty(state: &mut EditorState, dirty_mask:&mut u32) {
    for monitor_idx in 0..state.placements.len() {
        mark_dirty(dirty_mask, monitor_idx);
    }
}