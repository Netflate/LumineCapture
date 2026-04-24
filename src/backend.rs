pub mod wayland;

use crate::{types::{CaptureResult, CapturedFrame, OverlayEvent, Placement}};
use async_trait::async_trait;
use wayland_client::Connection;



#[async_trait]
pub trait CaptureMethod {
	async fn capture_frame(&self) -> Result<CaptureResult, Box<dyn std::error::Error>>;
}

#[async_trait]
pub trait ScreenOverlay {
	// height and width are not currently used, but fixing scaling related bugs would presumably require them
	fn present(&mut self, width:u32, height:u32, placements: &[Placement]) -> Result<(), Box<dyn std::error::Error>>; 
	fn update_frame(&mut self, pixels: &[u8]) -> Result<(), Box<dyn std::error::Error>>;
	fn next_event(&mut self) -> Result<OverlayEvent, Box<dyn std::error::Error>>;
	fn ensure_runtime(&mut self) ->Result<(), Box<dyn std::error::Error>>;
	//fn show_overlay(&self); todo
	//fn update_pixels(&self); todo
}
#[async_trait]
pub trait ClipboardProvider {
	fn copy_image_to_clipboard(&self, png_data: Vec<u8>,) -> Result<(), Box<dyn std::error::Error>>;
}

pub fn initialize_capture() -> Box<dyn CaptureMethod> {
	let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();

	match desktop.as_str() {
		"KDE" | "GNOME" => Box::new(wayland::portal::PortalMethod) as Box<dyn CaptureMethod>,
		_ => Box::new(wayland::portal::PortalMethod) as Box<dyn CaptureMethod>, // TODO : For now it needs to stop the app from running
	}
}

pub fn initialize_overlay(conn: Connection) -> Box<dyn ScreenOverlay> {
	let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();

	match desktop.as_str() {
		"KDE" => Box::new(wayland::overlay::kde::KdeOverlay::new(conn)) as Box<dyn ScreenOverlay>,
		_ => Box::new(wayland::overlay::kde::KdeOverlay::new(conn)) as Box<dyn ScreenOverlay>, // TODO : For now it needs to stop the app from running
	}
}

pub fn initialize_clipboard(conn: Connection) -> Box<dyn ClipboardProvider> {
	let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();

	match desktop.as_str() {
		"KDE" | "GNOME" => Box::new(wayland::clipboard::ClipboardMethod { connection: conn }) as Box<dyn ClipboardProvider>,
		_ => Box::new(wayland::clipboard::ClipboardMethod { connection: conn }) as Box<dyn ClipboardProvider>, // TODO : For now it needs to stop the app from running
	}
}

