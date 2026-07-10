// temporary super bloatet and disorganized file

use std::collections::HashMap;
use std::collections::VecDeque;


use crate::backend::wayland::utils::surface::SurfaceData;
use crate::types::{MouseButton, OutputInfo, OverlayEvent, SpecialKey};

use wayland_client::globals::registry_queue_init;
use wayland_protocols::wp::{
    fractional_scale::v1::client::{wp_fractional_scale_manager_v1, wp_fractional_scale_v1},
    viewporter::client::{wp_viewport, wp_viewporter},
};

use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle,
    protocol::{
        wl_buffer, wl_compositor, wl_keyboard, wl_output, wl_pointer, wl_region,
        wl_seat, wl_shm, wl_shm_pool, wl_surface,
    },
};
use wayland_protocols_wlr::layer_shell::v1::client::{
    zwlr_layer_shell_v1::{self, ZwlrLayerShellV1},
};

use crate::backend::wayland::overlay::kde_state::KdeState;

// new smithay 
use smithay_client_toolkit::registry::{ProvidesRegistryState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{delegate_registry, delegate_output, delegate_shm, registry_handlers, delegate_compositor, delegate_layer, delegate_seat,delegate_pointer, delegate_keyboard};
use smithay_client_toolkit::compositor::CompositorHandler;
use smithay_client_toolkit::shell::wlr_layer::{LayerShellHandler, LayerSurface, LayerSurfaceConfigure};
use smithay_client_toolkit::seat::{
    SeatHandler, Capability,
    pointer :: {PointerHandler, PointerEvent, PointerEventKind},
    keyboard :: {KeyboardHandler, KeyEvent, Modifiers, RepeatInfo, Keysym}
};
// import end 

pub struct OverlayState {
    // global
    pub outputs: Vec<OutputInfo>,
    pub frac: Option<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1>,
    pub frac_scale: Option<wp_fractional_scale_v1::WpFractionalScaleV1>,
    pub viewporter: Option<wp_viewporter::WpViewporter>,
    
    pub pointer_enter_serial: u32,
    pub pointer_surface_idx: Option<usize>,
    // runtime
    pub surfaces: HashMap<usize, SurfaceData>,
    pub events: VecDeque<OverlayEvent>,
    pub scale: f64,
    pub pending_flush: bool,
    // gnome/kde
    pub kde: Option<KdeState>,
    //others
    pub ctrl: bool,
    pub shift: bool,
    
    // sctk 
    pub compositor_state: smithay_client_toolkit::compositor::CompositorState,
    pub shm: smithay_client_toolkit::shm::Shm,
    pub layer_shell: smithay_client_toolkit::shell::wlr_layer::LayerShell,
    pub registry_state: smithay_client_toolkit::registry::RegistryState,
    pub output_state: smithay_client_toolkit::output::OutputState,
    pub cursor_shape_manager: smithay_client_toolkit::seat::pointer::cursor_shape::CursorShapeManager,
    pub seat: smithay_client_toolkit::seat::SeatState,
}

pub struct OverlayRunTime {
    pub event_queue: EventQueue<OverlayState>,
    pub state: OverlayState,
}

impl OverlayRunTime {
    pub fn new(conn: &Connection) -> Result<Self, Box<dyn std::error::Error>> {

        let (globals, mut event_queue) = registry_queue_init(&conn)?;
        let qh = event_queue.handle();

        // SCTK bindings
        let registry_state = smithay_client_toolkit::registry::RegistryState::new(&globals);
        let output_state = OutputState::new(&globals, &qh);
        let shm = Shm::bind(&globals, &qh)?;
        let compositor_state = smithay_client_toolkit::compositor::CompositorState::bind(&globals, &qh)?;
        let layer_shell = smithay_client_toolkit::shell::wlr_layer::LayerShell::bind(&globals, &qh)?;
        let seat = smithay_client_toolkit::seat::SeatState::new(&globals, &qh)    ;
        let cursor_shape_manager = smithay_client_toolkit::seat::pointer::cursor_shape::CursorShapeManager::bind(&globals, &qh)?;
        

        // outside of sctk
        let frac = globals.bind::<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1, _, _>(&qh, 1..=1, ()).ok();
        let viewporter = globals.bind::<wp_viewporter::WpViewporter, _, _>(&qh, 1..=1, ()).ok();
    
        let mut state = OverlayState {
            compositor_state,
            registry_state,
            output_state, 
            shm,
            layer_shell,
            cursor_shape_manager, 

            seat,
            frac, 
            viewporter, 

            outputs: Vec::new(),
            surfaces: HashMap::new(),
            events: VecDeque::new(),
            frac_scale: None,
            pointer_surface_idx: None,
            scale: 0.0,
            pending_flush: false,
            kde: Some(KdeState {
                virtual_desktop_manager: None, 
                current_desktop: None,
                pending_desktop_ids: Vec::new(),
            }),
            ctrl: false,
            shift: false,
            pointer_enter_serial: 0,
        };

        event_queue.roundtrip(&mut state)?;
        event_queue.roundtrip(&mut state)?;

        Ok(Self { event_queue, state })
    }
}

impl CompositorHandler for OverlayState {
    fn scale_factor_changed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _surface: &wl_surface::WlSurface, _new_factor: i32) {}
    fn transform_changed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _surface: &wl_surface::WlSurface, _new_transform: wl_output::Transform) {}
    fn frame(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _surface: &wl_surface::WlSurface, _time: u32) {}
    fn surface_enter(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _surface: &wl_surface::WlSurface, _output: &wl_output::WlOutput) {}
    fn surface_leave(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _surface: &wl_surface::WlSurface, _output: &wl_output::WlOutput) {}
}

