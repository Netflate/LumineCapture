use crate::editor::dirty::damage_annotation;
use crate::editor::{DamageZone, EditorState};
use crate::types::Annotation;

enum Step {
    Undo,
    Redo,
}

impl EditorState {
    pub fn push_undo(&mut self) {
        self.undo_stack.push(self.annotations.clone());
        self.redo_stack.clear();
    }

    pub fn commit_snapshot(&mut self, snapshot: Option<Vec<Annotation>>) {
        if let Some(snapshot) = snapshot
            && self.annotations != snapshot
        {
            self.undo_stack.push(snapshot);
            self.redo_stack.clear();
        }
    }

    pub fn undo(&mut self, dirty_mask: &mut u32) {
        if self.cancel_unfinished_edit(dirty_mask) {
            return;
        }
        self.step_history(Step::Undo, dirty_mask);
    }

    pub fn redo(&mut self, dirty_mask: &mut u32) {
        if self.pending.is_some() || self.ann_drag.is_some() {
            return;
        }
        self.step_history(Step::Redo, dirty_mask);
    }

    fn cancel_unfinished_edit(&mut self, dirty_mask: &mut u32) -> bool {
        if let Some(drag) = self.ann_drag.take() {
            if let Some(ann) = self.pending.take() {
                damage_annotation(&mut self.damage_rects, &mut self.layer_damage_rects, &ann);
                let insert_idx = drag.orig_index.min(self.annotations.len());
                damage_annotation(
                    &mut self.damage_rects,
                    &mut self.layer_damage_rects,
                    &drag.orig,
                );
                self.bake_annotation(&drag.orig);
                self.annotations.insert(insert_idx, drag.orig);
                self.selected_annotation = Some(insert_idx);
            }
            self.prev_pending = None;
            self.annotations_dirty = true;
            *dirty_mask = u32::MAX;
            return true;
        }

        if let Some(ann) = self.pending.take() {
            self.damage_rects
                .push(DamageZone::Global(ann.damage_bbox(true)));
            self.prev_pending = None;
            self.selected_annotation = None;
            self.pending_pen_baked = 0;
            self.annotations_dirty = true;
            *dirty_mask = u32::MAX;
            return true;
        }

        false
    }

    fn step_history(&mut self, step: Step, dirty_mask: &mut u32) {
        // Commit any uncommitted settings/color stepper or slider snapshot
        let settings_snapshot = self.settings_panel.pre_edit_snapshot.take();
        self.commit_snapshot(settings_snapshot);
        let color_snapshot = self.color_popover.pre_edit_snapshot.take();
        self.commit_snapshot(color_snapshot);

        let selected_id = self
            .selected_annotation
            .and_then(|sel_idx| self.annotations.get(sel_idx))
            .map(|ann| ann.id);

        let restored = match step {
            Step::Undo => self.undo_stack.pop(),
            Step::Redo => self.redo_stack.pop(),
        };
        let Some(restored) = restored else {
            return;
        };

        if let Some(sel_idx) = self.selected_annotation
            && let Some(ann) = self.annotations.get(sel_idx)
        {
            self.damage_rects
                .push(DamageZone::Global(ann.damage_bbox(true)));
        }
        Self::record_history_damage(
            &mut self.damage_rects,
            &mut self.layer_damage_rects,
            &self.annotations,
            &restored,
        );

        let replaced = std::mem::replace(&mut self.annotations, restored);
        match step {
            Step::Undo => self.redo_stack.push(replaced),
            Step::Redo => self.undo_stack.push(replaced),
        }

        self.reselect_by_id(selected_id);

        self.ann_drag = None;
        self.text.editing = None;
        self.annotations_dirty = true;

        self.relayout_text_annotations();

        *dirty_mask = u32::MAX;
    }

    /// Annotations returns as copies, selection is followed by id.
    fn reselect_by_id(&mut self, selected_id: Option<u64>) {
        self.selected_annotation =
            selected_id.and_then(|id| self.annotations.iter().position(|ann| ann.id == id));

        if let Some(idx) = self.selected_annotation {
            self.damage_rects
                .push(DamageZone::Global(self.annotations[idx].damage_bbox(true)));
        }
    }

    fn relayout_text_annotations(&mut self) {
        for ann in &mut self.annotations {
            if matches!(ann.shape, crate::types::AnnotationShape::Text { .. }) {
                let editor = crate::tools::text::ensure_text_editor(
                    ann,
                    &mut self.text.editors,
                    &mut self.text.font_system,
                );
                crate::tools::text::update_text_bbox_inline(
                    ann,
                    editor,
                    &mut self.text.font_system,
                );
            }
        }
    }
}
