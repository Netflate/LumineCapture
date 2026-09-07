pub mod notify;
pub mod wayland;

use crate::types::{CaptureResult, CursorIcon, DamageRect, Output, OverlayEvent};
use async_trait::async_trait;
use wayland_client::Connection;

#[async_trait]
pub trait CaptureMethod {
    async fn capture_frame(
        &self,
        outputs: &[Output],
    ) -> Result<CaptureResult, Box<dyn std::error::Error>>;
}

pub fn initialize_capture() -> Box<dyn CaptureMethod> {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    match desktop.as_str() {
        "KDE" => Box::new(wayland::capture::kde::KdeMethod::new()),
        _ => Box::new(wayland::capture::portal::PortalMethod),
    }
}

pub trait ClipboardProvider {
    fn copy_image_to_clipboard(&self, png_data: Vec<u8>) -> Result<(), Box<dyn std::error::Error>>;
}

pub trait ScreenOverlay: Send {
    fn present(&mut self) -> Result<&[Output], Box<dyn std::error::Error>>;
    fn stage_frame(
        &mut self,
        monitor_idx: usize,
        pixels: &[u8],
        damage: Option<DamageRect>,
    ) -> Result<(), Box<dyn std::error::Error>>;
    fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>>;
    fn next_event(&mut self, timeout_ms: i32) -> Result<OverlayEvent, Box<dyn std::error::Error>>;
    fn discovered_outputs(&self) -> &[Output];
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
