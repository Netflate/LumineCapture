use tiny_skia::Color;

use crate::theme::Rgba;
use crate::tools::Tool;
use crate::types::AnnotationShape;

/// redaction strength in percent
pub const STRENGTH: (f32, f32) = (1.0, 100.0);

pub fn default_color() -> Rgba {
    crate::editor::saved::get()
        .color
        .unwrap_or(crate::config::get().tools.color)
}

/// Tools that share one stroke width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StrokeGroup {
    Rectangle,
    Circle,
    Arrow,
    Line,
    Numbered,
    Freehand,
}

impl StrokeGroup {
    pub const ALL: [Self; 6] = [
        Self::Rectangle,
        Self::Circle,
        Self::Arrow,
        Self::Line,
        Self::Numbered,
        Self::Freehand,
    ];

    /// name in the saved state file
    pub fn key(self) -> &'static str {
        match self {
            Self::Rectangle => "rectangle",
            Self::Circle => "circle",
            Self::Arrow => "arrow",
            Self::Line => "line",
            Self::Numbered => "numbered",
            Self::Freehand => "freehand",
        }
    }

    /// tools without a stroke width never show the stepper, so the fallback is never read
    pub fn of_tool(tool: Tool) -> Self {
        match tool {
            Tool::Circle => Self::Circle,
            Tool::Arrow => Self::Arrow,
            Tool::Line => Self::Line,
            Tool::NumeratedArrow => Self::Numbered,
            Tool::Pen | Tool::Highlighter => Self::Freehand,
            _ => Self::Rectangle,
        }
    }

    pub fn of_shape(shape: &AnnotationShape) -> Self {
        match shape {
            AnnotationShape::Circle { .. } => Self::Circle,
            AnnotationShape::Arrow { .. } => Self::Arrow,
            AnnotationShape::Line { .. } => Self::Line,
            AnnotationShape::NumeratedArrow { .. } => Self::Numbered,
            AnnotationShape::Pen { .. } | AnnotationShape::Highlighter { .. } => Self::Freehand,
            _ => Self::Rectangle,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolSettings {
    strokes: [f32; StrokeGroup::ALL.len()],
    pub font_size: f32,
    pub strength: f32,
    pub pixelate: bool,
    pub bold: bool,
    pub italic: bool,
    pub outline: bool,
    pub outline_color: Color,
    pub fill: bool,
    pub color: Color,
}

impl ToolSettings {
    pub fn stroke(&self, group: StrokeGroup) -> f32 {
        self.strokes[group as usize]
    }

    pub fn set_stroke(&mut self, group: StrokeGroup, width: f32) {
        self.strokes[group as usize] = width;
    }
}

impl Default for ToolSettings {
    fn default() -> Self {
        let tools = &crate::config::get().tools;
        let saved = crate::editor::saved::get();
        let strokes = StrokeGroup::ALL.map(|group| {
            saved
                .stroke(group)
                .unwrap_or(tools.stroke.width)
                .clamp(tools.stroke.min, tools.stroke.max)
        });
        Self {
            strokes,
            font_size: saved
                .font_size
                .unwrap_or(tools.font.size)
                .clamp(tools.font.min, tools.font.max),
            strength: saved
                .strength
                .unwrap_or(tools.redact.strength)
                .clamp(STRENGTH.0, STRENGTH.1),
            pixelate: saved.pixelate.unwrap_or(tools.redact.pixelate),
            bold: saved.bold.unwrap_or(tools.font.bold),
            italic: saved.italic.unwrap_or(tools.font.italic),
            outline: saved.outline.unwrap_or(tools.font.outline),
            outline_color: saved
                .outline_color
                .unwrap_or(tools.font.outline_color)
                .color(),
            fill: saved.fill.unwrap_or(tools.fill),
            color: default_color().color(),
        }
    }
}
