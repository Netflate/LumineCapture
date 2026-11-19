// ── Wayland surface data container ───────────────────────────────────────────
// utility structure that bundles a raw Wayland surface, its layer_shell
// integration, and its allocated pixel buffers into a single logical window
//
// in 'utils' because it represents a pure data layout used by the
// sub-orchestrator ('overlay.rs'), keeping protocol handlers free of state management
// reminder: each output has its own surface

use crate::backend::wayland::utils::decor::{self, Decor, GRAB, MARGIN};
use crate::backend::wayland::utils::input;
use crate::backend::wayland::utils::shm::ShmBuffer;
use smithay_client_toolkit::compositor::CompositorState;
use std::time::Instant;
use wayland_client::protocol::{wl_output, wl_subsurface, wl_surface};
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

// NOT USED ANYWHERE
// for keeping role alive
pub enum Role {
    Window(smithay_client_toolkit::shell::xdg::window::Window),
    Layer(smithay_client_toolkit::shell::wlr_layer::LayerSurface),
}

/// Scope of pointer events accepted by the overlay surface.
/// Kept on the surface state rather than immediately applied because unmapped
/// layer surfaces reject state commits with invalid_size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InputRegion {
    #[default]
    Full,
    Hole(crate::backend::InputHole),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Inset {
    pub canvas: (u32, u32),
    pub origin: (i32, i32),
}

pub struct SurfaceData {
    pub background: Option<Background>,
    pub role: Role,
    pub surface: wl_surface::WlSurface,
    pub viewport: Option<wp_viewport::WpViewport>,

    pub shm_buffer: Option<ShmBuffer>,
    /// Whether the surface currently has a buffer attached.
    pub mapped: bool,
    /// Pending input region applied upon configure/commit.
    pub input: InputRegion,

    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub frame_sent: Option<Instant>,
    pub inset: Option<Inset>,
    pub output: Option<wl_output::WlOutput>,
    pub states: (bool, bool),
    pub decor: Option<Decor>,
    pub active: bool,
    pub dark: bool,
    pub geometry: Option<(i32, i32, i32, i32)>,
    pub decor_painted: Option<Decor>,
    pub framed: bool,
}

impl SurfaceData {
    pub fn margin(&self) -> i32 {
        if self.decor.is_some() {
            MARGIN as i32
        } else {
            0
        }
    }

    pub fn buffer_size(&self) -> (u32, u32) {
        crate::types::px_size((self.width, self.height), self.scale)
    }

    pub fn shm_size(&self) -> (u32, u32) {
        self.decor.map_or_else(
            || self.buffer_size(),
            |d| decor::buffer_size(self.buffer_size(), &d),
        )
    }

    /// Sets the pending input region on the surface without committing.
    pub fn apply_input(&self, compositor: &CompositorState) {
        match self.input {
            InputRegion::Full => {
                if self.decor.is_some() {
                    if let Ok(region) = smithay_client_toolkit::compositor::Region::new(compositor)
                    {
                        let g = GRAB as i32;
                        let m = self.margin();
                        region.add(
                            m - g,
                            m - g,
                            self.width as i32 + 2 * g,
                            self.height as i32 + 2 * g,
                        );
                        self.surface.set_input_region(Some(region.wl_region()));
                    }
                } else {
                    self.surface.set_input_region(None)
                }
            }
            InputRegion::Hole(mut hole) => {
                let size = (self.width as i32, self.height as i32);
                if let Some(inset) = self.inset {
                    let (dx, dy) = (-inset.origin.0, -inset.origin.1);
                    hole.hole = shift(hole.hole, dx, dy);
                    hole.keep = hole.keep.map(|keep| shift(keep, dx, dy));
                }
                input::apply_input_hole(compositor, &self.surface, size, hole)
            }
        }
    }
}

fn shift((x, y, w, h): (i32, i32, i32, i32), dx: i32, dy: i32) -> (i32, i32, i32, i32) {
    (x + dx, y + dy, w, h)
}
