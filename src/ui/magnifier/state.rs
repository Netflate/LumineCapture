
pub const ZOOM: f32 = 10.0;
pub const CELLS: u32 = 21; // must be uneven
pub const SIZE: u32 = (CELLS as f32 * ZOOM) as u32;
pub const OFFSET: f32 = 24.0;

#[derive(Debug)]
pub struct MagnifierState {
    pub monitor_idx: usize,
    pub pos: (f64, f64),
}
