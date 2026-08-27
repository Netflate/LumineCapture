// Contains all of EditorState initialization logic, including building base pixmaps 
// from captured frames, rendering layers, monitor placements, toolbar icon cache, 
// and performing the initial paint before entering the main event loop

use crate::backend::ScreenOverlay;
use crate::editor::EditorState;
use crate::profiler::Profiler;
use crate::renderer;
use crate::types::toolbar::{ToolbarButton, ToolbarItem};
use crate::types::{icons, MonitorFrame, Output, Placement};

use std::collections::HashMap;
use tiny_skia::{Pixmap, PixmapPaint, Transform};
use usvg::Tree;

use super::selection_render_info;

pub fn build_base_pixmap(frames: &Vec<MonitorFrame>) -> Vec<Pixmap> {
    frames
        .iter()
        .enumerate()
        .map(|(monitor_idx, f)| {
            let (src_w, src_h) = (f.pw_width, f.pw_height);
            let mut src_pixmap =
                Pixmap::new(src_w, src_h).expect("Failed to create source Pixmap for monitor");

            let row_bytes = (src_w as usize) * 4;
            let src_stride = f.pw_stride as usize;
            let dst = src_pixmap.data_mut();

            if src_stride < row_bytes {
                panic!(
                    "Invalid stride for monitor {}: stride={} row_bytes={}",
                    monitor_idx, src_stride, row_bytes
                );
            }

            let needed = src_stride * (src_h as usize);
            let src = f.pixels.get(..needed).unwrap_or_else(|| {
                panic!(
                    "Not enough pixel data for monitor {}: have={} need={}",
                    monitor_idx,
                    f.pixels.len(),
                    needed
                )
            });

            for row in 0..(src_h as usize) {
                let src_off = row * src_stride;
                let dst_off = row * row_bytes;
                dst[dst_off..dst_off + row_bytes]
                    .copy_from_slice(&src[src_off..src_off + row_bytes]);
            }

            let (logical_w_i32, logical_h_i32) =
                f.info.size.unwrap_or((src_w as i32, src_h as i32));
            let logical_w = logical_w_i32.max(1) as u32;
            let logical_h = logical_h_i32.max(1) as u32;

            if logical_w == src_w && logical_h == src_h {
                return src_pixmap;
            }

            let mut logical_pixmap = Pixmap::new(logical_w, logical_h)
                .expect("Failed to create logical Pixmap for monitor");
            let sx = logical_w as f32 / src_w as f32;
            let sy = logical_h as f32 / src_h as f32;
            logical_pixmap.draw_pixmap(
                0,
                0,
                src_pixmap.as_ref(),
                &PixmapPaint::default(),
                Transform::from_row(sx, 0.0, 0.0, sy, 0.0, 0.0),
                None,
            );
            logical_pixmap
        })
        .collect()
}

pub fn build_layers(base_pixmaps: &[Pixmap]) -> (Vec<Pixmap>, Vec<Pixmap>, Vec<Pixmap>) {
    let len = base_pixmaps.len();
    let mut canvases = Vec::with_capacity(len);
    let mut dimmed_layers = Vec::with_capacity(len);
    let mut annotation_layers = Vec::with_capacity(len);

    for p in base_pixmaps {
        let w = p.width();
        let h = p.height();

        canvases.push(Pixmap::new(w, h).expect("Failed to create canvas"));
        dimmed_layers.push(Pixmap::new(w, h).expect("Failed to create dimmed"));
        annotation_layers.push(Pixmap::new(w, h).expect("Failed to create annotations"));
    }

    (canvases, dimmed_layers, annotation_layers)
}

pub fn build_placements(outputs: &[Output]) -> Vec<Placement> {
    outputs.iter().map(|o| Placement {
        position: o.info.logical_position.unwrap_or(o.info.location),
        size: o.info.logical_size.unwrap_or_else(|| {
            o.info.modes.iter().find(|m| m.current)
                .map(|m| m.dimensions)
                .unwrap_or((0, 0))
        }),
    }).collect()
}

pub fn load_icons_cache() -> HashMap<ToolbarButton, Tree> {
    let mut cache = HashMap::new();
    let opt = usvg::Options::default();

    for item in crate::types::toolbar::TOOLBAR_ITEMS {
        let ToolbarItem::Button(button) = item else {
            continue;
        };
        let (svg_str, _) = icons::get_svg(button);
        let tree =
            Tree::from_str(svg_str, &opt).expect("Critical: Failed to parse embedded SVG icon");
        cache.insert(*button, tree);
    }
    cache
}

pub fn initial_paint(
    editor_state: &mut EditorState,
    overlay: &mut Box<dyn ScreenOverlay>,
    prof: &mut Profiler,
) -> Result<(), Box<dyn std::error::Error>> {
    let n = editor_state.base.len();

    let EditorState {
        base,
        canvas,
        dimmed,
        annotations_layer,
        placements,
        icon_cache,
        magnifier,
        selection,
        ..
    } = editor_state;

    let sel_zone = &selection.zone;
    let prev_zone = &selection.prev_zone;
    let icon_cache_ref = &*icon_cache;
    let magnifier_ref = &*magnifier;

    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(n);

        for (i, (((base_i, canvas_i), dimmed_i), ann_i)) in base
            .iter()
            .zip(canvas.iter_mut())
            .zip(dimmed.iter_mut())
            .zip(annotations_layer.iter_mut())
            .enumerate()
        {
            let placement = &placements[i];

            handles.push(scope.spawn(move || {
                let (local_sel, prev_local, edges) =
                    selection_render_info(sel_zone, prev_zone, placement);

                renderer::init_dimming(dimmed_i, base_i, &local_sel);

                renderer::render_frame(&mut renderer::RenderRequest {
                    canvas: canvas_i,
                    base: base_i,
                    dimmed: dimmed_i,
                    selection: local_sel.as_ref(),
                    prev_selection: prev_local.as_ref(),
                    dirty_rect: None,
                    selection_edges: edges.as_ref(),
                    selection_dirty: false,
                    magnifier: magnifier_ref.as_ref(),
                    is_mag_monitor: false,
                    toolbar: None,
                    settings_panel: None,
                    icons_cache: icon_cache_ref,
                    offset: (0.0, 0.0),
                    annotations_layer: ann_i,
                    annotations_layer_empty: true,
                    font_system: None,
                    swash_cache: None,
                });
            }));
        }

        for h in handles {
            h.join().expect("initial_paint render thread panicked");
        }
    });

    prof.mark(&format!("dimming+render for {n} monitors (parallel)"));

    for i in 0..n {
        overlay.stage_frame(i, editor_state.canvas[i].data(), None)?;
    }
    overlay.flush()?;
    prof.mark("frames staged + flushed");
    Ok(())
}