// Overlay for the OCR tool: the shaded scan region, a plate behind every
// recognized line, the selection on top, and the progress badge that stands in
// for all of it while recognition is still running.
//
// Everything here is translucent and everything here is transient - it is
// repainted from the dim layer on every pointer move. Two rules keep that cheap
// and keep the washes from stacking up:
//
//   * `clip` is the area the caller has just restored from the dim layer, and
//     everything is painted into a scratch pixmap covering exactly that area,
//     which is then composited once. Shapes keep their true geometry - the
//     pixmap's own bounds do the cutting - and nothing lands outside the damage
//     the tool reported.
//   * the scratch pixmap starts transparent, so the washes composite against
//     each other exactly as they would drawn straight onto the canvas.

use std::collections::HashMap;
use tiny_skia::{Color, FillRule, Paint, Pixmap, Rect, Transform};
use usvg::Tree;

use super::paths::{draw_panel_border, draw_svg_icon, rect_bounds, rounded_rect_path};
use crate::ocr::OcrView;
use crate::types::icons;
use crate::types::panel::{BUTTON_SELECTED, ICON_COLOR};

/// Wash over the whole scanned area. Deliberately paired with `PLATE` below: the
/// shade pushes the screenshot back and the plate lifts the text roughly back to
/// where it started, so the lines read as the foreground without the region ever
/// getting as dark as the overlay dim outside it.
const REGION_SHADE: (u8, u8, u8, u8) = (10, 8, 20, 72);

/// Plate behind a block of text.
const PLATE: (u8, u8, u8, u8) = (255, 255, 255, 40);
const PLATE_HOVER: (u8, u8, u8, u8) = (255, 255, 255, 78);

const SELECT_FILL: (u8, u8, u8, u8) = (96, 152, 255, 130);

/// Grown around a block's own bounds so the plate reads as a box around the
/// text rather than a tight box on it.
const PLATE_PAD_X: f32 = 5.0;
const PLATE_PAD_Y: f32 = 3.0;
const PLATE_RADIUS: f32 = 5.0;

// ── progress badge ──────────────────────────────────────────────────────────

const BADGE_SIZE: f32 = 62.0;
const BADGE_RADIUS: f32 = 17.0;
const BADGE_BG: (u8, u8, u8, u8) = (17, 17, 27, 250);
const BADGE_ICON_SIZE: f32 = 30.0;
/// Sweeps per second, counting there and back as two.
const SCAN_RATE: f32 = 1.15;
const SCAN_WIDTH: f32 = 2.5;

pub fn draw_ocr_overlay(
    canvas: &mut Pixmap,
    view: &OcrView,
    offset: (f32, f32),
    clip: Option<&Rect>,
) {
    let Some(mut painter) = Painter::new(canvas, offset, clip) else {
        return;
    };

    if let Some(region) = view.region() {
        painter.fill(region, 0.0, REGION_SHADE);
    }

    for plate in view.block_plates() {
        let Some(bounds) = pad(plate.bounds, PLATE_PAD_X, PLATE_PAD_Y) else {
            continue;
        };
        let color = if plate.hovered { PLATE_HOVER } else { PLATE };
        painter.fill(bounds, PLATE_RADIUS, color);
    }

    for i in 0..view.lines.len() {
        let Some(sel) = view.line_selection(i) else {
            continue;
        };
        // At least a hairline wide, so an empty span still reads as a caret.
        let Some(span) = Rect::from_ltrb(
            sel.x.0,
            sel.y.0 - PLATE_PAD_Y,
            sel.x.1.max(sel.x.0 + 1.0),
            sel.y.1 + PLATE_PAD_Y,
        ) else {
            continue;
        };
        painter.fill(span, 2.0, SELECT_FILL);
    }

    painter.finish(canvas);
}

