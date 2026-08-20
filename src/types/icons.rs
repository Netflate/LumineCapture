use crate::types::toolbar::BUTTON_CELL_SIZE;
use crate::types::toolbar::ToolbarButton;

pub const SELECTION: &str = include_str!("../../assets/icons/selection.svg");
pub const ARROW: &str = include_str!("../../assets/icons/arrow.svg");
pub const RECTANGLE: &str = include_str!("../../assets/icons/rectangle.svg");
pub const CIRCLE: &str = include_str!("../../assets/icons/circle.svg");
pub const TEXT: &str = include_str!("../../assets/icons/text.svg");
pub const PEN: &str = include_str!("../../assets/icons/pen.svg");
pub const LINE: &str = include_str!("../../assets/icons/line.svg");
pub const PICK: &str = include_str!("../../assets/icons/cursor.svg");
pub const NUMERATED_ARROW: &str = include_str!("../../assets/icons/numerated_arrow.svg");

// svg icon sizes
const DEFAULT_ICON_SIZE: f32 = BUTTON_CELL_SIZE - 4.0;

pub fn get_svg(button: &ToolbarButton) -> (&'static str, f32) {
    match button {
        ToolbarButton::Tool(tool) => match tool {
            crate::tools::Tool::Selection => (SELECTION, DEFAULT_ICON_SIZE - 10.0),
            crate::tools::Tool::Pick => (PICK, DEFAULT_ICON_SIZE - 5.0),
            crate::tools::Tool::Text => (TEXT, DEFAULT_ICON_SIZE - 9.0),
            crate::tools::Tool::Pen => (PEN, DEFAULT_ICON_SIZE - 11.0),
            crate::tools::Tool::Line => (LINE, DEFAULT_ICON_SIZE - 3.0),
            crate::tools::Tool::Arrow => (ARROW, DEFAULT_ICON_SIZE - 7.0),
            crate::tools::Tool::Rectangle => (RECTANGLE, DEFAULT_ICON_SIZE),
            crate::tools::Tool::Circle => (CIRCLE, DEFAULT_ICON_SIZE - 6.0),
            crate::tools::Tool::NumeratedArrow => (NUMERATED_ARROW, DEFAULT_ICON_SIZE - 6.0),
        },
        // ToolbarButton::Action(action) => match action {
        //     crate::types::toolbar::ToolbarAction::SideChange => {
        //         (SIDE_CHANGE, DEFAULT_ICON_SIZE - 9.0)
        //     }
        // },
    }
}
