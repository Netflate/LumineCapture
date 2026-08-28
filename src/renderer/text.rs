use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping, SwashCache, SwashContent};
use tiny_skia::{Color, Paint, Pixmap, PixmapPaint, PremultipliedColorU8, Rect, Transform};

use crate::types::text_field::LineEditState;
use super::paths::rounded_rect_path;

pub enum HAlign {
    Left,
    Center,
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
                let width = image.placement.width;
                let height = image.placement.height;

                if width == 0 || height == 0 {
                    continue;
                }

                let draw_x =
                    (x_start + physical.x as f32 + image.placement.left as f32 - offset.0) as i32;
                let draw_y = (y_start + run.line_y - image.placement.top as f32 - offset.1) as i32;

                if image.content == SwashContent::Mask
                    && let Some(mut glyph_pixmap) = Pixmap::new(width, height)
                {
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
        }
    }
}

pub fn shape_single_line(
    font_system: &mut FontSystem,
    text: &str,
    font_size: f32,
    weight: cosmic_text::Weight,
    style: cosmic_text::Style, 
) -> (Buffer, f32, f32) {
    let line_height = font_size * 1.1;
    let mut buffer = Buffer::new(font_system, Metrics::new(font_size, line_height));

    buffer.set_size(None, None);
    buffer.set_text(
        text,
        &Attrs::new().weight(weight).style(style),   
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

    (buffer, text_width, text_height)
}

pub fn draw_aligned_text(
    canvas: &mut Pixmap,
    text: &str,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    rect: Rect,
    font_size: f32,
    color: Color,
    align: HAlign,
    offset: (f32, f32),
    weight: cosmic_text::Weight,
    style: cosmic_text::Style,        // ← новый параметр
) {
    if text.is_empty() {
        return;
    }

    let (buffer, text_w, text_h) =
        shape_single_line(font_system, text, font_size, weight, style);

    let px = match align {
        HAlign::Left => rect.left(),
        HAlign::Center => rect.left() + (rect.width() - text_w) / 2.0,
    };
    let py = rect.top() + (rect.height() - text_h) / 2.0;

    draw_text_buffer(canvas, &buffer, font_system, swash_cache, (px, py), color, offset);
}

// ── common input field stuff ─────────────────────────────────────
pub fn draw_input_box(canvas: &mut Pixmap, rect: Rect, radius: f32) {
    let Some(path) = rounded_rect_path(&rect, radius, true, true, true, true) else { return };

    let mut fill_paint = Paint::default();
    fill_paint.set_color(Color::from_rgba8(255, 255, 255, 18));
    fill_paint.anti_alias = true;
    canvas.fill_path(&path, &fill_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
}

pub fn measure_text_prefix_width(
    text: &str,
    upto_byte: usize,
    font_size: f32,
    font_system: &mut FontSystem,
) -> f32 {
    if upto_byte == 0 || text.is_empty() {
        return 0.0;
    }

    let metrics = Metrics::new(font_size, font_size * 1.2);
    let mut buffer = Buffer::new_empty(metrics);
    buffer.set_size(None, None);
    buffer.set_text(
        text,
        &Attrs::new().family(cosmic_text::Family::SansSerif),
        Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(font_system, false);

    for run in buffer.layout_runs() {
        for glyph in run.glyphs.iter() {
            if glyph.start >= upto_byte {
                return glyph.x;
            }
        }
        return run.line_w;
    }
    0.0
}

pub fn draw_text_selection(canvas: &mut Pixmap, rect: Rect, start_x: f32, end_x: f32) {
    let sel_h = rect.height() * 0.75;
    let sel_y = rect.top() + (rect.height() - sel_h) / 2.0;
    let Some(sel_rect) = Rect::from_xywh(rect.left() + start_x, sel_y, (end_x - start_x).max(1.0), sel_h) else {
        return;
    };

    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(100, 150, 255, 110));
    paint.anti_alias = true;
    canvas.fill_rect(sel_rect, &paint, Transform::identity(), None);
}

pub fn draw_text_caret(canvas: &mut Pixmap, rect: Rect, cursor_x: f32) {
    let cur_h = rect.height() * 0.75;
    let cur_y = rect.top() + (rect.height() - cur_h) / 2.0;
    let Some(cur_rect) = Rect::from_xywh((rect.left() + cursor_x).round(), cur_y, 1.5, cur_h) else {
        return;
    };

    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(255, 255, 255, 220));
    paint.anti_alias = false;
    canvas.fill_rect(cur_rect, &paint, Transform::identity(), None);
}

pub fn draw_line_edit(
    canvas: &mut Pixmap,
    rect: Rect,
    display_text: &str,
    editing: Option<&LineEditState>,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    font_size: f32,
    text_color: Color,
    weight: cosmic_text::Weight,
) {
    if let Some(field) = editing {
        let raw_text = &field.text;

        if let Some((start_byte, end_byte)) = field.selection_byte_range() {
            let sx = measure_text_prefix_width(raw_text, start_byte, font_size, font_system);
            let ex = measure_text_prefix_width(raw_text, end_byte, font_size, font_system);
            draw_text_selection(canvas, rect, sx, ex);
        }

        let cx = measure_text_prefix_width(raw_text, field.cursor_byte(), font_size, font_system);
        draw_text_caret(canvas, rect, cx);
    }

    if !display_text.is_empty() {
        draw_aligned_text(canvas, display_text, font_system, swash_cache, rect, font_size, text_color, HAlign::Left, (0.0, 0.0), weight, cosmic_text::Style::Normal);
    }
}