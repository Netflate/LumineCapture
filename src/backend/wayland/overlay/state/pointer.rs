// ── pointer input handling ───────────────────────────────────────────
// pointer mouvement, actions, and shape change
// use of 'pointer_frame', to group multiple mouse events in one cycle
// instead of sending them one by one

use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use wayland_client::protocol::wl_pointer;
use wayland_client::{Connection, QueueHandle};

use crate::backend::wayland::overlay::state::OverlayState;
use crate::types::{MouseButton, OverlayEvent};
impl PointerHandler for OverlayState {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        for event in events {
            // searching on which surface (each surface correspond to one output) it occured
            let monitor_idx = self
                .surfaces
                .iter()
                .find(|(_, sd)| sd.surface == event.surface)
                .map(|(id, _)| *id);
            match event.kind {
                PointerEventKind::Enter { serial } => {
                    self.pointer_enter_serial = serial;
                    self.pointer_surface_idx = monitor_idx;

                    self.cursor_applied_since_enter = false;
                    self.apply_cursor(self.current_cursor_icon);

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

                PointerEventKind::Press { button, .. }
                | PointerEventKind::Release { button, .. } => {
                    let pressed = matches!(event.kind, PointerEventKind::Press { .. });

                    // linux input codes (BTN_LEFT=0x110, BTN_RIGHT=0x111, BTN_MIDDLE=0x112)
                    let mb = match button {
                        0x110 => MouseButton::Left,
                        0x111 => MouseButton::Right,
                        0x112 => MouseButton::Middle,
                        _ => continue,
                    };
                    self.events.push_back(OverlayEvent::PointerButton {
                        button: mb,
                        pressed,
                    });
                }

                PointerEventKind::Axis {
                    horizontal,
                    vertical,
                    ..
                } => {
                    let delta_x = horizontal.absolute as f32;
                    let delta_y = -vertical.absolute as f32;

                    if delta_x != 0.0 || delta_y != 0.0 {
                        self.events
                            .push_back(OverlayEvent::Scroll { delta_x, delta_y });
                    }
                }
            }
        }
    }
}
