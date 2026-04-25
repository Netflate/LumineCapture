use crate::types::{EditorState, OutputInfo};
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke, Transform};

const ZOOM: f32 = 4.5;
const MAG_SIZE: u32 = 160;
const MAG_OFFSET: f32 = 24.0;

pub fn render_frame(state: &EditorState, outputs: &[OutputInfo]) -> (Vec<u8>, u32, u32) {
    let w = state.base.width();
    let h = state.base.height();
    let mut canvas = state.base.clone();

    draw_dimming(&mut canvas, &state.selection, w, h);
    draw_magnifier(&mut canvas, &state.base, (state.pointer.0 as f32, state.pointer.1 as f32), outputs);

    (canvas.take(), w, h)
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

fn draw_magnifier(canvas: &mut Pixmap, source: &Pixmap, cursor: (f32, f32), outputs: &[OutputInfo]) {
    let screen_w = source.width();
    let screen_h = source.height();

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

    let current_monitor = find_monitor(cursor, outputs);
    let (mag_x, mag_y) = magnifier_position(cursor, current_monitor);

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

fn find_monitor(cursor: (f32, f32), outputs: &[OutputInfo]) -> (f32, f32, f32, f32) {
    outputs.iter()
        .find(|o| {
            cursor.0 >= o.x as f32
                && cursor.0 < (o.x + o.width) as f32
                && cursor.1 >= o.y as f32
                && cursor.1 < (o.y + o.height) as f32
        })
        .map(|o| (o.x as f32, o.y as f32, o.width as f32, o.height as f32))
        .unwrap_or((0.0, 0.0, 99999.0, 99999.0)) 
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