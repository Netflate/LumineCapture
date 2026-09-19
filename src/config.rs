// One file at ~/.config/LumineCapture/config.toml, loaded once at startup.
// Every field must have a default

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use log::warn;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::ocr::settings::{Device, Mode};
use crate::theme::Rgba;
use crate::types::Outputs;

/// Declares a config section: every field sits next to its default.
macro_rules! section {
    ($name:ident { $($field:ident : $ty:ty = $default:expr),* $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
        #[serde(default)]
        pub struct $name {
            $(pub $field: $ty,)*
        }

        impl Default for $name {
            fn default() -> Self {
                Self { $($field: $default,)* }
            }
        }
    };
}

const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
    Rgba(r, g, b, 255)
}

// Catppuccin Mocha
const TEXT: Rgba = rgb(205, 214, 244);
const SUBTEXT0: Rgba = rgb(166, 173, 200);
const OVERLAY2: Rgba = rgb(147, 153, 178);
const SURFACE0: Rgba = rgb(49, 50, 68);
const SURFACE1: Rgba = rgb(69, 71, 90);
const BASE: Rgba = rgb(30, 30, 46);
const CRUST: Rgba = rgb(17, 17, 27);
const LAVENDER: Rgba = rgb(180, 190, 254);
const MAUVE: Rgba = rgb(203, 166, 247);
const BLUE: Rgba = rgb(137, 180, 250);
const ROSEWATER: Rgba = rgb(245, 224, 220);

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub save: Save,
    pub notifications: Notifications,
    pub animation: Animation,
    pub theme: Theme,
    pub toolbar: Toolbar,
    pub settings_panel: SettingsPanel,
    pub color_picker: ColorPicker,
    pub model_picker: ModelPicker,
    pub magnifier: Magnifier,
    pub selection: Selection,
    pub annotations: Annotations,
    pub tools: Tools,
    pub toasts: Toasts,
    pub input: Input,
    pub ocr: Ocr,
    pub log: Log,
    pub keys: Keys,
}

impl Default for Config {
    fn default() -> Self {
        let mut config = Self {
            general: General::default(),
            save: Save::default(),
            notifications: Notifications::default(),
            animation: Animation::default(),
            theme: Theme::default(),
            toolbar: Toolbar::default(),
            settings_panel: SettingsPanel::default(),
            color_picker: ColorPicker::default(),
            model_picker: ModelPicker::default(),
            magnifier: Magnifier::default(),
            selection: Selection::default(),
            annotations: Annotations::default(),
            tools: Tools::default(),
            toasts: Toasts::default(),
            input: Input::default(),
            ocr: Ocr::default(),
            log: Log::default(),
            keys: Keys::default(),
        };
        config.resolve();
        config
    }
}

impl Config {
    /// Fills every color left unset with the theme role it follows.
    fn resolve(&mut self) {
        let t = &self.theme;
        let toolbar = &mut self.toolbar;
        toolbar.background.or(t.background);
        toolbar.icon.or(t.foreground);
        toolbar.icon_hovered.or(t.on_accent);
        toolbar.icon_selected.or(t.on_accent);
        toolbar.button_hovered.or(t.hover);
        toolbar.button_selected.or(t.accent);
        toolbar.separator.or(t.foreground);

        let panel = &mut self.settings_panel;
        panel.background.or(t.background);
        panel.text.or(t.foreground);
        panel.icon.or(t.foreground);
        panel.icon_hovered.or(t.hover);
        panel.icon_active.or(t.accent);
        panel.separator.or(t.foreground);

        let picker = &mut self.color_picker;
        picker.background.or(t.background);
        picker.text.or(t.foreground);
        picker.label.or(t.muted);
        picker.marker.or(t.foreground);

        self.model_picker.background.or(t.background);

        let magnifier = &mut self.magnifier;
        magnifier.outline.or(t.foreground);
        magnifier.label_background.or(t.background);
        magnifier.label_text.or(t.foreground);

        self.selection.border.or(t.foreground);
        self.annotations.handles.color.or(t.foreground);
        self.annotations.text.selected_text.or(t.foreground);

        self.toasts.background.or(t.background);
        self.toasts.text.or(t.foreground);

        let look = &mut self.ocr.look;
        look.badge_background.or(t.background);
        look.badge_icon.or(t.foreground);
        look.scan.or(t.accent);
    }
}

// ==========================================
// Value types
// ==========================================

/// A color that follows a theme role unless the config sets it; `""` also means "follow".
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ThemeColor(Option<Rgba>);

impl ThemeColor {
    pub fn get(self) -> Rgba {
        // resolve() runs on every Config, so an unset color here is a bug; make it loud
        self.0.unwrap_or(Rgba(255, 0, 255, 255))
    }

    fn or(&mut self, role: Rgba) {
        self.0.get_or_insert(role);
    }
}

impl<'de> Deserialize<'de> for ThemeColor {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        if s.trim().is_empty() {
            return Ok(Self(None));
        }
        parse_rgba(&s).map(|c| Self(Some(c))).map_err(serde::de::Error::custom)
    }
}

impl Serialize for ThemeColor {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Some(color) => color.serialize(s),
            None => s.serialize_str(""),
        }
    }
}

fn parse_rgba(s: &str) -> Result<Rgba, String> {
    Rgba::parse_hex(s).ok_or_else(|| format!("invalid color {s:?}, expected #RRGGBB or #RRGGBBAA"))
}

