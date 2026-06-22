pub mod selection;
pub mod simple_shapes;
pub mod pen;
pub mod text;

use strum::EnumIter;

use crate::tools::pen::PenTool;
use crate::tools::selection::SelectionTool;
use crate::tools::simple_shapes::SimpleShapeTool;
use crate::tools::text::TextTool;
use crate::types::annotations::AnnotationShape;
use crate::types::EditorState;
use crate::types::MouseButton;

use tiny_skia::Color;

// ==========================================
// 1. Available Tools, Initialization
// ==========================================
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter)]
pub enum Tool {
    Selection,
    Rectangle,
    Arrow,
    Circle,
    Pen,
    Line,
    Text,
}

// ==========================================
// 2. Tools behaviour implementation
// ==========================================
pub trait ToolBehavior {
    fn on_button(&self, state: &mut EditorState, button: MouseButton, pressed: bool, dirty_mask: &mut u32);
    fn on_move(
        &self,
        state: &mut EditorState,
        global: (f64, f64),
        selection_dirty: &mut bool,
        dirty_mask: &mut u32,
    );
}

pub fn get_tool(tool: Tool) -> Box<dyn ToolBehavior> {
    match tool {
        Tool::Selection => Box::new(SelectionTool),
        Tool::Text => Box::new(TextTool),
        Tool::Pen => Box::new(PenTool),

        
        Tool::Rectangle => Box::new(SimpleShapeTool {
            make_shape: |start, end| AnnotationShape::Rectangle { start, end },
            color: Color::from_rgba8(255, 0, 0, 255),
            stroke_width: 2.0,
        }),
        Tool::Arrow => Box::new(SimpleShapeTool {
            make_shape: |start, end| AnnotationShape::Arrow { start, end },
            color: Color::from_rgba8(255, 0, 0, 255),
            stroke_width: 2.0,
        }),
        Tool::Circle => Box::new(SimpleShapeTool {
            make_shape: |start, end| AnnotationShape::Circle { start, end },
            color: Color::from_rgba8(255, 0, 0, 255),
            stroke_width: 2.0,
        }),
        Tool::Line => Box::new(SimpleShapeTool {
            make_shape: |start, end| AnnotationShape::Line { start, end },
            color: Color::from_rgba8(255, 0, 0, 255),
            stroke_width: 2.0,
        }),
    }
}

pub fn dispatch_move(
    tool: Tool,
    state: &mut EditorState,
    global: (f64, f64),
    selection_dirty: &mut bool,
    dirty_mask: &mut u32,
) {
    match tool {
        Tool::Selection => SelectionTool.on_move(state, global, selection_dirty, dirty_mask),
        _ => get_tool(tool).on_move(state, global, selection_dirty, dirty_mask),
    }
}

pub fn dispatch_button(
    tool: Tool,
    state: &mut EditorState,
    button: MouseButton,
    pressed: bool,
    dirty_mask: &mut u32,
) {
    match tool {
        Tool::Selection => SelectionTool.on_button(state, button, pressed, dirty_mask),
        _ => get_tool(tool).on_button(state, button, pressed, dirty_mask),
    }
}