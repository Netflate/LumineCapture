// Scrolling capture inside the editor loop: starting it on the selected region,
// grabbing frames while the page is live, drawing on the long image while paused,
// its control panel, and the final long shot.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use log::info;
use shot_stitch::Axis;
use tiny_skia::{IntRect, Rect};
use wayland_client::Connection;

use crate::backend::ScreenOverlay;
use crate::editor::EditorState;
use crate::editor::dirty::{expand_rect, mark_dirty};
use crate::renderer::Painter;
use crate::scroll::panel::{self, PanelButton};
use crate::scroll::state::inside;
use crate::scroll::{Scroll, ScrollSession, frame};
use crate::tools::Tool;
use crate::tools::selection::global_selection_to_local;
use crate::types::Outputs;
use crate::ui::toolbar::{ToolbarButton, ToolbarItem};
use crate::utils::rects_overlap;

/// pixels of the long image per unit of the wheel
const WHEEL_STEP: f32 = 3.0;

pub async fn start(
    conn: &Connection,
    editor_state: &mut EditorState,
    dirty_mask: &mut u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let zone = editor_state
        .selection
        .zone
        .ok_or("scrolling capture needs a selected region")?;
    let pointer = editor_state.input.pointer.monitor_idx;
    let monitor_idx = std::iter::once(pointer)
        .chain(0..editor_state.placements.len())
        .find(|&i| global_selection_to_local(&zone, &editor_state.placements[i]).is_some())
        .ok_or("the selected region is outside the monitors")?;
    let placement = &editor_state.placements[monitor_idx];
    let local = global_selection_to_local(&zone, placement)
        .ok_or("the selected region is outside the monitor")?;

    let scale = editor_state.captures[monitor_idx].scale;
    let crop = IntRect::from_ltrb(
        (local.left() * scale).round() as i32,
        (local.top() * scale).round() as i32,
        (local.right() * scale).round() as i32,
        (local.bottom() * scale).round() as i32,
    )
    .ok_or("the selected region is too small")?;
    let region = Rect::from_xywh(
        crop.x() as f32 / scale,
        crop.y() as f32 / scale,
        crop.width() as f32 / scale,
        crop.height() as f32 / scale,
    )
    .ok_or("the selected region is too small")?;
    let session = ScrollSession::new(region, monitor_idx)?;

    let capture = crate::backend::initialize_capture(conn);
    capture.prepare().await?;

    let placement = &editor_state.placements[monitor_idx];
    let (x, y) = placement.offset();
    let zone = Rect::from_xywh(
        region.left() + x,
        region.top() + y,
        region.width(),
        region.height(),
    )
    .ok_or("the selected region is too small")?;
    let size = (placement.size.0 as f32, placement.size.1 as f32);
    let canvas_scale = placement.scale;

    crate::tools::dispatch_deactivate(editor_state.selected_tool, editor_state, dirty_mask);
    if editor_state.selected_tool == Tool::Ocr {
        editor_state.selected_tool = Tool::Selection;
    }
    editor_state.tool_active = false;
    editor_state.selection.zone = Some(zone);

    let mut scroll = Scroll::new(session, zone, crop, scale, size, capture);
    scroll.paint_base(
        &mut editor_state.captures[monitor_idx],
        &mut editor_state.dimmed[monitor_idx],
        canvas_scale,
    );
    editor_state.scroll = Some(scroll);
    info!("scrolling capture started on monitor {monitor_idx}");

    highlight(editor_state);
    redraw_monitor(editor_state, monitor_idx, dirty_mask);
    super::input::refresh_panels(editor_state, dirty_mask);
    Ok(())
}

pub fn next_capture(editor_state: &EditorState) -> Option<Duration> {
    let scroll = editor_state.scroll.as_ref().filter(|s| s.live())?;
    Some(
        scroll
            .session
            .next_capture()
            .saturating_duration_since(Instant::now()),
    )
}