impl<'de> Deserialize<'de> for Rgba {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        parse_rgba(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

impl Serialize for Rgba {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let Rgba(r, g, b, a) = *self;
        let hex = if a == 255 {
            format!("#{r:02X}{g:02X}{b:02X}")
        } else {
            format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
        };
        s.serialize_str(&hex)
    }
}

impl<'de> Deserialize<'de> for Outputs {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Outputs::parse(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

impl Serialize for Outputs {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let letters: String = [(self.copy, 'c'), (self.pin, 'p'), (self.save, 's')]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, letter)| letter)
            .collect();
        s.serialize_str(&letters)
    }
}

// ==========================================
// Sections
// ==========================================

section!(General {
    save_always: bool = true,
    accept: Outputs = Outputs { copy: true, ..Outputs::default() },
});

section!(Save {
    directory: String = "screenshots".into(),
    month_format: String = "%Y-%m".into(),
    filename_format: String = "%Y-%m-%d_%H-%M".into(),
});

section!(Notifications {
    enabled: bool = true,
});

section!(Animation {
    speed: f32 = 1.0,
    frame_ms: u64 = 10,
    popover_fade: f32 = 8.0,
    toolbar_fade: f32 = 5.0,
    toolbar_slide: f32 = 12.0,
    toolbar_slide_distance: f32 = 340.0,
    toast_fade_in: f32 = 0.18,
    toast_fade_out: f32 = 0.14,
});

section!(Theme {
    font_size: f32 = 14.0,
    small_font_size: f32 = 12.0,
    line_height: f32 = 1.2,
    background: Rgba = BASE.with_alpha(250),
    foreground: Rgba = TEXT,
    muted: Rgba = SUBTEXT0,
    hover: Rgba = LAVENDER,
    accent: Rgba = MAUVE,
    on_accent: Rgba = BASE,
    border: Rgba = SURFACE1,
    border_on_light: Rgba = CRUST.with_alpha(55),
    field: Rgba = SURFACE0,
    track: Rgba = SURFACE1,
    caret: Rgba = ROSEWATER,
    text_selection: Rgba = BLUE.with_alpha(110),
    panel_height: f32 = 42.0,
    panel_padding: f32 = 8.0,
    panel_margin: f32 = 5.0,
    panel_radius: f32 = 8.0,
    item_radius: f32 = 4.0,
    border_width: f32 = 1.0,
    separator_width: f32 = 2.0,
    separator_length: f32 = 0.5,
    separator_radius: f32 = 1.0,
    progress_height: f32 = 4.0,
    caret_width: f32 = 1.5,
    field_text_height: f32 = 0.75,
});

section!(ToolbarIcons {
    selection: f32 = 31.0,
    pick: f32 = 26.0,
    ocr: f32 = 29.5,
    eyedropper: f32 = 28.0,
    text: f32 = 22.0,
    pen: f32 = 20.0,
    line: f32 = 28.0,
    arrow: f32 = 24.0,
    rectangle: f32 = 31.0,
    circle: f32 = 25.0,
    numerated_arrow: f32 = 25.0,
    pin: f32 = 21.0,
    copy: f32 = 20.0,
    save: f32 = 20.0,
});

section!(Toolbar {
    button_size: f32 = 35.0,
    button_gap: f32 = 4.0,
    separator_slot: f32 = 20.0,
    highlight_height: f32 = 0.8,
    background: ThemeColor = ThemeColor::default(),
    icon: ThemeColor = ThemeColor::default(),
    icon_hovered: ThemeColor = ThemeColor::default(),
    icon_selected: ThemeColor = ThemeColor::default(),
    button_hovered: ThemeColor = ThemeColor::default(),
    button_selected: ThemeColor = ThemeColor::default(),
    separator: ThemeColor = ThemeColor::default(),
    icons: ToolbarIcons = ToolbarIcons::default(),
});

section!(SettingsPanel {
    item_gap: f32 = 8.0,
    item_height: f32 = 0.7,
    separator_slot: f32 = 16.0,
    char_width: f32 = 7.0,
    label_padding: f32 = 8.0,
    swatch_size: f32 = 28.0,
    swatch_dot: f32 = 0.35,
    icon_button_size: f32 = 28.0,
    icon_size: f32 = 16.0,
    copy_icon_size: f32 = 15.0,
    stepper_width: f32 = 84.0,
    stepper_bold: bool = true,
    stepper_arrow_zone: f32 = 30.0,
    stepper_arrow_width: f32 = 15.0,
    stepper_arrow_height: f32 = 6.0,
    stepper_arrow_gap: f32 = 9.0,
    stepper_arrow_stroke: f32 = 1.6,
    hex_width: f32 = 84.0,
    rgb_width: f32 = 120.0,
    download_width: f32 = 196.0,
    download_label_width: f32 = 96.0,
    download_percent_width: f32 = 38.0,
    checkbox_size: f32 = 18.0,
    checkbox_gap: f32 = 6.0,
    checkbox_radius: f32 = 3.0,
    checkmark_width: f32 = 2.0,
    background: ThemeColor = ThemeColor::default(),
    text: ThemeColor = ThemeColor::default(),
    icon: ThemeColor = ThemeColor::default(),
    icon_hovered: ThemeColor = ThemeColor::default(),
    icon_active: ThemeColor = ThemeColor::default(),
    separator: ThemeColor = ThemeColor::default(),
});

