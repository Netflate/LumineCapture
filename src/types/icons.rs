use crate::types::toolbar::BUTTON_CELL_SIZE;
use crate::types::toolbar::ToolbarButton;


pub const SELECTION:   &str = include_str!("../../assets/icons/selection.svg");
pub const ARROW:       &str = include_str!("../../assets/icons/arrow.svg");
pub const RECTANGLE:   &str = include_str!("../../assets/icons/rectangle.svg");
pub const CIRCLE:      &str = include_str!("../../assets/icons/circle.svg");
pub const TEXT:        &str = include_str!("../../assets/icons/text.svg");
pub const PEN:         &str = include_str!("../../assets/icons/pen.svg");
pub const LINE:        &str = include_str!("../../assets/icons/line.svg");
pub const SIDE_CHANGE: &str = include_str!("../../assets/icons/side_change.svg");

// svg icon sizes
const DEFAULT_ICON_SIZE: f32 = BUTTON_CELL_SIZE - 8.0;


pub fn get_svg(button: &ToolbarButton) -> (&'static str, f32) {
    match button {
        ToolbarButton::Tool(tool) => match tool {
            crate::tools::Tool::Selection => (SELECTION, BUTTON_CELL_SIZE - 13.0),
            crate::tools::Tool::Arrow     => (ARROW, DEFAULT_ICON_SIZE),
            crate::tools::Tool::Rectangle => (RECTANGLE, DEFAULT_ICON_SIZE),
            crate::tools::Tool::Circle    => (CIRCLE, DEFAULT_ICON_SIZE),
            crate::tools::Tool::Pen       => (PEN, DEFAULT_ICON_SIZE - 4.0),
            crate::tools::Tool::Line      => (LINE, DEFAULT_ICON_SIZE),
            crate::tools::Tool::Text      => (TEXT, BUTTON_CELL_SIZE - 13.0),
        },
        ToolbarButton::Action(action) => match action {
            crate::types::toolbar::ToolbarAction::SideChange => (SIDE_CHANGE, DEFAULT_ICON_SIZE - 3.0),
        }
    }
}