use crate::tools::ToolBehavior;
use crate::types::{MouseButton};
use crate::editor::EditorState;


pub struct TextTool;

impl ToolBehavior for TextTool {
    fn on_button(&self, _state: &mut EditorState, _button: MouseButton, _pressed: bool, _dirty_mask: &mut u32) {
        return;
    }
    fn on_move(&self, _state: &mut EditorState, _global: (f64, f64), _selection_dirty: &mut bool, _dirty_mask: &mut u32) {
        return;
    }
}