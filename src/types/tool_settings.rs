use tiny_skia::Color;

use crate::theme::Rgba;

pub fn default_color() -> Rgba {
    crate::editor::saved::get()
        .color
        .unwrap_or(crate::config::get().tools.default_color)
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolSettings {
    pub stroke_width: f32,
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    pub fill: bool,
    pub color: Color,
}

impl Default for ToolSettings {
    fn default() -> Self {
        let tools = &crate::config::get().tools;
        let saved = crate::editor::saved::get();
        let (stroke_min, stroke_max) = crate::ui::settings_panel::STROKE_RANGE;
        let (font_min, font_max) = crate::ui::settings_panel::FONT_RANGE;
        Self {
            stroke_width: saved
                .stroke_width
                .unwrap_or(tools.default_stroke_width)
                .clamp(stroke_min, stroke_max),
            font_size: saved
                .font_size
                .unwrap_or(tools.default_font_size)
                .clamp(font_min, font_max),
            bold: saved.bold.unwrap_or(tools.default_bold),
            italic: saved.italic.unwrap_or(tools.default_italic),
            fill: saved.fill.unwrap_or(tools.default_fill),
            color: default_color().color(),
        }
    }
}
