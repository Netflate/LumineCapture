// Frame building and shared drawing stuff: paths, text, and annotations.
// UI component rendering lives in ui/<component>/draw.rs
// OCR overlay rendering lives in ocr/draw.rs alongside its state

mod annotations;
pub mod blur;
mod painter;
pub mod paths;
pub mod text;

pub use annotations::{
    draw_annotation, selection_chrome_pad, shadow_color_for, stroke_pen_segment, visual_pad,
};
pub use painter::{Icons, Painter};
pub use paths::{rect_bounds, rounded_rect_path, scale_rect};
pub use text::measure_line_width;

use crate::types::SelectionEdges;
use crate::types::annotations::Annotation;
use crate::ui::color_popover::ColorPickerPopover;
use crate::ui::magnifier::MagnifierState;
use crate::ui::settings_panel::SettingsPanel;
use crate::ui::toolbar::Toolbar;
use cosmic_text::{Editor, FontSystem, SwashCache};
use std::collections::HashMap;
use tiny_skia::{Color, PathBuilder, Pixmap, Rect, Stroke, Transform};
use usvg::Tree;

pub struct TextContext<'a> {
    pub font_system: &'a mut FontSystem,
    pub swash_cache: &'a mut SwashCache,
    pub editors: &'a mut HashMap<u64, Editor<'static>>,
}

pub struct RenderRequest<'a> {
    // basic layers
    pub canvas: &'a mut Pixmap,
    pub capture: &'a crate::types::Capture,
    pub dimmed: &'a mut Pixmap,
    // selection + magnifier + toolbar
    pub selection: Option<&'a Rect>,
    pub prev_selection: Option<&'a Rect>,
    pub dirty_rect: Option<&'a Rect>,
    pub selection_edges: Option<&'a SelectionEdges>,
    pub selection_dirty: bool,
    pub magnifier: Option<&'a MagnifierState>,
    pub is_mag_monitor: bool,
    /// Eyedropper only: the loupe also carries the colour plate under it.
    pub mag_label: bool,
    pub toolbar: Option<&'a mut Toolbar>,
    pub settings_panel: Option<&'a mut SettingsPanel>,
    pub current_color: Color,
    pub outline_color: Color,
    pub color_picker: Option<&'a mut ColorPickerPopover>,
    pub model_popover: Option<&'a mut crate::ui::model_popover::ModelPopover>,
    pub icons_cache: &'a HashMap<&'static str, Tree>,
    pub offset: (f32, f32),
    // annotations
    pub annotations_layer: &'a Pixmap,
    pub annotations_layer_empty: bool,
    pub pending: Option<&'a Annotation>,
    pub pending_pen_baked: usize,
    pub is_pending_selected: bool,
    /// index of dragged annotation in `annotations`
    pub dragged_index: Option<usize>,
    pub selected_annotation: Option<usize>,
    pub annotations: &'a [Annotation],
    pub blurs: &'a HashMap<u64, Pixmap>,
    /// Everything needed to lay out and draw text.
    pub text: TextContext<'a>,
    pub active_text_id: Option<u64>,
    // OCR tool: recognized lines + selection overlay
    pub ocr_view: Option<&'a crate::ocr::OcrView>,
    /// OCR tool: region being scanned (global) + spinner animation, while a
    /// recognition is working
    pub ocr_scan: Option<(Rect, f32)>,
    pub monitor_idx: usize,
    pub toasts: &'a crate::ui::toast::Toasts,
    /// logical size of the monitor; canvas / scale is off by a fraction,
    /// and the loupe would flip at another spot than its damage does
    pub size: (f32, f32),
    pub scale: f32,
}

/// the highest scale among monitors the region touches
pub fn capture_scale(
    captures: &[crate::types::Capture],
    placements: &[crate::types::Placement],
    region: Rect,
) -> f32 {
    let max = placements
        .iter()
        .zip(captures)
        .filter(|(p, _)| {
            Rect::from_xywh(
                p.position.0 as f32,
                p.position.1 as f32,
                p.size.0 as f32,
                p.size.1 as f32,
            )
            .is_some_and(|r| crate::utils::rects_overlap(&r, &region))
        })
        .map(|(_, c)| c.scale)
        .fold(0.0_f32, f32::max);
    if max > 0.0 { max } else { 1.0 }
}

