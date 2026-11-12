use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::wp::text_input::zv3::client::{
    zwp_text_input_manager_v3::{self, ZwpTextInputManagerV3},
    zwp_text_input_v3::{self, ContentHint, ContentPurpose, ZwpTextInputV3},
};

use crate::backend::wayland::overlay::state::OverlayState;
use crate::types::OverlayEvent;

pub type Area = (i32, i32, i32, i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Enable(Area),
    Move(Area),
    Disable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Update {
    pub preedit: Option<String>,
    pub commit: Option<String>,
}

/// text-input-v3 state; compositor forgets it on every enter, so we enable again
#[derive(Default)]
pub struct Ime {
    area: Option<Area>,
    entered: bool,
    enabled: bool,
    sent: Option<Area>,
    preedit: Option<String>,
    commit: Option<String>,
    shown: bool,
}

impl Ime {
    pub fn want(&mut self, area: Option<Area>) -> Option<Request> {
        self.area = area;
        self.sync()
    }

    pub fn enter(&mut self) -> Option<Request> {
        self.entered = true;
        self.enabled = false;
        self.sent = None;
        self.sync()
    }

    pub fn leave(&mut self) -> Option<Update> {
        self.entered = false;
        self.enabled = false;
        self.sent = None;
        self.preedit = None;
        self.commit = None;
        self.clear()
    }

    pub fn preedit(&mut self, text: Option<String>) {
        self.preedit = text.filter(|t| !t.is_empty());
    }

    pub fn commit(&mut self, text: Option<String>) {
        self.commit = text.filter(|t| !t.is_empty());
    }

    /// batch with no preedit simply means the old one is gone, so it is reported too
    pub fn done(&mut self) -> Option<Update> {
        let update = Update {
            preedit: self.preedit.take(),
            commit: self.commit.take(),
        };
        let shown = update.preedit.is_some();
        let changed = update.commit.is_some() || shown || self.shown;
        self.shown = shown;
        changed.then_some(update)
    }

    fn clear(&mut self) -> Option<Update> {
        std::mem::take(&mut self.shown).then_some(Update {
            preedit: None,
            commit: None,
        })
    }

    fn sync(&mut self) -> Option<Request> {
        let area = if self.entered { self.area } else { None };
        match (area, self.enabled) {
            (Some(area), false) => {
                self.enabled = true;
                self.sent = Some(area);
                Some(Request::Enable(area))
            }
            (Some(area), true) if self.sent != Some(area) => {
                self.sent = Some(area);
                Some(Request::Move(area))
            }
            (None, true) => {
                self.enabled = false;
                self.sent = None;
                Some(Request::Disable)
            }
            _ => None,
        }
    }
}

impl OverlayState {
    pub fn set_ime(&mut self, area: Option<Area>) -> bool {
        let request = self.ime.want(area);
        self.send_ime(request)
    }

    fn send_ime(&self, request: Option<Request>) -> bool {
        let (Some(request), Some(input)) = (request, self.text_input.as_ref()) else {
            return false;
        };
        match request {
            Request::Enable(area) => {
                input.enable();
                input.set_content_type(ContentHint::None, ContentPurpose::Normal);
                input.set_cursor_rectangle(area.0, area.1, area.2, area.3);
            }
            Request::Move(area) => {
                input.set_cursor_rectangle(area.0, area.1, area.2, area.3);
            }
            Request::Disable => input.disable(),
        }
        input.commit();
        true
    }
}

impl Dispatch<ZwpTextInputManagerV3, ()> for OverlayState {
    fn event(
        _: &mut Self,
        _: &ZwpTextInputManagerV3,
        _: zwp_text_input_manager_v3::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpTextInputV3, ()> for OverlayState {
    fn event(
        state: &mut Self,
        _: &ZwpTextInputV3,
        event: zwp_text_input_v3::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwp_text_input_v3::Event::Enter { .. } => {
                let request = state.ime.enter();
                if state.send_ime(request) {
                    state.pending_flush = true;
                }
            }
            zwp_text_input_v3::Event::Leave { .. } => {
                if let Some(update) = state.ime.leave() {
                    state.events.push_back(OverlayEvent::Ime {
                        preedit: update.preedit,
                        commit: update.commit,
                    });
                }
            }
            zwp_text_input_v3::Event::PreeditString { text, .. } => state.ime.preedit(text),
            zwp_text_input_v3::Event::CommitString { text } => state.ime.commit(text),
            zwp_text_input_v3::Event::Done { .. } => {
                if let Some(update) = state.ime.done() {
                    state.events.push_back(OverlayEvent::Ime {
                        preedit: update.preedit,
                        commit: update.commit,
                    });
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Area = (10, 20, 1, 30);

    fn ready() -> Ime {
        let mut ime = Ime::default();
        assert_eq!(ime.want(Some(AREA)), None);
        assert_eq!(ime.enter(), Some(Request::Enable(AREA)));
        ime
    }

    #[test]
    fn waits_for_the_enter() {
        let mut ime = Ime::default();
        assert_eq!(ime.want(Some(AREA)), None);
        assert_eq!(ime.enter(), Some(Request::Enable(AREA)));
    }

    #[test]
    fn enables_at_once_when_already_entered() {
        let mut ime = Ime::default();
        assert_eq!(ime.enter(), None);
        assert_eq!(ime.want(Some(AREA)), Some(Request::Enable(AREA)));
    }

    #[test]
    fn follows_the_caret_only_when_it_moves() {
        let mut ime = ready();
        assert_eq!(ime.want(Some(AREA)), None);
        let moved = (12, 20, 1, 30);
        assert_eq!(ime.want(Some(moved)), Some(Request::Move(moved)));
        assert_eq!(ime.want(Some(moved)), None);
    }

    #[test]
    fn disables_when_nothing_is_edited() {
        let mut ime = ready();
        assert_eq!(ime.want(None), Some(Request::Disable));
        assert_eq!(ime.want(None), None);
    }

    #[test]
    fn enables_again_after_the_focus_comes_back() {
        let mut ime = ready();
        assert_eq!(ime.leave(), None);
        assert_eq!(ime.want(Some(AREA)), None);
        assert_eq!(ime.enter(), Some(Request::Enable(AREA)));
    }

    #[test]
    fn batch_applies_on_done_only() {
        let mut ime = ready();
        ime.preedit(Some("ni".into()));
        assert_eq!(
            ime.done(),
            Some(Update {
                preedit: Some("ni".into()),
                commit: None
            })
        );
        ime.commit(Some("你".into()));
        assert_eq!(
            ime.done(),
            Some(Update {
                preedit: None,
                commit: Some("你".into())
            })
        );
    }

    #[test]
    fn empty_batch_after_a_preedit_clears_it() {
        let mut ime = ready();
        ime.preedit(Some("ni".into()));
        ime.done();
        assert_eq!(
            ime.done(),
            Some(Update {
                preedit: None,
                commit: None
            })
        );
        assert_eq!(ime.done(), None);
    }

    #[test]
    fn empty_preedit_string_counts_as_none() {
        let mut ime = ready();
        ime.preedit(Some(String::new()));
        assert_eq!(ime.done(), None);
    }

    #[test]
    fn leaving_drops_the_shown_preedit_once() {
        let mut ime = ready();
        ime.preedit(Some("ni".into()));
        ime.done();
        assert_eq!(
            ime.leave(),
            Some(Update {
                preedit: None,
                commit: None
            })
        );
        assert_eq!(ime.leave(), None);
    }
}
