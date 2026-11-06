// ── Shared Memory (shm) buffer Utilities ──────────────────────────────────────
// manage low-level shared memory allocations, stride math, and raw pixel  copying for surface drawing.
//
// in 'utils' to separate raw memory manipulation and buffer rendering
// from the high-level Wayland protocol handlers ('state/compositor_shm_layer.rs')
// reminder: each output has its own surface

use crate::types::DamageRect;
use crate::utils::{copy_swizzled, sync_swizzled};
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use wayland_client::protocol::{wl_buffer, wl_shm};
pub struct ShmBuffer {
    pub buffer: Buffer,
}

pub fn create_shm_buffer(
    pool: &mut SlotPool,
    width: u32,
    height: u32,
) -> Result<ShmBuffer, Box<dyn std::error::Error>> {
    let stride = width as i32 * 4;
    let (buffer, _canvas) = pool.create_buffer(
        width as i32,
        height as i32,
        stride,
        wl_shm::Format::Argb8888,
    )?;

    Ok(ShmBuffer { buffer })
}

impl ShmBuffer {
    pub fn wl_buffer(&self) -> &wl_buffer::WlBuffer {
        self.buffer.wl_buffer()
    }

    pub fn write_pixels(&mut self, pool: &mut SlotPool, pixels: &[u8]) {
        if let Some(canvas) = pool.canvas(&self.buffer) {
            let len = pixels.len().min(canvas.len());

            copy_swizzled(&mut canvas[..len], &pixels[..len]);
        }
    }

    pub fn sync_rect(
        &mut self,
        pool: &mut SlotPool,
        pixels: &[u8],
        width: u32,
        rect: DamageRect,
    ) -> Vec<DamageRect> {
        match pool.canvas(&self.buffer) {
            Some(dst) => sync_rect(dst, pixels, width, rect),
            None => Vec::new(),
        }
    }
}
/// Synchronizes a specified rect of the buffer with new pixels, and then updates only the
/// tiles that have changed, and returns an optimized list of damage rect for the compositor.
///
/// 1. Iterates over tiles within the given rect.
/// 2. Compares new pixels with the current `dst` buffer and updates only cahnged areas.
/// 3. Groups adjacent changed tiles into minimal damage rects.
///
///    If number of resulting rects exceeds `MAX_RECTS`, they are merged into bounding box because.
///    Submitting too many small updates to the compositor (at least kwin) becomes more expensive
///    than updating a single larger area.
pub fn sync_rect(dst: &mut [u8], pixels: &[u8], width: u32, rect: DamageRect) -> Vec<DamageRect> {
    let stride = (width * 4) as usize;
    let rows = (dst.len().min(pixels.len()) / stride.max(1)) as u32;
    let (x, y) = (rect.0.min(width), rect.1.min(rows));
    let (w, h) = (rect.2.min(width - x), rect.3.min(rows - y));
    let cols = w.div_ceil(TILE) as usize;
    let mut changed = vec![false; cols * h.div_ceil(TILE) as usize];

    for row in 0..h {
        let line = &mut changed[(row / TILE) as usize * cols..][..cols];
        let start = (y + row) as usize * stride + x as usize * 4;
        for (tx, tile) in line.iter_mut().enumerate() {
            let from = start + tx * TILE as usize * 4;
            let to = (from + TILE as usize * 4).min(start + w as usize * 4);
            *tile |= sync_swizzled(&mut dst[from..to], &pixels[from..to]);
        }
    }

    tile_rects(&changed, cols)
        .into_iter()
        .map(|(tx, ty, tw, th)| {
            let (rx, ry) = (x + tx * TILE, y + ty * TILE);
            (
                rx,
                ry,
                (tw * TILE).min(x + w - rx),
                (th * TILE).min(y + h - ry),
            )
        })
        .collect()
}

const TILE: u32 = 32;
const MAX_RECTS: usize = 32;

fn tile_rects(changed: &[bool], cols: usize) -> Vec<DamageRect> {
    let mut rects: Vec<DamageRect> = Vec::new();
    for (ty, line) in changed.chunks(cols.max(1)).enumerate() {
        let ty = ty as u32;
        let mut tx = 0;
        while tx < line.len() {
            if !line[tx] {
                tx += 1;
                continue;
            }
            let start = tx;
            while tx < line.len() && line[tx] {
                tx += 1;
            }
            let (sx, sw) = (start as u32, (tx - start) as u32);
            match rects
                .iter_mut()
                .find(|r| r.0 == sx && r.2 == sw && r.1 + r.3 == ty)
            {
                Some(r) => r.3 += 1,
                None => rects.push((sx, ty, sw, 1)),
            }
        }
    }
    if rects.len() > MAX_RECTS {
        let x0 = rects.iter().map(|r| r.0).min().unwrap_or(0);
        let y0 = rects.iter().map(|r| r.1).min().unwrap_or(0);
        let x1 = rects.iter().map(|r| r.0 + r.2).max().unwrap_or(0);
        let y1 = rects.iter().map(|r| r.1 + r.3).max().unwrap_or(0);
        rects = vec![(x0, y0, x1 - x0, y1 - y0)];
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::{sync_rect, tile_rects};

    fn grid(rows: &[&str]) -> (Vec<bool>, usize) {
        let cols = rows[0].len();
        (
            rows.iter()
                .flat_map(|r| r.chars().map(|c| c == '#'))
                .collect(),
            cols,
        )
    }

    #[test]
    fn edges_of_a_grown_rect_stay_apart() {
        let (g, cols) = grid(&["...#", "...#", "...#", "####"]);
        assert_eq!(tile_rects(&g, cols), vec![(3, 0, 1, 3), (0, 3, 4, 1)]);
    }

    #[test]
    fn same_span_rows_merge() {
        let (g, cols) = grid(&[".##.", ".##.", "#..#"]);
        assert_eq!(
            tile_rects(&g, cols),
            vec![(1, 0, 2, 2), (0, 2, 1, 1), (3, 2, 1, 1)]
        );
    }

    #[test]
    fn too_many_rects_become_their_bounds() {
        let rows: Vec<String> = (0..40)
            .map(|i| if i % 2 == 0 { "#.".into() } else { ".#".into() })
            .collect();
        let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
        let (g, cols) = grid(&rows);
        assert_eq!(tile_rects(&g, cols), vec![(0, 0, 2, 40)]);
    }

    #[test]
    fn sync_damages_every_changed_pixel() {
        let (w, h) = (100u32, 70u32);
        let mut seed = 7u32;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed
        };
        let mut frame: Vec<u8> = (0..w * h * 4).map(|_| next() as u8).collect();
        let mut buffer = vec![0; frame.len()];
        sync_rect(&mut buffer, &frame, w, (0, 0, w, h));
        for _ in 0..50 {
            let before = buffer.clone();
            for _ in 0..next() % 20 {
                let i = (next() % (w * h)) as usize * 4;
                frame[i + (next() % 4) as usize] ^= 0x10;
            }
            let rects = sync_rect(&mut buffer, &frame, w, (0, 0, w, h));
            for (i, (old, new)) in before.chunks(4).zip(buffer.chunks(4)).enumerate() {
                let (x, y) = (i as u32 % w, i as u32 / w);
                let inside = rects
                    .iter()
                    .any(|r| x >= r.0 && x < r.0 + r.2 && y >= r.1 && y < r.1 + r.3);
                assert!(old == new || inside, "({x}, {y}) changed outside damage");
                let s = &frame[i * 4..i * 4 + 4];
                assert_eq!(new, [s[2], s[1], s[0], s[3]]);
            }
        }
    }
}
