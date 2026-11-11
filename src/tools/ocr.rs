use crate::editor::dirty::mark_all_dirty;
use crate::editor::{DamageZone, EditorState};
use crate::interaction::ClickTarget;
use crate::ocr::draw::scan_badge_rect;
use crate::ocr::models::MODELS;
use crate::ocr::{self, StartOutcome};
use crate::tools::selection::SelectionTool;
use crate::tools::{Tool, ToolBehavior};
use crate::types::{CursorIcon, MouseButton, SelectionHandle, SpecialKey};
use crate::ui::toast::ToastKind;
use log::{error, info, warn};
use std::time::Instant;
use tiny_skia::Rect;

pub struct OcrTool;

impl ToolBehavior for OcrTool {
    // Picking the tool recognizes straight away when a selection is already
    // there. The work runs on a background thread; `app` polls for the result
    // and calls `finish_ocr`.
    fn on_activate(&self, state: &mut EditorState, _dirty_mask: &mut u32) {
        state.ocr.runtime.prepare();
        if state.ocr.runtime.is_busy() {
            return;
        }

        damage_all(state);

        // If this selection zone is already scanned, shows older result without recanning
        if state.ocr.view.is_active() && state.ocr.view.region() == state.selection.zone {
            return;
        }
        state.ocr.view.clear();

        if state.ocr.models.installed_count() == 0 {
            ask_for_model(state);
            return;
        }

        // if there is no selection, ask for dragging one, using notification toast
        if real_box(state).is_none() {
            await_region(state);
            return;
        }
        start_ocr(state);
    }

    fn on_button(
        &self,
        state: &mut EditorState,
        button: MouseButton,
        pressed: bool,
        dirty_mask: &mut u32,
    ) {
        if matches!(button, MouseButton::Right) {
            if pressed {
                back_to_awaiting(state, dirty_mask);
            }
            return;
        }
        if !matches!(button, MouseButton::Left) {
            return;
        }
        state.input.mouse_down = pressed;

        if !pressed {
            end_region_drag(state);
            return;
        }

        let hit = state.ocr.view.line_at(state.input.pointer.global);
        if let Some(line) = hit {
            let pos = (
                state.input.pointer.global.0 as f32,
                state.input.pointer.global.1 as f32,
            );
            if state.input.clicks.register(ClickTarget::OcrLine(line), pos) {
                let damage = state.ocr.view.select_block(line);
                damage_overlay(state, damage);
                return;
            }
        }

        // Empty space inside the scanned area still starts a text drag
        if hit.is_some() || pressed_inside_region(state) {
            let damage = state.ocr.view.begin_drag(state.input.pointer.global);
            damage_overlay(state, damage);
            return;
        }

        let damage = state.ocr.view.deselect();
        damage_overlay(state, damage);
        begin_region_drag(state);
    }

    fn on_move(&self, state: &mut EditorState, global: (f64, f64), dirty_mask: &mut u32) {
        if state.ocr.redrag {
            SelectionTool.on_move(state, global, dirty_mask);
            if boxed_out(state) && state.ocr.view.is_active() {
                // A real drag, not a stray click: the old result no longer
                // describes what is on screen, so take it down now rather than
                // leaving stale plates floating over the new box.
                damage_all(state);
                state.ocr.view.clear();
            }
            return;
        }

        if !state.input.mouse_down || !state.ocr.view.is_active() {
            return;
        }

        let damage = state.ocr.view.extend_drag(global);
        damage_overlay(state, damage);
    }

    fn on_key(&self, state: &mut EditorState, key: SpecialKey, dirty_mask: &mut u32) {
        if !state.ocr.view.is_active() || !state.input.ctrl {
            return;
        }
        match key {
            SpecialKey::KeyA => select_all_text(state),
            SpecialKey::KeyC | SpecialKey::KeyX => copy_selection(state, dirty_mask),
            _ => {}
        }
    }

