use crate::editor::{EditorState, DamageZone};
use crate::renderer::{self};
use crate::tools::selection::global_selection_to_local;
use crate::types::annotations::{Annotation, AnnotationShape};
use crate::types::{HANDLE_RADIUS, MagnifierState, Placement};
use crate::utils::get_overlapping_monitors;

use tiny_skia::Rect;

impl EditorState {
    pub fn monitor_dirty_rect(&self, monitor_idx: usize) -> Option<Rect> {
        let placement = &self.placements[monitor_idx];
        let mut dirty: Option<Rect> = None;

        dirty = union_rect(dirty, self.calc_selection_dirty(placement));
        dirty = union_rect(dirty, self.calc_magnifier_dirty(monitor_idx, placement));
        //dirty = union_rect(dirty, self.calc_toolbar_dirty(monitor_idx));
        dirty = union_rect(dirty, self.calc_damage_zones_dirty(monitor_idx, placement));

        dirty
    }

    fn calc_selection_dirty(&self, placement: &Placement) -> Option<Rect> {
        if self.selection.zone == self.selection.prev_zone {
            return None;
        }
        let mut dirty = None;
        let selection_pad = (HANDLE_RADIUS as f32).max(4.0);

        let local_sel = self
            .selection
            .zone
            .as_ref()
            .and_then(|sel| global_selection_to_local(sel, placement));

        let prev_local = self
            .selection
            .prev_zone
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
                    let rect =
                        renderer::magnifier_rect((mag.pos.0 as f32, mag.pos.1 as f32), mw, mh);
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

    fn calc_damage_zones_dirty(&self, monitor_idx: usize, placement: &Placement) -> Option<Rect> {
        let mut dirty = None;
        let offset = (placement.position.0 as f32, placement.position.1 as f32);
        let pad = 4.0;

        fn global_to_local_padded(global_bbox: &Rect, offset: (f32, f32), pad: f32) -> Option<Rect> {
            Rect::from_ltrb(
                global_bbox.left() - offset.0,
                global_bbox.top() - offset.1,
                global_bbox.right() - offset.0,
                global_bbox.bottom() - offset.1,
            )
            .and_then(|local| expand_rect(&local, pad))
        }

        if let Some(ann) = &self.pending
            && !matches!(ann.shape, AnnotationShape::Pen { .. })
        {
            dirty = union_rect(dirty, global_to_local_padded(&ann.bbox, offset, pad));
        }
        if let Some(ann) = &self.prev_pending
            && !matches!(ann.shape, AnnotationShape::Pen { .. })
        {
            dirty = union_rect(dirty, global_to_local_padded(&ann.bbox, offset, pad));
        }
        if let Some(ann) = self.selected_annotation {
            dirty = union_rect(dirty, global_to_local_padded(&self.annotations[ann].bbox, offset, pad));
        }

        for zone in &self.damage_rects {
            match zone {
                DamageZone::Global(rect) => {
                    dirty = union_rect(dirty, global_to_local_padded(rect, offset, pad));
                }
                DamageZone::Local { monitor_idx: idx, rect } if *idx == monitor_idx => {
                    dirty = union_rect(dirty, expand_rect(rect, pad));
                }
                DamageZone::Local { .. } => {}
            }
        }

        dirty
    }

    pub fn record_history_damage(
        damage_rects: &mut Vec<DamageZone>,
        state_a: &[Annotation],
        state_b: &[Annotation],
    ) {
        for ann in state_a {
            if !state_b.iter().any(|a| a.id == ann.id)
                && let Some(expanded) = expand_rect(&ann.bbox, ann.stroke_width * 2.0 + 4.0)
            {
                damage_rects.push(DamageZone::Global(expanded));
            }
        }

        for ann in state_b {
            if !state_a.iter().any(|a| a.id == ann.id)
                && let Some(expanded) = expand_rect(&ann.bbox, ann.stroke_width * 2.0 + 4.0)
            {
                damage_rects.push(DamageZone::Global(expanded));
            }
        }
    }

    // when code push dirty zones in damage_dirty
    // sometimes it pushes local monitor zones (like for toolbar)
    // sometimes global (which is necessary for )
    pub fn damage_global(&mut self, rect: Rect) {
        self.damage_rects.push(DamageZone::Global(rect));
    }

    pub fn damage_local(&mut self, monitor_idx: usize, rect: Rect) {
        self.damage_rects.push(DamageZone::Local { monitor_idx, rect });
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

pub fn mark_dirty(mask: &mut u32, idx: usize) {
    *mask |= 1 << idx;
}

pub fn is_dirty(mask: u32, idx: usize) -> bool {
    (mask & (1 << idx)) != 0
}

pub fn apply_damage_rects(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    for zone in &editor_state.damage_rects {
        match zone {
            DamageZone::Global(rect) => {
                *dirty_mask |= get_overlapping_monitors(rect, &editor_state.placements);
            }
            DamageZone::Local { monitor_idx, .. } => {
                mark_dirty(dirty_mask, *monitor_idx);
            }
        }
    }
}