use crate::editor::{DamageZone, EditorState};

impl EditorState {
    pub fn push_undo(&mut self) {
        self.undo_stack.push(self.annotations.clone());
        self.redo_stack.clear();
    }

    pub fn undo(&mut self, dirty_mask: &mut u32) {
        // If a drag of an existing annotation is currently happening, cancel it and restore original
        if let Some(drag) = self.ann_drag.take() {
            if let Some(ann) = self.pending.take() {
                self.damage_rects
                    .push(DamageZone::Global(ann.damage_bbox(true)));
                self.layer_damage_rects.push(ann.damage_bbox(false));
                let insert_idx = drag.orig_index.min(self.annotations.len());
                self.damage_rects
                    .push(DamageZone::Global(drag.orig.damage_bbox(true)));
                self.layer_damage_rects.push(drag.orig.damage_bbox(false));
                self.bake_annotation(&drag.orig);
                self.annotations.insert(insert_idx, drag.orig);
                self.selected_annotation = Some(insert_idx);
            }
            self.prev_pending = None;
            self.annotations_dirty = true;
            *dirty_mask = u32::MAX;
            return;
        }

        // If a new pending shape is in progress, cancel it
        if let Some(ann) = self.pending.take() {
            self.damage_rects
                .push(DamageZone::Global(ann.damage_bbox(true)));
            self.prev_pending = None;
            self.selected_annotation = None;
            self.annotations_dirty = true;
            *dirty_mask = u32::MAX;
            return;
        }

        // Commit any uncommitted settings/color stepper or slider snapshot
        if let Some(snapshot) = self.settings_panel.pre_edit_snapshot.take() {
            if self.annotations != snapshot {
                self.undo_stack.push(snapshot);
                self.redo_stack.clear();
            }
        }
        if let Some(snapshot) = self.color_popover.pre_edit_snapshot.take() {
            if self.annotations != snapshot {
                self.undo_stack.push(snapshot);
                self.redo_stack.clear();
            }
        }

        let selected_id = self
            .selected_annotation
            .and_then(|sel_idx| self.annotations.get(sel_idx))
            .map(|ann| ann.id);

        if let Some(prev_state) = self.undo_stack.pop() {
            if let Some(sel_idx) = self.selected_annotation {
                if let Some(ann) = self.annotations.get(sel_idx) {
                    self.damage_rects
                        .push(DamageZone::Global(ann.damage_bbox(true)));
                }
            }
            Self::record_history_damage(
                &mut self.damage_rects,
                &mut self.layer_damage_rects,
                &self.annotations,
                &prev_state,
            );
            self.redo_stack.push(self.annotations.clone());
            self.annotations = prev_state;

            if let Some(sel_id) = selected_id {
                if let Some(new_idx) = self.annotations.iter().position(|a| a.id == sel_id) {
                    self.selected_annotation = Some(new_idx);
                    self.damage_rects.push(DamageZone::Global(
                        self.annotations[new_idx].damage_bbox(true),
                    ));
                } else {
                    self.selected_annotation = None;
                }
            } else {
                self.selected_annotation = None;
            }

            self.ann_drag = None;
            self.text_editing = None;
            self.annotations_dirty = true;

            for ann in &mut self.annotations {
                if matches!(ann.shape, crate::types::AnnotationShape::Text { .. }) {
                    let editor = crate::tools::text::ensure_text_editor(
                        ann,
                        &mut self.text_editors,
                        &mut self.font_system,
                    );
                    crate::tools::text::update_text_bbox_inline(ann, editor, &mut self.font_system);
                }
            }

            *dirty_mask = u32::MAX;
        }
    }

    pub fn redo(&mut self, dirty_mask: &mut u32) {
        if self.pending.is_some() || self.ann_drag.is_some() {
            return;
        }

        // Commit any uncommitted settings/color stepper or slider snapshot
        if let Some(snapshot) = self.settings_panel.pre_edit_snapshot.take() {
            if self.annotations != snapshot {
                self.undo_stack.push(snapshot);
                self.redo_stack.clear();
            }
        }
        if let Some(snapshot) = self.color_popover.pre_edit_snapshot.take() {
            if self.annotations != snapshot {
                self.undo_stack.push(snapshot);
                self.redo_stack.clear();
            }
        }

        let selected_id = self
            .selected_annotation
            .and_then(|sel_idx| self.annotations.get(sel_idx))
            .map(|ann| ann.id);

        if let Some(next_state) = self.redo_stack.pop() {
            if let Some(sel_idx) = self.selected_annotation {
                if let Some(ann) = self.annotations.get(sel_idx) {
                    self.damage_rects
                        .push(DamageZone::Global(ann.damage_bbox(true)));
                }
            }
            Self::record_history_damage(
                &mut self.damage_rects,
                &mut self.layer_damage_rects,
                &self.annotations,
                &next_state,
            );

            self.undo_stack.push(self.annotations.clone());
            self.annotations = next_state;

            if let Some(sel_id) = selected_id {
                if let Some(new_idx) = self.annotations.iter().position(|a| a.id == sel_id) {
                    self.selected_annotation = Some(new_idx);
                    self.damage_rects.push(DamageZone::Global(
                        self.annotations[new_idx].damage_bbox(true),
                    ));
                } else {
                    self.selected_annotation = None;
                }
            } else {
                self.selected_annotation = None;
            }

            self.ann_drag = None;
            self.text_editing = None;
            self.annotations_dirty = true;

            for ann in &mut self.annotations {
                if matches!(ann.shape, crate::types::AnnotationShape::Text { .. }) {
                    let editor = crate::tools::text::ensure_text_editor(
                        ann,
                        &mut self.text_editors,
                        &mut self.font_system,
                    );
                    crate::tools::text::update_text_bbox_inline(ann, editor, &mut self.font_system);
                }
            }

            *dirty_mask = u32::MAX;
        }
    }
}
