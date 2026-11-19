pub mod compositor_shm;
pub mod cursor;
pub mod global;
pub mod ime;
pub mod keyboard;
pub mod layer;
pub mod output;
pub mod pointer;
pub mod seat;
pub mod xdg;

use std::collections::HashMap;
use std::collections::VecDeque;

use crate::backend::wayland::utils::surface::SurfaceData;
use crate::types::{CursorIcon, Output, OverlayEvent};
use wayland_client::globals::registry_queue_init;
use wayland_protocols::wp::{
    fractional_scale::v1::client::{wp_fractional_scale_manager_v1, wp_fractional_scale_v1},
    viewporter::client::wp_viewporter,
};

use wayland_client::{Connection, EventQueue};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::WpCursorShapeDeviceV1;
use wayland_protocols::wp::text_input::zv3::client::{
    zwp_text_input_manager_v3::ZwpTextInputManagerV3, zwp_text_input_v3::ZwpTextInputV3,
};

// new smithay
use smithay_client_toolkit::output::OutputState;
use smithay_client_toolkit::shm::Shm;

pub struct OverlayState {
    // ── sctk ─────────────────────────────────────────────────────────────────────
    pub compositor_state: smithay_client_toolkit::compositor::CompositorState,
    pub shm: smithay_client_toolkit::shm::Shm,
    pub xdg_shell: smithay_client_toolkit::shell::xdg::XdgShell,
    pub registry_state: smithay_client_toolkit::registry::RegistryState,
    pub output_state: smithay_client_toolkit::output::OutputState,
    pub cursor_shape_manager:
        smithay_client_toolkit::seat::pointer::cursor_shape::CursorShapeManager,
    pub seat: smithay_client_toolkit::seat::SeatState,
    pub pool: smithay_client_toolkit::shm::slot::SlotPool,
    // ── wayland protocol ─────────────────────────────────────────────────────────
    pub outputs: Vec<Output>,
    pub frac: Option<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1>,
    pub frac_scale: Option<wp_fractional_scale_v1::WpFractionalScaleV1>,
    pub viewporter: Option<wp_viewporter::WpViewporter>,
    pub subcompositor: Option<smithay_client_toolkit::subcompositor::SubcompositorState>,
    pub layer_shell: Option<smithay_client_toolkit::shell::wlr_layer::LayerShell>,
    pub probe: Option<Probe>,
    pub text_input_manager: Option<ZwpTextInputManagerV3>,
    pub text_input: Option<ZwpTextInputV3>,
    pub ime: ime::Ime,
    // ── pointer ──────────────────────────────────────────────────────────────────
    pub cursor_shape_device: Option<WpCursorShapeDeviceV1>,
    pub pointer_enter_serial: u32,
    pub pointer_surface_idx: Option<usize>,
    // current cursor shape, and flag is it applied for last enter
    pub current_cursor_icon: CursorIcon,
    pub cursor_applied_since_enter: bool,
    // ── ui & input state ─────────────────────────────────────────────────────────
    pub surfaces: HashMap<usize, SurfaceData>,
    pub events: VecDeque<OverlayEvent>,
    pub scale: f64,
    pub configure_error: Option<String>,
    pub mods: crate::keys::Mods,
    pub toplevel: bool,
    pub min_size: Option<(u32, u32)>,
    pub resize: Option<(usize, (u32, u32))>,
    // ── window frame (client side decorations) ───────────────────────────────────
    // which is used, hopefully only and only on GNOME
    // `seat` is taken by SCTK's SeatState, so the wl_seat lives in `wl_seat`
    pub wl_seat: Option<wayland_client::protocol::wl_seat::WlSeat>,
    pub press_serial: u32,
    pub frame: Option<crate::types::WindowFrame>,
}

pub struct Probe {
    pub layer: smithay_client_toolkit::shell::wlr_layer::LayerSurface,
    pub buffer: Option<crate::backend::wayland::utils::shm::ShmBuffer>,
    pub output: Option<wayland_client::protocol::wl_output::WlOutput>,
}

pub struct OverlayRunTime {
    pub event_queue: EventQueue<OverlayState>,
    pub state: OverlayState,
}

