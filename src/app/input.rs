// Processing of pointer and keyboard events: translating 
// coordinates into local/global, dispatching to the active tool,

// priority hits in Toolbar/SettingsPanel. logic of the Toolbar/SettingsPanel
// (what to show, where to commit changes) is in toolbar_logic.rs / settings_logic.rs,
// only event routing and update_pointer/update_magnifier as their direct side effects

use crate::editor::{EditorState, DamageZone};
use crate::renderer::char_index_for_x;
use crate::tools::{
    Tool, dispatch_button, dispatch_deactivate, dispatch_key, dispatch_move, dispatch_text,
};
use crate::types::panel::UiPanel;
use crate::types::toolbar::{ToolbarButton, ToolbarItem};
use crate::types::{
    MAG_FRAME_INTERVAL, MagnifierState, MouseButton, PointerState, SettingsWidget,
    SpecialKey, ArrowHoldState,
};
use crate::types::click::ClickTarget;
use crate::types::text_field::CursorInit;
use crate::types::settings_panel::SETTINGS_LABEL_FONT_SIZE;
use crate::utils::{get_full_workspace_rect, global_point_to_local};
use crate::editor::dirty::{mark_dirty, apply_damage_rects};

use std::time::Instant;

use super::toolbar_logic::update_toolbar;
use super::settings_logic::{
    apply_stepper_arrow_step, commit_stepper_text_edit, handle_settings_key_press,
    handle_settings_text_input, apply_toggle_field,
};

pub fn handle_pointer_move(
    editor_state: &mut EditorState,
    monitor_idx: usize,
    x: f64,
    y: f64,
    dirty_mask: &mut u32,
) {
    let global = (
        editor_state.placements[monitor_idx].position.0 as f64 + x,
        editor_state.placements[monitor_idx].position.1 as f64 + y,
    );
    let (current_monitor_idx, local_x, local_y) =
        global_point_to_local(&editor_state.placements, global, monitor_idx, (x, y));
    update_pointer(editor_state, current_monitor_idx, (local_x, local_y), global);
    update_magnifier(editor_state, dirty_mask);
    dispatch_move(editor_state.selected_tool, editor_state, global, dirty_mask);
    update_toolbar(editor_state, dirty_mask);
    apply_damage_rects(editor_state, dirty_mask);
}

