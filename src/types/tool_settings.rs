use tiny_skia::Color;

use crate::theme::Rgba;

pub const DEFAULT_COLOR: Rgba = Rgba(255, 255, 255, 255);
const DEFAULT_STROKE_WIDTH: f32 = 12.0;
const DEFAULT_FONT_SIZE: f32 = 24.0;
const DEFAULT_BOLD: bool = true;
const DEFAULT_ITALIC: bool = false;

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
        Self {
            stroke_width: DEFAULT_STROKE_WIDTH,
            font_size: DEFAULT_FONT_SIZE,
            bold: DEFAULT_BOLD,
            italic: DEFAULT_ITALIC,
            color: DEFAULT_COLOR.color(),
        }
    }
}
