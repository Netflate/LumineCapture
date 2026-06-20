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