use std::path::PathBuf;
use std::sync::OnceLock;

use log::warn;
use serde::{Deserialize, Serialize};
use tiny_skia::Color;

use crate::theme::Rgba;
use crate::types::ToolSettings;
use crate::ui::color_popover::color_to_hex_string;

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct File {
    color: Option<String>,
    stroke_width: Option<f32>,
    font_size: Option<f32>,
    bold: Option<bool>,
    italic: Option<bool>,
    fill: Option<bool>,
    recent_colors: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Saved {
    pub color: Option<Rgba>,
    pub stroke_width: Option<f32>,
    pub font_size: Option<f32>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub fill: Option<bool>,
    pub recent_colors: Vec<Rgba>,
}

fn path() -> Option<PathBuf> {
    Some(dirs::state_dir()?.join("LumineCapture").join("tools.toml"))
}

fn parse_color(s: &str) -> Option<Rgba> {
    let color = Rgba::parse_hex(s);
    if color.is_none() {
        warn!("tools state: invalid color {s:?}, ignoring it");
    }
    color
}

fn hex(color: Color) -> String {
    format!("#{}", color_to_hex_string(color))
}

fn load() -> Saved {
    let Some(path) = path() else {
        return Saved::default();
    };
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Saved::default();
    };
    let file: File = match toml::from_str(&text) {
        Ok(file) => file,
        Err(e) => {
            warn!("tools state: can't read {}: {e}", path.display());
            return Saved::default();
        }
    };

    Saved {
        color: file.color.as_deref().and_then(parse_color),
        stroke_width: file.stroke_width.filter(|w| w.is_finite()),
        font_size: file.font_size.filter(|s| s.is_finite()),
        bold: file.bold,
        italic: file.italic,
        fill: file.fill,
        recent_colors: file.recent_colors.iter().filter_map(|s| parse_color(s)).collect(),
    }
}

pub fn get() -> &'static Saved {
    static SAVED: OnceLock<Saved> = OnceLock::new();
    SAVED.get_or_init(load)
}

pub fn save(settings: &ToolSettings, history: &[Color], initial_history: &[Color]) {
    if *settings == ToolSettings::default() && history == initial_history {
        return;
    }
    let Some(path) = path() else {
        return;
    };

    let file = File {
        color: Some(hex(settings.color)),
        stroke_width: Some(settings.stroke_width),
        font_size: Some(settings.font_size),
        bold: Some(settings.bold),
        italic: Some(settings.italic),
        fill: Some(settings.fill),
        recent_colors: history.iter().map(|&c| hex(c)).collect(),
    };
    let text = match toml::to_string(&file) {
        Ok(text) => text,
        Err(e) => {
            warn!("tools state: can't serialize: {e}");
            return;
        }
    };

    let tmp = path.with_extension("toml.tmp");
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&tmp, text))
        .and_then(|()| std::fs::rename(&tmp, &path));
    if let Err(e) = written {
        warn!("tools state: can't write {}: {e}", path.display());
    }
}
