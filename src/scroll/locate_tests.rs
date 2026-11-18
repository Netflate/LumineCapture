use crate::scroll::locate::{draw, find};
use crate::types::{MonitorFrame, StreamInfo};
use tiny_skia::Pixmap;

fn noisy(monitor: (u32, u32), scale: f32) -> Pixmap {
    let (w, h) = (
        (monitor.0 as f32 * scale).round() as u32,
        (monitor.1 as f32 * scale).round() as u32,
    );
    let mut pixmap = Pixmap::new(w, h).unwrap();
    for (i, px) in pixmap
        .data_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .enumerate()
    {
        let hash = (i as u32).wrapping_mul(2_654_435_761).rotate_left(11);
        px.copy_from_slice(&[hash as u8, (hash >> 8) as u8, (hash >> 16) as u8, 255]);
    }
    pixmap
}

fn frame(pixmap: &Pixmap) -> MonitorFrame {
    MonitorFrame {
        output: 0,
        pixels: pixmap.data().to_vec(),
        pw_width: pixmap.width(),
        pw_height: pixmap.height(),
        pw_stride: pixmap.width() * 4,
        info: StreamInfo {
            node_id: 0,
            size: Some((pixmap.width() as i32, pixmap.height() as i32)),
            position: Some((0, 0)),
        },
    }
}

fn scene(monitor: (u32, u32), scale: f32, window: (u32, u32), origin: (i32, i32)) -> Pixmap {
    let mut canvas = noisy(monitor, scale);
    draw(&mut canvas, origin, window, scale);
    canvas
}

fn found(
    monitor: (u32, u32),
    scale: f32,
    window: (u32, u32),
    origin: (i32, i32),
) -> Option<(i32, i32)> {
    let canvas = scene(monitor, scale, window, origin);
    find(&frame(&canvas), window, scale)
}

#[test]
fn finds_the_origin_at_scale_one() {
    assert_eq!(
        found((1920, 1080), 1.0, (1856, 1048), (64, 32)),
        Some((64, 32))
    );
}

#[test]
fn finds_the_origin_at_scale_two() {
    assert_eq!(found((960, 540), 2.0, (896, 508), (64, 32)), Some((64, 32)));
}

#[test]
fn finds_the_origin_at_fractional_scale() {
    assert_eq!(
        found((1536, 864), 1.25, (1472, 832), (64, 32)),
        Some((64, 32))
    );
}

#[test]
fn window_as_big_as_the_monitor_is_at_the_origin() {
    assert_eq!(found((400, 300), 1.0, (400, 300), (0, 0)), Some((0, 0)));
}

#[test]
fn no_markers_is_not_found() {
    let canvas = noisy((400, 300), 1.0);
    assert_eq!(find(&frame(&canvas), (336, 268), 1.0), None);
}

#[test]
fn half_size_markers_are_not_found() {
    let mut canvas = noisy((400, 300), 1.0);
    draw(&mut canvas, (64, 32), (336, 268), 0.5);
    assert_eq!(find(&frame(&canvas), (336, 268), 1.0), None);
}
