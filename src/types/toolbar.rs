use crate::tools::Tool;
use crate::types::panel::{PanelItem, UiPanel};
use crate::types::Placement;
use std::time::Instant;
use tiny_skia::Pixmap;

// ==========================================
// 1. UI Layout Constants
// ==========================================
pub const TOOLBAR_ANIM_INTERVAL: std::time::Duration = std::time::Duration::from_millis(16);
pub const TOOLBAR_ANIM_DT: f32 = 0.016;
pub const TOOLBAR_TRANSITION_OFFSET: f32 = 340.0; // max cap on the transition entrance distance

pub const TOOLBAR_HEIGHT: f32 = 42.0;
pub const TOOLBAR_OFFSET: f32 = 5.0;
pub const TOOLBAR_PADDING: f32 = 8.0; // left & right

pub const BUTTON_CELL_SIZE: f32 = 35.0;
const SEPARATOR_CELL_SIZE: f32 = 20.0;

const fn unit(v: u8) -> f32 {
    v as f32 / 255.0
}

pub const TOOLBAR_COLOR: tiny_skia::Color = unsafe {
    tiny_skia::Color::from_rgba_unchecked(unit(13), unit(13), unit(23), unit(240))
};
pub const SEPARATOR_COLOR: tiny_skia::Color = unsafe {
    tiny_skia::Color::from_rgba_unchecked(unit(255), unit(255), unit(255), unit(255))
};
pub const BUTTON_HOVERED: tiny_skia::Color = unsafe {
    tiny_skia::Color::from_rgba_unchecked(unit(159), unit(48), unit(215), unit(255))
};
pub const BUTTON_SELECTED: tiny_skia::Color = unsafe {
    tiny_skia::Color::from_rgba_unchecked(unit(133), unit(44), unit(177), unit(255))
};

pub const ICON_COLOR: usvg::Color = usvg::Color { red: 255, green: 255, blue: 255 };

// Toolbar tools list
pub const TOOLBAR_ITEMS: &[ToolbarItem] = &[
    ToolbarItem::Button(ToolbarButton::Tool(Tool::Selection)),
    ToolbarItem::Button(ToolbarButton::Tool(Tool::Pick)),
    ToolbarItem::Seperator,
    ToolbarItem::Button(ToolbarButton::Tool(Tool::Text)),
    ToolbarItem::Button(ToolbarButton::Tool(Tool::Pen)),
    ToolbarItem::Button(ToolbarButton::Tool(Tool::Line)),
    ToolbarItem::Button(ToolbarButton::Tool(Tool::Arrow)),
    ToolbarItem::Button(ToolbarButton::Tool(Tool::Rectangle)),
    ToolbarItem::Button(ToolbarButton::Tool(Tool::Circle)),
    ToolbarItem::Button(ToolbarButton::Tool(Tool::NumeratedArrow)),
];

// ==========================================
// 2. Component State
// ==========================================
pub struct Toolbar {
    pub toolbar_pixmap: Option<Pixmap>, // to change its opacity (SVG icons limitation)

    pub items: &'static [ToolbarItem],
    pub position: (f32, f32),
    pub size: (f32, f32),
    pub opacity: f32,
    pub monitor_idx: usize,

    pub dirty: bool,

    pub selected: Option<usize>,
    pub hovered: Option<usize>,

    pub interferes: bool,
    pub last_tick: Option<Instant>,
    pub render_pos: (f32, f32),
    pub placement_kind: Option<ToolbarPlacementKind>,
}

impl Default for Toolbar {
    fn default() -> Self {
        Self::new()
    }
}

impl UiPanel for Toolbar {
    type Item = ToolbarItem;

    fn render_pos(&self) -> (f32, f32) { self.render_pos }
    fn size(&self) -> (f32, f32) { self.size }
    fn items(&self) -> &[Self::Item] { self.items }
    fn padding(&self) -> f32 { TOOLBAR_PADDING }
}

