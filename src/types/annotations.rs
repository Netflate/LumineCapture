use tiny_skia::{Rect, Color};
use crate::types::{SelectionHandle, SignedRect};

pub const HANDLE_PAD: f64 = 50.0;

#[derive(Clone)]
pub enum AnnotationShape {
    Arrow     { start: (f32, f32), end: (f32, f32) },
    Rectangle { start: (f32, f32), end: (f32, f32) },
    Circle    { start: (f32, f32), end: (f32, f32) },
    Line      { start: (f32, f32), end: (f32, f32) },
    Pen       { points: Vec<(f32, f32)> },
}

pub struct AnnDragState {
    pub handle: SelectionHandle,   
    pub start_global: (f64, f64),
    pub orig: Annotation,          // snapshot
}

impl AnnotationShape {
    pub fn start_point(&self) -> (f32, f32) {
        match self {
            AnnotationShape::Arrow     { start, ..   } => *start,
            AnnotationShape::Rectangle { start, ..   } => *start,
            AnnotationShape::Circle    { start, ..   } => *start,
            AnnotationShape::Line      { start, ..   } => *start,
            AnnotationShape::Pen       { points } => points.first().copied().unwrap_or((0.0, 0.0)),
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
            AnnotationShape::Line { start, end } => AnnotationShape::Line {
                start: (start.0 - offset.0, start.1 - offset.1),
                end:   (end.0   - offset.0, end.1   - offset.1),
            }, 
            AnnotationShape::Pen { points } => AnnotationShape::Pen {
                points: points.iter().map(|p| (p.0 - offset.0, p.1 - offset.1)).collect(),
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
    pub bbox: Rect,
}

impl Annotation {
    pub fn to_local(&self, offset: (f32, f32)) -> Annotation {
        let mut local = self.clone();
        local.shape = self.shape.to_local(offset);
        local.bbox = Rect::from_ltrb(
            local.bbox.left()   - offset.0,
            local.bbox.top()    - offset.1,
            local.bbox.right()  - offset.0,
            local.bbox.bottom() - offset.1,
        ).unwrap_or(local.bbox);
        local
    }
    
    pub fn update_bbox(&mut self) {
        match &self.shape {
            AnnotationShape::Arrow { start, end } |
            AnnotationShape::Rectangle { start, end } |
            AnnotationShape::Circle { start, end } |
            AnnotationShape::Line { start, end } => {
                
                self.bbox = Rect::from_ltrb(
                    start.0.min(end.0)  ,
                    start.1.min(end.1)   ,
                    start.0.max(end.0) ,
                    start.1.max(end.1),
                ).unwrap();
            }

            AnnotationShape::Pen { points } => {
                
                let min_x = points.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
                let min_y = points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
                let max_x = points.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max);
                let max_y = points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
                
                self.bbox = Rect::from_ltrb(
                    min_x, 
                    min_y, 
                    max_x, 
                    max_y 
                ).unwrap();
            }
        }    
    }

    pub fn last_segment_bbox(&self) -> Rect {
        match &self.shape {
            // to render only new pixels from pen, insted of rendering the whole rectangle 
            AnnotationShape::Pen { points } if points.len() >= 2 => {
                let from = points[points.len() - 2];
                let to   = points[points.len() - 1];

                Rect::from_ltrb(
                    from.0.min(to.0),
                    from.1.min(to.1),
                    from.0.max(to.0),
                    from.1.max(to.1),
                ).unwrap()
            }
            _ => self.bbox,
        }
    }
    
    pub fn translate(&self, dx: f32, dy: f32) -> Annotation {
        let shape = match &self.shape {
            AnnotationShape::Arrow { start, end } => AnnotationShape::Arrow {
                start: (start.0 + dx, start.1 + dy),
                end:   (end.0   + dx, end.1   + dy),
            },
            AnnotationShape::Rectangle { start, end } => AnnotationShape::Rectangle {
                start: (start.0 + dx, start.1 + dy),
                end:   (end.0   + dx, end.1   + dy),
            },
            AnnotationShape::Circle { start, end } => AnnotationShape::Circle {
                start: (start.0 + dx, start.1 + dy),
                end:   (end.0   + dx, end.1   + dy),
            },
            AnnotationShape::Line { start, end } => AnnotationShape::Line {
                start: (start.0 + dx, start.1 + dy),
                end:   (end.0   + dx, end.1   + dy),
            },
            AnnotationShape::Pen { points } => AnnotationShape::Pen {
                points: points.iter().map(|p| (p.0 + dx, p.1 + dy)).collect(),
            },
        };
        let mut result = Annotation { shape, ..self.clone() };
        result.update_bbox();
        result
    }

    pub fn resize_to_bbox(&self, new_bbox: SignedRect) -> Annotation {
        let orig = self.bbox;

        let sx = if orig.width()  > 0.0 { new_bbox.width()  / orig.width()  } else { 1.0 };
        let sy = if orig.height() > 0.0 { new_bbox.height() / orig.height() } else { 1.0 };

        let remap = |p: (f32, f32)| -> (f32, f32) {
            (
                new_bbox.left + (p.0 - orig.left()) * sx,
                new_bbox.top  + (p.1 - orig.top())  * sy,
            )
        };

        let shape = match &self.shape {
            AnnotationShape::Arrow { start, end } => AnnotationShape::Arrow {
                start: remap(*start), end: remap(*end),
            },
            AnnotationShape::Rectangle { start, end } => AnnotationShape::Rectangle {
                start: remap(*start), end: remap(*end),
            },
            AnnotationShape::Circle { start, end } => AnnotationShape::Circle {
                start: remap(*start), end: remap(*end),
            },
            AnnotationShape::Line { start, end } => AnnotationShape::Line {
                start: remap(*start), end: remap(*end),
            },
            AnnotationShape::Pen { points } => AnnotationShape::Pen {
                points: points.iter().map(|p| remap(*p)).collect(),
            },
        };

        let mut result = Annotation { shape, ..self.clone() };
        result.update_bbox();
        result
    }

    pub fn initial_hit_test(&self, coordinates: (f64, f64)) -> bool {
        // TODO: separate for pen
        let (x, y) = (coordinates.0 as f32, coordinates.1 as f32);
        let pad = HANDLE_PAD as f32; 

        x >= self.bbox.left() - pad && 
        x <= self.bbox.right() + pad && 
        y >= self.bbox.top() - pad && 
        y <= self.bbox.bottom() + pad
    }

    pub fn damage_bbox(&self, is_selected: bool) -> Rect {
        if is_selected {
            let pad = HANDLE_PAD as f32; // if its selected we need to add handlers padding
            Rect::from_ltrb(
                self.bbox.left() - pad,
                self.bbox.top() - pad,
                self.bbox.right() + pad,
                self.bbox.bottom() + pad,
            ).unwrap_or(self.bbox)
        } else {
            self.bbox
        }
    }
}




