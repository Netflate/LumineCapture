use tiny_skia::{Rect, Color, Pixmap};
use wayland_client::{
    protocol::{wl_output},
};
pub enum Annotation {
    Arrow { from: (f32,f32), to: (f32,f32), color: Color },
    Rect  { rect: Rect, color: Color },
    Text  { pos: (f32,f32), content: String },              // Toadd text fonts, or for now system's default font 
}

#[derive(Clone)]
pub struct OutputInfo {
    pub output: wl_output::WlOutput,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}


pub struct EditorState {
    pub base: Pixmap,     // doesn't change
    pub selection: Option<Rect>, 
    pub mode: EditMode,               
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
    PointerMove { x: f64, y: f64 },
    // MouseDownLeft,
    // MouseUpLeft,
    EscapePressed,
}