pub mod compositor_shm_xdg;
pub mod global;
pub mod keyboard;
pub mod output;
pub mod pointer;
pub mod seat;

use std::collections::HashMap;
use std::collections::VecDeque;

use crate::backend::wayland::utils::surface::SurfaceData;
use crate::types::{Output, OverlayEvent};

use wayland_client::globals::registry_queue_init;
use wayland_protocols::wp::{
    fractional_scale::v1::client::{wp_fractional_scale_manager_v1, wp_fractional_scale_v1},
    viewporter::client::wp_viewporter,
};

use wayland_client::{Connection, EventQueue};
use wayland_protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::WpCursorShapeDeviceV1;

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
    // ── pointer ──────────────────────────────────────────────────────────────────
    pub cursor_shape_device: Option<WpCursorShapeDeviceV1>,
    pub pointer_enter_serial: u32,
    pub pointer_surface_idx: Option<usize>,
    // ── ui & input state ─────────────────────────────────────────────────────────
    pub surfaces: HashMap<usize, SurfaceData>,
    pub events: VecDeque<OverlayEvent>,
    pub scale: f64,
    pub pending_flush: bool,
    pub ctrl: bool,
    pub shift: bool,
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
        let xdg_shell =
            smithay_client_toolkit::shell::xdg::XdgShell::bind(&globals, &qh)?;
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

            cursor_shape_device: None,
            outputs: Vec::new(),
            surfaces: HashMap::new(),
            events: VecDeque::new(),
            frac_scale: None,
            pointer_surface_idx: None,
            scale: 0.0,
            pending_flush: false,
            ctrl: false,
            shift: false,
            pointer_enter_serial: 0,
        }
    }
}