section!(ColorPicker {
    max_recent: usize = 6,
    palette: Vec<Rgba> = vec![
        rgb(0, 0, 0),
        rgb(255, 255, 255),
        rgb(243, 139, 168),
        rgb(250, 179, 135),
        rgb(249, 226, 175),
        rgb(166, 227, 161),
    ],
    padding: f32 = 15.0,
    sv_size: f32 = 170.0,
    sv_radius: f32 = 10.0,
    hue_gap: f32 = 12.0,
    hue_width: f32 = 17.0,
    hue_radius: f32 = 5.0,
    marker_radius: f32 = 10.0,
    marker_stroke: f32 = 2.0,
    marker_outline: f32 = 0.1,
    border_width: f32 = 2.0,
    label_gap: f32 = 10.0,
    label_height: f32 = 14.0,
    label_font_size: f32 = 13.0,
    swatch_row_gap: f32 = 6.0,
    swatch_size: f32 = 22.0,
    swatch_gap: f32 = 8.0,
    swatch_hover_grow: f32 = 3.0,
    eyedropper_icon_size: f32 = 13.0,
    row_gap: f32 = 10.0,
    field_height: f32 = 24.0,
    field_gap: f32 = 6.0,
    field_radius: f32 = 4.0,
    field_text_inset: f32 = 6.0,
    hex_label_width: f32 = 28.0,
    rgba_label_width: f32 = 14.0,
    background: ThemeColor = ThemeColor::default(),
    text: ThemeColor = ThemeColor::default(),
    label: ThemeColor = ThemeColor::default(),
    marker: ThemeColor = ThemeColor::default(),
    marker_outline_color: Rgba = Rgba(0, 0, 0, 160),
});

section!(ModelPicker {
    width: f32 = 320.0,
    padding: f32 = 8.0,
    title_height: f32 = 30.0,
    title_gap: f32 = 4.0,
    row_height: f32 = 44.0,
    row_padding: f32 = 10.0,
    name_top: f32 = 5.0,
    name_height: f32 = 20.0,
    note_top: f32 = 24.0,
    note_height: f32 = 15.0,
    bar_top: f32 = 30.0,
    status_width: f32 = 62.0,
    button_size: f32 = 26.0,
    icon_size: f32 = 14.0,
    background: ThemeColor = ThemeColor::default(),
});

section!(Magnifier {
    zoom: f32 = 10.0,
    cells: u32 = 21,
    offset: f32 = 24.0,
    outline_width: f32 = 2.0,
    grid_width: f32 = 1.0,
    label_height: f32 = 26.0,
    label_gap: f32 = 6.0,
    swatch_size: f32 = 14.0,
    swatch_gap: f32 = 8.0,
    swatch_radius: f32 = 3.0,
    grid: Rgba = TEXT.with_alpha(40),
    crosshair: Rgba = OVERLAY2.with_alpha(80),
    outline: ThemeColor = ThemeColor::default(),
    label_background: ThemeColor = ThemeColor::default(),
    label_text: ThemeColor = ThemeColor::default(),
});

impl Magnifier {
    pub fn size(&self) -> u32 {
        (self.cells as f32 * self.zoom) as u32
    }
}

section!(Selection {
    dim: Rgba = CRUST.with_alpha(140),
    border: ThemeColor = ThemeColor::default(),
    border_width: f32 = 2.0,
    border_radius: f32 = 8.0,
    nudge: f32 = 1.0,
    nudge_fast: f32 = 10.0,
});

section!(Shadow {
    color: Rgba = CRUST.with_alpha(130),
    offset: [f32; 2] = [0.0, 3.0],
    layers: usize = 2,
    spread: f32 = 1.5,
    falloff: f32 = 1.2,
    pen_halo: f32 = 3.0,
});

section!(Handles {
    color: ThemeColor = ThemeColor::default(),
    width: f32 = 3.0,
    radius: f32 = 4.0,
    min_length: f32 = 8.0,
    corner_length: f32 = 0.2,
    middle_length: f32 = 0.32,
});

section!(Arrow {
    head_length: f32 = 4.0,
    head_min: f32 = 12.0,
    head_max: f32 = 0.6,
    head_width: f32 = 0.55,
});

section!(Numbered {
    circle: f32 = 3.0,
    tail_width: f32 = 0.48,
    digit_sizes: [f32; 4] = [1.1, 0.85, 0.65, 0.5],
    bold: bool = true,
    dark_digits: Rgba = rgb(0, 0, 0),
    light_digits: Rgba = rgb(255, 255, 255),
    digits_flip: f32 = 0.55,
});

section!(TextBox {
    line_height: f32 = 1.2,
    min_width: f32 = 10.0,
    resize_min: f32 = 6.0,
    resize_max: f32 = 300.0,
    selected_text: ThemeColor = ThemeColor::default(),
});

section!(Annotations {
    shadow: Shadow = Shadow::default(),
    handles: Handles = Handles::default(),
    arrow: Arrow = Arrow::default(),
    numbered: Numbered = Numbered::default(),
    text: TextBox = TextBox::default(),
});

section!(Stroke {
    width: f32 = 12.0,
    min: f32 = 1.0,
    max: f32 = 40.0,
    step: f32 = 1.0,
});

section!(Font {
    size: f32 = 24.0,
    min: f32 = 8.0,
    max: f32 = 72.0,
    step: f32 = 1.0,
    bold: bool = true,
    italic: bool = false,
});

section!(Pen {
    smoothing: f32 = 0.7,
    min_distance: f32 = 1.0,
});

section!(Tools {
    color: Rgba = rgb(255, 255, 255),
    fill: bool = false,
    stroke: Stroke = Stroke::default(),
    font: Font = Font::default(),
    pen: Pen = Pen::default(),
});

