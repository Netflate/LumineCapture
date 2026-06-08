use super::toolbar::Tool;

pub const SELECTION: &str = include_str!("../../assets/icons/selection.svg");
pub const ARROW:     &str = include_str!("../../assets/icons/arrow.svg");
pub const RECTANGLE: &str = include_str!("../../assets/icons/rectangle.svg");
pub const CIRCLE:    &str = include_str!("../../assets/icons/circle.svg");
pub const TEXT:      &str = include_str!("../../assets/icons/text.svg");

pub fn get_svg(tool: &Tool) -> &'static str {
    match tool {
        Tool::Selection => SELECTION,
        Tool::Arrow     => ARROW,
        Tool::Rectangle => RECTANGLE,
        Tool::Circle    => CIRCLE,
        Tool::Text      => TEXT,
    }
}