pub fn composite(
    captures: &[crate::types::Capture],
    placements: &[crate::types::Placement],
    region: Rect,
    scale: Option<f32>,
) -> Option<(Pixmap, (i32, i32), f32)> {
    let left = region.left().floor() as i32;
    let top = region.top().floor() as i32;
    let width = (region.right().ceil() as i32 - left).max(0) as u32;
    let height = (region.bottom().ceil() as i32 - top).max(0) as u32;
    if width == 0 || height == 0 {
        return None;
    }
    let area = Rect::from_xywh(left as f32, top as f32, width as f32, height as f32)?;

    let scale = scale.unwrap_or_else(|| capture_scale(captures, placements, area));

    let out_w = (width as f32 * scale).round() as u32;
    let out_h = (height as f32 * scale).round() as u32;
    let mut out = Pixmap::new(out_w, out_h)?;

    for (placement, capture) in placements.iter().zip(captures) {
        let src = &capture.pixmap;
        let sx = scale * placement.size.0 as f32 / src.width() as f32;
        let sy = scale * placement.size.1 as f32 / src.height() as f32;
        let tx = (placement.position.0 - left) as f32 * scale;
        let ty = (placement.position.1 - top) as f32 * scale;

        if sx == 1.0 && sy == 1.0 && tx.fract() == 0.0 && ty.fract() == 0.0 {
            out.draw_pixmap(
                tx as i32,
                ty as i32,
                src.as_ref(),
                &tiny_skia::PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        } else {
            out.draw_pixmap(
                0,
                0,
                src.as_ref(),
                &tiny_skia::PixmapPaint {
                    quality: tiny_skia::FilterQuality::Bicubic,
                    ..tiny_skia::PixmapPaint::default()
                },
                Transform::from_row(sx, 0.0, 0.0, sy, tx, ty),
                None,
            );
        }
    }

    Some((out, (left, top), scale))
}

pub fn render_frame(req: &mut RenderRequest) {
    let s = req.scale;
    if req.selection_dirty {
        update_dimming_delta(
            req.dimmed,
            req.prev_selection,
            req.selection,
            req.selection_edges,
            s,
        );
    }
    let dirty_rect = req.dirty_rect;

    if let Some(dirty) = dirty_rect {
        blit_rect(req.dimmed, req.canvas, dirty, s);
    } else {
        req.canvas.data_mut().copy_from_slice(req.dimmed.data());
    }

    if !req.annotations_layer_empty {
        if let Some(dirty) = dirty_rect {
            blit_annotations(req.annotations_layer, req.canvas, dirty, s);
        } else {
            req.canvas.draw_pixmap(
                0,
                0,
                req.annotations_layer.as_ref(),
                &tiny_skia::PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
    }

    // Dynamic annotations: pending (in-progress drawing or dragging)
    if let Some(pending) = req.pending {
        let text = &mut req.text;
        let mut p = Painter::new(req.canvas)
            .with_scale(s)
            .with_text(text.font_system, text.swash_cache)
            .with_origin(req.offset);
        if !req.is_pending_selected
            && let crate::types::AnnotationShape::Pen { points } = &pending.shape
        {
            annotations::draw_pen_active_tail(
                &mut p,
                points,
                req.pending_pen_baked,
                pending.color,
                pending.stroke_width,
            );
        } else if !req.is_pending_selected
            && let crate::types::AnnotationShape::Highlighter { points, flat_ends } = &pending.shape
        {
            annotations::draw_highlighter_live(
                &mut p,
                points,
                *flat_ends,
                pending.color,
                pending.stroke_width,
                dirty_rect,
            );
        } else {
            annotations::draw_annotation(
                &mut p,
                pending,
                false,
                text.editors,
                req.blurs,
                req.active_text_id,
            );
        }

        // blurs above the dragged annotation are opaque, so redrawing them
        // with whatever lies on them puts it back under
        if let Some(at) = req.dragged_index {
            let (w, h) = p.size();
            let area = dirty_rect.copied().or(Rect::from_xywh(0.0, 0.0, w, h));
            for (i, ann) in req.annotations.iter().enumerate().skip(at) {
                if let crate::types::AnnotationShape::Redact { start, end, .. } = &ann.shape
                    && let Some(rect) = paths::normalized_rect(*start, *end)
                    && let Some(local) = Rect::from_xywh(
                        rect.x() - req.offset.0,
                        rect.y() - req.offset.1,
                        rect.width(),
                        rect.height(),
                    )
                    && let Some(region) = area.and_then(|area| local.intersect(&area))
                {
                    rebuild_annotations_layer(
                        &mut p,
                        &req.annotations[i..],
                        text.editors,
                        req.blurs,
                        req.active_text_id,
                        Some(region),
                    );
                }
            }
        }
    }

    if let Some(sel) = req.selection {
        draw_selection_border(
            &mut Painter::new(req.canvas).with_scale(s),
            sel,
            req.selection_edges,
        );
    }

    if let Some(pending) = req.pending
        && req.is_pending_selected
    {
        annotations::draw_annotation_handles_only(
            &mut Painter::new(req.canvas)
                .with_scale(s)
                .with_origin(req.offset),
            pending,
        );
    }

    // Dynamic selection chrome: handles for the currently selected annotation
    if let Some(idx) = req.selected_annotation
        && let Some(ann) = req.annotations.get(idx)
    {
        annotations::draw_annotation_handles_only(
            &mut Painter::new(req.canvas)
                .with_scale(s)
                .with_origin(req.offset),
            ann,
        );
    }

    // Clipped to the dirty rect: everything the overlay paints is translucent,
    // so anything drawn outside the area just restored from `dimmed` would
    // stack a second layer on top of last frame's.
    if let Some(view) = req.ocr_view {
        crate::ocr::draw::draw_ocr_overlay(req.canvas, view, req.offset, dirty_rect, s);
    }

    if let Some((region, phase)) = req.ocr_scan {
        crate::ocr::draw::draw_ocr_scan(
            req.canvas,
            region,
            phase,
            req.offset,
            req.icons_cache,
            dirty_rect,
            s,
        );
    }

    let mut p = Painter::new(req.canvas)
        .with_scale(s)
        .with_icons(req.icons_cache)
        .with_text(req.text.font_system, req.text.swash_cache);

    if req.is_mag_monitor
        && let Some(mag) = req.magnifier
    {
        crate::ui::magnifier::draw_magnifier(
            &mut p,
            req.capture,
            (mag.pos.0 as f32, mag.pos.1 as f32),
            req.size,
            req.mag_label,
        );
    }

    if let Some(tb) = req.toolbar.as_deref_mut()
        && tb.dirty
    {
        crate::ui::toolbar::draw_toolbar(&mut p, tb);
    }

    if let Some(settings) = req.settings_panel.as_deref_mut()
        && settings.dirty
    {
        crate::ui::settings_panel::draw_settings_panel(
            &mut p,
            settings,
            req.current_color,
            req.outline_color,
        );
    }
    if let Some(color_picker) = req.color_picker.as_deref_mut()
        && color_picker.dirty
    {
        crate::ui::color_popover::draw_color_popover(&mut p, color_picker);
    }
    if let Some(model_popover) = req.model_popover.as_deref_mut()
        && model_popover.dirty
    {
        crate::ui::model_popover::draw_model_popover(&mut p, model_popover);
    }

    if !req.toasts.items.is_empty() {
        crate::ui::toast::draw_toasts(&mut p, req.toasts, req.monitor_idx, dirty_rect);
    }
}
// ***************************/
/// SELECTION + DIMMING  ////
// **************************/
//
// The dim layer is `base` layer but darkened everywhere except inside the selection
//
// The bright rectangle's corners are rounded to the *inner* edge of the
// selection border, so the border doesn't have a hard-edged hole.
// rounded corners are only visual, the screenshot result won't have such corners
//
/// Radius of the bright area
fn hole_radius() -> f32 {
    let sel = &crate::config::get().selection;
    sel.border_radius - sel.border_width / 2.0
}

fn dim_pixel() -> [u8; 4] {
    let crate::theme::Rgba(r, g, b, a) = crate::config::get().selection.dim;
    let px = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
    [px.red(), px.green(), px.blue(), px.alpha()]
}

pub fn init_dimming(
    dimmed: &mut Pixmap,
    selection: Option<&Rect>,
    edges: Option<&SelectionEdges>,
    scale: f32,
) {
    let px = dim_pixel();
    for d in dimmed.data_mut().as_chunks_mut::<4>().0 {
        d.copy_from_slice(&px);
    }

    if let Some(sel) = selection {
        fill_rect(dimmed, sel, [0; 4], scale);
        dim_hole_corners(dimmed, sel, edges, scale);
    }
}

fn draw_selection_border(p: &mut Painter, sel: &Rect, edges: Option<&SelectionEdges>) {
    let cfg = &crate::config::get().selection;
    let stroke = Stroke {
        width: cfg.border_width,
        ..Stroke::default()
    };

    if let Some(edges) = edges {
        let half = stroke.width / 2.0;
        let outer = Rect::from_ltrb(
            sel.left() - half,
            sel.top() - half,
            sel.right() + half,
            sel.bottom() + half,
        )
        .unwrap_or(*sel);

        if let Some(path) = rounded_rect_path(
            &outer,
            cfg.border_radius,
            edges.top && edges.left,
            edges.top && edges.right,
            edges.bottom && edges.right,
            edges.bottom && edges.left,
        ) {
            p.stroke_path(&path, cfg.border.get(), &stroke);
        }
    }
}

/// Darken the four corners between the sharp rectangle and the rounded border.
/// Each corner is very small (at most `hole_radius()` square), so this is fast
/// and uses almost no performance.
///
/// A corner is only rounded if both sides are visible screen edges. If the
/// selection goes off the edge of the monitor, that corner stays flat.
fn dim_hole_corners(canvas: &mut Pixmap, sel: &Rect, edges: Option<&SelectionEdges>, scale: f32) {
    let Some(edges) = edges else { return };
    let radius = hole_radius().min(sel.width() / 2.0).min(sel.height() / 2.0);
    if radius <= 0.0 {
        return;
    }

    let dim = crate::config::get().selection.dim;
    let mut p = Painter::new(canvas).with_scale(scale);

    // corner point, then the direction the rectangle's interior lies in
    let corners = [
        (edges.top && edges.left, (sel.left(), sel.top()), (1.0, 1.0)),
        (
            edges.top && edges.right,
            (sel.right(), sel.top()),
            (-1.0, 1.0),
        ),
        (
            edges.bottom && edges.right,
            (sel.right(), sel.bottom()),
            (-1.0, -1.0),
        ),
        (
            edges.bottom && edges.left,
            (sel.left(), sel.bottom()),
            (1.0, -1.0),
        ),
    ];

    for (rounded, (cx, cy), (sx, sy)) in corners {
        if !rounded {
            continue;
        }
        let mut pb = PathBuilder::new();
        pb.move_to(cx, cy);
        pb.line_to(cx + sx * radius, cy);
        pb.cubic_to(
            cx + sx * radius * (1.0 - paths::KAPPA),
            cy,
            cx,
            cy + sy * radius * (1.0 - paths::KAPPA),
            cx,
            cy + sy * radius,
        );
        pb.close();
        if let Some(path) = pb.finish() {
            p.fill_path(&path, dim);
        }
    }
}

fn update_dimming_delta(
    dimmed: &mut Pixmap,
    prev: Option<&Rect>,
    next: Option<&Rect>,
    edges: Option<&SelectionEdges>,
    scale: f32,
) {
    if let Some(old) = prev {
        fill_rect(dimmed, old, dim_pixel(), scale);
    }
    if let Some(cur) = next {
        fill_rect(dimmed, cur, [0; 4], scale);
        dim_hole_corners(dimmed, cur, edges, scale);
    }
}

fn fill_rect(pixmap: &mut Pixmap, rect: &Rect, px: [u8; 4], scale: f32) {
    let (w, h) = (pixmap.width(), pixmap.height());
    let Some((x, y, rw, rh)) = scale_rect(rect, scale).and_then(|r| rect_bounds(&r, w, h)) else {
        return;
    };

    let stride = (w * 4) as usize;
    let row_bytes = rw as usize * 4;
    let data = pixmap.data_mut();
    for row in 0..rh {
        let off = (y + row) as usize * stride + x as usize * 4;
        for d in data[off..off + row_bytes].as_chunks_mut::<4>().0 {
            d.copy_from_slice(&px);
        }
    }
}

fn blit_rect(src: &Pixmap, dst: &mut Pixmap, rect: &Rect, scale: f32) {
    let (w, h) = (dst.width(), dst.height());
    let Some((x, y, rw, rh)) = scale_rect(rect, scale).and_then(|r| rect_bounds(&r, w, h)) else {
        return;
    };

    let row_bytes = (rw * 4) as usize;
    let src_stride = (src.width() * 4) as usize;
    let dst_stride = (dst.width() * 4) as usize;

    let src_data = src.data();
    let dst_data = dst.data_mut();

    for row in 0..rh {
        let sy = (y + row) as usize;
        let sx = x as usize;
        let src_off = sy * src_stride + sx * 4;

        let dy = sy;
        let dx = sx;
        let dst_off = dy * dst_stride + dx * 4;

        dst_data[dst_off..dst_off + row_bytes]
            .copy_from_slice(&src_data[src_off..src_off + row_bytes]);
    }
}

fn blit_annotations(src: &Pixmap, dst: &mut Pixmap, rect: &Rect, scale: f32) {
    let (w, h) = (dst.width(), dst.height());
    let Some((x, y, rw, rh)) = scale_rect(rect, scale).and_then(|r| rect_bounds(&r, w, h)) else {
        return;
    };
    let src_stride = (src.width() * 4) as usize;
    let dst_stride = (dst.width() * 4) as usize;
    let src_data = src.data();
    let dst_data = dst.data_mut();

    for row in 0..rh {
        let sy = (y + row) as usize;
        let sx = x as usize;
        let src_off = sy * src_stride + sx * 4;
        let dst_off = sy * dst_stride + sx * 4;

        for col in 0..rw as usize {
            let s = &src_data[src_off + col * 4..src_off + col * 4 + 4];
            let d = &mut dst_data[dst_off + col * 4..dst_off + col * 4 + 4];
            let sa = s[3] as u32;
            if sa == 0 {
                continue;
            }
            if sa == 255 {
                d.copy_from_slice(s);
                continue;
            }
            let inv = 255 - sa;
            d[0] = (s[0] as u32 + (d[0] as u32 * inv + 127) / 255).min(255) as u8;
            d[1] = (s[1] as u32 + (d[1] as u32 * inv + 127) / 255).min(255) as u8;
            d[2] = (s[2] as u32 + (d[2] as u32 * inv + 127) / 255).min(255) as u8;
            d[3] = (sa + (d[3] as u32 * inv + 127) / 255).min(255) as u8;
        }
    }
}

// since pixels are opaque, there is no need in blending
// and copying rows is much faster than draw_pixmap
pub fn paste(canvas: &mut Pixmap, pixmap: &Pixmap, x: i32, y: i32) {
    let (cw, ch) = (canvas.width() as i32, canvas.height() as i32);
    let (x0, x1) = (x.max(0), (x + pixmap.width() as i32).min(cw));
    if x1 <= x0 {
        return;
    }
    let len = (x1 - x0) as usize * 4;
    let src_len = pixmap.width() as usize * 4;
    let skip = (x0 - x) as usize * 4;
    for row in y.max(0)..(y + pixmap.height() as i32).min(ch) {
        let src = &pixmap.data()[(row - y) as usize * src_len + skip..][..len];
        let at = (row * cw + x0) as usize * 4;
        canvas.data_mut()[at..at + len].copy_from_slice(src);
    }
}

/// Clears a rectangle to transparent in the persistent layer without anti-aliasing
/// by writing zeroes directly to the slice.
fn clear_rect_transparent(layer: &mut Pixmap, rect: &Rect) {
    let (w, h) = (layer.width(), layer.height());
    let Some((x, y, rw, rh)) = rect_bounds(rect, w, h) else {
        return;
    };
    let stride = (layer.width() * 4) as usize;
    let row_bytes = rw as usize * 4;
    let data = layer.data_mut();

    for row in 0..rh {
        let off = (y + row) as usize * stride + x as usize * 4;
        data[off..off + row_bytes].fill(0);
    }
}

/// Rebuilds the annotations layer.
///
/// If `dirty_rect` is provided (in local layer coordinates), clears and
/// redraws ONLY annotations whose visual bbox intersects with this area.
///
/// `None` indicates a full rebuild (first frame, resize, safety fallback).
pub fn rebuild_annotations_layer(
    p: &mut Painter,
    annotations: &[Annotation],
    text_editors: &mut HashMap<u64, Editor<'static>>,
    blurs: &HashMap<u64, Pixmap>,
    active_text_id: Option<u64>,
    dirty_rect: Option<Rect>,
) {
    let offset = p.origin;
    let s = p.scale();
    let (lw, lh) = p.size();

    let Some(base_rect) = dirty_rect.or_else(|| Rect::from_xywh(0.0, 0.0, lw, lh)) else {
        return;
    };
    let Some(px_rect) = scale_rect(&base_rect, s) else {
        return;
    };

    if dirty_rect.is_none() {
        clear_rect_transparent(p.canvas, &px_rect);
        for ann in annotations {
            draw_annotation(p, ann, false, text_editors, blurs, active_text_id);
        }
        return;
    }

    clear_rect_transparent(p.canvas, &px_rect);

    // Determine the integer bounds of the dirty rect within the layer.
    let lw = p.canvas.width();
    let lh = p.canvas.height();
    let x0 = (px_rect.left().floor() as i32).clamp(0, lw as i32) as u32;
    let y0 = (px_rect.top().floor() as i32).clamp(0, lh as i32) as u32;
    let x1 = (px_rect.right().ceil() as i32).clamp(0, lw as i32) as u32;
    let y1 = (px_rect.bottom().ceil() as i32).clamp(0, lh as i32) as u32;
    let tw = x1.saturating_sub(x0);
    let th = y1.saturating_sub(y0);

    // Allocate a clean temporary pixmap the size of the dirty rect.
    // Annotations are drawn into it with an offset that maps global
    // coordinates into temp-pixmap-local space, keeping shadow blending
    // identical to a full-rebuild (always transparent background).
    let Some(mut tmp) = Pixmap::new(tw, th) else {
        return;
    };
    let tmp_offset = (offset.0 + x0 as f32 / s, offset.1 + y0 as f32 / s);
    let mut tmp_painter = p.sub(&mut tmp).with_origin(tmp_offset);

    for ann in annotations {
        let pad = ann.visual_pad();
        let l = ann.bbox.left() - offset.0 - pad;
        let t = ann.bbox.top() - offset.1 - pad;
        let r = ann.bbox.right() - offset.0 + pad;
        let b = ann.bbox.bottom() - offset.1 + pad;

        if l < base_rect.right()
            && r > base_rect.left()
            && t < base_rect.bottom()
            && b > base_rect.top()
        {
            draw_annotation(
                &mut tmp_painter,
                ann,
                false,
                text_editors,
                blurs,
                active_text_id,
            );
        }
    }

    // Composite the temp pixmap back into the real layer at the correct position.
    paste(p.canvas, &tmp, x0 as i32, y0 as i32);
}
