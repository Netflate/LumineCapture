use crate::tools::ToolBehavior;
use crate::types::{MouseButton, AnnDragState};
use crate::editor::{EditorState};
use crate::utils::hit_test_rect_handle;

pub struct PickTool;

impl ToolBehavior for PickTool {
    fn on_button(&self, state: &mut EditorState, button: MouseButton, pressed: bool, dirty_mask: &mut u32) {
        if !matches!(button, MouseButton::Left) { return; }

        if pressed {
            if let Some(idx) = state.selected_annotation {
                let ann = &state.annotations[idx];
                if let Some(handle) = hit_test_rect_handle(&ann.bbox, state.pointer.global) {
                    state.ann_drag = Some(AnnDragState {
                        handle,
                        start_global: state.pointer.global,
                        orig: ann.clone(),
                    });
                    return;
                }
            }

            state.selected_annotation = None;
            state.ann_drag = None;
            for (i, ann) in state.annotations.iter().enumerate().rev() {
                if bbox_contains(&ann.bbox, state.pointer.global) {
                    state.selected_annotation = Some(i);
                    break;
                }
            }
            mark_dirty(dirty_mask, state.pointer.monitor_idx);
        } else {
            if state.ann_drag.is_some() {
                state.push_undo();
            }
            state.ann_drag = None;
        }
    }

    fn on_move(&self, _state: &mut EditorState, _global: (f64, f64), _selection_dirty: &mut bool, _dirty_mask: &mut u32) {
        return;
    }
}