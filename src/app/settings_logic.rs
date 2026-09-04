// Settings panel animation and positioning logic.
// TODO: need to comment a lot of stuff here before forgetting details

use crate::editor::dirty::{apply_damage_rects, mark_dirty};
use crate::editor::{DamageZone, EditorState};
use crate::tools::Tool;
use crate::types::annotations::rebuild_annotation;
use crate::ui::panel::{emit_panel_damage, sync_panel_hover, sync_panel_rect};
use crate::types::{Annotation, AnnotationShape, SpecialKey, ToolSettings};
use crate::interaction::{HOLD_ACCEL_AFTER, HOLD_FAST_INTERVAL, HOLD_INITIAL_DELAY, HOLD_REPEAT_INTERVAL};
use crate::ui::panel::UiPanel;
use crate::ui::settings_panel::{OCR_AWAITING_WIDGETS, OCR_DOWNLOADING_WIDGETS, OCR_NO_MODEL_WIDGETS, OCR_SCANNING_WIDGETS, OCR_WIDGETS, OCR_WIDGETS_DOWNLOADING, SettingsAction, SettingsSource, SettingsWidget, StepperArrow, ToggleField, compute_settings_placement, widgets_for_annotation, widgets_for_tool};
use std::time::Instant;


pub fn active_annotation_idx(editor_state: &EditorState) -> Option<usize> {
    if editor_state.selected_tool == Tool::Pick || editor_state.selected_tool == Tool::Text {
        editor_state.selected_annotation
    } else {
        None
    }
}

