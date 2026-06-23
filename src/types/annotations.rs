use tiny_skia::{Rect, Color};

#[derive(Clone)]
pub enum AnnotationShape {
    Arrow     { start: (f32, f32), end: (f32, f32) },
    Rectangle { start: (f32, f32), end: (f32, f32) },
    Circle    { start: (f32, f32), end: (f32, f32) },
    Line      { start: (f32, f32), end: (f32, f32) },
    Pen       { points: Vec<(f32, f32)> },
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
        local
    }
    
    pub fn update_bbox(&mut self) {
        match &self.shape {
            AnnotationShape::Arrow { start, end } |
            AnnotationShape::Rectangle { start, end } |
            AnnotationShape::Circle { start, end } |
            AnnotationShape::Line { start, end } => {
                let pad = self.stroke_width * 2.0 + 12.0;
                
                self.bbox = Rect::from_ltrb(
                    start.0.min(end.0) - pad,
                    start.1.min(end.1) - pad,
                    start.0.max(end.0) + pad,
                    start.1.max(end.1) + pad,
                ).unwrap();
            }

            AnnotationShape::Pen { points } => {
                let pad = self.stroke_width * 2.0;
                
                let min_x = points.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
                let min_y = points.iter().map(|p| p.1).fold(f32::INFINITY, f32::min);
                let max_x = points.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max);
                let max_y = points.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max);
                
                self.bbox = Rect::from_ltrb(
                    min_x - pad, 
                    min_y - pad, 
                    max_x + pad, 
                    max_y + pad
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
                let pad  = self.stroke_width * 2.0 + 2.0;
                Rect::from_ltrb(
                    from.0.min(to.0) - pad,
                    from.1.min(to.1) - pad,
                    from.0.max(to.0) + pad,
                    from.1.max(to.1) + pad,
                ).unwrap()
            }
            _ => self.bbox,
        }
    }

    // pub fn mark_dirty(&self, placements: &[Placement], dirty_mask: &mut u32) {
    //     if let Some(bbox) = self.bounding_box() {
    //         *dirty_mask |= get_overlapping_monitors(&bbox, placements);
    //     }
    // }
}

