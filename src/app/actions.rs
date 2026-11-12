// kind of a hotkey manager
// transfers key actions to it respective function, depends of the config
// handles some special cases
// and overall is context aware, and does some safety checks
use tiny_skia::Rect;

use crate::editor::dirty::{apply_damage_rects, mark_all_dirty, mark_dirty};
use crate::editor::{DamageZone, EditorState};
use crate::keys::{Action, Dir};
use crate::tools::Tool;
use crate::types::{Finish, Mode};
use crate::ui::magnifier::magnifier_rect;
use crate::ui::settings_panel::{SettingsWidget, StepperArrow, ValueField};

use super::input::{refresh_panels, select_tool};
use super::panels::color::close_color_popover;
use super::panels::model::close_model_popover;
use super::panels::settings::{
    apply_stepper_arrow_step, sync_stepper_edit_text, update_settings_panel,
};

pub fn run(editor_state: &mut EditorState, action: Action, dirty_mask: &mut u32) {
    if editor_state.mode == Mode::Ocr && !matches!(action, Action::Cancel | Action::ToggleMagnifier)
    {
        return;
    }
    editor_state.settings_panel.cancel_scroll();
    editor_state.color_popover.cancel_scroll();

    // when dragging doesn't allow to change tool and etc
    let busy = editor_state.tool_active || editor_state.input.mouse_down;
    match action {
        Action::Accept => {
            if !tool_copy(editor_state, dirty_mask) && !editor_state.to.is_empty() {
                editor_state.finish = Some(editor_state.to);
            }
        }
        Action::Finish(Finish::Copy) => {
            if !tool_copy(editor_state, dirty_mask) {
                editor_state.finish = Some(Finish::Copy.into());
            }
        }
        Action::Finish(finish) => editor_state.finish = Some(finish.into()),
        Action::Cancel => {
            if crate::tools::ocr::interrupt_scan(editor_state, dirty_mask) {
                refresh_panels(editor_state, dirty_mask);
            } else {
                editor_state.cancel = true;
            }
        }
        Action::Undo => step_history(editor_state, EditorState::undo, dirty_mask),
        Action::Redo => step_history(editor_state, EditorState::redo, dirty_mask),
        Action::Delete => {
            if editor_state.selected_tool == Tool::Pick {
                crate::tools::pick::delete_selected(editor_state, dirty_mask);
            }
        }
        // the region can't change under a running scroll
        Action::SelectAll | Action::Move { .. } | Action::Resize { .. }
            if editor_state.scroll.is_some() => {}
        Action::SelectAll => select_all(editor_state, dirty_mask),
        Action::ToggleUi | Action::Tool(_) if editor_state.region() => {}
        Action::ToggleUi => toggle_ui(editor_state, dirty_mask),
        Action::ToggleMagnifier => toggle_magnifier(editor_state, dirty_mask),
        Action::ScrollCapture => begin_scroll_capture(editor_state, dirty_mask),
        Action::SizeUp => step_size(editor_state, StepperArrow::Up, dirty_mask),
        Action::SizeDown => step_size(editor_state, StepperArrow::Down, dirty_mask),
        Action::Tool(tool) if !busy => select_tool(editor_state, tool, dirty_mask),
        Action::Move { dir, fast } if !busy => {
            nudge_selection(editor_state, dir, fast, false, dirty_mask)
        }
        Action::Resize { dir, fast } if !busy => {
            nudge_selection(editor_state, dir, fast, true, dirty_mask)
        }
        Action::Tool(_) | Action::Move { .. } | Action::Resize { .. } => {}
        Action::ZoomIn | Action::ZoomOut | Action::ZoomFit | Action::ZoomActual => {
            super::window::zoom_key(editor_state, action, dirty_mask)
        }
    }
    apply_damage_rects(editor_state, dirty_mask);
}

fn step_history(
    editor_state: &mut EditorState,
    step: fn(&mut EditorState, &mut u32),
    dirty_mask: &mut u32,
) {
    let ocr = crate::tools::ocr::release_crop_for_history(editor_state, dirty_mask);
    step(editor_state, dirty_mask);
    if ocr {
        crate::tools::ocr::retake_crop(editor_state, dirty_mask);
    }
    super::refresh_panels_after_history(editor_state, dirty_mask);
}

fn tool_copy(editor_state: &mut EditorState, dirty_mask: &mut u32) -> bool {
    match editor_state.selected_tool {
        Tool::Eyedropper => {
            let color = editor_state.tool_settings.color;
            crate::tools::eyedropper::copy_value(editor_state, ValueField::Hex, color);
            true
        }
        Tool::Ocr if editor_state.ocr.view.is_active() => {
            crate::tools::ocr::copy_selection(editor_state, dirty_mask);
            true
        }
        Tool::Ocr if crate::tools::ocr::awaiting(editor_state) => {
            crate::tools::ocr::scan_all(editor_state, dirty_mask);
            refresh_panels(editor_state, dirty_mask);
            true
        }
        _ => false,
    }
}

