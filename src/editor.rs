pub mod history;
pub mod dirty;

use std::time::{Instant};
use std::collections::HashMap;
use usvg::Tree;
use tiny_skia::{Pixmap, Rect};

use crate::tools::Tool;
use crate::types::{PointerState, Placement, Toolbar, SelectionState, ToolbarButton, Annotation, MagnifierState};

pub struct EditorState {
    pub base: Vec<Pixmap>,          
    pub canvas: Vec<Pixmap>,
    pub dimmed: Vec<Pixmap>,
    pub placements : Vec<Placement>, 
    pub drag_start: Option<(f64, f64)>,
    pub selected_tool: Tool,               
    pub tool_active: bool,
    pub pointer: PointerState,
    pub magnifier: Option<MagnifierState> ,
    pub prev_magnifier: Option<MagnifierState>,
    pub last_mag_update: Option<Instant>,
    pub mouse_down_left : bool,
    pub selection: SelectionState,
    pub toolbar : Toolbar,
    pub icon_cache : HashMap<ToolbarButton, Tree>,
    
    // annotations
    pub annotations: Vec<Annotation>,
    pub pending: Option<Annotation>,
    pub prev_pending: Option<Annotation>,
    pub next_id: u64,

    pub undo_stack: Vec<Vec<Annotation>>,
    pub redo_stack: Vec<Vec<Annotation>>,
    pub damage_rects: Vec<Rect>
}
