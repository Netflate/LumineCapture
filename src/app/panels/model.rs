// popover with list of model languages

use crate::editor::EditorState;
use crate::editor::dirty::apply_damage_rects;
use crate::ocr::models::{MODELS, ModelEvent, ModelStatus};
use crate::ui::model_popover::{ModelPopoverElement, ModelRow};
use crate::ui::panel::{UiPanel, emit_panel_damage, sync_panel_hover, sync_panel_rect};
use crate::ui::settings_panel::{SettingsAction, SettingsWidget};
use crate::ui::toast::ToastKind;

use super::settings::{compute_dropdown_placement, update_settings_panel};

pub fn update_model_popover(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let old_rect = editor_state.model_popover.rect();
    let old_monitor = editor_state.model_popover.monitor_idx;

    if editor_state.model_popover.is_visible() {
        let size = editor_state.model_popover.size;
        let panel = &editor_state.settings_panel;
        let anchor = panel
            .widget_left(|w| {
                matches!(
                    w,
                    SettingsWidget::Action {
                        action: SettingsAction::OcrLanguages,
                        ..
                    }
                )
            })
            .unwrap_or(panel.render_pos.0);
        let (pos, monitor_idx) =
            compute_dropdown_placement(editor_state, size, crate::theme::size::margin(), anchor);
        editor_state.model_popover.position = pos;
        editor_state.model_popover.render_pos = pos;
        editor_state.model_popover.monitor_idx = monitor_idx;
    }

    let models = &editor_state.ocr.models;
    let no_model = models.installed_count() == 0;
    let rows = (0..MODELS.len()).map(|idx| ModelRow {
        status: models.status(idx),
        active: models.active() == Some(idx),
        size: models.download_size(idx),
        recommended: no_model && models.recommended() == idx,
    });
    let rows_changed = editor_state.model_popover.sync_rows(rows);

    sync_panel_rect(
        &mut editor_state.model_popover,
        old_rect,
        old_monitor,
        rows_changed,
        &mut editor_state.damage_rects,
        dirty_mask,
    );

    if editor_state.model_popover.rect().is_some() {
        let hovered = editor_state
            .model_popover
            .element_at(editor_state.input.pointer.local);
        sync_panel_hover(
            &mut editor_state.model_popover,
            hovered,
            &mut editor_state.damage_rects,
            dirty_mask,
        );
    }

    sync_languages_button(editor_state, dirty_mask);
}

fn sync_languages_button(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let panel = &mut editor_state.settings_panel;
    let Some(idx) = panel.widgets.iter().position(|w| {
        matches!(
            w,
            SettingsWidget::Action {
                action: SettingsAction::OcrLanguages,
                ..
            }
        )
    }) else {
        return;
    };

    let selected = if editor_state.model_popover.open {
        Some(idx)
    } else {
        panel.selected.filter(|&s| s != idx)
    };
    if panel.selected == selected {
        return;
    }
    panel.selected = selected;
    panel.dirty = true;
    if let Some(rect) = panel.rect() {
        emit_panel_damage(
            rect,
            panel.monitor_idx,
            &mut editor_state.damage_rects,
            dirty_mask,
        );
    }
}

pub fn toggle_model_popover(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    editor_state.model_popover.open = !editor_state.model_popover.open;
    update_model_popover(editor_state, dirty_mask);
}

pub fn close_model_popover(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    editor_state.model_popover.open = false;
    update_model_popover(editor_state, dirty_mask);
}

pub fn handle_model_popover_click(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let Some(element) = editor_state
        .model_popover
        .element_at(editor_state.input.pointer.local)
    else {
        return;
    };
    let (idx, on_button) = match element {
        ModelPopoverElement::Row(idx) => (idx, false),
        ModelPopoverElement::Button(idx) => (idx, true),
    };
    let is_active = editor_state.ocr.models.active() == Some(idx);

    match editor_state.ocr.models.status(idx) {
        ModelStatus::Installed if !on_button => {
            crate::tools::ocr::use_model(editor_state, idx, dirty_mask)
        }
        ModelStatus::Installed if !is_active => editor_state.ocr.models.remove(idx),
        ModelStatus::Missing | ModelStatus::Failed => {
            editor_state.toasts.dismiss(ToastKind::OcrDownloadFailed);
            editor_state.ocr.models.download(idx);
        }
        ModelStatus::Queued | ModelStatus::Downloading(_) if on_button => {
            crate::tools::ocr::cancel_model_download(editor_state, idx)
        }
        _ => {}
    }

    update_settings_panel(editor_state, dirty_mask);
    update_model_popover(editor_state, dirty_mask);
}

