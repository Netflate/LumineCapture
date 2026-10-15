use std::collections::HashMap;
use tiny_skia::{IntRect, Pixmap, Rect};

use crate::editor::dirty::expand_rect;
use crate::editor::{DamageZone, EditorState};
use crate::renderer::blur::{self, Blurred};
use crate::renderer::paths::normalized_rect;
use crate::types::{Annotation, AnnotationShape};
use crate::utils::{get_full_workspace_rect, get_overlapping_monitors, rects_overlap};

// Approach is largely taken from Spectacle (KQuickImageEditor, `Blur::image`)
// everything below is blurred, older annotations included, once per gesture,
// and only then cropped.

#[derive(Default)]
pub struct BlurState {
    pub cache: HashMap<u64, Pixmap>,
    base: Option<Base>,
}

// the whole workspace blurred once per gesture
// every frame only crops from it
struct Base {
    id: u64,
    below: usize,
    radius: f32,
    origin: (i32, i32),
    blurred: Blurred,
}

impl Base {
    fn crop(&self, at: IntRect) -> Option<Pixmap> {
        self.blurred
            .crop(at.translate(-self.origin.0, -self.origin.1)?)
    }
}

impl EditorState {
    // bottom to top, so a recomputed blur also damages blurs above it; the margin counts too,
    // since pixels outside the rect still bleed into it
    pub fn refresh_blurs(&mut self, dirty_mask: &mut u32) {
        self.blur
            .cache
            .retain(|id, _| self.annotations.iter().any(|ann| ann.id == *id));

        for i in 0..self.annotations.len() {
            let ann = &self.annotations[i];
            let Some(at) = blur_rect(ann) else {
                continue;
            };
            let margin = blur::margin(ann.stroke_width);
            let parts: Vec<IntRect> = match self.blur.cache.get(&ann.id) {
                Some(cached) if cached.width() == at.width() && cached.height() == at.height() => {
                    self.layer_damage_rects
                        .iter()
                        .filter_map(|rect| expand_rect(rect, margin)?.round_out()?.intersect(&at))
                        .collect()
                }
                _ => vec![at],
            };
            if parts.is_empty() {
                continue;
            }
            let ann = ann.clone();
            for part in parts {
                self.cache_blur_part(&ann, i, at, part);
                let rect = part.to_rect();
                self.layer_damage_rects.push(rect);
                self.damage_rects.push(DamageZone::Global(rect));
                *dirty_mask |= get_overlapping_monitors(&rect, &self.placements);
            }
        }

        self.cache_pending_blur();
    }

    pub fn cache_blur(&mut self, ann: &Annotation, below: usize) {
        if let Some(at) = blur_rect(ann) {
            self.cache_blur_part(ann, below, at, at);
        }
    }

    pub fn blur_area(&self, sel: Rect, scale: f32) -> Rect {
        let Some(workspace) = get_full_workspace_rect(&self.placements) else {
            return sel;
        };
        let mut area = sel;
        let mut grown = true;
        while grown {
            grown = false;
            for ann in &self.annotations {
                let Some(at) = blur_rect(ann).map(|at| at.to_rect()) else {
                    continue;
                };
                let margin = blur::margin(ann.stroke_width * scale) / scale;
                if rects_overlap(&at, &area)
                    && let Some(next) = expand_rect(&at, margin)
                        .and_then(|need| need.intersect(&workspace))
                        .and_then(|need| need.join(&area))
                    && next != area
                {
                    area = next;
                    grown = true;
                }
            }
        }
        area
    }

    fn cache_pending_blur(&mut self) {
        let Some(ann) = self
            .pending
            .as_ref()
            .filter(|ann| matches!(ann.shape, AnnotationShape::Blur { .. }))
        else {
            self.blur.base = None;
            return;
        };
        let (id, radius, at) = (ann.id, ann.stroke_width, blur_rect(ann));
        // while dragging, the annotation itself is out of the list, only what was below it stays
        let below = self
            .ann_drag
            .as_ref()
            .map_or(self.annotations.len(), |drag| {
                drag.orig_index.min(self.annotations.len())
            });
        if self
            .blur
            .base
            .as_ref()
            .is_none_or(|base| (base.id, base.below, base.radius) != (id, below, radius))
        {
            self.blur.base = self.build_blur_base(id, below, radius);
        }

        if let Some(pixmap) = at
            .zip(self.blur.base.as_ref())
            .and_then(|(at, base)| base.crop(at))
        {
            self.blur.cache.insert(id, pixmap);
        }
    }

    fn cache_blur_part(&mut self, ann: &Annotation, below: usize, at: IntRect, part: IntRect) {
        let Some(pixmap) = self.render_blur(ann, below, part) else {
            return;
        };
        match self.blur.cache.get_mut(&ann.id) {
            Some(cached) if part != at => {
                blur::paste(cached, &pixmap, part.x() - at.x(), part.y() - at.y());
            }
            _ => {
                self.blur.cache.insert(ann.id, pixmap);
            }
        }
    }

    fn build_blur_base(&mut self, id: u64, below: usize, radius: f32) -> Option<Base> {
        let workspace = get_full_workspace_rect(&self.placements)?;
        let (src, origin) = self.blur_source(workspace, below)?;
        let whole = IntRect::from_xywh(0, 0, src.width(), src.height())?;
        Some(Base {
            id,
            below,
            radius,
            origin,
            blurred: Blurred::new(&src, whole, radius, origin)?,
        })
    }

    fn render_blur(&mut self, ann: &Annotation, below: usize, at: IntRect) -> Option<Pixmap> {
        // right after the gesture we have base, which is cheaper to crop than to recompute
        if let Some(base) = &self.blur.base
            && (base.id, base.below, base.radius) == (ann.id, below, ann.stroke_width)
        {
            return base.crop(at);
        }
        let workspace = get_full_workspace_rect(&self.placements)?;
        let region =
            expand_rect(&at.to_rect(), blur::margin(ann.stroke_width))?.intersect(&workspace)?;
        let (src, origin) = self.blur_source(region, below)?;
        let local = at.translate(-origin.0, -origin.1)?;
        Blurred::new(&src, local, ann.stroke_width, origin)?.crop(local)
    }

    fn blur_source(&mut self, region: Rect, below: usize) -> Option<(Pixmap, (i32, i32))> {
        let (mut src, origin, _) =
            crate::renderer::composite(&self.captures, &self.placements, region, Some(1.0))?;
        for other in &self.annotations[..below] {
            if rects_overlap(&other.damage_bbox(false), &region) {
                crate::renderer::draw_annotation(
                    &mut src,
                    other,
                    (origin.0 as f32, origin.1 as f32),
                    false,
                    &mut self.text.font_system,
                    &mut self.text.swash_cache,
                    &mut self.text.editors,
                    &self.blur.cache,
                    None,
                );
            }
        }
        Some((src, origin))
    }
}

fn blur_rect(ann: &Annotation) -> Option<IntRect> {
    let AnnotationShape::Blur { start, end } = &ann.shape else {
        return None;
    };
    normalized_rect(*start, *end)?.round_out()
}
