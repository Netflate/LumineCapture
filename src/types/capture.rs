pub type DamageRect = (u32, u32, u32, u32);
pub struct Placement {
    pub size: (i32, i32),
    pub position: (i32, i32),
}
// Wayland outputs
use wayland_client::protocol::wl_output;
use smithay_client_toolkit::output::OutputInfo as SctkOutputInfo;

#[derive(Debug, Clone)]
pub struct Output {
    pub wl_output: wl_output::WlOutput,
    pub info: SctkOutputInfo,
}


#[derive(Debug, Clone)]
pub enum SourceType {
    Monitor = 1,
    Window = 2,
    Virtual = 4,
}

// Pipewire and pixels
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
