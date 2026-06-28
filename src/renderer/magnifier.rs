use tiny_skia::{Color, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform};
use crate::types::{MAG_OFFSET, MAG_SIZE, ZOOM, MAG_CELLS};

pub fn magnifier_rect(cursor: (f32, f32), monitor_w: f32, monitor_h: f32) -> Rect {
    let (mag_x, mag_y) = magnifier_position(cursor, (0.0, 0.0, monitor_w, monitor_h));
    Rect::from_xywh(mag_x, mag_y, MAG_SIZE as f32, MAG_SIZE as f32).unwrap()
}

pub fn draw_magnifier(canvas: &mut Pixmap, source: &Pixmap, cursor: (f32, f32)) {
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