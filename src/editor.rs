pub mod dirty;
pub mod edits;
pub mod history;

use cosmic_text::{Editor, FontSystem, SwashCache};
use std::collections::HashMap;
use std::time::Instant;
use tiny_skia::{Color, PathBuilder, Pixmap, Rect};
use usvg::Tree;

use crate::tools::Tool;
use crate::types::{AnnDragState, Annotation, Placement, PointerState, SelectionState, TextEditState, ToolSettings};
use crate::interaction::{ClickTarget, DoubleClickTracker};
use crate::ui::color_popover::ColorPickerPopover;
use crate::ui::magnifier::MagnifierState;
use crate::ui::settings_panel::SettingsPanel;
use crate::ui::toolbar::Toolbar;
use crate::utils::rects_overlap;

/// Pointer and keyboard state of the current gesture.
pub struct InputState {
    pub pointer: PointerState,
    pub drag_start: Option<(f64, f64)>,
    pub mouse_down: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub clicks: DoubleClickTracker<ClickTarget>,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            pointer: PointerState::default(),
            drag_start: None,
            mouse_down: false,
            ctrl: false,
            shift: false,
            clicks: DoubleClickTracker::new(),
        }
    }
}

/// The loupe under the cursor: where it is now, where it was, and when it moved.
#[derive(Default)]
pub struct Magnifier {
    pub current: Option<MagnifierState>,
    pub prev: Option<MagnifierState>,
    pub last_update: Option<Instant>,
}

/// Text shaping and the live editors, one per text annotation.
///
/// Its fields are borrowed apart all over the tools (an editor and the font
/// system at the same time), so this deliberately has no `&mut self` methods.
pub struct TextState {
    pub font_system: FontSystem,
    pub swash_cache: SwashCache,
    pub editors: HashMap<u64, Editor<'static>>,
    pub editing: Option<TextEditState>,
}

/// The OCR engine, the installed models and the result on screen.
pub struct OcrState {
    pub runtime: crate::ocr::OcrRuntime,
    pub models: crate::ocr::models::OcrModels,
    pub view: crate::ocr::OcrView,
    /// Set when the user drags to select a new OCR area. It saves the original
    /// selection from before the drag started.
    ///
    /// When the drag ends, OCR only re-runs if the box size actually changed.
    /// This prevents a simple click from clearing the current result.
    pub redrag: bool,
    pub redrag_from: Option<Rect>,
    pub await_region: bool,
    /// When the running recognition started, driving the progress badge's spin.
    /// `None` whenever nothing is in flight.
    pub scan_started: Option<Instant>,
}

pub struct EditorState {
    pub base: Vec<Pixmap>,
    pub native: Vec<Option<Pixmap>>,
    pub canvas: Vec<Pixmap>,
    pub dimmed: Vec<Pixmap>,
    pub placements: Vec<Placement>,
    pub selected_tool: Tool,
    pub tool_active: bool,
    pub input: InputState,
    pub magnifier: Magnifier,
    pub selection: SelectionState,
    pub icons_cache: HashMap<&'static str, Tree>,
    pub damage_rects: Vec<DamageZone>,

    pub toolbar: Toolbar,
    pub settings_panel: SettingsPanel,
    pub color_popover: ColorPickerPopover,
    pub model_popover: crate::ui::model_popover::ModelPopover,
    pub toasts: crate::ui::toast::Toasts,
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
    pub text: TextState,
    pub tool_settings: ToolSettings,

    /// Set to true when the user is about to pick a color from the palette, since the action is a one-time thing
    pub pick_once: bool,
    pub finish: Option<crate::types::Finish>,
    pub cancel: bool,
    pub ui_hidden: bool,

    pub ocr: OcrState,
}

// types.rs
#[derive(Clone, Copy)]
pub enum DamageZone {
    Global(Rect),
    Local { monitor_idx: usize, rect: Rect },
}

/// What the capture pipeline has to build before the editor can start.
pub struct Layers {
    pub base: Vec<Pixmap>,
    pub native: Vec<Option<Pixmap>>,
    pub canvas: Vec<Pixmap>,
    pub dimmed: Vec<Pixmap>,
    pub annotations: Vec<Pixmap>,
}