section!(Toasts {
    height: f32 = 38.0,
    padding: f32 = 18.0,
    margin: f32 = 8.0,
    lifetime: f32 = 4.0,
    short_lifetime: f32 = 3.0,
    copied_lifetime: f32 = 1.4,
    background: ThemeColor = ThemeColor::default(),
    text: ThemeColor = ThemeColor::default(),
});

section!(Input {
    double_click_ms: u64 = 400,
    double_click_distance: f32 = 6.0,
    handle_hit_width: f32 = 20.0,
    corner_ratio: f32 = 0.3,
    corner_min: f32 = 8.0,
    corner_max: f32 = 40.0,
    scroll_sensitivity: f32 = 4.0,
    scroll_step_pixels: f32 = 10.0,
    hold_delay_ms: u64 = 400,
    hold_repeat_ms: u64 = 120,
    hold_fast_after: u32 = 8,
    hold_fast_repeat_ms: u64 = 40,
});

section!(OcrLook {
    region_shade: Rgba = CRUST.with_alpha(96),
    plate: Rgba = TEXT.with_alpha(48),
    plate_pad_x: f32 = 4.0,
    plate_pad_y: f32 = 2.0,
    plate_radius: f32 = 5.0,
    selection_radius: f32 = 2.0,
    badge_size: f32 = 62.0,
    badge_radius: f32 = 17.0,
    badge_icon_size: f32 = 30.0,
    scan_rate: f32 = 1.15,
    scan_width: f32 = 2.5,
    badge_background: ThemeColor = ThemeColor::default(),
    badge_icon: ThemeColor = ThemeColor::default(),
    scan: ThemeColor = ThemeColor::default(),
});

section!(OcrLayout {
    block_gap: f32 = 0.4,
    block_gap_max: f32 = 0.7,
    block_overlap: f32 = 0.5,
    block_align: f32 = 0.35,
    row_tolerance: f32 = 0.6,
    row_split_gap: f32 = 4.0,
    column_gutter: f32 = 6.0,
});

section!(OcrDetect {
    max_side: u32 = 1920,
    score_threshold: f32 = 0.3,
    box_threshold: f32 = 0.6,
    unclip_ratio: f32 = 1.5,
    max_candidates: usize = 1000,
});

section!(OcrFilter {
    min_confidence: f32 = 0.55,
    single_char_confidence: f32 = 0.6,
    single_char_height: f32 = 0.6,
    glyph_height: f32 = 1.8,
    glyph_aspect: f32 = 1.2,
    speck_size: f32 = 0.5,
    rule_contrast: i32 = 32,
    rule_coverage: f32 = 0.9,
    rule_reach: f32 = 0.5,
});

section!(Ocr {
    mode: Mode = Mode::OnDemand,
    device: Device = Device::Auto,
    daemon_idle_secs: u64 = 900,
    hit_slack: f32 = 3.0,
    vertical_slack: f32 = 10.0,
    min_region: f32 = 16.0,
    look: OcrLook = OcrLook::default(),
    layout: OcrLayout = OcrLayout::default(),
    detect: OcrDetect = OcrDetect::default(),
    filter: OcrFilter = OcrFilter::default(),
});

section!(Log {
    level: Option<String> = None,
    max_file_size_mb: u64 = 1,
});

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Binding {
    One(String),
    Many(Vec<String>),
}

