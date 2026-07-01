use std::collections::HashMap;
use std::collections::VecDeque;
use wayland_client::protocol::wl_seat::Capability;

use crate::backend::wayland::utils::surface::SurfaceData;
use crate::types::{MouseButton, OutputInfo, OverlayEvent, SpecialKey};
use crate::utils::keycode_to_char;

use wayland_protocols::wp::{
    fractional_scale::v1::client::{wp_fractional_scale_manager_v1, wp_fractional_scale_v1},
    viewporter::client::{wp_viewport, wp_viewporter},
};

use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    protocol::{
        wl_buffer, wl_compositor, wl_keyboard, wl_output, wl_pointer, wl_region, wl_registry,
        wl_seat, wl_shm, wl_shm_pool, wl_surface,
    },
};
use wayland_cursor::CursorTheme;
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
    zwlr_layer_surface_v1::{self, ZwlrLayerSurfaceV1},
};

use crate::backend::wayland::overlay::kde_state::KdeState;
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

    pub cursor_surface: Option<wl_surface::WlSurface>,
    pub cursor_theme: Option<CursorTheme>,
    pub cursor_hotspot: (i32, i32),
    pub pointer_enter_serial: u32,
    // runtime
    pub surfaces: HashMap<usize, SurfaceData>,
    pub events: VecDeque<OverlayEvent>,
    pub pointer_surface_idx: Option<usize>,
    pub scale: f64,
    pub pending_flush: bool,
    // gnome/kde
    pub kde: Option<KdeState>,
    //others
    ctrl_held: bool,
    shift_held: bool,
}

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
            pending_flush: false,
            kde: Some(KdeState {
                virtual_desktop_manager: None,
                current_desktop: None,
                pending_desktop_ids: Vec::new(),
            }),
            ctrl_held: false,
            shift_held: false,
            pointer_enter_serial: 0,
            cursor_surface: None,
            cursor_theme: None,
            cursor_hotspot: (0, 0),
        };

        event_queue.roundtrip(&mut state)?;
        event_queue.roundtrip(&mut state)?;

        // KDE virtual desktops
        let pending = std::mem::take(&mut state.kde.as_mut().unwrap().pending_desktop_ids);
        if let Some(manager) = state
            .kde
            .as_ref()
            .and_then(|k| k.virtual_desktop_manager.as_ref())
        {
            for desktop_id in pending {
                manager.get_virtual_desktop(desktop_id.clone(), &qh, desktop_id);
            }
        }

        event_queue.roundtrip(&mut state)?;
        event_queue.roundtrip(&mut state)?;

        // setting visual cursor
        if let (Some(compositor), Some(shm)) = (&state.compositor, &state.shm) {
            let size = 24u32;
            if let Ok(mut theme) = CursorTheme::load(conn, shm.clone(), size) {
                let cursor = if let Some(cursor) = theme.get_cursor("crosshair") {
                    Some(cursor)
                } else if let Some(cursor) = theme.get_cursor("cross") {
                    Some(cursor)
                } else {
                    theme.get_cursor("default")
                };

                if let Some(cursor) = cursor {
                    let img = &cursor[0];
                    let (hx, hy) = img.hotspot();
                    let surface = compositor.create_surface(&qh, ());
                    surface.attach(Some(img), 0, 0);
                    surface.commit();
                    state.cursor_surface = Some(surface);
                    state.cursor_hotspot = (hx as i32, hy as i32);
                }

                // Keep the theme alive so the cursor buffers stay valid.
                state.cursor_theme = Some(theme);
            }
        }
        state.compositor.as_ref().ok_or("no wl_compositor")?;
        state.layer_shell.as_ref().ok_or("no zwlr_layer_shell_v1")?;
        state.shm.as_ref().ok_or("no wl_shm")?;

        Ok(Self { event_queue, state })
    }
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
                    let ver = version.min(wl_compositor::WlCompositor::interface().version);
                    state.compositor = Some(registry.bind(name, ver, qh, ()));
                }
                "wl_shm" => {
                    let ver = version.min(wl_shm::WlShm::interface().version);
                    state.shm = Some(registry.bind(name, ver, qh, ()));
                }
                "wl_output" => {
                    let ver = version.min(wl_output::WlOutput::interface().version);
                    let output = registry.bind(name, ver, qh, ());
                    state.outputs.push(OutputInfo {
                        output,
                        x: 0,
                        y: 0,
                        width: 0,
                        height: 0,
                    });
                }
                "zwlr_layer_shell_v1" => {
                    let ver =
                        version.min(zwlr_layer_shell_v1::ZwlrLayerShellV1::interface().version);
                    state.layer_shell = Some(registry.bind(name, ver, qh, ()));
                }
                "wl_seat" => {
                    let ver = version.min(wl_seat::WlSeat::interface().version);
                    state.seat = Some(registry.bind(name, ver, qh, ()));
                }
                "wp_fractional_scale_manager_v1" => {
                    let ver = version.min(
                        wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1::interface()
                            .version,
                    );
                    state.frac = Some(registry.bind(name, ver, qh, ()));
                }
                "wp_viewporter" => {
                    let ver: u32 = version.min(wp_viewporter::WpViewporter::interface().version);
                    state.viewporter = Some(registry.bind(name, ver, qh, ()));
                }
                "org_kde_plasma_virtual_desktop_management" => {
                    let ver = version.min(2);
                    state.kde.as_mut().unwrap().virtual_desktop_manager =
                        Some(registry.bind(name, ver, qh, ()));
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

// all of this temporarily poor implemented stuff
// better one isn't worth it, since i'll rewrite all of this
// using simthay
impl Dispatch<wl_keyboard::WlKeyboard, ()> for OverlayState {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Key {
                key,
                state: key_state,
                ..
            } => {
                let pressed =
                    key_state == wayland_client::WEnum::Value(wl_keyboard::KeyState::Pressed);

                if pressed {
                    if key == 1 {
                        state.events.push_back(OverlayEvent::EscapePressed);
                    }

                    if key == 46 && state.ctrl_held {
                        state.events.push_back(OverlayEvent::SaveToClipboard);
                    }

                    // (Undo / Redo) ctrl + z, or ctrl + shit + z
                    if key == 44 {
                        if state.ctrl_held && state.shift_held {
                            state.events.push_back(OverlayEvent::Redo);
                        } else if state.ctrl_held {
                            state.events.push_back(OverlayEvent::Undo);
                        }
                    }

                    // (Redo) ctrl + y
                    if key == 21 && state.ctrl_held {
                        state.events.push_back(OverlayEvent::Redo);
                    }

                    if !state.ctrl_held {
                        match key {
                            14 => state
                                .events
                                .push_back(OverlayEvent::KeyPress(SpecialKey::Backspace)),
                            28 => state
                                .events
                                .push_back(OverlayEvent::KeyPress(SpecialKey::Enter)),
                            105 => state
                                .events
                                .push_back(OverlayEvent::KeyPress(SpecialKey::Left)),
                            106 => state
                                .events
                                .push_back(OverlayEvent::KeyPress(SpecialKey::Right)),
                            102 => state
                                .events
                                .push_back(OverlayEvent::KeyPress(SpecialKey::Home)),
                            107 => state
                                .events
                                .push_back(OverlayEvent::KeyPress(SpecialKey::End)),
                            _ => {
                                let ch = keycode_to_char(key, state.shift_held);
                                if let Some(c) = ch {
                                    state.events.push_back(OverlayEvent::TextInput(c));
                                }
                            }
                        }
                    }
                }
            }
            wl_keyboard::Event::Modifiers { mods_depressed, .. } => {
                state.ctrl_held = (mods_depressed & 4) != 0;
                state.shift_held = (mods_depressed & 1) != 0;
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for OverlayState {
    fn event(
        state: &mut Self,
        ptr: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter {
                surface,
                surface_x,
                surface_y,
                serial,
                ..
            } => {
                state.pointer_enter_serial = serial;
                state.pointer_surface_idx = state
                    .surfaces
                    .iter()
                    .find(|(_, sd)| sd.surface == surface)
                    .map(|(id, _)| *id);

                if let Some(cs) = &state.cursor_surface {
                    let (hx, hy) = state.cursor_hotspot;
                    ptr.set_cursor(serial, Some(cs), hx, hy);
                }

                if let Some(monitor_idx) = state.pointer_surface_idx {
                    state.events.push_back(OverlayEvent::PointerMove {
                        monitor_idx,
                        x: surface_x,
                        y: surface_y,
                    });
                }
            }
            wl_pointer::Event::Leave { .. } => {
                state.pointer_surface_idx = None;
            }
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => {
                if let Some(monitor_idx) = state.pointer_surface_idx {
                    state.events.push_back(OverlayEvent::PointerMove {
                        monitor_idx,
                        x: surface_x,
                        y: surface_y,
                    });
                }
            }
            wl_pointer::Event::Button {
                button,
                state: button_state,
                ..
            } => {
                let pressed =
                    button_state == wayland_client::WEnum::Value(wl_pointer::ButtonState::Pressed);
                let mb = match button {
                    0x110 => MouseButton::Left,
                    0x111 => MouseButton::Right,
                    0x112 => MouseButton::Middle,
                    _ => return,
                };
                state.events.push_back(OverlayEvent::PointerButton {
                    button: mb,
                    pressed,
                });
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
