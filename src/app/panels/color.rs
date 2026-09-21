use super::settings::compute_popover_placement;
use crate::editor::EditorState;
use crate::editor::edits::{apply_color_selection, damage_color_popover, use_color};
use crate::interaction::ClickTarget;
use crate::types::SpecialKey;
use crate::ui::color_popover;
use crate::ui::color_popover::{ColorField, ColorPopoverElement, field_font_size, step_hex_text};
use crate::ui::panel::{UiPanel, sync_panel_hover, sync_panel_rect};
use crate::ui::settings_panel::char_index_for_x;
use crate::ui::text_field::{CursorInit, is_hex_char, is_rgba_channel_char};

pub fn update_color_popover(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let old_rect = editor_state.color_popover.rect();
    let old_monitor = editor_state.color_popover.monitor_idx;

    // Placed off the settings panel, which is itself placed off the toolbar's
    // animated position - so re-placing here every frame is what makes the
    // popover travel with the toolbar. It keeps tracking while fading out too,
    // otherwise it would detach and sit there for the length of the fade.
    if editor_state.color_popover.is_visible() {
        let (pos, monitor_idx) = compute_popover_placement(
            editor_state,
            (color_popover::width(), color_popover::height()),
            crate::theme::size::margin(),
        );
        editor_state.color_popover.position = pos;
        editor_state.color_popover.render_pos = pos;
        editor_state.color_popover.monitor_idx = monitor_idx;
    }

    editor_state.color_popover.sync_field_values();

    let picking = editor_state.pick_once;
    let picking_changed = editor_state.color_popover.picking != picking;
    editor_state.color_popover.picking = picking;

    sync_panel_rect(
        &mut editor_state.color_popover,
        old_rect,
        old_monitor,
        picking_changed,
        &mut editor_state.damage_rects,
        dirty_mask,
    );

    if editor_state.color_popover.rect().is_some() {
        let prev_hover = editor_state.color_popover.hovered;
        let hovered = hover_test(editor_state, editor_state.input.pointer.local);
        if prev_hover != hovered
            && !editor_state.color_popover.fields.is_editing()
            && !editor_state.color_popover.sv_square.dragging
            && !editor_state.color_popover.hue_dragging
        {
            let snapshot = editor_state.color_popover.pre_edit_snapshot.take();
            editor_state.commit_snapshot(snapshot);
        }
        sync_panel_hover(
            &mut editor_state.color_popover,
            hovered,
            &mut editor_state.damage_rects,
            dirty_mask,
        );
    } else if !editor_state.color_popover.fields.is_editing()
        && !editor_state.color_popover.sv_square.dragging
        && !editor_state.color_popover.hue_dragging
    {
        let snapshot = editor_state.color_popover.pre_edit_snapshot.take();
        editor_state.commit_snapshot(snapshot);
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
    if cp.eyedropper_hit(local) {
        return Some(ColorPopoverElement::Eyedropper);
    }
    if cp.hex_field_hit(local) {
        return Some(ColorPopoverElement::Field(ColorField::Hex));
    }
    if let Some(field) = cp.rgba_field_hit(local) {
        return Some(ColorPopoverElement::Field(field));
    }
    None
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
    record_undo: bool,
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
    apply_color_selection(editor_state, color, record_undo, dirty_mask);
    true
}

fn live_apply_color_field(editor_state: &mut EditorState, field: ColorField, dirty_mask: &mut u32) {
    let Some(text) = editor_state
        .color_popover
        .fields
        .editing
        .as_ref()
        .filter(|e| e.key == field)
        .map(|e| e.field.text.clone())
    else {
        return;
    };

    apply_color_field_text(editor_state, field, &text, false, false, dirty_mask);
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

    editor_state.color_popover.pre_edit_snapshot = Some(editor_state.annotations.clone());

    let current_text = editor_state.color_popover.field_text(field);

    let click_pos = (local.0 as f32, local.1 as f32);
    let is_double_click = editor_state
        .input
        .clicks
        .register(ClickTarget::ColorField(field), click_pos);

    let cursor_init = if is_double_click {
        CursorInit::SelectAll
    } else if let Some(rect) = editor_state.color_popover.field_rect(field) {
        let click_x = local.0 as f32 - rect.left();
        let idx = char_index_for_x(
            &current_text,
            click_x,
            field_font_size(),
            &mut editor_state.text.font_system,
        );
        CursorInit::At(idx)
    } else {
        CursorInit::End
    };

    editor_state
        .color_popover
        .fields
        .begin_edit(field, current_text, cursor_init);
    damage_color_popover(editor_state, dirty_mask);
}

pub fn commit_color_field_edit(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let snapshot = editor_state.color_popover.pre_edit_snapshot.take();

    let Some((field, text)) = editor_state.color_popover.fields.commit_edit() else {
        return;
    };

    apply_color_field_text(editor_state, field, &text, true, false, dirty_mask);

    if let Some(snapshot) = snapshot
        && editor_state.annotations != snapshot
    {
        editor_state.undo_stack.push(snapshot);
        editor_state.redo_stack.clear();
    }

    editor_state.color_popover.sync_field_values();
    damage_color_popover(editor_state, dirty_mask);
}

pub fn handle_color_field_text_input(
    editor_state: &mut EditorState,
    ch: char,
    dirty_mask: &mut u32,
) {
    let Some(field) = editor_state
        .color_popover
        .fields
        .editing
        .as_ref()
        .map(|e| e.key)
    else {
        return;
    };
    let allowed = field_char_filter(field);
    if editor_state.color_popover.fields.insert_char(ch, allowed) {
        damage_color_popover(editor_state, dirty_mask);
        live_apply_color_field(editor_state, field, dirty_mask);
    }
}

pub fn handle_color_field_key_press(
    editor_state: &mut EditorState,
    key: SpecialKey,
    dirty_mask: &mut u32,
) {
    let Some(field) = editor_state
        .color_popover
        .fields
        .editing
        .as_ref()
        .map(|e| e.key)
    else {
        return;
    };
    let allowed = field_char_filter(field);
    let ctrl = editor_state.input.ctrl;
    let shift = editor_state.input.shift;

    let (changed, commit) = editor_state
        .color_popover
        .fields
        .handle_key(key, ctrl, shift, allowed);

    if changed {
        damage_color_popover(editor_state, dirty_mask);
        live_apply_color_field(editor_state, field, dirty_mask);
    }

    if commit {
        commit_color_field_edit(editor_state, dirty_mask);
    }
}

pub fn step_color_field(
    editor_state: &mut EditorState,
    field: ColorField,
    steps: i32,
    dirty_mask: &mut u32,
) {
    if steps == 0 {
        return;
    }

    if editor_state.color_popover.pre_edit_snapshot.is_none() {
        editor_state.color_popover.pre_edit_snapshot = Some(editor_state.annotations.clone());
    }

    let current_text = editor_state.color_popover.confirmed_field_text(field);

    let new_text = match field {
        ColorField::Hex => step_hex_text(&current_text, steps),
        ColorField::R | ColorField::G | ColorField::B | ColorField::A => {
            let current: i32 = current_text.trim().parse().unwrap_or(0);
            (current + steps).clamp(0, 255).to_string()
        }
    };

    if apply_color_field_text(editor_state, field, &new_text, true, false, dirty_mask) {
        damage_color_popover(editor_state, dirty_mask);

        editor_state
            .color_popover
            .fields
            .set_editing_text(field, new_text);
    }
}

pub fn handle_color_field_scroll(
    editor_state: &mut EditorState,
    field: ColorField,
    delta_y: f32,
    dirty_mask: &mut u32,
) {
    let steps = editor_state.color_popover.scroll_step(
        field,
        delta_y / crate::config::get().input.scroll_step_pixels,
    );

    step_color_field(editor_state, field, steps, dirty_mask);
}

pub fn handle_color_popover_click(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let local = editor_state.input.pointer.local;

    if editor_state.pick_once && !editor_state.color_popover.eyedropper_hit(local) {
        crate::tools::eyedropper::end_pick_once(editor_state, dirty_mask);
    }

    if editor_state.color_popover.sv_square_hit(local) {
        editor_state.color_popover.pre_edit_snapshot = Some(editor_state.annotations.clone());
        editor_state.color_popover.sv_square.dragging = true;
        editor_state.color_popover.set_sv_from_local(local);
        editor_state.color_popover.sync_field_values();
        damage_color_popover(editor_state, dirty_mask);

        let color = editor_state.color_popover.sv_square.color();
        apply_color_selection(editor_state, color, false, dirty_mask);
        return;
    }

    if editor_state.color_popover.hue_slider_hit(local) {
        editor_state.color_popover.pre_edit_snapshot = Some(editor_state.annotations.clone());
        editor_state.color_popover.hue_dragging = true;
        editor_state.color_popover.set_hue_from_local(local);
        editor_state.color_popover.sync_field_values();
        damage_color_popover(editor_state, dirty_mask);

        let color = editor_state.color_popover.sv_square.color();
        apply_color_selection(editor_state, color, false, dirty_mask);
        return;
    }

    if editor_state.color_popover.eyedropper_hit(local) {
        crate::tools::eyedropper::toggle_pick_once(editor_state, dirty_mask);
        return;
    }

    if let Some(idx) = editor_state.color_popover.swatch_hit(local) {
        if let Some(color) = editor_state.color_popover.palette().get(idx).copied() {
            use_color(editor_state, color, dirty_mask);
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
    let local = editor_state.input.pointer.local;

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

    damage_color_popover(editor_state, dirty_mask);

    let color = editor_state.color_popover.sv_square.color();
    apply_color_selection(editor_state, color, false, dirty_mask);
}

pub fn handle_color_popover_release(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let was_dragging =
        editor_state.color_popover.sv_square.dragging || editor_state.color_popover.hue_dragging;

    editor_state.color_popover.sv_square.dragging = false;
    editor_state.color_popover.hue_dragging = false;

    if !was_dragging {
        return;
    }

    let snapshot = editor_state.color_popover.pre_edit_snapshot.take();
    if let Some(snapshot) = snapshot
        && editor_state.annotations != snapshot
    {
        editor_state.undo_stack.push(snapshot);
        editor_state.redo_stack.clear();
    }

    let color = editor_state.color_popover.sv_square.color();
    editor_state.color_popover.record_used_color(color);
    damage_color_popover(editor_state, dirty_mask);
}

pub fn close_color_popover(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    if editor_state.color_popover.fields.is_editing() {
        commit_color_field_edit(editor_state, dirty_mask);
    }
    if let Some(snapshot) = editor_state.color_popover.pre_edit_snapshot.take()
        && editor_state.annotations != snapshot
    {
        editor_state.undo_stack.push(snapshot);
        editor_state.redo_stack.clear();
    }
    editor_state.color_popover.open = false;
    update_color_popover(editor_state, dirty_mask);
}
