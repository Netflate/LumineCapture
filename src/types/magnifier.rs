use std::time::Duration;


pub const ZOOM: f32 = 10.0;
pub const MAG_CELLS: u32 = 21;  // must be uneven
pub const MAG_SIZE: u32 = (MAG_CELLS as f32 * ZOOM) as u32;  
pub const MAG_OFFSET: f32 = 24.0;
pub const MAG_FRAME_INTERVAL: Duration = Duration::from_millis(16); // magnifier fps - 60


#[derive(Debug)]
pub struct MagnifierState {
    pub monitor_idx:usize,
    pub pos : (f64, f64),    
}