impl Binding {
    pub fn chords(&self) -> Vec<&str> {
        let all: Vec<&str> = match self {
            Binding::One(chord) => vec![chord.as_str()],
            Binding::Many(chords) => chords.iter().map(String::as_str).collect(),
        };
        all.into_iter().filter(|c| !c.trim().is_empty()).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Keys(pub BTreeMap<String, Binding>);

impl Default for Keys {
    fn default() -> Self {
        let binding = |chords: &[&str]| match chords {
            [one] => Binding::One((*one).into()),
            many => Binding::Many(many.iter().map(|c| (*c).into()).collect()),
        };
        Self(
            crate::keys::ACTIONS
                .iter()
                .map(|(name, _, chords)| ((*name).into(), binding(chords)))
                .collect(),
        )
    }
}

pub const TEMPLATE: &str = r##"# LumineCapture config. Delete a line (or the whole file) to get its default back.
# A line with a mistake is skipped with a warning in ~/.local/state/LumineCapture/lumine.log,
# the rest of the file still applies.
#
# Colors are "#RRGGBB" or "#RRGGBBAA". A commented color follows the [theme] role written
# next to it: uncomment it to give that one element its own color ("" also means "follow").
# Sizes are in logical pixels unless the comment says "ratio" (a share of something else).

[general]
save_always = true
# what Enter, a double click on the selection, the --region release and the instant modes
# do with the shot: any of c (copy), p (pin), s (save) in any order, "" does nothing.
# --to overrides it for one run
accept = "c"

[save]
directory = "screenshots"
month_format = "%Y-%m"
filename_format = "%Y-%m-%d_%H-%M"

[notifications]
enabled = true

[animation]
# multiplies every animation below; 0.1 at least
speed = 1.0
# one animation tick
frame_ms = 10
# opacity per second
popover_fade = 8.0
toolbar_fade = 5.0
# share of the remaining distance per second
toolbar_slide = 12.0
# the toolbar never slides in from further than this
toolbar_slide_distance = 340.0
# seconds
toast_fade_in = 0.18
toast_fade_out = 0.14

# Shared by every panel, popover, toast and the magnifier label.
[theme]
font_size = 14.0
small_font_size = 12.0
# ratio of the font size
line_height = 1.2
background = "#1E1E2EFA"
foreground = "#CDD6F4"
muted = "#A6ADC8"
hover = "#B4BEFE"
# selected and active things, progress bars, titles
accent = "#CBA6F7"
# icons and text drawn over a hover or accent fill
on_accent = "#1E1E2E"
border = "#45475A"
# used instead of border when the background is light
border_on_light = "#11111B37"
field = "#313244"
# the empty part of progress bars
track = "#45475A"
caret = "#F5E0DC"
text_selection = "#89B4FA6E"
panel_height = 42.0
panel_padding = 8.0
# gap between a panel and what it is attached to
panel_margin = 5.0
panel_radius = 8.0
item_radius = 4.0
border_width = 1.0
separator_width = 2.0
# ratio of the panel height
separator_length = 0.5
separator_radius = 1.0
progress_height = 4.0
caret_width = 1.5
# height of the caret and the text selection in input fields, ratio of the field height
field_text_height = 0.75

[toolbar]
button_size = 35.0
button_gap = 4.0
separator_slot = 20.0
# height of the hover and selected fill, ratio of the toolbar height
highlight_height = 0.8
# background = "#1E1E2EFA"      # theme.background
# icon = "#CDD6F4"              # theme.foreground
# icon_hovered = "#1E1E2E"      # theme.on_accent
# icon_selected = "#1E1E2E"     # theme.on_accent
# button_hovered = "#B4BEFE"    # theme.hover
# button_selected = "#CBA6F7"   # theme.accent
# separator = "#CDD6F4"         # theme.foreground

# icon size of every toolbar button
[toolbar.icons]
selection = 31.0
pick = 26.0
ocr = 29.5
eyedropper = 28.0
text = 22.0
pen = 20.0
line = 28.0
arrow = 24.0
rectangle = 31.0
circle = 25.0
numerated_arrow = 25.0
pin = 21.0
copy = 20.0
save = 20.0

# The panel with the current tool's settings.
[settings_panel]
item_gap = 8.0
# ratio of the panel height
item_height = 0.7
separator_slot = 16.0
# rough width of one character, used to size labels
char_width = 7.0
label_padding = 8.0
swatch_size = 28.0
# radius of the color dot, ratio of the swatch
swatch_dot = 0.35
icon_button_size = 28.0
icon_size = 16.0
copy_icon_size = 15.0
stepper_width = 84.0
stepper_bold = true
stepper_arrow_zone = 30.0
stepper_arrow_width = 15.0
stepper_arrow_height = 6.0
stepper_arrow_gap = 9.0
stepper_arrow_stroke = 1.6
hex_width = 84.0
rgb_width = 120.0
download_width = 196.0
download_label_width = 96.0
download_percent_width = 38.0
checkbox_size = 18.0
checkbox_gap = 6.0
checkbox_radius = 3.0
checkmark_width = 2.0
# background = "#1E1E2EFA"      # theme.background
# text = "#CDD6F4"              # theme.foreground
# icon = "#CDD6F4"              # theme.foreground
# icon_hovered = "#B4BEFE"      # theme.hover
# icon_active = "#CBA6F7"       # theme.accent
# separator = "#CDD6F4"         # theme.foreground

[color_picker]
max_recent = 6
# fills the recent colors until there is a history
palette = ["#000000", "#FFFFFF", "#F38BA8", "#FAB387", "#F9E2AF", "#A6E3A1"]
padding = 15.0
sv_size = 170.0
sv_radius = 10.0
hue_gap = 12.0
hue_width = 17.0
hue_radius = 5.0
marker_radius = 10.0
marker_stroke = 2.0
marker_outline = 0.1
border_width = 2.0
label_gap = 10.0
label_height = 14.0
label_font_size = 13.0
swatch_row_gap = 6.0
swatch_size = 22.0
swatch_gap = 8.0
swatch_hover_grow = 3.0
eyedropper_icon_size = 13.0
row_gap = 10.0
field_height = 24.0
field_gap = 6.0
field_radius = 4.0
field_text_inset = 6.0
hex_label_width = 28.0
rgba_label_width = 14.0
marker_outline_color = "#000000A0"
# background = "#1E1E2EFA"      # theme.background
# text = "#CDD6F4"              # theme.foreground
# label = "#A6ADC8"             # theme.muted
# marker = "#CDD6F4"            # theme.foreground

# The OCR language list.
[model_picker]
width = 320.0
padding = 8.0
title_height = 30.0
title_gap = 4.0
row_height = 44.0
row_padding = 10.0
name_top = 5.0
name_height = 20.0
note_top = 24.0
note_height = 15.0
bar_top = 30.0
status_width = 62.0
button_size = 26.0
icon_size = 14.0
# background = "#1E1E2EFA"      # theme.background

[magnifier]
zoom = 10.0
# pixels across, must be odd
cells = 21
# distance from the pointer
offset = 24.0
outline_width = 2.0
grid_width = 1.0
label_height = 26.0
label_gap = 6.0
swatch_size = 14.0
swatch_gap = 8.0
swatch_radius = 3.0
grid = "#CDD6F428"
crosshair = "#9399B250"
# outline = "#CDD6F4"           # theme.foreground
# label_background = "#1E1E2EFA" # theme.background
# label_text = "#CDD6F4"        # theme.foreground

# The captured region on the screen.
[selection]
# over everything outside the selection
dim = "#11111B8C"
border_width = 2.0
border_radius = 8.0
# arrow keys move and resize it by this much, Shift by the fast one
nudge = 1.0
nudge_fast = 10.0
# border = "#CDD6F4"            # theme.foreground

[annotations.shadow]
color = "#11111B82"
offset = [0.0, 3.0]
layers = 2
spread = 1.5
# how fast each next layer fades
falloff = 1.2
# the pen gets a halo instead of a drop shadow, this much wider than the stroke
pen_halo = 3.0

# The frame around a selected annotation.
[annotations.handles]
width = 3.0
radius = 4.0
min_length = 8.0
# ratio of the side
corner_length = 0.2
middle_length = 0.32
# color = "#CDD6F4"             # theme.foreground

[annotations.arrow]
# head length, ratio of the stroke width
head_length = 4.0
head_min = 12.0
# the head takes at most this ratio of the arrow
head_max = 0.6
# half the head width, ratio of its length
head_width = 0.55

[annotations.numbered]
# circle radius, ratio of the stroke width
circle = 3.0
# ratio of the circle radius
tail_width = 0.48
# digit size for 1, 2, 3 and 4+ digits, ratio of the circle radius
digit_sizes = [1.1, 0.85, 0.65, 0.5]
bold = true
dark_digits = "#000000"
light_digits = "#FFFFFF"
# circles brighter than this get dark digits
digits_flip = 0.55

[annotations.text]
line_height = 1.2
min_width = 10.0
# font size limits while resizing a text box with the mouse
resize_min = 6.0
resize_max = 300.0
# selected_text = "#CDD6F4"     # theme.foreground

# What a new annotation starts with. The last used values are remembered in
# ~/.local/state/LumineCapture/tools.toml and win over these.
[tools]
color = "#FFFFFF"
fill = false

[tools.stroke]
width = 12.0
min = 1.0
max = 40.0
step = 1.0

[tools.font]
size = 24.0
min = 8.0
max = 72.0
step = 1.0
bold = true
italic = false

[tools.pen]
# 0 draws the raw pointer path, closer to 1 is smoother (and lags more)
smoothing = 0.7
min_distance = 1.0

[toasts]
height = 38.0
padding = 18.0
margin = 8.0
# seconds
lifetime = 4.0
short_lifetime = 3.0
copied_lifetime = 1.4
# background = "#1E1E2EFA"      # theme.background
# text = "#CDD6F4"              # theme.foreground

[input]
double_click_ms = 400
double_click_distance = 6.0
# how far from a selection or annotation edge it still grabs
handle_hit_width = 20.0
# size of the corner grab zones, ratio of the side, between corner_min and corner_max
corner_ratio = 0.3
corner_min = 8.0
corner_max = 40.0
scroll_sensitivity = 4.0
scroll_step_pixels = 10.0
# holding a stepper arrow
hold_delay_ms = 400
hold_repeat_ms = 120
hold_fast_after = 8
hold_fast_repeat_ms = 40

[ocr]
# on-demand  build the engine when OCR is used, inside this process
# at-launch  build it at every launch, inside this process
# daemon     keep a ready engine in a background process, reused by later launches
mode = "on-demand"
# only the daemon uses this: auto | cpu | gpu
device = "auto"
# seconds the daemon may sit unused before it exits, 0 keeps it forever
daemon_idle_secs = 900
# how far from a line the pointer still selects it
hit_slack = 3.0
vertical_slack = 10.0
# a smaller drag counts as a click, not a new region
min_region = 16.0

[ocr.look]
region_shade = "#11111B60"
plate = "#CDD6F430"
plate_pad_x = 4.0
plate_pad_y = 2.0
plate_radius = 5.0
selection_radius = 2.0
badge_size = 62.0
badge_radius = 17.0
badge_icon_size = 30.0
# sweeps per second
scan_rate = 1.15
scan_width = 2.5
# badge_background = "#1E1E2EFA" # theme.background
# badge_icon = "#CDD6F4"        # theme.foreground
# scan = "#CBA6F7"              # theme.accent

# How recognized lines are grouped, in median line heights. The daemon reads
# [ocr.layout], [ocr.detect] and [ocr.filter] when it starts:
# run `lumine-capture --ocr-daemon stop` to apply changes to a running one.
[ocr.layout]
block_gap = 0.4
block_gap_max = 0.7
block_overlap = 0.5
block_align = 0.35
row_tolerance = 0.6
row_split_gap = 4.0
column_gutter = 6.0

[ocr.detect]
# larger screenshots are scaled down to this before detection
max_side = 1920
score_threshold = 0.3
box_threshold = 0.6
unclip_ratio = 1.5
max_candidates = 1000

# Recognized text dropped as noise.
[ocr.filter]
min_confidence = 0.55
single_char_confidence = 0.6
single_char_height = 0.6
glyph_height = 1.8
glyph_aspect = 1.2
speck_size = 0.5
# table rules between cells
rule_contrast = 32
rule_coverage = 0.9
rule_reach = 0.5

[log]
# off | error | warn | info | debug | trace; unset keeps each process's own default
# level = "info"
max_file_size_mb = 1

[keys]
accept = "Return"
copy = "Ctrl+C"
save = "Ctrl+S"
pin = "Ctrl+P"
cancel = "Escape"
undo = "Ctrl+Z"
redo = ["Ctrl+Shift+Z", "Ctrl+Y"]
select_all = "Ctrl+A"
delete = ["Delete", "Backspace"]
toggle_ui = "Space"
toggle_magnifier = "M"
size_up = "]"
size_down = "["
tool_selection = "S"
tool_pick = "V"
tool_ocr = "O"
tool_eyedropper = "G"
tool_text = "T"
tool_pen = "P"
tool_line = "D"
tool_arrow = "A"
tool_rectangle = "R"
tool_circle = "C"
tool_numerated_arrow = "N"
move_left = "Left"
move_right = "Right"
move_up = "Up"
move_down = "Down"
move_left_fast = "Shift+Left"
move_right_fast = "Shift+Right"
move_up_fast = "Shift+Up"
move_down_fast = "Shift+Down"
resize_left = "Alt+Left"
resize_right = "Alt+Right"
resize_up = "Alt+Up"
resize_down = "Alt+Down"
resize_left_fast = "Alt+Shift+Left"
resize_right_fast = "Alt+Shift+Right"
resize_up_fast = "Alt+Shift+Up"
resize_down_fast = "Alt+Shift+Down"
"##;

static CONFIG: OnceLock<Config> = OnceLock::new();

static WARNINGS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn complain(message: String) {
    if let Ok(mut warnings) = WARNINGS.lock() {
        warnings.push(message);
    }
}

/// Logs what went wrong while reading the config; call once the logger is up.
pub fn log_warnings() {
    let warnings = WARNINGS.lock().map(|mut w| std::mem::take(&mut *w)).unwrap_or_default();
    for message in warnings {
        warn!("{message}");
    }
}

fn config_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("LumineCapture").join("config.toml"))
}

