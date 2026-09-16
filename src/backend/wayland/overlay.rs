// ── Wayland Screen Overlay Implementation ─────────────────────────────────────
//
// this module implements the 'ScreenOverlay' trait for wayland using SCTK
// It is responsible for creating surfaces, mapping them to the correct outputs
// via layer_shell, managing shm pixel buffers, and driving the event loop
use rustix::{
    event::{PollFd, PollFlags, poll},
    time::Timespec,
};
use std::io::ErrorKind;
use std::os::unix::io::AsFd;
use std::time::{Duration, Instant};
use wayland_client::backend::WaylandError;

pub mod state;

use crate::backend::ScreenOverlay;
use crate::backend::wayland::utils::shm::create_shm_buffer;
use crate::backend::wayland::utils::surface::{Background, SurfaceData};
use smithay_client_toolkit::compositor::Region;
use crate::types::{CursorIcon, DamageRect, Output, OverlayEvent};

const CONFIGURE_TIMEOUT: Duration = Duration::from_secs(5);

pub struct WaylandOverlay {
    runtime: state::OverlayRunTime,
}

impl WaylandOverlay {
    pub fn new(connection: wayland_client::Connection) -> Result<Self, Box<dyn std::error::Error>> {
        let rt = state::OverlayRunTime::new(&connection)?;

        Ok(Self { runtime: rt })
    }
}

impl ScreenOverlay for WaylandOverlay {
    fn present(&mut self) -> Result<&[Output], Box<dyn std::error::Error>> {
        let rt = &mut self.runtime;
        let qh = rt.event_queue.handle();

        if rt.state.outputs.is_empty() {
            return Err("compositor reported no outputs".into());
        }
        let outputs_snapshot: Vec<_> = rt
            .state
            .outputs
            .iter()
            .map(|o| (o.wl_output.clone(), o.info.logical_size.unwrap_or((0, 0))))
            .collect();

        // for each output
        for (i, (wl_output, (w, h))) in outputs_snapshot.into_iter().enumerate() {
            let (w, h) = (w as u32, h as u32);

            let surface = rt.state.compositor_state.create_surface(&qh);

            // scale the surface layout if the viewporter protocol is available
            if let Some(viewporter) = rt.state.viewporter.clone() {
                let viewport = viewporter.get_viewport(&surface, &qh, ());
                viewport.set_destination(w as i32, h as i32);
            }

            // ── creating a forced full screen window, with screenshot itself and etc ────────────────────
            let window = rt.state.xdg_shell.create_window(
                surface.clone(),
                smithay_client_toolkit::shell::xdg::window::WindowDecorations::None,
                &qh,
            );
            window.set_title("lumine-capture");
            window.set_app_id("lumine-capture");
            window.set_fullscreen(Some(&wl_output));

            // handle fractional scaling calculations for HiDPI setups
            if let Some(frac) = rt.state.frac.as_ref() {
                rt.state.frac_scale = Some(frac.get_fractional_scale(&surface, &qh, ()));
            }

            surface.commit();

            rt.state.surfaces.insert(
                i,
                SurfaceData {
                    background: None,
                    surface,
                    window,
                    shm_buffer: None,
                    transparent_buffer: None,
                    width: w,
                    height: h,
                },
            );
        }

        let deadline = Instant::now() + CONFIGURE_TIMEOUT;
        while rt.state.surfaces.values().any(|sd| sd.shm_buffer.is_none()) {
            // freezes untill WindowHandler create necessary buffers in utils/compositor_shm_xdg.rs
            // if we continue without waiting compositor response, app will crash
            rt.event_queue.roundtrip(&mut rt.state)?;
            if let Some(e) = rt.state.configure_error.take() {
                return Err(format!("can't allocate overlay buffer: {e}").into());
            }
            if Instant::now() > deadline {
                return Err("compositor didn't configure the overlay windows".into());
            }
        }

        Ok(&rt.state.outputs)
    }

    fn stage_frame(
        &mut self,
        monitor_idx: usize,
        pixels: &[u8],
        damage: Option<DamageRect>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let rt = &mut self.runtime;
        let sd = rt
            .state
            .surfaces
            .get_mut(&monitor_idx)
            .ok_or("surface not found")?;

        let pool = &mut rt.state.pool;

        let buffer = sd
            .shm_buffer
            .as_mut()
            .ok_or("SHM buffer is not configured yet")?;

        // optimization: only upload and redraw the modified area
        if let Some((x, y, w, h)) = damage {
            if x == 0 && y == 0 && w == sd.width && h == sd.height {
                buffer.write_pixels(pool, pixels);
                sd.surface.attach(Some(buffer.wl_buffer()), 0, 0);
                sd.surface
                    .damage_buffer(0, 0, sd.width as i32, sd.height as i32);
            } else {
                buffer.write_pixels_rect(pool, pixels, sd.width, (x, y, w, h));
                sd.surface.attach(Some(buffer.wl_buffer()), 0, 0);
                sd.surface
                    .damage_buffer(x as i32, y as i32, w as i32, h as i32);
            }
        } else {
            // fallback to full frame redraw
            buffer.write_pixels(pool, pixels);
            sd.surface.attach(Some(buffer.wl_buffer()), 0, 0);
            sd.surface
                .damage_buffer(0, 0, sd.width as i32, sd.height as i32);
        }

        sd.surface.commit();
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.runtime.event_queue.flush()?;
        Ok(())
    }

