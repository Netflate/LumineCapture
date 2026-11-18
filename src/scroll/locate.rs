// Corner markers that tell where a maximized overlay sits on its monitor
// drawn into the canvas, then searched for in a captured frame among the possible offsets.
// since we need to find out how the screenshot changed after switching from fullscreen to maxmized
// for now, and hopefully forever, its used only on gnome
use tiny_skia::{Paint, Pixmap, Rect, Transform};

use crate::types::MonitorFrame;

const CELL: f32 = 8.0;
const SIZE: f32 = CELL * 3.0;
const TOLERANCE: u8 = 40;
const MAX_SCORE: f32 = 0.05;

// bottom-right marker is this one transposed
const TL: [[[u8; 3]; 3]; 3] = [
    [[255, 0, 0], [0, 255, 0], [0, 0, 255]],
    [[255, 255, 255], [0, 0, 0], [255, 0, 255]],
    [[0, 255, 255], [255, 255, 0], [255, 0, 0]],
];

fn color(marker: usize, row: usize, col: usize) -> [u8; 3] {
    if marker == 0 {
        TL[row][col]
    } else {
        TL[col][row]
    }
}

pub fn rects(origin: (i32, i32), window: (u32, u32)) -> [Rect; 2] {
    let (ox, oy) = (origin.0 as f32, origin.1 as f32);
    let (w, h) = (window.0 as f32, window.1 as f32);
    [
        Rect::from_xywh(ox, oy, SIZE, SIZE),
        Rect::from_xywh(ox + w - SIZE, oy + h - SIZE, SIZE, SIZE),
    ]
    .map(|rect| rect.unwrap())
}

pub fn draw(pixmap: &mut Pixmap, origin: (i32, i32), window: (u32, u32), scale: f32) {
    let px = |v: f32| (v * scale).round();
    for (marker, rect) in rects(origin, window).into_iter().enumerate() {
        for row in 0..3 {
            for col in 0..3 {
                let [r, g, b] = color(marker, row, col);
                let x0 = rect.left() + col as f32 * CELL;
                let y0 = rect.top() + row as f32 * CELL;
                let Some(cell) = Rect::from_ltrb(px(x0), px(y0), px(x0 + CELL), px(y0 + CELL))
                else {
                    continue;
                };
                let mut paint = Paint::default();
                paint.set_color_rgba8(r, g, b, 255);
                paint.anti_alias = false;
                pixmap.fill_rect(cell, &paint, Transform::identity(), None);
            }
        }
    }
}

pub fn find(frame: &MonitorFrame, window: (u32, u32), scale: f32) -> Option<(i32, i32)> {
    let (w, h) = (window.0 as f32, window.1 as f32);
    if w < SIZE || h < SIZE {
        return None;
    }
    let wp = (w * scale).round() as u32;
    let hp = (h * scale).round() as u32;
    if wp > frame.pw_width || hp > frame.pw_height {
        return None;
    }
    let n = (SIZE * scale).round() as u32;
    let br = (
        ((w - SIZE) * scale).round() as u32,
        ((h - SIZE) * scale).round() as u32,
    );
    let total = 2 * n * n;
    // misses must stay below this, which is the same as a score under MAX_SCORE
    let open = (total as f32 * MAX_SCORE).ceil() as u32;

    let mut best: Option<(u32, (u32, u32))> = None;
    for dy in 0..=frame.pw_height - hp {
        for dx in 0..=frame.pw_width - wp {
            let bound = best.map_or(open, |(misses, _)| misses);
            let mut misses = 0;
            'blocks: for (marker, (bx, by)) in [(0, (dx, dy)), (1, (dx + br.0, dy + br.1))] {
                for y in 0..n {
                    for x in 0..n {
                        if !matches(frame, bx + x, by + y, expected(marker, x, y, scale)) {
                            misses += 1;
                            if misses >= bound {
                                break 'blocks;
                            }
                        }
                    }
                }
            }
            if misses < bound {
                best = Some((misses, (dx, dy)));
            }
        }
    }

    let (_, (dx, dy)) = best?;
    Some((
        (dx as f32 / scale).round() as i32,
        (dy as f32 / scale).round() as i32,
    ))
}

fn expected(marker: usize, x: u32, y: u32, scale: f32) -> [u8; 3] {
    let cell = |v: u32| (((v as f32 + 0.5) / scale / CELL) as usize).min(2);
    color(marker, cell(y), cell(x))
}

fn matches(frame: &MonitorFrame, x: u32, y: u32, expected: [u8; 3]) -> bool {
    let i = y as usize * frame.pw_stride as usize + x as usize * 4;
    let Some(px) = frame.pixels.get(i..i + 3) else {
        return false;
    };
    px.iter()
        .zip(expected)
        .all(|(&a, b)| a.abs_diff(b) <= TOLERANCE)
}
