mod annotations;
mod color_popover;
mod magnifier;
mod ocr;
mod paths;
mod settings_panel;
mod text;
mod toast;
mod toolbar;

pub use annotations::{
    draw_annotation, draw_annotation_handles_only, draw_pen_active_tail, draw_pen_tail,
    selection_chrome_pad, shadow_color_for, stroke_pen_segment, visual_pad,
};
pub use magnifier::magnifier_rect;
pub use ocr::scan_badge_rect;
pub use paths::{rect_bounds, rounded_rect_path};
pub use settings_panel::char_index_for_x;
pub use text::measure_line_width;

use crate::types::annotations::Annotation;
use crate::types::color_popover::ColorPickerPopover;
use crate::types::settings_panel::SettingsPanel;
use crate::types::toolbar::Toolbar;
use crate::types::{MagnifierState, SelectionEdges};
use cosmic_text::{Editor, FontSystem, SwashCache};
use std::collections::HashMap;
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform};
use usvg::Tree;

pub struct RenderRequest<'a> {
    // basic layers
    pub canvas: &'a mut Pixmap,
    pub base: &'a Pixmap,
    pub dimmed: &'a mut Pixmap,
    // selection + magnifier + toolbar
    pub selection: Option<&'a Rect>,
    pub prev_selection: Option<&'a Rect>,
    pub dirty_rect: Option<&'a Rect>,
    pub selection_edges: Option<&'a SelectionEdges>,
    pub selection_dirty: bool,
    pub magnifier: Option<&'a MagnifierState>,
    pub is_mag_monitor: bool,
    pub toolbar: Option<&'a mut Toolbar>,
    pub settings_panel: Option<&'a mut SettingsPanel>,
    pub current_color: Color,
    pub color_picker: Option<&'a mut ColorPickerPopover>,
    pub icons_cache: &'a HashMap<&'static str, Tree>,
    pub offset: (f32, f32),
    // annotations
    pub annotations_layer: &'a Pixmap,
    pub annotations_layer_empty: bool,
    pub pending: Option<&'a Annotation>,
    pub is_pending_selected: bool,
    pub selected_annotation: Option<usize>,
    pub annotations: &'a [Annotation],
    pub font_system: Option<&'a mut FontSystem>,
    pub swash_cache: Option<&'a mut SwashCache>,
    pub text_editors: Option<&'a mut HashMap<u64, Editor<'static>>>,
    pub active_text_id: Option<u64>,
    // OCR tool: recognized lines + selection overlay
    pub ocr_view: Option<&'a crate::ocr::OcrView>,
    /// OCR tool: region being scanned (global) + spinner animation, while a
    /// recognition is working
    pub ocr_scan: Option<(Rect, f32)>,
    pub monitor_idx: usize,
    pub toasts: &'a crate::types::toast::Toasts,
    /// Intro fade. `Some(strength)` rebuilds the whole dim layer from `base` at
    /// that strength (0 = untouched, 1 = fully dimmed) and repaints the whole
    /// monitor; `None` is the normal incremental path.
    pub dim_fade: Option<f32>,
}

