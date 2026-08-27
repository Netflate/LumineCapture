use crate::tools::Tool;
use crate::types::toolbar::BUTTON_CELL_SIZE;

pub const SELECTION: &str = include_str!("../../assets/icons/selection.svg");
pub const ARROW: &str = include_str!("../../assets/icons/arrow.svg");
pub const RECTANGLE: &str = include_str!("../../assets/icons/rectangle.svg");
pub const CIRCLE: &str = include_str!("../../assets/icons/circle.svg");
pub const TEXT: &str = include_str!("../../assets/icons/text.svg");
pub const PEN: &str = include_str!("../../assets/icons/pen.svg");
pub const LINE: &str = include_str!("../../assets/icons/line.svg");
pub const PICK: &str = include_str!("../../assets/icons/cursor.svg");
pub const NUMERATED_ARROW: &str = include_str!("../../assets/icons/numerated_arrow.svg");
pub const ITALIC: &str = include_str!("../../assets/icons/italic.svg");
pub const BOLD: &str = include_str!("../../assets/icons/bold.svg");
// svg icon sizes
const DEFAULT_ICON_SIZE: f32 = BUTTON_CELL_SIZE - 4.0;

/// Icons tied to a drawing Tool (used in the toolbar).
pub fn get_svg_for_tool(tool: Tool) -> (&'static str, f32) {
    match tool {
        Tool::Selection => (SELECTION, DEFAULT_ICON_SIZE - 10.0),
        Tool::Pick => (PICK, DEFAULT_ICON_SIZE - 5.0),
        Tool::Text => (TEXT, DEFAULT_ICON_SIZE - 9.0),
        Tool::Pen => (PEN, DEFAULT_ICON_SIZE - 11.0),
        Tool::Line => (LINE, DEFAULT_ICON_SIZE - 3.0),
        Tool::Arrow => (ARROW, DEFAULT_ICON_SIZE - 7.0),
        Tool::Rectangle => (RECTANGLE, DEFAULT_ICON_SIZE),
        Tool::Circle => (CIRCLE, DEFAULT_ICON_SIZE - 6.0),
        Tool::NumeratedArrow => (NUMERATED_ARROW, DEFAULT_ICON_SIZE - 6.0),
    }
}

/// Icons not tied to any Tool (e.g. SettingsWidget::Toggle icons).
/// Add every new ToggleVisual::Icon svg here so load_icons_cache() preloads it.
pub const EXTRA_ICONS: &[&str] = &[
    ITALIC,
    BOLD,
];