use crate::editor::dirty::damage_annotation;
use crate::editor::{DamageZone, EditorState};
use crate::types::Annotation;
use tiny_skia::Rect;

enum Step {
    Undo,
    Redo,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub annotations: Vec<Annotation>,
    pub zone: Option<Rect>,
}

impl Snapshot {
    // Decided not to register selection (crop) actions in history in regular overlay mode.
    // However, window mode crops slightly differently (more like a classic crop in Photoshop)
    // so we figured it's better to save it in history here
    fn differs(&self, annotations: &[Annotation], zone: Option<Rect>, crop: bool) -> bool {
        self.annotations != annotations || (crop && self.zone != zone)
    }
}

impl EditorState {
    pub fn snapshot(&self) -> Snapshot {
        self.snapshot_of(self.annotations.clone())
    }

    pub fn snapshot_of(&self, annotations: Vec<Annotation>) -> Snapshot {
        Snapshot {
            annotations,
            zone: self.selection.zone,
        }
    }

    pub fn push_undo(&mut self) {
        let snapshot = self.snapshot();
        self.push_snapshot(snapshot);
    }

    pub fn push_snapshot(&mut self, snapshot: Snapshot) {
        self.undo_stack.push(snapshot);
        self.redo_stack.clear();
    }

    pub fn commit_snapshot(&mut self, snapshot: Option<Snapshot>) {
        if let Some(snapshot) = snapshot
            && snapshot.differs(&self.annotations, self.selection.zone, self.sheet.is_some())
        {
            self.push_snapshot(snapshot);
        }
    }

    pub fn record_crop(&mut self, before: Option<Rect>) {
        if self.sheet.is_some() && self.selection.zone != before {
            let snapshot = Snapshot {
                annotations: self.annotations.clone(),
                zone: before,
            };
            self.push_snapshot(snapshot);
        }
    }

    pub fn begin_crop(&mut self) {
        self.selection.crop_from = self.sheet.is_some().then_some(self.selection.zone);
    }

    pub fn end_crop(&mut self) {
        if let Some(before) = self.selection.crop_from.take() {
            self.record_crop(before);
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
        let Some(Snapshot {
            annotations: restored,
            zone,
        }) = restored
        else {
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

        let before = self.snapshot();
        let replaced = Snapshot {
            annotations: std::mem::replace(&mut self.annotations, restored),
            zone: before.zone,
        };
        match step {
            Step::Undo => self.redo_stack.push(replaced),
            Step::Redo => self.undo_stack.push(replaced),
        }
        if self.sheet.is_some() {
            self.selection.zone = zone;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::AnnotationShape;
    use tiny_skia::Color;

    fn rect(id: u64) -> Annotation {
        Annotation {
            id,
            shape: AnnotationShape::Rectangle {
                start: (0.0, 0.0),
                end: (10.0, 10.0),
                filled: false,
            },
            color: Color::BLACK,
            shadow_color: Color::BLACK,
            stroke_width: 2.0,
            bbox: Rect::from_xywh(0.0, 0.0, 10.0, 10.0).unwrap(),
        }
    }

    fn zone(w: f32) -> Option<Rect> {
        Rect::from_xywh(0.0, 0.0, w, 20.0)
    }

    #[test]
    fn crop_makes_a_step_only_in_the_window() {
        let snapshot = Snapshot {
            annotations: vec![rect(1)],
            zone: zone(100.0),
        };
        assert!(snapshot.differs(&[rect(1)], zone(50.0), true));
        assert!(!snapshot.differs(&[rect(1)], zone(50.0), false));
        assert!(!snapshot.differs(&[rect(1)], zone(100.0), true));
    }

    #[test]
    fn annotations_make_a_step_everywhere() {
        let snapshot = Snapshot {
            annotations: vec![rect(1)],
            zone: zone(100.0),
        };
        assert!(snapshot.differs(&[rect(1), rect(2)], zone(100.0), false));
        assert!(snapshot.differs(&[], zone(100.0), true));
    }

    #[test]
    fn lost_crop_is_a_step() {
        let snapshot = Snapshot {
            annotations: Vec::new(),
            zone: zone(100.0),
        };
        assert!(snapshot.differs(&[], None, true));
    }
}
