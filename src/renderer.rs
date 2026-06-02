use crate::types::{MagnifierState, SelectionEdges, SelectionHandle, MAG_OFFSET, MAG_SIZE, ZOOM, MAG_CELLS};
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform};
use crate::utils::make_rect;






pub fn render_frame(
    canvas: &mut Pixmap,
    base: &Pixmap,          // only for magnifier 
    dimmed: &mut Pixmap,            
    selection: &Option<Rect>,
    prev_selection: &Option<Rect>,
    dirty_rect: Option<&Rect>,
    selection_edges: Option<&SelectionEdges>,
    selection_dirty: bool, 
    magnifier: &Option<MagnifierState>,
    is_mag_monitor: bool,
) {
    if selection_dirty {
        update_dimming_delta(dimmed, base, prev_selection, selection);
    }
    if let Some(dirty) = dirty_rect {
        blit_rect(dimmed, canvas, dirty);
    } else {
        canvas.data_mut().copy_from_slice(dimmed.data());
    }
    if let Some(sel) = selection {
        draw_selection_border(canvas, sel, selection_edges);
    }

    if is_mag_monitor {
        if let Some(mag) = magnifier {
            draw_magnifier(canvas, base, (mag.pos.0 as f32, mag.pos.1 as f32));
        }
    }
}

// ***************************/
//// SELECTION + DIMMING  ////
// **************************/
pub fn init_dimming(
    dimmed: &mut Pixmap,
    base: &Pixmap,
    selection: &Option<Rect>,
) {
    dimmed.data_mut().copy_from_slice(base.data());
    draw_dimming(dimmed, selection, base.width(), base.height());
}

