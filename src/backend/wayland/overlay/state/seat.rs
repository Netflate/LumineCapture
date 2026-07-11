// ── Seat Management ──────────────────────────────────────────────────────────
// reminder: wl_seat represents a logical input device that bundles together
// stuff like mouse (pointer), keyboard
//
// We monitor for new capabilities (keyboard & mouse) to initialize input handlers
use smithay_client_toolkit::delegate_seat;
use smithay_client_toolkit::seat::{Capability, SeatHandler};
use wayland_client::protocol::wl_seat;
use wayland_client::{Connection, QueueHandle};

use crate::backend::wayland::overlay::state::OverlayState;

impl SeatHandler for OverlayState {
    fn seat_state(&mut self) -> &mut smithay_client_toolkit::seat::SeatState {
        &mut self.seat
    }
    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: smithay_client_toolkit::seat::Capability,
    ) {
        match capability {
            Capability::Pointer => {
                // Initialize pointer and bind the cursor shape manager
                // to allow controlling cursors shape on our overlay
                let pointer = self
                    .seat
                    .get_pointer(qh, &seat)
                    .expect("Failed to get pointer");
                let device = self.cursor_shape_manager.get_shape_device(&pointer, qh);
                self.cursor_shape_device = Some(device);
            }
            Capability::Keyboard => {
                // Initialize keyboard
                self.seat
                    .get_keyboard(qh, &seat, None)
                    .expect("Failed to get keyboard");
            }
            _ => {}
        }
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        _: smithay_client_toolkit::seat::Capability,
    ) {
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

delegate_seat!(OverlayState);
