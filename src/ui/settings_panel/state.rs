use crate::editor::EditorState;
use crate::tools::Tool;
use crate::ui::text_field::CursorInit;
use crate::types::SpecialKey;
use crate::types::annotations::{Annotation, AnnotationShape};
use crate::interaction::ScrollAccumulator;
use crate::ui::panel::{HoverablePanel, PanelItem, UiPanel};
use crate::ui::text_field::{TextFieldGroup, is_stepper_char};
use crate::ui::toolbar;
use tiny_skia::{Pixmap, Rect};

use std::collections::HashMap;
use std::time::Instant;

pub const HEIGHT: f32 = 42.0;
pub const PADDING: f32 = 8.0;
pub const ITEM_GAP: f32 = 8.0;
pub const SWATCH_SIZE: f32 = 28.0;
pub const SEPARATOR_SIZE: f32 = 16.0;
pub const STEPPER_WIDTH: f32 = 84.0;
pub const DOWNLOAD_WIDTH: f32 = 196.0;
pub const VALUE_HEX_WIDTH: f32 = 84.0;
pub const VALUE_RGB_WIDTH: f32 = 120.0;
pub const DOWNLOAD_LABEL_WIDTH: f32 = 96.0;
pub const DOWNLOAD_PERCENT_WIDTH: f32 = 38.0;

pub const ICON_BUTTON_SIZE: f32 = SWATCH_SIZE;
pub const CHECKBOX_BOX_SIZE: f32 = 18.0;
pub const CHECKBOX_LABEL_GAP: f32 = 6.0;

pub const STEPPER_ARROW_ZONE: f32 = 30.0;
pub const STEPPER_ARROW_WIDTH: f32 = 15.0;
pub const STEPPER_ARROW_HEIGHT: f32 = 6.0;
pub const STEPPER_ARROW_GAP: f32 = 9.0;
pub const STEPPER_ARROW_STROKE: f32 = 1.6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueField {
    Hex,
    Rgb,
}

impl ValueField {
    pub fn text(self, color: tiny_skia::Color) -> String {
        let c = color.to_color_u8();
        match self {
            ValueField::Hex => format!("#{}", crate::ui::color_popover::color_to_hex_string(color)),
            ValueField::Rgb => format!("{}, {}, {}", c.red(), c.green(), c.blue()),
        }
    }

