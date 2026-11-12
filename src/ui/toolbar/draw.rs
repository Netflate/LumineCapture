use crate::renderer::{Corners, Painter};
use crate::theme::{radius, size};
use crate::ui::icons::{get_svg_for_finish, get_svg_for_tool};
use crate::ui::panel::PanelItem;
use crate::ui::panel::UiPanel;
use crate::ui::toolbar::{Attached, Toolbar, ToolbarButton, ToolbarItem};
use tiny_skia::Rect;

pub fn draw_toolbar(p: &mut Painter, toolbar: &mut Toolbar) {
    let Some(tb_rect) = toolbar.rect() else {
        return;
    };

    let mut cache = toolbar.toolbar_pixmap.take();
    let look = toolbar.look();
    let k = toolbar.shrink;
    p.panel_in(
        &mut cache,
        (tb_rect.left(), tb_rect.top()),
        toolbar.size,
        toolbar.opacity,
        toolbar.drawn_look != Some(look),
        corners(toolbar),
        |p| draw_toolbar_content(&mut p.rescaled(k), toolbar),
    );
    toolbar.toolbar_pixmap = cache;
    toolbar.drawn_look = Some(look);
}

fn corners(toolbar: &Toolbar) -> Corners {
    match (toolbar.docked, toolbar.attached) {
        (false, _) => Corners::All,
        (true, Attached::No) => Corners::Bottom,
        (true, Attached::Part) => Corners::BottomRight,
        (true, Attached::Full) => Corners::Flat,
    }
}

fn draw_toolbar_content(p: &mut Painter, toolbar: &Toolbar) {
    let cfg = &crate::config::get().toolbar;
    let theme = &crate::config::get().theme;
    let (w, h) = toolbar.natural_size();
    let Some(rect) = Rect::from_xywh(0.0, 0.0, w, h) else {
        return;
    };

    let background = if toolbar.docked {
        crate::theme::color::backdrop()
    } else {
        cfg.background.get()
    };
    // border around it isn't shrunk, so neither are the corners
    p.fill_rounded_in(
        rect,
        radius::panel() / toolbar.shrink,
        corners(toolbar),
        background,
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
                {
                    let color = if toolbar.selected == Some(index) {
                        cfg.button_selected.get()
                    } else {
                        cfg.button_hovered.get()
                    };
                    p.fill_rounded(cell_rect, radius::item(), color);
                }

                let (svg_str, icon_size) = match button {
                    ToolbarButton::Tool(tool) => get_svg_for_tool(*tool),
                    ToolbarButton::Finish(finish) => get_svg_for_finish(*finish),
                    ToolbarButton::Scroll => (crate::ui::icons::SCROLL, cfg.icons.scroll),
                };

                let icon_x = current_x + (cell_size - icon_size) / 2.0;
                let icon_y = rect.top() + (h - icon_size) / 2.0;
                let tint = if toolbar.selected == Some(index) {
                    cfg.icon_selected.get()
                } else if toolbar.hovered == Some(index) {
                    cfg.icon_hovered.get()
                } else if cfg.highlight_to
                    && matches!(button, ToolbarButton::Finish(f) if toolbar.to.has(*f))
                {
                    cfg.icon_to.get()
                } else {
                    cfg.icon.get()
                };

                p.icon(svg_str, icon_size, icon_x, icon_y, tint);
            }
            ToolbarItem::Seperator => {
                let sep_w = theme.separator_width;
                let sep_h = h * theme.separator_length;
                let sep_x = current_x + (cell_size - sep_w) / 2.0;
                let sep_y = rect.top() + (h - sep_h) / 2.0;

                if let Some(sep_rect) = Rect::from_xywh(sep_x, sep_y, sep_w, sep_h) {
                    p.fill_rounded(sep_rect, radius::separator(), cfg.separator.get());
                }
            }
        }
        current_x += cell_size + item.trailing_padding();
    }
}