impl OverlayRunTime {
    pub fn new(conn: &Connection) -> Result<Self, Box<dyn std::error::Error>> {
        let (globals, mut event_queue) = registry_queue_init(conn)?;
        let qh = event_queue.handle();

        // ── sctk managers & protocols ─────────────────────────────────────────────────────────────────────
        let registry_state = smithay_client_toolkit::registry::RegistryState::new(&globals);
        let output_state = OutputState::new(&globals, &qh);
        let shm = Shm::bind(&globals, &qh)?;
        let compositor_state =
            smithay_client_toolkit::compositor::CompositorState::bind(&globals, &qh)?;
        let xdg_shell = smithay_client_toolkit::shell::xdg::XdgShell::bind(&globals, &qh)?;
        let seat = smithay_client_toolkit::seat::SeatState::new(&globals, &qh);
        let cursor_shape_manager =
            smithay_client_toolkit::seat::pointer::cursor_shape::CursorShapeManager::bind(
                &globals, &qh,
            )?;

        let pool = smithay_client_toolkit::shm::slot::SlotPool::new(256 * 1024, &shm)?;

        // ── other wayland globals ─────────────────────────────────────────────────────────────────────────
        let frac = globals
            .bind::<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1, _, _>(
                &qh,
                1..=1,
                (),
            )
            .ok();
        let viewporter = globals
            .bind::<wp_viewporter::WpViewporter, _, _>(&qh, 1..=1, ())
            .ok();

        let text_input_manager = globals
            .bind::<ZwpTextInputManagerV3, _, _>(&qh, 1..=1, ())
            .ok();

        // ───── construction
        let mut state = OverlayState::new(
            compositor_state,
            shm,
            xdg_shell,
            registry_state,
            output_state,
            cursor_shape_manager,
            seat,
            pool,
            frac,
            viewporter,
        );
        state.text_input_manager = text_input_manager;
        state.subcompositor = smithay_client_toolkit::subcompositor::SubcompositorState::bind(
            state.compositor_state.wl_compositor().clone(),
            &globals,
            &qh,
        )
        .ok();
        state.layer_shell =
            smithay_client_toolkit::shell::wlr_layer::LayerShell::bind(&globals, &qh).ok();

        // two roundtrips: first gets globals
        // second waits for events update
        event_queue.roundtrip(&mut state)?;
        event_queue.roundtrip(&mut state)?;

        Ok(Self { event_queue, state })
    }
}

// ── overlaystate initialization ─────────────────────────────────────────────────────────────────────
impl OverlayState {
    pub fn new(
        compositor_state: smithay_client_toolkit::compositor::CompositorState,
        shm: smithay_client_toolkit::shm::Shm,
        xdg_shell: smithay_client_toolkit::shell::xdg::XdgShell,
        registry_state: smithay_client_toolkit::registry::RegistryState,
        output_state: smithay_client_toolkit::output::OutputState,
        cursor_shape_manager: smithay_client_toolkit::seat::pointer::cursor_shape::CursorShapeManager,
        seat: smithay_client_toolkit::seat::SeatState,
        pool: smithay_client_toolkit::shm::slot::SlotPool,
        frac: Option<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1>,
        viewporter: Option<wp_viewporter::WpViewporter>,
    ) -> Self {
        Self {
            compositor_state,
            shm,
            xdg_shell,
            registry_state,
            output_state,
            cursor_shape_manager,
            seat,
            pool,
            frac,
            viewporter,
            subcompositor: None,
            layer_shell: None,
            probe: None,
            text_input_manager: None,
            text_input: None,
            ime: ime::Ime::default(),

            cursor_shape_device: None,
            current_cursor_icon: CursorIcon::default(),
            cursor_applied_since_enter: false,
            outputs: Vec::new(),
            surfaces: HashMap::new(),
            events: VecDeque::new(),
            resize: None,
            frac_scale: None,
            pointer_surface_idx: None,
            scale: 0.0,
            configure_error: None,
            mods: crate::keys::Mods::default(),
            pointer_enter_serial: 0,
            toplevel: false,
            min_size: None,
            wl_seat: None,
            press_serial: 0,
            frame: None,
        }
    }
}