    // on deactivate we cancel scan in progress, or cache scanned text if it was done.
    // and in both cases clear the visuals
    fn on_deactivate(&self, state: &mut EditorState, _dirty_mask: &mut u32) {
        let scanning = state.ocr.runtime.is_busy();
        cancel_scan(state);
        if scanning {
            state.ocr.view.clear();
        } else {
            let _ = state.ocr.view.deselect();
        }
        state.ocr.redrag = false;
        state.tool_active = false;
        state.ocr.await_region = false;
        state.model_popover.open = false;
        state.toasts.dismiss(ToastKind::OcrPickRegion);
        state.toasts.dismiss(ToastKind::OcrNoModel);
        dismiss_scan_toasts(state);
    }

    fn cursor(&self, state: &EditorState) -> CursorIcon {
        if state.ocr.view.is_active()
            && state.ocr.view.line_at(state.input.pointer.global).is_some()
        {
            CursorIcon::Text
        } else {
            CursorIcon::Crosshair
        }
    }
}

fn cancel_scan(state: &mut EditorState) {
    state.ocr.runtime.cancel();
    state.ocr.scan_started = None;
    damage_all(state);
}

/// Takes down what the previous scan said, it no longer describes the screen.
fn dismiss_scan_toasts(state: &mut EditorState) {
    for kind in [
        ToastKind::OcrNoText,
        ToastKind::OcrFailed,
        ToastKind::CopyFailed,
    ] {
        state.toasts.dismiss(kind);
    }
}

/// waiting for drag with showing toast
fn await_region(state: &mut EditorState) {
    state.ocr.await_region = true;
    state
        .toasts
        .show(ToastKind::OcrPickRegion, &mut state.text.font_system);
}

fn ask_for_model(state: &mut EditorState) {
    state.model_popover.open = true;
    state
        .toasts
        .show(ToastKind::OcrNoModel, &mut state.text.font_system);
}

fn boxed_out(state: &EditorState) -> bool {
    let min_region = crate::config::get().ocr.min_region;
    state.selection.zone.is_some_and(|zone| {
        Some(zone) != state.ocr.redrag_from
            && zone.width() >= min_region
            && zone.height() >= min_region
    })
}

/// Checks if the click was inside the scanned area. Clicking slightly outside
/// the text should not clear the result, so only a click far outside means
/// "select a new area".
fn pressed_inside_region(state: &EditorState) -> bool {
    state.ocr.view.region().is_some_and(|region| {
        let (x, y) = (
            state.input.pointer.global.0 as f32,
            state.input.pointer.global.1 as f32,
        );
        x >= region.left() && x <= region.right() && y >= region.top() && y <= region.bottom()
    })
}

/// Start dragging to select a new area. Unlike the main selection tool, this
/// never grabs edges or handles.
///
/// In the OCR tool, dragging anywhere on empty space always starts a new selection.
/// This prevents accidentally moving a full-screen box off the monitor.
fn begin_region_drag(state: &mut EditorState) {
    if state.ocr.runtime.is_busy() {
        cancel_scan(state);
        state.ocr.view.clear();
    }

    state.toasts.dismiss(ToastKind::OcrPickRegion);
    dismiss_scan_toasts(state);
    state.ocr.redrag = true;
    state.ocr.redrag_from = state.selection.zone;
    state.tool_active = true;
    state.selection.set_drag(SelectionHandle::None, None, None);
    state.input.drag_start = Some(state.input.pointer.global);
}

fn end_region_drag(state: &mut EditorState) {
    if !state.ocr.redrag {
        return;
    }
    state.ocr.redrag = false;
    state.tool_active = false;
    state.input.drag_start = None;
    state.selection.set_drag(SelectionHandle::None, None, None);

    if boxed_out(state) {
        state.ocr.await_region = false;
        start_ocr(state);
        return;
    }

    restore_zone(state);

    if state.ocr.await_region {
        await_region(state);
    }
}

fn restore_zone(state: &mut EditorState) {
    if state.selection.zone != state.ocr.redrag_from {
        for zone in [state.selection.zone, state.ocr.redrag_from]
            .into_iter()
            .flatten()
        {
            state.damage_rects.push(DamageZone::Global(zone));
        }
        state.selection.zone = state.ocr.redrag_from;
    }
}

