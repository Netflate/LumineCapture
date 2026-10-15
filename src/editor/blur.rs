use std::collections::HashMap;
use tiny_skia::{IntRect, Pixmap, Rect};

use crate::editor::EditorState;
use crate::editor::dirty::expand_rect;
use crate::renderer::blur::{self, Blurred};
use crate::renderer::paths::normalized_rect;
use crate::types::{Annotation, AnnotationShape};
use crate::utils::{get_full_workspace_rect, rects_overlap};

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
    below: usize,
    radius: f32,
    origin: (i32, i32),
    blurred: Blurred,
}

impl EditorState {
    pub fn cache_pending_blur(&mut self) {
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
            .is_none_or(|base| base.below != below || base.radius != radius)
        {
            self.blur.base = self.build_blur_base(below, radius);
        }

        let (Some(base), Some(at)) = (&self.blur.base, at) else {
            return;
        };
        if let Some(pixmap) = IntRect::from_xywh(
            at.x() - base.origin.0,
            at.y() - base.origin.1,
            at.width(),
            at.height(),
        )
        .and_then(|local| base.blurred.crop(local))
        {
            self.blur.cache.insert(id, pixmap);
        }
    }

    pub fn cache_blur(&mut self, ann: &Annotation, below: usize) {
        if let Some(pixmap) = self.render_blur(ann, below) {
            self.blur.cache.insert(ann.id, pixmap);
        }
    }

    fn build_blur_base(&mut self, below: usize, radius: f32) -> Option<Base> {
        let workspace = get_full_workspace_rect(&self.placements)?;
        let (src, origin) = self.blur_source(workspace, below)?;
        let whole = IntRect::from_xywh(0, 0, src.width(), src.height())?;
        Some(Base {
            below,
            radius,
            origin,
            blurred: Blurred::new(&src, whole, radius, (0, 0))?,
        })
    }

    fn render_blur(&mut self, ann: &Annotation, below: usize) -> Option<Pixmap> {
        let at = blur_rect(ann)?;
        let workspace = get_full_workspace_rect(&self.placements)?;
        let region =
            expand_rect(&at.to_rect(), blur::margin(ann.stroke_width))?.intersect(&workspace)?;
        let (src, origin) = self.blur_source(region, below)?;

        let local = IntRect::from_xywh(
            at.x() - origin.0,
            at.y() - origin.1,
            at.width(),
            at.height(),
        )?;
        // same grid as the base
        let phase = (
            origin.0 - workspace.left() as i32,
            origin.1 - workspace.top() as i32,
        );
        Blurred::new(&src, local, ann.stroke_width, phase)?.crop(local)
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
