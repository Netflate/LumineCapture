// Temporary overlay for the OCR tool (selection box, text block outlines, highlights).
// 
// Redraws on every mouse move over a freshly cleared background so transparent colors
// don't stack up.
//
// Performance note: Text blocks are outlined instead of filled because filling large
// areas on every move caused severe lag for almost invisible visual feedback. Hovering
// brightens the outline instead.

use tiny_skia::{Color, FillRule, Paint, Path, PathBuilder, Pixmap, Rect, Stroke, Transform};

use crate::ocr::OcrView;

const BLOCK_BORDER: (u8, u8, u8, u8) = (150, 180, 255, 48);
const BLOCK_BORDER_HOVER: (u8, u8, u8, u8) = (170, 200, 255, 110);
const UNDERLINE: (u8, u8, u8, u8) = (255, 255, 255, 56);
const HOVER_FILL: (u8, u8, u8, u8) = (255, 255, 255, 30);
const SELECT_FILL: (u8, u8, u8, u8) = (96, 152, 255, 120);

pub fn draw_ocr_overlay(canvas: &mut Pixmap, view: &OcrView, offset: (f32, f32)) {
    let (cw, ch) = (canvas.width() as f32, canvas.height() as f32);
    let to_local = |r: Rect| -> Option<Rect> {
        let local = Rect::from_ltrb(
            r.left() - offset.0,
            r.top() - offset.1,
            r.right() - offset.0,
            r.bottom() - offset.1,
        )?;
        (local.right() >= 0.0 && local.left() <= cw && local.bottom() >= 0.0 && local.top() <= ch)
            .then_some(local)
    };

    // Block chrome first, beneath the per-line washes.
    let mut smooth = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    let hairline = Stroke {
        width: 1.0,
        ..Stroke::default()
    };
    for block in view.block_overlays() {
        if !block.multiline {
            continue;
        }
        let Some(local) = to_local(block.bounds) else {
            continue;
        };
        let radius = (local.height() * 0.12).clamp(3.0, 8.0);
        let Some(path) = rounded_rect(local, radius) else {
            continue;
        };
        smooth.set_color(color(if block.hovered {
            BLOCK_BORDER_HOVER
        } else {
            BLOCK_BORDER
        }));
        canvas.stroke_path(&path, &smooth, &hairline, Transform::identity(), None);
    }

    let mut flat = Paint {
        anti_alias: false,
        ..Paint::default()
    };
    for (i, line) in view.lines.iter().enumerate() {
        let Some(local) = to_local(line.bounds) else {
            continue;
        };
        if let Some(sel) = view.line_selection(i) {
            // At least a hairline wide, so an empty span still reads as a caret.
            let span = Rect::from_ltrb(sel.x.0, sel.y.0, sel.x.1.max(sel.x.0 + 1.0), sel.y.1);
            if let Some(local) = span.and_then(&to_local) {
                fill_rect(canvas, local, SELECT_FILL, &mut flat);
            }
        } else if view.is_hovered(i) {
            fill_rect(canvas, local, HOVER_FILL, &mut flat);
        } else if !view.line_boxed(i)
            && let Some(underline) =
                Rect::from_xywh(local.left(), local.bottom() - 1.0, local.width(), 1.0)
        {
            fill_rect(canvas, underline, UNDERLINE, &mut flat);
        }
    }
}

fn color(c: (u8, u8, u8, u8)) -> Color {
    Color::from_rgba8(c.0, c.1, c.2, c.3)
}

fn fill_rect(canvas: &mut Pixmap, rect: Rect, c: (u8, u8, u8, u8), paint: &mut Paint) {
    paint.set_color(color(c));
    canvas.fill_path(
        &PathBuilder::from_rect(rect),
        paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );
}

/// A rectangle with rounded corners, clamped so the radius never exceeds half
/// the shorter side. Falls back to a plain rect when there's no room to round.
fn rounded_rect(rect: Rect, radius: f32) -> Option<Path> {
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    if r <= 0.0 {
        return Some(PathBuilder::from_rect(rect));
    }
    let (l, t, ri, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let mut pb = PathBuilder::new();
    pb.move_to(l + r, t);
    pb.line_to(ri - r, t);
    pb.quad_to(ri, t, ri, t + r);
    pb.line_to(ri, b - r);
    pb.quad_to(ri, b, ri - r, b);
    pb.line_to(l + r, b);
    pb.quad_to(l, b, l, b - r);
    pb.line_to(l, t + r);
    pb.quad_to(l, t, l + r, t);
    pb.close();
    pb.finish()
}
