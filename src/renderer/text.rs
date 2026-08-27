use cosmic_text::{Attrs, Buffer, FontSystem, Metrics, Shaping, SwashCache, SwashContent};
use tiny_skia::{Color, Pixmap, PixmapPaint, PremultipliedColorU8, Rect, Transform};

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
) -> (Buffer, f32, f32) {
    let line_height = font_size * 1.1;
    let mut buffer = Buffer::new(font_system, Metrics::new(font_size, line_height));

    buffer.set_size(None, None);
    buffer.set_text(text, &Attrs::new().weight(weight), Shaping::Advanced, None);
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
) {
    if text.is_empty() {
        return;
    }

    let (buffer, text_w, text_h) =
        shape_single_line(font_system, text, font_size, weight); 

    let px = match align {
        HAlign::Left => rect.left(),
        HAlign::Center => rect.left() + (rect.width() - text_w) / 2.0,
    };
    let py = rect.top() + (rect.height() - text_h) / 2.0;

    draw_text_buffer(canvas, &buffer, font_system, swash_cache, (px, py), color, offset);
}