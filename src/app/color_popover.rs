use crate::editor::EditorState;
use crate::renderer::char_index_for_x;
use crate::types::click::ClickTarget;
use crate::types::panel::{sync_panel_rect, sync_panel_hover, emit_panel_damage, UiPanel};
use crate::types::color_popover::{
    ColorField, ColorPopoverElement, COLORPICKER_WIDTH, COLORPICKER_HEIGHT, COLORPICKER_OFFSET, FIELD_FONT_SIZE,
};
use crate::types::text_field::{is_hex_char, is_rgba_channel_char, CursorInit};
use crate::types::SpecialKey;
use super::settings_logic::commit_settings_change;
use tiny_skia::Color;

pub fn update_color_popover(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let old_rect = editor_state.color_popover.rect();
    let old_monitor = editor_state.color_popover.monitor_idx;

    if editor_state.color_popover.open {
        let (pos, monitor_idx) = compute_color_popover_placement(editor_state);
        editor_state.color_popover.position = pos;
        editor_state.color_popover.render_pos = pos;
        editor_state.color_popover.monitor_idx = monitor_idx;
    }

    editor_state.color_popover.sync_field_values();

    sync_panel_rect(
        &mut editor_state.color_popover,
        old_rect,
        old_monitor,
        false,
        &mut editor_state.damage_rects,
        dirty_mask,
    );

    if editor_state.color_popover.rect().is_some() {
        let hovered = hover_test(editor_state, editor_state.pointer.local);
        sync_panel_hover(
            &mut editor_state.color_popover,
            hovered,
            &mut editor_state.damage_rects,
            dirty_mask,
        );
    }
}

/// One hit-test for all clickable elements for hover 
/// priority is the same as in `handle_color_popover_click`
fn hover_test(editor_state: &EditorState, local: (f64, f64)) -> Option<ColorPopoverElement> {
    let cp = &editor_state.color_popover;

    if cp.sv_square_hit(local) {
        return Some(ColorPopoverElement::SvSquare);
    }
    if cp.hue_slider_hit(local) {
        return Some(ColorPopoverElement::HueSlider);
    }
    if let Some(idx) = cp.swatch_hit(local) {
        return Some(ColorPopoverElement::Swatch(idx));
    }
    if cp.hex_field_hit(local) {
        return Some(ColorPopoverElement::Field(ColorField::Hex));
    }
    if let Some(field) = cp.rgba_field_hit(local) {
        return Some(ColorPopoverElement::Field(field));
    }
    None
}

fn compute_color_popover_placement(editor_state: &EditorState) -> ((f32, f32), usize) {
    let sp = &editor_state.settings_panel;
    let tb = &editor_state.toolbar;
    let monitor_idx = sp.monitor_idx;

    let monitor_width = editor_state.placements[monitor_idx].size.0 as f32;
    let monitor_height = editor_state.placements[monitor_idx].size.1 as f32;

    let sp_bottom = sp.render_pos.1 + sp.size.1;
    let tb_bottom = tb.render_pos.1 + tb.size.1;
    let combined_bottom = sp_bottom.max(tb_bottom);

    let mut side_y = combined_bottom - COLORPICKER_HEIGHT;

    if side_y < COLORPICKER_OFFSET {
        side_y = COLORPICKER_OFFSET;
    }
    if side_y + COLORPICKER_HEIGHT > monitor_height - COLORPICKER_OFFSET {
        side_y = monitor_height - COLORPICKER_HEIGHT - COLORPICKER_OFFSET;
    }

    let x_left = sp.render_pos.0 - COLORPICKER_WIDTH - COLORPICKER_OFFSET;
    let x_right = sp.render_pos.0 + sp.size.0 + COLORPICKER_OFFSET;

    let space_left = x_left >= COLORPICKER_OFFSET;
    let space_right = x_right + COLORPICKER_WIDTH <= monitor_width - COLORPICKER_OFFSET;

    if space_left {
        return ((x_left, side_y), monitor_idx);
    } else if space_right {
        return ((x_right, side_y), monitor_idx);
    }

    let mut final_x = sp.render_pos.0;

    if final_x < COLORPICKER_OFFSET {
        final_x = COLORPICKER_OFFSET;
    }
    if final_x + COLORPICKER_WIDTH > monitor_width - COLORPICKER_OFFSET {
        final_x = monitor_width - COLORPICKER_WIDTH - COLORPICKER_OFFSET;
    }

    let y_below = sp.render_pos.1 + sp.size.1 + COLORPICKER_OFFSET;
    let y_above = sp.render_pos.1 - COLORPICKER_OFFSET - COLORPICKER_HEIGHT;

    let space_below = y_below + COLORPICKER_HEIGHT <= monitor_height;
    let space_above = y_above >= 0.0;

    let sp_is_below_tb = sp.render_pos.1 >= tb.render_pos.1;

    let final_y = if sp_is_below_tb {
        if space_below {
            y_below
        } else if space_above {
            y_above
        } else {
            monitor_height - COLORPICKER_HEIGHT - COLORPICKER_OFFSET
        }
    } else {
        if space_above {
            y_above
        } else if space_below {
            y_below
        } else {
            COLORPICKER_OFFSET
        }
    };

    ((final_x, final_y), monitor_idx)
}

