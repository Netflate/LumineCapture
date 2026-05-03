use crate::types::{MagnifierState, SelectionEdges, SelectionHandle, HANDLE_RADIUS, MAG_OFFSET, MAG_SIZE, ZOOM};
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform};
use crate::utils::make_rect;




pub fn render_frame(
    canvas: &mut Pixmap,
    base: &Pixmap,          // only for magnifier 
    dimmed: &mut Pixmap,            
    selection: &Option<Rect>,
    selection_edges: Option<&SelectionEdges>,
    handle_points: &[(f32, f32)],
    selection_dirty: bool, 
    magnifier: &Option<MagnifierState>,
    is_mag_monitor: bool,
) {
    if selection_dirty {
        dimmed.data_mut().copy_from_slice(base.data());
        draw_dimming(dimmed, selection, base.width(), base.height());
    }
    canvas.data_mut().copy_from_slice(dimmed.data());
    if let Some(sel) = selection {
        draw_selection_border(canvas, sel, selection_edges);
        if !handle_points.is_empty() {
            draw_handles(canvas, handle_points);
        }
    }

    if is_mag_monitor {
        if let Some(mag) = magnifier {
            draw_magnifier(canvas, base, (mag.pos.0 as f32, mag.pos.1 as f32));
        }
    }
}

fn draw_selection_border(
    canvas: &mut Pixmap,
    sel: &Rect,
    edges: Option<&SelectionEdges>,
) {
    let mut paint = Paint::default();
    paint.set_color(Color::WHITE);
    paint.anti_alias = false;
    let mut stroke = Stroke::default();
    stroke.width = 1.0;

    if let Some(edges) = edges {
        let mut pb = PathBuilder::new();
        if edges.top {
            pb.move_to(sel.left(), sel.top());
            pb.line_to(sel.right(), sel.top());
        }
        if edges.right {
            pb.move_to(sel.right(), sel.top());
            pb.line_to(sel.right(), sel.bottom());
        }
        if edges.bottom {
            pb.move_to(sel.left(), sel.bottom());
            pb.line_to(sel.right(), sel.bottom());
        }
        if edges.left {
            pb.move_to(sel.left(), sel.top());
            pb.line_to(sel.left(), sel.bottom());
        }
        if let Some(path) = pb.finish() {
            canvas.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    } else {
        let path = PathBuilder::from_rect(*sel);
        canvas.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

fn draw_dimming(canvas: &mut Pixmap, selection: &Option<Rect>, w: u32, h: u32) {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(0, 0, 0, 140));

    match selection {
        None => {
            let rect = Rect::from_xywh(0.0, 0.0, w as f32, h as f32).unwrap();
            let path = PathBuilder::from_rect(rect);
            canvas.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
        }
        Some(sel) => {
            let rects = [
                Rect::from_xywh(0.0,         0.0,          w as f32,                sel.top()            ),
                Rect::from_xywh(0.0,         sel.bottom(), w as f32,                h as f32 - sel.bottom()),
                Rect::from_xywh(0.0,         sel.top(),    sel.left(),              sel.height()          ),
                Rect::from_xywh(sel.right(),  sel.top(),   w as f32 - sel.right(),  sel.height()          ),
            ];
            for rect in rects {
                if let Some(r) = rect {
                    if r.width() > 0.0 && r.height() > 0.0 {
                        let path = PathBuilder::from_rect(r);
                        canvas.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
                    }
                }
            }
        }
    }
}

fn draw_magnifier(canvas: &mut Pixmap, source: &Pixmap, cursor: (f32, f32)) {
    let screen_w = source.width() as f32;
    let screen_h = source.height() as f32;

    let sample_size = (MAG_SIZE as f32 / ZOOM) as i32;
    let src_x = (cursor.0 as i32 - sample_size / 2).max(0).min(screen_w as i32 - sample_size) as u32;
    let src_y = (cursor.1 as i32 - sample_size / 2).max(0).min(screen_h as i32 - sample_size) as u32;

    let mut cropped = Pixmap::new(sample_size as u32, sample_size as u32).unwrap();
    cropped.draw_pixmap(
        -(src_x as i32),
        -(src_y as i32),
        source.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );

    let (mag_x, mag_y) = magnifier_position(cursor, (0.0, 0.0, screen_w, screen_h));

    let magnifier_transform = Transform::from_row(ZOOM, 0.0, 0.0, ZOOM, mag_x, mag_y);
    canvas.draw_pixmap(
        0,
        0,
        cropped.as_ref(),
        &PixmapPaint::default(),
        magnifier_transform,
        None,
    );

    let mut paint = Paint::default();
    paint.set_color(Color::WHITE);
    paint.anti_alias = true;
    let rect = Rect::from_xywh(mag_x, mag_y, MAG_SIZE as f32, MAG_SIZE as f32).unwrap();
    let path = PathBuilder::from_rect(rect);
    let mut stroke = Stroke::default();
    stroke.width = 2.0;
    canvas.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}


fn magnifier_position(cursor: (f32, f32), monitor: (f32, f32, f32, f32)) -> (f32, f32) {
    let mag = MAG_SIZE as f32;
    let (cx, cy) = cursor;
    let (mx, my, mw, mh) = monitor;

    let x = if cx + mag + MAG_OFFSET < mx + mw {
        cx + MAG_OFFSET
    } else {
        cx - mag - MAG_OFFSET
    };

    let y = if cy + mag + MAG_OFFSET < my + mh {
        cy + MAG_OFFSET
    } else {
        cy - mag - MAG_OFFSET
    };

    (x, y)
}



pub fn apply_handle_drag(
    orig: &Rect,
    handle: SelectionHandle,
    delta: (f64, f64),
) -> Option<Rect> {
    let (dx, dy) = delta;
    let (mut l, mut r, mut t, mut b) = (
        orig.left() as f64, orig.right() as f64,
        orig.top() as f64, orig.bottom() as f64,
    );

    match handle {
        SelectionHandle::TopLeft     => { l += dx; t += dy; }
        SelectionHandle::Top         => { t += dy; }
        SelectionHandle::TopRight    => { r += dx; t += dy; }
        SelectionHandle::Left        => { l += dx; }
        SelectionHandle::Right       => { r += dx; }
        SelectionHandle::BottomLeft  => { l += dx; b += dy; }
        SelectionHandle::Bottom      => { b += dy; }
        SelectionHandle::BottomRight => { r += dx; b += dy; }
        SelectionHandle::Move        => { l += dx; r += dx; t += dy; b += dy; }
        SelectionHandle::None        => {}
    }

    make_rect(
        (l.min(r), t.min(b)),
        (l.max(r), t.max(b)),
    )
}

fn draw_handles(canvas: &mut Pixmap, handles: &[(f32, f32)]) {
    for (hx, hy) in handles {
        draw_handle_dot(canvas, *hx, *hy);
    }
}

fn draw_handle_dot(canvas: &mut Pixmap, x: f32, y: f32) {
    let size = HANDLE_RADIUS as f32;
    let half = size / 2.0;
    let rect = Rect::from_xywh(x - half, y - half, size, size);
    if let Some(r) = rect {
        let mut paint = Paint::default();
        paint.set_color(Color::WHITE);
        let path = PathBuilder::from_rect(r);
        canvas.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
    }
}