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
    BUTTON_SIZE, ICON_SIZE, ModelPopover, ModelPopoverElement, ModelRow, note_font_size, PADDING,
    RADIUS, ROW_HEIGHT, ROW_PAD_X, STATUS_WIDTH, TITLE_HEIGHT, WIDTH, button_geom, row_geom,
};
use crate::ui::panel::UiPanel;

const TITLE: &str = "Languages ·";
const TITLE_ENGLISH: &str = "each includes English";
/// gap between the title's dot and its highlighted part
const TITLE_GAP: f32 = 4.0;
const TITLE_NO_MODEL: &str = "Choose a model to use OCR";
const RECOMMENDED: &str = "Recommended for your system";

const NAME_TOP: f32 = 5.0;
const NAME_HEIGHT: f32 = 20.0;
const NOTE_TOP: f32 = 24.0;
const NOTE_HEIGHT: f32 = 15.0;
const BAR_TOP: f32 = 30.0;

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

    draw_panel_border(canvas, x, y, w, h, RADIUS, popover.opacity);

    popover.pixmap = Some(pixmap);
}

fn draw_content(
    canvas: &mut Pixmap,
    popover: &ModelPopover,
    icons_cache: &HashMap<&'static str, Tree>,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) {
    let (w, h) = popover.size;
    if let Some(bg) = Rect::from_xywh(0.0, 0.0, w, h) {
        fill(canvas, bg, RADIUS, color::panel().color());
    }

    let title_x = PADDING + ROW_PAD_X;
    let title_w = WIDTH - title_x * 2.0;
    let no_model = popover.rows.iter().all(|row| row.status != ModelStatus::Installed);
    let parts: &[(&str, Rgba, f32)] = if no_model {
        &[(TITLE_NO_MODEL, color::accent_bright(), font::label())]
    } else {
        &[
            (TITLE, color::MUTED, note_font_size()),
            (TITLE_ENGLISH, color::accent_bright(), note_font_size()),
        ]
    };
    let mut x = title_x;
    for &(text, text_color, size) in parts {
        if let Some(rect) = Rect::from_xywh(x, PADDING, (title_x + title_w - x).max(1.0), TITLE_HEIGHT) {
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
        x += measure_line_width(text, size, font_system) + TITLE_GAP;
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
    let (Some(rect), Some(button), Some(model)) =
        (row_geom((0.0, 0.0), idx), button_geom((0.0, 0.0), idx), MODELS.get(idx))
    else {
        return;
    };
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
            radius::ITEM,
            stroke::BORDER,
            row_hovered,
            row_selected,
        );
    }

    let left = rect.left() + ROW_PAD_X;
    let text_w = (button.left() - STATUS_WIDTH - left).max(0.0);

    let name_color = if row.active {
        color::accent_bright()
    } else {
        color::ON_PANEL
    };
    if let Some(name_rect) = Rect::from_xywh(left, rect.top() + NAME_TOP, text_w, NAME_HEIGHT) {
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
        ModelStatus::Queued => draw_progress_bar(canvas, left, rect.top() + BAR_TOP, text_w, 0),
        ModelStatus::Downloading(percent) => {
            draw_progress_bar(canvas, left, rect.top() + BAR_TOP, text_w, percent)
        }
        _ => {
            if let Some(note_rect) =
                Rect::from_xywh(left, rect.top() + NOTE_TOP, text_w, NOTE_HEIGHT)
            {
                let (note, note_color) = if row.recommended {
                    (RECOMMENDED, color::accent_bright())
                } else {
                    (model.note, color::MUTED)
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
    if let Some(status_rect) =
        Rect::from_xywh(button.left() - STATUS_WIDTH, rect.top(), STATUS_WIDTH, ROW_HEIGHT)
    {
        draw_aligned_text(
            canvas,
            &status_text,
            font_system,
            swash_cache,
            status_rect,
            note_font_size(),
            color::MUTED.color(),
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
            BUTTON_SIZE,
            BUTTON_SIZE,
            radius::ITEM,
            stroke::BORDER,
            true,
            false,
        );
    }
    let tint = if checked {
        color::accent_bright()
    } else if button_hovered {
        color::accent()
    } else {
        color::ON_PANEL
    };
    draw_svg_icon(
        canvas,
        icons_cache,
        icon,
        ICON_SIZE,
        button.left() + (BUTTON_SIZE - ICON_SIZE) / 2.0,
        button.top() + (BUTTON_SIZE - ICON_SIZE) / 2.0,
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
    canvas.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
}

fn size_label(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}
