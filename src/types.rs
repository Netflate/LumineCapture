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
    pub base: Vec<Pixmap>,           // doesn't change, original screenshots
    pub canvas: Vec<Pixmap>,
    pub dimmed: Vec<Pixmap>,
    pub selection: Option<Rect>, 
    pub mode: EditMode,               
    pub pointer: (usize, f64, f64),
    pub mouse_down: bool,
    pub magnifier: Option<MagnifierState> ,
}

pub struct MagnifierState {
    pub monitor_idx:usize,
    pub pos : (f64, f64),    
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

pub struct Placement {
    pub size: (i32, i32),
    pub position: (i32, i32),
}

pub struct StreamInfo {
    pub node_id: u32,
    pub size: Option<(i32, i32)>,
    pub position: Option<(i32, i32)>,
}

pub struct MonitorFrame {
    pub pixels: Vec<u8>,
    pub pw_width: u32,
    pub pw_height: u32,
    pub pw_stride: u32,
    pub info: StreamInfo,
}

pub struct CaptureResult {
    pub frames: Vec<MonitorFrame>,
}


#[derive(Debug, Clone, Copy)]
pub enum OverlayEvent {
    PointerMove { monitor_idx: usize, x: f64, y: f64},
    // MouseDownLeft,
    // MouseUpLeft,
    EscapePressed,
}