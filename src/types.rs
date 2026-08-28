pub mod annotations;
pub mod capture;
pub mod events;
pub mod icons;
pub mod magnifier;
pub mod selection;
pub mod text;
pub mod toolbar;
pub mod settings_panel;
pub mod panel;
pub mod text_field;
pub mod tool_settings;
pub mod click;
pub mod color_popover; 

pub use annotations::*;
pub use capture::*;
pub use events::*;
pub use icons::*;
pub use magnifier::*;
pub use selection::*;
pub use text::*;
pub use toolbar::*;
pub use settings_panel::*;
pub use panel::*;
pub use text_field::*;
pub use tool_settings::*;
pub use click::*;
pub use color_popover::*;

use tiny_skia::Rect;
#[derive(Clone, Copy, Debug)]
pub struct SignedRect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl SignedRect {
    pub fn width(&self) -> f32 {
        self.right - self.left
    }
    pub fn height(&self) -> f32 {
        self.bottom - self.top
    }

    pub fn to_rect(&self) -> Option<Rect> {
        Rect::from_ltrb(
            self.left.min(self.right),
            self.top.min(self.bottom),
            self.left.max(self.right),
            self.top.max(self.bottom),
        )
    }
}
