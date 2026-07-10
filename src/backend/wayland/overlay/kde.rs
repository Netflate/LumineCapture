use crate::backend::wayland::utils::shm::create_shm_buffer;
use crate::backend::wayland::utils::surface::SurfaceData;

use crate::backend::ScreenOverlay;
use crate::backend::wayland::utils::state::{OverlayRunTime, OverlayState};

use crate::types::{DamageRect, OutputInfo, OverlayEvent, Placement};

use rustix::{
    event::{PollFd, PollFlags, poll},
    time::Timespec,
};
use std::os::unix::io::AsFd;

use wayland_protocols_plasma::plasma_virtual_desktop::client::{
    org_kde_plasma_virtual_desktop_management::{self, OrgKdePlasmaVirtualDesktopManagement},
};

use wayland_client::{Connection, Dispatch, QueueHandle, protocol::wl_output};

pub struct KdeOverlay {
    pub connection: wayland_client::Connection,
    runtime: Option<OverlayRunTime>,
}

impl KdeOverlay {
    pub fn new(connection: wayland_client::Connection) -> Self {
        Self {
            connection: connection,
            runtime: None,
        }
    }
}

impl Dispatch<OrgKdePlasmaVirtualDesktopManagement, ()> for OverlayState {
    fn event(
        state: &mut Self,
        _: &OrgKdePlasmaVirtualDesktopManagement,
        event: org_kde_plasma_virtual_desktop_management::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let org_kde_plasma_virtual_desktop_management::Event::DesktopCreated {
            desktop_id, ..
        } = event
        {
            state
                .kde
                .as_mut()
                .unwrap()
                .pending_desktop_ids
                .push(desktop_id);
        }
    }
}

impl ScreenOverlay for KdeOverlay {
    fn present(
        &mut self,
        placements: &[Placement],
    ) -> Result<&[OutputInfo], Box<dyn std::error::Error>> {
        self.ensure_runtime()?;
        let rt = self.runtime.as_mut().ok_or("runtime missing")?;
        let qh = rt.event_queue.handle();

        // comparing monitors from outputs with placements, to find the correnct one 
        let found_outputs: Vec<wl_output::WlOutput> = placements
            .iter()
            .map(|placement| {
                // searching in outputs now 
                rt.state
                    .output_state
                    .outputs()
                    .find(|output| {
                        if let Some(info) = rt.state.output_state.info(output) {
                            info.location.0 == placement.position.0 && info.location.1 == placement.position.1
                        } else {
                            false
                        }
                    })
                    .or_else(|| {
                        let current_sctk_outputs: Vec<_> = rt.state.output_state.outputs()
                            .map(|o| rt.state.output_state.info(&o))
                            .collect();
                        eprintln!(
                            "SCTK Direct Check: placement {:?}, current SCTK data: {:?}",
                            placement.position, current_sctk_outputs
                        );
                        
                        rt.state.output_state.outputs().next()
                    })
                    .ok_or("no outputs found in SCTK output_state")
            })
            .collect::<Result<_, _>>()?;

        for (i, (placement, output)) in placements.iter().zip(found_outputs.iter()).enumerate() {
            let (w, h) = (placement.size.0 as u32, placement.size.1 as u32);

            let surface = rt.state.compositor_state.create_surface(&qh);

            if let Some(viewporter) = rt.state.viewporter.clone() {
                let viewport = viewporter.get_viewport(&surface, &qh, ());
                viewport.set_destination(w as i32, h as i32);
            }

            let layer_surface = rt.state.layer_shell.create_layer_surface(
                &qh,
                surface.clone(),
                smithay_client_toolkit::shell::wlr_layer::Layer::Overlay,
                Some("lumine-capture".to_string()),
                Some(output),
            );

            layer_surface.set_size(w, h);
            layer_surface.set_anchor(
                smithay_client_toolkit::shell::wlr_layer::Anchor::TOP 
                | smithay_client_toolkit::shell::wlr_layer::Anchor::BOTTOM 
                | smithay_client_toolkit::shell::wlr_layer::Anchor::LEFT 
                | smithay_client_toolkit::shell::wlr_layer::Anchor::RIGHT
            );
            layer_surface.set_keyboard_interactivity(
                smithay_client_toolkit::shell::wlr_layer::KeyboardInteractivity::Exclusive,
            );
            layer_surface.set_exclusive_zone(-1);
            
            surface.commit();

            let frac_scale = rt
                .state
                .frac
                .as_ref()
                .expect("no fractional scale manager")
                .get_fractional_scale(&surface, &qh, ());
            rt.state.frac_scale = Some(frac_scale);

            rt.event_queue.roundtrip(&mut rt.state)?;

            let wl_shm_raw = rt.state.shm.wl_shm();
            let shm_buffer = create_shm_buffer(wl_shm_raw, &qh, w, h)?;
            
            let transparent_pixels = vec![0u8; (w * h * 4) as usize];
            let mut transparent_buffer = create_shm_buffer(wl_shm_raw, &qh, w, h)?;
            transparent_buffer.write_pixels(&transparent_pixels);
            
            let empty_region = smithay_client_toolkit::compositor::Region::new(&rt.state.compositor_state)?;

            surface.damage_buffer(0, 0, w as i32, h as i32);
            surface.commit();

            rt.state.surfaces.insert(
                i,
                SurfaceData {
                    surface,
                    layer_surface,
                    shm_buffer,
                    transparent_buffer,
                    empty_region,
                    width: w,
                    height: h,
                    configured: false,
                },
            );
        }

        Ok(&rt.state.outputs)
    }

