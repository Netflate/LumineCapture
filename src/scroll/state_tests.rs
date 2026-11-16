use crate::backend::CaptureMethod;
use crate::scroll::{Scroll, ScrollSession, frame};
use crate::types::{CaptureResult, MonitorFrame, Output, StreamInfo};
use tiny_skia::{IntRect, Pixmap, Rect};

struct NoCapture;

#[async_trait::async_trait]
impl CaptureMethod for NoCapture {
    async fn capture_frame(
        &self,
        _: &[Output],
    ) -> Result<CaptureResult, Box<dyn std::error::Error>> {
        Err("no capture in tests".into())
    }
}

const W: u32 = 40;
const H: u32 = 160;

fn page(origin: i32) -> MonitorFrame {
    let mut pixels = Vec::with_capacity((W * H * 4) as usize);
    for y in origin..origin + H as i32 {
        for x in 0..W {
            let hash = (y as u32).wrapping_mul(2_654_435_761).rotate_left(7) ^ x;
            pixels.extend_from_slice(&[hash as u8, (hash >> 8) as u8, (hash >> 16) as u8, 255]);
        }
    }
    MonitorFrame {
        output: 0,
        pixels,
        pw_width: W,
        pw_height: H,
        pw_stride: W * 4,
        info: StreamInfo {
            node_id: 0,
            size: Some((W as i32, H as i32)),
            position: Some((0, 0)),
        },
    }
}

/// a monitor at (100, 50), the region is its top-left corner
fn scroll() -> Scroll {
    let region = Rect::from_xywh(0.0, 0.0, W as f32, H as f32).unwrap();
    let session = ScrollSession::new(region, 0).unwrap();
    let zone = Rect::from_xywh(100.0, 50.0, W as f32, H as f32).unwrap();
    let crop = IntRect::from_xywh(0, 0, W, H).unwrap();
    Scroll::new(
        session,
        zone,
        crop,
        1.0,
        (400.0, 400.0),
        Box::new(NoCapture),
        1,
    )
}

fn show(scroll: &mut Scroll, origin: i32) {
    let image = frame::crop(&page(origin), (0, 0, W, H)).unwrap();
    if let Some(result) = scroll.session.push(image) {
        scroll.observe(&result);
    }
}

fn row(pixmap: &Pixmap, y: u32) -> Vec<u8> {
    let start = (y * pixmap.width() * 4) as usize;
    pixmap.data()[start..start + (W * 4) as usize].to_vec()
}

#[test]
fn scrolling_above_the_start_moves_the_long_image_up() {
    let mut scroll = scroll();
    show(&mut scroll, 0);
    show(&mut scroll, -40);
    let doc = scroll.doc_rect().unwrap();
    assert_eq!((doc.left(), doc.top(), doc.height()), (100.0, 10.0, 200.0));
}

#[test]
fn grown_once_the_page_has_moved() {
    let mut scroll = scroll();
    assert!(!scroll.grown());
    show(&mut scroll, 0);
    show(&mut scroll, 0);
    assert!(!scroll.grown());
    show(&mut scroll, 40);
    assert!(scroll.grown());
}

#[test]
fn live_region_is_a_hole_in_the_base() {
    let mut scroll = scroll();
    show(&mut scroll, 0);
    let mut dimmed = Pixmap::new(400, 400).unwrap();
    scroll.paint_base(&mut dimmed, 1.0);

    assert!(row(&dimmed, 0).iter().all(|&b| b == 0));
    let outside = dimmed.pixel(W + 5, 0).unwrap();
    assert_eq!(outside.alpha(), crate::config::get().selection.scroll_dim.3);
    assert!(outside.alpha() < 255, "the live screen shows through");
}

fn cornered() -> Scroll {
    let region = Rect::from_xywh(0.0, 0.0, 400.0, 400.0).unwrap();
    let session = ScrollSession::new(region, 0).unwrap();
    let zone = Rect::from_xywh(0.0, 0.0, 400.0, 400.0).unwrap();
    let crop = IntRect::from_xywh(0, 0, 400, 400).unwrap();
    Scroll::new(
        session,
        zone,
        crop,
        1.0,
        (400.0, 400.0),
        Box::new(NoCapture),
        1,
    )
}

#[test]
fn the_crop_ends_before_a_panel_the_frames_would_show() {
    let scroll = cornered();
    assert_eq!(scroll.full_crop.height(), 400);
    assert!(scroll.crop.height() < 400);
    assert!(scroll.crop.bottom() as f32 <= scroll.layout.rect.top());
    assert_eq!((scroll.crop.x(), scroll.crop.y()), (0, 0));
    assert_eq!(scroll.crop.width(), 400);
    assert!(scroll.layout.preview.is_none());
}

#[test]
fn a_crop_cut_to_less_than_half_stays_whole() {
    let region = Rect::from_xywh(0.0, 0.0, 400.0, 150.0).unwrap();
    let session = ScrollSession::new(region, 0).unwrap();
    let zone = Rect::from_xywh(0.0, 0.0, 400.0, 150.0).unwrap();
    let crop = IntRect::from_xywh(0, 0, 400, 150).unwrap();
    let scroll = Scroll::new(
        session,
        zone,
        crop,
        1.0,
        (400.0, 150.0),
        Box::new(NoCapture),
        1,
    );
    assert_eq!(scroll.crop, scroll.full_crop);
}

#[test]
fn switching_the_axis_recuts_the_crop_and_starts_stitching_over() {
    let mut scroll = cornered();
    let vertical = scroll.crop;
    let image = frame::crop(&page(0), (0, 0, W, H)).unwrap();
    scroll.session.push(image);
    assert!(scroll.toggle_axis());
    assert_eq!(scroll.axis(), shot_stitch::Axis::Horizontal);
    assert_ne!(scroll.crop, vertical);
    assert_eq!(scroll.crop.height(), 400);
    assert_eq!(scroll.session.stitcher.dimensions(), (0, 0));
    assert!(scroll.toggle_axis());
    assert_eq!(scroll.crop, vertical);
}