pub fn render_frame(req: &mut RenderRequest) {
    // fade repaints the whole dim layer every frame, so it also forces a
    // whole-monitor repaint
    // TODO: should be there an option to disable or enable 
    let dirty_rect = match req.dim_fade {
        Some(strength) => {
            init_dimming(
                req.dimmed,
                req.base,
                req.selection,
                req.selection_edges,
                strength,
            );
            None
        }
        None => {
            if req.selection_dirty {
                update_dimming_delta(
                    req.dimmed,
                    req.base,
                    req.prev_selection,
                    req.selection,
                    req.selection_edges,
                );
            }
            req.dirty_rect
        }
    };

    if let Some(dirty) = dirty_rect {
        blit_rect(req.dimmed, req.canvas, dirty);
    } else {
        req.canvas.data_mut().copy_from_slice(req.dimmed.data());
    }

    if let Some(sel) = req.selection {
        draw_selection_border(req.canvas, sel, req.selection_edges);
    }

    if !req.annotations_layer_empty {
        if let Some(dirty) = dirty_rect {
            blit_annotations(req.annotations_layer, req.canvas, dirty);
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
    if let Some(p) = req.pending {
        if let (Some(font_system), Some(swash_cache), Some(text_editors)) = (
            req.font_system.as_deref_mut(),
            req.swash_cache.as_deref_mut(),
            req.text_editors.as_deref_mut(),
        ) {
            if !req.is_pending_selected
                && let crate::types::AnnotationShape::Pen { points } = &p.shape
            {
                annotations::draw_pen_active_tail(
                    req.canvas,
                    points,
                    p.color,
                    p.stroke_width,
                    req.offset,
                );
            } else {
                annotations::draw_annotation(
                    req.canvas,
                    p,
                    req.offset,
                    false,
                    font_system,
                    swash_cache,
                    text_editors,
                    req.active_text_id,
                );
            }
        }
        if req.is_pending_selected {
            annotations::draw_annotation_handles_only(req.canvas, p, req.offset);
        }
    }

    // Dynamic selection chrome: handles for the currently selected annotation
    if let Some(idx) = req.selected_annotation {
        if let Some(ann) = req.annotations.get(idx) {
            annotations::draw_annotation_handles_only(req.canvas, ann, req.offset);
        }
    }

    // Clipped to the dirty rect: everything the overlay paints is translucent,
    // so anything drawn outside the area just restored from `dimmed` would
    // stack a second layer on top of last frame's.
    if let Some(view) = req.ocr_view {
        ocr::draw_ocr_overlay(req.canvas, view, req.offset, dirty_rect);
    }

    if let Some((region, phase)) = req.ocr_scan {
        ocr::draw_ocr_scan(
            req.canvas,
            region,
            phase,
            req.offset,
            req.icons_cache,
            dirty_rect,
        );
    }

    if req.is_mag_monitor
        && let Some(mag) = req.magnifier
    {
        magnifier::draw_magnifier(req.canvas, req.base, (mag.pos.0 as f32, mag.pos.1 as f32));
    }

    if let Some(tb) = req.toolbar.as_deref_mut()
        && tb.dirty
    {
        toolbar::draw_toolbar(req.canvas, tb, req.icons_cache);
    }

    if let Some(settings) = req.settings_panel.as_deref_mut()
        && settings.dirty
    {
        match (
            req.font_system.as_deref_mut(),
            req.swash_cache.as_deref_mut(),
        ) {
            (Some(font_system), Some(swash_cache)) => {
                settings_panel::draw_settings_panel(
                    req.canvas,
                    settings,
                    req.current_color,
                    req.icons_cache,
                    font_system,
                    swash_cache,
                );
            }
            _ => debug_assert!(
                false,
                "font_system/swash_cache are required for drawing settings panel"
            ),
        }
    }
    if let Some(color_picker) = req.color_picker.as_deref_mut()
        && color_picker.dirty
    {
        match (
            req.font_system.as_deref_mut(),
            req.swash_cache.as_deref_mut(),
        ) {
            (Some(font_system), Some(swash_cache)) => {
                color_popover::draw_color_popover(
                    req.canvas,
                    color_picker,
                    //req.icons_cache,
                    font_system,
                    swash_cache,
                );
            }
            _ => debug_assert!(
                false,
                "font_system/swash_cache are required for drawing color popover"
            ),
        }
    }

    if !req.toasts.items.is_empty()
        && let (Some(font_system), Some(swash_cache)) = (
            req.font_system.as_deref_mut(),
            req.swash_cache.as_deref_mut(),
        )
    {
        toast::draw_toasts(
            req.canvas,
            req.toasts,
            req.monitor_idx,
            dirty_rect,
            font_system,
            swash_cache,
        );
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
/// Black laid over everything outside the selection, at full strength.
const DIM_ALPHA: f32 = 140.0;
/// Corner radius of the selection border, measured on its outer edge.
const SELECTION_RADIUS: f32 = 8.0;
const SELECTION_STROKE: f32 = 2.0;
/// Radius of the bright area
const HOLE_RADIUS: f32 = SELECTION_RADIUS - SELECTION_STROKE / 2.0;

/// `base_channel -> dimmed_channel` at `strength` (0 = untouched, 1 = full dim).
fn dim_lut(strength: f32) -> [u8; 256] {
    let keep = 255.0 - DIM_ALPHA * strength.clamp(0.0, 1.0);
    let mut lut = [0u8; 256];
    for (value, slot) in lut.iter_mut().enumerate() {
        *slot = (value as f32 * keep / 255.0 + 0.5) as u8;
    }
    lut
}

pub fn init_dimming(
    dimmed: &mut Pixmap,
    base: &Pixmap,
    selection: Option<&Rect>,
    edges: Option<&SelectionEdges>,
    strength: f32,
) {
    let lut = dim_lut(strength);
    for (s, d) in base
        .data()
        .chunks_exact(4)
        .zip(dimmed.data_mut().chunks_exact_mut(4))
    {
        d[0] = lut[s[0] as usize];
        d[1] = lut[s[1] as usize];
        d[2] = lut[s[2] as usize];
        d[3] = s[3];
    }

    if let Some(sel) = selection {
        blit_rect(base, dimmed, sel);
        dim_hole_corners(dimmed, sel, edges, strength);
    }
}

fn draw_selection_border(canvas: &mut Pixmap, sel: &Rect, edges: Option<&SelectionEdges>) {
    let mut paint = Paint::default();
    paint.set_color(Color::WHITE);
    paint.anti_alias = true;
    let stroke = Stroke {
        width: SELECTION_STROKE,
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
            SELECTION_RADIUS,
            edges.top && edges.left,
            edges.top && edges.right,
            edges.bottom && edges.right,
            edges.bottom && edges.left,
        ) {
            canvas.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }
}

/// Darken the four corners between the sharp rectangle and the rounded border.
/// Each corner is very small (at most `HOLE_RADIUS` square), so this is fast
/// and uses almost no performance.
///
/// A corner is only rounded if both sides are visible screen edges. If the
/// selection goes off the edge of the monitor, that corner stays flat.
fn dim_hole_corners(
    canvas: &mut Pixmap,
    sel: &Rect,
    edges: Option<&SelectionEdges>,
    strength: f32,
) {
    let Some(edges) = edges else { return };
    let radius = HOLE_RADIUS.min(sel.width() / 2.0).min(sel.height() / 2.0);
    if radius <= 0.0 {
        return;
    }

    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(
        0,
        0,
        0,
        (DIM_ALPHA * strength.clamp(0.0, 1.0)) as u8,
    ));
    paint.anti_alias = true;

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

    const K: f32 = 0.5523;
    for (rounded, (cx, cy), (sx, sy)) in corners {
        if !rounded {
            continue;
        }
        let mut pb = PathBuilder::new();
        pb.move_to(cx, cy);
        pb.line_to(cx + sx * radius, cy);
        pb.cubic_to(
            cx + sx * radius * (1.0 - K),
            cy,
            cx,
            cy + sy * radius * (1.0 - K),
            cx,
            cy + sy * radius,
        );
        pb.close();
        if let Some(path) = pb.finish() {
            canvas.fill_path(
                &path,
                &paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
}

fn update_dimming_delta(
    dimmed: &mut Pixmap,
    base: &Pixmap,
    prev: Option<&Rect>,
    next: Option<&Rect>,
    edges: Option<&SelectionEdges>,
) {
    if let Some(old) = prev {
        dim_rect(dimmed, base, old);
    }
    if let Some(cur) = next {
        blit_rect(base, dimmed, cur);
        dim_hole_corners(dimmed, cur, edges, 1.0);
    }
}

/// Reset `rect` to full dimming using the `base` image.
///
/// Redraw it instead of painting over it because rounded corners leave
/// semi-transparent pixels behind. Painting over them again would make
/// those pixels too dark.
fn dim_rect(dimmed: &mut Pixmap, base: &Pixmap, rect: &Rect) {
    let (w, h) = (dimmed.width(), dimmed.height());
    let Some((x, y, rw, rh)) = rect_bounds(rect, w, h) else {
        return;
    };

    let lut = dim_lut(1.0);
    let stride = (w * 4) as usize;
    let row_bytes = rw as usize * 4;
    let src = base.data();
    let dst = dimmed.data_mut();

    for row in 0..rh {
        let off = (y + row) as usize * stride + x as usize * 4;
        let src_row = &src[off..off + row_bytes];
        let dst_row = &mut dst[off..off + row_bytes];
        for (s, d) in src_row.chunks_exact(4).zip(dst_row.chunks_exact_mut(4)) {
            d[0] = lut[s[0] as usize];
            d[1] = lut[s[1] as usize];
            d[2] = lut[s[2] as usize];
            d[3] = s[3];
        }
    }
}

fn blit_rect(src: &Pixmap, dst: &mut Pixmap, rect: &Rect) {
    let (w, h) = (dst.width(), dst.height());
    let Some((x, y, rw, rh)) = rect_bounds(rect, w, h) else {
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

fn blit_annotations(src: &Pixmap, dst: &mut Pixmap, rect: &Rect) {
    let (w, h) = (dst.width(), dst.height());
    let Some((x, y, rw, rh)) = rect_bounds(rect, w, h) else {
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
    layer: &mut Pixmap,
    annotations: &[Annotation],
    offset: (f32, f32),
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    text_editors: &mut HashMap<u64, Editor<'static>>,
    active_text_id: Option<u64>,
    dirty_rect: Option<Rect>,
) {
    let (lw, lh) = (layer.width() as f32, layer.height() as f32);

    let Some(base_rect) = dirty_rect.or_else(|| Rect::from_xywh(0.0, 0.0, lw, lh)) else {
        return;
    };

    if dirty_rect.is_none() {
        clear_rect_transparent(layer, &base_rect);
        for ann in annotations {
            draw_annotation(
                layer,
                ann,
                offset,
                false,
                font_system,
                swash_cache,
                text_editors,
                active_text_id,
            );
        }
        return;
    }

    clear_rect_transparent(layer, &base_rect);

    // Determine the integer bounds of the dirty rect within the layer.
    let lw = layer.width();
    let lh = layer.height();
    let x0 = (base_rect.left().floor() as i32).clamp(0, lw as i32) as u32;
    let y0 = (base_rect.top().floor() as i32).clamp(0, lh as i32) as u32;
    let x1 = (base_rect.right().ceil() as i32).clamp(0, lw as i32) as u32;
    let y1 = (base_rect.bottom().ceil() as i32).clamp(0, lh as i32) as u32;
    let tw = x1.saturating_sub(x0);
    let th = y1.saturating_sub(y0);

    // Allocate a clean temporary pixmap the size of the dirty rect.
    // Annotations are drawn into it with an offset that maps global
    // coordinates into temp-pixmap-local space, keeping shadow blending
    // identical to a full-rebuild (always transparent background).
    let Some(mut tmp) = Pixmap::new(tw.max(1), th.max(1)) else {
        return;
    };
    let tmp_offset = (offset.0 + x0 as f32, offset.1 + y0 as f32);

    for ann in annotations {
        let pad = annotations::visual_pad(ann.stroke_width);
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
                &mut tmp,
                ann,
                tmp_offset,
                false,
                font_system,
                swash_cache,
                text_editors,
                active_text_id,
            );
        }
    }

    // Composite the temp pixmap back into the real layer at the correct position.
    layer.draw_pixmap(
        x0 as i32,
        y0 as i32,
        tmp.as_ref(),
        &tiny_skia::PixmapPaint::default(),
        tiny_skia::Transform::identity(),
        None,
    );
}
