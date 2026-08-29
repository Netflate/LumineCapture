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
    TextEditState, Toolbar, SettingsPanel, ToolSettings, DoubleClickTracker,
    ClickTarget, ColorPickerPopover,
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
    pub icons_cache: HashMap<&'static str, Tree>,
    pub damage_rects: Vec<DamageZone>,
    
    pub toolbar: Toolbar,
    pub settings_panel: SettingsPanel,
    pub color_popover: ColorPickerPopover,
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

    // Regions of the persistent annotation layers that must be cleared
    // and rebuilt. Separate from `damage_rects`, since layer damage
    // describes what must be rerendered in the cached annotation layer,
    // while damage_rects describes what must be rerendered on the final canvas.
    pub layer_damage_rects: Vec<Rect>,
    pub pending_pen_baked: usize,
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub text_editors: HashMap<u64, Editor<'static>>,
    pub text_editing: Option<TextEditState>,
    pub tool_settings: ToolSettings,
    pub click_tracker: DoubleClickTracker<ClickTarget>,

    pub mod_ctrl: bool,
    pub mod_shift: bool,
}

// types.rs
#[derive(Clone, Copy)]
pub enum DamageZone {
    Global(Rect),
    Local { monitor_idx: usize, rect: Rect },
}

impl EditorState {
    // to avoid revbuilding the entire annotation layer like it was implemented before
    // instead commited annotations are `baked`, so pending new annotations are separate from them 
    // so there will be absolutely no lags while drawing something on top of 10000th circles
    pub fn bake_annotation(&mut self, ann: &Annotation) {
        for (i, placement) in self.placements.iter().enumerate() {
            let offset = (placement.position.0 as f32, placement.position.1 as f32);
            let pad = crate::renderer::visual_pad(ann.stroke_width);
            let visual = Rect::from_ltrb(
                ann.bbox.left() - offset.0 - pad,
                ann.bbox.top() - offset.1 - pad,
                ann.bbox.right() - offset.0 + pad,
                ann.bbox.bottom() - offset.1 + pad,
            );
            let monitor_rect = Rect::from_xywh(0.0, 0.0, placement.size.0 as f32, placement.size.1 as f32);
            if let (Some(vis), Some(mon)) = (visual, monitor_rect) {
                if vis.left() < mon.right() && vis.right() > mon.left() && vis.top() < mon.bottom() && vis.bottom() > mon.top() {
                    crate::renderer::draw_annotation(
                        &mut self.annotations_layer[i],
                        ann,
                        offset,
                        false,
                        &mut self.font_system,
                        &mut self.swash_cache,
                        &mut self.text_editors,
                        None,
                    );
                }
            }
        }
    }
}