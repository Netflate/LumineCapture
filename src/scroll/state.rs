// Scrolling capture inside the editor: the live page shows through the region

use shot_stitch::{Axis, Edge, Status, StitchResult};
use tiny_skia::{IntRect, Pixmap, Rect};

use crate::backend::{CaptureMethod, InputHole};
use crate::scroll::panel::{self, PanelButton, PanelLayout};
use crate::scroll::session::{ScrollSession, to_pixmap};

pub struct Scroll {
    pub session: ScrollSession,
    pub monitor_idx: usize,
    /// monitor-local, logical
    pub region: Rect,
    /// same region in global coordinates
    pub zone: Rect,
    /// region in pixels of captured frames
    pub crop: IntRect,
    /// pixels of captured frames per logical one
    pub scale: f32,
    /// rows the long image grew above the first frame
    lead: i32,
    pub layout: PanelLayout,
    pub hovered: Option<PanelButton>,
    pub capture: Box<dyn CaptureMethod>,
    pub backgrounds: Vec<bool>,
    pub input: Option<Option<InputHole>>,
    long: Option<(u64, Pixmap)>,
}

impl Scroll {
    pub fn new(
        session: ScrollSession,
        zone: Rect,
        crop: IntRect,
        scale: f32,
        monitor_size: (f32, f32),
        capture: Box<dyn CaptureMethod>,
        monitors: usize,
    ) -> Self {
        let region = session.region;
        Self {
            monitor_idx: session.monitor_idx,
            layout: panel::layout(region, monitor_size, None),
            session,
            region,
            zone,
            crop,
            scale,
            lead: 0,
            hovered: None,
            capture,
            backgrounds: vec![true; monitors],
            input: None,
            long: None,
        }
    }

    pub fn live(&self) -> bool {
        !self.session.paused
    }

    pub fn axis(&self) -> Axis {
        self.session.axis()
    }

    pub fn contains(&self, local: (f64, f64)) -> bool {
        inside(&self.region, local)
    }

    pub fn grown(&self) -> bool {
        let (w, h) = self.session.stitcher.dimensions();
        match self.axis() {
            Axis::Vertical => h > self.crop.height() as i32,
            Axis::Horizontal => w > self.crop.width() as i32,
        }
    }

    pub fn observe(&mut self, result: &StitchResult) {
        match result.status {
            Status::FirstFrame => self.lead = 0,
            Status::Appended if result.edge == Edge::Start => self.lead += result.added,
            _ => {}
        }
    }

    pub fn reset(&mut self) {
        self.lead = 0;
    }

    /// long image in global coordinates, as when shot started
    pub fn doc_rect(&self) -> Option<Rect> {
        let (w, h) = self.session.stitcher.dimensions();
        let lead = self.lead as f32 / self.scale;
        let (x, y) = match self.axis() {
            Axis::Vertical => (self.zone.left(), self.zone.top() - lead),
            Axis::Horizontal => (self.zone.left() - lead, self.zone.top()),
        };
        Rect::from_xywh(x, y, w as f32 / self.scale, h as f32 / self.scale)
    }

    pub fn long_pixmap(&mut self) -> Option<&Pixmap> {
        let revision = self.session.stitcher.revision();
        if self.long.as_ref().is_none_or(|(at, _)| *at != revision) {
            let image = self.session.result()?;
            self.long = Some((revision, to_pixmap(&image)?));
        }
        self.long.as_ref().map(|(_, pixmap)| pixmap)
    }

    pub fn paint_region(&self, dimmed: &mut Pixmap, canvas_scale: f32) {
        if let Some(target) = canvas_rect(self.region, canvas_scale, dimmed) {
            clear(dimmed, target);
        }
    }

    pub fn paint_base(&self, dimmed: &mut Pixmap, canvas_scale: f32) {
        fill_live(dimmed);
        self.paint_region(dimmed, canvas_scale);
    }

    pub fn input_hole(&self) -> Option<InputHole> {
        Some(InputHole {
            hole: logical_rect(self.region),
            keep: Some(logical_rect(self.layout.rect)),
        })
    }

    pub fn relayout(&mut self, monitor_size: (f32, f32)) -> bool {
        let preview = self.session.preview().map(|p| (p.width(), p.height()));
        let layout = panel::layout(self.region, monitor_size, preview);
        let moved = layout.rect != self.layout.rect;
        self.layout = layout;
        moved
    }
}

pub fn fill_live(dimmed: &mut Pixmap) {
    dimmed.fill(crate::config::get().selection.scroll_dim.color());
}

pub fn inside(rect: &Rect, (x, y): (f64, f64)) -> bool {
    let (x, y) = (x as f32, y as f32);
    x >= rect.left() && x < rect.right() && y >= rect.top() && y < rect.bottom()
}

fn logical_rect(rect: Rect) -> (i32, i32, i32, i32) {
    let (l, t) = (rect.left().round() as i32, rect.top().round() as i32);
    let (r, b) = (rect.right().round() as i32, rect.bottom().round() as i32);
    (l, t, r - l, b - t)
}

/// rounded to the nearest pixel
fn canvas_rect(region: Rect, scale: f32, pixmap: &Pixmap) -> Option<IntRect> {
    let px = |v: f32, max: u32| ((v * scale).round() as i32).clamp(0, max as i32);
    let (w, h) = (pixmap.width(), pixmap.height());
    IntRect::from_ltrb(
        px(region.left(), w),
        px(region.top(), h),
        px(region.right(), w),
        px(region.bottom(), h),
    )
}

fn clear(pixmap: &mut Pixmap, at: IntRect) {
    let stride = pixmap.width() as usize * 4;
    let len = at.width() as usize * 4;
    for row in at.y()..at.y() + at.height() as i32 {
        let start = row as usize * stride + at.x() as usize * 4;
        pixmap.data_mut()[start..start + len].fill(0);
    }
}
