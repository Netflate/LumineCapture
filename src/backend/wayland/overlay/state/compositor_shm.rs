// ── compositor and shm handlers ────────────────────────────────────
// just a reminder, its for graphical initialization:
// - wl_compositor: creating surfaces
// - wl_shm: deviding memory for transferring frame pixels

use smithay_client_toolkit::compositor::CompositorHandler;

use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shm::ShmHandler;

use wayland_client::protocol::{wl_output, wl_surface};
use wayland_client::{Connection, QueueHandle};

use crate::backend::wayland::overlay::state::OverlayState;
use crate::backend::wayland::utils::shm::create_shm_buffer;
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
        surface: &wl_surface::WlSurface,
        output: &wl_output::WlOutput,
    ) {
        if let Some(probe) = self.probe.as_mut()
            && probe.layer.wl_surface() == surface
        {
            probe.output = Some(output.clone());
        }
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
// ── shm ──────────────────────────────────────────────────────────────────────────────────────
impl ShmHandler for OverlayState {
    fn shm_state(&mut self) -> &mut smithay_client_toolkit::shm::Shm {
        &mut self.shm
    }
}

// with both the window and the layer surface we cannot work exclusively from the Wayland client side
// here we must wait for a response from the compositor itself. Also, we must use its width and height
// otherwise the compositor might reject the window. Only after obtaining this data do we create buffers
// and attach the window to the surface.

impl OverlayState {
    /// shared code between window and layer overlay
    pub fn configure_surface(&mut self, surface: &wl_surface::WlSurface, size: Option<(u32, u32)>) {
        let Some(sd) = self.surfaces.values_mut().find(|sd| &sd.surface == surface) else {
            return;
        };

        // to avoid livelock, if compositor sends configure without specifying sizes
        // we just use ours
        let (w, h) = size.unwrap_or((sd.width, sd.height));

        // if its already configured, and sizes weren't changed
        // there is no point of continuing
        if sd.shm_buffer.is_some() && sd.width == w && sd.height == h {
            return;
        }
        // creating buffers and attaching
        let pool = &mut self.pool;

        let shm_buffer = match create_shm_buffer(pool, w, h) {
            Ok(buffer) => buffer,
            Err(e) => {
                self.configure_error = Some(e.to_string());
                return;
            }
        };

        sd.surface.attach(Some(shm_buffer.wl_buffer()), 0, 0);
        sd.surface.damage_buffer(0, 0, w as i32, h as i32);
        sd.surface.commit();

        sd.shm_buffer = Some(shm_buffer);
        sd.width = w;
        sd.height = h;
    }
}