impl Toolbar {
    pub fn new() -> Self {
        let mut toolbar = Self {
            toolbar_pixmap: None,
            items: TOOLBAR_ITEMS,
            size: (0.0, TOOLBAR_HEIGHT),
            opacity: 1.0,
            monitor_idx: 0,
            position: (0.0, 0.0),

            dirty: false,
            selected: Some(0),
            hovered: None,

            interferes: false,
            last_tick: None,
            render_pos: (0.0, 0.0),
            placement_kind: None,
        };

        toolbar.size.0 = toolbar.width();
        toolbar.size.1 = TOOLBAR_HEIGHT;

        toolbar
    }

    pub fn get_selected_tool(&self) -> Option<&Tool> {
        self.selected
            .and_then(|idx| self.items.get(idx))
            .and_then(|item| match item {
                ToolbarItem::Button(ToolbarButton::Tool(tool)) => Some(tool),
                ToolbarItem::Seperator => None,
            })
    }

    /// Hit-tests a local point against the toolbar's current render rect
    /// and returns the index of the button item under it, if any.
    pub fn hit_test(&self, local: (f64, f64)) -> Option<usize> {
        let rect = self.rect()?;
        let px = local.0 as f32;
        let py = local.1 as f32;

        if py < rect.top() || py > rect.bottom() || px < rect.left() || px > rect.right() {
            return None;
        }

        let mut current_x = rect.left() + TOOLBAR_PADDING;
        for (idx, item) in self.items.iter().enumerate() {
            let item_w = item.size();
            let item_right = current_x + item_w;
            if px >= current_x && px <= item_right {
                return match item {
                    ToolbarItem::Button(_) => Some(idx),
                    ToolbarItem::Seperator => None,
                };
            }
            current_x += item_w + item.trailing_padding();
        }
        None
    }

    /// Computes the render_pos to start a transition animation from when the
    /// toolbar moves to a new monitor/position. The travel distance is capped
    /// at `max_offset`: on short moves the toolbar starts from its real
    /// current position, on long jumps (e.g. monitor switch) it spawns
    /// `max_offset` pixels away from the target instead of snapping across
    /// the whole screen.
    pub fn compute_transition_start(
        &self,
        old_monitor: usize,
        new_target_local: (f32, f32),
        new_monitor: usize,
        placements: &[Placement],
        max_offset: f32,
    ) -> (f32, f32) {
        let old_p = &placements[old_monitor];
        let new_p = &placements[new_monitor];

        let old_global = (
            old_p.position.0 as f32 + self.render_pos.0,
            old_p.position.1 as f32 + self.render_pos.1,
        );
        let new_global = (
            new_p.position.0 as f32 + new_target_local.0,
            new_p.position.1 as f32 + new_target_local.1,
        );

        let dx = new_global.0 - old_global.0;
        let dy = new_global.1 - old_global.1;
        let dist = (dx * dx + dy * dy).sqrt();

        if dist < 0.001 {
            return new_target_local;
        }

        let (ux, uy) = (dx / dist, dy / dist);
        let offset = dist.min(max_offset);

        let start_global = (new_global.0 - ux * offset, new_global.1 - uy * offset);

        (
            start_global.0 - new_p.position.0 as f32,
            start_global.1 - new_p.position.1 as f32,
        )
    }
}

// ==========================================
// 3. UI Elements Definition
// ==========================================
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolbarAction {
    //SideChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolbarButton {
    Tool(Tool),
    //Action(ToolbarAction),
}

#[derive(Debug, Clone, Copy)]
pub enum ToolbarItem {
    Button(ToolbarButton),
    Seperator,
}

impl PanelItem for ToolbarItem {
    fn size(&self) -> f32 {
        match self {
            ToolbarItem::Button(_) => BUTTON_CELL_SIZE,
            ToolbarItem::Seperator => SEPARATOR_CELL_SIZE,
        }
    }

    fn trailing_padding(&self) -> f32 {
        match self {
            ToolbarItem::Button(_) => 4.0,
            ToolbarItem::Seperator => 0.0,
        }
    }

    fn is_button(&self) -> bool {
        matches!(self, ToolbarItem::Button(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarPlacementKind {
    Idle,        // no selection: top-center on the cursor's monitor
    Hidden,      // active selection drag: toolbar hidden
    AtSelection, // finished selection: toolbar anchored next to it
}