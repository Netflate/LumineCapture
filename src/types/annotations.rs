use tiny_skia::{Rect, Color};

#[derive(Clone)]
pub enum AnnotationShape {
    Arrow { start: (f32, f32), end: (f32, f32) },
    Rect  { rect: Rect },
}
#[derive(Clone)]
pub struct Annotation {
    pub id: u64,
    pub shape: AnnotationShape,
    pub color: Color,
    pub stroke_width: f32,
}

pub fn annotation_to_local(ann: &Annotation, offset: (f32, f32)) -> Annotation {
    let mut local = ann.clone();
    local.shape = match &ann.shape {
        AnnotationShape::Arrow { start, end } => AnnotationShape::Arrow {
            start: (start.0 - offset.0, start.1 - offset.1),
            end:   (end.0   - offset.0, end.1   - offset.1),
        },
        AnnotationShape::Rect { rect } => AnnotationShape::Rect {
            rect: Rect::from_ltrb(
                rect.left()   - offset.0,
                rect.top()    - offset.1,
                rect.right()  - offset.0,
                rect.bottom() - offset.1,
            ).unwrap_or(*rect),
        },
    };
    local
}

pub fn annotation_bounding_box(ann: &Annotation) -> Option<Rect> {
    match &ann.shape {
        AnnotationShape::Arrow { start, end } => {
            let pad = ann.stroke_width * 2.0 + 12.0; 
            Rect::from_ltrb(
                start.0.min(end.0) - pad,
                start.1.min(end.1) - pad,
                start.0.max(end.0) + pad,
                start.1.max(end.1) + pad,
            )
        }
        AnnotationShape::Rect { rect } => {
            let pad = ann.stroke_width;
            Rect::from_ltrb(
                rect.left() - pad,
                rect.top() - pad,
                rect.right() + pad,
                rect.bottom() + pad,
            )
        }
    }
}
