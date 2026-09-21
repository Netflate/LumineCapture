use std::collections::HashMap;

use cosmic_text::{FontSystem, Style, SwashCache, Weight};
use tiny_skia::{
    BlendMode, Color, FillRule, FilterQuality, Paint, Pixmap, PixmapPaint, Rect, Transform,
};
use usvg::Tree;

use crate::ocr::models::{MODELS, ModelStatus};
use crate::renderer::paths::{
    draw_item_border, draw_panel_border, draw_progress_bar, draw_svg_icon, rounded_rect_path,
};
use crate::renderer::text::{HAlign, draw_aligned_text, measure_line_width};
use crate::theme::{Rgba, color, font, radius, stroke};
use crate::ui::icons;
use crate::ui::model_popover::{
    ModelPopover, ModelPopoverElement, ModelRow, button_geom, cfg, note_font_size, row_geom,
};
use crate::ui::panel::UiPanel;

const TITLE: &str = "Languages ·";
const TITLE_ENGLISH: &str = "each includes English";
const TITLE_NO_MODEL: &str = "Choose a model to use OCR";
const RECOMMENDED: &str = "Recommended for your system";

pub fn draw_model_popover(
    canvas: &mut Pixmap,
    popover: &mut ModelPopover,
    icons_cache: &HashMap<&'static str, Tree>,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) {
    let Some(rect) = popover.rect() else {
        return;
    };

    // rounding is obligatory to avoid desync and visual glitches
    let x = rect.left().round();
    let y = rect.top().round();

    let (w, h) = popover.size;
    let pw = w.ceil() as u32;
    let ph = h.ceil() as u32;

    let needs_resize = popover
        .pixmap
        .as_ref()
        .is_none_or(|p| p.width() != pw || p.height() != ph);
    if needs_resize {
        popover.pixmap = Pixmap::new(pw, ph);
    }

    let Some(mut pixmap) = popover.pixmap.take() else {
        return;
    };

    if popover.dirty {
        pixmap.fill(Color::TRANSPARENT);
        draw_content(&mut pixmap, popover, icons_cache, font_system, swash_cache);
    }

    canvas.draw_pixmap(
        x as i32,
        y as i32,
        pixmap.as_ref(),
        &PixmapPaint {
            opacity: popover.opacity,
            blend_mode: BlendMode::SourceOver,
            quality: FilterQuality::Nearest,
        },
        Transform::identity(),
        None,
    );

    draw_panel_border(canvas, x, y, w, h, radius::panel(), popover.opacity);

    popover.pixmap = Some(pixmap);
}

fn draw_content(
    canvas: &mut Pixmap,
    popover: &ModelPopover,
    icons_cache: &HashMap<&'static str, Tree>,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) {
    let cfg = cfg();
    let (w, h) = popover.size;
    if let Some(bg) = Rect::from_xywh(0.0, 0.0, w, h) {
        fill(canvas, bg, radius::panel(), cfg.background.get().color());
    }

    let title_x = cfg.padding + cfg.row_padding;
    let title_w = cfg.width - title_x * 2.0;
    let no_model = popover
        .rows
        .iter()
        .all(|row| row.status != ModelStatus::Installed);
    let parts: &[(&str, Rgba, f32)] = if no_model {
        &[(TITLE_NO_MODEL, color::active(), font::label())]
    } else {
        &[
            (TITLE, color::muted(), note_font_size()),
            (TITLE_ENGLISH, color::active(), note_font_size()),
        ]
    };
    let mut x = title_x;
    for &(text, text_color, size) in parts {
        if let Some(rect) = Rect::from_xywh(
            x,
            cfg.padding,
            (title_x + title_w - x).max(1.0),
            cfg.title_height,
        ) {
            draw_aligned_text(
                canvas,
                text,
                font_system,
                swash_cache,
                rect,
                size,
                text_color.color(),
                HAlign::Left,
                (0.0, 0.0),
                Weight::NORMAL,
                Style::Normal,
            );
        }
        // title_gap: between the title's dot and its highlighted part
        x += measure_line_width(text, size, font_system) + cfg.title_gap;
    }

    for (idx, row) in popover.rows.iter().enumerate() {
        draw_row(
            canvas,
            idx,
            *row,
            popover.hovered,
            icons_cache,
            font_system,
            swash_cache,
        );
    }
}

