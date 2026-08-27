use super::paths::{rounded_rect_path, draw_panel_border, tint_pixmap};
use crate::types::icons::get_svg;
use crate::types::panel::PanelItem;
use crate::types::UiPanel;
use crate::types::toolbar::{TOOLBAR_PADDING, ICON_COLOR, SEPARATOR_COLOR, TOOLBAR_COLOR, BUTTON_HOVERED, BUTTON_SELECTED, Toolbar, ToolbarButton, ToolbarItem};
use std::collections::HashMap;
use tiny_skia::{BlendMode, FilterQuality, Paint, Pixmap, PixmapPaint, Rect, Transform};
use usvg::Tree;

pub fn draw_toolbar(
    canvas: &mut Pixmap,
    toolbar: &mut Toolbar,
    icons_cache: &HashMap<ToolbarButton, Tree>,
) {
    let Some(tb_rect) = toolbar.rect() else { return };
    
    let x = tb_rect.left();
    let y = tb_rect.top();
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

    draw_panel_border(canvas, x, y, w, h, 8.0, toolbar.opacity);

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

    let (top_left, top_right, bot_left, bot_right) = (true, true, true, true);
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

    let bg_h = h * 0.80;
    let bg_y = rect.top() + (h - bg_h) / 2.0;

    let mut current_x = rect.left() + TOOLBAR_PADDING;
    for (index, item) in toolbar.items.iter().enumerate() {
        let cell_size = item.size();
        match item {
            ToolbarItem::Button(button) => {
                if toolbar.selected == Some(index) || toolbar.hovered == Some(index) {
                    if let Some(cell_rect) = Rect::from_xywh(current_x, bg_y, cell_size, bg_h) {
                        if let Some(cell_path) =
                            rounded_rect_path(&cell_rect, 4.0, true, true, true, true)
                        {
                            let mut cell_paint = Paint::default();
                            let color = if toolbar.selected == Some(index) {
                                BUTTON_SELECTED
                            } else {
                                BUTTON_HOVERED
                            };
                            cell_paint.set_color(color);
                            cell_paint.anti_alias = true;
                            canvas.fill_path(
                                &cell_path,
                                &cell_paint,
                                tiny_skia::FillRule::Winding,
                                Transform::identity(),
                                None,
                            );
                        }
                    }
                }

                if let Some(rtree) = icons_cache.get(button) {
                    let (_, icon_size) = get_svg(button);

                    let icon_x = current_x + (cell_size - icon_size) / 2.0;
                    let icon_y = rect.top() + (h - icon_size) / 2.0;

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
                let sep_h = h * 0.5; 
                let sep_x = current_x + (cell_size - sep_w) / 2.0;
                let sep_y = rect.top() + (h - sep_h) / 2.0;

                if let Some(sep_rect) = Rect::from_xywh(sep_x, sep_y, sep_w, sep_h) {
                    if let Some(sep_path) = rounded_rect_path(&sep_rect, 1.0, true, true, true, true) {
                        let mut sep_paint = Paint::default();
                        sep_paint.set_color(SEPARATOR_COLOR);
                        sep_paint.anti_alias = true;
                        canvas.fill_path(
                            &sep_path,
                            &sep_paint,
                            tiny_skia::FillRule::Winding,
                            Transform::identity(),
                            None,
                        );
                    }
                }
            }
        }
        current_x += cell_size + item.trailing_padding();
    }
}