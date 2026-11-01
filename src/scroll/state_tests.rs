use crate::backend::CaptureMethod;
use crate::scroll::{Scroll, ScrollSession, frame};
use crate::types::{Capture, CaptureResult, MonitorFrame, Output, StreamInfo};
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

fn page_row(y: i32) -> Vec<u8> {
    page(y).pixels[..(W * 4) as usize].to_vec()
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

fn screen() -> (Capture, Pixmap) {
    let capture = Capture {
        pixmap: Pixmap::new(400, 400).unwrap(),
        scale: 1.0,
    };
    (capture, Pixmap::new(400, 400).unwrap())
}

#[test]
fn live_view_follows_the_page() {
    let mut scroll = scroll();
    show(&mut scroll, 0);
    show(&mut scroll, 40);
    show(&mut scroll, 100);
    assert_eq!(scroll.view, 100.0);
    assert_eq!(scroll.range(), (0.0, 100.0));
    assert_eq!(scroll.shift(), (0.0, 100.0));
}

#[test]
fn scrolling_above_the_start_moves_the_long_image_up() {
    let mut scroll = scroll();
    show(&mut scroll, 0);
    show(&mut scroll, -40);
    assert_eq!(scroll.view, -40.0);
    assert_eq!(scroll.range(), (-40.0, 0.0));
    let doc = scroll.doc_rect().unwrap();
    assert_eq!((doc.left(), doc.top(), doc.height()), (100.0, 10.0, 200.0));
}

#[test]
fn paused_view_stays_on_the_long_image() {
    let mut scroll = scroll();
    show(&mut scroll, 0);
    show(&mut scroll, 50);
    show(&mut scroll, 100);
    scroll.session.paused = true;

    assert!(scroll.scroll_by(-30.0));
    assert_eq!(scroll.view, 70.0);
    assert!(scroll.scroll_by(500.0));
    assert_eq!(scroll.view, 100.0);
    assert!(!scroll.scroll_by(1.0));
    assert!(scroll.scroll_by(-1000.0));
    assert_eq!(scroll.view, 0.0);

    scroll.follow_page();
    assert_eq!(scroll.view, 100.0);
}

#[test]
fn paused_region_shows_the_long_image_at_the_view() {
    let mut scroll = scroll();
    show(&mut scroll, 0);
    show(&mut scroll, 50);
    show(&mut scroll, 100);
    scroll.session.paused = true;
    scroll.scroll_by(-60.0);

    let (mut capture, mut dimmed) = screen();
    scroll.paint_region(&mut capture, &mut dimmed, 1.0);
    assert_eq!(row(&capture.pixmap, 0), page_row(40));
    assert_eq!(row(&capture.pixmap, H - 1), page_row(40 + H as i32 - 1));
    assert_eq!(row(&dimmed, 0), page_row(40));
}

#[test]
fn live_region_is_a_hole_in_the_base() {
    let mut scroll = scroll();
    show(&mut scroll, 0);
    let (mut capture, mut dimmed) = screen();
    capture.pixmap.fill(tiny_skia::Color::WHITE);
    scroll.paint_base(&mut capture, &mut dimmed, 1.0);

    assert!(row(&dimmed, 0).iter().all(|&b| b == 0));
    let outside = dimmed.pixel(W + 5, 0).unwrap();
    assert_eq!(outside.alpha(), 255);
    assert!(outside.red() < 255, "the snapshot outside is dimmed");
}

#[test]
fn compose_reads_the_long_image_in_global_coordinates() {
    let mut scroll = scroll();
    show(&mut scroll, 0);
    show(&mut scroll, -40);
    let top = Rect::from_xywh(100.0, 10.0, W as f32, 1.0).unwrap();
    let (pixmap, origin) = scroll.compose(top).unwrap();
    assert_eq!(origin, (100, 10));
    assert_eq!(row(&pixmap, 0), page_row(-40));
}
