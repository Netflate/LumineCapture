use tiny_skia::Pixmap;
use std::time::Instant;
use crate::tools::Tool;
// ==========================================
// 1. UI Layout Constants 
// ==========================================
pub const TOOLBAR_HEIGHT: f32 = 36.0;
pub const TOOLBAR_OFFSET: f32 = 0.0;
pub const TOOLBAR_PADDING: f32 = 8.0; // left & right 

pub const BUTTON_CELL_SIZE: f32 = 36.0;
const SEPARATOR_CELL_SIZE: f32 = 20.0;

// Toolbar tools list
pub const TOOLBAR_ITEMS: &[ToolbarItem] = &[
    ToolbarItem::Button(ToolbarButton::Tool(Tool::Selection)),
    ToolbarItem::Button(ToolbarButton::Tool(Tool::Arrow)),
    ToolbarItem::Button(ToolbarButton::Tool(Tool::Rectangle)),
    ToolbarItem::Button(ToolbarButton::Tool(Tool::Circle)),
    ToolbarItem::Button(ToolbarButton::Tool(Tool::Text)),
    ToolbarItem::Seperator,
    ToolbarItem::Button(ToolbarButton::Action(ToolbarAction::SideChange)),
];

// ==========================================
// 2. Component State
// ==========================================
#[derive(Debug, Clone, Copy)]
pub enum ToolbarSide {
    Top, 
    Bottom,
}

pub struct Toolbar { 
    pub toolbar_pixmap: Option<Pixmap>, // to change it's opacity (svg icons fault)

    pub items: &'static [ToolbarItem],
    pub position: (f32, f32), 
    pub size: (f32, f32),
    pub opacity: f32,
    pub current_side: ToolbarSide,
    pub monitor_idx: usize, 

    pub prev_position: (f32, f32),
    pub prev_monitor_idx: usize,

    pub dirty : bool,

    pub selected : Option<usize>,
    pub hovered : Option<usize>,

    pub anim : Option<ToolbarAnimation>,
    pub render_y: f32,                   // visual y for render 
    pub interferes : bool,
    pub last_tick: Option<Instant>,
}

impl Toolbar {
    pub fn new() -> Self {            
        let mut toolbar = Self { 
            toolbar_pixmap: None,
            items: TOOLBAR_ITEMS,
            size: (0.0, TOOLBAR_HEIGHT), 
            opacity: 1.0,
            current_side: ToolbarSide::Top, 
            monitor_idx: 0,
            position: (0.0,0.0),

            prev_position: (0.0,0.0),
            prev_monitor_idx: 0,

            dirty : false,
            selected : None, 
            hovered: None,

            anim: None,
            render_y: 0.0, 
            interferes: false,
            last_tick: None,

        };

        toolbar.size.0 = toolbar.toolbar_width() as f32;
        toolbar.size.1 = TOOLBAR_HEIGHT;
                
        toolbar
    }

    
    pub fn toolbar_width(&self) -> f32 {
        let mut total_width= TOOLBAR_PADDING * 2.0; 

        for item in self.items {
            total_width += item.size() + item.trailing_padding();
        }
        total_width
    }

    pub fn get_selected_tool(&self) -> Option<&Tool> {
        self.selected.and_then ( 
            |idx| self.items.get(idx)).and_then(|item| {
                match item {
                    ToolbarItem::Button(ToolbarButton::Tool(tool)) => Some(tool),
                    ToolbarItem::Seperator => None,
                    _ => None,
                }
            })
        }
    }

    

// ==========================================
// 3. UI Elements Definition
// ==========================================
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolbarAction {
    SideChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolbarButton {
    Tool(Tool),
    Action(ToolbarAction),
}

#[derive(Debug, Clone, Copy)]
pub enum ToolbarItem {
    Button(ToolbarButton),
    Seperator, 
}

impl ToolbarItem {
    pub fn size(&self) -> f32 { 
        match self {
            ToolbarItem::Button(_) => BUTTON_CELL_SIZE,
            ToolbarItem::Seperator => SEPARATOR_CELL_SIZE,
        }
    }

    pub fn trailing_padding(&self) -> f32 {
        match self {
            ToolbarItem::Button(_) => 0.0,
            ToolbarItem::Seperator => 0.0,
        }
    }


}
// animation
pub struct ToolbarAnimation {
    pub start: Instant,
    pub duration_ms : u64, 
    pub from_y: f32,
    pub to_y: f32,
}