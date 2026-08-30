mod annotations;
mod color_popover;
mod magnifier;
mod paths;
mod settings_panel;
mod text;
mod toolbar;

pub use annotations::{
    draw_annotation, draw_annotation_handles_only, draw_pen_tail, selection_chrome_pad, visual_pad,
};
pub use magnifier::magnifier_rect;
pub use paths::{rect_bounds, rounded_rect_path};
pub use settings_panel::char_index_for_x;

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
}

pub fn render_frame(req: &mut RenderRequest) {
    if req.selection_dirty {
        update_dimming_delta(req.dimmed, req.base, req.prev_selection, req.selection);
    }

    if let Some(dirty) = req.dirty_rect {
        blit_rect(req.dimmed, req.canvas, dirty);
    } else {
        req.canvas.data_mut().copy_from_slice(req.dimmed.data());
    }

    if let Some(sel) = req.selection {
        draw_selection_border(req.canvas, sel, req.selection_edges);
    }

    if !req.annotations_layer_empty {
        if let Some(dirty) = req.dirty_rect {
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
}
// ***************************/
/// SELECTION + DIMMING  ////
// **************************/
pub fn init_dimming(dimmed: &mut Pixmap, base: &Pixmap, selection: &Option<Rect>) {
    match selection {
        None => {
            let src = base.data();
            let dst = dimmed.data_mut();

            for (s, d) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
                d[0] = ((s[0] as u16 * 115 + 127) / 255) as u8;
                d[1] = ((s[1] as u16 * 115 + 127) / 255) as u8;
                d[2] = ((s[2] as u16 * 115 + 127) / 255) as u8;
                d[3] = s[3];
            }
        }
        Some(sel) => {
            // just in case if somehow something going to be selected with init in the future
            dimmed.data_mut().copy_from_slice(base.data());
            draw_dimming(dimmed, &Some(*sel), base.width(), base.height());
        }
    }
}

fn draw_selection_border(canvas: &mut Pixmap, sel: &Rect, edges: Option<&SelectionEdges>) {
    let mut paint = Paint::default();
    paint.set_color(Color::WHITE);
    paint.anti_alias = true;
    let mut stroke = Stroke::default();
    stroke.width = 2.0;

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
            8.0,
            edges.top,
            edges.right,
            edges.bottom,
            edges.left,
        ) {
            canvas.stroke_path(&path, &paint, &stroke, Transform::identity(), None);
        }
    }
}

fn draw_dimming(canvas: &mut Pixmap, selection: &Option<Rect>, w: u32, h: u32) {
    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(0, 0, 0, 140));

    match selection {
        None => {
            let rect = Rect::from_xywh(0.0, 0.0, w as f32, h as f32).unwrap();
            let path = PathBuilder::from_rect(rect);
            canvas.fill_path(
                &path,
                &paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
        Some(sel) => {
            let rects = [
                Rect::from_xywh(0.0, 0.0, w as f32, sel.top()),
                Rect::from_xywh(0.0, sel.bottom(), w as f32, h as f32 - sel.bottom()),
                Rect::from_xywh(0.0, sel.top(), sel.left(), sel.height()),
                Rect::from_xywh(sel.right(), sel.top(), w as f32 - sel.right(), sel.height()),
            ];
            for rect in rects {
                if let Some(r) = rect
                    && r.width() > 0.0
                    && r.height() > 0.0
                {
                    let path = PathBuilder::from_rect(r);
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
    }
}

fn update_dimming_delta(
    dimmed: &mut Pixmap,
    base: &Pixmap,
    prev: Option<&Rect>,
    next: Option<&Rect>,
) {
    if let Some(old) = prev {
        dim_rect(dimmed, old);
    }
    if let Some(cur) = next {
        blit_rect(base, dimmed, cur);
    }
}

fn dim_rect(canvas: &mut Pixmap, rect: &Rect) {
    let (w, h) = (canvas.width(), canvas.height());
    let Some((x, y, rw, rh)) = rect_bounds(rect, w, h) else {
        return;
    };

    let Some(r) = Rect::from_xywh(x as f32, y as f32, rw as f32, rh as f32) else {
        return;
    };

    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(0, 0, 0, 140));
    let path = PathBuilder::from_rect(r);
    canvas.fill_path(
        &path,
        &paint,
        tiny_skia::FillRule::Winding,
        Transform::identity(),
        None,
    );
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
            d[0] = ((s[0] as u32 * sa + d[0] as u32 * inv) / 255) as u8;
            d[1] = ((s[1] as u32 * sa + d[1] as u32 * inv) / 255) as u8;
            d[2] = ((s[2] as u32 * sa + d[2] as u32 * inv) / 255) as u8;
            d[3] = (sa + (d[3] as u32 * inv / 255)) as u8;
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
    }
}
