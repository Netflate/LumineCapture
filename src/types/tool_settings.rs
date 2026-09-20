use tiny_skia::Color;

use crate::theme::Rgba;

pub fn default_color() -> Rgba {
    crate::editor::saved::get()
        .color
        .unwrap_or(crate::config::get().tools.color)
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
        Self {
            stroke_width: saved
                .stroke_width
                .unwrap_or(tools.stroke.width)
                .clamp(tools.stroke.min, tools.stroke.max),
            font_size: saved
                .font_size
                .unwrap_or(tools.font.size)
                .clamp(tools.font.min, tools.font.max),
            bold: saved.bold.unwrap_or(tools.font.bold),
            italic: saved.italic.unwrap_or(tools.font.italic),
            fill: saved.fill.unwrap_or(tools.fill),
            color: default_color().color(),
        }
    }
}
