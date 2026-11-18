// Scrolling capture inside the editor: the live page shows through the region

use std::time::Instant;

use shot_stitch::{Axis, Edge, Status, StitchResult};
use tiny_skia::{IntRect, Pixmap, Rect};

use crate::backend::{CaptureMethod, InputHole};
use crate::scroll::panel::{self, PanelButton, PanelLayout, Slot};
use crate::scroll::session::{ScrollSession, to_pixmap};

/// Clear: frozen screenshot still to drop once the canvas has the dimmed one;
/// Cleared: dropped, put back when scrolling stops; Keep: never touched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Snapshot {
    Clear,
    Cleared,
    Keep,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locate {
    Off,
    Pending,
    Measuring {
        window: (u32, u32),
        origin: (i32, i32),
        attempts: u32,
        since: Instant,
        painted: bool,
    },
    Done,
}

pub struct Scroll {
    pub session: ScrollSession,
    pub monitor_idx: usize,
    /// monitor-local, logical
    pub region: Rect,
    /// same region in global coordinates
    pub zone: Rect,
    /// region in pixels of captured frames
    pub full_crop: IntRect,
    pub crop: IntRect,
    /// pixels of captured frames per logical one
    pub scale: f32,
    /// rows the long image grew above the first frame
    lead: i32,
    pub layout: PanelLayout,
    slot: Slot,
    compact: bool,
    pub hovered: Option<PanelButton>,
    pub capture: Box<dyn CaptureMethod>,
    pub snapshots: Vec<Snapshot>,
    pub bounds: Rect,
    pub locate: Locate,
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
        let slot = panel::choose_slot(region, monitor_size);
        let compact = slot == Slot::Inside && capture.captures_overlay();
        let layout = panel::layout(region, monitor_size, None, slot, compact);
        let mut scroll = Self {
            monitor_idx: session.monitor_idx,
            layout,
            slot,
            compact,
            session,
            region,
            zone,
            full_crop: crop,
            crop,
            scale,
            lead: 0,
            hovered: None,
            capture,
            snapshots: vec![Snapshot::Clear; monitors],
            bounds: Rect::from_xywh(0.0, 0.0, monitor_size.0, monitor_size.1).unwrap_or(region),
            locate: Locate::Off,
            input: None,
            long: None,
        };
        scroll.crop = scroll.crop_for(scroll.axis());
        scroll
    }

    fn crop_for(&self, axis: Axis) -> IntRect {
        let full = self.full_crop;
        if !self.compact {
            return full;
        }
        let gap = crate::theme::size::margin();
        let panel = self.layout.rect;
        let (shrunk, kept, whole) = match axis {
            Axis::Vertical => {
                let bottom = ((panel.top() - gap - 2.0) * self.scale).floor() as i32;
                (
                    IntRect::from_ltrb(full.left(), full.top(), full.right(), bottom),
                    bottom - full.top(),
                    full.height() as i32,
                )
            }
            Axis::Horizontal => {
                let right = ((panel.left() - gap - 2.0) * self.scale).floor() as i32;
                (
                    IntRect::from_ltrb(full.left(), full.top(), right, full.bottom()),
                    right - full.left(),
                    full.width() as i32,
                )
            }
        };
        shrunk.filter(|_| kept * 2 >= whole).unwrap_or(full)
    }

    pub fn toggle_axis(&mut self) -> bool {
        let next = match self.axis() {
            Axis::Vertical => Axis::Horizontal,
            Axis::Horizontal => Axis::Vertical,
        };
        let crop = self.crop_for(next);
        let switched = if crop == self.crop {
            self.session.toggle_axis()
        } else {
            self.session.restart(next)
        };
        if switched {
            self.crop = crop;
            self.reset();
        }
        switched
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
        self.long = None;
    }

    pub fn restart(&mut self) {
        if self.session.restart(self.axis()) {
            self.session.paused = false;
            self.reset();
        }
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

    pub fn relayout(&mut self) -> bool {
        let preview = self.session.preview().map(|p| (p.width(), p.height()));
        let layout = self.bounded_layout(self.slot, preview, self.compact);
        let moved = layout.rect != self.layout.rect;
        self.layout = layout;
        moved
    }

    pub fn set_bounds(&mut self, bounds: Rect) {
        self.bounds = bounds;
        let local = shift(self.region, -bounds.left(), -bounds.top());
        self.slot = panel::choose_slot(local, (bounds.width(), bounds.height()));
        self.compact = self.slot == Slot::Inside && self.capture.captures_overlay();
        self.relayout();
        self.crop = self.crop_for(self.axis());
    }

    fn bounded_layout(
        &self,
        slot: Slot,
        preview: Option<(u32, u32)>,
        compact: bool,
    ) -> PanelLayout {
        let bounds = self.bounds;
        let local = shift(self.region, -bounds.left(), -bounds.top());
        let size = (bounds.width(), bounds.height());
        panel::layout(local, size, preview, slot, compact).translated(bounds.left(), bounds.top())
    }
}

pub fn fill_live(dimmed: &mut Pixmap) {
    dimmed.fill(crate::config::get().selection.scroll_dim.color());
}

pub fn inside(rect: &Rect, (x, y): (f64, f64)) -> bool {
    let (x, y) = (x as f32, y as f32);
    x >= rect.left() && x < rect.right() && y >= rect.top() && y < rect.bottom()
}

fn shift(rect: Rect, dx: f32, dy: f32) -> Rect {
    Rect::from_xywh(
        rect.left() + dx,
        rect.top() + dy,
        rect.width(),
        rect.height(),
    )
    .unwrap_or(rect)
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
