use crate::tools::Tool;
use crate::types::Finish;

pub const SELECTION: &str = include_str!("../../assets/icons/selection.svg");
pub const ARROW: &str = include_str!("../../assets/icons/arrow.svg");
pub const RECTANGLE: &str = include_str!("../../assets/icons/rectangle.svg");
pub const CIRCLE: &str = include_str!("../../assets/icons/circle.svg");
pub const TEXT: &str = include_str!("../../assets/icons/text.svg");
pub const PEN: &str = include_str!("../../assets/icons/pen.svg");
pub const LINE: &str = include_str!("../../assets/icons/line.svg");
pub const PICK: &str = include_str!("../../assets/icons/cursor.svg");
pub const EYEDROPPER: &str = include_str!("../../assets/icons/eyedropper.svg");
pub const NUMERATED_ARROW: &str = include_str!("../../assets/icons/numerated_arrow.svg");
pub const OCR: &str = include_str!("../../assets/icons/ocr.svg");
pub const ITALIC: &str = include_str!("../../assets/icons/italic.svg");
pub const BOLD: &str = include_str!("../../assets/icons/bold.svg");
pub const RETRY: &str = include_str!("../../assets/icons/retry.svg");
pub const COPY: &str = include_str!("../../assets/icons/copy.svg");
pub const GLOBE: &str = include_str!("../../assets/icons/globe.svg");
pub const DOWNLOAD: &str = include_str!("../../assets/icons/download.svg");
pub const CLOSE: &str = include_str!("../../assets/icons/close.svg");
pub const TRASH: &str = include_str!("../../assets/icons/trash.svg");
pub const CHECK: &str = include_str!("../../assets/icons/check.svg");
pub const PIN: &str = include_str!("../../assets/icons/pin.svg");
pub const SAVE: &str = include_str!("../../assets/icons/save.svg");

/// used in the toolbar
pub fn get_svg_for_tool(tool: Tool) -> (&'static str, f32) {
    let size = &crate::config::get().toolbar.icons;
    match tool {
        Tool::Selection => (SELECTION, size.selection),
        Tool::Pick => (PICK, size.pick),
        Tool::Eyedropper => (EYEDROPPER, size.eyedropper),
        Tool::Text => (TEXT, size.text),
        Tool::Pen => (PEN, size.pen),
        Tool::Line => (LINE, size.line),
        Tool::Arrow => (ARROW, size.arrow),
        Tool::Rectangle => (RECTANGLE, size.rectangle),
        Tool::Circle => (CIRCLE, size.circle),
        Tool::NumeratedArrow => (NUMERATED_ARROW, size.numerated_arrow),
        Tool::Ocr => (OCR, size.ocr),
    }
}

pub fn get_svg_for_finish(finish: Finish) -> (&'static str, f32) {
    let size = &crate::config::get().toolbar.icons;
    match finish {
        Finish::Pin => (PIN, size.pin),
        Finish::Copy => (COPY, size.copy),
        Finish::Save => (SAVE, size.save),
    }
}

/// icons besides tools
pub const EXTRA_ICONS: &[&str] = &[
    ITALIC, BOLD, RETRY, COPY, GLOBE, DOWNLOAD, CLOSE, TRASH, CHECK, PIN, SAVE,
];

#[cfg(test)]
mod tests {
    use super::*;
    use strum::IntoEnumIterator;

    /// `load_icons_cache` parses every icon with `expect`, so inccorrent one is
    /// a panic on launch rather than a missing glyph. Catch it here instead.
    #[test]
    fn every_icon_parses() {
        let opt = usvg::Options::default();
        let all = Tool::iter()
            .map(|tool| get_svg_for_tool(tool).0)
            .chain(EXTRA_ICONS.iter().copied());
        for svg in all {
            usvg::Tree::from_str(svg, &opt).expect("embedded icon must parse");
        }
    }
}
