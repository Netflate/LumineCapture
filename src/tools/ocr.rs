use crate::editor::{DamageZone, EditorState};
use crate::ocr::{self, StartOutcome};
use crate::tools::ToolBehavior;
use crate::types::click::ClickTarget;
use crate::types::{MouseButton, SpecialKey};
use tiny_skia::Rect;

pub struct OcrTool;

impl ToolBehavior for OcrTool {
    // Picking the tool recognizes straight away: the active selection if
    // there is one, otherwise the monitor under the cursor. The work runs on a
    // background thread; `app` polls for the result and calls `finish_ocr`.
    fn on_activate(&self, state: &mut EditorState, _dirty_mask: &mut u32) {
        damage_all(state);
        state.ocr_view.clear();
        start_ocr(state);
    }

    fn on_button(
        &self,
        state: &mut EditorState,
        button: MouseButton,
        pressed: bool,
        _dirty_mask: &mut u32,
    ) {
        if !matches!(button, MouseButton::Left) {
            return;
        }
        state.mouse_down_left = pressed;
        if !pressed || !state.ocr_view.is_active() {
            return;
        }

        damage_all(state); // selection about to change wholesale
        match state.ocr_view.line_at(state.pointer.global) {
            Some(line) => {
                let pos = (state.pointer.global.0 as f32, state.pointer.global.1 as f32);
                if state.click_tracker.register(ClickTarget::OcrLine(line), pos) {
                    state.ocr_view.select_block(line);
                } else {
                    state.ocr_view.begin_drag(state.pointer.global);
                }
            }
            None => {
                state.ocr_view.deselect();
            }
        }
    }

    fn on_move(&self, state: &mut EditorState, global: (f64, f64), _dirty_mask: &mut u32) {
        if !state.ocr_view.is_active() {
            return;
        }

        let changed = if state.mouse_down_left {
            state.ocr_view.extend_drag(global)
        } else {
            let line = state.ocr_view.line_at(global);
            state.ocr_view.set_hovered(line)
        };
        if changed {
            damage_all(state);
        }
    }

    fn on_key(&self, state: &mut EditorState, key: SpecialKey, _dirty_mask: &mut u32) {
        if !state.ocr_view.is_active() || !state.mod_ctrl {
            return;
        }
        match key {
            SpecialKey::KeyA => {
                state.ocr_view.select_all();
                damage_all(state);
            }
            SpecialKey::KeyC | SpecialKey::KeyX => {
                let text = state.ocr_view.text_to_copy();
                if !text.is_empty() {
                    crate::utils::copy_to_clipboard(&text);
                    eprintln!("ocr: copied {} line(s)", text.lines().count());
                }
            }
            _ => {}
        }
    }

    fn on_deactivate(&self, state: &mut EditorState, _dirty_mask: &mut u32) {
        damage_all(state);
        state.ocr_view.clear();
    }
}

/// Damage the whole recognized area rather than single lines: `render_frame`
/// re-blits the region from the dimmed layer before redrawing the overlay, so
/// the translucent highlights can't stack up.
fn damage_all(state: &mut EditorState) {
    if let Some(bounds) = state.ocr_view.bounds() {
        state.damage_rects.push(DamageZone::Global(bounds));
    }
}

fn start_ocr(state: &mut EditorState) {
    let region = match state.selection.zone {
        Some(zone) => zone,
        None => {
            let placement = &state.placements[state.pointer.monitor_idx];
            match Rect::from_xywh(
                placement.position.0 as f32,
                placement.position.1 as f32,
                placement.size.0 as f32,
                placement.size.1 as f32,
            ) {
                Some(rect) => rect,
                None => return,
            }
        }
    };

    let Some(capture) = ocr::composite_region(&state.base, &state.placements, region) else {
        return;
    };

    match state.ocr.start(capture) {
        StartOutcome::Started => {}
        StartOutcome::Busy => eprintln!("ocr: still working on the previous region"),
        StartOutcome::Unavailable(e) => eprintln!("ocr: engine unavailable: {e}"),
    }
}

/// Called from the event loop when a recognition finishes: load the lines
/// into the view and dump them to a text file in reading order.
pub fn finish_ocr(
    state: &mut EditorState,
    result: Result<ocr::OcrText, String>,
    dirty_mask: &mut u32,
) {
    let text = match result {
        Ok(text) => text,
        Err(e) => {
            eprintln!("ocr: recognition failed: {e}");
            return;
        }
    };

    if text.is_empty() {
        eprintln!("ocr: no text found in the selected region");
        return;
    }

    state.ocr_view.set_lines(text.lines);

    match ocr::write_text_file(&state.ocr_view.text_to_copy()) {
        Ok(path) => eprintln!(
            "ocr: {} line(s), also written to {}",
            state.ocr_view.lines.len(),
            path.display()
        ),
        Err(e) => eprintln!("ocr: failed to write output file: {e}"),
    }

    // Marking the monitors dirty isn't enough on its own; without a damage
    // rect the overlay only appears once something else damages the area.
    if let Some(zone) = state.selection.zone {
        state.damage_rects.push(DamageZone::Global(zone));
    }
    if let Some(bounds) = state.ocr_view.bounds() {
        state.damage_rects.push(DamageZone::Global(bounds));
    }
    for i in 0..state.placements.len() {
        crate::editor::dirty::mark_dirty(dirty_mask, i);
    }
}