/// A broken file never gets further than this many skipped lines.
const MAX_BAD_LINES: usize = 64;

/// Reads a config, skipping every line that doesn't parse instead of dropping the whole file.
fn parse(text: &str, origin: &str) -> Config {
    let mut text = text.to_owned();
    for _ in 0..MAX_BAD_LINES {
        match toml::from_str::<Config>(&text) {
            Ok(mut config) => {
                warn_unknown_keys(&text, origin);
                config.resolve();
                validate(&mut config);
                return config;
            }
            Err(e) => {
                let Some(span) = e.span() else { break };
                let start = text[..span.start].rfind('\n').map_or(0, |i| i + 1);
                let end = text[span.start..].find('\n').map_or(text.len(), |i| span.start + i);
                let line = text[..start].matches('\n').count() + 1;
                complain(format!(
                    "config: {origin}:{line}: {}, skipping `{}`",
                    e.message().trim(),
                    text[start..end].trim()
                ));
                text.replace_range(start..end, "");
            }
        }
    }
    complain(format!("config: {origin} can't be read, using the defaults"));
    Config::default()
}

/// Options without a default value, so the serialized defaults don't list them.
const UNSET_BY_DEFAULT: &[&str] = &["log.level"];

fn warn_unknown_keys(text: &str, origin: &str) {
    let (Ok(user), Ok(known)) = (text.parse::<toml::Table>(), toml::Table::try_from(Config::default())) else {
        return;
    };
    fn walk(user: &toml::Table, known: &toml::Table, path: &str, origin: &str) {
        for (key, value) in user {
            let full = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
            match (known.get(key), value) {
                (None, _) if UNSET_BY_DEFAULT.contains(&full.as_str()) => {}
                (None, _) => complain(format!("config: {origin}: unknown key {full}, ignoring it")),
                // keys.rs names the unknown actions itself
                (Some(_), _) if full == "keys" => {}
                (Some(toml::Value::Table(known)), toml::Value::Table(user)) => walk(user, known, &full, origin),
                _ => {}
            }
        }
    }
    walk(&user, &known, "", origin);
}

