pub mod selection;
pub mod simple_shapes;
pub mod pen;
pub mod text;

use crate::tools::pen::PenTool;
use crate::tools::selection::SelectionTool;
use crate::tools::simple_shapes::SimpleShapeTool; 
use crate::tools::text::TextTool;
use crate::types::annotations::AnnotationShape;
use crate::types::{MouseButton};
use crate::editor::EditorState;
// ==========================================
// 1. Available Tools
// ==========================================
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
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
// 2. trait
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

// ==========================================
// 3. Static dispatching
// ==========================================
pub fn dispatch_move(
    tool: Tool,
    state: &mut EditorState,
    global: (f64, f64),
    selection_dirty: &mut bool,
    dirty_mask: &mut u32,
) {
    match tool {
        Tool::Selection => SelectionTool.on_move(state, global, selection_dirty, dirty_mask),
        Tool::Text => TextTool.on_move(state, global, selection_dirty, dirty_mask),
        Tool::Pen => PenTool.on_move(state, global, selection_dirty, dirty_mask),
        
        Tool::Rectangle | Tool::Arrow | Tool::Circle | Tool::Line => {
            let tool_impl = SimpleShapeTool {
                make_shape: match tool {
                    Tool::Rectangle => |start, end| AnnotationShape::Rectangle { start, end },
                    Tool::Arrow => |start, end| AnnotationShape::Arrow { start, end },
                    Tool::Circle => |start, end| AnnotationShape::Circle { start, end },
                    _ => |start, end| AnnotationShape::Line { start, end },
                },
                color: tiny_skia::Color::from_rgba8(255, 0, 0, 255), 
                stroke_width: 2.0,
            };
            
            tool_impl.on_move(state, global, selection_dirty, dirty_mask);
        }
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
        Tool::Text => TextTool.on_button(state, button, pressed, dirty_mask),
        Tool::Pen => PenTool.on_button(state, button, pressed, dirty_mask),
        
        Tool::Rectangle | Tool::Arrow | Tool::Circle | Tool::Line => {
            let tool_impl = SimpleShapeTool {
                make_shape: match tool {
                    Tool::Rectangle => |start, end| AnnotationShape::Rectangle { start, end },
                    Tool::Arrow => |start, end| AnnotationShape::Arrow { start, end },
                    Tool::Circle => |start, end| AnnotationShape::Circle { start, end },
                    _ => |start, end| AnnotationShape::Line { start, end },
                },
                color: tiny_skia::Color::from_rgba8(255, 0, 0, 255),
                stroke_width: 2.0,
            };
            
            tool_impl.on_button(state, button, pressed, dirty_mask);
        }
    }
}