use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};
use crate::types::annotations::{Annotation, AnnotationShape, HANDLE_PAD};
use super::paths::{normalized_rect, oval_path};

pub fn draw_annotation(canvas: &mut Pixmap, ann: &Annotation, offset: (f32, f32), selected: bool) {
    match &ann.shape {
        AnnotationShape::Arrow { start, end } => {
            draw_arrow(canvas, *start, *end, ann.color, ann.stroke_width, offset);
        }
        AnnotationShape::Rectangle { start, end } => {
            if let Some(rect) = normalized_rect(*start, *end) {
                draw_rect(canvas, &rect, ann.color, ann.stroke_width, offset);
            }
        }
        AnnotationShape::Circle { start, end } => {
            if let Some(rect) = normalized_rect(*start, *end) {
                draw_circle(canvas, &rect, ann.color, ann.stroke_width, offset);
            }
        }
        AnnotationShape::Line { start, end } => {
            draw_line(canvas, *start, *end, ann.color, ann.stroke_width, offset);
        }
        AnnotationShape::Pen { points } => {
            draw_pen(canvas, points, ann.color, ann.stroke_width, offset);
        }
        _ => {}
    }
    if selected {draw_annotation_handles(canvas, &ann.bbox, offset);}
}

fn draw_arrow(canvas: &mut Pixmap, start: (f32, f32), end: (f32, f32), color: Color, stroke_width: f32, offset: (f32, f32)) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;

    let mut stroke = Stroke::default();
    stroke.width = stroke_width;
    let transform = Transform::from_translate(-offset.0, -offset.1);

    let mut pb = PathBuilder::new();
    pb.move_to(start.0, start.1);
    pb.line_to(end.0, end.1);
    if let Some(path) = pb.finish() {
        canvas.stroke_path(&path, &paint, &stroke, transform, None);
    }

    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 { return; }

    let ux = dx / len;
    let uy = dy / len;
    let head_len = (stroke_width * 4.0).max(12.0);
    let head_width = head_len * 0.5;
    let px = -uy;
    let py =  ux;

    let tip   = end;
    let base1 = (end.0 - ux * head_len + px * head_width, end.1 - uy * head_len + py * head_width);
    let base2 = (end.0 - ux * head_len - px * head_width, end.1 - uy * head_len - py * head_width);

    let mut pb = PathBuilder::new();
    pb.move_to(tip.0, tip.1);
    pb.line_to(base1.0, base1.1);
    pb.line_to(base2.0, base2.1);
    pb.close();
    if let Some(path) = pb.finish() {
        canvas.fill_path(&path, &paint, tiny_skia::FillRule::Winding, transform, None);
    }
}

fn draw_rect(canvas: &mut Pixmap, rect: &Rect, color: Color, stroke_width: f32, offset: (f32, f32)) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    let mut stroke = Stroke::default();
    stroke.width = stroke_width;
    let transform = Transform::from_translate(-offset.0, -offset.1);
    let path = PathBuilder::from_rect(*rect);
    canvas.stroke_path(&path, &paint, &stroke, transform, None);
}

fn draw_circle(canvas: &mut Pixmap, rect: &Rect, color: Color, stroke_width: f32, offset: (f32, f32)) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    let mut stroke = Stroke::default();
    stroke.width = stroke_width;
    let transform = Transform::from_translate(-offset.0, -offset.1);

    let cx = (rect.left() + rect.right()) / 2.0;
    let cy = (rect.top() + rect.bottom()) / 2.0;
    let rx = rect.width() / 2.0;
    let ry = rect.height() / 2.0;

    if let Some(path) = oval_path(cx, cy, rx, ry) {
        canvas.stroke_path(&path, &paint, &stroke, transform, None);
    }
}

fn draw_line(canvas: &mut Pixmap, start: (f32, f32), end: (f32, f32), color: Color, stroke_width: f32, offset: (f32, f32)) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;

    let mut stroke = Stroke::default();
    stroke.width = stroke_width;
    let transform = Transform::from_translate(-offset.0, -offset.1);

    let mut pb = PathBuilder::new();
    pb.move_to(start.0, start.1);
    pb.line_to(end.0, end.1);
    
    if let Some(path) = pb.finish() {
        canvas.stroke_path(&path, &paint, &stroke, transform, None);
    }
}

fn draw_pen(canvas: &mut Pixmap, points: &[(f32, f32)], color: Color, stroke_width: f32, offset: (f32, f32)) {
    if points.len() < 2 { return; }
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    let mut stroke = Stroke::default();
    stroke.width = stroke_width;
    stroke.line_cap = tiny_skia::LineCap::Round;
    stroke.line_join = tiny_skia::LineJoin::Round;
    let transform = Transform::from_translate(-offset.0, -offset.1);

    let mut pb = PathBuilder::new();
    pb.move_to(points[0].0, points[0].1);
    for p in &points[1..] {
        pb.line_to(p.0, p.1);
    }
    if let Some(path) = pb.finish() {
        canvas.stroke_path(&path, &paint, &stroke, transform, None);
    }
}