fn validate(config: &mut Config) {
    let magnifier = &mut config.magnifier;
    if magnifier.cells.is_multiple_of(2) {
        complain(format!("config: magnifier.cells must be odd, using the default"));
        magnifier.cells = Magnifier::default().cells;
    }
    let animation = &mut config.animation;
    if animation.speed < 0.1 {
        complain(format!("config: animation.speed must be at least 0.1, using the default"));
        animation.speed = Animation::default().speed;
    }
    if animation.frame_ms == 0 {
        complain(format!("config: animation.frame_ms must be at least 1, using the default"));
        animation.frame_ms = Animation::default().frame_ms;
    }
    let pen = &mut config.tools.pen;
    if !(0.0..1.0).contains(&pen.smoothing) {
        complain(format!("config: tools.pen.smoothing must be at least 0 and below 1, using the default"));
        pen.smoothing = Pen::default().smoothing;
    }

    // clamp() panics when min > max, so a flipped range falls back as a whole
    let stroke = &mut config.tools.stroke;
    if stroke.min > stroke.max || stroke.step <= 0.0 {
        complain(format!("config: tools.stroke needs min <= max and a positive step, using the defaults"));
        *stroke = Stroke::default();
    }
    let font = &mut config.tools.font;
    if font.min > font.max || font.step <= 0.0 {
        complain(format!("config: tools.font needs min <= max and a positive step, using the defaults"));
        *font = Font::default();
    }
    let text = &mut config.annotations.text;
    if text.resize_min > text.resize_max {
        complain(format!("config: annotations.text.resize_min is above resize_max, using the defaults"));
        text.resize_min = TextBox::default().resize_min;
        text.resize_max = TextBox::default().resize_max;
    }
    if config.theme.line_height.is_nan() || config.theme.line_height <= 0.0 {
        complain(format!("config: theme.line_height must be above 0, using the default"));
        config.theme.line_height = Theme::default().line_height;
    }
    if config.annotations.text.line_height.is_nan() || config.annotations.text.line_height <= 0.0 {
        complain(format!("config: annotations.text.line_height must be above 0, using the default"));
        config.annotations.text.line_height = TextBox::default().line_height;
    }
    let input = &mut config.input;
    if input.corner_min > input.corner_max {
        complain(format!("config: input.corner_min is above corner_max, using the defaults"));
        input.corner_min = Input::default().corner_min;
        input.corner_max = Input::default().corner_max;
    }
}

