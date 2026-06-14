use crate::backend::wayland::utils::surface::{SurfaceData, SurfaceVisibility};
use crate::backend::wayland::utils::shm::{create_shm_buffer};

use crate::backend::ScreenOverlay;
use crate::backend::wayland::utils::state::{OverlayRunTime, OverlayState};

use crate::types::{DamageRect, OverlayEvent, Placement, OutputInfo};

use std::os::unix::io::AsFd;
use rustix::event::{poll, PollFd, PollFlags};

// use wayland_cursor::CursorTheme;

use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{Layer},
    zwlr_layer_surface_v1::{self, Anchor},
};

use wayland_protocols_plasma::plasma_virtual_desktop::client::{
    org_kde_plasma_virtual_desktop::{self, OrgKdePlasmaVirtualDesktop},
    org_kde_plasma_virtual_desktop_management::{self, OrgKdePlasmaVirtualDesktopManagement},
};

use wayland_client::{
    Connection, Dispatch, QueueHandle, 
    protocol::wl_output,
};



pub struct KdeOverlay {
    pub connection: wayland_client::Connection,
    runtime: Option<OverlayRunTime>,
}

impl KdeOverlay {
    pub fn new(connection : wayland_client::Connection) -> Self {
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
            state.kde.as_mut().unwrap().pending_desktop_ids.push(desktop_id);
        }
    }
}

impl Dispatch<OrgKdePlasmaVirtualDesktop, String> for OverlayState {
    fn event(
        state: &mut Self,
        _: &OrgKdePlasmaVirtualDesktop,
        event: org_kde_plasma_virtual_desktop::Event,
        desktop_id: &String,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            org_kde_plasma_virtual_desktop::Event::Activated {} => {
                let kde = state.kde.as_mut().unwrap();
                if kde.current_desktop.is_none() {
                    kde.current_desktop = Some(desktop_id.clone());
                } else if kde.current_desktop.as_deref() != Some(desktop_id) {
                    for sd in state.surfaces.values_mut() {
                        sd.set_hidden();
                        state.pending_flush = true;
                    }
                } else {
                    for sd in state.surfaces.values_mut() {
                        sd.set_visible();
                    }
                }
            }
            _ => {}
        }
    }
}











impl ScreenOverlay for KdeOverlay {
    fn present(&mut self, placements: &[Placement]) -> Result<&[OutputInfo], Box<dyn std::error::Error>> {
        self.ensure_runtime()?;
        let rt = self.runtime.as_mut().ok_or("runtime missing")?;
        let qh  = rt.event_queue.handle();

        let compositor = rt.state.compositor.clone().ok_or("no compositor")?;
        let layer_shell = rt.state.layer_shell.clone().ok_or("no layer_shell")?;
        let shm = rt.state.shm.clone().ok_or("no shm")?;

        let outputs_ref = &rt.state.outputs;
        let found_outputs: Vec<wl_output::WlOutput> = placements.iter().map(|placement| {
            outputs_ref
                .iter()
                .find(|o| o.x == placement.position.0 && o.y == placement.position.1)
                .or_else(|| {
                    eprintln!("No screens found with position {:?}, using main screen (0,0)", placement.position);
                    outputs_ref.first()
                })
                .map(|o| o.output.clone())
                .ok_or("no outputs found")
        }).collect::<Result<_, _>>()?;


        for (i, (placement, output)) in placements.iter().zip(found_outputs.iter()).enumerate() {      
            let (w, h) = (placement.size.0 as u32, placement.size.1 as u32);
            
            let surface = compositor.create_surface(&qh, ());

            if let Some(viewporter) = rt.state.viewporter.clone() {
                let viewport = viewporter.get_viewport(&surface, &qh, ());
                viewport.set_destination(w as i32, h as i32);
            }



            let layer_surface = layer_shell.get_layer_surface(
                &surface,
                Some(output),
                Layer::Overlay,
                "lumine-capture".to_string(),
                &qh,
                (),
            );

            layer_surface.set_size(w, h);
            layer_surface.set_anchor(Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right);
            layer_surface.set_keyboard_interactivity(
                zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive,
            );
            layer_surface.set_exclusive_zone(-1);
            surface.commit();

            let frac_scale =  rt.state.frac
                .as_ref()
                .expect("no fractional scale manager")
                .get_fractional_scale(&surface, &qh, ());
             rt.state.frac_scale =Some(frac_scale);

            rt.event_queue.roundtrip( &mut rt.state)?;

            let shm_buffer = create_shm_buffer(&shm, &qh, w, h)?;
            let transparent_pixels = vec![0u8; (w * h * 4) as usize];
            let mut transparent_buffer = create_shm_buffer(&shm, &qh, w, h)?;
            transparent_buffer.write_pixels(&transparent_pixels);                                       
            let empty_region = compositor.create_region(&qh, ());

            surface.damage_buffer(0, 0, w as i32, h as i32);
            surface.commit();

             rt.state.surfaces.insert(i, SurfaceData {
                surface,
                layer_surface,
                shm_buffer,
                transparent_buffer,
                empty_region,
                width: w,
                height: h,
                configured: false,
                visibility: SurfaceVisibility::Visible,
            });
        }

        Ok(&rt.state.outputs)
    }
    fn update_frame(&mut self, monitor_idx: usize, pixels: &[u8], damage: Option<DamageRect>) -> Result<(), Box<dyn std::error::Error>> {
        let rt = self.runtime.as_mut().ok_or("runtime missing")?;
        let sd = rt.state.surfaces.get_mut(&monitor_idx).ok_or("surface not found")?;
        if matches!(sd.visibility, SurfaceVisibility::Hidden) {
            return Ok(());
        }

        if let Some((x, y, w, h)) = damage {
            if x == 0 && y == 0 && w == sd.width && h == sd.height {
                sd.shm_buffer.write_pixels(&pixels);
                sd.surface.attach(Some(&sd.shm_buffer.buffer), 0, 0);
                sd.surface.damage_buffer(0, 0, sd.width as i32, sd.height as i32);
            } else {
                sd.shm_buffer.write_pixels_rect(&pixels, sd.width, (x, y, w, h));
                sd.surface.attach(Some(&sd.shm_buffer.buffer), 0, 0);
                sd.surface.damage_buffer(x as i32, y as i32, w as i32, h as i32);
            }
        } else {
            sd.shm_buffer.write_pixels(&pixels);
            sd.surface.attach(Some(&sd.shm_buffer.buffer), 0, 0);
            sd.surface.damage_buffer(0, 0, sd.width as i32, sd.height as i32);
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
            rt.event_queue.blocking_dispatch(&mut rt.state)?;
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
