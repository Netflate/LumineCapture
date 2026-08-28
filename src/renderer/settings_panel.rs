use super::paths::{draw_svg_icon, rounded_rect_path, draw_panel_border, draw_item_border};
use super::text::{draw_aligned_text, draw_line_edit, HAlign};
use crate::types::panel::{
    ICON_COLOR, SEPARATOR_COLOR, PANEL_COLOR, BUTTON_HOVERED, ICON_HOVERED, ICON_SELECTED,
    PanelItem, DEFAULT_ITEM_BORDER_STROKE
};
use crate::types::panel::UiPanel;
use crate::types::settings_panel::{
    SettingsPanel, SettingsWidget, StepperArrow, ToggleVisual, SETTINGS_CHECKBOX_LABEL_GAP, SETTINGS_CHECKBOX_BOX_SIZE, 
    SETTINGS_LABEL_FONT_SIZE, SETTINGS_PADDING, STEPPER_ARROW_GAP, STEPPER_ARROW_HEIGHT,
    STEPPER_ARROW_STROKE, STEPPER_ARROW_WIDTH, STEPPER_ARROW_ZONE
};
use std::collections::HashMap;
use usvg::Tree;
use cosmic_text::{FontSystem, SwashCache};
use tiny_skia::{
    BlendMode, Color, FilterQuality, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke,
    Transform,
};

pub fn draw_settings_panel(
    canvas: &mut Pixmap,
    panel: &mut SettingsPanel,
    icons_cache: &HashMap<&'static str, Tree>,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) {
    if !panel.visible {
        return;
    }

    let Some(panel_rect) = panel.rect() else { return };

    let x = panel_rect.left();
    let y = panel_rect.top();
    let (w, h) = panel.size;
    let pw = w.ceil() as u32;
    let ph = h.ceil() as u32;

    if pw == 0 || ph == 0 {
        return;
    }

    let needs_resize = panel
        .panel_pixmap
        .as_ref()
        .is_none_or(|p| p.width() != pw || p.height() != ph);

    if needs_resize {
        panel.panel_pixmap = Pixmap::new(pw, ph);
    }

    let Some(mut panel_pixmap) = panel.panel_pixmap.take() else {
        return;
    };

    panel_pixmap.fill(Color::TRANSPARENT);
    draw_settings_content(&mut panel_pixmap, panel, icons_cache, font_system, swash_cache);

    canvas.draw_pixmap(
        x as i32,
        y as i32,
        panel_pixmap.as_ref(),
        &PixmapPaint {
            opacity: panel.opacity,
            blend_mode: BlendMode::SourceOver,
            quality: FilterQuality::Nearest,
        },
        Transform::identity(),
        None,
    );

    draw_panel_border(canvas, x, y, w, h, 8.0, panel.opacity);

    panel.panel_pixmap = Some(panel_pixmap);
}

