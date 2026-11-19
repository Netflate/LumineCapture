// cursor form set
// names match cursor-shape-v1 / CSS

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CursorIcon {
    #[default]
    Default,
    Crosshair,
    Pointer,
    Text,
    Grab,
    Grabbing,
    NsResize,
    EwResize,
    NeswResize,
    NwseResize,
}

/// the window draws its own frame, the compositor gave none
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WindowFrame {
    /// not maximized, fullscreen or tiled
    pub resizable: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edge {
    Top,
    Bottom,
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowRequest {
    Move,
    Resize(Edge),
    Menu((i32, i32)),
    ToggleMaximized,
}
