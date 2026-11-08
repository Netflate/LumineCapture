// Scrolling capture inside the editor: the live page shows through the region

use shot_stitch::{Axis, Edge, Status, StitchResult};
use tiny_skia::{
    BlendMode, FilterQuality, IntRect, Paint, Pattern, Pixmap, Rect, SpreadMode, Transform,
};

use crate::backend::{CaptureMethod, InputHole};
use crate::scroll::panel::{self, PanelButton, PanelLayout};
use crate::scroll::session::{ScrollSession, to_pixmap};
use crate::types::Capture;

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
    pub background: bool,
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
            background: true,
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

    pub fn paint_base(&self, capture: &Capture, dimmed: &mut Pixmap, canvas_scale: f32) {
        let (cw, ch) = (capture.pixmap.width(), capture.pixmap.height());
        let (dw, dh) = (dimmed.width(), dimmed.height());
        if let (Some(from), Some(at)) = (
            IntRect::from_xywh(0, 0, cw, ch),
            IntRect::from_xywh(0, 0, dw, dh),
        ) {
            paste(dimmed, at, &capture.pixmap, from);
            let mut paint = Paint::default();
            paint.set_color(crate::config::get().selection.dim.color());
            dimmed.fill_rect(at.to_rect(), &paint, Transform::identity(), None);
        }
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

/// `from` of `src` stretched over `at` of `dst`, replacing what was there.
fn paste(dst: &mut Pixmap, at: IntRect, src: &Pixmap, from: IntRect) {
    if (at.width(), at.height()) == (from.width(), from.height()) {
        copy(dst, (at.x(), at.y()), src, from);
        return;
    }
    let sx = at.width() as f32 / from.width() as f32;
    let sy = at.height() as f32 / from.height() as f32;
    let transform = Transform::from_row(
        sx,
        0.0,
        0.0,
        sy,
        at.x() as f32 - from.x() as f32 * sx,
        at.y() as f32 - from.y() as f32 * sy,
    );
    let paint = Paint {
        shader: Pattern::new(
            src.as_ref(),
            SpreadMode::Pad,
            FilterQuality::Bilinear,
            1.0,
            transform,
        ),
        blend_mode: BlendMode::Source,
        anti_alias: false,
        ..Paint::default()
    };
    dst.fill_rect(at.to_rect(), &paint, Transform::identity(), None);
}

fn copy(dst: &mut Pixmap, at: (i32, i32), src: &Pixmap, from: IntRect) {
    let (dw, dh) = (dst.width() as i32, dst.height() as i32);
    let (sw, sh) = (src.width() as i32, src.height() as i32);
    let x0 = (-at.0).max(-from.x()).max(0);
    let x1 = (from.width() as i32).min(dw - at.0).min(sw - from.x());
    if x1 <= x0 {
        return;
    }
    let len = (x1 - x0) as usize * 4;
    for row in 0..from.height() as i32 {
        let (sy, dy) = (from.y() + row, at.1 + row);
        if sy < 0 || sy >= sh || dy < 0 || dy >= dh {
            continue;
        }
        let s = (sy * sw + from.x() + x0) as usize * 4;
        let d = (dy * dw + at.0 + x0) as usize * 4;
        dst.data_mut()[d..d + len].copy_from_slice(&src.data()[s..s + len]);
    }
}
