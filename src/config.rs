use std::path::PathBuf;
use std::sync::OnceLock;

use log::warn;
use serde::{Deserialize, Deserializer};

use crate::ocr::settings::{Device, Mode};
use crate::theme::Rgba;

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
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct General {
    pub save_always: bool,
    pub dim_alpha: u8,
    pub animation_speed: f32,
}

impl Default for General {
    fn default() -> Self {
        Self { save_always: true, dim_alpha: 140, animation_speed: 1.0 }
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
# level = \"info\"
max_file_size_mb = 1
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