impl ProvidesRegistryState for OverlayState {
    fn registry(&mut self) -> &mut smithay_client_toolkit::registry::RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState]; 
}

impl OutputHandler for OverlayState {
    fn output_state(&mut self) -> &mut OutputState { &mut self.output_state }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.rebuild_outputs();
    }
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.rebuild_outputs();
    }
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.rebuild_outputs();
    }
}

impl ShmHandler for OverlayState {
    fn shm_state(&mut self) -> &mut smithay_client_toolkit::shm::Shm {
        &mut self.shm
    }
}

impl LayerShellHandler for OverlayState {
    fn configure(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _layer_surface: &LayerSurface,
        _configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        
    }

    fn closed(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _layer_surface: &LayerSurface) {
    }
}

impl SeatHandler for OverlayState {
    fn seat_state(&mut self) -> &mut smithay_client_toolkit::seat::SeatState {
        &mut self.seat
    }
    fn new_capability(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: smithay_client_toolkit::seat::Capability
    ) {
        match capability {
            Capability::Pointer => {
                self.seat.get_pointer(qh, &seat).expect("Failed to get pointer");
            }
            Capability::Keyboard => {
                self.seat.get_keyboard(qh, &seat, None).expect("Failed to get keyboard");
            }
            _ => {}
        }
    }

    fn new_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}
    fn remove_capability(&mut self,_conn: &Connection,_qh: &QueueHandle<Self>,_seat: wl_seat::WlSeat,_capability: smithay_client_toolkit::seat::Capability) {}
    fn remove_seat(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _seat: wl_seat::WlSeat) {}
}
impl OverlayState {
    pub fn rebuild_outputs(&mut self) {
        self.outputs = self.output_state.outputs().map(|output| {
            let info = self.output_state.info(&output);
            let (x, y, width, height) = match info {
                Some(i) => (
                    i.location.0,
                    i.location.1,
                    i.modes.iter().find(|m| m.current).map(|m| m.dimensions.0).unwrap_or(0),
                    i.modes.iter().find(|m| m.current).map(|m| m.dimensions.1).unwrap_or(0),
                ),
                None => (0, 0, 0, 0),
            };
            OutputInfo { output, x, y, width, height }
        }).collect();
    }

    fn process_key(&mut self, event: &KeyEvent) {
        match event.keysym {
            Keysym::Escape => { self.events.push_back(OverlayEvent::EscapePressed); return; }
            Keysym::BackSpace => { self.events.push_back(OverlayEvent::KeyPress(SpecialKey::Backspace)); return; }
            Keysym::Delete => { self.events.push_back(OverlayEvent::KeyPress(SpecialKey::Delete)); return; }
            Keysym::Return => { self.events.push_back(OverlayEvent::KeyPress(SpecialKey::Enter)); return; }
            Keysym::Left => { self.events.push_back(OverlayEvent::KeyPress(SpecialKey::Left)); return; }
            Keysym::Right => { self.events.push_back(OverlayEvent::KeyPress(SpecialKey::Right)); return; }
            Keysym::Up => { self.events.push_back(OverlayEvent::KeyPress(SpecialKey::Up)); return; }
            Keysym::Down => { self.events.push_back(OverlayEvent::KeyPress(SpecialKey::Down)); return; }
            Keysym::Home => { self.events.push_back(OverlayEvent::KeyPress(SpecialKey::Home)); return; }
            Keysym::End => { self.events.push_back(OverlayEvent::KeyPress(SpecialKey::End)); return; }
            _ => {}
        }

        if self.ctrl {
            match event.raw_code {
                44 => {                                                                // 44 -> Z
                    if self.shift {
                        self.events.push_back(OverlayEvent::Redo);
                    } else {
                        self.events.push_back(OverlayEvent::Undo);
                    }
                }
                21 => self.events.push_back(OverlayEvent::Redo),                       // 21 -> Y
                31 => self.events.push_back(OverlayEvent::SaveToClipboard),            // 31 -> S
                30 => self.events.push_back(OverlayEvent::KeyPress(SpecialKey::KeyA)), // 30 -> A
                _ => {}
            }
        } else {
            if let Some(txt) = event.utf8.as_deref() {
                for c in txt.chars() {
                    self.events.push_back(OverlayEvent::TextInput(c));
                }
            }
        }
    }
}

