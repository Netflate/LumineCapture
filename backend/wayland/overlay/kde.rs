// GNOME does not support wlr-layer-shell
// SO THIS PART IS WRITTEN, AT LEAST FOR NOW, ONLY FOR
//             KDE WAYLAND

use crate::backend::ScreenOverlay;
pub struct KdeOverlay {
    pub connection: wayland_client::Connection,
}

use crate::types::{CaptureResult, CapturedFrame, SourceType};
use nix::sys::memfd::{MFdFlags, memfd_create};
use nix::unistd::ftruncate;
use std::ffi::CStr;
use std::os::fd::AsFd;

use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, ZwlrLayerSurfaceV1},
};

use wayland_protocols_plasma::plasma_virtual_desktop::client::{
    org_kde_plasma_virtual_desktop::{self, OrgKdePlasmaVirtualDesktop},
    org_kde_plasma_virtual_desktop_management::{self, OrgKdePlasmaVirtualDesktopManagement},
};

use wayland_client::{
    Connection, Dispatch, QueueHandle,
    protocol::{
        wl_buffer, wl_compositor, wl_keyboard, wl_output, wl_region, wl_registry, wl_seat, wl_shm,
        wl_shm_pool, wl_surface,
    },
};

struct ShmBuffer {
    buffer: wl_buffer::WlBuffer,
    _mmap: memmap2::MmapMut,
    _fd: std::os::fd::OwnedFd,
}

