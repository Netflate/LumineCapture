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
use std::collections::HashMap;
use tiny_skia::{IntSize, Pixmap};
use usvg::Tree;


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

            // for small performance gain giving directly the frame to pixmap without any copying
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

        let mut dimmed = Pixmap::new(w, h).expect("Failed to create dimmed");
        renderer::init_dimming(&mut dimmed, None, None);
        canvases.push(dimmed.clone());
        dimmed_layers.push(dimmed);
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
    for (i, canvas) in editor_state.canvas.iter().enumerate() {
        overlay.stage_frame(i, canvas.data(), None)?;
    }
    overlay.flush()?;
    prof.mark("frames staged + flushed");
    Ok(())
}