fn select_all(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    if editor_state.selected_tool == Tool::Ocr {
        if editor_state.ocr.view.is_active() {
            crate::tools::ocr::select_all_text(editor_state);
        }
        return;
    }
    let before = editor_state.selection.zone;
    editor_state.selection.zone = editor_state.whole_zone(editor_state.input.pointer.monitor_idx);
    editor_state.record_crop(before);
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
    let (Some(old), Some(bounds)) = (editor_state.selection.zone, editor_state.workspace()) else {
        return;
    };

    let selection = &crate::config::get().selection;
    let step = if fast {
        selection.nudge_fast
    } else {
        selection.nudge
    }
    .round()
    .max(1.0);
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
    editor_state.record_crop(Some(old));
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

/// Switch to scrolling capture if a region is selected, or toast otherwise.
pub(super) fn begin_scroll_capture(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    // no screen to scroll in the window
    if editor_state.sheet.is_some() {
        return;
    }
    if let Some(scroll) = editor_state.scroll.as_ref() {
        if scroll.grown() {
            editor_state.to_window = true;
        } else {
            super::scroll::stop(editor_state, dirty_mask);
        }
        return;
    }
    if editor_state.selection.zone.is_none() {
        editor_state.toasts.show(
            crate::ui::toast::ToastKind::ScrollNeedsRegion,
            &mut editor_state.text.font_system,
        );
        mark_dirty(dirty_mask, editor_state.input.pointer.monitor_idx);
        return;
    }
    editor_state.mode = Mode::Scroll;
    editor_state.cancel = false;
    editor_state.finish = None;
    editor_state.tool_active = false;
}

fn toggle_magnifier(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let was_shown = editor_state.magnifier_shown();
    editor_state.magnifier.hidden = !editor_state.magnifier.hidden;
    if editor_state.magnifier_shown() == was_shown {
        return;
    }
    let Some(mag) = editor_state.magnifier.current.as_ref() else {
        return;
    };
    let monitor_idx = mag.monitor_idx;
    let placement = &editor_state.placements[monitor_idx];
    let rect = magnifier_rect(
        (mag.pos.0 as f32, mag.pos.1 as f32),
        placement.size.0 as f32,
        placement.size.1 as f32,
        editor_state.picking(),
    );
    editor_state.damage_local(monitor_idx, rect);
    mark_dirty(dirty_mask, monitor_idx);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::testing::editor;

    #[test]
    fn enter_reads_everything_only_while_awaiting() {
        let mut state = editor(false);
        state.ocr.models.fake_installed();
        state.selected_tool = Tool::Ocr;
        run(&mut state, Action::Accept, &mut 0);
        assert_eq!(state.selection.zone, None);
        state.finish = None;

        state.ocr.await_region = true;
        run(&mut state, Action::Accept, &mut 0);
        assert_eq!(state.selection.zone, state.whole_zone(0));
        assert!(state.finish.is_none());

        state.ocr.runtime.park(crate::ocr::testing::image(4, 4));
        run(&mut state, Action::Cancel, &mut 0);
        assert!(!state.cancel);
        run(&mut state, Action::Cancel, &mut 0);
        assert!(state.cancel);
    }

    #[test]
    fn undo_with_the_ocr_tool_steps_the_real_crop() {
        use crate::tools::{dispatch_activate, dispatch_deactivate};

        let mut state = editor(true);
        state.ocr.models.fake_installed();
        let old = Rect::from_xywh(0.0, 0.0, 200.0, 100.0);
        let new = Rect::from_xywh(10.0, 20.0, 100.0, 50.0);
        state.selection.zone = old;
        state.selection.zone = new;
        state.record_crop(old);
        state.selected_tool = Tool::Ocr;
        dispatch_activate(Tool::Ocr, &mut state, &mut 0);
        state.selection.zone = Rect::from_xywh(30.0, 30.0, 40.0, 40.0);
        assert_eq!(state.crop(), new);

        run(&mut state, Action::Undo, &mut 0);
        assert_eq!(state.crop(), old);
        assert_eq!(state.selection.zone, old);
        assert!(state.ocr.await_region);
        dispatch_deactivate(Tool::Ocr, &mut state, &mut 0);
        assert_eq!(state.selection.zone, old);
        assert!(state.ocr.crop.is_none());

        state.selected_tool = Tool::Ocr;
        dispatch_activate(Tool::Ocr, &mut state, &mut 0);
        run(&mut state, Action::Redo, &mut 0);
        dispatch_deactivate(Tool::Ocr, &mut state, &mut 0);
        assert_eq!(state.selection.zone, new);
    }

    #[test]
    fn scroll_capture_does_nothing_in_the_window() {
        let mut state = editor(true);
        state.selection.zone = state.whole_zone(0);
        run(&mut state, Action::ScrollCapture, &mut 0);
        assert_eq!(state.mode, Mode::Editor);
        assert!(state.scroll.is_none());
        assert!(state.toasts.items.is_empty());
    }
}
