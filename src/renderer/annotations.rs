use super::paths::{normalized_rect, oval_path};
use crate::types::annotations::{Annotation, AnnotationShape, HANDLE_PAD};

use cosmic_text::{Buffer, FontSystem, SwashCache, SwashContent, Metrics, Shaping, Attrs};
use std::collections::HashMap;
use tiny_skia::{
    Color, Paint, PathBuilder, Pixmap, PixmapPaint, PremultipliedColorU8, Rect, Stroke, Transform, LineCap, LineJoin, FillRule,
};

pub fn draw_annotation(
    canvas: &mut Pixmap,
    ann: &Annotation,
    offset: (f32, f32),
    selected: bool,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    text_buffers: &HashMap<u64, Buffer>,
) {
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
        AnnotationShape::Text { start, .. } => {
            if let Some(buffer) = text_buffers.get(&ann.id) {
                draw_text_buffer(
                    canvas,
                    buffer,
                    font_system,
                    swash_cache,
                    *start,
                    ann.color,
                    offset,
                );
            }
        }
        AnnotationShape::NumeratedArrow { start, end, number } => {
            draw_numerated_arrow(
                canvas,
                *start,
                *end,
                *number,
                ann.color,
                ann.stroke_width,
                offset,
                font_system,
                swash_cache,
            );
        }
    }
    if selected {
        draw_annotation_handles(canvas, &ann.bbox, offset);
    }
}

fn draw_arrow(
    canvas: &mut Pixmap,
    start: (f32, f32),
    end: (f32, f32),
    color: Color,
    stroke_width: f32,
    offset: (f32, f32),
) {
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
    if len < 1.0 {
        return;
    }

    let ux = dx / len;
    let uy = dy / len;
    let head_len = (stroke_width * 4.0).max(12.0);
    let head_width = head_len * 0.5;
    let px = -uy;
    let py = ux;

    let tip = end;
    let base1 = (
        end.0 - ux * head_len + px * head_width,
        end.1 - uy * head_len + py * head_width,
    );
    let base2 = (
        end.0 - ux * head_len - px * head_width,
        end.1 - uy * head_len - py * head_width,
    );

    let mut pb = PathBuilder::new();
    pb.move_to(tip.0, tip.1);
    pb.line_to(base1.0, base1.1);
    pb.line_to(base2.0, base2.1);
    pb.close();
    if let Some(path) = pb.finish() {
        canvas.fill_path(&path, &paint, tiny_skia::FillRule::Winding, transform, None);
    }
}

fn draw_rect(
    canvas: &mut Pixmap,
    rect: &Rect,
    color: Color,
    stroke_width: f32,
    offset: (f32, f32),
) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    let mut stroke = Stroke::default();
    stroke.width = stroke_width;
    let transform = Transform::from_translate(-offset.0, -offset.1);
    let path = PathBuilder::from_rect(*rect);
    canvas.stroke_path(&path, &paint, &stroke, transform, None);
}