fn draw_row(
    canvas: &mut Pixmap,
    idx: usize,
    row: ModelRow,
    hovered: Option<ModelPopoverElement>,
    icons_cache: &HashMap<&'static str, Tree>,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) {
    let (Some(rect), Some(button), Some(model)) = (
        row_geom((0.0, 0.0), idx),
        button_geom((0.0, 0.0), idx),
        MODELS.get(idx),
    ) else {
        return;
    };
    let cfg = cfg();
    let row_hovered = hovered == Some(ModelPopoverElement::Row(idx));
    let button_hovered = hovered == Some(ModelPopoverElement::Button(idx));

    let row_selected = row.active || row.recommended;
    if row_hovered || row_selected {
        draw_item_border(
            canvas,
            rect.left(),
            rect.top(),
            rect.width(),
            rect.height(),
            radius::item(),
            stroke::border(),
            row_hovered,
            row_selected,
        );
    }

    let left = rect.left() + cfg.row_padding;
    let text_w = (button.left() - cfg.status_width - left).max(0.0);

    let name_color = if row.active {
        color::active()
    } else {
        color::foreground()
    };
    if let Some(name_rect) =
        Rect::from_xywh(left, rect.top() + cfg.name_top, text_w, cfg.name_height)
    {
        draw_aligned_text(
            canvas,
            model.name,
            font_system,
            swash_cache,
            name_rect,
            font::label(),
            name_color.color(),
            HAlign::Left,
            (0.0, 0.0),
            Weight::NORMAL,
            Style::Normal,
        );
    }

    match row.status {
        ModelStatus::Queued => draw_progress_bar(canvas, left, rect.top() + cfg.bar_top, text_w, 0),
        ModelStatus::Downloading(percent) => {
            draw_progress_bar(canvas, left, rect.top() + cfg.bar_top, text_w, percent)
        }
        _ => {
            if let Some(note_rect) =
                Rect::from_xywh(left, rect.top() + cfg.note_top, text_w, cfg.note_height)
            {
                let (note, note_color) = if row.recommended {
                    (RECOMMENDED, color::active())
                } else {
                    (model.note, color::muted())
                };
                draw_aligned_text(
                    canvas,
                    note,
                    font_system,
                    swash_cache,
                    note_rect,
                    note_font_size(),
                    note_color.color(),
                    HAlign::Left,
                    (0.0, 0.0),
                    Weight::NORMAL,
                    Style::Normal,
                );
            }
        }
    }

    let status_text = match row.status {
        ModelStatus::Missing => size_label(row.size),
        ModelStatus::Queued => "Waiting".to_string(),
        ModelStatus::Downloading(percent) => format!("{percent}%"),
        ModelStatus::Failed => "Failed".to_string(),
        ModelStatus::Installed => String::new(),
    };
    if let Some(status_rect) = Rect::from_xywh(
        button.left() - cfg.status_width,
        rect.top(),
        cfg.status_width,
        cfg.row_height,
    ) {
        draw_aligned_text(
            canvas,
            &status_text,
            font_system,
            swash_cache,
            status_rect,
            note_font_size(),
            color::muted().color(),
            HAlign::Center,
            (0.0, 0.0),
            Weight::NORMAL,
            Style::Normal,
        );
    }

    let checked = row.active && row.status == ModelStatus::Installed;
    let icon = match row.status {
        ModelStatus::Missing => icons::DOWNLOAD,
        ModelStatus::Failed => icons::RETRY,
        ModelStatus::Queued | ModelStatus::Downloading(_) => icons::CLOSE,
        ModelStatus::Installed if checked => icons::CHECK,
        ModelStatus::Installed if row_hovered || button_hovered => icons::TRASH,
        ModelStatus::Installed => return,
    };

    if !checked && button_hovered {
        draw_item_border(
            canvas,
            button.left(),
            button.top(),
            cfg.button_size,
            cfg.button_size,
            radius::item(),
            stroke::border(),
            true,
            false,
        );
    }
    let tint = if checked {
        color::active()
    } else if button_hovered {
        color::hover()
    } else {
        color::foreground()
    };
    draw_svg_icon(
        canvas,
        icons_cache,
        icon,
        cfg.icon_size,
        button.left() + (cfg.button_size - cfg.icon_size) / 2.0,
        button.top() + (cfg.button_size - cfg.icon_size) / 2.0,
        tint.usvg(),
    );
}

fn fill(canvas: &mut Pixmap, rect: Rect, radius: f32, color: Color) {
    let Some(path) = rounded_rect_path(&rect, radius, true, true, true, true) else {
        return;
    };
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    canvas.fill_path(
        &path,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

fn size_label(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}