impl PointerHandler for OverlayState {
    fn pointer_frame(
        &mut self,
        _conn: &Connection,
        qh: &QueueHandle<Self>,
        pointer: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            let monitor_idx = self
                .surfaces
                .iter()
                .find(|(_, sd)| sd.surface == event.surface)
                .map(|(id, _)| *id);

            match event.kind {
                PointerEventKind::Enter { serial } => {
                    self.pointer_enter_serial = serial;
                    self.pointer_surface_idx = monitor_idx;

                    let device = self.cursor_shape_manager.get_shape_device(pointer, qh);
                    device.set_shape(
                        serial,
                        wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::Shape::Crosshair,
                    );

                    if let Some(idx) = monitor_idx {
                        self.events.push_back(OverlayEvent::PointerMove {
                            monitor_idx: idx,
                            x: event.position.0,
                            y: event.position.1,
                        });
                    }
                }
                PointerEventKind::Leave { .. } => {
                    self.pointer_surface_idx = None;
                }
                PointerEventKind::Motion { .. } => {
                    if let Some(idx) = self.pointer_surface_idx {
                        self.events.push_back(OverlayEvent::PointerMove {
                            monitor_idx: idx,
                            x: event.position.0,
                            y: event.position.1,
                        });
                    }
                }
                PointerEventKind::Press { button, .. } | PointerEventKind::Release { button, .. } => {
                    let pressed = matches!(event.kind, PointerEventKind::Press { .. });
                    let mb = match button {
                        0x110 => MouseButton::Left,
                        0x111 => MouseButton::Right,
                        0x112 => MouseButton::Middle,
                        _ => continue,
                    };
                    self.events.push_back(OverlayEvent::PointerButton { button: mb, pressed });
                }
                _ => {}
            }
        }
    }
}

impl KeyboardHandler for OverlayState {
    fn press_key(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _keyboard: &wl_keyboard::WlKeyboard, _serial: u32, event: KeyEvent) {
        self.process_key(&event);
    }
    fn repeat_key(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _keyboard: &wl_keyboard::WlKeyboard, _serial: u32, event: KeyEvent) {
        self.process_key(&event);
    }


    fn update_modifiers(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        _keyboard: &wl_keyboard::WlKeyboard,
        _serial: u32,
        modifiers: Modifiers,
        _raw_modifiers: smithay_client_toolkit::seat::keyboard::RawModifiers,
        _layout: u32,
    ) {
        self.ctrl = modifiers.ctrl;
        self.shift = modifiers.shift;
        self.events.push_back(OverlayEvent::ModifiersChanged {
            ctrl: self.ctrl,
            shift: self.shift,
        });
    }
    
    fn enter(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _keyboard: &wl_keyboard::WlKeyboard, _surface: &wl_surface::WlSurface, _serial: u32, _raw: &[u32], _keysyms: &[Keysym]) {}
    fn release_key(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _keyboard: &wl_keyboard::WlKeyboard, _serial: u32, _event: KeyEvent) {}
    fn update_repeat_info( &mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _keyboard: &wl_keyboard::WlKeyboard, _info: RepeatInfo) {}
    fn leave(&mut self, _conn: &Connection, _qh: &QueueHandle<Self>, _keyboard: &wl_keyboard::WlKeyboard, _surface: &wl_surface::WlSurface, _serial: u32) {}
}

delegate_compositor!(OverlayState);
delegate_registry!  (OverlayState);
delegate_output!    (OverlayState);
delegate_shm!       (OverlayState);
delegate_layer!     (OverlayState);
delegate_seat!      (OverlayState);
delegate_pointer!   (OverlayState);
delegate_keyboard!  (OverlayState);



// other dispatches 
// most of them will be removed, the one that can be switched
// to sctk
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