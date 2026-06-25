use crate::types::{MagnifierState, Placement, HANDLE_RADIUS};
use crate::types::toolbar::TOOLBAR_HEIGHT;
use tiny_skia::Rect;
use crate::renderer::{self};
use crate::tools::selection::{global_selection_to_local};
use crate::types::annotations::{Annotation, AnnotationShape};
use crate::editor::EditorState;


impl EditorState {
    // for maximal optimization we render only a specific zone of the screen 
    // instead of entire screen 
    // entire function take what have changed, and add to dirty rectangle what need to be deleted
    // and what need to be added 
    pub fn monitor_dirty_rect(&self,monitor_idx: usize,selection_dirty: bool) -> Option<Rect> {
        let mut dirty: Option<Rect> = None;
        let placement = &self.placements[monitor_idx];

        dirty = union_rect(dirty, self.calc_selection_dirty(placement, selection_dirty));
        dirty = union_rect(dirty, self.calc_magnifier_dirty(monitor_idx, placement));
        dirty = union_rect(dirty, self.calc_toolbar_dirty(monitor_idx));
        dirty = union_rect(dirty, self.calc_annotations_dirty(placement));

        dirty
    }

    fn calc_selection_dirty(&self, placement: &Placement, selection_dirty: bool) -> Option<Rect> {
        if !selection_dirty { return None;}
        let mut dirty = None;
        let selection_pad = (HANDLE_RADIUS as f32).max(4.0);
        
        let local_sel = self.selection.zone
            .as_ref()
            .and_then(|sel| global_selection_to_local(sel, placement));
            
        let prev_local = self.selection.prev_zone
            .as_ref()
            .and_then(|sel| global_selection_to_local(sel, placement));

        if let Some(r) = local_sel.as_ref().and_then(|sel| expand_rect(sel, selection_pad)) {
            dirty = union_rect(dirty, Some(r));
        }
        if let Some(r) = prev_local.as_ref().and_then(|sel| expand_rect(sel, selection_pad)) {
            dirty = union_rect(dirty, Some(r));
        }
        dirty
    }


    fn calc_magnifier_dirty(&self, monitor_idx: usize, placement: &Placement) -> Option<Rect> {
        let mut dirty = None;
        let (mw, mh) = (placement.size.0 as f32, placement.size.1 as f32);
        if mw > 0.0 && mh > 0.0 {
            let mag_pad = 2.0;
            
            let mut add_mag_dirty = |mag_state: &Option<MagnifierState>| {
                if let Some(mag) = mag_state.as_ref().filter(|m| m.monitor_idx == monitor_idx) {
                    let rect = renderer::magnifier_rect((mag.pos.0 as f32, mag.pos.1 as f32), mw, mh);
                    if let Some(r) = expand_rect(&rect, mag_pad) {
                        dirty = union_rect(dirty, Some(r));
                    }
                }
            };

            add_mag_dirty(&self.magnifier);
            add_mag_dirty(&self.prev_magnifier);
        }
        dirty
    }

    
    fn calc_toolbar_dirty(&self, monitor_idx: usize) -> Option<Rect> {
        let tb = &self.toolbar;
        let mut dirty = None;
        if tb.dirty {
            if tb.monitor_idx == monitor_idx {
                if let Some(r) = Rect::from_xywh(tb.position.0, tb.render_y, tb.size.0, TOOLBAR_HEIGHT) {
                    dirty = union_rect(dirty, Some(r));
                }

                // if toolbar position (side) changed
                if tb.prev_monitor_idx == monitor_idx && tb.prev_position != tb.position {
                    if let Some(r) = Rect::from_xywh(tb.prev_position.0, tb.prev_position.1, tb.size.0, TOOLBAR_HEIGHT) {
                        dirty = union_rect(dirty, Some(r));
                    }
                }
            }
            // if toolbar monitor changed
            if tb.prev_monitor_idx == monitor_idx && tb.prev_monitor_idx != tb.monitor_idx {
                if let Some(r) = Rect::from_xywh(tb.prev_position.0, tb.prev_position.1, tb.size.0, TOOLBAR_HEIGHT) {
                    dirty = union_rect(dirty, Some(r));
                }
            }
        }
        dirty
    }


    fn calc_annotations_dirty(&self, placement: &Placement) -> Option<Rect> {
        let mut dirty = None;
        let offset = (placement.position.0 as f32, placement.position.1 as f32);
        let pad = 4.0;

        let mut add_global_rect_dirty = |global_bbox: &Rect| {
            if let Some(local) = Rect::from_ltrb(
                global_bbox.left()   - offset.0,
                global_bbox.top()    - offset.1,
                global_bbox.right()  - offset.0,
                global_bbox.bottom() - offset.1,
            ) {
                if let Some(r) = expand_rect(&local, pad) {
                    dirty = union_rect(dirty, Some(r));
                }
            }
        };

        if let Some(ann) = &self.pending {
            if !matches!(ann.shape, AnnotationShape::Pen { .. }) {
                add_global_rect_dirty(&ann.bbox);
            }
        }
        if let Some(ann) = &self.prev_pending {
            if !matches!(ann.shape, AnnotationShape::Pen { .. }) {
                add_global_rect_dirty(&ann.bbox);
            }
        }

        // undo & redo & pen (to avoid updating its whole bbox)
        for damage_bbox in &self.damage_rects {
            add_global_rect_dirty(damage_bbox);
        }

        dirty
    }


    pub fn record_history_damage(
        damage_rects: &mut Vec<Rect>, 
        state_a: &[Annotation], 
        state_b: &[Annotation]
    ) {
        for ann in state_a {
            if !state_b.iter().any(|a| a.id == ann.id) {
                if let Some(expanded) = expand_rect(&ann.bbox, ann.stroke_width * 2.0 + 4.0) {
                    damage_rects.push(expanded);
                }
            }
        }
        
        for ann in state_b {
            if !state_a.iter().any(|a| a.id == ann.id) {
                if let Some(expanded) = expand_rect(&ann.bbox, ann.stroke_width * 2.0 + 4.0) {
                    damage_rects.push(expanded);
                }
            }
        }
    }
}




fn expand_rect(rect: &Rect, pad: f32) -> Option<Rect> {
    Rect::from_ltrb(
        rect.left() - pad,
        rect.top() - pad,
        rect.right() + pad,
        rect.bottom() + pad,
    )
}

fn union_rect(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (None, None) => None,
        (Some(r), None) | (None, Some(r)) => Some(r),
        (Some(r1), Some(r2)) => Rect::from_ltrb(
            r1.left().min(r2.left()),
            r1.top().min(r2.top()),
            r1.right().max(r2.right()),
            r1.bottom().max(r2.bottom()),
        ),
    }
}

