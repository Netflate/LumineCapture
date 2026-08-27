use crate::editor::{EditorState, DamageZone};
use crate::tools::ToolBehavior;
use crate::types::MouseButton;
use crate::types::annotations::{Annotation, AnnotationShape};
use tiny_skia::Rect;

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
                color: state.tool_settings.color,
                stroke_width: state.tool_settings.stroke_width,
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
        let raw_pos = (state.pointer.global.0 as f32, state.pointer.global.1 as f32);

        if let Some(ann) = state.pending.as_mut() {
            if let AnnotationShape::Pen { points } = &mut ann.shape {
                let smoothed = if let Some(&last) = points.last() {
                    const SMOOTHING: f32 = 0.6; // 0.1-0.9 range
                    (
                        last.0 + (raw_pos.0 - last.0) * (1.0 - SMOOTHING),
                        last.1 + (raw_pos.1 - last.1) * (1.0 - SMOOTHING),
                    )
                } else {
                    raw_pos
                };

                if let Some(&last) = points.last() {
                    let dx = smoothed.0 - last.0;
                    let dy = smoothed.1 - last.1;
                    const MIN_DIST_SQ: f32 = 1.0;
                    if dx * dx + dy * dy < MIN_DIST_SQ {
                        return;
                    }
                    points.push(smoothed);

                    state.damage_rects.push(DamageZone::Global(ann.last_segment_bbox()));
                    state.annotations_dirty = true;
                } else {
                    points.push(smoothed);
                } 
            };
            ann.update_bbox();
        }
    }
}
