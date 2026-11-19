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
    pub fresh: bool,
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

    Ok(ShmBuffer {
        buffer,
        fresh: true,
    })
}

impl ShmBuffer {
    pub fn canvas<'a>(&self, pool: &'a mut SlotPool) -> Option<&'a mut [u8]> {
        pool.canvas(&self.buffer)
    }
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

    pub fn sync_window(
        &mut self,
        pool: &mut SlotPool,
        pixels: &[u8],
        (dst_width, src_width): (u32, u32),
        origin: (u32, u32),
        rect: DamageRect,
    ) -> Vec<DamageRect> {
        match pool.canvas(&self.buffer) {
            Some(dst) => sync_window(dst, dst_width, pixels, src_width, origin, rect),
            None => Vec::new(),
        }
    }
}
// first frame of a new buffer is fully damaged, its slot may still hold the same pixels
// from earlier buffer, nothing looks changed then, but the compositor has never seen this one
pub fn damage_of(fresh: bool, size: (u32, u32), changed: Vec<DamageRect>) -> Vec<DamageRect> {
    if fresh {
        vec![(0, 0, size.0, size.1)]
    } else {
        changed
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
    sync_window(dst, width, pixels, width, (0, 0), rect)
}

/// `rect` is in source pixels, the damage comes back in `dst` ones, since `pixels` is the whole canvas.
pub fn sync_window(
    dst: &mut [u8],
    dst_width: u32,
    pixels: &[u8],
    src_width: u32,
    (ox, oy): (u32, u32),
    rect: DamageRect,
) -> Vec<DamageRect> {
    sync_into(dst, dst_width, (0, 0), pixels, src_width, (ox, oy), rect)
}

pub fn sync_into(
    dst: &mut [u8],
    dst_width: u32,
    dst_offset: (u32, u32),
    pixels: &[u8],
    src_width: u32,
    src_origin: (u32, u32),
    rect: DamageRect,
) -> Vec<DamageRect> {
    let (ox, oy) = src_origin;
    let dst_stride = (dst_width * 4) as usize;
    let src_stride = (src_width * 4) as usize;
    let dst_rows = (dst.len() / dst_stride.max(1)) as u32;
    let src_rows = (pixels.len() / src_stride.max(1)) as u32;

    let left = rect.0.max(ox);
    let top = rect.1.max(oy);
    let right = rect
        .0
        .saturating_add(rect.2)
        .min(ox.saturating_add(dst_width.saturating_sub(dst_offset.0)))
        .min(src_width);
    let bottom = rect
        .1
        .saturating_add(rect.3)
        .min(oy.saturating_add(dst_rows.saturating_sub(dst_offset.1)))
        .min(src_rows);
    if left >= right || top >= bottom {
        return Vec::new();
    }
    let (x, y) = (left - ox + dst_offset.0, top - oy + dst_offset.1);
    let (w, h) = (right - left, bottom - top);

    let cols = w.div_ceil(TILE) as usize;
    let mut changed = vec![false; cols * h.div_ceil(TILE) as usize];

    for row in 0..h {
        let line = &mut changed[(row / TILE) as usize * cols..][..cols];
        let start_dst = (y + row) as usize * dst_stride + x as usize * 4;
        let start_src = (top + row) as usize * src_stride + left as usize * 4;
        for (tx, tile) in line.iter_mut().enumerate() {
            let from = tx * TILE as usize * 4;
            let to = (from + TILE as usize * 4).min(w as usize * 4);
            let d = start_dst + from..start_dst + to;
            let s = start_src + from..start_src + to;
            *tile |= sync_swizzled(&mut dst[d], &pixels[s]);
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
    use super::{damage_of, sync_into, sync_rect, sync_window, tile_rects};

    #[test]
    fn fresh_buffer_is_damaged_completely() {
        assert_eq!(damage_of(true, (80, 50), vec![]), vec![(0, 0, 80, 50)]);
        assert_eq!(
            damage_of(true, (80, 50), vec![(0, 0, 32, 32)]),
            vec![(0, 0, 80, 50)]
        );
        assert_eq!(damage_of(false, (80, 50), vec![]), vec![]);
        assert_eq!(
            damage_of(false, (80, 50), vec![(32, 0, 32, 32)]),
            vec![(32, 0, 32, 32)]
        );
    }

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

    fn ramp(w: u32, h: u32) -> Vec<u8> {
        (0..w * h * 4).map(|i| (i % 251 + 1) as u8).collect()
    }

    fn swizzled(src: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
        let i = (y * w + x) as usize * 4;
        [src[i + 2], src[i + 1], src[i], src[i + 3]]
    }

    #[test]
    fn window_copies_the_part_at_origin() {
        let (sw, sh, dw, dh) = (40u32, 30u32, 20u32, 10u32);
        let (ox, oy) = (5u32, 7u32);
        let src = ramp(sw, sh);
        let mut dst = vec![0; (dw * dh * 4) as usize];
        let rects = sync_window(&mut dst, dw, &src, sw, (ox, oy), (0, 0, sw, sh));
        assert_eq!(rects, vec![(0, 0, dw, dh)]);
        for y in 0..dh {
            for x in 0..dw {
                let i = (y * dw + x) as usize * 4;
                assert_eq!(dst[i..i + 4], swizzled(&src, sw, x + ox, y + oy));
            }
        }
    }

    #[test]
    fn damage_outside_the_window_is_ignored() {
        let (sw, sh) = (40u32, 30u32);
        let src = ramp(sw, sh);
        let mut dst = vec![0; 20 * 10 * 4];
        let outside = [(0, 0, 5, 7), (25, 0, 10, 30), (0, 17, 40, 5), (40, 0, 3, 3)];
        for rect in outside {
            assert!(sync_window(&mut dst, 20, &src, sw, (5, 7), rect).is_empty());
        }
        assert!(dst.iter().all(|&b| b == 0));
    }

    #[test]
    fn partial_damage_is_clipped_and_moved_to_window_coords() {
        let (sw, sh) = (40u32, 30u32);
        let src = ramp(sw, sh);
        let mut dst = vec![0; 20 * 10 * 4];
        let rects = sync_window(&mut dst, 20, &src, sw, (5, 7), (0, 0, 10, 10));
        assert_eq!(rects, vec![(0, 0, 5, 3)]);
        for y in 0..3 {
            for x in 0..5 {
                let i = (y * 20 + x) as usize * 4;
                assert_eq!(dst[i..i + 4], swizzled(&src, sw, x + 5, y + 7));
            }
        }
        assert_eq!(dst[(3 * 20 * 4) as usize], 0);
    }

    #[test]
    fn offset_copy_clips_and_reports_buffer_coordinates() {
        let (sw, sh, dw, dh) = (8u32, 6u32, 12u32, 10u32);
        let src = ramp(sw, sh);
        let mut dst = vec![0; (dw * dh * 4) as usize];
        let damage = sync_into(&mut dst, dw, (3, 2), &src, sw, (0, 0), (5, 3, 5, 5));
        assert!(
            damage
                .iter()
                .any(|r| r.0 <= 8 && r.1 <= 5 && r.0 + r.2 >= 11 && r.1 + r.3 >= 8)
        );
        for y in 0..dh {
            for x in 0..dw {
                let i = ((y * dw + x) * 4) as usize;
                if (8..11).contains(&x) && (5..8).contains(&y) {
                    assert_eq!(&dst[i..i + 4], &swizzled(&src, sw, x - 3, y - 2));
                } else {
                    assert_eq!(&dst[i..i + 4], &[0; 4]);
                }
            }
        }
    }
}
