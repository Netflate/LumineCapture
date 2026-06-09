use super::toolbar::{Tool, BUTTON_CELL_SIZE};

pub const SELECTION: &str = include_str!("../../assets/icons/selection.svg");
pub const ARROW:     &str = include_str!("../../assets/icons/arrow.svg");
pub const RECTANGLE: &str = include_str!("../../assets/icons/rectangle.svg");
pub const CIRCLE:    &str = include_str!("../../assets/icons/circle.svg");
pub const TEXT:      &str = include_str!("../../assets/icons/text.svg");

// svg icon sizes
const DEFAULT_ICON_SIZE: f32 = BUTTON_CELL_SIZE - 8.0;


pub fn get_svg(tool: &Tool) -> (&'static str, f32) {
    match tool {
        Tool::Selection => (SELECTION, BUTTON_CELL_SIZE - 13.0),
        Tool::Arrow     => (ARROW, DEFAULT_ICON_SIZE),
        Tool::Rectangle => (RECTANGLE, DEFAULT_ICON_SIZE),
        Tool::Circle    => (CIRCLE, DEFAULT_ICON_SIZE),
        Tool::Text      => (TEXT, BUTTON_CELL_SIZE - 13.0),
    }
}