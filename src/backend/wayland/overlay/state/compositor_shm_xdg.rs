// ── compositor, shm and layer_shell (in the future) handlers ────────────────────────────────────
// just a reminder, its for graphical initialization:
// - wl_compositor: creating surfaces
// - wl_shm: deviding memory for transferring frame pixels
// - xdg_shell: used for managing desktop windows (doesn't work on gnome)

use smithay_client_toolkit::compositor::CompositorHandler;
use smithay_client_toolkit::shell::xdg::window::{Window, WindowConfigure, WindowHandler};

use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    LayerShellHandler, LayerSurface, LayerSurfaceConfigure,
};
use smithay_client_toolkit::shm::ShmHandler;

use wayland_client::protocol::{wl_output, wl_surface};
use wayland_client::{Connection, QueueHandle};

use crate::backend::wayland::overlay::state::OverlayState;
use crate::backend::wayland::utils::shm::create_shm_buffer;
use crate::types::OverlayEvent;
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

// ── xdg ───────────────────────────────────────────────────────────────────────────────────────
// unlike with the overlay, in the window display method we cannot work exclusively from the Wayland client side
// here we must wait for a response from the compositor itself. Also, we must use its width and height
// otherwise the compositor might reject the window. Only after obtaining this data do we create buffers
// and attach the window to the surface.

impl WindowHandler for OverlayState {
    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        // getting surface where this window is attached to
        let Some(sd) = self.surfaces.values_mut().find(|sd| &sd.window == window) else {
            return;
        };

        // to avoid livelock, if compositor sends configure without specifying sizes
        // we just use ours
        let (w, h) = match configure.new_size {
            (Some(width), Some(height)) => (width.get(), height.get()),
            _ => (sd.width, sd.height),
        };

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

    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.events.push_back(OverlayEvent::EscapePressed);
    }
}

impl LayerShellHandler for OverlayState {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {}

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        _: LayerSurfaceConfigure,
        _: u32,
    ) {
        let Some(probe) = self.probe.as_mut() else {
            return;
        };
        if &probe.layer != layer || probe.buffer.is_some() {
            return;
        }
        let Ok(mut buffer) = create_shm_buffer(&mut self.pool, 1, 1) else {
            return;
        };
        buffer.write_pixels(&mut self.pool, &[0; 4]);
        layer.wl_surface().attach(Some(buffer.wl_buffer()), 0, 0);
        layer.wl_surface().damage_buffer(0, 0, 1, 1);
        layer.commit();
        probe.buffer = Some(buffer);
    }
}
