use tiny_skia::Color;

/// The only source of truth 
/// for annotation settings, used by the settings panel and tools
/// 
/// TODO : should use config 
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
            stroke_width: 12.0, 
            font_size: 24.0,   
            bold: true,
            italic: false,
            color: Color::from_rgba8(255, 255, 255, 255),
        }
    }
}