fn write_template(path: &PathBuf) {
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(path, TEMPLATE));
    if let Err(e) = written {
        complain(format!("config: cannot write {}: {e}", path.display()));
    }
}

/// Config overide path set via `--config`. Only the first call takes effect
pub fn init(override_path: Option<PathBuf>) {
    let path = override_path.or_else(config_path);
    let config = match &path {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(text) => parse(&text, &path.display().to_string()),
            Err(_) => {
                write_template(path);
                Config::default()
            }
        },
        None => Config::default(),
    };
    let _ = CONFIG.set(config);
}

pub fn get() -> &'static Config {
    CONFIG.get_or_init(Config::default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(text: &str) -> Config {
        parse(text, "test")
    }

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(read(""), Config::default());
    }

    #[test]
    fn the_template_reads_back_as_the_defaults() {
        assert_eq!(read(TEMPLATE), Config::default());
    }

    #[test]
    fn the_template_mentions_every_option() {
        fn walk(table: &toml::Table, path: &str, missing: &mut Vec<String>) {
            for (key, value) in table {
                let full = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                match value {
                    // [keys] is covered by the template-reads-back test
                    toml::Value::Table(_) if full == "keys" => {}
                    toml::Value::Table(inner) => walk(inner, &full, missing),
                    _ => {
                        let line = |prefix: &str| format!("\n{prefix}{key} =");
                        if !TEMPLATE.contains(&line("")) && !TEMPLATE.contains(&line("# ")) {
                            missing.push(full);
                        }
                    }
                }
            }
        }
        let mut missing = Vec::new();
        walk(&toml::Table::try_from(Config::default()).unwrap(), "", &mut missing);
        assert!(missing.is_empty(), "not in TEMPLATE: {missing:?}");
    }

    #[test]
    fn partial_file_keeps_the_rest_default() {
        let config = read("[toolbar]\nbutton_size = 50.0\n");
        assert_eq!(config.toolbar.button_size, 50.0);
        assert_eq!(config.save, Save::default());
    }

    #[test]
    fn a_bad_line_is_skipped_and_the_rest_applies() {
        let config = read("[toolbar]\nbutton_size = \"big\"\nbutton_gap = 9.0\n[theme]\naccent = \"nope\"\nhover = \"#112233\"\n");
        assert_eq!(config.toolbar.button_size, Toolbar::default().button_size);
        assert_eq!(config.toolbar.button_gap, 9.0);
        assert_eq!(config.theme.accent, Theme::default().accent);
        assert_eq!(config.theme.hover, Rgba(0x11, 0x22, 0x33, 255));
    }

    #[test]
    fn colors_parse_with_and_without_alpha() {
        let config = read("[theme]\nhover = \"#112233\"\nbackground = \"#11223344\"\n");
        assert_eq!(config.theme.hover, Rgba(0x11, 0x22, 0x33, 255));
        assert_eq!(config.theme.background, Rgba(0x11, 0x22, 0x33, 0x44));
    }

    #[test]
    fn unset_colors_follow_the_theme() {
        let config = read("[theme]\nforeground = \"#112233\"\n[toolbar]\nicon_selected = \"#445566\"\nicon_hovered = \"\"\n");
        assert_eq!(config.toolbar.icon.get(), Rgba(0x11, 0x22, 0x33, 255));
        assert_eq!(config.toolbar.icon_selected.get(), Rgba(0x44, 0x55, 0x66, 255));
        assert_eq!(config.toolbar.icon_hovered.get(), config.theme.on_accent);
    }

    #[test]
    fn unknown_keys_are_ignored() {
        assert_eq!(read("[general]\nfuture_key = 1\n[nope]\nx = 1\n"), Config::default());
    }

    #[test]
    fn accept_takes_letters_and_can_be_turned_off() {
        let config = read("[general]\naccept = \"ps\"\n");
        assert_eq!(config.general.accept, Outputs { pin: true, save: true, copy: false });
        assert!(read("[general]\naccept = \"\"\n").general.accept.is_empty());
        assert_eq!(read("[general]\naccept = \"x\"\n").general.accept, General::default().accept);
    }

    #[test]
    fn keys_take_a_string_or_a_list() {
        let config = read("[keys]\nsave = \"Ctrl+Shift+S\"\nredo = [\"Ctrl+Y\", \"\"]\n");
        assert_eq!(config.keys.0["save"].chords(), ["Ctrl+Shift+S"]);
        assert_eq!(config.keys.0["redo"].chords(), ["Ctrl+Y"]);
    }

    #[test]
    fn even_magnifier_cells_fall_back_to_the_default() {
        assert_eq!(read("[magnifier]\ncells = 20\n").magnifier.cells, Magnifier::default().cells);
    }

    #[test]
    fn too_slow_animation_speed_falls_back_to_the_default() {
        assert_eq!(read("[animation]\nspeed = 0.0\n").animation.speed, Animation::default().speed);
    }

    #[test]
    fn a_flipped_range_falls_back_to_the_default() {
        let config = read("[tools.stroke]\nmin = 50.0\nmax = 10.0\n");
        assert_eq!(config.tools.stroke, Stroke::default());
    }

    #[test]
    fn magnifier_size_matches_cells_times_zoom() {
        assert_eq!(Magnifier::default().size(), 210);
    }
}
