// ── Wayland surface data container ───────────────────────────────────────────
// utility structure that bundles a raw Wayland surface, its layer_shell
// integration, and its allocated pixel buffers into a single logical window
//
// in 'utils' because it represents a pure data layout used by the
// sub-orchestrator ('overlay.rs'), keeping protocol handlers free of state management
// reminder: each output has its own surface

use crate::backend::wayland::utils::shm::ShmBuffer;
use wayland_client::protocol::{wl_subsurface, wl_surface};
use wayland_protocols::wp::viewporter::client::wp_viewport;

/// screenshot under overlay is with Native resolution, for it to be Rendered 1:1 while the overlay
/// only handles background dimming and UI drawing.
/// otherwise screenshot will be low quality when changed scales
pub struct Background {
    pub subsurface: wl_subsurface::WlSubsurface,
    pub surface: wl_surface::WlSurface,
    pub viewport: Option<wp_viewport::WpViewport>,
    pub buffer: ShmBuffer,
}

impl Drop for Background {
    fn drop(&mut self) {
        if let Some(viewport) = &self.viewport {
            viewport.destroy();
        }
        self.subsurface.destroy();
        self.surface.destroy();
    }
}

pub struct SurfaceData {
    pub background: Option<Background>,
    pub window: smithay_client_toolkit::shell::xdg::window::Window,
    pub surface: wl_surface::WlSurface,

    pub shm_buffer: Option<ShmBuffer>,
    pub transparent_buffer: Option<ShmBuffer>,

    pub width: u32,
    pub height: u32,
}
