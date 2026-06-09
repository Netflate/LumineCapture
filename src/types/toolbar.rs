use strum::EnumIter;
use tiny_skia::Pixmap;
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
    ToolbarItem::ToolButton(Tool::Selection),
    ToolbarItem::ToolButton(Tool::Arrow),
    ToolbarItem::ToolButton(Tool::Rectangle),
    ToolbarItem::ToolButton(Tool::Circle),
    ToolbarItem::Seperator,
    ToolbarItem::ToolButton(Tool::Rectangle),
    ToolbarItem::ToolButton(Tool::Text),
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
}

impl Toolbar {
    pub fn new() -> Self {            
        let mut toolbar = Self { 
            toolbar_pixmap: None,
            items: TOOLBAR_ITEMS,
            size: (0.0, TOOLBAR_HEIGHT), 
            opacity: 0.05,
            current_side: ToolbarSide::Top, 
            monitor_idx: 0,
            position: (0.0,0.0),

            prev_position: (0.0,0.0),
            prev_monitor_idx: 0,

            dirty : false,
            selected : None, 
            hovered: None,
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
}

// ==========================================
// 3. UI Elements Definition
// ==========================================
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter)]
pub enum Tool {
    Selection, 
    Rectangle, 
    Arrow, 
    Circle,
    Text,
}

#[derive(Debug, Clone, Copy)]
pub enum ToolbarItem {
    ToolButton(Tool),
    Seperator, 
}

impl ToolbarItem {
    pub fn size(&self) -> f32 { 
        match self {
            ToolbarItem::ToolButton(_) => BUTTON_CELL_SIZE,
            ToolbarItem::Seperator => SEPARATOR_CELL_SIZE,
        }
    }

    pub fn trailing_padding(&self) -> f32 {
        match self {
            ToolbarItem::ToolButton(_) => 0.0,
            ToolbarItem::Seperator => 0.0,
        }
    }


}