/// box the user actually set: in the window, whole picture is just "no crop"
fn real_box(state: &EditorState) -> Option<Rect> {
    let zone = state.selection.zone?;
    let whole = state.whole_zone(state.input.pointer.monitor_idx);
    (state.sheet.is_none() || Some(zone) != whole).then_some(zone)
}

pub fn awaiting(state: &EditorState) -> bool {
    state.selected_tool == Tool::Ocr && state.ocr.await_region && !state.ocr.runtime.is_busy()
}

fn back_to_awaiting(state: &mut EditorState, dirty_mask: &mut u32) {
    if state.ocr.redrag {
        state.ocr.redrag = false;
        state.tool_active = false;
        state.input.mouse_down = false;
        state.input.drag_start = None;
        state.selection.set_drag(SelectionHandle::None, None, None);
        restore_zone(state);
    }
    cancel_scan(state);
    state.ocr.view.clear();
    dismiss_scan_toasts(state);
    if state.ocr.models.installed_count() > 0 {
        await_region(state);
    }
    mark_all_dirty(dirty_mask, state.placements.len());
}

pub fn interrupt_scan(state: &mut EditorState, dirty_mask: &mut u32) -> bool {
    if !state.ocr.runtime.is_busy() {
        return false;
    }
    back_to_awaiting(state, dirty_mask);
    true
}

pub fn scan_all(state: &mut EditorState, dirty_mask: &mut u32) {
    if state.ocr.runtime.is_busy() {
        return;
    }
    if !state.ocr.models.active_installed() {
        ask_for_model(state);
        return;
    }
    let Some(zone) = state.whole_zone(state.input.pointer.monitor_idx) else {
        return;
    };
    damage_all(state);
    if let Some(old) = state.selection.zone {
        state.damage_rects.push(DamageZone::Global(old));
    }
    state.selection.zone = Some(zone);
    state.ocr.await_region = false;
    state.toasts.dismiss(ToastKind::OcrPickRegion);
    state.ocr.view.clear();
    start_ocr(state);
    mark_all_dirty(dirty_mask, state.placements.len());
}

/// Redraw the entire overlay: all covered areas, plus the progress badge if
/// a scan is active (since the background behind the badge must be redrawn too).
fn damage_all(state: &mut EditorState) {
    let mut rects: Vec<Rect> = state.ocr.view.bounds().into_iter().collect();
    if let Some(region) = state.ocr.view.region() {
        rects.push(scan_badge_rect(region));
    }
    for rect in rects {
        state.damage_rects.push(DamageZone::Global(rect));
    }
}

/// Redraw just the area an interaction changed. The overlay redraws itself
/// clipped to this, so nothing else on screen is touched.
pub fn select_all_text(state: &mut EditorState) {
    let damage = state.ocr.view.select_all();
    damage_overlay(state, damage);
}

fn damage_overlay(state: &mut EditorState, rect: Option<Rect>) {
    if let Some(rect) = rect {
        state.damage_rects.push(DamageZone::Global(rect));
    }
}

fn start_ocr(state: &mut EditorState) {
    let Some(region) = state.selection.zone else {
        return;
    };
    dismiss_scan_toasts(state);

    if !state.ocr.models.active_installed() {
        ask_for_model(state);
        return;
    }

    let Some(capture) = ocr::composite_region(&state.captures, state.sources(), region) else {
        return;
    };

    match state.ocr.runtime.start(capture) {
        StartOutcome::Started => {
            state.ocr.view.set_region(region);
            state.ocr.scan_started = Some(Instant::now());
            // The shade and the badge both go up on the next frame.
            state.damage_rects.push(DamageZone::Global(region));
        }
        StartOutcome::Busy => info!("ocr: still working on the previous region"),
        StartOutcome::Unavailable(e) => {
            error!("ocr: engine unavailable: {e}");
            state
                .toasts
                .show(ToastKind::OcrFailed, &mut state.text.font_system);
        }
    }
}

/// Re-run recognition over the current selection. Driven by the panel's rescan
/// button; another area is picked by dragging a new box instead.
pub fn restart_ocr(state: &mut EditorState, _dirty_mask: &mut u32) {
    damage_all(state);
    state.ocr.view.clear();
    start_ocr(state);
}