fn draw_settings_content(
    canvas: &mut Pixmap,
    panel: &SettingsPanel,
    icons_cache: &HashMap<&'static str, Tree>,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) {
    let (w, h) = panel.size;
    let Some(rect) = Rect::from_xywh(0.0, 0.0, w, h) else {
        return;
    };

    if let Some(path) = rounded_rect_path(&rect, 8.0, true, true, true, true) {
        let mut paint = Paint::default();
        paint.set_color(PANEL_COLOR);
        paint.anti_alias = true;
        canvas.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
    }

    let item_h = h * 0.70;
    let item_y = rect.top() + (h - item_h) / 2.0;
    let mut current_x = rect.left() + SETTINGS_PADDING;
    let icon_color = Color::from_rgba8(ICON_COLOR.red, ICON_COLOR.green, ICON_COLOR.blue, 255);

    for (index, item) in panel.widgets.iter().enumerate() {
        let item_w = item.size();
        let is_hovered = panel.hovered == Some(index);
        let is_selected = panel.selected == Some(index);

        match item {
            SettingsWidget::ColorSwatch => {
                draw_color_swatch(canvas, current_x, item_y, item_w, item_h, is_hovered, is_selected);
            }
            SettingsWidget::Stepper { label, unit, .. } => {
                let display_text = stepper_display_text(panel, index, label, unit);
                draw_stepper(
                    canvas,
                    current_x,
                    item_y,
                    item_w,
                    item_h,
                    panel,
                    index,
                    &display_text,
                    is_hovered,
                    is_selected,
                    icon_color,
                    font_system,
                    swash_cache,
                );
            }
            SettingsWidget::Toggle { visual, .. } => {
                let is_on = panel.is_toggled(index);
                draw_toggle(
                    canvas, current_x, item_y, item_w, item_h,
                    visual, is_on, is_hovered, 
                    icons_cache, icon_color, font_system, swash_cache,
                );
            }
            SettingsWidget::Label(text) => {
                if let Some(label_rect) = Rect::from_xywh(current_x, item_y, item_w, item_h) {
                    draw_aligned_text(
                        canvas,
                        text,
                        font_system,
                        swash_cache,
                        label_rect,
                        SETTINGS_LABEL_FONT_SIZE,
                        icon_color,
                        HAlign::Center,
                        (0.0, 0.0),
                        cosmic_text::Weight::NORMAL,
                    );
                }
            }
            SettingsWidget::Separator => {
                let sep_w = 2.0;
                let sep_h = h * 0.5;
                let sep_x = current_x + (item_w - sep_w) / 2.0;
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

        current_x += item_w + item.trailing_padding();
    }
}

fn draw_color_swatch(canvas: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, is_hovered: bool, is_selected: bool) {
    draw_item_border(canvas, x, y, w, h, 4.0, DEFAULT_ITEM_BORDER_STROKE, is_hovered, is_selected);

    let circle_r = (h.min(w) * 0.35).max(4.0);
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;

    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, circle_r);
    if let Some(circle_path) = pb.finish() {
        let mut swatch_paint = Paint::default();
        let skia_icon_color = Color::from_rgba8(ICON_COLOR.red, ICON_COLOR.green, ICON_COLOR.blue, 255);
        swatch_paint.set_color(skia_icon_color);
        swatch_paint.anti_alias = true;
        canvas.fill_path(&circle_path, &swatch_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
    }
}

fn draw_stepper(
    canvas: &mut Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    panel: &SettingsPanel,
    index: usize,
    text: &str,
    is_hovered: bool,
    is_selected: bool,
    icon_color: Color,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) {
    draw_item_border(canvas, x, y, w, h, 4.0, DEFAULT_ITEM_BORDER_STROKE, is_hovered, is_selected);

    let Some(label_rect) = Rect::from_xywh(
        x + SETTINGS_PADDING,
        y,
        (w - STEPPER_ARROW_ZONE - SETTINGS_PADDING).max(0.0),
        h,
    ) else {
        let hovered_arrow = panel.hovered_arrow
            .filter(|(idx, _)| *idx == index)
            .map(|(_, arrow)| arrow);
        draw_stepper_arrows(canvas, x, y, w, h, icon_color, hovered_arrow);
        return;
    };

    let editing = panel.fields.editing.as_ref()
        .filter(|e| e.key == index)
        .map(|e| &e.field);

    draw_line_edit(
        canvas, label_rect, text, editing, font_system, swash_cache,
        SETTINGS_LABEL_FONT_SIZE, icon_color, cosmic_text::Weight::BOLD,
    );

    let hovered_arrow = panel.hovered_arrow
        .filter(|(idx, _)| *idx == index)
        .map(|(_, arrow)| arrow);
    draw_stepper_arrows(canvas, x, y, w, h, icon_color, hovered_arrow);
}

fn draw_stepper_arrows(
    canvas: &mut Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    icon_color: Color,
    hovered: Option<StepperArrow>,
) {
    let cx = x + w - STEPPER_ARROW_ZONE / 2.0;
    let mid_y = y + h / 2.0;

    let up_cy = mid_y - STEPPER_ARROW_GAP / 2.0 - STEPPER_ARROW_HEIGHT / 2.0;
    let down_cy = mid_y + STEPPER_ARROW_GAP / 2.0 + STEPPER_ARROW_HEIGHT / 2.0;

    let up_color = if hovered == Some(StepperArrow::Up) { BUTTON_HOVERED } else { icon_color };
    let down_color = if hovered == Some(StepperArrow::Down) { BUTTON_HOVERED } else { icon_color };

    let mut paint = Paint::default();
    paint.anti_alias = true;

    let mut stroke = Stroke::default();
    stroke.width = STEPPER_ARROW_STROKE;
    stroke.line_cap = tiny_skia::LineCap::Round;
    stroke.line_join = tiny_skia::LineJoin::Round;

    paint.set_color(up_color);
    let mut pb = PathBuilder::new();
    pb.move_to(cx - STEPPER_ARROW_WIDTH / 2.0, up_cy + STEPPER_ARROW_HEIGHT / 2.0);
    pb.line_to(cx, up_cy - STEPPER_ARROW_HEIGHT / 2.0);
    pb.line_to(cx + STEPPER_ARROW_WIDTH / 2.0, up_cy + STEPPER_ARROW_HEIGHT / 2.0);
    if let Some(path) = pb.finish() {
        canvas.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }

    paint.set_color(down_color);
    let mut pb = PathBuilder::new();
    pb.move_to(cx - STEPPER_ARROW_WIDTH / 2.0, down_cy - STEPPER_ARROW_HEIGHT / 2.0);
    pb.line_to(cx, down_cy + STEPPER_ARROW_HEIGHT / 2.0);
    pb.line_to(cx + STEPPER_ARROW_WIDTH / 2.0, down_cy - STEPPER_ARROW_HEIGHT / 2.0);
    if let Some(path) = pb.finish() {
        canvas.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
    }
}

fn stepper_display_text(panel: &SettingsPanel, index: usize, label: &str, unit: &str) -> String {
    if let Some(edit) = panel.fields.editing.as_ref().filter(|e| e.key == index) {
        return edit.field.text.clone();
    }

    let value = panel.fields.value(index).map(String::as_str).unwrap_or("");

    if label.is_empty() {
        format!("{value}{unit}")
    } else {
        format!("{label}: {value}{unit}")
    }
}

pub fn char_index_for_x(
    text: &str,
    target_x: f32,
    font_size: f32,
    font_system: &mut FontSystem,
) -> usize {
    if text.is_empty() || target_x <= 0.0 {
        return 0;
    }

    let metrics = cosmic_text::Metrics::new(font_size, font_size * 1.2);
    let mut buffer = cosmic_text::Buffer::new_empty(metrics);
    buffer.set_size(None, None);
    buffer.set_text(
        text,
        &cosmic_text::Attrs::new().family(cosmic_text::Family::SansSerif),
        cosmic_text::Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(font_system, false);

    for run in buffer.layout_runs() {
        let mut last_byte = 0usize;
        for glyph in run.glyphs.iter() {
            let glyph_mid = glyph.x + glyph.w / 2.0;
            if target_x < glyph_mid {
                return byte_to_char_index(text, glyph.start);
            }
            last_byte = glyph.end;
        }
        return byte_to_char_index(text, last_byte);
    }
    text.chars().count()
}

fn byte_to_char_index(text: &str, byte_idx: usize) -> usize {
    text[..byte_idx.min(text.len())].chars().count()
}

fn draw_toggle(
    canvas: &mut Pixmap,
    x: f32, y: f32, w: f32, h: f32,
    visual: &ToggleVisual,
    is_on: bool,
    is_hovered: bool,
    icons_cache: &HashMap<&'static str, Tree>,
    icon_color: Color,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) {
    match visual {
        ToggleVisual::Icon { svg, icon_size } => {
        draw_item_border(canvas, x, y, w, h, 4.0, DEFAULT_ITEM_BORDER_STROKE, is_hovered, is_on);
            let tint = if is_on {
                ICON_SELECTED
            } else if is_hovered {
                ICON_HOVERED
            }else {
                usvg::Color { red: ICON_COLOR.red, green: ICON_COLOR.green, blue: ICON_COLOR.blue }
            };

            let icon_x = x + (w - icon_size) / 2.0;
            let icon_y = y + (h - icon_size) / 2.0;
            draw_svg_icon(canvas, icons_cache, svg, *icon_size, icon_x, icon_y, tint);
        }
        ToggleVisual::Checkbox { label } => {
            let box_size = SETTINGS_CHECKBOX_BOX_SIZE;
            let box_x = x + w - box_size;
            let box_y = y + (h - box_size) / 2.0;

            if let Some(label_rect) =
                Rect::from_xywh(x, y, (w - box_size - SETTINGS_CHECKBOX_LABEL_GAP).max(0.0), h)
            {
                draw_aligned_text(
                    canvas, label, font_system, swash_cache, label_rect,
                    SETTINGS_LABEL_FONT_SIZE, icon_color, HAlign::Left, (0.0, 0.0),
                    cosmic_text::Weight::NORMAL,
                );
            }
            
            draw_item_border(canvas, box_x, box_y, box_size, box_size, 3.0, DEFAULT_ITEM_BORDER_STROKE, is_hovered, is_on);
            
            if is_on {
                draw_checkmark(canvas, box_x, box_y, box_size, icon_color);
            }
        }
    }
}

fn draw_checkmark(canvas: &mut Pixmap, x: f32, y: f32, size: f32, color: Color) {
    let mut pb = PathBuilder::new();
    pb.move_to(x + size * 0.22, y + size * 0.55);
    pb.line_to(x + size * 0.42, y + size * 0.75);
    pb.line_to(x + size * 0.80, y + size * 0.28);
    let Some(path) = pb.finish() else { return };

    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;

    let stroke = Stroke { width: 2.0, line_cap: tiny_skia::LineCap::Round, line_join: tiny_skia::LineJoin::Round, ..Default::default() };
    canvas.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
}