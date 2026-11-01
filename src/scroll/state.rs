// Scrolling capture inside the editor: the live page shows through the region while
// capturing, the stitched long image (scrolled by the wheel) while drawing

use shot_stitch::{Axis, Edge, Status, StitchResult};
use tiny_skia::{
    BlendMode, FilterQuality, IntRect, Paint, Pattern, Pixmap, PixmapPaint, Rect, SpreadMode,
    Transform,
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
    /// how far the region is scrolled from where the shot started, logical
    pub view: f32,
    /// where the page itself was at the last capture
    page: f32,
    /// rows the long image grew above the first frame
    lead: i32,
    pub layout: PanelLayout,
    pub hovered: Option<PanelButton>,
    pub capture: Box<dyn CaptureMethod>,
    pub layer_stale: bool,
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
            view: 0.0,
            page: 0.0,
            lead: 0,
            hovered: None,
            capture,
            layer_stale: true,
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

    pub fn shift(&self) -> (f32, f32) {
        match self.axis() {
            Axis::Vertical => (0.0, self.view),
            Axis::Horizontal => (self.view, 0.0),
        }
    }

    pub fn contains(&self, local: (f64, f64)) -> bool {
        inside(&self.region, local)
    }

    fn lengths(&self) -> (i32, i32) {
        let (w, h) = self.session.stitcher.dimensions();
        match self.axis() {
            Axis::Vertical => (h, self.crop.height() as i32),
            Axis::Horizontal => (w, self.crop.width() as i32),
        }
    }

    pub fn range(&self) -> (f32, f32) {
        let (long, frame) = self.lengths();
        let min = -self.lead as f32 / self.scale;
        let max = (long - self.lead - frame) as f32 / self.scale;
        (min, max.max(min))
    }

    pub fn observe(&mut self, result: &StitchResult) {
        match result.status {
            Status::NoMatch => return,
            Status::FirstFrame => self.lead = 0,
            Status::Appended if result.edge == Edge::Start => self.lead += result.added,
            _ => {}
        }
        let (min, max) = self.range();
        self.page = ((result.position - self.lead) as f32 / self.scale).clamp(min, max);
        if self.live() {
            self.view = self.page;
        }
    }

    pub fn reset(&mut self) {
        self.lead = 0;
        self.view = 0.0;
        self.page = 0.0;
    }

    pub fn follow_page(&mut self) {
        self.view = self.page;
    }

    pub fn scroll_by(&mut self, delta: f32) -> bool {
        let (min, max) = self.range();
        // whole pixels of the long image, or the annotations would sit
        // a fraction of a pixel off the rows under them
        let view = ((self.view + delta) * self.scale).round() / self.scale;
        let view = view.clamp(min, max);
        let moved = view != self.view;
        self.view = view;
        moved
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

    /// Logical pixels of the long image under a global `region`, like `renderer::composite`.
    pub fn compose(&mut self, region: Rect) -> Option<(Pixmap, (i32, i32))> {
        let doc = self.doc_rect()?;
        let scale = self.scale;
        let left = region.left().floor() as i32;
        let top = region.top().floor() as i32;
        let width = (region.right().ceil() as i32 - left).max(0) as u32;
        let height = (region.bottom().ceil() as i32 - top).max(0) as u32;
        let mut out = Pixmap::new(width, height)?;
        let long = self.long_pixmap()?;
        let transform = Transform::from_row(
            1.0 / scale,
            0.0,
            0.0,
            1.0 / scale,
            doc.left() - left as f32,
            doc.top() - top as f32,
        );
        out.draw_pixmap(
            0,
            0,
            long.as_ref(),
            &PixmapPaint {
                quality: FilterQuality::Bicubic,
                ..PixmapPaint::default()
            },
            transform,
            None,
        );
        Some((out, (left, top)))
    }

    fn window(&self, long: &Pixmap) -> Option<IntRect> {
        let start = self.lead + (self.view * self.scale).round() as i32;
        let (w, h) = (long.width() as i32, long.height() as i32);
        match self.axis() {
            Axis::Vertical => {
                let len = (self.crop.height() as i32).min(h);
                IntRect::from_xywh(0, start.clamp(0, h - len), w as u32, len as u32)
            }
            Axis::Horizontal => {
                let len = (self.crop.width() as i32).min(w);
                IntRect::from_xywh(start.clamp(0, w - len), 0, len as u32, h as u32)
            }
        }
    }

    /// capture gets the same pixels, so the loupe and the eyedropper see them too.
    pub fn paint_region(&mut self, capture: &mut Capture, dimmed: &mut Pixmap, canvas_scale: f32) {
        let Some(target) = canvas_rect(self.region, canvas_scale, dimmed) else {
            return;
        };
        if self.live() {
            clear(dimmed, target);
            return;
        }
        let crop = self.crop;
        self.long_pixmap();
        if let Some((_, long)) = self.long.as_ref()
            && let Some(window) = self.window(long)
        {
            paste(&mut capture.pixmap, crop, long, window);
        }
        paste(dimmed, target, &capture.pixmap, crop);
    }

    pub fn paint_base(&mut self, capture: &mut Capture, dimmed: &mut Pixmap, canvas_scale: f32) {
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
        self.paint_region(capture, dimmed, canvas_scale);
    }

    pub fn input_hole(&self) -> Option<InputHole> {
        self.live().then(|| InputHole {
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