pub fn update_settings_panel(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let ann_idx = active_annotation_idx(editor_state);
    let selected_ann = ann_idx.and_then(|i| editor_state.annotations.get(i));

    // A running scan owns the panel outright: its buttons act on a result that
    // isn't there yet.
    let ocr = editor_state.selected_tool == Tool::Ocr;
    let scanning = ocr && editor_state.ocr.is_busy();
    let awaiting = ocr && editor_state.ocr_await_region;
    let no_model = ocr && editor_state.ocr_models.installed_count() == 0;
    let downloading = editor_state.ocr_models.is_downloading();

    let new_source = match selected_ann {
        _ if no_model => SettingsSource::OcrNoModel { downloading },
        _ if scanning => SettingsSource::OcrScanning,
        _ if awaiting => SettingsSource::OcrAwaiting { downloading },
        _ if ocr => SettingsSource::OcrResult { downloading },
        Some(ann) => SettingsSource::Annotation(ann.id),
        None => SettingsSource::Tool(editor_state.selected_tool),
    };

    let new_widgets = match new_source {
        SettingsSource::OcrScanning => OCR_SCANNING_WIDGETS,
        SettingsSource::OcrNoModel { downloading: false } => OCR_NO_MODEL_WIDGETS,
        SettingsSource::OcrAwaiting { downloading: false } => OCR_AWAITING_WIDGETS,
        SettingsSource::OcrNoModel { downloading: true }
        | SettingsSource::OcrAwaiting { downloading: true } => OCR_DOWNLOADING_WIDGETS,
        SettingsSource::OcrResult { downloading: false } => OCR_WIDGETS,
        SettingsSource::OcrResult { downloading: true } => OCR_WIDGETS_DOWNLOADING,
        SettingsSource::Annotation(_) => selected_ann.map_or(&[][..], widgets_for_annotation),
        SettingsSource::Tool(tool) => widgets_for_tool(tool),
    };
    let source_changed = editor_state.settings_panel.active_source != Some(new_source);

    let old_rect = editor_state.settings_panel.rect();
    let old_monitor = editor_state.settings_panel.monitor_idx;

    if source_changed {
        editor_state.settings_panel.widgets = new_widgets;
        editor_state.settings_panel.active_source = Some(new_source);
        editor_state.settings_panel.size.0 = editor_state.settings_panel.width();
        editor_state.settings_panel.fields.values.clear();
    }

    let widgets = editor_state.settings_panel.widgets;
    for (idx, widget) in widgets.iter().enumerate() {
        match widget {
            SettingsWidget::Stepper { .. } => {
                let value = match selected_ann {
                    Some(ann) => match &ann.shape {
                        AnnotationShape::Text { font_size, .. } => *font_size,
                        _ => ann.stroke_width,
                    },
                    None => match editor_state.selected_tool {
                        Tool::Text => editor_state.tool_settings.font_size,
                        _ => editor_state.tool_settings.stroke_width,
                    },
                };
                editor_state
                    .settings_panel
                    .sync_value(idx, format_stepper_number(value));
            }
            SettingsWidget::Toggle { field, .. } => {
                let value = match selected_ann {
                    Some(ann) => match &ann.shape {
                        AnnotationShape::Text { bold, italic, .. } => match field {
                            ToggleField::Bold => *bold,
                            ToggleField::Italic => *italic,
                        },
                        _ => false,
                    },
                    None => match field {
                        ToggleField::Bold => editor_state.tool_settings.bold,
                        ToggleField::Italic => editor_state.tool_settings.italic,
                    },
                };
                editor_state.settings_panel.set_toggled(idx, value);
            }
            _ => {}
        }
    }

    let should_be_visible = !new_widgets.is_empty() && !editor_state.tool_active;

    if should_be_visible {
        let (pos, monitor_idx) = compute_settings_placement(editor_state);
        editor_state.settings_panel.position = pos;
        editor_state.settings_panel.render_pos = pos;
        editor_state.settings_panel.monitor_idx = monitor_idx;
        editor_state.settings_panel.visible = true;
    } else {
        editor_state.settings_panel.visible = false;
        editor_state.settings_panel.arrow_held = None;
    }

    let download = editor_state.ocr_models.download_progress();
    let download_changed = editor_state.settings_panel.download != download;
    editor_state.settings_panel.download = download;

    sync_panel_rect(
        &mut editor_state.settings_panel,
        old_rect,
        old_monitor,
        source_changed || download_changed,
        &mut editor_state.damage_rects,
        dirty_mask,
    );

    if editor_state.settings_panel.rect().is_some() {
        let prev_hover = editor_state.settings_panel.hovered;
        let (_, hovered) = editor_state
            .settings_panel
            .hit_test(editor_state.pointer.local);
        let hovered_arrow = hovered.and_then(|idx| {
            editor_state
                .settings_panel
                .stepper_arrow_hit(idx, editor_state.pointer.local)
                .map(|arrow| (idx, arrow))
        });

        if hovered != prev_hover && !editor_state.settings_panel.is_editing()
            && let Some(snapshot) = editor_state.settings_panel.pre_edit_snapshot.take()
                && editor_state.annotations != snapshot {
                    editor_state.undo_stack.push(snapshot);
                    editor_state.redo_stack.clear();
                }

        sync_panel_hover(
            &mut editor_state.settings_panel,
            (hovered, hovered_arrow),
            &mut editor_state.damage_rects,
            dirty_mask,
        );

        if let Some(hold) = editor_state.settings_panel.arrow_held.as_ref() {
            let still_on_arrow = hovered_arrow == Some((hold.widget_idx, hold.arrow));
            if !still_on_arrow {
                editor_state.settings_panel.arrow_held = None;
            }
        }
    } else if !editor_state.settings_panel.is_editing()
        && let Some(snapshot) = editor_state.settings_panel.pre_edit_snapshot.take()
            && editor_state.annotations != snapshot {
                editor_state.undo_stack.push(snapshot);
                editor_state.redo_stack.clear();
            }
}

/// Run a one-shot panel button. Kept here rather than in `input` so the panel's
/// buttons stay next to the rest of its behaviour.
pub fn run_settings_action(
    editor_state: &mut EditorState,
    action: SettingsAction,
    dirty_mask: &mut u32,
) {
    match action {
        SettingsAction::OcrRescan => crate::tools::ocr::restart_ocr(editor_state, dirty_mask),
        SettingsAction::OcrCopyAll => crate::tools::ocr::copy_all(editor_state, dirty_mask),
        SettingsAction::OcrLanguages => {
            super::model_popover::toggle_model_popover(editor_state, dirty_mask)
        }
    }
    update_settings_panel(editor_state, dirty_mask);
}

