use super::paths::rounded_rect_path;
use crate::types::icons::get_svg;
use crate::types::toolbar::{TOOLBAR_PADDING, ICON_COLOR, SEPARATOR_COLOR, TOOLBAR_COLOR, BUTTON_HOVERED, BUTTON_SELECTED, Toolbar, ToolbarButton, ToolbarItem, ToolbarSide};
use std::collections::HashMap;
use tiny_skia::{BlendMode, FilterQuality, Paint, Pixmap, PixmapPaint, Rect, Transform};
use usvg::Tree;

pub fn draw_toolbar(
    canvas: &mut Pixmap,
    toolbar: &mut Toolbar,
    icons_cache: &HashMap<ToolbarButton, Tree>,
) {
    let (x, y) = (toolbar.position.0, toolbar.render_y);
    let (w, h) = toolbar.size;
    let pw = w.ceil() as u32;
    let ph = h.ceil() as u32;

    let needs_resize = toolbar
        .toolbar_pixmap
        .as_ref()
        .is_none_or(|p| p.width() != pw || p.height() != ph);

    if needs_resize {
        toolbar.toolbar_pixmap = Pixmap::new(pw, ph);
        toolbar.dirty = true;
    }

    let Some(mut toolbar_pixmap) = toolbar.toolbar_pixmap.take() else {
        return;
    };

    if toolbar.dirty {
        toolbar_pixmap.fill(tiny_skia::Color::TRANSPARENT);
        draw_toolbar_content(&mut toolbar_pixmap, toolbar, icons_cache);
    }

    canvas.draw_pixmap(
        x as i32,
        y as i32,
        toolbar_pixmap.as_ref(),
        &PixmapPaint {
            opacity: toolbar.opacity,
            blend_mode: BlendMode::SourceOver,
            quality: FilterQuality::Nearest,
        },
        Transform::identity(),
        None,
    );

    toolbar.toolbar_pixmap = Some(toolbar_pixmap);
}

fn draw_toolbar_content(
    canvas: &mut Pixmap,
    toolbar: &Toolbar,
    icons_cache: &HashMap<ToolbarButton, Tree>,
) {
    let (w, h) = toolbar.size;
    let Some(rect) = Rect::from_xywh(0.0, 0.0, w, h) else {
        return;
    };

    let (top_left, top_right, bot_left, bot_right) = match toolbar.current_side {
        ToolbarSide::Top => (false, false, true, true),
        _ => (true, true, false, false),
    };
    let Some(path) = rounded_rect_path(&rect, 8.0, top_left, top_right, bot_left, bot_right) else {
        return;
    };

    let mut paint = Paint::default();
    paint.set_color(TOOLBAR_COLOR);
    paint.anti_alias = true;
    canvas.fill_path(
        &path,
        &paint,
        tiny_skia::FillRule::Winding,
        Transform::identity(),
        None,
    );

    let mut current_x = rect.left() + TOOLBAR_PADDING;
    for (index, item) in toolbar.items.iter().enumerate() {
        let cell_size = item.size();
        match item {
            ToolbarItem::Button(button) => {
                if let Some(rtree) = icons_cache.get(button) {
                    let (_, icon_size) = get_svg(button);
                    let padding = (cell_size - icon_size) / 2.0;
                    let icon_x = current_x + padding;
                    let icon_y = rect.top() + padding;

                    if (toolbar.selected == Some(index) || toolbar.hovered == Some(index))
                        && let Some(cell_rect) =
                            Rect::from_xywh(current_x, rect.top(), cell_size, cell_size)
                    {
                        let mut cell_paint = Paint::default();
                        let color = if toolbar.selected == Some(index) {
                            BUTTON_SELECTED
                        } else {
                            BUTTON_HOVERED
                        };
                        cell_paint.set_color(color);
                        cell_paint.anti_alias = true;
                        canvas.fill_rect(cell_rect, &cell_paint, Transform::identity(), None);
                    }

                    let scale_x = icon_size / rtree.size().width();
                    let scale_y = icon_size / rtree.size().height();

                    // Render icon into its own pixmap so recoloring doesn't
                    // touch the toolbar background/hover cells.
                    let px_size = icon_size.ceil().max(1.0) as u32;
                    if let Some(mut icon_pixmap) = Pixmap::new(px_size, px_size) {
                        let transform = Transform::from_scale(scale_x, scale_y);
                        resvg::render(rtree, transform, &mut icon_pixmap.as_mut());
                        tint_pixmap(&mut icon_pixmap, ICON_COLOR);

                        canvas.draw_pixmap(
                            icon_x.round() as i32,
                            icon_y.round() as i32,
                            icon_pixmap.as_ref(),
                            &tiny_skia::PixmapPaint::default(),
                            Transform::identity(),
                            None,
                        );
                    }
                }
            }
            ToolbarItem::Seperator => {
                let sep_w = 2.0;
                let sep_h = 20.0;
                let sep_x = current_x + (cell_size - sep_w) / 2.0;
                let sep_y = rect.top() + (h - sep_h) / 2.0;
                if let Some(sep_rect) = Rect::from_xywh(sep_x, sep_y, sep_w, sep_h) {
                    let mut sep_paint = Paint::default();
                    sep_paint.set_color(SEPARATOR_COLOR);
                    sep_paint.anti_alias = true;
                    canvas.fill_rect(sep_rect, &sep_paint, Transform::identity(), None);
                }
            }
        }
        current_x += cell_size + item.trailing_padding();
    }
}

fn tint_pixmap(pixmap: &mut tiny_skia::Pixmap, color: usvg::Color) {
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