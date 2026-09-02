use crate::editor::dirty::mark_dirty;
use crate::editor::{DamageZone, EditorState};
use crate::ocr::{self, StartOutcome};
use crate::renderer::scan_badge_rect;
use crate::tools::ToolBehavior;
use crate::tools::selection::SelectionTool;
use crate::types::click::ClickTarget;
use crate::types::{MouseButton, SelectionHandle, SpecialKey};
use std::time::Instant;
use tiny_skia::Rect;

pub struct OcrTool;

impl ToolBehavior for OcrTool {
    // Picking the tool recognizes straight away: the active selection if
    // there is one, otherwise the monitor under the cursor. The work runs on a
    // background thread; `app` polls for the result and calls `finish_ocr`.
    fn on_activate(&self, state: &mut EditorState, dirty_mask: &mut u32) {
        if state.ocr.is_busy() {
            return;
        }
        damage_all(state);
        state.ocr_view.clear();
        start_ocr(state, dirty_mask);
    }

    fn on_button(
        &self,
        state: &mut EditorState,
        button: MouseButton,
        pressed: bool,
        dirty_mask: &mut u32,
    ) {
        if !matches!(button, MouseButton::Left) {
            return;
        }
        state.mouse_down_left = pressed;

        if !pressed {
            end_region_drag(state, dirty_mask);
            return;
        }

        match state.ocr_view.line_at(state.pointer.global) {
            Some(line) => {
                let pos = (state.pointer.global.0 as f32, state.pointer.global.1 as f32);
                let damage = if state.click_tracker.register(ClickTarget::OcrLine(line), pos) {
                    state.ocr_view.select_block(line)
                } else {
                    state.ocr_view.begin_drag(state.pointer.global)
                };
                damage_overlay(state, damage);
            }
            None => {
                let damage = state.ocr_view.deselect();
                damage_overlay(state, damage);
                if !pressed_inside_region(state) {
                    begin_region_drag(state);
                }
            }
        }
    }

    fn on_move(&self, state: &mut EditorState, global: (f64, f64), dirty_mask: &mut u32) {
        if state.ocr_redrag {
            SelectionTool.on_move(state, global, dirty_mask);
            if boxed_out(state) && state.ocr_view.is_active() {
                // A real drag, not a stray click: the old result no longer
                // describes what is on screen, so take it down now rather than
                // leaving stale plates floating over the new box.
                damage_all(state);
                state.ocr_view.clear();
            }
            return;
        }

        if !state.mouse_down_left || !state.ocr_view.is_active() {
            return;
        }

        let damage = state.ocr_view.extend_drag(global);
        damage_overlay(state, damage);
    }

    fn on_key(&self, state: &mut EditorState, key: SpecialKey, dirty_mask: &mut u32) {
        if !state.ocr_view.is_active() || !state.mod_ctrl {
            return;
        }
        match key {
            SpecialKey::KeyA => {
                let damage = state.ocr_view.select_all();
                damage_overlay(state, damage);
            }
            SpecialKey::KeyC | SpecialKey::KeyX => copy_selection(state, dirty_mask),
            _ => {}
        }
    }

    fn on_deactivate(&self, state: &mut EditorState, _dirty_mask: &mut u32) {
        damage_all(state);
        state.ocr_view.clear();
        state.ocr_scan_started = None;
        state.ocr_redrag = false;
        state.tool_active = false;
    }
}

/// A drag has to cover at least this much before it counts as boxing out a new
/// region. Below it, it was a shaky click - and honouring that would throw the
/// result away and leave a handful of pixels selected.
const MIN_REGION: f32 = 16.0;

fn boxed_out(state: &EditorState) -> bool {
    state.selection.zone.is_some_and(|zone| {
        Some(zone) != state.ocr_redrag_from
            && zone.width() >= MIN_REGION
            && zone.height() >= MIN_REGION
    })
}

/// Checks if the click was inside the scanned area. Clicking slightly outside
/// the text should not clear the result, so only a click far outside means
/// "select a new area".
fn pressed_inside_region(state: &EditorState) -> bool {
    state.ocr_view.region().is_some_and(|region| {
        let (x, y) = (state.pointer.global.0 as f32, state.pointer.global.1 as f32);
        x >= region.left() && x <= region.right() && y >= region.top() && y <= region.bottom()
    })
}

/// Start dragging to select a new area. Unlike the main selection tool, this
/// never grabs edges or handles.
///
/// In the OCR tool, dragging anywhere on empty space always starts a new selection.
/// This prevents accidentally moving a full-screen box off the monitor.
fn begin_region_drag(state: &mut EditorState) {
    if state.ocr.is_busy() {
        // A scan already owns the region; moving it now would leave the badge
        // and the shade describing an area nobody asked for.
        return;
    }
    state.ocr_redrag = true;
    state.ocr_redrag_from = state.selection.zone;
    state.tool_active = true;
    state.selection.set_drag(SelectionHandle::None, None, None);
    state.drag_start = Some(state.pointer.global);
}

fn end_region_drag(state: &mut EditorState, dirty_mask: &mut u32) {
    if !state.ocr_redrag {
        return;
    }
    state.ocr_redrag = false;
    state.tool_active = false;
    state.drag_start = None;
    state.selection.set_drag(SelectionHandle::None, None, None);

    if boxed_out(state) {
        start_ocr(state, dirty_mask);
        return;
    }

    // Restore the original selection if the drag barely moved, so a simple click does nothing.
    if state.selection.zone != state.ocr_redrag_from {
        for zone in [state.selection.zone, state.ocr_redrag_from].into_iter().flatten() {
            state.damage_rects.push(DamageZone::Global(zone));
        }
        state.selection.zone = state.ocr_redrag_from;
    }
}