    fn width(self) -> f32 {
        match self {
            ValueField::Hex => VALUE_HEX_WIDTH,
            ValueField::Rgb => VALUE_RGB_WIDTH,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ToggleVisual {
    Icon { svg: &'static str, icon_size: f32 },
    Checkbox { label: &'static str },
}

/// A one-shot action button. No value of its own, it just runs
/// something. see aswell `settings_logic::run_settings_action`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsAction {
    OcrRescan,
    OcrCopyAll,
    OcrLanguages,
}

#[derive(Debug, Clone)]
pub enum SettingsWidget {
    ColorSwatch,
    Action {
        action: SettingsAction,
        svg: &'static str,
        icon_size: f32,
    },
    Stepper {
        label: &'static str,
        min: f32,
        max: f32,
        step: f32,
        unit: &'static str,
    },
    Toggle {
        visual: ToggleVisual,
        field: ToggleField,
    },
    Label(&'static str),
    Value {
        field: ValueField,
    },
    Download,
    Separator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepperArrow {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrowHoldState {
    pub widget_idx: usize,
    pub arrow: StepperArrow,
    pub started_at: Instant,
    pub last_step_at: Instant,
    pub repeat_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSource {
    Tool(Tool),
    Annotation(u64),
    OcrScanning,
    OcrAwaiting { downloading: bool }, // <- waiting for drag
    OcrNoModel { downloading: bool },
    OcrResult { downloading: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleField {
    Bold,
    Italic,
}

pub fn widgets_for_tool(tool: Tool) -> &'static [SettingsWidget] {
    match tool {
        Tool::Pen | Tool::Line | Tool::Arrow | Tool::NumeratedArrow => &[
            SettingsWidget::ColorSwatch,
            SettingsWidget::Separator,
            SettingsWidget::Stepper {
                label: "",
                min: 1.0,
                max: 40.0,
                step: 1.0,
                unit: "px",
            },
        ],
        Tool::Text => &[
            SettingsWidget::ColorSwatch,
            SettingsWidget::Separator,
            SettingsWidget::Stepper {
                label: "",
                min: 8.0,
                max: 72.0,
                step: 1.0,
                unit: "px",
            },
            SettingsWidget::Separator,
            SettingsWidget::Toggle {
                visual: ToggleVisual::Icon {
                    svg: crate::ui::icons::BOLD,
                    icon_size: 16.0,
                },
                field: ToggleField::Bold,
            },
            SettingsWidget::Toggle {
                visual: ToggleVisual::Icon {
                    svg: crate::ui::icons::ITALIC,
                    icon_size: 16.0,
                },
                field: ToggleField::Italic,
            },
        ],
        Tool::Rectangle | Tool::Circle => &[
            SettingsWidget::ColorSwatch,
            SettingsWidget::Separator,
            SettingsWidget::Stepper {
                label: "",
                min: 1.0,
                max: 40.0,
                step: 1.0,
                unit: "px",
            },
        ],
        Tool::Eyedropper => EYEDROPPER_WIDGETS,
        Tool::Ocr => OCR_WIDGETS,
        _ => &[],
    }
}

pub const EYEDROPPER_WIDGETS: &[SettingsWidget] = &[
    SettingsWidget::Value { field: ValueField::Hex },
    SettingsWidget::Value { field: ValueField::Rgb },
    SettingsWidget::Separator,
    SettingsWidget::Label("Click anywhere to pick a color"),
];

/// What the panel offers once text has been read. The hint is the answer to
/// "how do I read something else": dragging on empty space inside the tool
/// boxes out a new region - on this screen or any other - and reads it on
/// release, so nothing has to be re-picked from the toolbar.
pub const OCR_WIDGETS: &[SettingsWidget] = &[
    OCR_LANGUAGES,
    SettingsWidget::Separator,
    OCR_RESCAN,
    OCR_COPY_ALL,
    SettingsWidget::Separator,
    SettingsWidget::Label("Drag a new box to read another area"),
];

/// when downloading
pub const OCR_WIDGETS_DOWNLOADING: &[SettingsWidget] = &[
    OCR_LANGUAGES,
    SettingsWidget::Separator,
    OCR_RESCAN,
    OCR_COPY_ALL,
    SettingsWidget::Separator,
    SettingsWidget::Download,
];

/// Shown while the worker thread is busy; the progress badge sits over the
/// region itself, this just keeps the panel from advertising dead buttons.
pub const OCR_SCANNING_WIDGETS: &[SettingsWidget] = &[SettingsWidget::Label("Reading text...")];

pub const OCR_AWAITING_WIDGETS: &[SettingsWidget] = &[OCR_LANGUAGES];

pub const OCR_NO_MODEL_WIDGETS: &[SettingsWidget] = &[
    OCR_LANGUAGES,
    SettingsWidget::Separator,
    SettingsWidget::Label("Choose a language model to use OCR"),
];

pub const OCR_DOWNLOADING_WIDGETS: &[SettingsWidget] = &[
    OCR_LANGUAGES,
    SettingsWidget::Separator,
    SettingsWidget::Download,
];

const OCR_LANGUAGES: SettingsWidget = SettingsWidget::Action {
    action: SettingsAction::OcrLanguages,
    svg: crate::ui::icons::GLOBE,
    icon_size: 16.0,
};

const OCR_RESCAN: SettingsWidget = SettingsWidget::Action {
    action: SettingsAction::OcrRescan,
    svg: crate::ui::icons::RETRY,
    icon_size: 16.0,
};

const OCR_COPY_ALL: SettingsWidget = SettingsWidget::Action {
    action: SettingsAction::OcrCopyAll,
    svg: crate::ui::icons::COPY,
    icon_size: 15.0,
};

pub fn widgets_for_annotation(ann: &Annotation) -> &'static [SettingsWidget] {
    match &ann.shape {
        AnnotationShape::Text { .. } => widgets_for_tool(Tool::Text),
        AnnotationShape::Rectangle { .. } | AnnotationShape::Circle { .. } => {
            widgets_for_tool(Tool::Rectangle)
        }
        AnnotationShape::Pen { .. }
        | AnnotationShape::Line { .. }
        | AnnotationShape::Arrow { .. }
        | AnnotationShape::NumeratedArrow { .. } => widgets_for_tool(Tool::Pen),
    }
}

impl PanelItem for SettingsWidget {
    fn size(&self) -> f32 {
        match self {
            SettingsWidget::ColorSwatch => SWATCH_SIZE,
            SettingsWidget::Action { .. } => ICON_BUTTON_SIZE,
            SettingsWidget::Stepper { .. } => STEPPER_WIDTH,
            SettingsWidget::Toggle { visual, .. } => match visual {
                ToggleVisual::Icon { .. } => ICON_BUTTON_SIZE,
                ToggleVisual::Checkbox { label } => {
                    CHECKBOX_BOX_SIZE
                        + CHECKBOX_LABEL_GAP
                        + label.len() as f32 * 7.0
                }
            },
            SettingsWidget::Label(text) => text.chars().count() as f32 * 7.0 + 8.0,
            SettingsWidget::Value { field } => field.width(),
            SettingsWidget::Download => DOWNLOAD_WIDTH,
            SettingsWidget::Separator => SEPARATOR_SIZE,
        }
    }

    fn trailing_padding(&self) -> f32 {
        match self {
            SettingsWidget::Separator => 0.0,
            _ => ITEM_GAP,
        }
    }

    fn is_button(&self) -> bool {
        matches!(
            self,
            SettingsWidget::ColorSwatch
                | SettingsWidget::Action { .. }
                | SettingsWidget::Stepper { .. }
                | SettingsWidget::Toggle { .. }
                | SettingsWidget::Value { .. }
        )
    }
}

pub struct SettingsPanel {
    pub widgets: &'static [SettingsWidget],
    pub active_source: Option<SettingsSource>,
    pub position: (f32, f32),
    pub render_pos: (f32, f32),
    pub size: (f32, f32),
    pub monitor_idx: usize,
    pub visible: bool,
    pub dirty: bool,
    pub hovered: Option<usize>,
    pub hovered_arrow: Option<(usize, StepperArrow)>,
    pub selected: Option<usize>,
    pub opacity: f32,
    pub panel_pixmap: Option<Pixmap>,
    pub fields: TextFieldGroup<usize>,
    pub arrow_held: Option<ArrowHoldState>,
    pub toggled: HashMap<usize, bool>,
    pub pre_edit_snapshot: Option<Vec<Annotation>>,
    /// % of download progress if any
    pub download: Option<u8>,

    /// Scroll accumulator for scrollable fields (hex/rgba)
    /// Keeps scroll fractional state separate from raw events to avoid
    /// processing every single scroll event, trackpad or mouse wheel held
    /// will flood thousands of events that would freeze 
    pub scroll: ScrollAccumulator<usize>,
}

impl Default for SettingsPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl SettingsPanel {
    pub fn new() -> Self {
        Self {
            widgets: &[],
            active_source: None,
            position: (0.0, 0.0),
            render_pos: (0.0, 0.0),
            size: (0.0, HEIGHT),
            monitor_idx: 0,
            visible: false,
            dirty: true,
            hovered: None,
            hovered_arrow: None,
            selected: None,
            opacity: 1.0,
            panel_pixmap: None,
            fields: TextFieldGroup::new(),
            arrow_held: None,
            toggled: HashMap::new(),
            scroll: ScrollAccumulator::new(),
            pre_edit_snapshot: None,
            download: None,
        }
    }

    pub fn hit_test(&self, local: (f64, f64)) -> (bool, Option<usize>) {
        let Some(rect) = self.rect() else {
            return (false, None);
        };
        let px = local.0 as f32;
        let py = local.1 as f32;

        if py < rect.top() || py > rect.bottom() || px < rect.left() || px > rect.right() {
            return (false, None);
        }

        let mut current_x = rect.left() + PADDING;
        for (idx, widget) in self.widgets.iter().enumerate() {
            let w = widget.size();
            let right = current_x + w;
            if px >= current_x && px <= right {
                return if widget.is_button() { (true, Some(idx)) } else { (true, None) };
            }
            current_x += w + widget.trailing_padding();
        }
        (true, None)
    }

    fn widget_local_rect(&self, widget_idx: usize) -> Option<(f32, f32, f32, f32)> {
        let rect = self.rect()?;
        let h = self.size.1;
        let item_h = h * 0.70;
        let item_y = rect.top() + (h - item_h) / 2.0;

        let mut current_x = rect.left() + PADDING;
        for (idx, widget) in self.widgets.iter().enumerate() {
            let w = widget.size();
            if idx == widget_idx {
                return Some((current_x, item_y, w, item_h));
            }
            current_x += w + widget.trailing_padding();
        }
        None
    }

    pub fn stepper_arrow_hit(&self, widget_idx: usize, local: (f64, f64)) -> Option<StepperArrow> {
        if !matches!(
            self.widgets.get(widget_idx),
            Some(SettingsWidget::Stepper { .. })
        ) {
            return None;
        }
        let (item_x, item_y, item_w, item_h) = self.widget_local_rect(widget_idx)?;

        let px = local.0 as f32;
        let py = local.1 as f32;

        let zone_left = item_x + item_w - STEPPER_ARROW_ZONE;
        if px < zone_left || px > item_x + item_w || py < item_y || py > item_y + item_h {
            return None;
        }

        let mid_y = item_y + item_h / 2.0;
        Some(if py < mid_y {
            StepperArrow::Up
        } else {
            StepperArrow::Down
        })
    }

    // ── editing input fields ─────────────────────────────────────────
    // wrapper around TextFieldGroup: same external signature, but with dirty flag management
    pub fn begin_edit(&mut self, widget_idx: usize, initial_text: String, cursor: CursorInit) {
        self.fields.begin_edit(widget_idx, initial_text, cursor);
        self.dirty = true;
    }

    pub fn cancel_edit(&mut self) {
        if self.fields.cancel_edit() {
            self.dirty = true;
        }
    }

    pub fn commit_edit(&mut self) -> Option<(usize, String)> {
        let result = self.fields.commit_edit();
        if result.is_some() {
            self.dirty = true;
        }
        result
    }

    pub fn is_editing(&self) -> bool {
        self.fields.is_editing()
    }

    pub fn insert_char(&mut self, ch: char) -> bool {
        let inserted = self.fields.insert_char(ch, is_stepper_char);
        if inserted {
            self.dirty = true;
        }
        inserted
    }

    pub fn handle_key(&mut self, key: SpecialKey, ctrl: bool, shift: bool) -> (bool, bool) {
        let result = self.fields.handle_key(key, ctrl, shift, is_stepper_char);
        if result.0 {
            self.dirty = true;
        }
        result
    }

    pub fn sync_value(&mut self, idx: usize, text: String) {
        self.fields.sync_value(idx, text);
    }

    pub fn widget_text_x(&self, widget_idx: usize) -> Option<f32> {
        let rect = self.rect()?;
        let mut current_x = rect.left() + PADDING;
        for (idx, widget) in self.widgets.iter().enumerate() {
            if idx == widget_idx {
                return Some(current_x + PADDING);
            }
            current_x += widget.size() + widget.trailing_padding();
        }
        None
    }

    pub fn is_toggled(&self, idx: usize) -> bool {
        self.toggled.get(&idx).copied().unwrap_or(false)
    }
    pub fn set_toggled(&mut self, idx: usize, value: bool) {
        self.toggled.insert(idx, value);
        self.dirty = true;
    }
    pub fn toggle(&mut self, idx: usize) -> bool {
        let v = !self.is_toggled(idx);
        self.set_toggled(idx, v);
        v
    }

    /// Accumulate a fractional scroll `delta` (already in "steps", not raw
    /// pixels) for the given field and return how many whole steps have
    /// accumulated since the last call (may be negative). Switching fields
    /// discards the leftover from the previous one.
    ///
    /// Delegates to [`ScrollAccumulator`], see its docs for the two
    /// flood-prevention safeguards (rate limit + step cap)
    pub fn scroll_step(&mut self, widget_idx: usize, delta: f32) -> i32 {
        self.scroll.step(widget_idx, delta)
    }

    /// scroll reset. Call on any non-scroll user action
    /// (click, key press) so stale queued events don't trigger afterwards
    pub fn cancel_scroll(&mut self) {
        self.scroll.cancel();
    }
}

impl UiPanel for SettingsPanel {
    type Item = SettingsWidget;

    fn render_pos(&self) -> (f32, f32) {
        self.render_pos
    }
    fn size(&self) -> (f32, f32) {
        self.size
    }
    fn items(&self) -> &[Self::Item] {
        self.widgets
    }
    fn padding(&self) -> f32 {
        PADDING
    }
    fn monitor_idx(&self) -> usize {
        self.monitor_idx
    }
    fn set_dirty(&mut self) {
        self.dirty = true;
    }

    fn rect(&self) -> Option<Rect> {
        if !self.visible {
            return None;
        }
        let (x, y) = self.render_pos();
        let (w, h) = self.size();
        Rect::from_xywh(x, y, w, h)
    }
}

impl HoverablePanel for SettingsPanel {
    type Hover = (Option<usize>, Option<(usize, StepperArrow)>);

    fn hovered(&self) -> Self::Hover {
        (self.hovered, self.hovered_arrow)
    }
    fn set_hovered(&mut self, hover: Self::Hover) {
        self.hovered = hover.0;
        self.hovered_arrow = hover.1;
    }
}

// ── Placement ─────────────────────────────────────────

pub fn compute_settings_placement(editor_state: &EditorState) -> ((f32, f32), usize) {
    let tb = &editor_state.toolbar;
    let panel_h = editor_state.settings_panel.size.1;
    let tb_h = tb.size.1;

    let target_y = tb.position.1;
    let is_above = (target_y - panel_h - toolbar::OFFSET) >= 0.0;

    let (render_x, render_y) = tb.render_pos;

    let y = if is_above {
        render_y - panel_h - toolbar::OFFSET
    } else {
        render_y + tb_h + toolbar::OFFSET
    };

    ((render_x, y), tb.monitor_idx)
}