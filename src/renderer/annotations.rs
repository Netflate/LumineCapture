use super::paths::{normalized_rect, oval_path};
use super::text::{draw_text_buffer, shape_single_line};
use crate::tools::text::render_text_annotation;
use crate::types::annotations::{
    Annotation, AnnotationShape, HANDLE_PAD, SHADOW_COLOR, SHADOW_OFFSET, SHADOW_LAYERS, SPREAD_PER_LAYER
};

use cosmic_text::{Editor, FontSystem, SwashCache};
use std::collections::HashMap;
use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform,
};

/// Scales the shadow's own fixed alpha by the annotation color's alpha
fn shadow_alpha_for(color: Color) -> u8 {
    (SHADOW_COLOR.3 as f32 * color.alpha()) as u8
}

fn shadow_color_for(color: Color) -> Color {
    Color::from_rgba8(
        SHADOW_COLOR.0,
        SHADOW_COLOR.1,
        SHADOW_COLOR.2,
        shadow_alpha_for(color),
    )
}

/// Builds the "real" transform 
fn transforms_for(offset: (f32, f32)) -> (Transform, Transform) {
    let transform = Transform::from_translate(-offset.0, -offset.1);
    let shadow_transform =
        Transform::from_translate(-offset.0 + SHADOW_OFFSET.0, -offset.1 + SHADOW_OFFSET.1);
    (transform, shadow_transform)
}

fn stroke_segment_with_shadow(
    canvas: &mut Pixmap,
    path: &tiny_skia::Path,
    paint: &Paint,
    stroke: &Stroke,
    base_shadow_color: Color,
    transform: Transform,
    shadow_transform: Transform,
) {
    for i in (1..=SHADOW_LAYERS).rev() {
        let mut layer_stroke = stroke.clone();

        let extra_width = i as f32 * SPREAD_PER_LAYER;
        layer_stroke.width = stroke.width + extra_width;

        let alpha_factor = 1.0 / (1.0 + (i as f32 * 1.2));
        let current_alpha = (base_shadow_color.alpha() * alpha_factor).clamp(0.0, 1.0);

        if let Some(layer_color) = Color::from_rgba(
            base_shadow_color.red(),
            base_shadow_color.green(),
            base_shadow_color.blue(),
            current_alpha,
        ) {
            let mut layer_paint = Paint::default();
            layer_paint.set_color(layer_color);
            layer_paint.anti_alias = true;

            canvas.stroke_path(path, &layer_paint, &layer_stroke, shadow_transform, None);
        }
    }

    canvas.stroke_path(path, paint, stroke, transform, None);
}

