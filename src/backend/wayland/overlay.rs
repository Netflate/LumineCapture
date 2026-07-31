// ── Wayland Screen Overlay Implementation ─────────────────────────────────────
//
// this module implements the 'ScreenOverlay' trait for wayland using SCTK
// It is responsible for creating surfaces, mapping them to the correct outputs
// via layer_shell, managing shm pixel buffers, and driving the event loop
use rustix::{
    event::{PollFd, PollFlags, poll},
    time::Timespec,
};
use std::os::unix::io::AsFd;

pub mod state;

use crate::backend::ScreenOverlay;
use crate::types::{DamageRect, Output, OverlayEvent};
use crate::backend::wayland::utils::surface::SurfaceData;

pub struct WaylandOverlay {
    pub connection: wayland_client::Connection,
    runtime: state::OverlayRunTime,
}

impl WaylandOverlay {
    pub fn new(connection: wayland_client::Connection) -> Result<Self, Box<dyn std::error::Error>> {        
        let rt = state::OverlayRunTime::new(&connection)?;

        Ok(Self {
            connection,
            runtime: rt,
        })
    }
}

impl ScreenOverlay for WaylandOverlay {
    fn present(&mut self) -> Result<&[Output], Box<dyn std::error::Error>> {
        let rt = &mut self.runtime;
        let qh = rt.event_queue.handle();

        let outputs_snapshot: Vec<_> = rt.state.outputs.iter()
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
                &qh
            );
            window.set_title("lumine-capture");
            window.set_app_id("lumine-capture");
            window.set_fullscreen(Some(&wl_output));

            // handle fractional scaling calculations for HiDPI setups
            let frac_scale = rt.state.frac.as_ref()
                .expect("no fractional scale manager")
                .get_fractional_scale(&surface, &qh, ());
            rt.state.frac_scale = Some(frac_scale);

            surface.commit();

            rt.state.surfaces.insert(i, SurfaceData {
                surface, window,
                shm_buffer: None,
                transparent_buffer: None,
                width: w,
                height: h,
            });
        }

        while rt.state.surfaces.values().any(|sd| sd.shm_buffer.is_none()) {
            // freezes untill WindowHandler create necessary buffers in utils/compositor_shm_xdg.rs
            // if we continue without waiting compositor response, app will crash 
            rt.event_queue.roundtrip(&mut rt.state)?;
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

        let buffer = sd.shm_buffer.as_mut().ok_or("SHM buffer is not configured yet")?;

        // optimization: only upload and redraw the modified area
        if let Some((x, y, w, h)) = damage {
            if x == 0 && y == 0 && w == sd.width && h == sd.height {
                buffer.write_pixels(pool, pixels);
                sd.surface.attach(Some(buffer.wl_buffer()), 0, 0);
                sd.surface.damage_buffer(0, 0, sd.width as i32, sd.height as i32);
            } else {
                buffer.write_pixels_rect(pool, pixels, sd.width, (x, y, w, h));
                sd.surface.attach(Some(buffer.wl_buffer()), 0, 0);
                sd.surface.damage_buffer(x as i32, y as i32, w as i32, h as i32);
            }
        } else {
            // fallback to full frame redraw
            buffer.write_pixels(pool, pixels);
            sd.surface.attach(Some(buffer.wl_buffer()), 0, 0);
            sd.surface.damage_buffer(0, 0, sd.width as i32, sd.height as i32);
        }
        
        sd.surface.commit();        
        Ok(())
    }

    fn flush(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.runtime.event_queue.flush()?;
        Ok(())
    }

    fn next_event(&mut self, timeout_ms: i32) -> Result<OverlayEvent, Box<dyn std::error::Error>> {
        let rt = &mut self.runtime;

        loop {
            // prepare the wayland connection socket for reading incoming server events
            if let Some(guard) = rt.event_queue.prepare_read() {
                let _ = guard.read();
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
            let timeout = if timeout_ms < 0 {
                None
            } else {
                Some(Timespec {
                    tv_sec: (timeout_ms / 1000) as i64,
                    tv_nsec: ((timeout_ms % 1000) * 1_000_000) as i64,
                })
            };

            poll(&mut fds, timeout.as_ref())?;

            // but if poll timed out without events, emit a Tick event to drive internal ui animations
            if !fds[0].revents().contains(PollFlags::IN) {
                return Ok(OverlayEvent::Tick);
            }
        }
    }

    fn discovered_outputs(&self) -> &[Output] {
        &self.runtime.state.outputs
    }
}
