use crate::theme::Rgba;
use crate::ui::magnifier::{CELLS, OFFSET, SIZE, ZOOM};
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform};

const GRID: Rgba = Rgba(255, 255, 255, 40);
const GRID_SOFT: Rgba = Rgba(180, 180, 180, 80);

pub fn magnifier_rect(cursor: (f32, f32), monitor_w: f32, monitor_h: f32) -> Rect {
    let (mag_x, mag_y) = magnifier_position(cursor, (0.0, 0.0, monitor_w, monitor_h));
    Rect::from_xywh(mag_x, mag_y, SIZE as f32, SIZE as f32).unwrap()
}

pub fn draw_magnifier(canvas: &mut Pixmap, source: &Pixmap, cursor: (f32, f32)) {
    let screen_w = source.width() as f32;
    let screen_h = source.height() as f32;

    let sample_size = CELLS as i32;

    let half = (CELLS / 2) as i32;
    let src_x = (cursor.0 as i32 - half)
        .max(0)
        .min(screen_w as i32 - sample_size) as u32;
    let src_y = (cursor.1 as i32 - half)
        .max(0)
        .min(screen_h as i32 - sample_size) as u32;

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
    let radius = SIZE as f32 / 2.0;
    let cx = mag_x + radius;
    let cy = mag_y + radius;

    let mut zoomed = Pixmap::new(SIZE, SIZE).unwrap();
    let magnifier_transform = Transform::from_row(ZOOM, 0.0, 0.0, ZOOM, 0.0, 0.0);
    zoomed.draw_pixmap(
        0,
        0,
        cropped.as_ref(),
        &PixmapPaint::default(),
        magnifier_transform,
        None,
    );

    overlay_crosshair(&mut zoomed);

    let mut mask = tiny_skia::Mask::new(SIZE, SIZE).unwrap();
    if let Some(circle_path) = PathBuilder::from_circle(radius, radius, radius) {
        mask.fill_path(
            &circle_path,
            tiny_skia::FillRule::Winding,
            true,
            Transform::identity(),
        );
    }
    zoomed.apply_mask(&mask);

    canvas.draw_pixmap(
        mag_x as i32,
        mag_y as i32,
        zoomed.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
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
    let mag = SIZE as f32;
    let (cx, cy) = cursor;
    let (mx, my, mw, mh) = monitor;

    let x = if cx + mag + OFFSET < mx + mw {
        cx + OFFSET
    } else {
        cx - mag - OFFSET
    };

    let y = if cy + mag + OFFSET < my + mh {
        cy + OFFSET
    } else {
        cy - mag - OFFSET
    };

    (x, y)
}

fn overlay_crosshair(zoomed: &mut Pixmap) {
    let cell = ZOOM;
    let w = zoomed.width() as f32;
    let h = zoomed.height() as f32;
    let mut paint = Paint::default();
    paint.anti_alias = false;

    paint.set_color(GRID.color());
    for i in 0..CELLS as i32 + 1 {
        let x = i as f32 * cell;
        if let Some(r) = Rect::from_xywh(x, 0.0, 1.0, h) {
            zoomed.fill_path(
                &PathBuilder::from_rect(r),
                &paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        let y = i as f32 * cell;
        if let Some(r) = Rect::from_xywh(0.0, y, w, 1.0) {
            zoomed.fill_path(
                &PathBuilder::from_rect(r),
                &paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    paint.set_color(GRID_SOFT.color());
    paint.blend_mode = tiny_skia::BlendMode::SourceOver;
    let center_idx = (CELLS / 2) as f32;
    let col_x = center_idx * cell;
    let row_y = center_idx * cell;
    if let Some(r) = Rect::from_xywh(col_x, 0.0, cell, h) {
        zoomed.fill_path(
            &PathBuilder::from_rect(r),
            &paint,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
    if let Some(r) = Rect::from_xywh(0.0, row_y, w, cell) {
        zoomed.fill_path(
            &PathBuilder::from_rect(r),
            &paint,
            tiny_skia::FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}
