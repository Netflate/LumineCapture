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

use crate::backend::wayland::keysym;
use crate::backend::wayland::overlay::state::OverlayState;
use crate::keys::Key;
use crate::types::OverlayEvent;
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
        self.mods = keysym::mods(&modifiers);
        self.events.push_back(OverlayEvent::ModifiersChanged {
            ctrl: self.mods.ctrl,
            shift: self.mods.shift,
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
        let chord = keysym::chord(event.keysym, event.raw_code, self.mods);
        let editing_key = chord.is_some_and(|c| {
            matches!(
                c.key,
                Key::Escape
                    | Key::Enter
                    | Key::Backspace
                    | Key::Delete
                    | Key::Left
                    | Key::Right
                    | Key::Up
                    | Key::Down
                    | Key::Home
                    | Key::End
            )
        });
        let text = event
            .utf8
            .clone()
            .filter(|t| !t.is_empty() && !self.mods.ctrl && !editing_key);
        if chord.is_some() || text.is_some() {
            self.events.push_back(OverlayEvent::Key { chord, text });
        }
    }
}
