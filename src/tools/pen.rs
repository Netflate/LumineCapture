use crate::editor::{DamageZone, EditorState};
use crate::renderer::shadow_color_for;
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
        dirty_mask: &mut u32,
    ) {
        let pos = (state.pointer.global.0 as f32, state.pointer.global.1 as f32);

        if pressed {
            state.pending_pen_baked = 0;
            let mut ann = Annotation {
                id: state.next_id,
                shape: AnnotationShape::Pen { points: vec![pos] },
                color: state.tool_settings.color,
                shadow_color: shadow_color_for(state.tool_settings.color),
                stroke_width: state.tool_settings.stroke_width,
                bbox: Rect::from_xywh(pos.0, pos.1, 1.0, 1.0).unwrap(),
            };
            ann.update_bbox();
            let tail_bbox = ann.pen_active_tail_bbox();
            state.damage_rects.push(DamageZone::Global(tail_bbox));
            state.pending = Some(ann.clone());
            state.prev_pending = Some(ann);
            *dirty_mask |= crate::utils::get_overlapping_monitors(&tail_bbox, &state.placements);
        } else if let Some(ann) = state.pending.take() {
            state.next_id += 1;
            state.push_undo();

            let damage = ann.damage_bbox(false);

            // final halo will be drawn in renderer.
            // simplifies mouse‑up handling and avoids costly redraws every frame

            // bake the full annotation with pristine single pass drop shadow
            state.bake_annotation(&ann);
            state.damage_rects.push(DamageZone::Global(damage));
            *dirty_mask |= crate::utils::get_overlapping_monitors(&damage, &state.placements);

            state.pending_pen_baked = 0;
            state.annotations.push(ann);
            state.prev_pending = None;
        }
    }

    fn on_move(&self, state: &mut EditorState, _global: (f64, f64), dirty_mask: &mut u32) {
        let raw_pos = (state.pointer.global.0 as f32, state.pointer.global.1 as f32);

        let Some(ann) = state.pending.as_ref() else {
            return;
        };

        let AnnotationShape::Pen { points } = &ann.shape else {
            return;
        };

        let last = points.last().copied();
        let smoothed = if let Some(last) = last {
            const SMOOTHING: f32 = 0.7; // 0.1-0.9 range
            (
                last.0 + (raw_pos.0 - last.0) * (1.0 - SMOOTHING),
                last.1 + (raw_pos.1 - last.1) * (1.0 - SMOOTHING),
            )
        } else {
            raw_pos
        };

        if let Some(last) = last {
            let dx = smoothed.0 - last.0;
            let dy = smoothed.1 - last.1;
            const MIN_DIST_SQ: f32 = 1.0;
            if dx * dx + dy * dy < MIN_DIST_SQ {
                return;
            }

            let prev_tail_bbox = ann.pen_active_tail_bbox();
            state.damage_rects.push(DamageZone::Global(prev_tail_bbox));
            *dirty_mask |=
                crate::utils::get_overlapping_monitors(&prev_tail_bbox, &state.placements);

            // extract segments to bake into a small Vec to avoid borrowing state.pending
            let mut segments_to_bake = Vec::new();
            {
                let ann = state.pending.as_mut().unwrap();
                let color = ann.color;
                let stroke_width = ann.stroke_width;
                let AnnotationShape::Pen { points } = &mut ann.shape else {
                    return;
                };
                points.push(smoothed);

                let mut k = state.pending_pen_baked;
                while points.len() >= k + 3 {
                    if k == 0 {
                        let p0 = points[0];
                        let p1 = points[1];
                        let p2 = points[2];
                        let mid1 = ((p1.0 + p2.0) / 2.0, (p1.1 + p2.1) / 2.0);
                        segments_to_bake.push((p0, Some(p1), mid1, color, stroke_width));
                    } else {
                        let p_k = points[k];
                        let p_next = points[k + 1];
                        let p_after = points[k + 2];
                        let mid_k = ((p_k.0 + p_next.0) / 2.0, (p_k.1 + p_next.1) / 2.0);
                        let mid_next =
                            ((p_next.0 + p_after.0) / 2.0, (p_next.1 + p_after.1) / 2.0);
                        segments_to_bake.push((mid_k, Some(p_next), mid_next, color, stroke_width));
                    }
                    k += 1;
                }
                ann.update_bbox();
            }

            for (start_p, ctrl, end_p, color, stroke_width) in segments_to_bake {
                let seg_bbox = state.bake_pen_segment(start_p, ctrl, end_p, color, stroke_width);
                state.pending_pen_baked += 1;
                state.damage_rects.push(DamageZone::Global(seg_bbox));
                *dirty_mask |=
                    crate::utils::get_overlapping_monitors(&seg_bbox, &state.placements);
            }

            if let Some(ann) = state.pending.as_ref() {
                let new_tail_bbox = ann.pen_active_tail_bbox();
                state.damage_rects.push(DamageZone::Global(new_tail_bbox));
                *dirty_mask |=
                    crate::utils::get_overlapping_monitors(&new_tail_bbox, &state.placements);
            }
        } else if let Some(ann) = state.pending.as_mut() {
            if let AnnotationShape::Pen { points } = &mut ann.shape {
                points.push(smoothed);
            }
            ann.update_bbox();
        }
    }
}
