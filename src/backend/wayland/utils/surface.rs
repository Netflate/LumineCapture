// ── Wayland surface data container ───────────────────────────────────────────
// utility structure that bundles a raw Wayland surface, its layer_shell
// integration, and its allocated pixel buffers into a single logical window
//
// in 'utils' because it represents a pure data layout used by the
// sub-orchestrator ('overlay.rs'), keeping protocol handlers free of state management
// reminder: each output has its own surface

use crate::backend::wayland::utils::shm::ShmBuffer;
use wayland_client::protocol::wl_surface;

pub struct SurfaceData {
    pub window: smithay_client_toolkit::shell::xdg::window::Window,
    pub surface: wl_surface::WlSurface,
    
    pub shm_buffer: Option<ShmBuffer>,
    pub transparent_buffer: Option<ShmBuffer>,

    pub width: u32,
    pub height: u32,
}