    fn next_event(
        &mut self,
        timeout: Option<Duration>,
    ) -> Result<OverlayEvent, Box<dyn std::error::Error>> {
        let rt = &mut self.runtime;

        loop {
            // prepare the wayland connection socket for reading incoming server events
            if let Some(guard) = rt.event_queue.prepare_read() {
                match guard.read() {
                    Ok(_) => {}
                    Err(WaylandError::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
                    Err(e) => return Err(e.into()),
                }
            }
            rt.event_queue.dispatch_pending(&mut rt.state)?;

            if rt.state.pending_flush {
                rt.state.pending_flush = false;
                rt.event_queue.flush()?;
            }

            if let Some(ev) = rt.state.events.pop_front() {
                // optimization: coalesce sequential mouse move events.
                // we only care about the latest mouse coordinate in the event queue per frame
                if let OverlayEvent::PointerMove { .. } = ev {
                    let mut latest_move = ev;
                    while let Some(OverlayEvent::PointerMove { .. }) = rt.state.events.front() {
                        latest_move = rt.state.events.pop_front().unwrap();
                    }
                    return Ok(latest_move);
                }
                return Ok(ev);
            }

            // block and wait until the Wayland connection file descriptor has data available (poll)
            let fd = rt.event_queue.as_fd();
            let mut fds = [PollFd::new(&fd, PollFlags::IN)];
            let deadline = timeout.map(|t| Timespec {
                tv_sec: t.as_secs() as i64,
                tv_nsec: t.subsec_nanos() as i64,
            });

            poll(&mut fds, deadline.as_ref())?;

            if fds[0].revents().intersects(PollFlags::ERR | PollFlags::HUP) {
                return Err("Wayland connection closed".into());
            }
            // but if poll timed out without events, emit a Tick event to drive internal ui animations
            if !fds[0].revents().contains(PollFlags::IN) {
                return Ok(OverlayEvent::Tick);
            }
        }
    }

    fn discovered_outputs(&self) -> &[Output] {
        &self.runtime.state.outputs
    }

    fn retain_outputs(&mut self, keep: &[usize]) -> Result<(), Box<dyn std::error::Error>> {
        let rt = &mut self.runtime;
        let remap = |idx: usize| keep.iter().position(|&k| k == idx);

        for (idx, sd) in std::mem::take(&mut rt.state.surfaces) {
            if let Some(new_idx) = remap(idx) {
                rt.state.surfaces.insert(new_idx, sd);
            }
        }

        rt.state.pointer_surface_idx = rt.state.pointer_surface_idx.and_then(remap);
        rt.state.events.retain_mut(|ev| match ev {
            OverlayEvent::PointerMove { monitor_idx, .. }
            | OverlayEvent::Focus { monitor_idx } => match remap(*monitor_idx) {
                Some(idx) => {
                    *monitor_idx = idx;
                    true
                }
                None => false,
            },
            _ => true,
        });

        rt.event_queue.flush()?;
        Ok(())
    }

    fn set_background(
        &mut self,
        monitor_idx: usize,
        pixels: &[u8],
        width: u32,
        height: u32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let rt = &mut self.runtime;
        let qh = rt.event_queue.handle();
        let state = &mut rt.state;

        let subcompositor = state
            .subcompositor
            .as_ref()
            .ok_or("compositor has no wl_subcompositor")?;
        let sd = state
            .surfaces
            .get_mut(&monitor_idx)
            .ok_or("surface not found")?;
        if state.viewporter.is_none() && (width, height) != (sd.width, sd.height) {
            return Err("can't scale the background without wp_viewporter".into());
        }

        let (subsurface, surface) = subcompositor.create_subsurface(sd.surface.clone(), &qh);
        subsurface.place_below(&sd.surface);
        let viewport = state.viewporter.as_ref().map(|viewporter| {
            let viewport = viewporter.get_viewport(&surface, &qh, ());
            viewport.set_destination(sd.width as i32, sd.height as i32);
            viewport
        });
        if let Ok(region) = Region::new(&state.compositor_state) {
            surface.set_input_region(Some(region.wl_region()));
        }

        let mut buffer = create_shm_buffer(&mut state.pool, width, height)?;
        buffer.write_pixels(&mut state.pool, pixels);
        surface.attach(Some(buffer.wl_buffer()), 0, 0);
        surface.damage_buffer(0, 0, width as i32, height as i32);
        surface.commit();

        sd.background = Some(Background {
            subsurface,
            surface,
            viewport,
            buffer,
        });
        Ok(())
    }

    fn set_cursor(&mut self, icon: CursorIcon) {
        self.runtime.state.apply_cursor(icon);
    }
}
