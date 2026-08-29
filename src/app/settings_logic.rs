// settings panel animation and positioning logic 
// TODO: need to comment a lot of stuff here before forgetting details 

use crate::editor::{EditorState, DamageZone};
use crate::tools::Tool;
use crate::types::annotations::rebuild_annotation;
use crate::types::{
    Annotation, AnnotationShape, SettingsSource, SettingsWidget,
    StepperArrow, ToolSettings, UiPanel, SpecialKey, ToggleField, compute_settings_placement, widgets_for_annotation,
    widgets_for_tool,
    STEPPER_HOLD_FAST_INTERVAL, STEPPER_HOLD_INITIAL_DELAY, STEPPER_HOLD_REPEAT_INTERVAL, STEPPER_HOLD_ACCEL_AFTER,
};
use crate::types::panel::{emit_panel_damage, sync_panel_rect, sync_panel_hover};
use crate::editor::dirty::{mark_dirty, apply_damage_rects};

use std::time::Instant;

const STEPPER_SCROLL_PIXELS_PER_STEP: f32 = 10.0;

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

    let new_widgets = match selected_ann {
        Some(ann) => widgets_for_annotation(ann),
        None => widgets_for_tool(editor_state.selected_tool),
    };

    let new_source = match selected_ann {
        Some(ann) => SettingsSource::Annotation(ann.id),
        None => SettingsSource::Tool(editor_state.selected_tool),
    };
    let source_changed = editor_state.settings_panel.active_source != Some(new_source);

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
                editor_state.settings_panel.sync_value(idx, format_stepper_number(value));
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

    let old_rect = editor_state.settings_panel.rect();
    let old_monitor = editor_state.settings_panel.monitor_idx;

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

    sync_panel_rect(
        &mut editor_state.settings_panel,
        old_rect,
        old_monitor,
        source_changed,
        &mut editor_state.damage_rects,
        dirty_mask,
    );

    if editor_state.settings_panel.rect().is_some() {
        let hovered = editor_state.settings_panel.hit_test(editor_state.pointer.local);
        let hovered_arrow = hovered.and_then(|idx| {
            editor_state
                .settings_panel
                .stepper_arrow_hit(idx, editor_state.pointer.local)
                .map(|arrow| (idx, arrow))
        });

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
    }
}

