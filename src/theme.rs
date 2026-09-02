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
}

impl From<Rgba> for Color {
    fn from(v: Rgba) -> Self {
        v.color()
    }
}

pub mod color {
    use super::Rgba;

    /// background color of all panels, popovers and toasts
    pub const PANEL: Rgba = Rgba(17, 17, 27, 250);
    /// hovering color
    pub const ACCENT: Rgba = Rgba(159, 48, 215, 255);
    /// selected color
    pub const ACCENT_BRIGHT: Rgba = Rgba(215, 132, 255, 255);
    /// main color of elements on the panel, like text, seperators and etc
    pub const ON_PANEL: Rgba = Rgba(255, 255, 255, 255);
    /// secondary small labels
    pub const MUTED: Rgba = Rgba(200, 200, 205, 160);

    /// blue color of text selection
    pub const SELECT: Rgba = Rgba(100, 150, 255, 110);
    /// input field
    pub const FIELD_BG: Rgba = Rgba(255, 255, 255, 18);
    pub const CARET: Rgba = Rgba(255, 255, 255, 220);

    pub const SHADOW: Rgba = Rgba(0, 0, 0, 130);

    /// panels border
    pub const BORDER_ON_DARK: Rgba = Rgba(255, 255, 255, 55);
    pub const BORDER_ON_LIGHT: Rgba = Rgba(0, 0, 0, 55);
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
}

// ==========================================
// font
// ==========================================

pub mod font {
    pub const LABEL: f32 = 14.0;
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

    pub const DIM_FADE: Duration = Duration::from_millis(400);
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