    fn update_frame(
        &mut self,
        monitor_idx: usize,
        pixels: &[u8],
        damage: Option<DamageRect>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let rt = self.runtime.as_mut().ok_or("runtime missing")?;
        let sd = rt
            .state
            .surfaces
            .get_mut(&monitor_idx)
            .ok_or("surface not found")?;

        if let Some((x, y, w, h)) = damage {
            if x == 0 && y == 0 && w == sd.width && h == sd.height {
                sd.shm_buffer.write_pixels(&pixels);
                sd.surface.attach(Some(&sd.shm_buffer.buffer), 0, 0);
                sd.surface
                    .damage_buffer(0, 0, sd.width as i32, sd.height as i32);
            } else {
                sd.shm_buffer
                    .write_pixels_rect(&pixels, sd.width, (x, y, w, h));
                sd.surface.attach(Some(&sd.shm_buffer.buffer), 0, 0);
                sd.surface
                    .damage_buffer(x as i32, y as i32, w as i32, h as i32);
            }
        } else {
            sd.shm_buffer.write_pixels(&pixels);
            sd.surface.attach(Some(&sd.shm_buffer.buffer), 0, 0);
            sd.surface
                .damage_buffer(0, 0, sd.width as i32, sd.height as i32);
        }
        sd.surface.commit();

        rt.event_queue.flush()?;
        Ok(())
    }

    fn next_event(&mut self, timeout_ms: i32) -> Result<OverlayEvent, Box<dyn std::error::Error>> {
        self.ensure_runtime()?;
        let rt = self.runtime.as_mut().ok_or("runtime missing")?;

        loop {
            if let Some(guard) = rt.event_queue.prepare_read() {
                let _ = guard.read();
            }
            rt.event_queue.dispatch_pending(&mut rt.state)?;
            if rt.state.pending_flush {
                rt.state.pending_flush = false;
                rt.event_queue.flush()?;
            }
            if let Some(ev) = rt.state.events.pop_front() {
                // there is no need in all of pointeEvents, only the last one getting send
                // otherwise there will be huge mouse delay
                if let OverlayEvent::PointerMove { .. } = ev {
                    let mut latest_move = ev;
                    while let Some(OverlayEvent::PointerMove { .. }) = rt.state.events.front() {
                        latest_move = rt.state.events.pop_front().unwrap();
                    }
                    return Ok(latest_move);
                }

                // if its not mouse sending immediately
                return Ok(ev);
            }
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

            if !fds[0].revents().contains(PollFlags::IN) {
                return Ok(OverlayEvent::Tick);
            }
        }
    }

    fn ensure_runtime(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.runtime.is_some() {
            return Ok(());
        }

        let rt = OverlayRunTime::new(&self.connection)?;
        self.runtime = Some(rt);
        Ok(())
    }
}