fn draw_circle(
    canvas: &mut Pixmap,
    rect: &Rect,
    color: Color,
    stroke_width: f32,
    offset: (f32, f32),
) {
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

fn draw_line(
    canvas: &mut Pixmap,
    start: (f32, f32),
    end: (f32, f32),
    color: Color,
    stroke_width: f32,
    offset: (f32, f32),
) {
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

fn draw_pen(
    canvas: &mut Pixmap,
    points: &[(f32, f32)],
    color: Color,
    stroke_width: f32,
    offset: (f32, f32),
) {
    if points.len() < 2 {
        return;
    }
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

    let l = bbox.left() - out_pad;
    let t = bbox.top() - out_pad;
    let ri = bbox.right() + out_pad;
    let b = bbox.bottom() + out_pad;

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
    if let Some(path) = pb.finish() {
        canvas.stroke_path(&path, &paint, &stroke, transform, None);
    }

    // Top-Right
    let mut pb = PathBuilder::new();
    pb.move_to(ri - corner_w, t);
    pb.line_to(ri - r, t);
    pb.cubic_to(ri - r * k, t, ri, t + r * k, ri, t + r);
    pb.line_to(ri, t + corner_h);
    if let Some(path) = pb.finish() {
        canvas.stroke_path(&path, &paint, &stroke, transform, None);
    }

    // Bottom-Right
    let mut pb = PathBuilder::new();
    pb.move_to(ri, b - corner_h);
    pb.line_to(ri, b - r);
    pb.cubic_to(ri, b - r * k, ri - r * k, b, ri - r, b);
    pb.line_to(ri - corner_w, b);
    if let Some(path) = pb.finish() {
        canvas.stroke_path(&path, &paint, &stroke, transform, None);
    }

    // Bottom-Left
    let mut pb = PathBuilder::new();
    pb.move_to(l + corner_w, b);
    pb.line_to(l + r, b);
    pb.cubic_to(l + r * k, b, l, b - r * k, l, b - r);
    pb.line_to(l, b - corner_h);
    if let Some(path) = pb.finish() {
        canvas.stroke_path(&path, &paint, &stroke, transform, None);
    }

    // Top middle
    let mut pb = PathBuilder::new();
    pb.move_to(mid_x - mid_hw / 2.0, t);
    pb.line_to(mid_x + mid_hw / 2.0, t);
    if let Some(path) = pb.finish() {
        canvas.stroke_path(&path, &paint, &stroke, transform, None);
    }

    // Bottom middle
    let mut pb = PathBuilder::new();
    pb.move_to(mid_x - mid_hw / 2.0, b);
    pb.line_to(mid_x + mid_hw / 2.0, b);
    if let Some(path) = pb.finish() {
        canvas.stroke_path(&path, &paint, &stroke, transform, None);
    }

    // Left middle
    let mut pb = PathBuilder::new();
    pb.move_to(l, mid_y - mid_hh / 2.0);
    pb.line_to(l, mid_y + mid_hh / 2.0);
    if let Some(path) = pb.finish() {
        canvas.stroke_path(&path, &paint, &stroke, transform, None);
    }

    // Right middle
    let mut pb = PathBuilder::new();
    pb.move_to(ri, mid_y - mid_hh / 2.0);
    pb.line_to(ri, mid_y + mid_hh / 2.0);
    if let Some(path) = pb.finish() {
        canvas.stroke_path(&path, &paint, &stroke, transform, None);
    }
}

pub fn draw_text_buffer(
    canvas: &mut Pixmap,
    buffer: &Buffer,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    pos: (f32, f32),
    color: Color,
    offset: (f32, f32),
) {
    let (x_start, y_start) = pos;

    for run in buffer.layout_runs() {
        for glyph in run.glyphs.iter() {
            let physical = glyph.physical((0., 0.), 1.0);

            if let Some(image) = swash_cache.get_image(font_system, physical.cache_key) {
                let width = image.placement.width as u32;
                let height = image.placement.height as u32;

                if width == 0 || height == 0 {
                    continue;
                }

                let draw_x =
                    (x_start + physical.x as f32 + image.placement.left as f32 - offset.0) as i32;
                let draw_y =
                    (y_start + run.line_y as f32 - image.placement.top as f32 - offset.1) as i32;

                match image.content {
                    SwashContent::Mask => {
                        if let Some(mut glyph_pixmap) = Pixmap::new(width, height) {
                            let pixels = glyph_pixmap.pixels_mut();

                            for (i, mask_alpha) in image.data.iter().enumerate() {
                                let a_f32 = (*mask_alpha as f32 / 255.0) * color.alpha();
                                let a_u8 = (a_f32 * 255.0) as u8;

                                let pr = (color.red() * a_f32 * 255.0) as u8;
                                let pg = (color.green() * a_f32 * 255.0) as u8;
                                let pb = (color.blue() * a_f32 * 255.0) as u8;

                                pixels[i] = PremultipliedColorU8::from_rgba(pr, pg, pb, a_u8)
                                    .unwrap_or(PremultipliedColorU8::TRANSPARENT);
                            }

                            canvas.draw_pixmap(
                                draw_x,
                                draw_y,
                                glyph_pixmap.as_ref(),
                                &PixmapPaint::default(),
                                Transform::identity(),
                                None,
                            );
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
fn draw_numerated_arrow(
    canvas: &mut Pixmap,
    start: (f32, f32),
    end: (f32, f32),
    number: u32,
    color: Color,
    stroke_width: f32,
    offset: (f32, f32),
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) {
    let transform = Transform::from_translate(-offset.0, -offset.1);

    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let len = (dx * dx + dy * dy).sqrt();

    let circle_radius = (stroke_width * 2.5).max(18.0);

    if len <= circle_radius * 2.0 {
        return;
    }

    let ux = dx / len;
    let uy = dy / len;

    // paints 

    let mut stroke_paint = Paint::default();
    stroke_paint.set_color(color);
    stroke_paint.anti_alias = true;

    let mut fill_paint = Paint::default();
    fill_paint.set_color(Color::WHITE);
    fill_paint.anti_alias = true;

    let mut stroke = Stroke::default();
    stroke.width = stroke_width;
    stroke.line_cap = LineCap::Round;
    stroke.line_join = LineJoin::Round;

    // circle 

    if let Some(circle) =
        oval_path(start.0, start.1, circle_radius, circle_radius)
    {
        canvas.fill_path(
            &circle,
            &fill_paint,
            FillRule::Winding,
            transform,
            None,
        );

        canvas.stroke_path(
            &circle,
            &stroke_paint,
            &stroke,
            transform,
            None,
        );
    }

    // Arrow line

    let head_len = (stroke_width * 5.0).max(14.0);
    let head_width = head_len * 0.7;

    let line_start = (
        start.0 + ux * circle_radius,
        start.1 + uy * circle_radius,
    );

    let line_end = (
        end.0 - ux * head_len,
        end.1 - uy * head_len,
    );

    let mut pb = PathBuilder::new();
    pb.move_to(line_start.0, line_start.1);
    pb.line_to(line_end.0, line_end.1);

    if let Some(path) = pb.finish() {
        canvas.stroke_path(
            &path,
            &stroke_paint,
            &stroke,
            transform,
            None,
        );
    }

    // arrow head

    let px = -uy;
    let py = ux;

    let left = (
        line_end.0 - ux * head_len + px * head_width,
        line_end.1 - uy * head_len + py * head_width,
    );

    let right = (
        line_end.0 - ux * head_len - px * head_width,
        line_end.1 - uy * head_len - py * head_width,
    );

    let mut pb = PathBuilder::new();
    pb.move_to(end.0, end.1);
    pb.line_to(left.0, left.1);
    pb.line_to(right.0, right.1);
    pb.close();

    if let Some(path) = pb.finish() {
        canvas.fill_path(
            &path,
            &stroke_paint,
            FillRule::Winding,
            transform,
            None,
        );
    }

    // number

    let digits = number.to_string().len();

    let font_size = match digits {
        1 => circle_radius * 1.1,
        2 => circle_radius * 0.85,
        3 => circle_radius * 0.65,
        _ => circle_radius * 0.5,
    };

    let line_height = font_size * 1.1;

    let mut buffer =
        Buffer::new(font_system, Metrics::new(font_size, line_height));

    buffer.set_size(
        Some(circle_radius * 2.0),
        Some(circle_radius * 2.0),
    );

    buffer.set_text(
        &number.to_string(),
        &Attrs::new(),
        Shaping::Advanced,
        None,
    );

    buffer.shape_until_scroll(font_system, false);

    let mut text_width: f32 = 0.0;
    let mut text_height: f32 = 0.0;

    for run in buffer.layout_runs() {
        text_width = text_width.max(run.line_w);
        text_height += run.line_height;
    }

    let text_pos = (
        start.0 - text_width / 2.0,
        start.1 - text_height / 2.0 - font_size * 0.1,
    );

    draw_text_buffer(
        canvas,
        &buffer,
        font_system,
        swash_cache,
        text_pos,
        color,
        offset,
    );
}