pub fn handle_pointer_button(
    editor_state: &mut EditorState,
    button: MouseButton,
    pressed: bool,
    dirty_mask: &mut u32,
) {
    let is_left_click_pressed = matches!(button, MouseButton::Left) && pressed;

    // releasing left click stops stepper repeat or acceleration if it was started (click + hold)
    if matches!(button, MouseButton::Left) && !pressed {
        editor_state.settings_panel.arrow_held = None;
    }
    // any left click anywhere first closes current stepper editing (commits value)
    if is_left_click_pressed && editor_state.settings_panel.is_editing() {
        commit_stepper_text_edit(editor_state, dirty_mask);
    }

    // ── 1. first testing toolbar hit test ─────────────────────────────────────────────────────────────

    let hit_toolbar = if is_left_click_pressed {
        editor_state.toolbar.hit_test(editor_state.pointer.local)
    } else {
        None
    };

    if let Some(tb_button) = hit_toolbar {
        editor_state.toolbar.dirty = true;
        editor_state.settings_panel.dirty = true;
        editor_state.settings_panel.selected = None;
        let monitor_idx = editor_state.toolbar.monitor_idx;

        if let Some(rect) = editor_state.settings_panel.rect() {
            editor_state.damage_rects.push(DamageZone::Local { monitor_idx, rect });
        }
        mark_dirty(dirty_mask, monitor_idx);

        if let Some(ToolbarItem::Button(btn)) = editor_state.toolbar.items.get(tb_button) {
            match btn {
                ToolbarButton::Tool(tool) => {
                    if editor_state.selection.zone.is_none() && *tool != Tool::Selection {
                        editor_state.selection.zone = get_full_workspace_rect(&editor_state.placements);
                        for i in 0..editor_state.placements.len() {
                            mark_dirty(dirty_mask, i);
                        }
                    } else if *tool == Tool::Selection
                        && editor_state.selection.zone == get_full_workspace_rect(&editor_state.placements)
                    {
                        editor_state.selection.zone = None;
                        for i in 0..editor_state.placements.len() {
                            mark_dirty(dirty_mask, i);
                        }
                    }
                    dispatch_deactivate(editor_state.selected_tool, editor_state, dirty_mask);
                    editor_state.selected_tool = *tool;
                    editor_state.toolbar.selected = Some(tb_button);
                }
            }
            editor_state.toolbar.dirty = true;
            update_toolbar(editor_state, dirty_mask);
            apply_damage_rects(editor_state, dirty_mask);
        }
        return;
    }
    // ── 2. settings panel hit test ─────────────────────────────────────────────────────────────

    let hit_settings = if is_left_click_pressed && editor_state.settings_panel.visible {
        editor_state.settings_panel.hit_test(editor_state.pointer.local)
    } else {
        None
    };

    if let Some(widget_idx) = hit_settings {
        let monitor_idx = editor_state.settings_panel.monitor_idx;

        editor_state.settings_panel.dirty = true;
        editor_state.settings_panel.selected = Some(widget_idx);

        println!("hit settings widget {widget_idx:?} at {:?}", editor_state.pointer.local);
        match editor_state.settings_panel.widgets[widget_idx] {
            SettingsWidget::Stepper { .. } => {
                if let Some(arrow) = editor_state.settings_panel.stepper_arrow_hit(widget_idx, editor_state.pointer.local) {
                    apply_stepper_arrow_step(editor_state, widget_idx, arrow, dirty_mask);
                    editor_state.settings_panel.arrow_held = Some(ArrowHoldState {
                        widget_idx,
                        arrow,
                        started_at: Instant::now(),
                        last_step_at: Instant::now(),
                        repeat_count: 0,
                    });
                } else {
                    let click_pos = (
                        editor_state.pointer.local.0 as f32,
                        editor_state.pointer.local.1 as f32,
                    );
                    let is_double_click = editor_state
                        .click_tracker
                        .register(ClickTarget::SettingsWidget(widget_idx), click_pos);

                    let current_value = editor_state
                        .settings_panel
                        .values
                        .get(&widget_idx)
                        .cloned()
                        .unwrap_or_default();

                    let cursor_init = if is_double_click {
                        CursorInit::SelectAll
                    } else if let Some(text_x) = editor_state.settings_panel.widget_text_x(widget_idx) {
                        let click_x = editor_state.pointer.local.0 as f32 - text_x;
                        let idx = char_index_for_x(
                            &current_value,
                            click_x,
                            SETTINGS_LABEL_FONT_SIZE,
                            &mut editor_state.font_system,
                        );
                        CursorInit::At(idx)
                    } else {
                        CursorInit::End
                    };

                    editor_state.settings_panel.begin_edit(widget_idx, current_value, cursor_init);
                }
            }
            SettingsWidget::Toggle { field, .. } => {
                let new_val = editor_state.settings_panel.toggle(widget_idx);
                apply_toggle_field(editor_state, field, new_val, dirty_mask);
            }
            _ => {}

        }

        mark_dirty(dirty_mask, monitor_idx);

        if let Some(rect) = editor_state.settings_panel.rect() {
            editor_state.damage_rects.push(DamageZone::Local { monitor_idx, rect });
        }

        apply_damage_rects(editor_state, dirty_mask);
        return;
    }

    // ── 3. if not anything related to ui, then dispatching event to tool ─────────────────────────────────────────────────────────────

    if is_left_click_pressed {
        editor_state.settings_panel.dirty = true;
        editor_state.settings_panel.selected = None;
        let monitor_idx = editor_state.toolbar.monitor_idx;

        if let Some(rect) = editor_state.settings_panel.rect() {
            editor_state.damage_rects.push(DamageZone::Local { monitor_idx, rect });
        }
        mark_dirty(dirty_mask, monitor_idx);
    }

    dispatch_button(editor_state.selected_tool, editor_state, button, pressed, dirty_mask);

    if matches!(button, MouseButton::Left) && !pressed {
        update_toolbar(editor_state, dirty_mask);
    }

    apply_damage_rects(editor_state, dirty_mask);
}

fn update_pointer(
    editor_state: &mut EditorState,
    monitor_idx: usize,
    local: (f64, f64),
    global: (f64, f64),
) {
    editor_state.pointer = PointerState::new(monitor_idx, local, global);
}

fn update_magnifier(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let now = Instant::now();
    if let Some(last) = editor_state.last_mag_update
        && now.duration_since(last) < MAG_FRAME_INTERVAL
    {
        return;
    }
    editor_state.last_mag_update = Some(now);

    let monitor_idx = editor_state.pointer.monitor_idx;
    let local = editor_state.pointer.local;

    if let Some(mag) = editor_state.magnifier.as_ref() {
        if mag.monitor_idx != monitor_idx {
            mark_dirty(dirty_mask, mag.monitor_idx);
        }
        editor_state.prev_magnifier = Some(MagnifierState {
            monitor_idx: mag.monitor_idx,
            pos: mag.pos,
        });
    } else {
        editor_state.prev_magnifier = None;
    }

    editor_state.magnifier = Some(MagnifierState { monitor_idx, pos: local });
    mark_dirty(dirty_mask, monitor_idx);
}

pub fn handle_text_input(editor_state: &mut EditorState, ch: char, dirty_mask: &mut u32) {
    if editor_state.settings_panel.is_editing() {
        handle_settings_text_input(editor_state, ch, dirty_mask);
        return;
    }
    dispatch_text(editor_state.selected_tool, editor_state, ch, dirty_mask);
    apply_damage_rects(editor_state, dirty_mask);
}

pub fn handle_key_press(editor_state: &mut EditorState, key: SpecialKey, dirty_mask: &mut u32) {
    if editor_state.settings_panel.is_editing() {
        handle_settings_key_press(editor_state, key, dirty_mask);
        return;
    }
    dispatch_key(editor_state.selected_tool, editor_state, key, dirty_mask);
    apply_damage_rects(editor_state, dirty_mask);
}