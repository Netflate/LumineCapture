use tiny_skia::Rect;

use crate::editor::dirty::{apply_damage_rects, mark_all_dirty};
use crate::editor::{DamageZone, EditorState};
use crate::keys::{Action, Dir};
use crate::tools::Tool;
use crate::types::Finish;
use crate::ui::settings_panel::{SettingsWidget, StepperArrow, ValueField};
use crate::utils::get_full_workspace_rect;

use super::input::{refresh_panels, select_tool};
use super::panels::color::close_color_popover;
use super::panels::model::close_model_popover;
use super::panels::settings::{apply_stepper_arrow_step, sync_stepper_edit_text, update_settings_panel};

const NUDGE: f32 = 1.0;
const NUDGE_FAST: f32 = 10.0;

pub fn run(editor_state: &mut EditorState, action: Action, dirty_mask: &mut u32) {
    editor_state.settings_panel.cancel_scroll();
    editor_state.color_popover.cancel_scroll();

    let busy = editor_state.tool_active || editor_state.input.mouse_down;
    match action {
        Action::Finish(Finish::Copy) => copy(editor_state, dirty_mask),
        Action::Finish(finish) => editor_state.finish = Some(finish),
        Action::Cancel => editor_state.cancel = true,
        Action::Undo => {
            editor_state.undo(dirty_mask);
            super::refresh_panels_after_history(editor_state, dirty_mask);
        }
        Action::Redo => {
            editor_state.redo(dirty_mask);
            super::refresh_panels_after_history(editor_state, dirty_mask);
        }
        Action::Delete => {
            if editor_state.selected_tool == Tool::Pick {
                crate::tools::pick::delete_selected(editor_state, dirty_mask);
            }
        }
        Action::SelectAll => select_all(editor_state, dirty_mask),
        Action::ToggleUi => toggle_ui(editor_state, dirty_mask),
        Action::SizeUp => step_size(editor_state, StepperArrow::Up, dirty_mask),
        Action::SizeDown => step_size(editor_state, StepperArrow::Down, dirty_mask),
        Action::Tool(tool) if !busy => select_tool(editor_state, tool, dirty_mask),
        Action::Move { dir, fast } if !busy => nudge_selection(editor_state, dir, fast, false, dirty_mask),
        Action::Resize { dir, fast } if !busy => nudge_selection(editor_state, dir, fast, true, dirty_mask),
        Action::Tool(_) | Action::Move { .. } | Action::Resize { .. } => {}
    }
    apply_damage_rects(editor_state, dirty_mask);
}

fn copy(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    match editor_state.selected_tool {
        Tool::Eyedropper => {
            let color = editor_state.tool_settings.color;
            crate::tools::eyedropper::copy_value(editor_state, ValueField::Hex, color);
        }
        Tool::Ocr if editor_state.ocr.view.is_active() => {
            crate::tools::ocr::copy_selection(editor_state, dirty_mask);
        }
        _ => editor_state.finish = Some(Finish::Copy),
    }
}

fn select_all(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    if editor_state.selected_tool == Tool::Ocr {
        if editor_state.ocr.view.is_active() {
            crate::tools::ocr::select_all_text(editor_state);
        }
        return;
    }
    let placement = &editor_state.placements[editor_state.input.pointer.monitor_idx];
    let (x, y) = placement.offset();
    editor_state.selection.zone =
        Rect::from_xywh(x, y, placement.size.0 as f32, placement.size.1 as f32);
    mark_all_dirty(dirty_mask, editor_state.placements.len());
    refresh_panels(editor_state, dirty_mask);
}

fn nudge_selection(
    editor_state: &mut EditorState,
    dir: Dir,
    fast: bool,
    resize: bool,
    dirty_mask: &mut u32,
) {
    if editor_state.selected_tool == Tool::Ocr {
        return;
    }
    let (Some(old), Some(bounds)) = (
        editor_state.selection.zone,
        get_full_workspace_rect(&editor_state.placements),
    ) else {
        return;
    };

    let step = if fast { NUDGE_FAST } else { NUDGE };
    let (dx, dy) = match dir {
        Dir::Left => (-step, 0.0),
        Dir::Right => (step, 0.0),
        Dir::Up => (0.0, -step),
        Dir::Down => (0.0, step),
    };

    let new = if resize {
        let right = (old.right() + dx).clamp(old.left() + 1.0, bounds.right());
        let bottom = (old.bottom() + dy).clamp(old.top() + 1.0, bounds.bottom());
        Rect::from_ltrb(old.left(), old.top(), right, bottom)
    } else {
        let x = (old.left() + dx).clamp(bounds.left(), bounds.right() - old.width());
        let y = (old.top() + dy).clamp(bounds.top(), bounds.bottom() - old.height());
        Rect::from_xywh(x, y, old.width(), old.height())
    };
    let Some(new) = new.filter(|new| *new != old) else {
        return;
    };

    editor_state.selection.zone = Some(new);
    editor_state.damage_rects.push(DamageZone::Global(old));
    editor_state.damage_rects.push(DamageZone::Global(new));
    refresh_panels(editor_state, dirty_mask);
}

fn toggle_ui(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    editor_state.ui_hidden = !editor_state.ui_hidden;
    if editor_state.ui_hidden {
        editor_state.settings_panel.selected = None;
        close_color_popover(editor_state, dirty_mask);
        close_model_popover(editor_state, dirty_mask);
    }
    editor_state.toolbar.dirty = true;
    refresh_panels(editor_state, dirty_mask);
}

fn step_size(editor_state: &mut EditorState, arrow: StepperArrow, dirty_mask: &mut u32) {
    let Some(widget_idx) = editor_state
        .settings_panel
        .widgets
        .iter()
        .position(|w| matches!(w, SettingsWidget::Stepper { .. }))
    else {
        return;
    };
    apply_stepper_arrow_step(editor_state, widget_idx, arrow, dirty_mask);
    let snapshot = editor_state.settings_panel.pre_edit_snapshot.take();
    editor_state.commit_snapshot(snapshot);
    update_settings_panel(editor_state, dirty_mask);
    sync_stepper_edit_text(editor_state, widget_idx);
}
