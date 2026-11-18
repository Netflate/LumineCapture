pub mod notify;
pub mod wayland;

use crate::types::{Capture, CaptureResult, CursorIcon, DamageRect, Output, OverlayEvent};
use crate::utils::format::FileFormat;
use async_trait::async_trait;
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use wayland_client::Connection;

pub const FRAME_FALLBACK: Duration = Duration::from_millis(150);

#[async_trait]
pub trait CaptureMethod: Send + Sync {
    async fn capture_frame(
        &self,
        outputs: &[Output],
    ) -> Result<CaptureResult, Box<dyn std::error::Error>>;

    async fn prepare(&self) -> Result<(), Box<dyn std::error::Error>> {
        Ok(())
    }

    fn captures_overlay(&self) -> bool {
        true
    }

    // im not super sure how it works on cosmic
    // maybe i'll implement it later
    async fn capture_active_window(&self) -> Result<Capture, Box<dyn std::error::Error>> {
        Err("capturing the active window is only supported on KDE Plasma".into())
    }
}

/// Backends that can be picked from the environment alone, without probing the compositor
fn env_method() -> Option<&'static str> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let forced = std::env::var("LUMINE_CAPTURE").unwrap_or_default();
    match forced.as_str() {
        "kde" => Some("kde"),
        "mutter" => Some("mutter"),
        "image-copy" => Some("image-copy"),
        "screencopy" => Some("screencopy"),
        "portal" => Some("portal"),
        // portal is the one asking for permission and monitor choice
        // kinda annoying, and kde protocol / image-copy seems to be faster than through portal
        _ if desktop == "KDE" => Some("kde"),
        _ if desktop.split(':').any(|d| d == "GNOME") => Some("mutter"),
        _ => None,
    }
}

/// we commit full screen window creation before getting the shot itself,
/// this does nothing on other DEs, but on gnome it hides panels
/// so spefically for it we create the window only after
///
/// creating window wihtout fullscreen first, and making it full screen after
/// the shot has no optimization gain.
pub fn window_after_shot() -> bool {
    env_method() == Some("mutter")
}

pub fn initialize_capture(conn: &Connection) -> Box<dyn CaptureMethod> {
    let forced = std::env::var("LUMINE_CAPTURE").unwrap_or_default();
    if !forced.is_empty()
        && !matches!(
            forced.as_str(),
            "kde" | "mutter" | "image-copy" | "screencopy" | "portal"
        )
    {
        warn!(
            "Unknown LUMINE_CAPTURE={forced}, expected kde, mutter, image-copy, screencopy or portal"
        );
    }
    let method = match env_method() {
        Some(method) => method,
        None if wayland::capture::image_copy::supported(conn) => "image-copy",
        None if wayland::capture::screencopy::supported(conn) => "screencopy",
        None => "portal",
    };
    info!("capture backend: {method}");

    match method {
        "kde" => Box::new(wayland::capture::kde::KdeMethod::new()),
        "mutter" => Box::new(wayland::capture::mutter::MutterMethod),
        "image-copy" => Box::new(wayland::capture::image_copy::ImageCopyMethod::new(
            conn.clone(),
        )),
        "screencopy" => Box::new(wayland::capture::screencopy::ScreencopyMethod::new(
            conn.clone(),
        )),
        _ => Box::new(wayland::capture::portal::PortalMethod),
    }
}

pub trait ClipboardProvider {
    fn copy_image_to_clipboard(
        &self,
        png_data: &[u8],
        extra: Option<(FileFormat, &[u8])>,
    ) -> Result<(), Box<dyn std::error::Error>>;
}