/// Redraw the entire overlay: all covered areas, plus the progress badge if
/// a scan is active (since the background behind the badge must be redrawn too).
fn damage_all(state: &mut EditorState) {
    let mut rects: Vec<Rect> = state.ocr_view.bounds().into_iter().collect();
    if let Some(region) = state.ocr_view.region() {
        rects.push(scan_badge_rect(region));
    }
    for rect in rects {
        state.damage_rects.push(DamageZone::Global(rect));
    }
}

/// Redraw just the area an interaction changed. The overlay redraws itself
/// clipped to this, so nothing else on screen is touched.
fn damage_overlay(state: &mut EditorState, rect: Option<Rect>) {
    if let Some(rect) = rect {
        state.damage_rects.push(DamageZone::Global(rect));
    }
}

/// The region to scan. If nothing is selected, it defaults to the monitor under
/// the cursor and selects it. This brightens the chosen screen, making it clear
/// which one was read. Choosing the tool again after making a selection elsewhere
/// will scan that new area instead.
fn scan_region(state: &mut EditorState, dirty_mask: &mut u32) -> Option<Rect> {
    if state.selection.zone.is_none() {
        let placement = &state.placements[state.pointer.monitor_idx];
        state.selection.zone = Rect::from_xywh(
            placement.position.0 as f32,
            placement.position.1 as f32,
            placement.size.0 as f32,
            placement.size.1 as f32,
        );
        for i in 0..state.placements.len() {
            mark_dirty(dirty_mask, i);
        }
    }
    state.selection.zone
}

fn start_ocr(state: &mut EditorState, dirty_mask: &mut u32) {
    let Some(region) = scan_region(state, dirty_mask) else {
        return;
    };

    let Some(capture) = ocr::composite_region(&state.base, &state.placements, region) else {
        return;
    };

    match state.ocr.start(capture) {
        StartOutcome::Started => {
            state.ocr_view.set_region(region);
            state.ocr_scan_started = Some(Instant::now());
            // The shade and the badge both go up on the next frame.
            state.damage_rects.push(DamageZone::Global(region));
        }
        StartOutcome::Busy => eprintln!("ocr: still working on the previous region"),
        StartOutcome::Unavailable(e) => eprintln!("ocr: engine unavailable: {e}"),
    }
}

/// Re-run recognition over the current selection. Driven by the panel's rescan
/// button, which is how the user points it at a different screen: move point there
/// and click rescan, but maybe its not the best approach
pub fn restart_ocr(state: &mut EditorState, dirty_mask: &mut u32) {
    damage_all(state);
    state.ocr_view.clear();
    start_ocr(state, dirty_mask);
}

/// Copy what is selected, or everything when nothing is.
pub fn copy_selection(state: &mut EditorState, _dirty_mask: &mut u32) {
    let text = state.ocr_view.text_to_copy();
    if text.is_empty() {
        return;
    }
    crate::utils::copy_to_clipboard(&text);
    eprintln!("ocr: copied {} line(s)", text.lines().count());
}

/// Select everything, then copy it, so the panel's copy button also shows what
/// it took.
pub fn copy_all(state: &mut EditorState, dirty_mask: &mut u32) {
    let damage = state.ocr_view.select_all();
    damage_overlay(state, damage);
    copy_selection(state, dirty_mask);
}

/// One animation step of the progress badge while recognition runs. Only the
/// badge is damaged 
pub fn tick_scan_badge(state: &mut EditorState, dirty_mask: &mut u32) {
    let Some(region) = state.ocr_view.region() else {
        return;
    };
    let badge = scan_badge_rect(region);
    state.damage_rects.push(DamageZone::Global(badge));
    *dirty_mask |= crate::utils::get_overlapping_monitors(&badge, &state.placements);
}

/// Called from the event loop when a recognition finishes: load the lines into
/// the view so they can be selected and copied.
pub fn finish_ocr(
    state: &mut EditorState,
    result: Result<ocr::OcrText, String>,
    dirty_mask: &mut u32,
) {
    state.ocr_scan_started = None;
    // Whatever happens next, the progress badge has to come off the canvas.
    damage_all(state);

    let text = match result {
        Ok(text) => text,
        Err(e) => {
            eprintln!("ocr: recognition failed: {e}");
            state.ocr_view.clear();
            mark_all_dirty(state, dirty_mask);
            return;
        }
    };

    if text.is_empty() {
        eprintln!("ocr: no text found in the selected region");
        // Nothing to shade or select: drop the overlay rather than leaving the
        // region sitting under a wash with no text in it.
        state.ocr_view.clear();
        mark_all_dirty(state, dirty_mask);
        return;
    }

    state.ocr_view.set_lines(text.lines);
    eprintln!("ocr: {} line(s)", state.ocr_view.lines.len());

    if std::env::var_os("LUMINE_OCR_DUMP").is_some() {
        match ocr::write_text_file(&state.ocr_view.text_to_copy()) {
            Ok(path) => eprintln!("ocr: written to {}", path.display()),
            Err(e) => eprintln!("ocr: failed to write output file: {e}"),
        }
    }

    damage_all(state);
    mark_all_dirty(state, dirty_mask);
}

fn mark_all_dirty(state: &EditorState, dirty_mask: &mut u32) {
    for i in 0..state.placements.len() {
        mark_dirty(dirty_mask, i);
    }
}
