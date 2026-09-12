// One file at ~/.config/LumineCapture/config.toml, loaded once at startup.
// Every field must have a default

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use log::warn;
use serde::{Deserialize, Deserializer};

use crate::ocr::settings::{Device, Mode};
use crate::theme::Rgba;
use crate::types::Finish;

#[derive(Debug, Clone, PartialEq, Deserialize, Default)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub save: Save,
    pub notifications: Notifications,
    pub tools: Tools,
    pub magnifier: Magnifier,
    pub ocr: Ocr,
    pub theme: Theme,
    pub log: Log,
    pub keys: Keys,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct General {
    pub save_always: bool,
    pub dim_alpha: u8,
    pub animation_speed: f32,
    pub double_click: DoubleClick,
}

impl Default for General {
    fn default() -> Self {
        Self { save_always: true, dim_alpha: 140, animation_speed: 1.0, double_click: DoubleClick::Copy }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DoubleClick {
    None,
    Copy,
    Save,
    Pin,
}

impl DoubleClick {
    pub fn finish(self) -> Option<Finish> {
        match self {
            DoubleClick::None => None,
            DoubleClick::Copy => Some(Finish::Copy),
            DoubleClick::Save => Some(Finish::Save),
            DoubleClick::Pin => Some(Finish::Pin),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Save {
    pub directory: String,
    pub month_format: String,
    pub filename_format: String,
}

impl Default for Save {
    fn default() -> Self {
        Self {
            directory: "screenshots".into(),
            month_format: "%Y-%m".into(),
            filename_format: "%Y-%m-%d_%H-%M".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Notifications {
    pub enabled: bool,
}

impl Default for Notifications {
    fn default() -> Self {
        Self { enabled: true }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Tools {
    #[serde(deserialize_with = "deserialize_rgba")]
    pub default_color: Rgba,
    pub default_stroke_width: f32,
    pub default_font_size: f32,
    pub default_bold: bool,
    pub default_italic: bool,
    pub max_recent_colors: usize,
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            default_color: Rgba(255, 255, 255, 255),
            default_stroke_width: 12.0,
            default_font_size: 24.0,
            default_bold: true,
            default_italic: false,
            max_recent_colors: 6,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Magnifier {
    pub zoom: f32,
    pub cells: u32,
    pub offset: f32,
}

impl Default for Magnifier {
    fn default() -> Self {
        Self { zoom: 10.0, cells: 21, offset: 24.0 }
    }
}

impl Magnifier {
    pub fn size(&self) -> u32 {
        (self.cells as f32 * self.zoom) as u32
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Ocr {
    pub mode: Mode,
    pub device: Device,
    pub daemon_idle_secs: u64,
}

impl Default for Ocr {
    fn default() -> Self {
        Self { mode: Mode::OnDemand, device: Device::Auto, daemon_idle_secs: 900 }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Theme {
    #[serde(deserialize_with = "deserialize_rgba")]
    pub accent: Rgba,
    #[serde(deserialize_with = "deserialize_rgba")]
    pub accent_bright: Rgba,
    #[serde(deserialize_with = "deserialize_rgba")]
    pub panel_background: Rgba,
    #[serde(deserialize_with = "deserialize_rgba")]
    pub selection: Rgba,
    pub font_size: f32,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            accent: Rgba(159, 48, 215, 255),
            accent_bright: Rgba(215, 132, 255, 255),
            panel_background: Rgba(17, 17, 27, 250),
            selection: Rgba(100, 150, 255, 110),
            font_size: 14.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Log {
    pub level: Option<String>,
    pub max_file_size_mb: u64,
}

impl Default for Log {
    fn default() -> Self {
        Self { level: None, max_file_size_mb: 1 }
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Deserialize)]
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

fn deserialize_rgba<'de, D: Deserializer<'de>>(d: D) -> Result<Rgba, D::Error> {
    let s = String::deserialize(d)?;
    Rgba::parse_hex(&s)
        .ok_or_else(|| serde::de::Error::custom(format!("invalid color {s:?}, expected #RRGGBB or #RRGGBBAA")))
}

pub const TEMPLATE: &str = "\
[general]
save_always = true
dim_alpha = 140
animation_speed = 1.0
# none | copy | save | pin (double click inside selection finishes the screenshot, the question is what to do with it)
double_click = \"copy\"

[save]
directory = \"screenshots\"
month_format = \"%Y-%m\"
filename_format = \"%Y-%m-%d_%H-%M\"

[notifications]
enabled = true

[tools]
default_color = \"#FFFFFFFF\"
default_stroke_width = 12.0
default_font_size = 24.0
default_bold = true
default_italic = false
max_recent_colors = 6

[magnifier]
zoom = 10.0
cells = 21
offset = 24.0

[ocr]
mode = \"on-demand\"
device = \"auto\"
daemon_idle_secs = 900

[theme]
accent = \"#9F30D7\"
accent_bright = \"#D784FF\"
panel_background = \"#11111BFA\"
selection = \"#6496FF6E\"
font_size = 14.0

[log]
# off | error | warn | info | debug | trace; unset keeps each process's own default
# level = \"info\"
max_file_size_mb = 1

[keys]
copy = [\"Ctrl+C\", \"Return\"]
save = \"Ctrl+S\"
pin = \"Ctrl+P\"
cancel = \"Escape\"
undo = \"Ctrl+Z\"
redo = [\"Ctrl+Shift+Z\", \"Ctrl+Y\"]
select_all = \"Ctrl+A\"
delete = [\"Delete\", \"Backspace\"]
toggle_ui = \"Space\"
size_up = \"]\"
size_down = \"[\"
tool_selection = \"S\"
tool_pick = \"V\"
tool_ocr = \"O\"
tool_eyedropper = \"G\"
tool_text = \"T\"
tool_pen = \"P\"
tool_line = \"D\"
tool_arrow = \"A\"
tool_rectangle = \"R\"
tool_circle = \"C\"
tool_numerated_arrow = \"N\"
move_left = \"Left\"
move_right = \"Right\"
move_up = \"Up\"
move_down = \"Down\"
move_left_fast = \"Shift+Left\"
move_right_fast = \"Shift+Right\"
move_up_fast = \"Shift+Up\"
move_down_fast = \"Shift+Down\"
resize_left = \"Alt+Left\"
resize_right = \"Alt+Right\"
resize_up = \"Alt+Up\"
resize_down = \"Alt+Down\"
resize_left_fast = \"Alt+Shift+Left\"
resize_right_fast = \"Alt+Shift+Right\"
resize_up_fast = \"Alt+Shift+Up\"
resize_down_fast = \"Alt+Shift+Down\"
";

static CONFIG: OnceLock<Config> = OnceLock::new();

fn config_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("LumineCapture").join("config.toml"))
}

fn validate(config: &mut Config) {
    if config.magnifier.cells.is_multiple_of(2) {
        warn!("config: magnifier.cells must be odd, using the default");
        config.magnifier.cells = Magnifier::default().cells;
    }
    if config.general.animation_speed < 0.1 {
        warn!("config: general.animation_speed must be at least 0.1, using the default");
        config.general.animation_speed = General::default().animation_speed;
    }
}

fn write_template(path: &PathBuf) {
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(path, TEMPLATE));
    if let Err(e) = written {
        warn!("config: cannot write {}: {e}", path.display());
    }
}

/// Config overide path set via `--config`. Only the first call takes effect
pub fn init(override_path: Option<PathBuf>) {
    let path = override_path.or_else(config_path);
    let config = match &path {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(text) => match toml::from_str::<Config>(&text) {
                Ok(mut config) => {
                    validate(&mut config);
                    config
                }
                Err(e) => {
                    warn!("config: {} is invalid, using defaults: {e}", path.display());
                    Config::default()
                }
            },
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

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(toml::from_str::<Config>("").unwrap(), Config::default());
    }

    #[test]
    fn the_template_reads_back_as_the_defaults() {
        assert_eq!(toml::from_str::<Config>(TEMPLATE).unwrap(), Config::default());
    }

    #[test]
    fn partial_file_keeps_the_rest_default() {
        let config = toml::from_str::<Config>("[general]\ndim_alpha = 200\n").unwrap();
        assert_eq!(config.general.dim_alpha, 200);
        assert_eq!(config.save, Save::default());
    }

    #[test]
    fn colors_parse_with_and_without_alpha() {
        let config = toml::from_str::<Config>(
            "[theme]\naccent = \"#112233\"\npanel_background = \"#11223344\"\n",
        )
        .unwrap();
        assert_eq!(config.theme.accent, Rgba(0x11, 0x22, 0x33, 255));
        assert_eq!(config.theme.panel_background, Rgba(0x11, 0x22, 0x33, 0x44));
    }

    #[test]
    fn bad_color_is_rejected() {
        assert!(toml::from_str::<Config>("[theme]\naccent = \"not-a-color\"\n").is_err());
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let config = toml::from_str::<Config>("[general]\nfuture_key = 1\n").unwrap();
        assert_eq!(config, Config::default());
    }

    #[test]
    fn double_click_can_be_turned_off() {
        let config = toml::from_str::<Config>("[general]\ndouble_click = \"none\"\n").unwrap();
        assert_eq!(config.general.double_click.finish(), None);
    }

    #[test]
    fn keys_take_a_string_or_a_list() {
        let config = toml::from_str::<Config>(
            "[keys]\nsave = \"Ctrl+Shift+S\"\nredo = [\"Ctrl+Y\", \"\"]\n",
        )
        .unwrap();
        assert_eq!(config.keys.0["save"].chords(), ["Ctrl+Shift+S"]);
        assert_eq!(config.keys.0["redo"].chords(), ["Ctrl+Y"]);
    }

    #[test]
    fn even_magnifier_cells_fall_back_to_the_default() {
        let mut config = toml::from_str::<Config>("[magnifier]\ncells = 20\n").unwrap();
        validate(&mut config);
        assert_eq!(config.magnifier.cells, Magnifier::default().cells);
    }

    #[test]
    fn too_slow_animation_speed_falls_back_to_the_default() {
        let mut config = toml::from_str::<Config>("[general]\nanimation_speed = 0.0\n").unwrap();
        validate(&mut config);
        assert_eq!(config.general.animation_speed, General::default().animation_speed);
    }

    #[test]
    fn magnifier_size_matches_cells_times_zoom() {
        let magnifier = Magnifier::default();
        assert_eq!(magnifier.size(), 210);
    }
}