/// Progress badge, centred in the region being scanned: the tool's own icon with
/// a bar sweeping back and forth across it. Only the badge animates - the shade
/// underneath it is painted once, when the scan starts, and simply survives on
/// the canvas because nothing damages it until results land.
pub fn draw_ocr_scan(
    canvas: &mut Pixmap,
    region: Rect,
    phase: f32,
    offset: (f32, f32),
    icons_cache: &HashMap<&'static str, Tree>,
    clip: Option<&Rect>,
) {
    let Some(mut painter) = Painter::new(canvas, offset, clip) else {
        return;
    };
    painter.fill(region, 0.0, REGION_SHADE);

    let badge = scan_badge_rect(region);
    painter.fill(badge, BADGE_RADIUS, BADGE_BG);

    let left = badge.left() - offset.0;
    let top = badge.top() - offset.1;
    let (bx, by) = painter.to_buf((left, top));
    draw_panel_border(&mut painter.buf, bx, by, BADGE_SIZE, BADGE_SIZE, BADGE_RADIUS, 1.0);
    draw_svg_icon(
        &mut painter.buf,
        icons_cache,
        icons::OCR,
        BADGE_ICON_SIZE,
        bx + (BADGE_SIZE - BADGE_ICON_SIZE) / 2.0,
        by + (BADGE_SIZE - BADGE_ICON_SIZE) / 2.0,
        ICON_COLOR,
    );

    // Ping-pong, eased at both ends so the bar decelerates into each turn
    // instead of snapping back.
    let swing = (phase * SCAN_RATE).rem_euclid(2.0);
    let t = if swing > 1.0 { 2.0 - swing } else { swing };
    let t = t * t * (3.0 - 2.0 * t);

    let track = BADGE_ICON_SIZE + 6.0;
    let x = left + (BADGE_SIZE - track) / 2.0 + t * track;
    let bar_top = top + (BADGE_SIZE - track) / 2.0;

    let c = BUTTON_SELECTED.to_color_u8();
    let half = SCAN_WIDTH / 2.0;
    if let Some(bar) = Rect::from_ltrb(x - half, bar_top, x + half, bar_top + track) {
        painter.fill_local(bar, half, (c.red(), c.green(), c.blue(), 255));
    }

    painter.finish(canvas);
}

/// Where the badge sits, in global coordinates. The tool damages exactly this
/// rectangle each frame, so it has to agree with what is drawn.
pub fn scan_badge_rect(region: Rect) -> Rect {
    let cx = region.left() + region.width() / 2.0;
    let cy = region.top() + region.height() / 2.0;
    Rect::from_xywh(
        cx - BADGE_SIZE / 2.0,
        cy - BADGE_SIZE / 2.0,
        BADGE_SIZE,
        BADGE_SIZE,
    )
    .unwrap_or(region)
}

// ── clipped painting ────────────────────────────────────────────────────────

struct Painter {
    buf: Pixmap,
    origin: (f32, f32),
    offset: (f32, f32),
}

impl Painter {
    fn new(canvas: &Pixmap, offset: (f32, f32), clip: Option<&Rect>) -> Option<Self> {
        let (w, h) = (canvas.width(), canvas.height());
        let (x, y, cw, ch) = match clip {
            Some(c) => rect_bounds(c, w, h)?,
            None => (0, 0, w, h),
        };
        Some(Self {
            buf: Pixmap::new(cw, ch)?,
            origin: (x as f32, y as f32),
            offset,
        })
    }

    fn fill(&mut self, rect: Rect, radius: f32, color: (u8, u8, u8, u8)) {
        let Some(local) = Rect::from_ltrb(
            rect.left() - self.offset.0,
            rect.top() - self.offset.1,
            rect.right() - self.offset.0,
            rect.bottom() - self.offset.1,
        ) else {
            return;
        };
        self.fill_local(local, radius, color);
    }

    fn fill_local(&mut self, rect: Rect, radius: f32, color: (u8, u8, u8, u8)) {
        let Some(rect) = Rect::from_ltrb(
            rect.left() - self.origin.0,
            rect.top() - self.origin.1,
            rect.right() - self.origin.0,
            rect.bottom() - self.origin.1,
        ) else {
            return;
        };
        if rect.right() <= 0.0
            || rect.bottom() <= 0.0
            || rect.left() >= self.buf.width() as f32
            || rect.top() >= self.buf.height() as f32
        {
            return;
        }

        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba8(color.0, color.1, color.2, color.3));
        paint.anti_alias = radius > 0.0;

        let path = if radius > 0.0 {
            rounded_rect_path(&rect, radius, true, true, true, true)
        } else {
            Some(tiny_skia::PathBuilder::from_rect(rect))
        };
        let Some(path) = path else { return };

        self.buf
            .fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
    }

    fn to_buf(&self, local: (f32, f32)) -> (f32, f32) {
        (local.0 - self.origin.0, local.1 - self.origin.1)
    }

    fn finish(self, canvas: &mut Pixmap) {
        canvas.draw_pixmap(
            self.origin.0 as i32,
            self.origin.1 as i32,
            self.buf.as_ref(),
            &tiny_skia::PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }
}

fn pad(rect: Rect, x: f32, y: f32) -> Option<Rect> {
    Rect::from_ltrb(
        rect.left() - x,
        rect.top() - y,
        rect.right() + x,
        rect.bottom() + y,
    )
}
