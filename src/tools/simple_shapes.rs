use crate::editor::{EditorState, DamageZone};
use crate::tools::ToolBehavior;
use crate::types::MouseButton;
use crate::types::annotations::{Annotation, AnnotationShape};
use tiny_skia::{Color, Rect};

pub struct SimpleShapeTool {
    pub make_shape: fn((f32, f32), (f32, f32)) -> AnnotationShape,
    pub color: Color,
    pub stroke_width: f32,
}

impl ToolBehavior for SimpleShapeTool {
    fn on_button(
        &self,
        state: &mut EditorState,
        _button: MouseButton,
        pressed: bool,
        _dirty_mask: &mut u32,
    ) {
        let pos = (state.pointer.global.0 as f32, state.pointer.global.1 as f32);
        if pressed {
            let mut ann = Annotation {
                id: state.next_id,
                shape: (self.make_shape)(pos, pos),
                color: self.color,
                stroke_width: self.stroke_width,
                bbox: Rect::from_xywh(pos.0, pos.1, 1.0, 1.0).unwrap(),
            };
            ann.update_bbox();

            state.pending = Some(ann.clone());
            state.prev_pending = Some(ann);
        } else if let Some(ann) = state.pending.take() {
            state.next_id += 1;
            state.push_undo();
            state.annotations.push(ann);
            state.prev_pending = None;
        }
    }

    fn on_move(
        &self,
        state: &mut EditorState,
        _global: (f64, f64),
        _dirty_mask: &mut u32,
    ) {
        if let Some(ann) = state.pending.as_mut() {
            state.damage_rects.push(DamageZone::Global(ann.bbox));

            let pos = (state.pointer.global.0 as f32, state.pointer.global.1 as f32);
            let start = ann.shape.start_point();
            ann.shape = (self.make_shape)(start, pos);
            ann.update_bbox();

            state.damage_rects.push(DamageZone::Global(ann.bbox));
            state.annotations_dirty = true;
        }
    }
}
