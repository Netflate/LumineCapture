use crate::types::panel::PANEL_COLOR;

use tiny_skia::{PathBuilder, Paint, Pixmap, Rect, Transform, Color, Stroke};
use usvg::Tree;
use std::collections::HashMap;

pub fn rounded_rect_path(
    rect: &Rect,
    r: f32,
    top_left: bool,
    top_right: bool,
    bottom_right: bool,
    bottom_left: bool,
) -> Option<tiny_skia::Path> {
    let r = r.min(rect.width() / 2.0).min(rect.height() / 2.0);

    let (l, t, ri, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    const K: f32 = 0.5523;

    let r_tl = if top_left { r } else { 0.0 };
    let r_tr = if top_right { r } else { 0.0 };
    let r_br = if bottom_right { r } else { 0.0 };
    let r_bl = if bottom_left { r } else { 0.0 };

    let mut pb = PathBuilder::new();

    pb.move_to(l + r_tl, t);
    pb.line_to(ri - r_tr, t);

    if top_right {
        pb.cubic_to(ri - r_tr * K, t, ri, t + r_tr * K, ri, t + r_tr);
    } else {
        pb.line_to(ri, t);
    }

    pb.line_to(ri, b - r_br);

    if bottom_right {
        pb.cubic_to(ri, b - r_br * K, ri - r_br * K, b, ri - r_br, b);
    } else {
        pb.line_to(ri, b);
    }

    pb.line_to(l + r_bl, b);

    if bottom_left {
        pb.cubic_to(l + r_bl * K, b, l, b - r_bl * K, l, b - r_bl);
    } else {
        pb.line_to(l, b);
    }

    pb.line_to(l, t + r_tl);

    if top_left {
        pb.cubic_to(l, t + r_tl * K, l + r_tl * K, t, l + r_tl, t);
    } else {
        pb.line_to(l, t);
    }

    pb.finish()
}

pub fn oval_path(cx: f32, cy: f32, rx: f32, ry: f32) -> Option<tiny_skia::Path> {
    const K: f32 = 0.5523;
    let mut pb = PathBuilder::new();
    pb.move_to(cx, cy - ry);
    pb.cubic_to(cx + rx * K, cy - ry, cx + rx, cy - ry * K, cx + rx, cy);
    pb.cubic_to(cx + rx, cy + ry * K, cx + rx * K, cy + ry, cx, cy + ry);
    pb.cubic_to(cx - rx * K, cy + ry, cx - rx, cy + ry * K, cx - rx, cy);
    pb.cubic_to(cx - rx, cy - ry * K, cx - rx * K, cy - ry, cx, cy - ry);
    pb.close();
    pb.finish()
}

pub fn normalized_rect(start: (f32, f32), end: (f32, f32)) -> Option<Rect> {
    Rect::from_ltrb(
        start.0.min(end.0),
        start.1.min(end.1),
        start.0.max(end.0),
        start.1.max(end.1),
    )
}

pub fn rect_bounds(rect: &Rect, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
    let x0 = rect.left().floor().max(0.0) as i32;
    let y0 = rect.top().floor().max(0.0) as i32;
    let x1 = rect.right().ceil().min(width as f32) as i32;
    let y1 = rect.bottom().ceil().min(height as f32) as i32;

    if x1 <= x0 || y1 <= y0 {
        return None;
    }

    Some((x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32))
}

pub fn tint_pixmap(pixmap: &mut tiny_skia::Pixmap, color: usvg::Color) {
    for pixel in pixmap.pixels_mut() {
        let a = pixel.alpha();
        if a == 0 {
            continue;
        }
        let r = (color.red as u16 * a as u16 / 255) as u8;
        let g = (color.green as u16 * a as u16 / 255) as u8;
        let b = (color.blue as u16 * a as u16 / 255) as u8;
        *pixel = tiny_skia::PremultipliedColorU8::from_rgba(r, g, b, a).unwrap();
    }
}

pub fn panel_border_color(bg: Color) -> Color {
    let r = bg.red() as f32;
    let g = bg.green() as f32;
    let b = bg.blue() as f32;
    let luminance = 0.299 * r + 0.587 * g + 0.114 * b;

    if luminance > 0.5 {
        Color::from_rgba8(0, 0, 0, 40)
    } else {
        Color::from_rgba8(255, 255, 255, 40)
    }
}

pub fn draw_panel_border(canvas: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, radius: f32, opacity: f32) {
    const BORDER_WIDTH: f32 = 1.0;
    const K: f32 = 0.5522847498;

    let inset = BORDER_WIDTH / 2.0;
    let bx = x + inset;
    let by = y + inset;
    let bw = (w - BORDER_WIDTH).max(0.0);
    let bh = (h - BORDER_WIDTH).max(0.0);
    let r = (radius - inset).max(0.0).min(bw / 2.0).min(bh / 2.0);
    let kr = r * K;

    let mut pb = PathBuilder::new();

    pb.move_to(bx, by + r);
    pb.line_to(bx, by + bh - r);
    pb.cubic_to(bx, by + bh - r + kr, bx + r - kr, by + bh, bx + r, by + bh);

    pb.line_to(bx + bw - r, by + bh);
    pb.cubic_to(
        bx + bw - r + kr, by + bh,
        bx + bw, by + bh - r + kr,
        bx + bw, by + bh - r,
    );

    pb.line_to(bx + bw, by + r);
    pb.cubic_to(
        bx + bw, by + r - kr,
        bx + bw - r + kr, by,
        bx + bw - r, by,
    );
    pb.line_to(bx + r, by);
    pb.cubic_to(
        bx + r - kr, by,
        bx, by + r - kr,
        bx, by + r,
    );
    pb.close();

    let Some(path) = pb.finish() else { return };

    let mut color = panel_border_color(PANEL_COLOR);
    color.set_alpha(color.alpha() * opacity);

    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;

    let stroke = Stroke {
        width: BORDER_WIDTH,
        ..Default::default()
    };

    canvas.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}



pub fn draw_svg_icon(
    canvas: &mut Pixmap,
    icons_cache: &HashMap<&'static str, Tree>,
    svg_str: &'static str,
    icon_size: f32,
    x: f32,
    y: f32,
    tint: usvg::Color,
) {
    let Some(rtree) = icons_cache.get(svg_str) else { return };

    let scale_x = icon_size / rtree.size().width();
    let scale_y = icon_size / rtree.size().height();

    let px_size = icon_size.ceil().max(1.0) as u32;
    let Some(mut icon_pixmap) = Pixmap::new(px_size, px_size) else { return };

    resvg::render(rtree, Transform::from_scale(scale_x, scale_y), &mut icon_pixmap.as_mut());
    tint_pixmap(&mut icon_pixmap, tint);

    canvas.draw_pixmap(
        x.round() as i32,
        y.round() as i32,
        icon_pixmap.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        Transform::identity(),
        None,
    );
}