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
    pub surface: wl_surface::WlSurface,

    pub layer_surface: smithay_client_toolkit::shell::wlr_layer::LayerSurface,
    pub shm_buffer: ShmBuffer,
    pub transparent_buffer: ShmBuffer,

    pub width: u32,
    pub height: u32,
}
