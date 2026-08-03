use crate::editor::EditorState;
use crate::tools::ToolBehavior;
use crate::types::MouseButton;
use crate::types::annotations::{Annotation, AnnotationShape};
use tiny_skia::{Color, Rect};

pub struct SimpleShapeTool {
    pub make_shape: fn((f32, f32), (f32, f32)) -> AnnotationShape,
    pub color: Color,
    pub stroke_width: f32,
}

pub struct PenTool;

impl ToolBehavior for PenTool {
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
                shape: AnnotationShape::Pen { points: vec![pos] },
                color: Color::from_rgba8(255, 255, 255, 255),
                stroke_width: 8.0,
                bbox: Rect::from_xywh(pos.0, pos.1, 1.0, 1.0).unwrap(),
            };
            ann.update_bbox();

            state.pending = Some(ann);
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
        let pos = (state.pointer.global.0 as f32, state.pointer.global.1 as f32);

        if let Some(ann) = state.pending.as_mut() {
            if let AnnotationShape::Pen { points } = &mut ann.shape {
                if let Some(&last) = points.last()
                    && let Some(segment_bbox) = Rect::from_ltrb(
                        last.0.min(pos.0),
                        last.1.min(pos.1),
                        last.0.max(pos.0),
                        last.1.max(pos.1),
                    )
                {
                    state.damage_rects.push(segment_bbox);
                    state.annotations_dirty = true;
                }
                points.push(pos);
            }

            ann.update_bbox();
        }
    }
}