pub async fn capture_step(
    editor_state: &mut EditorState,
    overlay: &dyn ScreenOverlay,
    dirty_mask: &mut u32,
) {
    let Some(scroll) = editor_state.scroll.as_mut() else {
        return;
    };
    if !scroll.session.wants_capture(Instant::now()) {
        return;
    }
    let before = (
        scroll.session.stitcher.revision(),
        scroll.session.status.clone(),
    );

    let pushed = match overlay
        .discovered_outputs()
        .get(scroll.monitor_idx)
        .cloned()
    {
        Some(output) => match scroll.capture.capture_frame(&[output]).await {
            Ok(shot) => match shot.frames.first() {
                Some(frame) => {
                    let crop = scroll.crop;
                    let rect = (
                        crop.x() as u32,
                        crop.y() as u32,
                        crop.width(),
                        crop.height(),
                    );
                    frame::crop(frame, rect).map(|image| scroll.session.push(image))
                }
                None => Ok(None),
            },
            Err(e) => Err(e.to_string()),
        },
        None => Err("the monitor used for scrolling is gone".into()),
    };

    match pushed {
        Ok(Some(result)) => scroll.observe(&result),
        Ok(None) => {}
        Err(e) => {
            log::error!("scrolling capture failed: {e}");
            scroll.session.status = "Capture error".into();
            scroll.session.paused = true;
        }
    }

    let paused = !scroll.live();
    let after = (
        scroll.session.stitcher.revision(),
        scroll.session.status.clone(),
    );
    if paused {
        switched(editor_state, dirty_mask);
    } else if before != after {
        refresh_panel(editor_state, dirty_mask);
    }
}

/// Backend side of the mode: input passes through the region while live,
/// and the snapshot subsurface goes once the canvas has the dimmed one.
pub fn sync(
    editor_state: &mut EditorState,
    overlay: &mut dyn ScreenOverlay,
    drawn: impl Fn(usize) -> bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(scroll) = editor_state.scroll.as_mut() else {
        return Ok(());
    };
    let idx = scroll.monitor_idx;
    if scroll.background && drawn(idx) {
        overlay.clear_background(idx)?;
        scroll.background = false;
    }
    let hole = scroll.input_hole();
    if scroll.input != Some(hole) {
        overlay.set_input_hole(idx, hole);
        scroll.input = Some(hole);
    }
    Ok(())
}

pub fn live(editor_state: &EditorState) -> bool {
    editor_state.scroll.as_ref().is_some_and(Scroll::live)
}

/// While live nothing reaches the tools; while drawing only presses inside the region do.
pub fn blocks_tool(editor_state: &EditorState, pressed: bool) -> bool {
    let Some(scroll) = editor_state.scroll.as_ref() else {
        return false;
    };
    let pointer = editor_state.input.pointer;
    let outside = pointer.monitor_idx != scroll.monitor_idx || !scroll.contains(pointer.local);
    scroll.live() || editor_state.selected_tool == Tool::Selection || (pressed && outside)
}

pub fn set_live(editor_state: &mut EditorState, live: bool, dirty_mask: &mut u32) {
    let busy = editor_state.tool_active || editor_state.pending.is_some();
    let Some(scroll) = editor_state.scroll.as_ref() else {
        return;
    };
    if scroll.live() == live || (live && busy) {
        return;
    }
    if live {
        crate::tools::dispatch_deactivate(editor_state.selected_tool, editor_state, dirty_mask);
        super::panels::color::close_color_popover(editor_state, dirty_mask);
    }
    let Some(scroll) = editor_state.scroll.as_mut() else {
        return;
    };
    scroll.session.paused = !live;
    scroll.session.status = if live { "Capturing" } else { "Paused" }.into();
    if live {
        scroll.follow_page();
    }
    switched(editor_state, dirty_mask);
}

