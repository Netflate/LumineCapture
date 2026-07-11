// ── compositor, shm and layer_shell (in the future) handlers ────────────────────────────────────
// just a reminder, its for graphical initialization:
// - wl_compositor: creating surfaces
// - wl_shm: deviding memory for transferring frame pixels
// - zwlr_layer_shell_v1: not sure, will be necessary in creating windows

use smithay_client_toolkit::compositor::CompositorHandler;
use smithay_client_toolkit::shell::wlr_layer::{
    LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
};
use smithay_client_toolkit::shm::ShmHandler;
use smithay_client_toolkit::{delegate_compositor, delegate_layer, delegate_shm};

use wayland_client::protocol::{wl_output, wl_surface};
use wayland_client::{Connection, QueueHandle};

use crate::backend::wayland::overlay::state::OverlayState;

// ── compositor ────────────────────────────────────────────────────────────────────────────────
impl CompositorHandler for OverlayState {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}
delegate_compositor!(OverlayState);

// ── shm ──────────────────────────────────────────────────────────────────────────────────────
impl ShmHandler for OverlayState {
    fn shm_state(&mut self) -> &mut smithay_client_toolkit::shm::Shm {
        &mut self.shm
    }
}
delegate_shm!(OverlayState);

// ── layer ─────────────────────────────────────────────────────────────────────────────────────
impl LayerShellHandler for OverlayState {
    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _layer_surface: &LayerSurface,
        _configure: LayerSurfaceConfigure,
        _: u32,
    ) {
    }
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _layer_surface: &LayerSurface) {}
}
delegate_layer!(OverlayState);
