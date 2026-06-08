use strum::EnumIter;
// ==========================================
// 1. UI Layout Constants 
// ==========================================
pub const TOOLBAR_HEIGHT: f32 = 40.0;
pub const TOOLBAR_OFFSET: f32 = 0.0;

// Toolbar tools list
pub const TOOLBAR_ITEMS: &[ToolbarItem] = &[
    ToolbarItem::ToolButton(Tool::Selection),
    ToolbarItem::ToolButton(Tool::Arrow),
    ToolbarItem::ToolButton(Tool::Rectangle),
    ToolbarItem::ToolButton(Tool::Circle),
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
    pub items: &'static [ToolbarItem],
    pub position: (f32, f32), 
    pub size: (f32, f32),
    pub transparent: bool,
    pub current_side: ToolbarSide,
    pub monitor_idx: usize, 

    pub prev_position: (f32, f32),
    pub prev_monitor_idx: usize,
}

impl Toolbar {
    pub fn new() -> Self {            
        let mut toolbar = Self { 
            items: TOOLBAR_ITEMS,
            size: (0.0, TOOLBAR_HEIGHT), 
            transparent: false,
            current_side: ToolbarSide::Top, 
            monitor_idx: 0,
            position: (0.0,0.0),

            prev_position: (0.0,0.0),
            prev_monitor_idx: 0,
        };

        toolbar.size.0 = toolbar.toolbar_width() as f32;
        toolbar.size.1 = TOOLBAR_HEIGHT;
        
        toolbar
    }

    
    pub fn toolbar_width(&self) -> u32 {
        let mut total_width = 8; 

        for item in self.items {
            total_width += item.width() + item.trailing_padding();
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
    pub fn width(&self) -> u32 { 
        const ICON_SIZE: u32 = 28;
        const SEPARATOR_SIZE: u32 = 2;

        match self {
            ToolbarItem::ToolButton(_) => ICON_SIZE,
            ToolbarItem::Seperator => SEPARATOR_SIZE,
        }
    }

    pub fn trailing_padding(&self) -> u32 {
        match self {
            ToolbarItem::ToolButton(_) => 8,
            ToolbarItem::Seperator => 6,
        }
    }


}