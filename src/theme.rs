// color, radius, stroke, font, anim, shadow and etc, everything visual.
// but here are only tokens that are shared between at least two components.
// if not, its constants stay in their prespective file.

use tiny_skia::Color;

// ==========================================
// Color
// ==========================================

/// rgba struct since there is multiple definition of color
/// (tiny_skia::color, usvg::color, (u8,u8,u8,u8) tuple in ocr) 
/// while we want to have a single source of truth
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    pub fn color(self) -> Color {
        Color::from_rgba8(self.0, self.1, self.2, self.3)
    }

    /// for transparency animations multiplying the alpha of the token by `k`
    pub fn fade(self, k: f32) -> Color {
        let mut c = self.color();
        c.set_alpha(c.alpha() * k.clamp(0.0, 1.0));
        c
    }

    pub fn cosmic(self) -> cosmic_text::Color {
        cosmic_text::Color::rgba(self.0, self.1, self.2, self.3)
    }

    pub fn usvg(self) -> usvg::Color {
        usvg::Color {
            red: self.0,
            green: self.1,
            blue: self.2,
        }
    }

    pub fn alpha(self) -> u8 {
        self.3
    }

    pub fn with_alpha(self, alpha: u8) -> Self {
        Self(self.0, self.1, self.2, alpha)
    }

    /// Parses "#RRGGBB" or "#RRGGBBAA" (case-insensitive, alpha defaults to 255).
    pub fn parse_hex(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#').unwrap_or(s);
        let byte = |i: usize| u8::from_str_radix(s.get(i..i + 2)?, 16).ok();
        match s.len() {
            6 => Some(Self(byte(0)?, byte(2)?, byte(4)?, 255)),
            8 => Some(Self(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => None,
        }
    }
}

impl From<Rgba> for Color {
    fn from(v: Rgba) -> Self {
        v.color()
    }
}

pub mod color {
    use super::Rgba;


    /// background color of all panels, popovers and toasts
    pub fn panel() -> Rgba {
        crate::config::get().theme.panel_background
    }
    /// hovering color
    pub fn accent() -> Rgba {
        crate::config::get().theme.accent
    }
    /// selected color
    pub fn accent_bright() -> Rgba {
        crate::config::get().theme.accent_bright
    }
    /// main color of elements on the panel, like text, seperators and etc
    pub const ON_PANEL: Rgba = Rgba(205, 214, 244, 255); // text
    /// secondary small labels
    pub const MUTED: Rgba = Rgba(166, 173, 200, 255); // subtext0

    /// blue color of text selection
    pub fn select() -> Rgba {
        crate::config::get().theme.selection
    }
    /// input field
    pub const FIELD_BG: Rgba = Rgba(49, 50, 68, 255); // surface0
    /// empty part of downloading bar
    pub const TRACK: Rgba = Rgba(69, 71, 90, 255); // surface1
    pub const CARET: Rgba = Rgba(245, 224, 220, 255); // rosewater

    pub const SHADOW: Rgba = Rgba(17, 17, 27, 130); // crust

    /// panels border
    pub const BORDER_ON_DARK: Rgba = Rgba(69, 71, 90, 255); // surface1
    pub const BORDER_ON_LIGHT: Rgba = Rgba(17, 17, 27, 55); // crust

    /// dimming outside the selection, alpha comes from `general.dim_alpha`
    pub const DIM: Rgba = Rgba(17, 17, 27, 255); // crust
}

// ==========================================
// Panel geometry
// ==========================================

pub mod size {
    /// Height of the toolbar and of the settings panel.
    pub const PANEL_HEIGHT: f32 = 42.0;
    /// Padding between a panel's edge and its items.
    pub const PADDING: f32 = 8.0;
    /// Gap between a panel and whatever it is anchored to.
    pub const OFFSET: f32 = 5.0;
}

// ==========================================
// Border Radius
// ==========================================

pub mod radius {
    pub const PANEL: f32 = 8.0;
    pub const ITEM: f32 = 4.0;
    pub const SEPARATOR: f32 = 1.0;
}

// ==========================================
// Stroke width
// ==========================================

pub mod stroke {
    pub const BORDER: f32 = 1.0;
    /// downloading bar stroke
    pub const PROGRESS: f32 = 4.0;
}

// ==========================================
// font
// ==========================================

pub mod font {
    pub fn label() -> f32 {
        crate::config::get().theme.font_size
    }
    /// Two points below the label size
    pub fn small() -> f32 {
        label() - 2.0
    }
    /// Line height as a factor of the font size.
    pub const LINE_HEIGHT: f32 = 1.2;
}

// ==========================================
// Animation
// ==========================================

pub mod anim {
    use std::time::Duration;

    /// The duration of a single frame in milliseconds.
    const FRAME_MS: u64 = 10;
    pub const FRAME: Duration = Duration::from_millis(FRAME_MS);
    pub const DT: f32 = FRAME_MS as f32 / 1000.0;

    /// Opacity per second while a popover fades in or out.
    pub const POPOVER_FADE: f32 = 8.0;
    /// Closer than this to the target opacity counts as settled.
    pub const OPACITY_EPSILON: f32 = 0.001;
}

// ==========================================
// Annotations's shadow
// ==========================================

pub mod shadow {
    pub const OFFSET: (f32, f32) = (0.0, 3.0);
    pub const LAYERS: usize = 2;
    pub const SPREAD_PER_LAYER: f32 = 1.5;
    // for damaged zone calculation
    pub const WIDTH_BONUS: f32 = 4.0;
}
