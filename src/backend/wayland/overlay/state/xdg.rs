// ── xdg ───────────────────────────────────────────────────────────────────────────────────────
// - xdg_shell: used for managing desktop windows (doesn't work on gnome)

use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::xdg::window::{
    DecorationMode, Window, WindowConfigure, WindowHandler,
};

use wayland_client::{Connection, QueueHandle};

use crate::backend::wayland::overlay::state::OverlayState;
use crate::backend::wayland::utils::decor::Decor;
use crate::types::{OverlayEvent, WindowFrame};

impl WindowHandler for OverlayState {
    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        window: &Window,
        configure: WindowConfigure,
        _serial: u32,
    ) {
        let size = match configure.new_size {
            (Some(width), Some(height)) => Some((width.get(), height.get())),
            _ => None,
        };
        let found = self
            .surfaces
            .iter_mut()
            .find(|(_, sd)| &sd.surface == window.wl_surface());
        if let Some((idx, sd)) = found {
            sd.states = (configure.is_maximized(), configure.is_fullscreen());
            if self.toplevel {
                self.frame =
                    (configure.decoration_mode == DecorationMode::Client).then_some(WindowFrame {
                        resizable: !(configure.is_maximized()
                            || configure.is_fullscreen()
                            || configure.is_tiled_left()
                            || configure.is_tiled_right()
                            || configure.is_tiled_top()
                            || configure.is_tiled_bottom()),
                    });
                let old_decor = sd.decor;
                sd.active = configure.is_activated();
                sd.decor = self
                    .frame
                    .filter(|f| f.resizable)
                    .map(|_| Decor::new(sd.scale, sd.active, sd.dark));
                if old_decor.is_some() == sd.decor.is_some() && old_decor != sd.decor {
                    self.events
                        .push_back(OverlayEvent::Redraw { monitor_idx: *idx });
                }
            }
        }
        self.configure_surface(window.wl_surface(), size);
    }

    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.events.push_back(OverlayEvent::EscapePressed);
    }
}
