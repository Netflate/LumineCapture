pub mod numerated_arrow;
pub mod pen;
pub mod pick;
pub mod selection;
pub mod simple_shapes;
pub mod text;

use crate::editor::EditorState;
use crate::tools::numerated_arrow::NumeratedArrowTool;
use crate::tools::pen::PenTool;
use crate::tools::pick::PickTool;
use crate::tools::selection::SelectionTool;
use crate::tools::simple_shapes::SimpleShapeTool;
use crate::tools::text::TextTool;
use crate::types::annotations::AnnotationShape;
use crate::types::{MouseButton, SpecialKey};

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
    Pick,
    NumeratedArrow,
}

// ==========================================
// 2. trait
// ==========================================
pub trait ToolBehavior {
    fn on_button(
        &self,
        state: &mut EditorState,
        button: MouseButton,
        pressed: bool,
        dirty_mask: &mut u32,
    );
    fn on_move(
        &self,
        state: &mut EditorState,
        global: (f64, f64),
        dirty_mask: &mut u32,
    );
    fn on_deactivate(&self, _state: &mut EditorState, _dirty_mask: &mut u32) {}
    fn on_text(&self, _state: &mut EditorState, _ch: char, _dirty_mask: &mut u32) {}
    fn on_key(&self, _state: &mut EditorState, _key: SpecialKey, _dirty_mask: &mut u32) {}
}

// ==========================================
// 3. Static dispatching
// ==========================================
pub fn dispatch_move(
    tool: Tool,
    state: &mut EditorState,
    global: (f64, f64),
    dirty_mask: &mut u32,
) {
    match tool {
        Tool::Selection => SelectionTool.on_move(state, global,  dirty_mask),
        Tool::Pick => PickTool.on_move(state, global,  dirty_mask),
        Tool::Text => TextTool.on_move(state, global,  dirty_mask),
        Tool::Pen => PenTool.on_move(state, global,  dirty_mask),
        Tool::NumeratedArrow => {
            NumeratedArrowTool.on_move(state, global,  dirty_mask)
        }

        Tool::Rectangle | Tool::Arrow | Tool::Circle | Tool::Line => {
            let tool_impl = SimpleShapeTool {
                make_shape: match tool {
                    Tool::Rectangle => |start, end| AnnotationShape::Rectangle { start, end },
                    Tool::Arrow => |start, end| AnnotationShape::Arrow { start, end },
                    Tool::Circle => |start, end| AnnotationShape::Circle { start, end },
                    _ => |start, end| AnnotationShape::Line { start, end },
                },
                color: tiny_skia::Color::from_rgba8(255, 255, 255, 255),
                stroke_width: 8.0,
            };

            tool_impl.on_move(state, global,  dirty_mask);
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
        Tool::Pick => PickTool.on_button(state, button, pressed, dirty_mask),
        Tool::Text => TextTool.on_button(state, button, pressed, dirty_mask),
        Tool::Pen => PenTool.on_button(state, button, pressed, dirty_mask),
        Tool::NumeratedArrow => NumeratedArrowTool.on_button(state, button, pressed, dirty_mask),

        Tool::Rectangle | Tool::Arrow | Tool::Circle | Tool::Line => {
            let tool_impl = SimpleShapeTool {
                make_shape: match tool {
                    Tool::Rectangle => |start, end| AnnotationShape::Rectangle { start, end },
                    Tool::Arrow => |start, end| AnnotationShape::Arrow { start, end },
                    Tool::Circle => |start, end| AnnotationShape::Circle { start, end },
                    _ => |start, end| AnnotationShape::Line { start, end },
                },
                color: tiny_skia::Color::from_rgba8(255, 255, 255, 255),
                stroke_width: 8.0,
            };

            tool_impl.on_button(state, button, pressed, dirty_mask);
        }
    }
}

// for now it only cancels active selection
pub fn dispatch_deactivate(tool: Tool, state: &mut EditorState, dirty_mask: &mut u32) {
    match tool {
        Tool::Pick => PickTool.on_deactivate(state, dirty_mask),
        Tool::Text => TextTool.on_deactivate(state, dirty_mask),
        _ => {}
    }
}

pub fn dispatch_text(tool: Tool, state: &mut EditorState, ch: char, dirty_mask: &mut u32) {
    if tool == Tool::Text {
        TextTool.on_text(state, ch, dirty_mask)
    }
}

pub fn dispatch_key(tool: Tool, state: &mut EditorState, key: SpecialKey, dirty_mask: &mut u32) {
    if tool == Tool::Text {
        TextTool.on_key(state, key, dirty_mask)
    }
}
