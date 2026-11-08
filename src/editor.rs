pub mod blur;
pub mod dirty;
pub mod edits;
pub mod history;
pub mod saved;

use cosmic_text::{Editor, FontSystem, SwashCache};
use std::collections::HashMap;
use std::time::Instant;
use tiny_skia::{Color, PathBuilder, Pixmap, Rect};
use usvg::Tree;

use crate::editor::blur::BlurState;
use crate::interaction::{ClickTarget, DoubleClickTracker};
use crate::tools::Tool;
use crate::tools::selection::{global_selection_to_local, selection_edges_for_monitor};
use crate::types::{
    AnnDragState, Annotation, Capture, Mode, Placement, PointerState, SelectionEdges,
    SelectionState, TextEditState, ToolSettings,
};
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
    pub alt: bool,
    pub straight: bool,
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
            alt: false,
            straight: false,
            clicks: DoubleClickTracker::new(),
        }
    }
}

/// The loupe under the cursor: where it is now and where the last frame drew it.
#[derive(Default)]
pub struct Magnifier {
    pub current: Option<MagnifierState>,
    pub prev: Option<MagnifierState>,
    pub hidden: bool,
}

/// Text shaping and the live editors, one per text annotation.
///
/// Its fields are borrowed apart all over the tools (an editor and the font
/// system at the same time), so this deliberately has no `&mut self` methods.
pub struct TextState {
    pub font_system: FontSystem,
    /// everything not included in Intern font loads up later after launch
    pub system_fonts: Option<std::thread::JoinHandle<cosmic_text::fontdb::Database>>,
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
    pub captures: Vec<Capture>,
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
    pub blur: BlurState,

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
    pub finish: Option<crate::types::Outputs>,
    /// What to do with the screenshot
    /// in the ui, finish adds it alongside the user chioce
    pub to: crate::types::Outputs,
    pub cancel: bool,
    pub to_window: bool,
    pub ui_hidden: bool,
    /// `--region`: no toolbar, releasing a freshly dragged selection finishes the shot.
    pub mode: Mode,
    /// `--output`: explicitely set the output folder path, overriding the one in config
    pub output: Option<std::path::PathBuf>,

    pub ocr: OcrState,
    pub scroll: Option<crate::scroll::Scroll>,
    pub scroll_left: Option<(usize, bool)>,
    pub sheet: Option<crate::edit::Sheet>,
}

// types.rs
#[derive(Clone, Copy)]
pub enum DamageZone {
    Global(Rect),
    Local { monitor_idx: usize, rect: Rect },
}

/// What the capture pipeline has to build before the editor can start.
pub struct Layers {
    pub captures: Vec<Capture>,
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
            captures: layers.captures,
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
            magnifier: Magnifier {
                hidden: crate::editor::saved::get().magnifier == Some(false),
                ..Magnifier::default()
            },

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
            blur: BlurState::default(),
            // Number of points of the current pending Pen already baked into persistent layer
            pending_pen_baked: 0,

            damage_rects: Vec::new(),
            layer_damage_rects: Vec::new(),

