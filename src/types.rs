pub mod annotations;
pub mod capture;
pub mod click;
pub mod color_popover;
pub mod events;
pub mod icons;
pub mod magnifier;
pub mod panel;
pub mod selection;
pub mod settings_panel;
pub mod text;
pub mod text_field;
pub mod toast;
pub mod tool_settings;
pub mod toolbar;

pub use annotations::*;
pub use capture::*;
pub use click::*;
pub use color_popover::*;
pub use events::*;
pub use icons::*;
pub use magnifier::*;
pub use panel::*;
pub use selection::*;
pub use settings_panel::*;
pub use text::*;
pub use text_field::*;
pub use toast::*;
pub use tool_settings::*;
pub use toolbar::*;

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
