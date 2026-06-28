use crate::tools::ToolBehavior;
use crate::types::{MouseButton, AnnDragState, SelectionHandle, HANDLE_PAD};
use crate::editor::{EditorState};
use crate::utils::{hit_test_rect_handle, apply_handle_drag};
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
            // nothing were selected, nothing is selected -> Nothing to do
            if selected_annotation == None && state.selected_annotation == None {
                return;
            }
            
            *dirty_mask = u32::MAX; 

            // Selecting nothing to unselected selected annotation
            if selected_annotation == None && let Some(old_idx) = state.selected_annotation {
                let old_ann = &state.annotations[old_idx];
                state.damage_rects.push(old_ann.bbox);
                state.selected_annotation = None;
                state.ann_drag = None;
                return;
            }

            // selecting new annotation
            if state.selected_annotation != selected_annotation && let Some(idx) = selected_annotation {
                if let Some(old_idx) = state.selected_annotation {
                    let old_ann = &state.annotations[old_idx];
                    state.damage_rects.push(old_ann.bbox); 
                }
                state.selected_annotation = selected_annotation;
                let ann = &state.annotations[idx];
                let handle = SelectionHandle::Move;
                        
                state.ann_drag = Some(AnnDragState {
                    handle,
                    start_global: state.pointer.global,
                    orig: ann.clone(),
                });
                return;
            }

            // selecting the same one
            if let Some(idx) = state.selected_annotation && state.selected_annotation == selected_annotation {
                let ann = &state.annotations[idx];
                
                let padded = Rect::from_ltrb(
                    ann.bbox.left()     + HANDLE_PAD,
                    ann.bbox.top()       + HANDLE_PAD,
                    ann.bbox.right()   - HANDLE_PAD,
                    ann.bbox.bottom() - HANDLE_PAD,
                ).unwrap(); 

                let handle = hit_test_rect_handle(&padded, state.pointer.global);
                        
                state.ann_drag = Some(AnnDragState {
                    handle,
                    start_global: state.pointer.global,
                    orig: ann.clone(),
                });
                return;
            }
        } else {
            if state.ann_drag.is_some() {
                state.push_undo();
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
                let new_bbox = apply_handle_drag(&orig.bbox, handle, (total_dx as f64, total_dy as f64));
                orig.resize_to_bbox(new_bbox)
            }
        };

        let idx = state.selected_annotation.unwrap();
        
        state.damage_rects.push(state.annotations[idx].bbox);
        state.damage_rects.push(updated.bbox);
        
        state.annotations[idx] = updated;
        *dirty_mask = u32::MAX;
    }
    
    fn on_deactivate(&self, state: &mut EditorState, dirty_mask: &mut u32) {
        if state.selected_annotation.is_some() {
            *dirty_mask |= u32::MAX;
            state.selected_annotation = None;
            state.ann_drag = None;
        }
    }
}