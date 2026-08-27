use std::collections::HashMap;
use std::time::Instant;
use tiny_skia::{Pixmap, Rect};
use crate::editor::EditorState;
use crate::tools::Tool;
use crate::types::panel::{HoverablePanel, PanelItem, UiPanel};
use crate::types::toolbar::TOOLBAR_OFFSET;
use crate::types::text_field::{is_stepper_char, LineEditState};
use crate::types::SpecialKey;
use crate::types::CursorInit;
use crate::types::annotations::{Annotation, AnnotationShape};

pub const SETTINGS_PANEL_HEIGHT: f32 = 42.0;
pub const SETTINGS_PADDING: f32 = 8.0;
pub const SETTINGS_ITEM_GAP: f32 = 8.0;
pub const SETTINGS_SWATCH_SIZE: f32 = 28.0;
pub const SETTINGS_SEPARATOR_SIZE: f32 = 16.0;
pub const SETTINGS_STEPPER_WIDTH: f32 = 84.0;
pub const SETTINGS_LABEL_FONT_SIZE: f32 = 14.0;

pub const SETTINGS_ICON_BUTTON_SIZE: f32 = SETTINGS_SWATCH_SIZE;
pub const SETTINGS_CHECKBOX_BOX_SIZE: f32 = 18.0;
pub const SETTINGS_CHECKBOX_LABEL_GAP: f32 = 6.0;

pub const STEPPER_ARROW_ZONE: f32 = 30.0;
pub const STEPPER_ARROW_WIDTH: f32 = 15.0;
pub const STEPPER_ARROW_HEIGHT: f32 = 6.0;
pub const STEPPER_ARROW_GAP: f32 = 9.0;
pub const STEPPER_ARROW_STROKE: f32 = 1.6;


#[derive(Debug, Clone, Copy)]
pub enum ToggleVisual {
    Icon { svg: &'static str, icon_size: f32 },
    Checkbox { label: &'static str },
}

#[derive(Debug, Clone)]
pub enum SettingsWidget {
    ColorSwatch,
    Stepper { label: &'static str, min: f32, max: f32, step: f32, unit: &'static str },
    Toggle { visual: ToggleVisual, field: ToggleField },
    Label(&'static str),
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToggleField {
    Bold,
    Italic,
}

pub fn widgets_for_tool(tool: Tool) -> &'static [SettingsWidget] {
    match tool {
        Tool::Pen | Tool::Line | Tool::Arrow => &[
            SettingsWidget::ColorSwatch,
            SettingsWidget::Separator,
            SettingsWidget::Stepper { label: "", min: 1.0, max: 40.0, step: 1.0, unit: "px" },
        ],
        Tool::Text => &[
            SettingsWidget::ColorSwatch,
            SettingsWidget::Separator,
            SettingsWidget::Stepper { label: "", min: 8.0, max: 72.0, step: 1.0, unit: "px" },
            SettingsWidget::Separator,
                SettingsWidget::Toggle { visual: ToggleVisual::Icon { svg: crate::types::icons::BOLD, icon_size: 16.0 }, field: ToggleField::Bold },
                SettingsWidget::Toggle { visual: ToggleVisual::Icon { svg: crate::types::icons::ITALIC, icon_size: 16.0 }, field: ToggleField::Italic },
        ],
        Tool::Rectangle | Tool::Circle => &[
            SettingsWidget::ColorSwatch,
            SettingsWidget::Separator,
            SettingsWidget::Stepper { label: "", min: 1.0, max: 40.0, step: 1.0, unit: "px" },
        ],
        _ => &[],
    }
}

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
            SettingsWidget::ColorSwatch => SETTINGS_SWATCH_SIZE,
            SettingsWidget::Stepper { .. } => SETTINGS_STEPPER_WIDTH,
            SettingsWidget::Toggle { visual, .. } => match visual {
                ToggleVisual::Icon { .. } => SETTINGS_ICON_BUTTON_SIZE,
                ToggleVisual::Checkbox { label } => {
                    SETTINGS_CHECKBOX_BOX_SIZE + SETTINGS_CHECKBOX_LABEL_GAP + label.len() as f32 * 7.0
                }
            },
            SettingsWidget::Label(text) => text.len() as f32 * 7.0 + 8.0,
            SettingsWidget::Separator => SETTINGS_SEPARATOR_SIZE,
        }
    }

    fn trailing_padding(&self) -> f32 {
        match self {
            SettingsWidget::Separator => 0.0,
            _ => SETTINGS_ITEM_GAP,
        }
    }

    fn is_button(&self) -> bool {
        matches!(
            self,
            SettingsWidget::ColorSwatch
                | SettingsWidget::Stepper { .. }
                | SettingsWidget::Toggle { .. }
        )
    }
}

pub struct StepperEdit {
    pub widget_idx: usize,
    pub field: LineEditState,
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
    pub editing: Option<StepperEdit>,
    pub arrow_held: Option<ArrowHoldState>,
    pub values: HashMap<usize, String>,
    pub toggled: HashMap<usize, bool>,
}

impl Default for SettingsPanel {
    fn default() -> Self { Self::new() }
}

impl SettingsPanel {
    pub fn new() -> Self {
        Self {
            widgets: &[],
            active_source: None,
            position: (0.0, 0.0),
            render_pos: (0.0, 0.0),
            size: (0.0, SETTINGS_PANEL_HEIGHT),
            monitor_idx: 0,
            visible: false,
            dirty: true,
            hovered: None,
            hovered_arrow: None,
            selected: None,
            opacity: 1.0,
            panel_pixmap: None,
            editing: None,
            arrow_held: None,
            values: HashMap::new(),
            toggled: HashMap::new(),
        }
    }