/// same as color popover
pub fn compute_popover_placement(
    editor_state: &EditorState,
    (width, height): (f32, f32),
    offset: f32,
) -> ((f32, f32), usize) {
    let sp = &editor_state.settings_panel;
    let tb = &editor_state.toolbar;
    let monitor_idx = sp.monitor_idx;

    let monitor_width = editor_state.placements[monitor_idx].size.0 as f32;
    let monitor_height = editor_state.placements[monitor_idx].size.1 as f32;

    let sp_bottom = sp.render_pos.1 + sp.size.1;
    let tb_bottom = tb.render_pos.1 + tb.size.1;
    let combined_bottom = sp_bottom.max(tb_bottom);

    let mut side_y = combined_bottom - height;

    if side_y < offset {
        side_y = offset;
    }
    if side_y + height > monitor_height - offset {
        side_y = monitor_height - height - offset;
    }

    let x_left = sp.render_pos.0 - width - offset;
    let x_right = sp.render_pos.0 + sp.size.0 + offset;

    let space_left = x_left >= offset;
    let space_right = x_right + width <= monitor_width - offset;

    if space_left {
        return ((x_left, side_y), monitor_idx);
    } else if space_right {
        return ((x_right, side_y), monitor_idx);
    }

    let mut final_x = sp.render_pos.0;

    if final_x < offset {
        final_x = offset;
    }
    if final_x + width > monitor_width - offset {
        final_x = monitor_width - width - offset;
    }

    let y_below = sp.render_pos.1 + sp.size.1 + offset;
    let y_above = sp.render_pos.1 - offset - height;

    let space_below = y_below + height <= monitor_height;
    let space_above = y_above >= 0.0;

    let sp_is_below_tb = sp.render_pos.1 >= tb.render_pos.1;

    let final_y = if sp_is_below_tb {
        if space_below {
            y_below
        } else if space_above {
            y_above
        } else {
            monitor_height - height - offset
        }
    } else {
        if space_above {
            y_above
        } else if space_below {
            y_below
        } else {
            offset
        }
    };

    ((final_x, final_y), monitor_idx)
}

pub fn tick_stepper_arrow_hold(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let Some(hold) = editor_state.settings_panel.arrow_held else {
        return;
    };
    let now = Instant::now();

    let next_due = if hold.repeat_count == 0 {
        hold.started_at + HOLD_INITIAL_DELAY
    } else {
        let interval = if hold.repeat_count >= HOLD_ACCEL_AFTER {
            HOLD_FAST_INTERVAL
        } else {
            HOLD_REPEAT_INTERVAL
        };
        hold.last_step_at + interval
    };

    if now < next_due {
        return;
    }

    apply_stepper_arrow_step(editor_state, hold.widget_idx, hold.arrow, dirty_mask);
    update_settings_panel(editor_state, dirty_mask);

    if let Some(hold) = editor_state.settings_panel.arrow_held.as_mut() {
        hold.repeat_count += 1;
        hold.last_step_at = now;
    }
}

pub fn handle_settings_text_input(editor_state: &mut EditorState, ch: char, dirty_mask: &mut u32) {
    let monitor_idx = editor_state.settings_panel.monitor_idx;
    let Some(rect) = editor_state.settings_panel.rect() else {
        return;
    };

    if editor_state.settings_panel.insert_char(ch) {
        emit_panel_damage(
            rect,
            monitor_idx,
            &mut editor_state.damage_rects,
            dirty_mask,
        );

        if let Some(widget_idx) = editor_state
            .settings_panel
            .fields
            .editing
            .as_ref()
            .map(|e| e.key)
        {
            live_apply_stepper_field(editor_state, widget_idx, dirty_mask);
        }
    }
}

pub fn handle_settings_key_press(
    editor_state: &mut EditorState,
    key: SpecialKey,
    dirty_mask: &mut u32,
) {
    let monitor_idx = editor_state.settings_panel.monitor_idx;
    let Some(rect) = editor_state.settings_panel.rect() else {
        return;
    };

    let ctrl = editor_state.mod_ctrl;
    let shift = editor_state.mod_shift;

    let (changed, commit) = editor_state.settings_panel.handle_key(key, ctrl, shift);

    if changed {
        emit_panel_damage(
            rect,
            monitor_idx,
            &mut editor_state.damage_rects,
            dirty_mask,
        );

        if let Some(widget_idx) = editor_state
            .settings_panel
            .fields
            .editing
            .as_ref()
            .map(|e| e.key)
        {
            live_apply_stepper_field(editor_state, widget_idx, dirty_mask);
        }
    }

    if commit {
        commit_stepper_text_edit(editor_state, dirty_mask);
    }
}