pub fn tick_stepper_arrow_hold(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let Some(hold) = editor_state.settings_panel.arrow_held else { return };
    let now = Instant::now();

    let next_due = if hold.repeat_count == 0 {
        hold.started_at + STEPPER_HOLD_INITIAL_DELAY
    } else {
        let interval = if hold.repeat_count >= STEPPER_HOLD_ACCEL_AFTER {
            STEPPER_HOLD_FAST_INTERVAL
        } else {
            STEPPER_HOLD_REPEAT_INTERVAL
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
    let Some(rect) = editor_state.settings_panel.rect() else { return };

    if editor_state.settings_panel.insert_char(ch) {
        emit_panel_damage(rect, monitor_idx, &mut editor_state.damage_rects, dirty_mask);

        if let Some(widget_idx) = editor_state.settings_panel.fields.editing.as_ref().map(|e| e.key) {
            live_apply_stepper_field(editor_state, widget_idx, dirty_mask);
        }
    }
}

pub fn handle_settings_key_press(editor_state: &mut EditorState, key: SpecialKey, dirty_mask: &mut u32) {
    let monitor_idx = editor_state.settings_panel.monitor_idx;
    let Some(rect) = editor_state.settings_panel.rect() else { return };

    let ctrl = editor_state.mod_ctrl;
    let shift = editor_state.mod_shift;

    let (changed, commit) = editor_state.settings_panel.handle_key(key, ctrl, shift);

    if changed {
        emit_panel_damage(rect, monitor_idx, &mut editor_state.damage_rects, dirty_mask);

        if let Some(widget_idx) = editor_state.settings_panel.fields.editing.as_ref().map(|e| e.key) {
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

    let Some((widget_idx, text)) = editor_state.settings_panel.commit_edit() else {
        return;
    };

    if let Some(rect) = rect {
        emit_panel_damage(rect, monitor_idx, &mut editor_state.damage_rects, dirty_mask);
    }

    try_apply_stepper_text(editor_state, widget_idx, &text, dirty_mask);
}

pub fn commit_settings_change(
    editor_state: &mut EditorState,
    changed: bool,
    apply_to_tool: impl FnOnce(&mut ToolSettings),
    apply_to_annotation: impl FnOnce(&mut Annotation),
    dirty_mask: &mut u32,
) {
    if !changed {
        return;
    }

    apply_to_tool(&mut editor_state.tool_settings);

    if let Some(idx) = active_annotation_idx(editor_state) {
        apply_to_annotation(&mut editor_state.annotations[idx]);
        rebuild_annotation(editor_state, idx);
    }

    editor_state.settings_panel.dirty = true;
    let monitor_idx = editor_state.settings_panel.monitor_idx;
    if let Some(rect) = editor_state.settings_panel.rect() {
        editor_state.damage_rects.push(DamageZone::Local { monitor_idx, rect });
    }
    mark_dirty(dirty_mask, monitor_idx);

    apply_damage_rects(editor_state, dirty_mask);
}

fn try_apply_stepper_text(
    editor_state: &mut EditorState,
    widget_idx: usize,
    text: &str,
    dirty_mask: &mut u32,
) -> bool {
    let Some(SettingsWidget::Stepper { min, max, .. }) =
        editor_state.settings_panel.widgets.get(widget_idx)
    else {
        return false;
    };
    let (min, max) = (*min, *max);

    let Ok(parsed) = text.parse::<f32>() else { return false };
    apply_stepper_field(editor_state, parsed.clamp(min, max), dirty_mask);
    true
}

fn live_apply_stepper_field(editor_state: &mut EditorState, widget_idx: usize, dirty_mask: &mut u32) {
    let Some(text) = editor_state.settings_panel.fields.editing.as_ref()
        .filter(|e| e.key == widget_idx)
        .map(|e| e.field.text.clone())
    else {
        return;
    };

    try_apply_stepper_text(editor_state, widget_idx, &text, dirty_mask);
}

fn apply_stepper_field(editor_state: &mut EditorState, new_value: f32, dirty_mask: &mut u32) {
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
        None => if is_text {
            editor_state.tool_settings.font_size
        } else {
            editor_state.tool_settings.stroke_width
        },
    };

    let changed = (current - new_value).abs() > f32::EPSILON;

    commit_settings_change(
        editor_state,
        changed,
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

    let delta = if arrow == StepperArrow::Up { step } else { -step };
    let new_value = (current + delta).clamp(min, max);

    apply_stepper_field(editor_state, new_value, dirty_mask);
}

pub fn sync_stepper_edit_text(editor_state: &mut EditorState, widget_idx: usize) {
    if !editor_state.settings_panel.fields.editing.as_ref().is_some_and(|e| e.key == widget_idx) {
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

    editor_state.settings_panel.fields.set_editing_text(widget_idx, format_stepper_number(value));
    editor_state.settings_panel.dirty = true;
}

pub fn handle_stepper_scroll(
    editor_state: &mut EditorState,
    widget_idx: usize,
    delta_y: f32,
    dirty_mask: &mut u32,
) {
    if !matches!(editor_state.settings_panel.widgets.get(widget_idx), Some(SettingsWidget::Stepper { .. })) {
        return;
    }

    if editor_state.settings_panel.fields.editing.as_ref().is_some_and(|e| e.key == widget_idx) {
        return;
    }

    let steps = editor_state
        .settings_panel
        .scroll_step(widget_idx, delta_y / STEPPER_SCROLL_PIXELS_PER_STEP);

    if steps == 0 {
        return;
    }

    let arrow = if steps > 0 { StepperArrow::Up } else { StepperArrow::Down };
    for _ in 0..steps.unsigned_abs() {
        apply_stepper_arrow_step(editor_state, widget_idx, arrow, dirty_mask);
    }

    update_settings_panel(editor_state, dirty_mask);
}

fn format_stepper_number(v: f32) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i32)
    } else {
        format!("{:.1}", v)
    }
}