use crate::backend::wayland::utils::shm::ShmBuffer;
use wayland_client::protocol::wl_surface;

pub struct SurfaceData {
    pub surface:    wl_surface::WlSurface,
    pub layer_surface:  smithay_client_toolkit::shell::wlr_layer::LayerSurface,
    pub shm_buffer: ShmBuffer,
    pub transparent_buffer: ShmBuffer,
    pub empty_region: smithay_client_toolkit::compositor::Region,
    pub width: u32,
    pub height: u32,
    pub configured: bool,
}