fn draw_annotation_handles(canvas: &mut Pixmap, bbox: &Rect, offset: (f32, f32)) {
    let mut paint = Paint::default();
    paint.set_color(Color::WHITE);
    paint.anti_alias = true;
    
    let mut stroke = Stroke::default();
    stroke.width = 3.0;
    stroke.line_cap = tiny_skia::LineCap::Round;
    stroke.line_join = tiny_skia::LineJoin::Round;
    let transform = Transform::from_translate(-offset.0, -offset.1);

    let out_pad = (HANDLE_PAD / 2.0) as f32;
    
    let l  = bbox.left() - out_pad;
    let t  = bbox.top() - out_pad;
    let ri = bbox.right() + out_pad;
    let b  = bbox.bottom() + out_pad;

    let w = ri - l;
    let h = b - t;
    let mid_x = (l + ri) / 2.0;
    let mid_y = (t + b) / 2.0;

    // Mid handles
    let mid_hw = (w * 0.32).clamp(8.0_f32.min(w * 0.5), w * 0.5);
    let mid_hh = (h * 0.32).clamp(8.0_f32.min(h * 0.5), h * 0.5);

    // Edges
    let corner_w = (w * 0.20).clamp(8.0_f32.min(w * 0.5), w * 0.5);
    let corner_h = (h * 0.20).clamp(8.0_f32.min(h * 0.5), h * 0.5);

    let r = 4.0_f32.min(corner_w * 0.5).min(corner_h * 0.5);
    let k = 0.5523_f32;

    // Top-Left
    let mut pb = PathBuilder::new();
    pb.move_to(l, t + corner_h);
    pb.line_to(l, t + r);
    pb.cubic_to(l, t + r * k, l + r * k, t, l + r, t);
    pb.line_to(l + corner_w, t);
    if let Some(path) = pb.finish() { canvas.stroke_path(&path, &paint, &stroke, transform, None); }

    // Top-Right
    let mut pb = PathBuilder::new();
    pb.move_to(ri - corner_w, t);
    pb.line_to(ri - r, t);
    pb.cubic_to(ri - r * k, t, ri, t + r * k, ri, t + r);
    pb.line_to(ri, t + corner_h);
    if let Some(path) = pb.finish() { canvas.stroke_path(&path, &paint, &stroke, transform, None); }

    // Bottom-Right
    let mut pb = PathBuilder::new();
    pb.move_to(ri, b - corner_h);
    pb.line_to(ri, b - r);
    pb.cubic_to(ri, b - r * k, ri - r * k, b, ri - r, b);
    pb.line_to(ri - corner_w, b);
    if let Some(path) = pb.finish() { canvas.stroke_path(&path, &paint, &stroke, transform, None); }

    // Bottom-Left
    let mut pb = PathBuilder::new();
    pb.move_to(l + corner_w, b);
    pb.line_to(l + r, b);
    pb.cubic_to(l + r * k, b, l, b - r * k, l, b - r);
    pb.line_to(l, b - corner_h);
    if let Some(path) = pb.finish() { canvas.stroke_path(&path, &paint, &stroke, transform, None); }

    // Top middle
    let mut pb = PathBuilder::new();
    pb.move_to(mid_x - mid_hw / 2.0, t);
    pb.line_to(mid_x + mid_hw / 2.0, t);
    if let Some(path) = pb.finish() { canvas.stroke_path(&path, &paint, &stroke, transform, None); }

    // Bottom middle
    let mut pb = PathBuilder::new();
    pb.move_to(mid_x - mid_hw / 2.0, b);
    pb.line_to(mid_x + mid_hw / 2.0, b);
    if let Some(path) = pb.finish() { canvas.stroke_path(&path, &paint, &stroke, transform, None); }

    // Left middle
    let mut pb = PathBuilder::new();
    pb.move_to(l, mid_y - mid_hh / 2.0);
    pb.line_to(l, mid_y + mid_hh / 2.0);
    if let Some(path) = pb.finish() { canvas.stroke_path(&path, &paint, &stroke, transform, None); }

    // Right middle
    let mut pb = PathBuilder::new();
    pb.move_to(ri, mid_y - mid_hh / 2.0);
    pb.line_to(ri, mid_y + mid_hh / 2.0);
    if let Some(path) = pb.finish() { canvas.stroke_path(&path, &paint, &stroke, transform, None); }
}