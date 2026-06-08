pub mod toolbar;
pub mod icons;

use crate::types::toolbar::{Toolbar, Tool};
use usvg::Tree;


use tiny_skia::{Rect, Pixmap};
use std::time::{Instant, Duration};
use wayland_client::{
    protocol::{wl_output},
};
use std::collections::HashMap;

pub struct SelectionEdges {
    pub left : bool, 
    pub right : bool, 
    pub top: bool,
    pub bottom: bool,
}



pub const ZOOM: f32 = 10.0;
pub const MAG_CELLS: u32 = 21;  // must be uneven
pub const MAG_SIZE: u32 = (MAG_CELLS as f32 * ZOOM) as u32;  
pub const MAG_OFFSET: f32 = 24.0;
pub const HANDLE_RADIUS: f64 = 8.0; // pixels around selection border 
pub const MAG_FRAME_INTERVAL: Duration = Duration::from_millis(16); // magnifier fps - 60



pub type DamageRect = (u32, u32, u32, u32);


#[derive(Clone)]
pub struct OutputInfo {
    pub output: wl_output::WlOutput,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
#[derive(Debug, Clone)]
pub enum MouseButton {
    Left,
    Right, 
    Middle,
}



pub enum MouseState {
    Up,
    Down(MouseButton),
}

pub struct SelectionState {
    pub zone: Option<Rect>,
    pub prev_zone: Option<Rect>,
    pub active_handle: SelectionHandle,
    pub drag_origin: Option<(f64, f64)>,   
    pub selection_at_drag_start: Option<Rect>,
}

impl Default for SelectionState {
    fn default() -> Self {
        Self {
            zone: None,
            prev_zone: None,
            active_handle: SelectionHandle::None,
            drag_origin: None,
            selection_at_drag_start: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PointerState {
    pub monitor_idx: usize,
    pub local: (f64, f64),
    pub global: (f64, f64),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SelectionHandle {
    TopLeft, Top, TopRight,
    Left,          Right,
    BottomLeft, Bottom, BottomRight,
    Move,   
    None,   
}


impl PointerState {
    pub fn new(monitor_idx: usize, local: (f64, f64), global: (f64, f64)) -> Self {
        Self {
            monitor_idx,
            local,
            global,
        }
    }
}

pub struct EditorState {
    pub base: Vec<Pixmap>,           // doesn't change, original screenshots
    pub canvas: Vec<Pixmap>,
    pub dimmed: Vec<Pixmap>,
    pub placements : Vec<Placement>, 
    pub drag_start: Option<(f64, f64)>,
    pub mode: EditMode,               
    pub pointer: PointerState,
    pub magnifier: Option<MagnifierState> ,
    pub prev_magnifier: Option<MagnifierState>,
    pub last_mag_update: Option<Instant>,
    pub mouse_down_left : bool,
    pub selection: SelectionState,
    pub toolbar : Toolbar,
    pub icon_cache : HashMap<Tool, Tree>,
}

#[derive(Debug)]
pub struct MagnifierState {
    pub monitor_idx:usize,
    pub pos : (f64, f64),    
}
#[derive(PartialEq)]
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


#[derive(Debug, Clone)]
pub enum OverlayEvent {
    PointerMove { monitor_idx: usize, x: f64, y: f64},
    PointerButton {button: MouseButton, pressed : bool},
    //MouseUpLeft,
    EscapePressed,
    SaveToClipboard,
}


