pub mod selection;
pub mod arrow;
pub mod circle;
pub mod text;
pub mod rectangle;

use strum::EnumIter;
use crate::types::EditorState;
use crate::types::MouseButton;

use crate::tools::selection::SelectionTool;
use crate::tools::rectangle::RectangleTool;
use crate::tools::arrow::ArrowTool;
use crate::tools::text::TextTool;
use crate::tools::circle::CircleTool;
// ==========================================
// 1. Available Tools, Initialization
// ==========================================
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter)]
pub enum Tool {
    Selection, 
    Rectangle, 
    Arrow, 
    Circle,
    Text,
}

// ==========================================
// 2. Tools behaviour implementation
// ==========================================

pub trait ToolBehavior {
    fn on_button(&self, state: &mut EditorState, button: MouseButton, pressed: bool, dirty_mask: &mut u32);
    fn on_move(&self, state: &mut EditorState, global: (f64, f64), 
               selection_dirty: &mut bool, dirty_mask: &mut u32);
}
// tools/mod.rs
pub fn get_tool(tool: Tool) -> Box<dyn ToolBehavior> {
    match tool {
        Tool::Selection => Box::new(SelectionTool),
        Tool::Rectangle => Box::new(RectangleTool),
        Tool::Arrow     => Box::new(ArrowTool),
        Tool::Text      => Box::new(TextTool),
        Tool::Circle    => Box::new(CircleTool),

    }
}


pub fn dispatch_move(tool: Tool, state: &mut EditorState, global: (f64, f64),
                     selection_dirty: &mut bool, dirty_mask: &mut u32) {
    match tool {
        Tool::Selection => SelectionTool.on_move(state, global, selection_dirty, dirty_mask),
        Tool::Arrow => ArrowTool.on_move(state, global, selection_dirty, dirty_mask),
        Tool::Rectangle => RectangleTool.on_move(state, global, selection_dirty, dirty_mask),
        _ => {}
    }
}

pub fn dispatch_button(tool: Tool, state: &mut EditorState, button: MouseButton, pressed: bool, dirty_mask: &mut u32) {
    match tool {
        Tool::Selection => SelectionTool.on_button(state, button, pressed, dirty_mask),
        Tool::Arrow => ArrowTool.on_button(state, button, pressed, dirty_mask),
        Tool::Rectangle => RectangleTool.on_button(state, button, pressed, dirty_mask),
        _ => {}
    }
}