pub trait ScreenOverlay: Send {
    fn prefault(&mut self);
    fn present(&mut self, targets: &[usize]) -> Result<(), Box<dyn std::error::Error>>;
    fn stage_frame(
        &mut self,
        monitor_idx: usize,
        pixels: &[u8],
        damage: Option<DamageRect>,
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    /// `None` waits for the compositor indefinitely, a duration wakes up with
    /// `OverlayEvent::Tick` once it runs out.
    fn next_event(
        &mut self,
        timeout: Option<Duration>,
    ) -> Result<OverlayEvent, Box<dyn std::error::Error>>;
    fn discovered_outputs(&self) -> &[Output];
    fn hidpi(&self) -> bool;
    /// true once compositor has shown the last frame
    /// or after FRAME_FALLBACK without an answer, to avoid monitor freezing
    fn ready(&self, monitor_idx: usize) -> bool;
    fn active_output(&mut self) -> Option<usize>;
    fn retain_outputs(&mut self, keep: &[usize]) -> Result<(), Box<dyn std::error::Error>>;
    fn set_background(
        &mut self,
        monitor_idx: usize,
        pixels: &[u8],
        width: u32,
        height: u32,
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn set_cursor(&mut self, icon: CursorIcon);
    /// Caret rect in surface coordinates while a text field is edited, `None` otherwise
    fn set_ime(&mut self, area: Option<(i32, i32, i32, i32)>);
    /// Drops the static snapshot subsurface on `monitor_idx` to expose the live screen beneath.
    fn clear_background(&mut self, monitor_idx: usize) -> Result<(), Box<dyn std::error::Error>>;
    /// lets the pointer fall through `hole` to the window beneath, `None` takes all input back
    fn set_input_hole(&mut self, monitor_idx: usize, hole: Option<InputHole>);
}

/// `keep` stays with the overlay even inside the hole (a panel squeezed into the region)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputHole {
    pub hole: (i32, i32, i32, i32),
    pub keep: Option<(i32, i32, i32, i32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OverlayShell {
    Auto,
    Window,
    Layer,
}

/// Selection of backend for displaying on the screenshot and interface screen
/// GNOME is not supported and has not been tested yet.
///
/// layer_shell works as a system overlay; it will be on top of everything else, so the screenshot
/// cannot be temporarily moved to another desktop or minimized, which seems non-optimal.
/// Therefore, a forced full-screen window is used as default.
///
/// Niri and Umbriel, and likely other scroll managers, place every new full-screen window as a new
/// column in the layout making the user experience clunky since every trigger plays an animation of
/// opening a new app. Therefore, layer_shell is used there instead. It also suits Umbriel's other
/// layouts (dwindle, master, stacking), since a layer surface is never tiled.
///
/// COSMIC gets layer_shell too since a full-screen window there flashes gray while it opens,
/// and there is few focus grabbing issues.
/// Make sense to return to Window if these issues get fixed
///
/// Hyprland doesn't draw what's under a fullscreen window, which breaks scrolling screenshot.
pub fn initialize_overlay(
    conn: Connection,
) -> Result<Box<dyn ScreenOverlay>, Box<dyn std::error::Error>> {
    let layer = match crate::config::get().general.overlay {
        OverlayShell::Auto => on_niri() || on_umbriel() || on_cosmic() || on_hyprland(),
        shell => shell == OverlayShell::Layer,
    };
    info!(
        "overlay surface: {} (XDG_CURRENT_DESKTOP={:?})",
        if layer { "layer" } else { "window" },
        std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default()
    );
    Ok(Box::new(wayland::overlay::WaylandOverlay::new(
        conn, layer,
    )?))
}

fn on_niri() -> bool {
    desktop_is("niri") || std::env::var_os("NIRI_SOCKET").is_some()
}

fn on_umbriel() -> bool {
    desktop_is("umbriel") || std::env::var_os("UMBRIEL_SOCKET").is_some()
}

fn on_cosmic() -> bool {
    desktop_is("COSMIC")
}

fn on_hyprland() -> bool {
    desktop_is("Hyprland") || std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
}

pub fn desktop_is(name: &str) -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.split(':').any(|s| s == name))
}

pub fn initialize_clipboard() -> Box<dyn ClipboardProvider> {
    Box::new(wayland::clipboard::ext_data_control::ClipboardMethod)
}

#[async_trait]
pub trait Notifier {
    async fn notify(
        &self,
        n: &notify::Notification<'_>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

pub fn initialize_notifier() -> Box<dyn Notifier> {
    Box::new(notify::freedesktop::FreedesktopNotifier)
}