fn emit_color_popover_damage(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let monitor_idx = editor_state.color_popover.monitor_idx;
    if let Some(rect) = editor_state.color_popover.rect() {
        emit_panel_damage(rect, monitor_idx, &mut editor_state.damage_rects, dirty_mask);
    }
}

pub fn apply_color_selection(editor_state: &mut EditorState, color: Color, dirty_mask: &mut u32) {
    commit_settings_change(
        editor_state,
        true,
        move |ts| ts.color = color,
        move |ann| ann.color = color,
        dirty_mask,
    );
}

fn field_char_filter(field: ColorField) -> impl Fn(char) -> bool {
    move |ch| match field {
        ColorField::Hex => is_hex_char(ch),
        ColorField::R | ColorField::G | ColorField::B | ColorField::A => is_rgba_channel_char(ch),
    }
}

fn apply_color_field_text(
    editor_state: &mut EditorState,
    field: ColorField,
    text: &str,
    record: bool,
    dirty_mask: &mut u32,
) -> bool {
    let applied = match field {
        ColorField::Hex => editor_state.color_popover.try_apply_hex_text(text),
        _ => editor_state.color_popover.try_apply_rgba_text(field, text),
    };

    let Some(color) = applied else { return false };

    if record {
        editor_state.color_popover.record_used_color(color);
    }
    editor_state.color_popover.sync_field_values();
    apply_color_selection(editor_state, color, dirty_mask);
    true
}

fn live_apply_color_field(editor_state: &mut EditorState, field: ColorField, dirty_mask: &mut u32) {
    let Some(text) = editor_state.color_popover.fields.editing.as_ref()
        .filter(|e| e.key == field)
        .map(|e| e.field.text.clone())
    else {
        return;
    };

    apply_color_field_text(editor_state, field, &text, false, dirty_mask);
}

fn begin_color_field_edit(
    editor_state: &mut EditorState,
    field: ColorField,
    local: (f64, f64),
    dirty_mask: &mut u32,
) {
    if editor_state.color_popover.fields.is_editing() {
        commit_color_field_edit(editor_state, dirty_mask);
    }

    let current_text = editor_state.color_popover.field_text(field);

    let click_pos = (local.0 as f32, local.1 as f32);
    let is_double_click = editor_state
        .click_tracker
        .register(ClickTarget::ColorField(field), click_pos);

    let cursor_init = if is_double_click {
        CursorInit::SelectAll
    } else if let Some(rect) = editor_state.color_popover.field_rect(field) {
        let click_x = local.0 as f32 - rect.left();
        let idx = char_index_for_x(&current_text, click_x, FIELD_FONT_SIZE, &mut editor_state.font_system);
        CursorInit::At(idx)
    } else {
        CursorInit::End
    };

    editor_state.color_popover.fields.begin_edit(field, current_text, cursor_init);
    editor_state.color_popover.dirty = true;
    emit_color_popover_damage(editor_state, dirty_mask);
}

pub fn commit_color_field_edit(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let Some((field, text)) = editor_state.color_popover.fields.commit_edit() else {
        return;
    };

    apply_color_field_text(editor_state, field, &text, true, dirty_mask);

    editor_state.color_popover.sync_field_values();
    editor_state.color_popover.dirty = true;
    emit_color_popover_damage(editor_state, dirty_mask);
}

