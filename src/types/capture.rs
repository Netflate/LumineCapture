pub type DamageRect = (u32, u32, u32, u32);
pub struct Placement {
    pub size: (i32, i32),
    pub position: (i32, i32),
}

impl Placement {
    /// Where this monitor starts in global coordinates, ready for drawing.
    pub fn offset(&self) -> (f32, f32) {
        (self.position.0 as f32, self.position.1 as f32)
    }

    pub fn rect(&self) -> Option<tiny_skia::Rect> {
        tiny_skia::Rect::from_xywh(0.0, 0.0, self.size.0 as f32, self.size.1 as f32)
    }
}
// Wayland outputs
use smithay_client_toolkit::output::OutputInfo as SctkOutputInfo;
use wayland_client::protocol::wl_output;

#[derive(Debug, Clone)]
pub struct Output {
    pub wl_output: wl_output::WlOutput,
    pub info: SctkOutputInfo,
}

// Pipewire and pixels
pub struct StreamInfo {
    pub node_id: u32,
    pub size: Option<(i32, i32)>,
    pub position: Option<(i32, i32)>,
}

pub struct MonitorFrame {
    pub output: usize,
    pub pixels: Vec<u8>,
    pub pw_width: u32,
    pub pw_height: u32,
    pub pw_stride: u32,
    pub info: StreamInfo,
}

pub struct Capture {
    pub pixmap: tiny_skia::Pixmap,
    pub scale: f32,
}

impl Capture {
    pub fn to_native(&self, point: (f64, f64)) -> (f64, f64) {
        let scale = self.scale as f64;
        (point.0 * scale, point.1 * scale)
    }
}

pub struct CaptureResult {
    pub frames: Vec<MonitorFrame>,
}
