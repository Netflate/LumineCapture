// ── compositor and shm handlers ────────────────────────────────────
// just a reminder, its for graphical initialization:
// - wl_compositor: creating surfaces
// - wl_shm: deviding memory for transferring frame pixels

use smithay_client_toolkit::compositor::CompositorHandler;

use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shm::ShmHandler;

use std::collections::VecDeque;

use wayland_client::protocol::{wl_output, wl_surface};
use wayland_client::{Connection, QueueHandle};

use crate::backend::wayland::overlay::state::OverlayState;
use crate::backend::wayland::utils::shm::create_shm_buffer;
use crate::backend::wayland::utils::surface::Role;
use crate::types::OverlayEvent;
// ── compositor ────────────────────────────────────────────────────────────────────────────────
impl CompositorHandler for OverlayState {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        factor: i32,
    ) {
        if self.frac.is_none() {
            self.rescale_window(factor as f32);
        }
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
        surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
        if let Some((&monitor_idx, sd)) = self
            .surfaces
            .iter_mut()
            .find(|(_, sd)| &sd.surface == surface)
        {
            sd.frame_sent = None;
            // only when the window is shown, with the first commit Hyprland opens a floating window at its min size
            if let (Some(min), Role::Window(window)) = (self.min_size.take(), &sd.role) {
                window.set_min_size(Some(min));
                sd.surface.commit();
            }
            self.events
                .push_back(OverlayEvent::FrameDone { monitor_idx });
        }
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
        let Some((&monitor_idx, sd)) = self
            .surfaces
            .iter_mut()
            .find(|(_, sd)| &sd.surface == surface)
        else {
            return;
        };

        // to avoid livelock, if compositor sends configure without specifying sizes
        // we just use ours
        let (w, h) = size.unwrap_or((sd.width, sd.height));

        // drag sends hundreds of configures: only the last size gets a buffer, in `apply_resize`
        if self.toplevel && sd.shm_buffer.is_some() {
            self.resize = ((sd.width, sd.height) != (w, h)).then_some((monitor_idx, (w, h)));
            return;
        }
        // if its already configured, and sizes weren't changed
        // there is no point of continuing
        if sd.shm_buffer.is_some() && sd.width == w && sd.height == h {
            return;
        }
        // creating buffers and attaching
        let pool = &mut self.pool;
        let (bw, bh) = crate::types::px_size((w, h), sd.scale);

        let mut shm_buffer = match create_shm_buffer(pool, bw, bh) {
            Ok(buffer) => buffer,
            Err(e) => {
                self.configure_error = Some(e.to_string());
                return;
            }
        };

        // app draws the new size before anything is attached,
        // since a blank buffer would flash on every resize
        if self.toplevel {
            if let Some(viewport) = &sd.viewport {
                viewport.set_destination(w as i32, h as i32);
            }
            sd.shm_buffer = Some(shm_buffer);
            sd.width = w;
            sd.height = h;
            sd.mapped = true;
            self.events.push_back(OverlayEvent::Resized {
                monitor_idx,
                size: (w, h),
                scale: sd.scale,
            });
            return;
        }

        // apply configured input region with the first frame after mapping
        sd.mapped = true;
        sd.apply_input(&self.compositor_state);
        // viewport maps buffer, which is is in physical pixels, back to the logical size
        if let Some(viewport) = &sd.viewport {
            viewport.set_destination(w as i32, h as i32);
        }
        sd.surface.attach(Some(shm_buffer.wl_buffer()), 0, 0);
        sd.surface.damage_buffer(0, 0, bw as i32, bh as i32);
        sd.surface.commit();
        shm_buffer.fresh = false;

        sd.shm_buffer = Some(shm_buffer);
        sd.width = w;
        sd.height = h;
    }

    pub fn apply_resize(&mut self) {
        let Some((monitor_idx, (w, h))) = self.resize.take() else {
            return;
        };
        let Some(sd) = self.surfaces.get_mut(&monitor_idx) else {
            return;
        };
        let (bw, bh) = crate::types::px_size((w, h), sd.scale);
        let shm_buffer = match create_shm_buffer(&mut self.pool, bw, bh) {
            Ok(buffer) => buffer,
            Err(e) => {
                self.configure_error = Some(e.to_string());
                return;
            }
        };
        if let Some(viewport) = &sd.viewport {
            viewport.set_destination(w as i32, h as i32);
        }
        sd.shm_buffer = Some(shm_buffer);
        sd.width = w;
        sd.height = h;
        resize_first(
            &mut self.events,
            OverlayEvent::Resized {
                monitor_idx,
                size: (w, h),
                scale: sd.scale,
            },
        );
    }

    pub fn rescale_window(&mut self, scale: f32) {
        if !self.toplevel || self.viewporter.is_none() || scale <= 0.0 {
            return;
        }
        let Some(sd) = self.surfaces.values_mut().next() else {
            return;
        };
        if sd.scale == scale {
            return;
        }
        sd.scale = scale;
        // before the first configure the scale is just picked up by it
        if sd.shm_buffer.take().is_some() {
            let surface = sd.surface.clone();
            let latest = self.resize.take().map(|(_, size)| size);
            self.configure_surface(&surface, latest);
        }
    }
}

/// app must meet only the newest `Resized`
/// an older one would draw a size that has no buffer
fn resize_first(events: &mut VecDeque<OverlayEvent>, resized: OverlayEvent) {
    if let OverlayEvent::Resized { monitor_idx, .. } = resized {
        events.retain(
            |e| !matches!(e, OverlayEvent::Resized { monitor_idx: m, .. } if *m == monitor_idx),
        );
    }
    events.push_front(resized);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resized(monitor_idx: usize, w: u32, scale: f32) -> OverlayEvent {
        OverlayEvent::Resized {
            monitor_idx,
            size: (w, 100),
            scale,
        }
    }

    #[test]
    fn newest_resize_replaces_queued_ones_and_goes_first() {
        let mut events = VecDeque::from([
            OverlayEvent::PointerLeave,
            resized(0, 300, 1.0),
            resized(1, 500, 1.0),
        ]);
        resize_first(&mut events, resized(0, 400, 2.0));
        let order: Vec<_> = events
            .iter()
            .map(|e| match e {
                OverlayEvent::Resized {
                    monitor_idx,
                    size,
                    scale,
                } => Some((*monitor_idx, size.0, *scale)),
                _ => None,
            })
            .collect();
        assert_eq!(order, [Some((0, 400, 2.0)), None, Some((1, 500, 1.0))]);
    }
}