/// Copy what is selected, or everything when nothing is.
pub fn copy_selection(state: &mut EditorState, _dirty_mask: &mut u32) {
    let text = state.ocr.view.text_to_copy();
    if text.is_empty() {
        return;
    }
    match crate::utils::copy_to_clipboard(&text) {
        Ok(()) => info!("ocr: copied {} line(s)", text.lines().count()),
        Err(e) => {
            warn!("ocr: can't copy: {e}");
            state
                .toasts
                .show(ToastKind::CopyFailed, &mut state.text.font_system);
        }
    }
}

/// Select everything, then copy it, so the panel's copy button also shows what
/// it took.
pub fn copy_all(state: &mut EditorState, dirty_mask: &mut u32) {
    let damage = state.ocr.view.select_all();
    damage_overlay(state, damage);
    copy_selection(state, dirty_mask);
}

/// One animation step of the progress badge while recognition runs. Only the
/// badge is damaged
pub fn tick_scan_badge(state: &mut EditorState, dirty_mask: &mut u32) {
    let Some(region) = state.ocr.view.region() else {
        return;
    };
    let badge = scan_badge_rect(region);
    state.damage_rects.push(DamageZone::Global(badge));
    *dirty_mask |= state.monitors_touching(&badge);
}

/// Called from the event loop when a recognition finishes: load the lines into
/// the view so they can be selected and copied.
pub fn finish_ocr(
    state: &mut EditorState,
    result: Result<ocr::OcrText, String>,
    dirty_mask: &mut u32,
) {
    state.ocr.scan_started = None;
    // Whatever happens next, the progress badge has to come off the canvas.
    damage_all(state);

    let text = match result {
        Ok(text) => text,
        Err(e) => {
            error!("ocr: recognition failed: {e}");
            state
                .toasts
                .show(ToastKind::OcrFailed, &mut state.text.font_system);
            state.ocr.view.clear();
            mark_all_dirty(dirty_mask, state.placements.len());
            return;
        }
    };

    if text.is_empty() {
        info!("ocr: no text found in the selected region");
        state
            .toasts
            .show(ToastKind::OcrNoText, &mut state.text.font_system);
        // Nothing to shade or select: drop the overlay rather than leaving the
        // region sitting under a wash with no text in it.
        state.ocr.view.clear();
        mark_all_dirty(dirty_mask, state.placements.len());
        return;
    }

    state.ocr.view.set_lines(text.lines);
    info!("ocr: {} line(s)", state.ocr.view.lines.len());

    damage_all(state);
    mark_all_dirty(dirty_mask, state.placements.len());
}

/// after downloading starts reading immediately
pub fn use_model(state: &mut EditorState, idx: usize, dirty_mask: &mut u32) {
    if state.ocr.models.active() == Some(idx) {
        return;
    }
    let Some(files) = state.ocr.models.files(idx) else {
        return;
    };
    state.ocr.models.set_active(Some(idx));
    state.ocr.runtime.load(files);

    if state.selected_tool != Tool::Ocr {
        state.ocr.view.clear();
        return;
    }
    state.ocr.runtime.prepare();
    if state.ocr.runtime.is_busy() || state.ocr.view.is_active() {
        cancel_scan(state);
        restart_ocr(state, dirty_mask);
    }
}

pub fn model_ready(state: &mut EditorState, idx: usize, dirty_mask: &mut u32) {
    if state.ocr.models.active() == Some(idx) {
        if let Some(files) = state.ocr.models.files(idx) {
            state.ocr.runtime.load(files);
            if state.selected_tool == Tool::Ocr {
                state.ocr.runtime.prepare();
            }
        }
    } else if !state.ocr.models.active_installed() {
        use_model(state, idx, dirty_mask);
    }

    if state.ocr.models.installed_count() != 1 || state.selected_tool != Tool::Ocr {
        return;
    }
    state.model_popover.open = false;
    state.toasts.dismiss(ToastKind::OcrNoModel);
    if real_box(state).is_some() {
        start_ocr(state);
    } else {
        await_region(state);
    }
}

