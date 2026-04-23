

pub struct EditorState {
    pub base: CapturedFrame,          
    pub mode: EditMode,               
    //pub selection: Option<Rect>,      
    pub pointer: (f64, f64),
    pub mouse_down: bool,
}
pub enum EditMode {
    Selection,
}


#[derive(Debug, Clone)]

pub enum SourceType {
    Monitor = 1,
    Window = 2,
    Virtual = 4,
}
#[derive(Clone)]
pub struct CapturedFrame {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub struct Placement {
    pub size: (i32, i32),
    pub position: (i32, i32),
}

pub struct StreamInfo {
    pub node_id: u32,
    pub size: Option<(i32, i32)>,
    pub position: Option<(i32, i32)>,
    pub source_type: SourceType,
}

pub struct CaptureResult {
    pub frame: CapturedFrame,
    pub streams: Vec<StreamInfo>,
}




#[derive(Debug, Clone, Copy)]
pub enum OverlayEvent {
    // PointerMove { x: f64, y: f64 },
    // MouseDownLeft,
    // MouseUpLeft,
    EscapePressed,
}