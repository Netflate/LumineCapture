pub mod dirty;
pub mod history;

use cosmic_text::{Editor, FontSystem, SwashCache};
use std::collections::HashMap;
use std::time::Instant;
use tiny_skia::{Pixmap, Rect};
use usvg::Tree;

use crate::tools::Tool;
use crate::types::{
    AnnDragState, Annotation, MagnifierState, Placement, PointerState, SelectionState,
    TextEditState, Toolbar, ToolbarButton,
};

pub struct EditorState {
    pub base: Vec<Pixmap>,
    pub canvas: Vec<Pixmap>,
    pub dimmed: Vec<Pixmap>,
    pub placements: Vec<Placement>,
    pub drag_start: Option<(f64, f64)>,
    pub selected_tool: Tool,
    pub tool_active: bool,
    pub pointer: PointerState,
    pub magnifier: Option<MagnifierState>,
    pub prev_magnifier: Option<MagnifierState>,
    pub last_mag_update: Option<Instant>,
    pub mouse_down_left: bool,
    pub selection: SelectionState,
    pub toolbar: Toolbar,
    pub icon_cache: HashMap<ToolbarButton, Tree>,
    pub damage_rects: Vec<Rect>,

    // annotations
    pub annotations: Vec<Annotation>,
    pub pending: Option<Annotation>,
    pub prev_pending: Option<Annotation>,
    pub next_id: u64,

    pub undo_stack: Vec<Vec<Annotation>>,
    pub redo_stack: Vec<Vec<Annotation>>,

    pub selected_annotation: Option<usize>,
    pub ann_drag: Option<AnnDragState>,

    pub annotations_layer: Vec<Pixmap>,
    pub annotations_dirty: bool,
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub text_editors: HashMap<u64, Editor<'static>>,
    pub text_editing: Option<TextEditState>,

    pub mod_ctrl: bool,
    pub mod_shift: bool,
}
