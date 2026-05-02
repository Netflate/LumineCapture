use std::collections::HashMap;
use wayland_client::protocol::wl_seat::Capability;
use std::collections::VecDeque;

use crate::backend::wayland::utils::surface::{SurfaceData};
use crate::types::{OverlayEvent, OutputInfo, MouseButton};

use wayland_protocols::wp:: {
    fractional_scale::v1::client::{wp_fractional_scale_manager_v1, wp_fractional_scale_v1},
    viewporter::client::{wp_viewport, wp_viewporter},
};

use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
};
use wayland_client::{
    Connection, Dispatch, QueueHandle, EventQueue,
    protocol::{
        wl_buffer, wl_compositor, wl_keyboard, wl_output, wl_region, wl_registry, wl_seat, wl_shm, wl_pointer,
        wl_shm_pool, wl_surface,
    },
};



use crate::backend::wayland::overlay::kde_state::KdeState;




pub struct OverlayRunTime {
    pub event_queue: EventQueue<OverlayState>, 
    pub state: OverlayState,
}

impl OverlayRunTime {
    pub fn new(conn: &Connection) -> Result<Self, Box<dyn std::error::Error>> {
        let mut event_queue = conn.new_event_queue();
        let qh = event_queue.handle();
        conn.display().get_registry(&qh, ());

        let mut state = OverlayState {
            compositor: None,
            layer_shell: None,
            shm: None,
            seat: None,
            outputs: Vec::new(),
            surfaces: HashMap::new(),
            events: VecDeque::new(),
            frac: None,
            frac_scale: None,
            viewporter: None,
            pointer_surface_idx: None,
            scale: 0.0,
            kde: Some(KdeState {
                virtual_desktop_manager: None,
                current_desktop: None,
                pending_desktop_ids: Vec::new(),
            }),
        };

        event_queue.roundtrip(&mut state)?;
        event_queue.roundtrip(&mut state)?;

        // KDE virtual desktops
        let pending = std::mem::take(&mut state.kde.as_mut().unwrap().pending_desktop_ids);
        if let Some(manager) = state.kde.as_ref().and_then(|k| k.virtual_desktop_manager.as_ref()) {
            for desktop_id in pending {
                manager.get_virtual_desktop(desktop_id.clone(), &qh, desktop_id);
            }
        }

        event_queue.roundtrip(&mut state)?;
        event_queue.roundtrip(&mut state)?;

        state.compositor.as_ref().ok_or("no wl_compositor")?;
        state.layer_shell.as_ref().ok_or("no zwlr_layer_shell_v1")?;
        state.shm.as_ref().ok_or("no wl_shm")?;

        Ok(Self { event_queue, state })
    }
}

pub struct OverlayState {
    // global
    pub compositor: Option<wl_compositor::WlCompositor>,
    pub layer_shell: Option<ZwlrLayerShellV1>,
    pub shm: Option<wl_shm::WlShm>,
    pub outputs: Vec<OutputInfo>,
    pub seat: Option<wl_seat::WlSeat>,
    pub frac: Option<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1>,
    pub frac_scale: Option<wp_fractional_scale_v1::WpFractionalScaleV1>,
    pub viewporter: Option<wp_viewporter::WpViewporter>,
    // runtime
    pub surfaces: HashMap<usize, SurfaceData>,
    pub events: VecDeque<OverlayEvent>,
    pub pointer_surface_idx: Option<usize>,
    pub scale: f64,

    // gnome/kde
    pub kde :Option<KdeState>,
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
                "wp_fractional_scale_manager_v1" => {
                    state.frac = Some(registry.bind(name, version, qh, ()));
                }
                "wp_viewporter" => {
                    state.viewporter = Some(registry.bind(name, version, qh, ()));
                }
                "org_kde_plasma_virtual_desktop_management" => {
                    let ver = version.min(2);
                    state.kde.as_mut().unwrap().virtual_desktop_manager = Some(registry.bind(name, ver, qh, ()));
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

impl Dispatch<wp_fractional_scale_v1::WpFractionalScaleV1, ()> for OverlayState {
    fn event(
        state: &mut Self,
        _: &wp_fractional_scale_v1::WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wp_fractional_scale_v1::Event::PreferredScale { scale } => {
                let scale = scale as f64 / 120.0;
                state.scale = scale;
            }
            _ => {}
        }
    }
}

impl Dispatch<wp_viewporter::WpViewporter, ()> for OverlayState {
    fn event(
        _: &mut Self,
        _: &wp_viewporter::WpViewporter,
        _: wp_viewporter::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wp_viewport::WpViewport, ()> for OverlayState {
    fn event(
        _: &mut Self,
        _: &wp_viewport::WpViewport,
        _: wp_viewport::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
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
        match event {
            wl_pointer::Event::Enter { surface, .. } => {
                state.pointer_surface_idx = state.surfaces.iter()
                    .find(|(_, sd)| sd.surface == surface)
                    .map(|(id, _)| *id);
            }
            wl_pointer::Event::Leave { .. } => {
                state.pointer_surface_idx = None;
            }
            wl_pointer::Event::Motion { surface_x, surface_y, .. } => {
                if let Some(monitor_idx) = state.pointer_surface_idx {
                    state.events.push_back(OverlayEvent::PointerMove {
                        monitor_idx,
                        x: surface_x,
                        y: surface_y,
                    });
                }
            }
            wl_pointer::Event::Button {button, state: button_state, ..} => {
                let pressed = button_state == wayland_client::WEnum::Value(wl_pointer::ButtonState::Pressed);
                let mb = match button {
                    0x110 => MouseButton::Left,
                    0x111 => MouseButton::Right,
                    0x112 => MouseButton::Middle,
                    _ => return,  
                };
                state.events.push_back(OverlayEvent::PointerButton {button: mb, pressed} );
            }
            _ => {}
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

impl Dispatch<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1, ()> for OverlayState {
    fn event(
        _state: &mut Self,
        _proxy: &wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1,
        _event: wp_fractional_scale_manager_v1::Event,
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
            for sd in state.surfaces.values_mut() {
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