// Scrolling capture session: manages capture intervals, stitcher state, preview, and results.

use log::info;
use shot_image::Image;
use shot_stitch::{Axis, Config, Status, StitchResult, Stitcher};
use std::time::{Duration, Instant};
use tiny_skia::{Pixmap, PremultipliedColorU8, Rect};

use crate::scroll::frame;
use crate::scroll::preview;

const CAPTURE_INTERVAL_MS: u64 = 45;
/// Polling while nothing moves. A flick right after a pause moves the page by a screen or
/// more within a few hundred milliseconds: wait longer and the next frame shares nothing with
/// the last one, the stitcher loses the page and polling never speeds up again.
const IDLE_INTERVAL_MS: u64 = 120;
const SIGNATURE_COLS: usize = 18;
const SIGNATURE_ROWS: usize = 24;
const PREVIEW_WIDTH: u32 = 132;
const IDLE_AFTER_MS: u128 = 1000;

pub struct ScrollSession {
    pub paused: bool,
    pub status: String,
    pub stitcher: Stitcher,
    pub region: Rect,
    pub monitor_idx: usize,
    last_signature: Vec<u8>,
    last_capture: std::time::Instant,
    /// last screen change, regardless of stitcher state
    last_motion: std::time::Instant,
    /// whether the screen has stayed unchanged
    settled: bool,
    preview: Option<Pixmap>,
    preview_revision: u64,
    pub frame_count: u32,
    pub total_height: i32,
    pub last_append: i32,
}

impl ScrollSession {
    pub fn new(region: Rect, monitor_idx: usize) -> Result<Self, String> {
        if region.width() < 16.0 || region.height() < 16.0 {
            return Err("the scroll region is too small".into());
        }
        let stitcher = Stitcher::new(Config::default()).map_err(|e| e.to_string())?;
        Ok(Self {
            paused: false,
            status: "Waiting for scroll".into(),
            stitcher,
            region,
            monitor_idx,
            last_signature: Vec::new(),
            last_capture: std::time::Instant::now(),
            last_motion: std::time::Instant::now(),
            settled: false,
            preview: None,
            preview_revision: u64::MAX,
            frame_count: 0,
            total_height: 0,
            last_append: 0,
        })
    }

    /// Current capture interval: backs off when idle, polls fast during scrolling.
    pub fn capture_interval(&self) -> Duration {
        let idle = self.last_motion.elapsed().as_millis() >= IDLE_AFTER_MS;
        Duration::from_millis(if idle {
            IDLE_INTERVAL_MS
        } else {
            CAPTURE_INTERVAL_MS
        })
    }

    pub fn next_capture(&self) -> Instant {
        self.last_capture + self.capture_interval()
    }

    pub fn wants_capture(&self, now: Instant) -> bool {
        if self.paused {
            return false;
        }
        now.duration_since(self.last_capture) >= self.capture_interval()
    }

    /// Checks whether scrolling has stopped.
    pub fn should_idle(&self, now: Instant) -> bool {
        self.frame_count > 0
            && !self.paused
            && now.duration_since(self.last_motion).as_millis() >= IDLE_AFTER_MS
    }

    /// Feeds a new frame into the stitcher, `None` when it was skipped.
    pub fn push(&mut self, image: Image) -> Option<StitchResult> {
        self.last_capture = std::time::Instant::now();

        // skip identical frames to avoid refreshing anchors repeatedly, all but the first:
        // once the page stops, the stitcher takes in what moved by less than it waits for
        let signature = frame::signature(&image, SIGNATURE_COLS, SIGNATURE_ROWS);
        if frame::is_duplicate(&self.last_signature, &signature) {
            if self.settled {
                self.status = "Waiting for scroll".into();
                return None;
            }
            self.settled = true;
        } else {
            self.last_signature = signature;
            self.last_motion = Instant::now();
            self.settled = false;
        }

        let outcome = match self.stitcher.push(image) {
            Ok(outcome) => outcome,
            Err(e) => {
                log::error!("stitcher failed: {e}");
                self.status = "Capture error".into();
                self.paused = true;
                return None;
            }
        };
        self.apply(outcome);
        Some(outcome)
    }

    fn apply(&mut self, outcome: StitchResult) {
        let stats = self.stitcher.stats();
        self.frame_count = stats.frame_count.max(0) as u32;
        self.total_height = stats.total_height;
        self.last_append = stats.last_append;
        self.status = match outcome.status {
            Status::FirstFrame => "Capturing".into(),
            Status::Appended => "Capturing".into(),
            Status::NoProgress => "Waiting for scroll".into(),
            // the page went further than one frame to the next can follow
            Status::NoMatch => "Lost it, scroll back".into(),
        };
    }

    pub fn axis(&self) -> Axis {
        self.stitcher.axis()
    }

    pub fn can_toggle_axis(&self) -> bool {
        !self.stitcher.axis_locked()
    }

    pub fn toggle_axis(&mut self) -> bool {
        let next = match self.stitcher.axis() {
            Axis::Vertical => Axis::Horizontal,
            Axis::Horizontal => Axis::Vertical,
        };
        match self.stitcher.set_axis(next) {
            Ok(true) => {
                self.last_signature.clear();
                self.status = "Waiting for scroll".into();
                info!("scrolling capture switched axis to {next:?}");
                true
            }
            Ok(false) => false,
            Err(e) => {
                log::error!("failed to switch axis: {e}");
                false
            }
        }
    }

    pub fn preview(&mut self) -> Option<&Pixmap> {
        let revision = self.stitcher.revision();
        if self.preview.is_none() || revision != self.preview_revision {
            let image = self.stitcher.full_image().ok().flatten();
            self.preview = image
                .as_ref()
                .and_then(|image| preview::thumbnail(image, PREVIEW_WIDTH));
            self.preview_revision = revision;
        }
        self.preview.as_ref()
    }

    pub fn result(&mut self) -> Option<Image> {
        self.stitcher.full_image().ok().flatten()
    }

    pub fn length_label(&self) -> String {
        let (width, height) = self.stitcher.dimensions();
        let length = match self.stitcher.axis() {
            Axis::Vertical => height,
            Axis::Horizontal => width,
        };
        if length <= 0 {
            return String::new();
        }
        format!("{length} px")
    }

    pub fn last_append(&self) -> i32 {
        self.last_append
    }
}

pub fn to_pixmap(image: &Image) -> Option<Pixmap> {
    let (w, h) = (image.width(), image.height());
    if w <= 0 || h <= 0 {
        return None;
    }
    let mut pixmap = Pixmap::new(w as u32, h as u32)?;
    let dst = pixmap.pixels_mut();
    for y in 0..h {
        let row = image.row(y);
        for x in 0..w {
            let pixel = row[x as usize];
            let r = ((pixel >> 16) & 255) as u8;
            let g = ((pixel >> 8) & 255) as u8;
            let b = (pixel & 255) as u8;
            dst[(y as u32 * w as u32 + x as u32) as usize] =
                PremultipliedColorU8::from_rgba(r, g, b, 255)
                    .unwrap_or(PremultipliedColorU8::TRANSPARENT);
        }
    }
    Some(pixmap)
}