    pub fn hit_test(&self, local: (f64, f64)) -> Option<usize> {
        let rect = self.rect()?;
        let px = local.0 as f32;
        let py = local.1 as f32;

        if py < rect.top() || py > rect.bottom() || px < rect.left() || px > rect.right() {
            return None;
        }

        let mut current_x = rect.left() + SETTINGS_PADDING;
        for (idx, widget) in self.widgets.iter().enumerate() {
            let w = widget.size();
            let right = current_x + w;
            if px >= current_x && px <= right {
                return if widget.is_button() { Some(idx) } else { None };
            }
            current_x += w + widget.trailing_padding();
        }
        None
    }

    fn widget_local_rect(&self, widget_idx: usize) -> Option<(f32, f32, f32, f32)> {
        let rect = self.rect()?;
        let h = self.size.1;
        let item_h = h * 0.70;
        let item_y = rect.top() + (h - item_h) / 2.0;

        let mut current_x = rect.left() + SETTINGS_PADDING;
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
        if !matches!(self.widgets.get(widget_idx), Some(SettingsWidget::Stepper { .. })) {
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
        Some(if py < mid_y { StepperArrow::Up } else { StepperArrow::Down })
    }

    // ── editing input fields ─────────────────────────────────────────
    pub fn begin_edit(&mut self, widget_idx: usize, initial_text: String, cursor: CursorInit) {
        let mut field = LineEditState::new(initial_text);
        match cursor {
            CursorInit::End => field.move_end(false),
            CursorInit::SelectAll => field.select_all(),
            CursorInit::At(idx) => {
                let len = field.text.chars().count();
                field.cursor = idx.min(len);
                field.selection_anchor = None;
            }
        }
        self.editing = Some(StepperEdit { widget_idx, field });
        self.dirty = true;
    }

    pub fn cancel_edit(&mut self) {
        if self.editing.take().is_some() {
            self.dirty = true;
        }
    }

    pub fn commit_edit(&mut self) -> Option<(usize, String)> {
        let edit = self.editing.take()?;
        self.dirty = true;
        Some((edit.widget_idx, edit.field.text))
    }

    pub fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    pub fn insert_char(&mut self, ch: char) -> bool {
        let Some(edit) = self.editing.as_mut() else { return false };
        if !is_stepper_char(ch) {
            return false;
        }
        edit.field.insert(ch);
        self.dirty = true;
        true
    }

    pub fn handle_key(&mut self, key: SpecialKey, ctrl: bool, shift: bool) -> (bool, bool) {
        let Some(edit) = self.editing.as_mut() else { return (false, false) };

        if ctrl {
            match key {
                SpecialKey::KeyA => {
                    edit.field.select_all();
                    self.dirty = true;
                    return (true, false);
                }
                SpecialKey::KeyC | SpecialKey::KeyX => {
                    if let Some(sel) = edit.field.selected_text() {
                        crate::utils::copy_to_clipboard(&sel);
                    }
                    if matches!(key, SpecialKey::KeyX) {
                        edit.field.backspace_selection_only();
                        self.dirty = true;
                        return (true, false);
                    }
                    return (false, false);
                }
                SpecialKey::KeyV => {
                    if let Some(text) = crate::utils::paste_from_clipboard() {
                        let filtered: String =
                            text.chars().filter(|c| is_stepper_char(*c)).collect();
                        if !filtered.is_empty() {
                            edit.field.insert_str(&filtered);
                            self.dirty = true;
                            return (true, false);
                        }
                    }
                    return (false, false);
                }
                _ => {}
            }
        }

        let result = match key {
            SpecialKey::Enter => (false, true),
            SpecialKey::Left => { edit.field.move_left(shift); (true, false) }
            SpecialKey::Right => { edit.field.move_right(shift); (true, false) }
            SpecialKey::Home => { edit.field.move_home(shift); (true, false) }
            SpecialKey::End => { edit.field.move_end(shift); (true, false) }
            SpecialKey::Backspace => { edit.field.backspace(); (true, false) }
            SpecialKey::Delete => { edit.field.delete_forward(); (true, false) }
            _ => (false, false),
        };

        if result.0 {
            self.dirty = true;
        }
        result
    }

    pub fn sync_value(&mut self, idx: usize, text: String) {
        if self.editing.as_ref().map(|e| e.widget_idx) == Some(idx) {
            return;
        }
        self.values.insert(idx, text);
    }
    pub fn widget_text_x(&self, widget_idx: usize) -> Option<f32> {
        let rect = self.rect()?;
        let mut current_x = rect.left() + SETTINGS_PADDING;
        for (idx, widget) in self.widgets.iter().enumerate() {
            if idx == widget_idx {
                return Some(current_x + SETTINGS_PADDING);
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
}

impl UiPanel for SettingsPanel {
    type Item = SettingsWidget;

    fn render_pos(&self) -> (f32, f32) { self.render_pos }
    fn size(&self) -> (f32, f32) { self.size }
    fn items(&self) -> &[Self::Item] { self.widgets }
    fn padding(&self) -> f32 { SETTINGS_PADDING }
    fn monitor_idx(&self) -> usize { self.monitor_idx }
    fn set_dirty(&mut self) { self.dirty = true; }

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

    fn hovered(&self) -> Self::Hover { (self.hovered, self.hovered_arrow) }
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
    let is_above = (target_y - panel_h - TOOLBAR_OFFSET) >= 0.0;

    let (render_x, render_y) = tb.render_pos;

    let y = if is_above {
        render_y - panel_h - TOOLBAR_OFFSET
    } else {
        render_y + tb_h + TOOLBAR_OFFSET
    };

    ((render_x, y), tb.monitor_idx)
}