struct SurfaceData {
    surface: wl_surface::WlSurface,
    layer_surface: ZwlrLayerSurfaceV1,
    shm_buffer: ShmBuffer,
    transparent_buffer: ShmBuffer,
    empty_region: wl_region::WlRegion, 
    width: u32,
    height: u32,
    configured: bool,
}
struct OutputInfo {
    output: wl_output::WlOutput,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

pub struct OverlayState {
    compositor: Option<wl_compositor::WlCompositor>,
    layer_shell: Option<ZwlrLayerShellV1>,
    shm: Option<wl_shm::WlShm>,
    seat: Option<wl_seat::WlSeat>,
    outputs: Vec<OutputInfo>,
    surfaces: Vec<SurfaceData>,
    current_desktop: Option<String>,
    virtual_desktop_manager: Option<OrgKdePlasmaVirtualDesktopManagement>,
    pending_desktop_ids: Vec<String>,
    pub done: bool,
}

impl Dispatch<wl_registry::WlRegistry, ()> for OverlayState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "wl_compositor" => {
                    state.compositor = Some(registry.bind(name, version, qh, ()));
                }
                "wl_shm" => {
                    state.shm = Some(registry.bind(name, version, qh, ()));
                }
                "wl_output" => {
                    let output = registry.bind(name, version, qh, ());
                    state.outputs.push(OutputInfo {
                        output,
                        x: 0,
                        y: 0,
                        width: 0,
                        height: 0,
                    });
                }
                "zwlr_layer_shell_v1" => {
                    state.layer_shell = Some(registry.bind(name, version, qh, ()));
                }
                "wl_seat" => {
                    state.seat = Some(registry.bind(name, version, qh, ()));
                }
                "org_kde_plasma_virtual_desktop_management" => {
                    let ver = version.min(2);
                    state.virtual_desktop_manager = Some(registry.bind(name, ver, qh, ()));
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_output::WlOutput, ()> for OverlayState {
    fn event(
        state: &mut Self,
        output: &wl_output::WlOutput,
        event: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_output::Event::Geometry { x, y, .. } => {
                if let Some(info) = state.outputs.iter_mut().find(|o| &o.output == output) {
                    info.x = x;
                    info.y = y;
                }
            }
            wl_output::Event::Mode { width, height, .. } => {
                if let Some(info) = state.outputs.iter_mut().find(|o| &o.output == output) {
                    info.width = width;
                    info.height = height;
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for OverlayState {
    fn event(
        _state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities { capabilities } = event {
            let has_keyboard = Into::<u32>::into(capabilities)
                & Into::<u32>::into(wl_seat::Capability::Keyboard)
                != 0;
            if has_keyboard {
                seat.get_keyboard(qh, ());
            }
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for OverlayState {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_keyboard::Event::Key {
            key,
            state: key_state,
            ..
        } = event
        {
            if key == 1 && key_state == wayland_client::WEnum::Value(wl_keyboard::KeyState::Pressed)
            {
                state.done = true;
            }
        }
    }
}

impl Dispatch<wl_compositor::WlCompositor, ()> for OverlayState {
    fn event(
        _: &mut Self,
        _: &wl_compositor::WlCompositor,
        _: wl_compositor::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<wl_shm_pool::WlShmPool, ()> for OverlayState {
    fn event(
        _: &mut Self,
        _: &wl_shm_pool::WlShmPool,
        _: wl_shm_pool::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<wl_buffer::WlBuffer, ()> for OverlayState {
    fn event(
        _: &mut Self,
        _: &wl_buffer::WlBuffer,
        _: wl_buffer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<wl_shm::WlShm, ()> for OverlayState {
    fn event(
        _: &mut Self,
        _: &wl_shm::WlShm,
        _: wl_shm::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<wl_surface::WlSurface, ()> for OverlayState {
    fn event(
        _: &mut Self,
        _: &wl_surface::WlSurface,
        _: wl_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<wl_region::WlRegion, ()> for OverlayState {
    fn event(
        _: &mut Self,
        _: &wl_region::WlRegion,
        _: wl_region::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<ZwlrLayerShellV1, ()> for OverlayState {
    fn event(
        _: &mut Self,
        _: &ZwlrLayerShellV1,
        _: zwlr_layer_shell_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
impl Dispatch<ZwlrLayerSurfaceV1, ()> for OverlayState {
    fn event(
        state: &mut Self,
        layer_surface: &ZwlrLayerSurfaceV1,
        event: zwlr_layer_surface_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zwlr_layer_surface_v1::Event::Configure { serial, .. } = event {
            layer_surface.ack_configure(serial);
            for sd in &mut state.surfaces {
                if !sd.configured {
                    sd.configured = true;
                    sd.surface.attach(Some(&sd.shm_buffer.buffer), 0, 0);
                    sd.surface
                        .damage_buffer(0, 0, sd.width as i32, sd.height as i32);
                    sd.surface.commit();
                }
            }
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
            state.pending_desktop_ids.push(desktop_id);
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
            org_kde_plasma_virtual_desktop::Event::Deactivated {} => {
                if state.current_desktop.as_deref() == Some(desktop_id) {
                    for sd in &state.surfaces {
                        sd.layer_surface.set_keyboard_interactivity(                               // Since there isn't straightforward implementation of hiding 
                            zwlr_layer_surface_v1::KeyboardInteractivity::None,                    // screenshot and its editing overlay, the best solution i've found
                        );                                                                         // is to make it transparent, and:
                        sd.surface.attach(Some(&sd.transparent_buffer.buffer), 0, 0); // * set KeyboardInteractivity::None to not accept keyboard input  
                        sd.surface.damage_buffer(0, 0, sd.width as i32, sd.height as i32);    // * set layer to background, so technically it will be lower than everthing else
                        sd.layer_surface.set_layer(Layer::Background);                             // * set input region - empty, even if its layer background, its still higher than 
                        sd.surface.set_input_region(Some(&sd.empty_region));                       //       user's desktop, so its necessary to not block mouse input 
                        sd.surface.commit();
                    }
                }
            }

            org_kde_plasma_virtual_desktop::Event::Activated {} => {
                if state.current_desktop.is_none() {
                    state.current_desktop = Some(desktop_id.clone());
                } else if state.current_desktop.as_deref() == Some(desktop_id) {
                    for sd in &state.surfaces {
                        sd.layer_surface.set_layer(Layer::Overlay);
                        sd.layer_surface.set_keyboard_interactivity(
                            zwlr_layer_surface_v1::KeyboardInteractivity::Exclusive,               //  setting keyboardInteractivity, layer, and input region back
                        );
                        sd.surface.attach(Some(&sd.shm_buffer.buffer), 0, 0);
                        sd.surface
                            .damage_buffer(0, 0, sd.width as i32, sd.height as i32);
                        sd.surface.set_input_region(None);
                        sd.surface.commit();
                    }
                }
            }
            _ => {}
        }
    }
}
impl ScreenOverlay for KdeOverlay {
    fn show_screenshot(&self, captured: CaptureResult) -> Result<(), Box<dyn std::error::Error>> {
        let t0 = std::time::Instant::now();
        let conn = &self.connection;
        let mut event_queue = conn.new_event_queue();
        let qh = event_queue.handle();
        conn.display().get_registry(&qh, ());

        let mut state = OverlayState {
            compositor: None,
            layer_shell: None,
            shm: None,
            seat: None,
            outputs: Vec::new(),
            surfaces: Vec::new(),
            pending_desktop_ids: Vec::new(),
            done: false,
            virtual_desktop_manager: None,
            current_desktop: None,
        };

        event_queue.roundtrip(&mut state)?;
        event_queue.roundtrip(&mut state)?;

        let pending = std::mem::take(&mut state.pending_desktop_ids);
        if let Some(manager) = &state.virtual_desktop_manager {
            for desktop_id in pending {
                manager.get_virtual_desktop(desktop_id.clone(), &qh, desktop_id);
            }
        }

        event_queue.roundtrip(&mut state)?;
        event_queue.roundtrip(&mut state)?;

        let compositor = state.compositor.as_ref().ok_or("no wl_compositor")?.clone();
        let layer_shell = state.layer_shell.take().ok_or("no zwlr_layer_shell_v1")?;
        let shm = state.shm.take().ok_or("no wl_shm")?;
        let outputs = std::mem::take(&mut state.outputs);

        for stream in &captured.streams {
            let is_window = matches!(stream.source_type, SourceType::Window);

            let stream_pos = stream.position.unwrap_or((0, 0));
            let stream_size = stream.size.unwrap_or((0, 0));

            
            
            let output = if is_window {
                eprintln!("If its a window, then we are going to use pos 0 0 screen for output");           // TOFIX: that's the easiest way, but actually it would be better to 
                match outputs.first() {                                                                     // find out whats the user main monitor, but currently i dont know how to
                    Some(o) => &o.output,                                                      // implement that on wayland
                    None => return Err("no outputs found".into()),
                }
            } else {
                    let matching_output = outputs
                    .iter()
                    .find(|o| o.x == stream_pos.0 && o.y == stream_pos.1);
                match matching_output {
                    Some(o) => &o.output,
                    None => {
                        eprintln!("No screens found with position {:?}, then we are going to use the main screen", stream_pos);
                        match outputs.first() {
                            Some(o) => &o.output,
                            None => return Err("no outputs found".into()),
                        }
                    }
                }
            };

            let (w, h) = if is_window {
                let info = outputs.first().ok_or("no outputs found")?;
                (info.width as u32, info.height as u32)
            } else {
                (stream_size.0 as u32, stream_size.1 as u32)
            };
            println!(" w is {}, and h is {}", w, h);
            let surface = compositor.create_surface(&qh, ());

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

            event_queue.roundtrip(&mut state)?;

            let pixels;
            let pixels_ref  = if is_window {
                pixels = fill_pixels(&captured.frame, w, h);
                &pixels
            } else {
                &captured.frame.pixels
            };

            let shm_buffer = create_shm_buffer(&shm, &qh, w, h, pixels_ref)?;
            let transparent_pixels = vec![0u8; (w * h * 4) as usize];
            let transparent_buffer = create_shm_buffer(&shm, &qh, w, h, &transparent_pixels)?;
            let empty_region = compositor.create_region(&qh, ());

            surface.attach(Some(&shm_buffer.buffer), 0, 0);
            surface.damage_buffer(0, 0, w as i32, h as i32);
            surface.commit();

            state.surfaces.push(SurfaceData {
                surface,
                layer_surface,
                shm_buffer,
                transparent_buffer,
                empty_region,
                width: w,
                height: h,
                configured: false,
            });
        }
        println!(
            "Time from getting pixels to show the image on screen: {}ms",
            t0.elapsed().as_millis()
        );
        event_queue.roundtrip(&mut state)?;

        loop {
            event_queue.blocking_dispatch(&mut state)?;
            if state.done {
                break;
            }
        }

        Ok(())
    }
}

fn create_shm_buffer(
    shm: &wl_shm::WlShm,
    qh: &QueueHandle<OverlayState>,
    width: u32,
    height: u32,
    pixels: &[u8],
) -> Result<ShmBuffer, Box<dyn std::error::Error>> {
    let stride = width * 4;
    let size = (stride * height) as usize;

    let fd = memfd_create(
        CStr::from_bytes_with_nul(b"lumine-shm\0")?,
        MFdFlags::empty(),
    )?;
    ftruncate(&fd, size as i64)?;
    let mut mmap = unsafe { memmap2::MmapMut::map_mut(&fd)? };
    mmap[..size].copy_from_slice(pixels);

    let pool = shm.create_pool(fd.as_fd(), size as i32, qh, ());
    let buffer = pool.create_buffer(0, width as i32, height as i32, stride as i32,
        wl_shm::Format::Argb8888, qh, ());
    pool.destroy();

    Ok(ShmBuffer { buffer, _mmap: mmap, _fd: fd })
}



fn fill_pixels(
    frame: &CapturedFrame,
    screen_width: u32, 
    screen_height: u32) -> Vec<u8> {
    let size = (screen_width * screen_height * 4) as usize;
    let mut buf = vec![0u8; size];

    let offset_x = (screen_width - frame.width) / 2;
    let offset_y = (screen_height - frame.height) / 2;

    for row in 0..frame.height {
        let src_start = (row * frame.width * 4) as usize;
        let src_end = src_start + (frame.width * 4) as usize;

        let dst_start = ((offset_y + row) * screen_width * 4 + offset_x * 4) as usize;
        let dst_end = dst_start + (frame.width * 4) as usize;

        buf[dst_start..dst_end].copy_from_slice(&frame.pixels[src_start..src_end]);
    }

    buf
}