/// once per iteration takes the download progress and updates everything that use it
pub fn tick_model_downloads(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let events = editor_state.ocr.models.poll();
    if events.is_empty() {
        return;
    }

    for event in events {
        match event {
            ModelEvent::Progress => {}
            ModelEvent::Ready(idx) => crate::tools::ocr::model_ready(editor_state, idx, dirty_mask),
            ModelEvent::Failed(idx, e) => crate::tools::ocr::model_failed(editor_state, idx, &e),
        }
    }

    update_settings_panel(editor_state, dirty_mask);
    if editor_state.model_popover.is_visible() {
        update_model_popover(editor_state, dirty_mask);
    }
    apply_damage_rects(editor_state, dirty_mask);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::testing::editor;
    use crate::tools::Tool;
    use crate::ui::settings_panel::{OCR_SCANNING_WIDGETS, OCR_WIDGETS};

    fn ocr_state(window: bool) -> EditorState {
        let mut state = editor(window);
        state.placements[0].size = (1600, 900);
        state.ocr.models.fake_installed();
        state.selected_tool = Tool::Ocr;
        state.toolbar.position = (300.0, 10.0);
        state.toolbar.render_pos = (300.0, 10.0);
        state.ocr.await_region = true;
        update_settings_panel(&mut state, &mut 0);
        toggle_model_popover(&mut state, &mut 0);
        state
    }

    #[test]
    fn the_list_stays_under_its_button_while_the_panel_changes_width() {
        let mut state = ocr_state(false);
        let at = state.model_popover.position;
        let panel = &state.settings_panel;
        let button = panel.render_pos.0 + crate::theme::size::padding();
        assert_eq!(at.0, button);
        assert_eq!(
            at.1,
            panel.render_pos.1 + panel.size.1 + crate::theme::size::margin()
        );

        let mut widths = vec![state.settings_panel.size.0];
        state.ocr.await_region = false;
        update_settings_panel(&mut state, &mut 0);
        update_model_popover(&mut state, &mut 0);
        widths.push(state.settings_panel.size.0);
        assert_eq!(state.model_popover.position, at);

        state.ocr.runtime.park(crate::ocr::testing::image(4, 4));
        update_settings_panel(&mut state, &mut 0);
        update_model_popover(&mut state, &mut 0);
        widths.push(state.settings_panel.size.0);
        assert_eq!(state.model_popover.position, at);
        assert!(widths[0] != widths[1] && widths[1] != widths[2]);
    }

    #[test]
    fn the_list_opens_upward_when_the_panel_is_above_the_toolbar() {
        let mut state = ocr_state(false);
        state.toolbar.position = (300.0, 840.0);
        state.toolbar.render_pos = (300.0, 840.0);
        update_settings_panel(&mut state, &mut 0);
        update_model_popover(&mut state, &mut 0);
        let panel = &state.settings_panel;
        assert!(panel.render_pos.1 < state.toolbar.render_pos.1);
        let (_, y) = state.model_popover.position;
        assert_eq!(
            y,
            panel.render_pos.1 - crate::theme::size::margin() - state.model_popover.size.1
        );
    }

    #[test]
    fn the_list_goes_under_the_toolbar_when_there_is_no_room_above_the_panel() {
        let mut state = ocr_state(false);
        state.toolbar.position = (300.0, 100.0);
        state.toolbar.render_pos = (300.0, 100.0);
        update_settings_panel(&mut state, &mut 0);
        update_model_popover(&mut state, &mut 0);
        let (tb, panel) = (&state.toolbar, &state.settings_panel);
        assert!(panel.render_pos.1 < tb.render_pos.1);
        assert_eq!(
            state.model_popover.position.1,
            tb.render_pos.1 + tb.size.1 + crate::theme::size::margin()
        );
    }

    #[test]
    fn the_list_hangs_from_its_button_in_the_window_too() {
        let mut state = ocr_state(true);
        state.placements[0].size = (1600, 900);
        state.toolbar.docked = true;
        state.toolbar.position = (300.0, 0.0);
        state.toolbar.render_pos = (300.0, 0.0);
        update_settings_panel(&mut state, &mut 0);
        update_model_popover(&mut state, &mut 0);
        let panel = &state.settings_panel;
        assert_eq!(
            state.model_popover.position.0,
            panel.render_pos.0 + crate::theme::size::padding()
        );
    }

    #[test]
    fn a_scan_keeps_the_languages_button() {
        let has_button = |widgets: &[SettingsWidget]| {
            widgets.iter().any(|w| {
                matches!(
                    w,
                    SettingsWidget::Action {
                        action: SettingsAction::OcrLanguages,
                        ..
                    }
                )
            })
        };
        assert!(has_button(OCR_WIDGETS));
        assert!(has_button(OCR_SCANNING_WIDGETS));
        assert!(matches!(
            OCR_SCANNING_WIDGETS[0],
            SettingsWidget::Action {
                action: SettingsAction::OcrLanguages,
                ..
            }
        ));
    }
}
