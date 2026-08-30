use crate::editor::{DamageZone, EditorState};
use crate::tools::ToolBehavior;
use crate::types::{
    AnnDragState, MouseButton, SelectionHandle, SpecialKey,
    annotations::{apply_annotation_drag, begin_drag_for_annotation, commit_drag_if_changed},
};
use cosmic_text::Edit;

pub struct PickTool;

impl ToolBehavior for PickTool {
    fn on_button(
        &self,
        state: &mut EditorState,
        button: MouseButton,
        pressed: bool,
        _dirty_mask: &mut u32,
    ) {
        if !matches!(button, MouseButton::Left) {
            return;
        }

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

            // select empty space > deselect
            if selected_annotation.is_none() {
                if let Some(old_idx) = state.selected_annotation {
                    if let Some(old_ann) = state.annotations.get(old_idx) {
                        state
                            .damage_rects
                            .push(DamageZone::Global(old_ann.damage_bbox(true)));
                        state.layer_damage_rects.push(old_ann.damage_bbox(false));
                        if let Some(editor) = state.text_editors.get_mut(&old_ann.id) {
                            editor.set_selection(cosmic_text::Selection::None);
                        }
                    }
                }
                state.selected_annotation = None;
                state.ann_drag = None;
                state.text_editing = None;
                state.annotations_dirty = true;
                return;
            }

            // select a different annotation -> switch selection, no undo commit
            if state.selected_annotation != selected_annotation {
                if let Some(old_idx) = state.selected_annotation {
                    if let Some(old_ann) = state.annotations.get(old_idx) {
                        state
                            .damage_rects
                            .push(DamageZone::Global(old_ann.damage_bbox(true)));
                        state.layer_damage_rects.push(old_ann.damage_bbox(false));
                        if let Some(editor) = state.text_editors.get_mut(&old_ann.id) {
                            editor.set_selection(cosmic_text::Selection::None);
                        }
                    }
                }
                state.selected_annotation = selected_annotation;
                let idx = selected_annotation.unwrap();
                let ann = &state.annotations[idx];
                state
                    .damage_rects
                    .push(DamageZone::Global(ann.damage_bbox(true)));
                state.text_editing = None;

                // always start as Move when switching selection
                state.ann_drag = Some(AnnDragState {
                    handle: SelectionHandle::Move,
                    start_global: state.pointer.global,
                    prev_global: state.pointer.global,
                    orig: ann.clone(),
                    orig_index: idx,
                });
                return;
            }

            // click the same annotation > handle hit test (Move or resize), push undo before drag
            if let Some(idx) = state.selected_annotation {
                begin_drag_for_annotation(state, idx);
            }
        } else {
            commit_drag_if_changed(state);
        }
    }

    fn on_move(&self, state: &mut EditorState, global: (f64, f64), _dirty_mask: &mut u32) {
        apply_annotation_drag(state, global);
    }

    fn on_key(&self, state: &mut EditorState, key: SpecialKey, dirty_mask: &mut u32) {
        if matches!(key, SpecialKey::Delete | SpecialKey::Backspace) {
            if let Some(idx) = state.selected_annotation.take() {
                if idx < state.annotations.len() {
                    state.push_undo();
                    let ann = state.annotations.remove(idx);
                    state
                        .damage_rects
                        .push(DamageZone::Global(ann.damage_bbox(true)));
                    state.layer_damage_rects.push(ann.damage_bbox(false));
                    state.ann_drag = None;
                    state.annotations_dirty = true;
                    *dirty_mask = u32::MAX;
                }
            }
        }
    }

    fn on_deactivate(&self, state: &mut EditorState, _dirty_mask: &mut u32) {
        if let Some(idx) = state.selected_annotation {
            state
                .damage_rects
                .push(DamageZone::Global(state.annotations[idx].damage_bbox(true)));
            state.selected_annotation = None;
            state.ann_drag = None;
        }
    }
}
