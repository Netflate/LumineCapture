use tiny_skia::Rect;

pub const HANDLE_RADIUS: f64 = 8.0; // pixels around selection border 

pub struct SelectionState {
    pub zone: Option<Rect>,
    pub prev_zone: Option<Rect>,
    pub active_handle: SelectionHandle,
    pub drag_origin: Option<(f64, f64)>,
    pub selection_at_drag_start: Option<Rect>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SelectionHandle {
    TopLeft,
    Top,
    TopRight,
    Left,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
    Move,
    None,
}

impl Default for SelectionState {
    fn default() -> Self {
        Self {
            zone: None,
            prev_zone: None,
            active_handle: SelectionHandle::None,
            drag_origin: None,
            selection_at_drag_start: None,
        }
    }
}

impl SelectionState {
    pub fn set_drag(
        &mut self,
        handle: SelectionHandle,
        origin: Option<(f64, f64)>,
        zone: Option<Rect>,
    ) {
        self.active_handle = handle;
        self.drag_origin = origin;
        self.selection_at_drag_start = zone;
    }
}

pub struct SelectionEdges {
    pub left: bool,
    pub right: bool,
    pub top: bool,
    pub bottom: bool,
}
