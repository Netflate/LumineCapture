// GNOME does not support wlr-layer-shell
// SO THIS PART IS WRITTEN, AT LEAST FOR NOW, ONLY FOR
//             KDE WAYLAND

// Known necessary TOFIX bugs before MVP:
// --> Applied scaling causes a critical issue, since the calculated pixels from the monitor do not match the screenshot pixels


use crate::backend::ScreenOverlay;
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

use crate::types::{CapturedFrame, OverlayEvent, Placement};
use nix::sys::memfd::{MFdFlags, memfd_create};
use nix::unistd::ftruncate;
use wayland_client::protocol::wl_seat::Capability;
use std::ffi::CStr;
use std::os::fd::AsFd;
use std::collections::VecDeque;

use wayland_cursor::CursorTheme;
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, Layer, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, Anchor, ZwlrLayerSurfaceV1},
};

use wayland_protocols_plasma::plasma_virtual_desktop::client::{
    org_kde_plasma_virtual_desktop::{self, OrgKdePlasmaVirtualDesktop},
    org_kde_plasma_virtual_desktop_management::{self, OrgKdePlasmaVirtualDesktopManagement},
};

use wayland_client::{
    Connection, Dispatch, QueueHandle, EventQueue, WEnum,
    protocol::{
        wl_buffer, wl_compositor, wl_keyboard, wl_output, wl_region, wl_registry, wl_seat, wl_shm, wl_pointer,
        wl_shm_pool, wl_surface,
    },
};

struct ShmBuffer {
    buffer: wl_buffer::WlBuffer,
    mmap: memmap2::MmapMut,
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


struct OverlayRunTime {
    event_queue: EventQueue<OverlayState>, 
    state: OverlayState,

    compositor: wl_compositor::WlCompositor,
    layer_shell: ZwlrLayerShellV1,
    shm: wl_shm::WlShm,
    outputs: Vec<OutputInfo>,
}

pub struct OverlayState {
    // wayland stuff
    compositor: Option<wl_compositor::WlCompositor>,
    layer_shell: Option<ZwlrLayerShellV1>,
    shm: Option<wl_shm::WlShm>,
    outputs: Vec<OutputInfo>,
    seat: Option<wl_seat::WlSeat>,
    surfaces: Vec<SurfaceData>,
    events: VecDeque<OverlayEvent>,

    // kde stuff
    virtual_desktop_manager: Option<OrgKdePlasmaVirtualDesktopManagement>,
    current_desktop: Option<String>,
    pending_desktop_ids: Vec<String>,

    // others
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
            if let Ok(c) = capabilities.into_result() {
                if c.contains(Capability::Pointer) {
                    seat.get_pointer(qh, ());
                }
                if c.contains(Capability::Keyboard) {
                    seat.get_keyboard(qh, ());
                }
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
                state.events.push_back(OverlayEvent::EscapePressed);
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for OverlayState {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_pointer::Event::Motion {
            surface_x,
            surface_y,
            ..
        } = event
        {
            state.events.push_back(OverlayEvent::PointerMove { x: surface_x, y: surface_y })
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
    fn present(&mut self, width:u32, height:u32, placements: &[Placement]) -> Result<(), Box<dyn std::error::Error>> {
        self.ensure_runtime()?;
        let rt = self.runtime.as_mut().ok_or("runtime missing")?;
        let qh  = rt.event_queue.handle();
        let state = &mut rt.state;

        let compositor = &rt.compositor;
        let layer_shell = &rt.layer_shell;
        let shm = &rt.shm;
        let outputs = &rt.outputs;

        for placement in placements {

            
            let output = outputs
                .iter()
                .find(|o| o.x == placement.position.0 && o.y == placement.position.1)
                .or_else(|| {
                    eprintln!("No screens found with position {:?}, using main screen (0,0)", placement.position);
                    outputs.first()
                })
                .map(|o| &o.output)
                .ok_or_else(|| "no outputs found")?;

            let (w, h) = (placement.size.0 as u32, placement.size.1 as u32);
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

            rt.event_queue.roundtrip(state)?;


            let shm_buffer = create_shm_buffer(&shm, &qh, w, h)?;
            let transparent_pixels = vec![0u8; (w * h * 4) as usize];
            let mut transparent_buffer = create_shm_buffer(&shm, &qh, w, h)?;
            transparent_buffer.write_pixels(&transparent_pixels);                                       
            let empty_region = compositor.create_region(&qh, ());

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
        rt.event_queue.roundtrip(state)?;



        Ok(())
    }
    fn update_frame(&mut self, pixels: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        let rt = self.runtime.as_mut().ok_or("runtime missing")?;

        for sd in &mut rt.state.surfaces {
            sd.shm_buffer.write_pixels(&pixels);
            sd.surface.attach(Some(&sd.shm_buffer.buffer), 0, 0);
            sd.surface.damage_buffer(0, 0, sd.width as i32, sd.height as i32);
            sd.surface.commit();
        }

        rt.event_queue.flush()?;
        Ok(())
    }

    fn next_event(&mut self) -> Result<OverlayEvent, Box<dyn std::error::Error>> {
        self.ensure_runtime()?;
        let rt = self.runtime.as_mut().ok_or("runtime missing")?;

        loop {
            rt.event_queue.dispatch_pending(&mut rt.state)?;

            if rt.state.events.is_empty() {
                rt.event_queue.blocking_dispatch(&mut rt.state)?;
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
        }
    }

fn ensure_runtime(&mut self) -> Result<(), Box<dyn std::error::Error>> {
    if self.runtime.is_some() {
        return Ok(());
    }

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
        events: VecDeque::new(),
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

    let compositor = state.compositor.take().ok_or("no wl_compositor")?;
    let layer_shell = state.layer_shell.take().ok_or("no zwlr_layer_shell_v1")?;
    let shm = state.shm.take().ok_or("no wl_shm")?;
    let outputs = std::mem::take(&mut state.outputs);

    self.runtime = Some(OverlayRunTime {
        compositor,
        layer_shell,
        shm,
        outputs,
        event_queue,
        state,
    });

    // by defualt it's cross, since screenshot starts with selection mode
    //let rt = self.runtime.as_ref().unwrap();
    //let mut cursor_theme = CursorTheme::load(conn, rt.shm.clone(), 32).expect("Could not load cursor theme");


    Ok(())
}

}


fn create_shm_buffer(
    shm: &wl_shm::WlShm,
    qh: &QueueHandle<OverlayState>,
    width: u32,
    height: u32,
) -> Result<ShmBuffer, Box<dyn std::error::Error>> {
    let stride = width * 4;
    let size = (stride * height) as usize;

    let fd = memfd_create(
        CStr::from_bytes_with_nul(b"lumine-shm\0")?,
        MFdFlags::empty(),
    )?;
    ftruncate(&fd, size as i64)?;
    let mut mmap = unsafe { memmap2::MmapMut::map_mut(&fd)? };
  

    let pool = shm.create_pool(fd.as_fd(), size as i32, qh, ());
    let buffer = pool.create_buffer(0, width as i32, height as i32, stride as i32,
        wl_shm::Format::Argb8888, qh, ());
    pool.destroy();

    Ok(ShmBuffer { buffer, mmap: mmap, _fd: fd })
}



impl ShmBuffer {
    fn write_pixels(&mut self, pixels : &[u8]) {
        self.mmap[..pixels.len()].copy_from_slice(pixels);
    }
}