            tool_settings: ToolSettings::default(),
            pick_once: false,
            finish: None,
            to: crate::config::get().general.to,
            cancel: false,
            to_window: false,
            ui_hidden: false,
            mode: Mode::Editor,
            output: None,
            scroll: None,
            scroll_left: None,
            sheet: None,
        }
    }

    /// Where annotations start on this monitor: its position.
    pub fn origin(&self, monitor_idx: usize) -> (f32, f32) {
        if let Some(sheet) = &self.sheet {
            return sheet.origin();
        }
        self.placements[monitor_idx].offset()
    }

    pub fn zoom(&self, monitor_idx: usize) -> f32 {
        match &self.sheet {
            Some(sheet) => sheet.zoom(self.placements[monitor_idx].scale),
            None => 1.0,
        }
    }

    pub fn layer_origin(&self, monitor_idx: usize) -> (f32, f32) {
        match &self.sheet {
            Some(sheet) => sheet.layer_origin(),
            None => self.origin(monitor_idx),
        }
    }

    pub fn layer_zoom(&self, monitor_idx: usize) -> f32 {
        match &self.sheet {
            Some(sheet) => sheet.layer_zoom(self.placements[monitor_idx].scale),
            None => self.zoom(monitor_idx),
        }
    }

    pub fn pointer_zoom(&self) -> f32 {
        self.zoom(self.input.pointer.monitor_idx)
    }

    pub fn sources(&self) -> &[Placement] {
        match &self.sheet {
            Some(sheet) => std::slice::from_ref(sheet.placement()),
            None => &self.placements,
        }
    }

    pub fn workspace(&self) -> Option<Rect> {
        crate::utils::get_full_workspace_rect(self.sources())
    }

    pub fn selection_on(&self, monitor_idx: usize, sel: &Rect) -> Option<Rect> {
        let placement = &self.placements[monitor_idx];
        match &self.sheet {
            Some(_) => global_selection_to_local(&self.to_screen(monitor_idx, sel)?, placement),
            None => global_selection_to_local(sel, placement),
        }
    }

    pub fn selection_edges_on(&self, monitor_idx: usize, sel: &Rect) -> SelectionEdges {
        let placement = &self.placements[monitor_idx];
        match self
            .sheet
            .as_ref()
            .and_then(|_| self.to_screen(monitor_idx, sel))
        {
            Some(screen) => selection_edges_for_monitor(&screen, placement),
            None => selection_edges_for_monitor(sel, placement),
        }
    }

    pub fn to_screen(&self, monitor_idx: usize, rect: &Rect) -> Option<Rect> {
        let ((x, y), zoom) = (self.origin(monitor_idx), self.zoom(monitor_idx));
        Rect::from_ltrb(
            (rect.left() - x) * zoom,
            (rect.top() - y) * zoom,
            (rect.right() - x) * zoom,
            (rect.bottom() - y) * zoom,
        )
    }

    pub fn pixel_under_pointer(&self) -> Option<(usize, (f64, f64))> {
        let pointer = &self.input.pointer;
        if self.sheet.is_none() {
            let capture = self.captures.get(pointer.monitor_idx)?;
            return Some((pointer.monitor_idx, capture.to_native(pointer.local)));
        }
        let capture = self.captures.first()?;
        Some((0, capture.to_native(pointer.global)))
    }

    /// Monitors an annotation rect lands on, counting the scroll of the region.
    pub fn monitors_touching(&self, rect: &Rect) -> u32 {
        let mut mask = 0;
        for (i, placement) in self.placements.iter().enumerate() {
            let (x, y) = self.origin(i);
            let zoom = self.zoom(i);
            let (w, h) = (
                placement.size.0 as f32 / zoom,
                placement.size.1 as f32 / zoom,
            );
            if rect.left() < x + w && rect.right() > x && rect.top() < y + h && rect.bottom() > y {
                mask |= 1 << i;
            }
        }
        mask
    }

    pub fn region(&self) -> bool {
        matches!(self.mode, Mode::Region | Mode::Ocr)
    }

    /// Returns true if the user is currently picking a color
    pub fn picking(&self) -> bool {
        self.selected_tool == Tool::Eyedropper || self.pick_once
    }

    pub fn magnifier_shown(&self) -> bool {
        // a live region would catch the loupe in its frames
        (!self.magnifier.hidden || self.picking()) && self.scroll.is_none()
    }

    // to avoid revbuilding the entire annotation layer like it was implemented before
    // instead commited annotations are `baked`, so pending new annotations are separate from them
    // so there will be absolutely no lags while drawing something on top of 10000th circles
    pub fn bake_annotation(&mut self, ann: &Annotation) {
        self.cache_blur(ann, self.annotations.len());
        for (i, placement) in self.placements.iter().enumerate() {
            let offset = self.layer_origin(i);
            let zoom = self.layer_zoom(i);
            let pad = ann.visual_pad();
            let visual = Rect::from_ltrb(
                (ann.bbox.left() - offset.0 - pad) * zoom,
                (ann.bbox.top() - offset.1 - pad) * zoom,
                (ann.bbox.right() - offset.0 + pad) * zoom,
                (ann.bbox.bottom() - offset.1 + pad) * zoom,
            );
            if let (Some(vis), Some(mon)) = (visual, placement.rect())
                && rects_overlap(&vis, &mon)
            {
                crate::renderer::draw_annotation(
                    &mut crate::renderer::Painter::new(&mut self.annotations_layer[i])
                        .with_scale(placement.scale)
                        .with_zoom(zoom)
                        .with_text(&mut self.text.font_system, &mut self.text.swash_cache)
                        .with_origin(offset),
                    ann,
                    false,
                    &mut self.text.editors,
                    &self.blur.cache,
                    None,
                );
            }
        }
    }

    /// where the dragged annotation is drawn, it's out of `annotations` while dragged
    pub fn dragged_index(&self) -> Option<usize> {
        self.ann_drag
            .as_ref()
            .filter(|_| self.pending.is_some())
            .map(|drag| drag.orig_index.min(self.annotations.len()))
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
        let min_x = start
            .0
            .min(end.0)
            .min(control.map(|c| c.0).unwrap_or(start.0))
            - pad;
        let min_y = start
            .1
            .min(end.1)
            .min(control.map(|c| c.1).unwrap_or(start.1))
            - pad;
        let max_x = start
            .0
            .max(end.0)
            .max(control.map(|c| c.0).unwrap_or(start.0))
            + pad;
        let max_y = start
            .1
            .max(end.1)
            .max(control.map(|c| c.1).unwrap_or(start.1))
            + pad;
        let segment_bbox = Rect::from_ltrb(min_x, min_y, max_x, max_y).unwrap_or_else(|| {
            Rect::from_xywh(start.0 - pad, start.1 - pad, pad * 2.0, pad * 2.0).unwrap()
        });

        if let Some(path) = pb.finish() {
            for (i, placement) in self.placements.iter().enumerate() {
                let offset = self.layer_origin(i);
                let zoom = self.layer_zoom(i);
                let visual = Rect::from_ltrb(
                    (segment_bbox.left() - offset.0) * zoom,
                    (segment_bbox.top() - offset.1) * zoom,
                    (segment_bbox.right() - offset.0) * zoom,
                    (segment_bbox.bottom() - offset.1) * zoom,
                );
                if let (Some(vis), Some(mon)) = (visual, placement.rect())
                    && rects_overlap(&vis, &mon)
                {
                    crate::renderer::stroke_pen_segment(
                        &mut crate::renderer::Painter::new(&mut self.annotations_layer[i])
                            .with_scale(placement.scale)
                            .with_zoom(zoom)
                            .with_origin(offset),
                        &path,
                        color,
                        stroke_width,
                    );
                }
            }
        }

        segment_bbox
    }
}