pub fn model_failed(state: &mut EditorState, idx: usize, err: &str) {
    error!("ocr: failed to download {}: {err}", MODELS[idx].name);
    if state.selected_tool == Tool::Ocr {
        state
            .toasts
            .show(ToastKind::OcrDownloadFailed, &mut state.text.font_system);
    }
}

pub fn cancel_model_download(state: &mut EditorState, idx: usize) {
    state.ocr.models.cancel(idx);
    if state.ocr.models.active() == Some(idx) {
        state.ocr.models.set_active(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::testing::editor;

    fn ready(window: bool) -> EditorState {
        let mut state = editor(window);
        state.ocr.models.fake_installed();
        state.selected_tool = Tool::Ocr;
        state
    }

    #[test]
    fn window_without_a_crop_waits_for_a_box() {
        let mut state = ready(true);
        state.selection.zone = state.workspace();
        assert_eq!(real_box(&state), None);
        OcrTool.on_activate(&mut state, &mut 0);
        assert!(state.ocr.await_region);
    }

    #[test]
    fn window_with_a_crop_scans_it() {
        let mut state = ready(true);
        let crop = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
        state.selection.zone = crop;
        assert_eq!(real_box(&state), crop);
        OcrTool.on_activate(&mut state, &mut 0);
        assert!(!state.ocr.await_region);
    }

    #[test]
    fn overlay_waits_without_a_selection_and_scans_with_one() {
        let mut state = ready(false);
        assert_eq!(real_box(&state), None);
        OcrTool.on_activate(&mut state, &mut 0);
        assert!(state.ocr.await_region);

        let mut state = ready(false);
        let zone = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
        state.selection.zone = zone;
        assert_eq!(real_box(&state), zone);
        OcrTool.on_activate(&mut state, &mut 0);
        assert!(!state.ocr.await_region);
    }

    #[test]
    fn scanning_everything_takes_the_whole_zone() {
        let mut state = ready(true);
        state.selection.zone = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
        state.ocr.await_region = true;
        assert!(awaiting(&state));
        scan_all(&mut state, &mut 0);
        assert_eq!(state.selection.zone, state.workspace());
        assert_eq!(
            state.selection.zone,
            Rect::from_xywh(0.0, 0.0, 600.0, 400.0)
        );
        assert!(!state.ocr.await_region);

        let mut state = ready(false);
        state.ocr.await_region = true;
        scan_all(&mut state, &mut 0);
        assert_eq!(
            state.selection.zone,
            Rect::from_xywh(0.0, 0.0, 300.0, 200.0)
        );
    }

    #[test]
    fn right_click_keeps_the_crop_and_waits() {
        let mut state = ready(true);
        let crop = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
        state.selection.zone = crop;
        OcrTool.on_button(&mut state, MouseButton::Right, true, &mut 0);
        assert_eq!(state.selection.zone, crop);
        assert!(state.ocr.await_region);
        assert!(state.undo_stack.is_empty());
    }

    #[test]
    fn right_click_during_a_drag_cancels_it() {
        let mut state = ready(false);
        let from = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
        state.selection.zone = from;
        state.ocr.redrag = true;
        state.ocr.redrag_from = from;
        state.input.mouse_down = true;
        state.selection.zone = Rect::from_xywh(0.0, 0.0, 30.0, 30.0);
        OcrTool.on_button(&mut state, MouseButton::Right, true, &mut 0);
        assert!(!state.ocr.redrag);
        assert!(!state.input.mouse_down);
        assert_eq!(state.selection.zone, from);
        assert!(state.ocr.await_region);
    }

    #[test]
    fn esc_stops_a_running_scan_and_only_then() {
        let mut state = ready(true);
        assert!(!interrupt_scan(&mut state, &mut 0));
        state.ocr.runtime.park(ocr::testing::image(4, 4));
        assert!(state.ocr.runtime.is_busy());
        assert!(interrupt_scan(&mut state, &mut 0));
        assert!(!state.ocr.runtime.is_busy());
        assert!(state.ocr.await_region);
        assert!(!interrupt_scan(&mut state, &mut 0));
    }
}