fn switched(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let Some(scroll) = editor_state.scroll.as_mut() else {
        return;
    };
    let idx = scroll.monitor_idx;
    scroll.layer_stale = true;
    scroll.paint_region(
        &mut editor_state.captures[idx],
        &mut editor_state.dimmed[idx],
        editor_state.placements[idx].scale,
    );
    // what lies under the blurs is the long image now
    editor_state.blur = Default::default();
    editor_state.selected_annotation = None;
    highlight(editor_state);
    redraw_monitor(editor_state, idx, dirty_mask);
    super::input::refresh_panels(editor_state, dirty_mask);
}

pub fn toggle_live(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let live = live(editor_state);
    set_live(editor_state, !live, dirty_mask);
}

/// wheel over the paused region scrolls the long image, returns whether it did.
pub fn wheel(
    editor_state: &mut EditorState,
    delta_x: f32,
    delta_y: f32,
    dirty_mask: &mut u32,
) -> bool {
    let pointer = editor_state.input.pointer;
    let Some(scroll) = editor_state.scroll.as_mut() else {
        return false;
    };
    if scroll.live() || pointer.monitor_idx != scroll.monitor_idx || !scroll.contains(pointer.local)
    {
        return false;
    }
    let delta = match scroll.axis() {
        Axis::Vertical => -delta_y,
        Axis::Horizontal => delta_x - delta_y,
    };
    if scroll.scroll_by(delta * WHEEL_STEP) {
        moved(editor_state, dirty_mask);
    }
    true
}

fn moved(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let Some(scroll) = editor_state.scroll.as_mut() else {
        return;
    };
    let idx = scroll.monitor_idx;
    let region = scroll.region;
    scroll.layer_stale = true;
    scroll.paint_region(
        &mut editor_state.captures[idx],
        &mut editor_state.dimmed[idx],
        editor_state.placements[idx].scale,
    );
    editor_state.damage_local(idx, region);
    mark_dirty(dirty_mask, idx);

    // pointer stays put while the page moves under it
    let pointer = editor_state.input.pointer;
    if pointer.monitor_idx == idx {
        let (x, y) = editor_state.origin(idx);
        let global = (x as f64 + pointer.local.0, y as f64 + pointer.local.1);
        editor_state.input.pointer.global = global;
        if !blocks_tool(editor_state, false) {
            crate::tools::dispatch_move(
                editor_state.selected_tool,
                editor_state,
                global,
                dirty_mask,
            );
        }
    }
}

fn redraw_monitor(editor_state: &mut EditorState, idx: usize, dirty_mask: &mut u32) {
    let (w, h) = editor_state.placements[idx].size;
    if let Some(rect) = Rect::from_xywh(0.0, 0.0, w as f32, h as f32) {
        editor_state.damage_local(idx, rect);
    }
    mark_dirty(dirty_mask, idx);
}

/// live is shown on the scroll button, drawing on the tool.
fn highlight(editor_state: &mut EditorState) {
    let live = live(editor_state);
    let tool = editor_state.selected_tool;
    editor_state.toolbar.selected = editor_state
        .toolbar
        .items
        .iter()
        .position(|item| match item {
            ToolbarItem::Button(ToolbarButton::Scroll) => live,
            ToolbarItem::Button(ToolbarButton::Tool(t)) => !live && *t == tool,
            _ => false,
        });
    editor_state.toolbar.dirty = true;
}

/// With room for the border, so the base is put back under all of it.
pub fn panel_rect(editor_state: &EditorState, monitor_idx: usize) -> Option<Rect> {
    let scroll = editor_state
        .scroll
        .as_ref()
        .filter(|s| s.monitor_idx == monitor_idx)?;
    expand_rect(&scroll.layout.rect, 2.0)
}

fn refresh_panel(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let Some(old) = editor_state
        .scroll
        .as_ref()
        .and_then(|s| panel_rect(editor_state, s.monitor_idx))
    else {
        return;
    };
    let Some(scroll) = editor_state.scroll.as_mut() else {
        return;
    };
    let idx = scroll.monitor_idx;
    let size = editor_state.placements[idx].size;
    scroll.relayout((size.0 as f32, size.1 as f32));
    editor_state.damage_local(idx, old);
    if let Some(new) = panel_rect(editor_state, idx) {
        editor_state.damage_local(idx, new);
    }
    mark_dirty(dirty_mask, idx);
}

