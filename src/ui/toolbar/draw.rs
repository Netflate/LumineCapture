use crate::renderer::paths::{draw_panel_border, draw_svg_icon, rounded_rect_path};
use crate::theme::{radius, size};
use crate::ui::icons::{get_svg_for_finish, get_svg_for_tool};
use crate::ui::panel::PanelItem;
use crate::ui::panel::UiPanel;
use crate::ui::toolbar::{Toolbar, ToolbarButton, ToolbarItem};
use std::collections::HashMap;
use tiny_skia::{BlendMode, FilterQuality, Paint, Pixmap, PixmapPaint, Rect, Transform};
use usvg::Tree;

pub fn draw_toolbar(
    canvas: &mut Pixmap,
    toolbar: &mut Toolbar,
    icons_cache: &HashMap<&'static str, Tree>,
) {
    let Some(tb_rect) = toolbar.rect() else {
        return;
    };

    let x = tb_rect.left().round();
    let y = tb_rect.top().round();
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

    draw_panel_border(canvas, x, y, w, h, radius::panel(), toolbar.opacity);

    toolbar.toolbar_pixmap = Some(toolbar_pixmap);
}

fn draw_toolbar_content(
    canvas: &mut Pixmap,
    toolbar: &Toolbar,
    icons_cache: &HashMap<&'static str, Tree>,
) {
    let cfg = &crate::config::get().toolbar;
    let theme = &crate::config::get().theme;
    let (w, h) = toolbar.size;
    let Some(rect) = Rect::from_xywh(0.0, 0.0, w, h) else {
        return;
    };

    let (top_left, top_right, bot_left, bot_right) = (true, true, true, true);
    let Some(path) = rounded_rect_path(
        &rect,
        radius::panel(),
        top_left,
        top_right,
        bot_left,
        bot_right,
    ) else {
        return;
    };

    let mut paint = Paint::default();
    paint.set_color(cfg.background.get().color());
    paint.anti_alias = true;
    canvas.fill_path(
        &path,
        &paint,
        tiny_skia::FillRule::Winding,
        Transform::identity(),
        None,
    );

    let bg_h = h * cfg.highlight_height;
    let bg_y = rect.top() + (h - bg_h) / 2.0;

    let mut current_x = rect.left() + size::padding();
    for (index, item) in toolbar.items.iter().enumerate() {
        let cell_size = item.size();
        match item {
            ToolbarItem::Button(button) => {
                if (toolbar.selected == Some(index) || toolbar.hovered == Some(index))
                    && let Some(cell_rect) = Rect::from_xywh(current_x, bg_y, cell_size, bg_h)
                    && let Some(cell_path) =
                        rounded_rect_path(&cell_rect, radius::item(), true, true, true, true)
                {
                    let mut cell_paint = Paint::default();
                    let color = if toolbar.selected == Some(index) {
                        cfg.button_selected.get().color()
                    } else {
                        cfg.button_hovered.get().color()
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

                let (svg_str, icon_size) = match button {
                    ToolbarButton::Tool(tool) => get_svg_for_tool(*tool),
                    ToolbarButton::Finish(finish) => get_svg_for_finish(*finish),
                };

                let icon_x = current_x + (cell_size - icon_size) / 2.0;
                let icon_y = rect.top() + (h - icon_size) / 2.0;
                let tint = if toolbar.selected == Some(index) {
                    cfg.icon_selected.get()
                } else if toolbar.hovered == Some(index) {
                    cfg.icon_hovered.get()
                } else {
                    cfg.icon.get()
                };

                draw_svg_icon(
                    canvas,
                    icons_cache,
                    svg_str,
                    icon_size,
                    icon_x,
                    icon_y,
                    tint,
                );
            }
            ToolbarItem::Seperator => {
                let sep_w = theme.separator_width;
                let sep_h = h * theme.separator_length;
                let sep_x = current_x + (cell_size - sep_w) / 2.0;
                let sep_y = rect.top() + (h - sep_h) / 2.0;

                if let Some(sep_rect) = Rect::from_xywh(sep_x, sep_y, sep_w, sep_h)
                    && let Some(sep_path) =
                        rounded_rect_path(&sep_rect, radius::separator(), true, true, true, true)
                {
                    let mut sep_paint = Paint::default();
                    sep_paint.set_color(cfg.separator.get().color());
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
        current_x += cell_size + item.trailing_padding();
    }
}
