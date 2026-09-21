// Shared stuff between color/setting changes and undo history
// Kept below `app` so tools can use it without depending on the app layer
// to keep one side relation yk

use tiny_skia::Color;

use crate::editor::dirty::{apply_damage_rects, mark_dirty};
use crate::editor::{DamageZone, EditorState};
use crate::tools::Tool;
use crate::types::annotations::rebuild_annotation;
use crate::types::{Annotation, ToolSettings};
use crate::ui::panel::{UiPanel, emit_panel_damage};

pub fn active_annotation_idx(editor_state: &EditorState) -> Option<usize> {
    if editor_state.selected_tool == Tool::Pick || editor_state.selected_tool == Tool::Text {
        editor_state.selected_annotation
    } else {
        None
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

/// Marks color popover for redraw and damages area it covers.
pub fn damage_color_popover(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    editor_state.color_popover.dirty = true;
    let monitor_idx = editor_state.color_popover.monitor_idx;
    if let Some(rect) = editor_state.color_popover.rect() {
        emit_panel_damage(
            rect,
            monitor_idx,
            &mut editor_state.damage_rects,
            dirty_mask,
        );
    }
}

/// common path for color selection, since we have two ways to use color selection
/// swatch click and eyedropper pick
pub fn use_color(editor_state: &mut EditorState, color: Color, dirty_mask: &mut u32) {
    editor_state.color_popover.select_color(color);
    editor_state.color_popover.record_used_color(color);
    editor_state.color_popover.sync_field_values();
    damage_color_popover(editor_state, dirty_mask);
    apply_color_selection(editor_state, color, true, dirty_mask);
}

pub fn apply_color_selection(
    editor_state: &mut EditorState,
    color: Color,
    record_undo: bool,
    dirty_mask: &mut u32,
) {
    commit_settings_change(
        editor_state,
        true,
        record_undo,
        move |ts| ts.color = color,
        move |ann| ann.color = color,
        dirty_mask,
    );
}