pub fn commit_stepper_text_edit(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let monitor_idx = editor_state.settings_panel.monitor_idx;
    let rect = editor_state.settings_panel.rect();

    let snapshot = editor_state.settings_panel.pre_edit_snapshot.take();

    let Some((widget_idx, text)) = editor_state.settings_panel.commit_edit() else {
        return;
    };

    if let Some(rect) = rect {
        emit_panel_damage(
            rect,
            monitor_idx,
            &mut editor_state.damage_rects,
            dirty_mask,
        );
    }

    try_apply_stepper_text(editor_state, widget_idx, &text, false, dirty_mask);

    update_settings_panel(editor_state, dirty_mask);
    apply_damage_rects(editor_state, dirty_mask);

    if let Some(snapshot) = snapshot
        && editor_state.annotations != snapshot {
            editor_state.undo_stack.push(snapshot);
            editor_state.redo_stack.clear();
        }
}

pub fn commit_settings_change(
    editor_state: &mut EditorState,
    changed: bool,
    record_undo: bool,
    apply_to_tool: impl FnOnce(&mut ToolSettings),
    apply_to_annotation: impl FnOnce(&mut Annotation),
    dirty_mask: &mut u32,
) {
    if !changed {
        return;
    }

    apply_to_tool(&mut editor_state.tool_settings);

    if let Some(idx) = active_annotation_idx(editor_state) {
        let old_damage = editor_state.annotations[idx].damage_bbox(true);
        let old_layer_damage = editor_state.annotations[idx].damage_bbox(false);
        editor_state
            .damage_rects
            .push(DamageZone::Global(old_damage));
        editor_state.layer_damage_rects.push(old_layer_damage);

        if record_undo {
            editor_state.push_undo();
        }
        apply_to_annotation(&mut editor_state.annotations[idx]);
        rebuild_annotation(editor_state, idx);
    }

    editor_state.settings_panel.dirty = true;
    let monitor_idx = editor_state.settings_panel.monitor_idx;
    if let Some(rect) = editor_state.settings_panel.rect() {
        editor_state
            .damage_rects
            .push(DamageZone::Local { monitor_idx, rect });
    }
    mark_dirty(dirty_mask, monitor_idx);

    apply_damage_rects(editor_state, dirty_mask);
}

fn try_apply_stepper_text(
    editor_state: &mut EditorState,
    widget_idx: usize,
    text: &str,
    record_undo: bool,
    dirty_mask: &mut u32,
) -> bool {
    let Some(SettingsWidget::Stepper { min, max, .. }) =
        editor_state.settings_panel.widgets.get(widget_idx)
    else {
        return false;
    };
    let (min, max) = (*min, *max);

    let Ok(parsed) = text.parse::<f32>() else {
        return false;
    };
    apply_stepper_field(
        editor_state,
        parsed.clamp(min, max),
        record_undo,
        dirty_mask,
    );
    true
}

fn live_apply_stepper_field(
    editor_state: &mut EditorState,
    widget_idx: usize,
    dirty_mask: &mut u32,
) {
    let Some(text) = editor_state
        .settings_panel
        .fields
        .editing
        .as_ref()
        .filter(|e| e.key == widget_idx)
        .map(|e| e.field.text.clone())
    else {
        return;
    };

    try_apply_stepper_text(editor_state, widget_idx, &text, false, dirty_mask);
}

fn apply_stepper_field(
    editor_state: &mut EditorState,
    new_value: f32,
    record_undo: bool,
    dirty_mask: &mut u32,
) {
    let ann_idx = active_annotation_idx(editor_state);

    let is_text = match ann_idx.and_then(|i| editor_state.annotations.get(i)) {
        Some(ann) => matches!(ann.shape, AnnotationShape::Text { .. }),
        None => editor_state.selected_tool == Tool::Text,
    };

    let current = match ann_idx.and_then(|i| editor_state.annotations.get(i)) {
        Some(ann) => match &ann.shape {
            AnnotationShape::Text { font_size, .. } => *font_size,
            _ => ann.stroke_width,
        },
        None => {
            if is_text {
                editor_state.tool_settings.font_size
            } else {
                editor_state.tool_settings.stroke_width
            }
        }
    };

    let changed = (current - new_value).abs() > f32::EPSILON;

    commit_settings_change(
        editor_state,
        changed,
        record_undo,
        move |ts| {
            if is_text {
                ts.font_size = new_value;
            } else {
                ts.stroke_width = new_value;
            }
        },
        move |ann| match &mut ann.shape {
            AnnotationShape::Text { font_size, .. } => *font_size = new_value,
            _ => ann.stroke_width = new_value,
        },
        dirty_mask,
    );
}