fn draw_selection_border(
    canvas: &mut Pixmap,
    sel: &Rect,
    edges: Option<&SelectionEdges>,
) {
    let mut paint = Paint::default();
    paint.set_color(Color::WHITE);
    paint.anti_alias = true;
    let mut stroke = Stroke::default();
    stroke.width = 2.0;

    if let Some(edges) = edges {
        let half = stroke.width / 2.0;
        let outer = Rect::from_ltrb(
            sel.left() - half,
            sel.top() - half,
            sel.right() + half,
            sel.bottom() + half,
        ).unwrap_or(*sel);
        
        if let Some(path) = rounded_rect_path(&outer, 8.0, edges.top, edges.right, edges.bottom, edges.left) {
            canvas.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }
}

fn rounded_rect_path(
    rect: &Rect,
    r: f32,
    top: bool,
    right: bool,
    bottom: bool,
    left: bool,
) -> Option<tiny_skia::Path> {
    let r = r.min(rect.width() / 2.0).min(rect.height() / 2.0);
    let (l, t, ri, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    const K: f32 = 0.5523;

    let tl = if top && left   { r } else { 0.0 };
    let tr = if top && right  { r } else { 0.0 };
    let br = if bottom && right { r } else { 0.0 };
    let bl = if bottom && left  { r } else { 0.0 };

    let mut pb = PathBuilder::new();

    if top {
        pb.move_to(l + tl, t);
        pb.line_to(ri - tr, t);
    }
    if top && right {
        pb.cubic_to(ri - tr * K, t, ri, t + tr * K, ri, t + tr);
    }
    if right {
        pb.move_to(ri, t + tr);
        pb.line_to(ri, b - br);
    }
    if bottom && right {
        pb.cubic_to(ri, b - br * K, ri - br * K, b, ri - br, b);
    }
    if bottom {
        pb.move_to(ri - br, b);
        pb.line_to(l + bl, b);
    }
    if bottom && left {
        pb.cubic_to(l + bl * K, b, l, b - bl * K, l, b - bl);
    }
    if left {
        pb.move_to(l, b - bl);
        pb.line_to(l, t + tl);
    }
    if top && left {
        pb.cubic_to(l, t + tl * K, l + tl * K, t, l + tl, t);
    }

    pb.finish()
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

fn update_dimming_delta(
    dimmed: &mut Pixmap,
    base: &Pixmap,
    prev: &Option<Rect>,
    next: &Option<Rect>,
) {
    if let Some(old) = prev {
        dim_rect(dimmed, old);
    }
    if let Some(cur) = next {
        blit_rect(base, dimmed, cur);
    }
}

fn dim_rect(canvas: &mut Pixmap, rect: &Rect) {
    let (w, h) = (canvas.width(), canvas.height());
    let Some((x, y, rw, rh)) = rect_bounds(rect, w, h) else {
        return;
    };

    let Some(r) = Rect::from_xywh(x as f32, y as f32, rw as f32, rh as f32) else {
        return;
    };

    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(0, 0, 0, 140));
    let path = PathBuilder::from_rect(r);
    canvas.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
}

fn blit_rect(src: &Pixmap, dst: &mut Pixmap, rect: &Rect) {
    let (w, h) = (dst.width(), dst.height());
    let Some((x, y, rw, rh)) = rect_bounds(rect, w, h) else {
        return;
    };

    let row_bytes = (rw * 4) as usize;
    let src_stride = (src.width() * 4) as usize;
    let dst_stride = (dst.width() * 4) as usize;

    let src_data = src.data();
    let dst_data = dst.data_mut();

    for row in 0..rh {
        let sy = (y + row) as usize;
        let sx = x as usize;
        let src_off = sy * src_stride + sx * 4;

        let dy = sy;
        let dx = sx;
        let dst_off = dy * dst_stride + dx * 4;

        dst_data[dst_off..dst_off + row_bytes]
            .copy_from_slice(&src_data[src_off..src_off + row_bytes]);
    }
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





//********************/
///  MAGNIFIER PART ///
//********************/



pub fn magnifier_rect(cursor: (f32, f32), monitor_w: f32, monitor_h: f32) -> Rect {
    let (mag_x, mag_y) = magnifier_position(cursor, (0.0, 0.0, monitor_w, monitor_h));
    Rect::from_xywh(mag_x, mag_y, MAG_SIZE as f32, MAG_SIZE as f32).unwrap()
}

fn draw_magnifier(canvas: &mut Pixmap, source: &Pixmap, cursor: (f32, f32)) {
    let screen_w = source.width() as f32;
    let screen_h = source.height() as f32;

    let sample_size = MAG_CELLS as i32; 

    let half = (MAG_CELLS / 2) as i32;  
    let src_x = (cursor.0 as i32 - half).max(0).min(screen_w as i32 - sample_size) as u32;
    let src_y = (cursor.1 as i32 - half).max(0).min(screen_h as i32 - sample_size) as u32;

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
    let radius = MAG_SIZE as f32 / 2.0;
    let cx = mag_x + radius;
    let cy = mag_y + radius;

    let mut zoomed = Pixmap::new(MAG_SIZE, MAG_SIZE).unwrap();
    let magnifier_transform = Transform::from_row(ZOOM, 0.0, 0.0, ZOOM, 0.0, 0.0);
    zoomed.draw_pixmap(
        0, 0,
        cropped.as_ref(),
        &PixmapPaint::default(),
        magnifier_transform,
        None,
    );

    overlay_crosshair(&mut zoomed);

    let mut mask = tiny_skia::Mask::new(canvas.width(), canvas.height()).unwrap();
    if let Some(circle_path) = PathBuilder::from_circle(cx, cy, radius) {
        mask.fill_path(&circle_path, tiny_skia::FillRule::Winding, true, Transform::identity());
    }

    canvas.draw_pixmap(
        mag_x as i32,
        mag_y as i32,
        zoomed.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        Some(&mask),
    );

    let mut paint = Paint::default();
    paint.set_color(Color::WHITE);
    paint.anti_alias = true;
    let mut stroke = Stroke::default();
    stroke.width = 2.0;
    if let Some(circle_path) = PathBuilder::from_circle(cx, cy, radius) {
        canvas.stroke_path(&circle_path, &paint, &stroke, Transform::identity(), None);
    }
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

fn overlay_crosshair(zoomed: &mut Pixmap) {
    let cell = ZOOM;
    let w = zoomed.width() as f32;
    let h = zoomed.height() as f32;
    let mut paint = Paint::default();
    paint.anti_alias = false;

    paint.set_color(Color::from_rgba8(255, 255, 255, 40));
    for i in 0..MAG_CELLS as i32 + 1 {
        let x = i as f32 * cell;
        if let Some(r) = Rect::from_xywh(x, 0.0, 1.0, h) {
            zoomed.fill_path(&PathBuilder::from_rect(r), &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
        }
        let y = i as f32 * cell;
        if let Some(r) = Rect::from_xywh(0.0, y, w, 1.0) {
            zoomed.fill_path(&PathBuilder::from_rect(r), &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
        }
    }

    paint.set_color(Color::from_rgba8(180, 180, 180, 80));
    paint.blend_mode = tiny_skia::BlendMode::SourceOver;
    let center_idx = (MAG_CELLS / 2) as f32;
    let col_x = center_idx * cell;
    let row_y = center_idx * cell;
    if let Some(r) = Rect::from_xywh(col_x, 0.0, cell, h) {
        zoomed.fill_path(&PathBuilder::from_rect(r), &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
    }
    if let Some(r) = Rect::from_xywh(0.0, row_y, w, cell) {
        zoomed.fill_path(&PathBuilder::from_rect(r), &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
    }
}


//********************/
///  TOOLBAR SECTION ///
//********************/

fn draw_toolbar(canvas: &mut Pixmap) {}