pub fn hover(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let pointer = editor_state.input.pointer;
    let Some(scroll) = editor_state.scroll.as_mut() else {
        return;
    };
    let hovered = (pointer.monitor_idx == scroll.monitor_idx)
        .then(|| panel::hit_test(&scroll.layout, pointer.local))
        .flatten();
    if hovered != scroll.hovered {
        scroll.hovered = hovered;
        refresh_panel(editor_state, dirty_mask);
    }
}

pub fn over_panel(editor_state: &EditorState) -> bool {
    let pointer = editor_state.input.pointer;
    editor_state.scroll.as_ref().is_some_and(|scroll| {
        pointer.monitor_idx == scroll.monitor_idx && inside(&scroll.layout.rect, pointer.local)
    })
}

/// whether the click went to the panel.
pub fn click(editor_state: &mut EditorState, dirty_mask: &mut u32) -> bool {
    if !over_panel(editor_state) {
        return false;
    }
    let Some(scroll) = editor_state.scroll.as_mut() else {
        return false;
    };
    let Some(button) = panel::hit_test(&scroll.layout, editor_state.input.pointer.local) else {
        return true;
    };
    if !panel::is_button_enabled(button, &scroll.session) {
        return true;
    }
    match button {
        PanelButton::TogglePause => toggle_live(editor_state, dirty_mask),
        PanelButton::ToggleAxis => {
            if scroll.session.toggle_axis() {
                scroll.reset();
                moved(editor_state, dirty_mask);
            }
        }
        PanelButton::Finish => editor_state.finish = Some(Outputs::default()),
        PanelButton::Cancel => editor_state.cancel = true,
    }
    refresh_panel(editor_state, dirty_mask);
    true
}

pub fn draw_panel(editor_state: &mut EditorState, monitor_idx: usize, dirty: Option<&Rect>) {
    let Some(scroll) = editor_state
        .scroll
        .as_mut()
        .filter(|s| s.monitor_idx == monitor_idx)
    else {
        return;
    };
    if dirty.is_some_and(|dirty| !rects_overlap(dirty, &scroll.layout.rect)) {
        return;
    }
    panel::draw(
        &mut Painter::new(&mut editor_state.canvas[monitor_idx])
            .with_scale(editor_state.placements[monitor_idx].scale)
            .with_icons(&editor_state.icons_cache)
            .with_text(
                &mut editor_state.text.font_system,
                &mut editor_state.text.swash_cache,
            ),
        &mut scroll.session,
        &scroll.layout,
        scroll.hovered,
    );
}

/// long image with the annotations, its global position and scale.
pub fn render_final(editor_state: &mut EditorState) -> Option<(Vec<u8>, (i32, i32), f32)> {
    let scroll = editor_state.scroll.as_mut()?;
    let scale = scroll.scale;
    let doc = scroll.doc_rect()?;
    let mut out = scroll.long_pixmap()?.clone();
    info!(
        "scrolling capture finished: {}x{}, {} frames",
        out.width(),
        out.height(),
        scroll.session.frame_count
    );
    let origin = (doc.left() * scale, doc.top() * scale);
    let mut scaled_editors = HashMap::new();
    let in_place = HashMap::new();
    for ann in &editor_state.annotations {
        let scaled;
        let (ann, editors) = if scale == 1.0 {
            (ann, &mut editor_state.text.editors)
        } else {
            scaled = ann.scaled(scale);
            (&scaled, &mut scaled_editors)
        };
        crate::renderer::draw_annotation(
            &mut Painter::new(&mut out)
                .with_text(
                    &mut editor_state.text.font_system,
                    &mut editor_state.text.swash_cache,
                )
                .with_origin(origin),
            ann,
            false,
            editors,
            &in_place,
            None,
        );
    }
    Some((
        crate::utils::encode_png(&out),
        (doc.left() as i32, doc.top() as i32),
        scale,
    ))
}
