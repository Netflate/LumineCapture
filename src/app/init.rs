// Contains all of EditorState initialization logic, including building base pixmaps
// from captured frames, rendering layers, monitor placements, toolbar icon cache,
// and performing the initial paint before entering the main event loop

use crate::backend::ScreenOverlay;
use crate::editor::EditorState;
use crate::profiler::Profiler;
use crate::renderer;
use crate::ui::toolbar::{ToolbarButton, ToolbarItem};
use crate::types::{Capture, MonitorFrame, Output, Placement};
use crate::ui::icons;
use crate::types::tool_settings::default_color;
use std::collections::HashMap;
use tiny_skia::{IntSize, Pixmap};
use usvg::Tree;

use super::selection_render_info;

pub fn build_captures(frames: Vec<MonitorFrame>) -> Result<Vec<Capture>, String> {
    frames
        .into_iter()
        .enumerate()
        .map(|(monitor_idx, mut f)| {
            let (src_w, src_h) = (f.pw_width, f.pw_height);
            let size = IntSize::from_wh(src_w, src_h).ok_or_else(|| {
                format!("Invalid frame size for monitor {monitor_idx}: {src_w}x{src_h}")
            })?;
            let logical_w = f.info.size.map_or(src_w as i32, |s| s.0).max(1);
            let scale = src_w as f32 / logical_w as f32;

            let row_bytes = (src_w as usize) * 4;
            let src_stride = f.pw_stride as usize;
            let tight_len = row_bytes * src_h as usize;

            if src_stride == row_bytes && f.pixels.len() >= tight_len {
                f.pixels.truncate(tight_len);
                let pixmap = Pixmap::from_vec(f.pixels, size)
                    .ok_or_else(|| format!("Invalid frame for monitor {monitor_idx}"))?;
                return Ok(Capture { pixmap, scale });
            }

            let mut pixmap = Pixmap::new(src_w, src_h).ok_or_else(|| {
                format!("Invalid frame size for monitor {monitor_idx}: {src_w}x{src_h}")
            })?;
            let dst = pixmap.data_mut();

            if src_stride < row_bytes {
                return Err(format!(
                    "Invalid stride for monitor {}: stride={} row_bytes={}",
                    monitor_idx, src_stride, row_bytes
                ));
            }

            // the last row may come without stride padding
            let needed = src_stride * (src_h as usize - 1) + row_bytes;
            let src = f.pixels.get(..needed).ok_or_else(|| {
                format!(
                    "Not enough pixel data for monitor {}: have={} need={}",
                    monitor_idx,
                    f.pixels.len(),
                    needed
                )
            })?;

            for row in 0..(src_h as usize) {
                let src_off = row * src_stride;
                let dst_off = row * row_bytes;
                dst[dst_off..dst_off + row_bytes]
                    .copy_from_slice(&src[src_off..src_off + row_bytes]);
            }

            Ok(Capture { pixmap, scale })
        })
        .collect()
}

pub fn build_layers(placements: &[Placement]) -> (Vec<Pixmap>, Vec<Pixmap>, Vec<Pixmap>) {
    let len = placements.len();
    let mut canvases = Vec::with_capacity(len);
    let mut dimmed_layers = Vec::with_capacity(len);
    let mut annotation_layers = Vec::with_capacity(len);

    for p in placements {
        let w = p.size.0.max(1) as u32;
        let h = p.size.1.max(1) as u32;

        canvases.push(Pixmap::new(w, h).expect("Failed to create canvas"));
        dimmed_layers.push(Pixmap::new(w, h).expect("Failed to create dimmed"));
        annotation_layers.push(Pixmap::new(w, h).expect("Failed to create annotations"));
    }

    (canvases, dimmed_layers, annotation_layers)
}

pub fn build_placements(outputs: &[Output]) -> Vec<Placement> {
    outputs
        .iter()
        .map(|o| Placement {
            position: o.info.logical_position.unwrap_or(o.info.location),
            size: o.info.logical_size.unwrap_or_else(|| {
                o.info
                    .modes
                    .iter()
                    .find(|m| m.current)
                    .map(|m| m.dimensions)
                    .unwrap_or((0, 0))
            }),
        })
        .collect()
}

pub fn load_icons_cache() -> HashMap<&'static str, Tree> {
    let opt = usvg::Options::default();

    let all_svgs = crate::ui::toolbar::ITEMS
        .iter()
        .filter_map(|item| match item {
            ToolbarItem::Button(ToolbarButton::Tool(tool)) => {
                Some(icons::get_svg_for_tool(*tool).0)
            }
            _ => None,
        })
        .chain(icons::EXTRA_ICONS.iter().copied());

    let mut cache = HashMap::new();
    for svg_str in all_svgs {
        cache.entry(svg_str).or_insert_with(|| {
            Tree::from_str(svg_str, &opt).expect("Critical: Failed to parse embedded SVG icon")
        });
    }
    cache
}

pub fn initial_paint(
    editor_state: &mut EditorState,
    overlay: &mut Box<dyn ScreenOverlay>,
    prof: &mut Profiler,
) -> Result<(), Box<dyn std::error::Error>> {
    let n = editor_state.captures.len();

    let EditorState {
        captures,
        canvas,
        dimmed,
        annotations_layer,
        placements,
        icons_cache,
        magnifier,
        selection,
        ..
    } = editor_state;

    let sel_zone = &selection.zone;
    let prev_zone = &selection.prev_zone;
    let icons_cache_ref = &*icons_cache;
    let magnifier_ref = magnifier.current.as_ref();

    let no_toasts = crate::ui::toast::Toasts::default();
    let no_toasts = &no_toasts;

    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(n);

        for (i, (((capture_i, canvas_i), dimmed_i), ann_i)) in captures
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

                renderer::init_dimming(dimmed_i, local_sel.as_ref(), edges.as_ref());

                renderer::render_frame(&mut renderer::RenderRequest {
                    canvas: canvas_i,
                    capture: capture_i,
                    dimmed: dimmed_i,
                    selection: local_sel.as_ref(),
                    prev_selection: prev_local.as_ref(),
                    dirty_rect: None,
                    selection_edges: edges.as_ref(),
                    selection_dirty: false,
                    magnifier: magnifier_ref,
                    is_mag_monitor: false,
                    mag_label: false,
                    toolbar: None,
                    settings_panel: None,
                    color_picker: None,
                    model_popover: None,
                    icons_cache: icons_cache_ref,
                    offset: (0.0, 0.0),
                    annotations_layer: ann_i,
                    annotations_layer_empty: true,
                    pending: None,
                    is_pending_selected: false,
                    selected_annotation: None,
                    annotations: &[],
                    text: None,
                    active_text_id: None,
                    current_color: default_color().color(),
                    ocr_view: None,
                    ocr_scan: None,
                    monitor_idx: i,
                    toasts: no_toasts,
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
