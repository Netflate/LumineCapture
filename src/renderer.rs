mod annotations;
mod magnifier;
mod paths;
mod toolbar;

pub use annotations::draw_annotation;
pub use magnifier::magnifier_rect;
pub use paths::{rect_bounds, rounded_rect_path};

use crate::types::annotations::Annotation;
use crate::types::toolbar::{Toolbar, ToolbarButton};
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
    pub icons_cache: &'a HashMap<ToolbarButton, Tree>,
    pub offset: (f32, f32),
    // annotations
    pub annotations_layer: &'a Pixmap,
    pub annotations_layer_empty: bool,
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
                0, 0,
                req.annotations_layer.as_ref(),
                &tiny_skia::PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
    }

    if req.is_mag_monitor
        && let Some(mag) = req.magnifier
    {
        magnifier::draw_magnifier(req.canvas, req.base, (mag.pos.0 as f32, mag.pos.1 as f32));
    }

    if let Some(tb) = req.toolbar.as_mut()
        && tb.dirty
    {
        toolbar::draw_toolbar(req.canvas, tb, req.icons_cache);
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
                d[0] = ((s[0] as u16 * 115) / 255) as u8; 
                d[1] = ((s[1] as u16 * 115) / 255) as u8; 
                d[2] = ((s[2] as u16 * 115) / 255) as u8; 
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

pub fn rebuild_annotations_layer(
    layer: &mut Pixmap,
    annotations: &[Annotation],
    pending: Option<&Annotation>,
    selected: Option<usize>,
    offset: (f32, f32),
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
    text_editors: &mut HashMap<u64, Editor<'static>>,
    active_text_id: Option<u64>,
) {
    layer.fill(tiny_skia::Color::TRANSPARENT);

    for (i, ann) in annotations.iter().enumerate() {
        draw_annotation(
            layer,
            ann,
            offset,
            selected == Some(i),
            font_system,
            swash_cache,
            text_editors,
            active_text_id,
        );
    }
    if let Some(p) = pending {
        draw_annotation(
            layer,
            p,
            offset,
            false,
            font_system,
            swash_cache,
            text_editors,
            active_text_id,
        );
    }
}
