use crate::keys::Chord;

#[derive(Debug, Clone, Copy)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PointerState {
    pub monitor_idx: usize,
    pub local: (f64, f64),
    pub global: (f64, f64),
}

impl PointerState {
    pub fn new(monitor_idx: usize, local: (f64, f64), global: (f64, f64)) -> Self {
        Self {
            monitor_idx,
            local,
            global,
        }
    }
}

#[derive(Debug, Clone)]
pub enum OverlayEvent {
    PointerMove {
        monitor_idx: usize,
        x: f64,
        y: f64,
    },
    Focus {
        monitor_idx: usize,
    },
    PointerButton {
        button: MouseButton,
        pressed: bool,
    },
    EscapePressed,
    Tick,
    Key {
        chord: Option<Chord>,
        text: Option<String>,
    },
    ModifiersChanged {
        ctrl: bool,
        shift: bool,
    },
    Scroll {
        delta_x: f32,
        delta_y: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Finish {
    Pin,
    Copy,
    Save,
}

/// What to do with the finished shot: any mix of copy, pin and save.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Outputs {
    pub copy: bool,
    pub pin: bool,
    pub save: bool,
}

impl Outputs {
    /// Letters c, p, s in any order; repeats are fine, an empty string means nothing.
    pub fn parse(letters: &str) -> Result<Self, String> {
        let mut outputs = Self::default();
        for letter in letters.trim().chars() {
            match letter.to_ascii_lowercase() {
                'c' => outputs.copy = true,
                'p' => outputs.pin = true,
                's' => outputs.save = true,
                other => {
                    return Err(format!(
                        "unknown letter {other:?} in {letters:?}, expected c (copy), p (pin) or s (save)"
                    ));
                }
            }
        }
        Ok(outputs)
    }

    pub fn is_empty(self) -> bool {
        self == Self::default()
    }
}

impl From<Finish> for Outputs {
    fn from(finish: Finish) -> Self {
        Self {
            copy: finish == Finish::Copy,
            pin: finish == Finish::Pin,
            save: finish == Finish::Save,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outputs_ignore_order_and_repeats() {
        let all = Outputs {
            copy: true,
            pin: true,
            save: true,
        };
        assert_eq!(Outputs::parse("cps"), Ok(all));
        assert_eq!(Outputs::parse("spc"), Ok(all));
        assert_eq!(Outputs::parse("PsSp c".replace(' ', "").as_str()), Ok(all));
        assert_eq!(Outputs::parse("p"), Ok(Outputs::from(Finish::Pin)));
    }

    #[test]
    fn empty_outputs_do_nothing() {
        assert!(Outputs::parse("").unwrap().is_empty());
    }

    #[test]
    fn unknown_letter_is_rejected() {
        assert!(Outputs::parse("cx").is_err());
    }
}

#[derive(Debug, Clone)]
pub enum SpecialKey {
    Backspace,
    Enter,
    Left,
    Right,
    Home,
    End,
    Delete,
    Up,
    Down,
    KeyA,
    KeyC,
    KeyX,
    KeyV,
}
