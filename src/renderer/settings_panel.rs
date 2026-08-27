use std::collections::HashMap;
use usvg::Tree;
use cosmic_text::{FontSystem, SwashCache};
use tiny_skia::{
    BlendMode, Color, FilterQuality, Paint, PathBuilder, Pixmap, PixmapPaint, Rect, Stroke,
    Transform,
};

use super::paths::{rounded_rect_path, draw_panel_border, panel_border_color};
use super::text::{draw_aligned_text, HAlign};
use crate::types::toolbar::{
    ToolbarButton, BUTTON_HOVERED, BUTTON_SELECTED, ICON_COLOR, SEPARATOR_COLOR, TOOLBAR_COLOR,
};

use crate::types::settings_panel::{
    SettingsPanel, SettingsWidget, StepperArrow, SETTINGS_BUTTON_GAP, SETTINGS_BUTTON_WIDTH,
    SETTINGS_LABEL_FONT_SIZE, SETTINGS_PADDING, STEPPER_ARROW_GAP, STEPPER_ARROW_HEIGHT,
    STEPPER_ARROW_STROKE, STEPPER_ARROW_WIDTH, STEPPER_ARROW_ZONE,
};
use crate::types::panel::PanelItem;

pub fn draw_settings_panel(
    canvas: &mut Pixmap,
    panel: &mut SettingsPanel,
    icons_cache: &HashMap<ToolbarButton, Tree>,
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
        panel.dirty = true;
    }

    let Some(mut panel_pixmap) = panel.panel_pixmap.take() else {
        return;
    };

    if panel.dirty {
        panel_pixmap.fill(Color::TRANSPARENT);
        draw_settings_content(&mut panel_pixmap, panel, icons_cache, font_system, swash_cache);
        panel.dirty = false;
    }

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
    _icons_cache: &HashMap<ToolbarButton, Tree>,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) {
    let (w, h) = panel.size;
    let Some(rect) = Rect::from_xywh(0.0, 0.0, w, h) else {
        return;
    };

    if let Some(path) = rounded_rect_path(&rect, 8.0, true, true, true, true) {
        let mut paint = Paint::default();
        paint.set_color(TOOLBAR_COLOR);
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
            SettingsWidget::ButtonGroup { options } => {
                draw_button_group(canvas, current_x, item_y, item_h, options, is_hovered, is_selected);
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

fn draw_item_border(
    canvas: &mut Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    is_hovered: bool,
    is_selected: bool,
) {
    if let Some(inset_rect) = Rect::from_xywh(x + 0.5, y + 0.5, (w - 1.0).max(0.1), (h - 1.0).max(0.1)) {
        if let Some(path) = rounded_rect_path(&inset_rect, radius, true, true, true, true) {
            let mut paint = Paint::default();
            paint.set_color(if is_selected {
                BUTTON_SELECTED
            } else if is_hovered {
                BUTTON_HOVERED
            } else {
                panel_border_color(TOOLBAR_COLOR) 
            });
            paint.anti_alias = true;
            
            let stroke = Stroke {
                width: 1.0,
                ..Default::default()
            };
            canvas.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }
}

fn draw_color_swatch(canvas: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, is_hovered: bool, is_selected: bool) {
    draw_item_border(canvas, x, y, w, h, 4.0, is_hovered, is_selected);

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
    draw_item_border(canvas, x, y, w, h, 4.0, is_hovered, is_selected);

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

    let is_editing = panel.editing.as_ref().is_some_and(|e| e.widget_idx == index);

    if is_editing {
        let edit = panel.editing.as_ref().unwrap();
        let raw_text = &edit.field.text;

        if let Some((start_byte, end_byte)) = edit.field.selection_byte_range() {
            let sx = measure_prefix_width(raw_text, start_byte, SETTINGS_LABEL_FONT_SIZE, font_system);
            let ex = measure_prefix_width(raw_text, end_byte, SETTINGS_LABEL_FONT_SIZE, font_system);
            let sel_h = label_rect.height() * 0.75;
            let sel_y = label_rect.top() + (label_rect.height() - sel_h) / 2.0;
            if let Some(sel_rect) = Rect::from_xywh(
                label_rect.left() + sx,
                sel_y,
                (ex - sx).max(1.0),
                sel_h,
            ) {
                let mut sel_paint = Paint::default();
                sel_paint.set_color(Color::from_rgba8(100, 150, 255, 110));
                sel_paint.anti_alias = true;
                canvas.fill_rect(sel_rect, &sel_paint, Transform::identity(), None);
            }
        }

        let cx = measure_prefix_width(raw_text, edit.field.cursor_byte(), SETTINGS_LABEL_FONT_SIZE, font_system);
        let cur_h = label_rect.height() * 0.75;
        let cur_y = label_rect.top() + (label_rect.height() - cur_h) / 2.0;
        if let Some(cur_rect) = Rect::from_xywh(
            (label_rect.left() + cx).round(),
            cur_y,
            1.5,
            cur_h,
        ) {
            let mut cur_paint = Paint::default();
            cur_paint.set_color(Color::from_rgba8(255, 255, 255, 220));
            cur_paint.anti_alias = false;
            canvas.fill_rect(cur_rect, &cur_paint, Transform::identity(), None);
        }
    }

    if !text.is_empty() {
        draw_aligned_text(
            canvas,
            text,
            font_system,
            swash_cache,
            label_rect,
            SETTINGS_LABEL_FONT_SIZE,
            icon_color,
            HAlign::Left,
            (0.0, 0.0),
            cosmic_text::Weight::BOLD,
        );
    }

    let hovered_arrow = panel.hovered_arrow
        .filter(|(idx, _)| *idx == index)
        .map(|(_, arrow)| arrow);
    draw_stepper_arrows(canvas, x, y, w, h, icon_color, hovered_arrow);
}

fn measure_prefix_width(
    text: &str,
    upto_byte: usize,
    font_size: f32,
    font_system: &mut FontSystem,
) -> f32 {
    if upto_byte == 0 || text.is_empty() {
        return 0.0;
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
        for glyph in run.glyphs.iter() {
            if glyph.start >= upto_byte {
                return glyph.x;
            }
        }
        return run.line_w;
    }
    0.0
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

fn draw_button_group(canvas: &mut Pixmap, x: f32, y: f32, h: f32, options: &[(&str, u8)], is_hovered: bool, is_selected: bool) {
    let mut curr_x = x;
    for (idx, _) in options.iter().enumerate() {
        let is_active = is_selected || (idx == 0);
        
        draw_item_border(canvas, curr_x, y, SETTINGS_BUTTON_WIDTH, h, 4.0, is_hovered, is_active);
        
        curr_x += SETTINGS_BUTTON_WIDTH + SETTINGS_BUTTON_GAP;
    }
}

fn stepper_display_text(panel: &SettingsPanel, index: usize, label: &str, unit: &str) -> String {
    if let Some(edit) = panel.editing.as_ref().filter(|e| e.widget_idx == index) {
        return edit.field.text.clone();
    }

    let value = panel.values.get(&index).map(String::as_str).unwrap_or("");

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