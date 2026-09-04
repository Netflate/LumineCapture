use tiny_skia::{Color, Pixmap};

// honestly my magnifier implementation sucks a bit
// im not sure how to properly change these constants without making it look crooked
pub const ZOOM: f32 = 10.0;
pub const CELLS: u32 = 21; // must be uneven
pub const SIZE: u32 = (CELLS as f32 * ZOOM) as u32;
pub const OFFSET: f32 = 24.0;

pub const LABEL_HEIGHT: f32 = 26.0;
pub const LABEL_GAP: f32 = 6.0;

#[derive(Debug)]
pub struct MagnifierState {
    pub monitor_idx: usize,
    pub pos: (f64, f64),
}

pub fn sample_pixel(source: &Pixmap, point: (f64, f64)) -> Option<Color> {
    let (x, y) = (point.0.floor(), point.1.floor());
    if x < 0.0 || y < 0.0 {
        return None;
    }
    let (x, y) = (x as u32, y as u32);
    if x >= source.width() || y >= source.height() {
        return None;
    }
    let px = source.pixels()[(y * source.width() + x) as usize].demultiply();
    Some(Color::from_rgba8(px.red(), px.green(), px.blue(), 255))
}