pub fn draw_annotation(
    canvas: &mut Pixmap,
    ann: &Annotation,
    offset: (f32, f32),
    selected: bool,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    text_editors: &mut HashMap<u64, Editor<'static>>,
    active_text_id: Option<u64>,
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
        AnnotationShape::Text { .. } => {
            let is_editing = active_text_id == Some(ann.id);
            if let Some(editor) = text_editors.get_mut(&ann.id) {
                let mut pixmap_mut = canvas.as_mut();
                render_text_annotation(
                    ann,
                    editor,
                    font_system,
                    swash_cache,
                    &mut pixmap_mut,
                    offset,
                    is_editing,
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
        if matches!(ann.shape, AnnotationShape::Text { .. }) {
            draw_text_box(canvas, &ann.bbox, offset);
        } else {
            draw_annotation_handles(canvas, &ann.bbox, offset);
        }
    }
}

fn draw_text_box(canvas: &mut Pixmap, bbox: &Rect, offset: (f32, f32)) {
    let mut paint = Paint::default(); paint.set_color(Color::WHITE); paint.anti_alias = true;
    let mut stroke = Stroke::default(); stroke.width = 3.0; stroke.line_cap = tiny_skia::LineCap::Round; stroke.line_join = tiny_skia::LineJoin::Round;

    let base_shadow_color = Color::from_rgba8(SHADOW_COLOR.0, SHADOW_COLOR.1, SHADOW_COLOR.2, SHADOW_COLOR.3);

    // halo instead of real shadow
    let transform = Transform::from_translate(-offset.0, -offset.1);
    let shadow_transform = transform;
    let pad = (HANDLE_PAD / 2.0) as f32;
    let (l, t, ri, b) = (bbox.left() - pad, bbox.top() - pad, bbox.right() + pad, bbox.bottom() + pad);

    let (w, h) = (ri - l, b - t);
    let corner_w = (w * 0.20).clamp(8.0_f32.min(w * 0.5), w * 0.5);
    let corner_h = (h * 0.20).clamp(8.0_f32.min(h * 0.5), h * 0.5);
    let r = 4.0_f32.min(corner_w * 0.5).min(corner_h * 0.5);
    let k = 0.5523_f32;

    let mut pb = PathBuilder::new();

    pb.move_to(l, t + corner_h); pb.line_to(l, t + r); pb.cubic_to(l, t + r * k, l + r * k, t, l + r, t); pb.line_to(l + corner_w, t);
    pb.move_to(ri - corner_w, t); pb.line_to(ri - r, t); pb.cubic_to(ri - r * k, t, ri, t + r * k, ri, t + r); pb.line_to(ri, t + corner_h);
    pb.move_to(ri, b - corner_h); pb.line_to(ri, b - r); pb.cubic_to(ri, b - r * k, ri - r * k, b, ri - r, b); pb.line_to(ri - corner_w, b);
    pb.move_to(l + corner_w, b); pb.line_to(l + r, b); pb.cubic_to(l + r * k, b, l, b - r * k, l, b - r); pb.line_to(l, b - corner_h);

    if let Some(path) = pb.finish() {
        stroke_segment_with_shadow(
            canvas, &path, &paint, &stroke, base_shadow_color,
            transform, shadow_transform,
        );
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
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1.0 {
        return;
    }

    let ux = dx / len;
    let uy = dy / len;

    let head_len = (stroke_width * 4.0).max(12.0).min(len * 0.6);
    let head_width = head_len * 0.55;

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
    
    pb.move_to(start.0, start.1);
    pb.line_to(tip.0, tip.1);

    pb.move_to(base1.0, base1.1);
    pb.line_to(tip.0, tip.1);
    pb.line_to(base2.0, base2.1);

    if let Some(path) = pb.finish() {
        stroke_with_shadow(
            canvas,
            &path,
            color,
            stroke_width,
            LineCap::Round,
            LineJoin::Round,
            offset,
        );
    }
}

fn draw_rect(
    canvas: &mut Pixmap,
    rect: &Rect,
    color: Color,
    stroke_width: f32,
    offset: (f32, f32),
) {
    let path = PathBuilder::from_rect(*rect);
    stroke_with_shadow(
        canvas,
        &path,
        color,
        stroke_width,
        LineCap::Butt,
        LineJoin::Miter,
        offset,
    );
}

fn draw_circle(
    canvas: &mut Pixmap,
    rect: &Rect,
    color: Color,
    stroke_width: f32,
    offset: (f32, f32),
) {
    let cx = (rect.left() + rect.right()) / 2.0;
    let cy = (rect.top() + rect.bottom()) / 2.0;
    let rx = rect.width() / 2.0;
    let ry = rect.height() / 2.0;

    if let Some(path) = oval_path(cx, cy, rx, ry) {
        stroke_with_shadow(
            canvas,
            &path,
            color,
            stroke_width,
            LineCap::Butt,
            LineJoin::Round,
            offset,
        );
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
    let mut pb = PathBuilder::new();
    pb.move_to(start.0, start.1);
    pb.line_to(end.0, end.1);

    if let Some(path) = pb.finish() {
        stroke_with_shadow(
            canvas,
            &path,
            color,
            stroke_width,
            LineCap::Round,
            LineJoin::Round,
            offset,
        );
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

    let mut pb = PathBuilder::new();
    pb.move_to(points[0].0, points[0].1);

    if points.len() == 2 {
        pb.line_to(points[1].0, points[1].1);
    } else {
        for i in 1..points.len() - 1 {
            let curr = points[i];
            let next = points[i + 1];
            let mid = ((curr.0 + next.0) / 2.0, (curr.1 + next.1) / 2.0);
            pb.quad_to(curr.0, curr.1, mid.0, mid.1);
        }
        let last = points[points.len() - 1];
        pb.line_to(last.0, last.1);
    }

    if let Some(path) = pb.finish() {
        stroke_with_shadow(
            canvas,
            &path,
            color,
            stroke_width,
            LineCap::Round,
            LineJoin::Round,
            offset,
        );
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

    let base_shadow_color = Color::from_rgba8(
        SHADOW_COLOR.0,
        SHADOW_COLOR.1,
        SHADOW_COLOR.2,
        SHADOW_COLOR.3,
    );

    // Eight small, disjoint segments — halo (no offset) instead of a drop shadow.
    let transform = Transform::from_translate(-offset.0, -offset.1);
    let shadow_transform = transform;

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

    let mut draw_segment = |pb: PathBuilder| {
        if let Some(path) = pb.finish() {
            stroke_segment_with_shadow(
                canvas, &path, &paint, &stroke, base_shadow_color,
                transform, shadow_transform,
            );
        }
    };

    // Top-Left
    let mut pb = PathBuilder::new();
    pb.move_to(l, t + corner_h);
    pb.line_to(l, t + r);
    pb.cubic_to(l, t + r * k, l + r * k, t, l + r, t);
    pb.line_to(l + corner_w, t);
    draw_segment(pb);

    // Top-Right
    let mut pb = PathBuilder::new();
    pb.move_to(ri - corner_w, t);
    pb.line_to(ri - r, t);
    pb.cubic_to(ri - r * k, t, ri, t + r * k, ri, t + r);
    pb.line_to(ri, t + corner_h);
    draw_segment(pb);

    // Bottom-Right
    let mut pb = PathBuilder::new();
    pb.move_to(ri, b - corner_h);
    pb.line_to(ri, b - r);
    pb.cubic_to(ri, b - r * k, ri - r * k, b, ri - r, b);
    pb.line_to(ri - corner_w, b);
    draw_segment(pb);

    // Bottom-Left
    let mut pb = PathBuilder::new();
    pb.move_to(l + corner_w, b);
    pb.line_to(l + r, b);
    pb.cubic_to(l + r * k, b, l, b - r * k, l, b - r);
    pb.line_to(l, b - corner_h);
    draw_segment(pb);

    // Top middle
    let mut pb = PathBuilder::new();
    pb.move_to(mid_x - mid_hw / 2.0, t);
    pb.line_to(mid_x + mid_hw / 2.0, t);
    draw_segment(pb);

    // Bottom middle
    let mut pb = PathBuilder::new();
    pb.move_to(mid_x - mid_hw / 2.0, b);
    pb.line_to(mid_x + mid_hw / 2.0, b);
    draw_segment(pb);

    // Left middle
    let mut pb = PathBuilder::new();
    pb.move_to(l, mid_y - mid_hh / 2.0);
    pb.line_to(l, mid_y + mid_hh / 2.0);
    draw_segment(pb);

    // Right middle
    let mut pb = PathBuilder::new();
    pb.move_to(ri, mid_y - mid_hh / 2.0);
    pb.line_to(ri, mid_y + mid_hh / 2.0);
    draw_segment(pb);
}

fn stroke_with_shadow(
    canvas: &mut Pixmap,
    path: &tiny_skia::Path,
    color: Color,
    stroke_width: f32,
    line_cap: LineCap,
    line_join: LineJoin,
    offset: (f32, f32),
) {
    let (transform, shadow_transform) = transforms_for(offset);

    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;

    let mut stroke = Stroke::default();
    stroke.width = stroke_width;
    stroke.line_cap = line_cap;
    stroke.line_join = line_join;

    let base_shadow_color = shadow_color_for(color);

    stroke_segment_with_shadow(
        canvas, path, &paint, &stroke, base_shadow_color,
        transform, shadow_transform,
    );
}

fn contrasting_text_color(circle_color: Color) -> Color {
    // Perceived luminance (ITU-R BT.601)
    let luminance = 0.299 * circle_color.red()
        + 0.587 * circle_color.green()
        + 0.114 * circle_color.blue();

    if luminance > 0.55 {
        Color::BLACK
    } else {
        Color::WHITE
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
    const CIRCLE_RADIUS_RATIO: f32 = 3.0;

    let (transform, shadow_transform) = transforms_for(offset);

    let circle_radius = stroke_width * CIRCLE_RADIUS_RATIO;

    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    let len = (dx * dx + dy * dy).sqrt();

    let mut fill_paint = Paint::default();
    fill_paint.set_color(color);
    fill_paint.anti_alias = true;

    let mut shadow_paint = Paint::default();
    shadow_paint.set_color(shadow_color_for(color));
    shadow_paint.anti_alias = true;

    if len > 2.0 {
        let ux = dx / len;
        let uy = dy / len;

        let px = -uy;
        let py = ux;

        let base_half_width = circle_radius * 0.48;

        let base_left = (
            start.0 + px * base_half_width,
            start.1 + py * base_half_width,
        );
        let base_right = (
            start.0 - px * base_half_width,
            start.1 - py * base_half_width,
        );

        let mut pb = PathBuilder::new();
        pb.move_to(end.0, end.1);
        pb.line_to(base_left.0, base_left.1);
        pb.line_to(base_right.0, base_right.1);
        pb.close();

        if let Some(path) = pb.finish() {
            canvas.fill_path(&path, &shadow_paint, FillRule::Winding, shadow_transform, None);
            canvas.fill_path(&path, &fill_paint, FillRule::Winding, transform, None);
        }
    }

    if let Some(circle) = oval_path(start.0, start.1, circle_radius, circle_radius) {
        canvas.fill_path(&circle, &shadow_paint, FillRule::Winding, shadow_transform, None);
        canvas.fill_path(&circle, &fill_paint, FillRule::Winding, transform, None);
    }

    let digits = number.to_string().len();

    let font_size = match digits {
        1 => circle_radius * 1.1,
        2 => circle_radius * 0.85,
        3 => circle_radius * 0.65,
        _ => circle_radius * 0.5,
    };

    let (buffer, text_width, text_height) = shape_single_line(
        font_system,
        &number.to_string(),
        font_size,
        cosmic_text::Weight::BOLD,
        cosmic_text::Style::Normal,
    );

    let text_pos = (
        start.0 - text_width / 2.0,
        start.1 - text_height / 2.0,
    );

    let text_color = contrasting_text_color(color);

    draw_text_buffer(
        canvas,
        &buffer,
        font_system,
        swash_cache,
        text_pos,
        text_color,
        offset,
    );
}