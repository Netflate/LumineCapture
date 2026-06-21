use tiny_skia::{Rect, Color};
use crate::utils::get_overlapping_monitors;
use crate::types::Placement;

#[derive(Clone)]
pub enum AnnotationShape {
    Arrow     { start: (f32, f32), end: (f32, f32) },
    Rectangle { start: (f32, f32), end: (f32, f32) },
    Circle    { start: (f32, f32), end: (f32, f32) },
}

impl AnnotationShape {
    pub fn start_point(&self) -> (f32, f32) {
        match self {
            AnnotationShape::Arrow     { start, .. } => *start,
            AnnotationShape::Rectangle { start, .. } => *start,
            AnnotationShape::Circle    { start, .. } => *start,
        }
    }

    fn to_local(&self, offset: (f32, f32)) -> AnnotationShape {
        match self {
            AnnotationShape::Arrow { start, end } => AnnotationShape::Arrow {
                start: (start.0 - offset.0, start.1 - offset.1),
                end:   (end.0   - offset.0, end.1   - offset.1),
            },
            AnnotationShape::Rectangle { start, end } => AnnotationShape::Rectangle {
                start: (start.0 - offset.0, start.1 - offset.1),
                end:   (end.0   - offset.0, end.1   - offset.1),
            },
            AnnotationShape::Circle { start, end } => AnnotationShape::Circle {
                start: (start.0 - offset.0, start.1 - offset.1),
                end:   (end.0   - offset.0, end.1   - offset.1),
            },
        }
    }
}

#[derive(Clone)]
pub struct Annotation {
    pub id: u64,
    pub shape: AnnotationShape,
    pub color: Color,
    pub stroke_width: f32,
}

impl Annotation {
    pub fn to_local(&self, offset: (f32, f32)) -> Annotation {
        let mut local = self.clone();
        local.shape = self.shape.to_local(offset);
        local
    }

    pub fn bounding_box(&self) -> Option<Rect> {
        let (start, end) = match &self.shape {
            AnnotationShape::Arrow { start, end } => (*start, *end),
            AnnotationShape::Rectangle { start, end } => (*start, *end),
            AnnotationShape::Circle { start, end } => (*start, *end),
        };
        let pad = self.stroke_width * 2.0 + 12.0;
        Rect::from_ltrb(
            start.0.min(end.0) - pad,
            start.1.min(end.1) - pad,
            start.0.max(end.0) + pad,
            start.1.max(end.1) + pad,
        )
    }

    pub fn mark_dirty(&self, placements: &[Placement], dirty_mask: &mut u32) {
        if let Some(bbox) = self.bounding_box() {
            *dirty_mask |= get_overlapping_monitors(&bbox, placements);
        }
    }
}