pub fn apply_toggle_field(
    editor_state: &mut EditorState,
    field: ToggleField,
    new_value: bool,
    dirty_mask: &mut u32,
) {
    commit_settings_change(
        editor_state,
        true,
        true,
        move |ts| match field {
            ToggleField::Bold => ts.bold = new_value,
            ToggleField::Italic => ts.italic = new_value,
        },
        move |ann| {
            if let AnnotationShape::Text { bold, italic, .. } = &mut ann.shape {
                match field {
                    ToggleField::Bold => *bold = new_value,
                    ToggleField::Italic => *italic = new_value,
                }
            }
        },
        dirty_mask,
    );
}

pub fn apply_stepper_arrow_step(
    editor_state: &mut EditorState,
    widget_idx: usize,
    arrow: StepperArrow,
    dirty_mask: &mut u32,
) {
    if editor_state.settings_panel.pre_edit_snapshot.is_none() {
        editor_state.settings_panel.pre_edit_snapshot = Some(editor_state.annotations.clone());
    }

    let Some(SettingsWidget::Stepper { min, max, step, .. }) =
        editor_state.settings_panel.widgets.get(widget_idx)
    else {
        return;
    };
    let (min, max, step) = (*min, *max, *step);

    let ann_idx = active_annotation_idx(editor_state);
    let current = match ann_idx.and_then(|i| editor_state.annotations.get(i)) {
        Some(ann) => match &ann.shape {
            AnnotationShape::Text { font_size, .. } => *font_size,
            _ => ann.stroke_width,
        },
        None => match editor_state.selected_tool {
            Tool::Text => editor_state.tool_settings.font_size,
            _ => editor_state.tool_settings.stroke_width,
        },
    };

    let delta = if arrow == StepperArrow::Up {
        step
    } else {
        -step
    };
    let new_value = (current + delta).clamp(min, max);

    apply_stepper_field(editor_state, new_value, false, dirty_mask);
}

pub fn sync_stepper_edit_text(editor_state: &mut EditorState, widget_idx: usize) {
    if editor_state
        .settings_panel
        .fields
        .editing
        .as_ref().is_none_or(|e| e.key != widget_idx)
    {
        return;
    }

    let ann_idx = active_annotation_idx(editor_state);
    let value = match ann_idx.and_then(|i| editor_state.annotations.get(i)) {
        Some(ann) => match &ann.shape {
            AnnotationShape::Text { font_size, .. } => *font_size,
            _ => ann.stroke_width,
        },
        None => match editor_state.selected_tool {
            Tool::Text => editor_state.tool_settings.font_size,
            _ => editor_state.tool_settings.stroke_width,
        },
    };

    editor_state
        .settings_panel
        .fields
        .set_editing_text(widget_idx, format_stepper_number(value));
    editor_state.settings_panel.dirty = true;
}

pub fn handle_stepper_scroll(
    editor_state: &mut EditorState,
    widget_idx: usize,
    delta_y: f32,
    dirty_mask: &mut u32,
) {
    if !matches!(
        editor_state.settings_panel.widgets.get(widget_idx),
        Some(SettingsWidget::Stepper { .. })
    ) {
        return;
    }

    let steps = editor_state
        .settings_panel
        .scroll_step(widget_idx, delta_y / crate::interaction::SCROLL_PIXELS_PER_STEP);

    if steps == 0 {
        return;
    }

    if editor_state.settings_panel.pre_edit_snapshot.is_none() {
        editor_state.settings_panel.pre_edit_snapshot = Some(editor_state.annotations.clone());
    }

    let arrow = if steps > 0 {
        StepperArrow::Up
    } else {
        StepperArrow::Down
    };
    for _ in 0..steps.unsigned_abs() {
        apply_stepper_arrow_step(editor_state, widget_idx, arrow, dirty_mask);
    }

    update_settings_panel(editor_state, dirty_mask);

    sync_stepper_edit_text(editor_state, widget_idx);
}

fn format_stepper_number(v: f32) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i32)
    } else {
        format!("{:.1}", v)
    }
}