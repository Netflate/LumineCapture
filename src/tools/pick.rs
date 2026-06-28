use crate::tools::ToolBehavior;
use crate::types::{MouseButton, AnnDragState, SelectionHandle};
use crate::editor::{EditorState};
use crate::utils::{hit_test_rect_handle, apply_handle_drag};
use crate::types::HANDLE_PAD; 
use tiny_skia::Rect;

pub struct PickTool;

impl ToolBehavior for PickTool {
    fn on_button(&self, state: &mut EditorState, button: MouseButton, pressed: bool, dirty_mask: &mut u32) {
        if !matches!(button, MouseButton::Left) { return; }

        if pressed {
            let mut selected_annotation = None;
            for (i, ann) in state.annotations.iter().enumerate().rev() {
                if ann.initial_hit_test(state.pointer.global) {
                    selected_annotation = Some(i);
                    break;
                }
            }

            // nothing selected, nothing was selected > nothing to do
            if selected_annotation.is_none() && state.selected_annotation.is_none() {
                return;
            }

            *dirty_mask = u32::MAX;

            // select empty space > deselect
            if selected_annotation.is_none() {
                if let Some(old_idx) = state.selected_annotation {
                    state.damage_rects.push(state.annotations[old_idx].damage_bbox(true));
                }
                state.selected_annotation = None;
                state.ann_drag = None;
                return;
            }

            // select a different annotation -> switch selection, no undo commit
            if state.selected_annotation != selected_annotation {
                if let Some(old_idx) = state.selected_annotation {
                    state.damage_rects.push(state.annotations[old_idx].damage_bbox(true));
                }
                state.selected_annotation = selected_annotation;
                let idx = selected_annotation.unwrap();
                let ann = &state.annotations[idx];
                state.damage_rects.push(ann.damage_bbox(true));

                // always start as Move when switching selection
                state.ann_drag = Some(AnnDragState {
                    handle: SelectionHandle::Move,
                    start_global: state.pointer.global,
                    orig: ann.clone(),
                });
                return;
            }

            // click the same annotation > handle hit test (Move or resize), push undo before drag
            if let Some(idx) = state.selected_annotation {
                let out_pad = (HANDLE_PAD / 2.0) as f32;
                let ann_bbox = state.annotations[idx].bbox;
                let ann_clone = state.annotations[idx].clone();

                let visual_handle_bbox = Rect::from_ltrb(
                    ann_bbox.left() - out_pad,
                    ann_bbox.top() - out_pad,
                    ann_bbox.right() + out_pad,
                    ann_bbox.bottom() + out_pad,
                ).unwrap_or(ann_bbox);

                let handle = hit_test_rect_handle(&visual_handle_bbox, state.pointer.global);

                state.ann_drag = Some(AnnDragState {
                    handle,
                    start_global: state.pointer.global,
                    orig: ann_clone,
                });
            }
        } else {
            // mouse up > commit to undo only if something actually changed
            if let Some(drag) = &state.ann_drag {
                if let Some(idx) = state.selected_annotation {
                    let actually_changed = !matches!(drag.handle, SelectionHandle::None)
                        && state.annotations[idx].bbox != drag.orig.bbox;

                    if actually_changed {
                        // annotations[idx] already has the new position from on_move
                        // we reconstruct the pre-drag snapshot using drag.orig
                        let pre_drag: Vec<_> = state.annotations.iter().enumerate()
                            .map(|(i, ann)| if i == idx { drag.orig.clone() } else { ann.clone() })
                            .collect();
                        state.undo_stack.push(pre_drag);
                        state.redo_stack.clear();
                    }
                }
            }
            state.ann_drag = None;
        }
    }

    fn on_move(&self, state: &mut EditorState, global: (f64, f64), _selection_dirty: &mut bool, dirty_mask: &mut u32) {
        let Some(drag) = state.ann_drag.as_ref() else { return };

        let total_dx = (global.0 - drag.start_global.0) as f32;
        let total_dy = (global.1 - drag.start_global.1) as f32;
        let handle = drag.handle;
        let orig = drag.orig.clone();

        let updated = match handle {
            SelectionHandle::Move => orig.translate(total_dx, total_dy),
            SelectionHandle::None => return,
            _ => {
                let out_pad = (HANDLE_PAD / 2.0) as f32;
                let visual_handle_bbox = Rect::from_ltrb(
                    orig.bbox.left() - out_pad,
                    orig.bbox.top() - out_pad,
                    orig.bbox.right() + out_pad,
                    orig.bbox.bottom() + out_pad,
                ).unwrap_or(orig.bbox);

                let new_visual_bbox = apply_handle_drag(&visual_handle_bbox, handle, (total_dx as f64, total_dy as f64));

                let clean_bbox = crate::types::SignedRect {
                    left: new_visual_bbox.left + out_pad,
                    top: new_visual_bbox.top + out_pad,
                    right: new_visual_bbox.right - out_pad,
                    bottom: new_visual_bbox.bottom - out_pad,
                };

                orig.resize_to_bbox(clean_bbox)
            }
        };

        let idx = state.selected_annotation.unwrap();

        state.damage_rects.push(state.annotations[idx].damage_bbox(true));
        state.damage_rects.push(updated.damage_bbox(true));

        state.annotations[idx] = updated;
        *dirty_mask = u32::MAX;
    }

    fn on_deactivate(&self, state: &mut EditorState, dirty_mask: &mut u32) {
        if let Some(idx) = state.selected_annotation {
            *dirty_mask |= u32::MAX;
            state.damage_rects.push(state.annotations[idx].damage_bbox(true));
            state.selected_annotation = None;
            state.ann_drag = None;
        }
    }
}