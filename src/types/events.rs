#[derive(Debug, Clone, Copy)]
pub enum MouseButton {
    Left,
    Right, 
    Middle,
}

pub enum MouseState {
    Up,
    Down(MouseButton),
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
    PointerMove { monitor_idx: usize, x: f64, y: f64},
    PointerButton {button: MouseButton, pressed : bool},
    EscapePressed,
    SaveToClipboard,
    Tick,
    Redo, 
    Undo,
    TextInput(char),
    KeyPress(SpecialKey),
    ModifiersChanged { ctrl: bool, shift: bool },
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
}