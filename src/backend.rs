pub mod notify;
pub mod wayland;

use crate::types::{Capture, CaptureResult, CursorIcon, DamageRect, Output, OverlayEvent};
use std::time::Duration;
use async_trait::async_trait;
use log::{info, warn};
use wayland_client::Connection;

#[async_trait]
pub trait CaptureMethod: Send + Sync {
    async fn capture_frame(
        &self,
        outputs: &[Output],
    ) -> Result<CaptureResult, Box<dyn std::error::Error>>;

    async fn capture_active_window(&self) -> Result<Capture, Box<dyn std::error::Error>> {
        Err("capturing the active window is only supported on KDE Plasma".into())
    }
}

pub fn initialize_capture(conn: &Connection) -> Box<dyn CaptureMethod> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    let forced = std::env::var("LUMINE_CAPTURE").unwrap_or_default();
    let method = match forced.as_str() {
        "kde" | "image-copy" | "portal" => forced.as_str(),
        other => {
            if !other.is_empty() {
                warn!("Unknown LUMINE_CAPTURE={other}, expected kde, image-copy or portal");
            }
            // portal is the one asking for permission and monitor choice
            // kinda annoying, and kde protocol / image-copy seems to be faster than through portal
            if desktop == "KDE" {
                "kde"
            } else if wayland::capture::image_copy::supported(conn) {
                "image-copy"
            } else {
                "portal"
            }
        }
    };
    info!("capture backend: {method}");

    match method {
        "kde" => Box::new(wayland::capture::kde::KdeMethod::new()),
        "image-copy" => Box::new(wayland::capture::image_copy::ImageCopyMethod::new(
            conn.clone(),
        )),
        _ => Box::new(wayland::capture::portal::PortalMethod),
    }
}

pub trait ClipboardProvider {
    fn copy_image_to_clipboard(&self, png_data: Vec<u8>) -> Result<(), Box<dyn std::error::Error>>;
}

pub trait ScreenOverlay: Send {
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
}

pub fn initialize_overlay(
    conn: Connection,
) -> Result<Box<dyn ScreenOverlay>, Box<dyn std::error::Error>> {
    // TODO: won't work on gnome anyways :p will be implemented in the future
    Ok(Box::new(wayland::overlay::WaylandOverlay::new(conn)?))
}

pub fn initialize_clipboard() -> Box<dyn ClipboardProvider> {
    Box::new(wayland::clipboard::ext_data_control::ClipboardMethod)
}

#[async_trait]
pub trait Notifier {
    async fn notify(
        &self,
        n: &notify::Notification,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

pub fn initialize_notifier() -> Box<dyn Notifier> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    Box::new(notify::freedesktop::FreedesktopNotifier { kde: desktop == "KDE" })
}