pub fn handle_color_field_text_input(editor_state: &mut EditorState, ch: char, dirty_mask: &mut u32) {
    let Some(field) = editor_state.color_popover.fields.editing.as_ref().map(|e| e.key) else { return };
    let allowed = field_char_filter(field);
    if editor_state.color_popover.fields.insert_char(ch, allowed) {
        editor_state.color_popover.dirty = true;
        emit_color_popover_damage(editor_state, dirty_mask);
        live_apply_color_field(editor_state, field, dirty_mask);
    }
}

pub fn handle_color_field_key_press(editor_state: &mut EditorState, key: SpecialKey, dirty_mask: &mut u32) {
    let Some(field) = editor_state.color_popover.fields.editing.as_ref().map(|e| e.key) else { return };
    let allowed = field_char_filter(field);
    let ctrl = editor_state.mod_ctrl;
    let shift = editor_state.mod_shift;

    let (changed, commit) = editor_state.color_popover.fields.handle_key(key, ctrl, shift, allowed);

    if changed {
        editor_state.color_popover.dirty = true;
        emit_color_popover_damage(editor_state, dirty_mask);
        live_apply_color_field(editor_state, field, dirty_mask);
    }

    if commit {
        commit_color_field_edit(editor_state, dirty_mask);
    }
}

pub fn handle_color_popover_click(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let local = editor_state.pointer.local;

    if editor_state.color_popover.sv_square_hit(local) {
        editor_state.color_popover.sv_square.dragging = true;
        editor_state.color_popover.set_sv_from_local(local);
        editor_state.color_popover.dirty = true;
        emit_color_popover_damage(editor_state, dirty_mask);

        let color = editor_state.color_popover.sv_square.color();
        apply_color_selection(editor_state, color, dirty_mask);
        return;
    }

    if editor_state.color_popover.hue_slider_hit(local) {
        editor_state.color_popover.hue_dragging = true;
        editor_state.color_popover.set_hue_from_local(local);
        editor_state.color_popover.dirty = true;
        emit_color_popover_damage(editor_state, dirty_mask);

        let color = editor_state.color_popover.sv_square.color();
        apply_color_selection(editor_state, color, dirty_mask);
        return;
    }

    if let Some(idx) = editor_state.color_popover.swatch_hit(local) {
        if let Some(color) = editor_state.color_popover.palette().get(idx).copied() {
            editor_state.color_popover.select_color(color);
            editor_state.color_popover.record_used_color(color);
            editor_state.color_popover.dirty = true;
            emit_color_popover_damage(editor_state, dirty_mask);
            apply_color_selection(editor_state, color, dirty_mask);
        }
        return;
    }

    if editor_state.color_popover.hex_field_hit(local) {
        begin_color_field_edit(editor_state, ColorField::Hex, local, dirty_mask);
        return;
    }

    if let Some(field) = editor_state.color_popover.rgba_field_hit(local) {
        begin_color_field_edit(editor_state, field, local, dirty_mask);
        return;
    }

    update_color_popover(editor_state, dirty_mask);
}

pub fn handle_color_popover_drag(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let local = editor_state.pointer.local;

    let changed = if editor_state.color_popover.sv_square.dragging {
        editor_state.color_popover.set_sv_from_local(local);
        true
    } else if editor_state.color_popover.hue_dragging {
        editor_state.color_popover.set_hue_from_local(local);
        true
    } else {
        false
    };

    if !changed {
        return;
    }

    editor_state.color_popover.dirty = true;
    emit_color_popover_damage(editor_state, dirty_mask);

    let color = editor_state.color_popover.sv_square.color();
    apply_color_selection(editor_state, color, dirty_mask);
}

pub fn handle_color_popover_release(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let was_dragging = editor_state.color_popover.sv_square.dragging
        || editor_state.color_popover.hue_dragging;

    editor_state.color_popover.sv_square.dragging = false;
    editor_state.color_popover.hue_dragging = false;

    if !was_dragging {
        return;
    }

    let color = editor_state.color_popover.sv_square.color();
    editor_state.color_popover.record_used_color(color);
    editor_state.color_popover.dirty = true;
    emit_color_popover_damage(editor_state, dirty_mask);
}

pub fn close_color_popover(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    if editor_state.color_popover.fields.is_editing() {
        commit_color_field_edit(editor_state, dirty_mask);
    }
    editor_state.color_popover.open = false;
    update_color_popover(editor_state, dirty_mask);
}