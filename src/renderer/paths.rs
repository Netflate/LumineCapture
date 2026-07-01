use tiny_skia::{PathBuilder, Rect};

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
