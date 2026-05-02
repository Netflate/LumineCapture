use crate::types::{MagnifierState};
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform};

const ZOOM: f32 = 4.5;
const MAG_SIZE: u32 = 160;
const MAG_OFFSET: f32 = 24.0;

pub fn render_frame(
    canvas: &mut Pixmap,
    base: &Pixmap,          // only for magnifier 
    dimmed: &mut Pixmap,            
    selection: &Option<Rect>,
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
        draw_selection_border(canvas, sel);
    }

    if is_mag_monitor {
        if let Some(mag) = magnifier {
            draw_magnifier(canvas, base, (mag.pos.0 as f32, mag.pos.1 as f32));
        }
    }
}

fn draw_selection_border(canvas: &mut Pixmap, sel: &Rect) {
    let mut paint = Paint::default();
    paint.set_color(Color::WHITE);
    paint.anti_alias = false;
    let path = PathBuilder::from_rect(*sel);
    let mut stroke = Stroke::default();
    stroke.width = 1.0;
    canvas.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
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