impl EditorState {
    pub fn new(
        layers: Layers,
        placements: Vec<Placement>,
        icons_cache: HashMap<&'static str, Tree>,
        text: TextState,
        ocr: OcrState,
    ) -> Self {
        Self {
            base: layers.base,
            native: layers.native,
            canvas: layers.canvas,
            dimmed: layers.dimmed,
            annotations_layer: layers.annotations,
            placements,
            icons_cache,
            text,
            ocr,

            selected_tool: Tool::Selection,
            tool_active: false,
            selection: SelectionState::default(),
            input: InputState::default(),
            magnifier: Magnifier::default(),

            toolbar: Toolbar::new(),
            settings_panel: SettingsPanel::new(),
            color_popover: ColorPickerPopover::new(),
            model_popover: crate::ui::model_popover::ModelPopover::new(),
            toasts: crate::ui::toast::Toasts::default(),

            annotations: Vec::new(),
            pending: None,
            prev_pending: None,
            next_id: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            selected_annotation: None,
            ann_drag: None,
            annotations_dirty: false,
            // Number of points of the current pending Pen already baked into persistent layer
            pending_pen_baked: 0,

            damage_rects: Vec::new(),
            layer_damage_rects: Vec::new(),

            tool_settings: ToolSettings::default(),
            pick_once: false,
            finish: None,
            cancel: false,
            ui_hidden: false,
        }
    }

    /// Returns true if the user is currently picking a color
    pub fn picking(&self) -> bool {
        self.selected_tool == Tool::Eyedropper || self.pick_once
    }

    // to avoid revbuilding the entire annotation layer like it was implemented before
    // instead commited annotations are `baked`, so pending new annotations are separate from them
    // so there will be absolutely no lags while drawing something on top of 10000th circles
    pub fn bake_annotation(&mut self, ann: &Annotation) {
        for (i, placement) in self.placements.iter().enumerate() {
            let offset = placement.offset();
            let pad = crate::renderer::visual_pad(ann.stroke_width);
            let visual = Rect::from_ltrb(
                ann.bbox.left() - offset.0 - pad,
                ann.bbox.top() - offset.1 - pad,
                ann.bbox.right() - offset.0 + pad,
                ann.bbox.bottom() - offset.1 + pad,
            );
            if let (Some(vis), Some(mon)) = (visual, placement.rect())
                && rects_overlap(&vis, &mon)
                {
                    crate::renderer::draw_annotation(
                        &mut self.annotations_layer[i],
                        ann,
                        offset,
                        false,
                        &mut self.text.font_system,
                        &mut self.text.swash_cache,
                        &mut self.text.editors,
                        None,
                    );
                }
        }
    }

    pub fn bake_pen_segment(
        &mut self,
        start: (f32, f32),
        control: Option<(f32, f32)>,
        end: (f32, f32),
        color: Color,
        stroke_width: f32,
    ) -> Rect {
        let mut pb = PathBuilder::new();
        pb.move_to(start.0, start.1);
        if let Some(ctrl) = control {
            pb.quad_to(ctrl.0, ctrl.1, end.0, end.1);
        } else {
            pb.line_to(end.0, end.1);
        }

        let pad = crate::renderer::visual_pad(stroke_width);
        let min_x = start.0.min(end.0).min(control.map(|c| c.0).unwrap_or(start.0)) - pad;
        let min_y = start.1.min(end.1).min(control.map(|c| c.1).unwrap_or(start.1)) - pad;
        let max_x = start.0.max(end.0).max(control.map(|c| c.0).unwrap_or(start.0)) + pad;
        let max_y = start.1.max(end.1).max(control.map(|c| c.1).unwrap_or(start.1)) + pad;
        let segment_bbox = Rect::from_ltrb(min_x, min_y, max_x, max_y).unwrap_or_else(|| {
            Rect::from_xywh(start.0 - pad, start.1 - pad, pad * 2.0, pad * 2.0).unwrap()
        });

        if let Some(path) = pb.finish() {
            for (i, placement) in self.placements.iter().enumerate() {
                let offset = placement.offset();
                let visual = Rect::from_ltrb(
                    segment_bbox.left() - offset.0,
                    segment_bbox.top() - offset.1,
                    segment_bbox.right() - offset.0,
                    segment_bbox.bottom() - offset.1,
                );
                if let (Some(vis), Some(mon)) = (visual, placement.rect())
                    && rects_overlap(&vis, &mon)
                    {
                        crate::renderer::stroke_pen_segment(
                            &mut self.annotations_layer[i],
                            &path,
                            color,
                            stroke_width,
                            offset,
                        );
                    }
            }
        }

        segment_bbox
    }
}
