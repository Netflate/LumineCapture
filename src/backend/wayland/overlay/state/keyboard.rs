// ── keyboard input handling ──────────────────────────────────────────────────
// handlers shortcuts using a two-tier approach:
// 1. first, checks 'Keysym' to match shortcuts by their actual letter/symbol
// 2. if not latin symbol matches (e.g, user is on a cyrillic layout), it falls
//    back to 'raw_code' to trigger the shortcut based on the physical key position
//
// TODO: it doesn't absolutyely corerctly works on latin keyboard, like
//       azerty users will redo on both ctrl z and ctrl w, will be fixed later, who cares
//       about these sublayouts users anyways

use smithay_client_toolkit::delegate_keyboard;
use smithay_client_toolkit::seat::keyboard::{
    KeyEvent, KeyboardHandler, Keysym, Modifiers, RepeatInfo,
};
use wayland_client::protocol::{wl_keyboard, wl_surface};
use wayland_client::{Connection, QueueHandle};

use crate::backend::wayland::overlay::state::OverlayState;
use crate::types::{OverlayEvent, SpecialKey};

impl KeyboardHandler for OverlayState {
    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.process_key(&event);
    }
    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        self.process_key(&event);
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: smithay_client_toolkit::seat::keyboard::RawModifiers,
        _: u32,
    ) {
        self.ctrl = modifiers.ctrl;
        self.shift = modifiers.shift;
        self.events.push_back(OverlayEvent::ModifiersChanged {
            ctrl: self.ctrl,
            shift: self.shift,
        });
    }

    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }
    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _event: KeyEvent,
    ) {
    }
    fn update_repeat_info(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _info: RepeatInfo,
    ) {
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
    }
}

delegate_keyboard!(OverlayState);

impl OverlayState {
    fn process_key(&mut self, event: &KeyEvent) {
        match event.keysym {
            Keysym::Escape => {
                self.events.push_back(OverlayEvent::EscapePressed);
                return;
            }
            Keysym::BackSpace => {
                self.events
                    .push_back(OverlayEvent::KeyPress(SpecialKey::Backspace));
                return;
            }
            Keysym::Delete => {
                self.events
                    .push_back(OverlayEvent::KeyPress(SpecialKey::Delete));
                return;
            }
            Keysym::Return => {
                self.events
                    .push_back(OverlayEvent::KeyPress(SpecialKey::Enter));
                return;
            }
            Keysym::Left => {
                self.events
                    .push_back(OverlayEvent::KeyPress(SpecialKey::Left));
                return;
            }
            Keysym::Right => {
                self.events
                    .push_back(OverlayEvent::KeyPress(SpecialKey::Right));
                return;
            }
            Keysym::Up => {
                self.events
                    .push_back(OverlayEvent::KeyPress(SpecialKey::Up));
                return;
            }
            Keysym::Down => {
                self.events
                    .push_back(OverlayEvent::KeyPress(SpecialKey::Down));
                return;
            }
            Keysym::Home => {
                self.events
                    .push_back(OverlayEvent::KeyPress(SpecialKey::Home));
                return;
            }
            Keysym::End => {
                self.events
                    .push_back(OverlayEvent::KeyPress(SpecialKey::End));
                return;
            }
            _ => {}
        }

        if self.ctrl {
            // try matching by Keysym first (for standard layouts/shortcuts)
            let matched_action = match event.keysym {
                Keysym::z | Keysym::Z => Some(if self.shift {
                    OverlayEvent::Redo
                } else {
                    OverlayEvent::Undo
                }),
                Keysym::y | Keysym::Y => Some(OverlayEvent::Redo),
                Keysym::s | Keysym::S => Some(OverlayEvent::SaveToClipboard),
                Keysym::a | Keysym::A => Some(OverlayEvent::KeyPress(SpecialKey::KeyA)),
                _ => None,
            };
            // fallback to raw physical scan codes (to support AZERTY/non-QWERTY layouts)
            let final_action = matched_action.or(match event.raw_code {
                44 => Some(if self.shift {
                    OverlayEvent::Redo
                } else {
                    OverlayEvent::Undo
                }), // 44 -> Z
                21 => Some(OverlayEvent::Redo),            // 21 -> Y
                31 => Some(OverlayEvent::SaveToClipboard), // 31 -> S
                30 => Some(OverlayEvent::KeyPress(SpecialKey::KeyA)), // 30 -> A
                _ => None,
            });

            if let Some(action) = final_action {
                self.events.push_back(action);
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
