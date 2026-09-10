use tiny_skia::Color;

use crate::theme::Rgba;

pub fn default_color() -> Rgba {
    crate::config::get().tools.default_color
}

#[derive(Debug, Clone)]
pub struct ToolSettings {
    pub stroke_width: f32,
    pub font_size: f32,
    pub bold: bool,
    pub italic: bool,
    pub color: Color,
}

impl Default for ToolSettings {
    fn default() -> Self {
        let tools = &crate::config::get().tools;
        Self {
            stroke_width: tools.default_stroke_width,
            font_size: tools.default_font_size,
            bold: tools.default_bold,
            italic: tools.default_italic,
            color: tools.default_color.color(),
        }
    }
}
