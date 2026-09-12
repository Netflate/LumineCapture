// Processing of pointer and keyboard events: translating
// coordinates into local/global, dispatching to the active tool,

// priority hits in Toolbar/SettingsPanel. logic of the Toolbar/SettingsPanel
// (what to show, where to commit changes) is in toolbar_logic.rs / settings_logic.rs,
// only event routing and update_pointer/update_magnifier as their direct side effects

use crate::editor::dirty::{apply_damage_rects, mark_all_dirty, mark_dirty};
use crate::editor::{DamageZone, EditorState};
use crate::ui::settings_panel::char_index_for_x;
use crate::tools::{
    Tool, dispatch_activate, dispatch_button, dispatch_cursor, dispatch_deactivate, dispatch_key,
    dispatch_move, dispatch_text,
};
use crate::interaction::ClickTarget;
use crate::ui::color_popover::ColorField;
use crate::ui::panel::UiPanel;
use crate::theme::{anim, font};
use crate::interaction::SCROLL_SENSITIVITY;
use crate::ui::text_field::CursorInit;
use crate::keys::{self, Chord, Key};
use crate::ui::toolbar::{ToolbarButton, ToolbarItem};
use crate::types::{CursorIcon, MouseButton, PointerState, SpecialKey};
use crate::ui::magnifier::MagnifierState;
use crate::ui::settings_panel::{ArrowHoldState, SettingsWidget, StepperArrow};
use crate::utils::{get_full_workspace_rect, global_point_to_local};

use std::time::Instant;

use super::panels::color::{
    close_color_popover, commit_color_field_edit, handle_color_field_key_press,
    handle_color_field_scroll, handle_color_field_text_input, handle_color_popover_click,
    handle_color_popover_drag, handle_color_popover_release, step_color_field,
    update_color_popover,
};
use super::panels::settings::{
    apply_stepper_arrow_step, apply_toggle_field, commit_stepper_text_edit,
    handle_settings_key_press, handle_settings_text_input, handle_stepper_scroll,
    run_settings_action, sync_stepper_edit_text, update_settings_panel,
};
use super::panels::model::{close_model_popover, handle_model_popover_click, update_model_popover};
use super::panels::toolbar::update_toolbar;

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
    update_pointer(
        editor_state,
        current_monitor_idx,
        (local_x, local_y),
        global,
    );
    update_magnifier(editor_state, dirty_mask);
    dispatch_move(editor_state.selected_tool, editor_state, global, dirty_mask);
    update_toolbar(editor_state, dirty_mask);
    if editor_state.settings_panel.visible {
        update_settings_panel(editor_state, dirty_mask);
    }
    if editor_state.color_popover.open {
        update_color_popover(editor_state, dirty_mask);
        handle_color_popover_drag(editor_state, dirty_mask);
    }
    if editor_state.model_popover.open {
        update_model_popover(editor_state, dirty_mask);
    }
    apply_damage_rects(editor_state, dirty_mask);
}

// ──── hit test with priority: color popover (if open) -> languages list (if open) -> toolbar -> settings ──────────────────

enum UiHit {
    ColorPopoverInside,
    ColorPopoverOutside,
    ModelPopoverInside,
    ModelPopoverOutside,
    ToolbarItem(usize),
    ToolbarBackground,
    SettingsItem(usize),
    SettingsBackground,
    None,
}

fn hit_test_ui(editor_state: &EditorState, local: (f64, f64)) -> UiHit {
    // 1. Popovers
    if editor_state.color_popover.open {
        return if editor_state.color_popover.hit_test(local) {
            UiHit::ColorPopoverInside
        } else {
            UiHit::ColorPopoverOutside
        };
    }

    if editor_state.model_popover.open {
        return if editor_state.model_popover.hit_test(local) {
            UiHit::ModelPopoverInside
        } else {
            UiHit::ModelPopoverOutside
        };
    }

    // 2. Toolbar (`(bool, Option<usize>)`)
    let (in_toolbar, tb_item) = editor_state.toolbar.hit_test(local);
    if in_toolbar {
        return match tb_item {
            Some(idx) => UiHit::ToolbarItem(idx),
            None => UiHit::ToolbarBackground,
        };
    }

    // 3. Settings Panel (`(bool, Option<usize>)`)
    if editor_state.settings_panel.visible {
        let (in_settings, widget_idx) = editor_state.settings_panel.hit_test(local);
        if in_settings {
            return match widget_idx {
                Some(idx) => UiHit::SettingsItem(idx),
                None => UiHit::SettingsBackground,
            };
        }
    }

    UiHit::None
}

pub fn compute_cursor(editor_state: &EditorState) -> CursorIcon {
    match hit_test_ui(editor_state, editor_state.input.pointer.local) {
        UiHit::ColorPopoverInside | UiHit::ToolbarItem(_) | UiHit::SettingsItem(_) => {
            CursorIcon::Pointer
        }
        UiHit::ModelPopoverInside => {
            if editor_state.model_popover.element_at(editor_state.input.pointer.local).is_some() {
                CursorIcon::Pointer
            } else {
                CursorIcon::Default
            }
        }
        UiHit::ToolbarBackground | UiHit::SettingsBackground => CursorIcon::Default,
        UiHit::ColorPopoverOutside | UiHit::ModelPopoverOutside | UiHit::None => {
            if editor_state.pick_once {
                CursorIcon::Crosshair
            } else {
                dispatch_cursor(editor_state.selected_tool, editor_state)
            }
        }
    }
}

pub fn handle_pointer_button(
    editor_state: &mut EditorState,
    button: MouseButton,
    pressed: bool,
    dirty_mask: &mut u32,
) {
    editor_state.settings_panel.cancel_scroll();
    editor_state.color_popover.cancel_scroll();

    let is_left_click_pressed = matches!(button, MouseButton::Left) && pressed;

    if matches!(button, MouseButton::Left) && !pressed {
        editor_state.settings_panel.arrow_held = None;
        handle_color_popover_release(editor_state, dirty_mask);
    }
    if is_left_click_pressed && editor_state.settings_panel.is_editing() {
        commit_stepper_text_edit(editor_state, dirty_mask);
    }
    if is_left_click_pressed && editor_state.color_popover.fields.is_editing() {
        commit_color_field_edit(editor_state, dirty_mask);
    }

    // ── 1. priority ui hit test ──────────────────────────────────────────────

    if is_left_click_pressed {
        let mut ui_hit = hit_test_ui(editor_state, editor_state.input.pointer.local);

        if let UiHit::ColorPopoverOutside = ui_hit {
            close_color_popover(editor_state, dirty_mask);
            ui_hit = hit_test_ui(editor_state, editor_state.input.pointer.local);
        }

        if let UiHit::ModelPopoverOutside = ui_hit {
            let on_toggle = languages_button_hit(editor_state);
            close_model_popover(editor_state, dirty_mask);
            if on_toggle {
                apply_damage_rects(editor_state, dirty_mask);
                return;
            }
            ui_hit = hit_test_ui(editor_state, editor_state.input.pointer.local);
        }

        match ui_hit {
            UiHit::ColorPopoverInside => {
                handle_color_popover_click(editor_state, dirty_mask);
                apply_damage_rects(editor_state, dirty_mask);
                return;
            }
            UiHit::ColorPopoverOutside => unreachable!("popover is closed at this point"),
            UiHit::ModelPopoverInside => {
                handle_model_popover_click(editor_state, dirty_mask);
                apply_damage_rects(editor_state, dirty_mask);
                return;
            }
            UiHit::ModelPopoverOutside => unreachable!("popover is closed at this point"),

            UiHit::ToolbarBackground => {
                return; 
            }

            UiHit::ToolbarItem(tb_button) => {
                editor_state.settings_panel.selected = None;

                if let Some(ToolbarItem::Button(btn)) = editor_state.toolbar.items.get(tb_button) {
                    match btn {
                        ToolbarButton::Tool(tool) => {
                            let tool = *tool;
                            select_tool(editor_state, tool, dirty_mask);
                            return;
                        }
                        ToolbarButton::Finish(finish) => {
                            editor_state.finish = Some(*finish);
                            return;
                        }
                    }
                }
                return; 
            }

            UiHit::SettingsBackground => {
                return; 
            }

            UiHit::SettingsItem(widget_idx) => {
                let monitor_idx = editor_state.settings_panel.monitor_idx;
                editor_state.settings_panel.dirty = true;
                // only exception: clicking outside of color swatch closes it, even if on the colorswatch icon itself
                if editor_state.settings_panel.selected == Some(widget_idx)
                    && matches!(
                        editor_state.settings_panel.widgets[widget_idx],
                        SettingsWidget::ColorSwatch
                    )
                {
                    editor_state.color_popover.open = false;
                    editor_state.settings_panel.selected = None;
                    mark_dirty(dirty_mask, monitor_idx);
                    apply_damage_rects(editor_state, dirty_mask);
                    return;
                }

                editor_state.settings_panel.selected = Some(widget_idx);

                match editor_state.settings_panel.widgets[widget_idx] {
                    SettingsWidget::Stepper { .. } => {
                        if let Some(arrow) = editor_state
                            .settings_panel
                            .stepper_arrow_hit(widget_idx, editor_state.input.pointer.local)
                        {
                            apply_stepper_arrow_step(editor_state, widget_idx, arrow, dirty_mask);
                            update_settings_panel(editor_state, dirty_mask);
                            editor_state.settings_panel.arrow_held = Some(ArrowHoldState {
                                widget_idx,
                                arrow,
                                started_at: Instant::now(),
                                last_step_at: Instant::now(),
                                repeat_count: 0,
                            });
                        } else {
                            let click_pos = (
                                editor_state.input.pointer.local.0 as f32,
                                editor_state.input.pointer.local.1 as f32,
                            );
                            let is_double_click = editor_state
                                .input.clicks
                                .register(ClickTarget::SettingsWidget(widget_idx), click_pos);

                            let current_value = editor_state
                                .settings_panel
                                .fields
                                .value(widget_idx)
                                .cloned()
                                .unwrap_or_default();

                            let cursor_init = if is_double_click {
                                CursorInit::SelectAll
                            } else if let Some(text_x) =
                                editor_state.settings_panel.widget_text_x(widget_idx)
                            {
                                let click_x = editor_state.input.pointer.local.0 as f32 - text_x;
                                let idx = char_index_for_x(
                                    &current_value,
                                    click_x,
                                    font::label(),
                                    &mut editor_state.text.font_system,
                                );
                                CursorInit::At(idx)
                            } else {
                                CursorInit::End
                            };

                            editor_state.settings_panel.pre_edit_snapshot =
                                Some(editor_state.annotations.clone());
                            editor_state.settings_panel.begin_edit(
                                widget_idx,
                                current_value,
                                cursor_init,
                            );
                        }
                    }
                    SettingsWidget::Toggle { field, .. } => {
                        let new_val = editor_state.settings_panel.toggle(widget_idx);
                        apply_toggle_field(editor_state, field, new_val, dirty_mask);
                    }
                    SettingsWidget::ColorSwatch => {
                        editor_state.color_popover.open = !editor_state.color_popover.open;
                    }
                    SettingsWidget::Action { action, .. } => {
                        editor_state.settings_panel.selected = None;
                        run_settings_action(editor_state, action, dirty_mask);
                    }

                    SettingsWidget::Value { field } => {
                        editor_state.settings_panel.selected = None;
                        let color = super::panels::settings::current_color(editor_state);
                        crate::tools::eyedropper::copy_value(editor_state, field, color);
                    }
                    _ => {}
                }
                if editor_state.color_popover.open {
                    update_color_popover(editor_state, dirty_mask);
                }

                mark_dirty(dirty_mask, monitor_idx);

                if let Some(rect) = editor_state.settings_panel.rect() {
                    editor_state
                        .damage_rects
                        .push(DamageZone::Local { monitor_idx, rect });
                }

                apply_damage_rects(editor_state, dirty_mask);
                return;
            }

            UiHit::None => {}
        }
    }

    // ── 2. dispatch event to active tool ─────────────────────────────────────

    if is_left_click_pressed {
        editor_state.settings_panel.dirty = true;
        editor_state.settings_panel.selected = None;
        let monitor_idx = editor_state.toolbar.monitor_idx;

        if let Some(rect) = editor_state.settings_panel.rect() {
            editor_state
                .damage_rects
                .push(DamageZone::Local { monitor_idx, rect });
        }
        mark_dirty(dirty_mask, monitor_idx);
    }

    //                                       ⬇⬇⬇⬇⬇⬇⬇⬇⬇
    if is_left_click_pressed && editor_state.pick_once {
        crate::tools::eyedropper::pick(editor_state, dirty_mask);
        apply_damage_rects(editor_state, dirty_mask);
        return;
    }

    dispatch_button(
        editor_state.selected_tool,
        editor_state,
        button,
        pressed,
        dirty_mask,
    );

    if (matches!(button, MouseButton::Left) && !pressed) || matches!(button, MouseButton::Right) {
        update_toolbar(editor_state, dirty_mask);
        update_settings_panel(editor_state, dirty_mask);
    }

    apply_damage_rects(editor_state, dirty_mask);
}

pub fn select_tool(editor_state: &mut EditorState, tool: Tool, dirty_mask: &mut u32) {
    let Some(tb_button) = editor_state.toolbar.items.iter().position(|item| {
        matches!(item, ToolbarItem::Button(ToolbarButton::Tool(t)) if *t == tool)
    }) else {
        return;
    };
    editor_state.settings_panel.selected = None;

    crate::tools::eyedropper::end_pick_once(editor_state, dirty_mask);
    // ocr falls back to the monitor under the cursor,
    // so it doesn't want a forced full-workspace one

    // TODO:  better handling  of  cross  monitor ocr
    // i think there must be a visual indicator, that 
    // ocr worked in ONE monitor, but  not  the other
    if editor_state.selection.zone.is_none()
        && tool != Tool::Selection
        && tool != Tool::Ocr
    {
        editor_state.selection.zone =
            get_full_workspace_rect(&editor_state.placements);
        mark_all_dirty(dirty_mask, editor_state.placements.len());
    } else if tool == Tool::Selection
        && editor_state.selection.zone
            == get_full_workspace_rect(&editor_state.placements)
    {
        editor_state.selection.zone = None;
        mark_all_dirty(dirty_mask, editor_state.placements.len());
    }
    dispatch_deactivate(
        editor_state.selected_tool,
        editor_state,
        dirty_mask,
    );
    editor_state.selected_tool = tool;
    editor_state.toolbar.selected = Some(tb_button);
    editor_state.toolbar.dirty = true;
    dispatch_activate(tool, editor_state, dirty_mask);

    refresh_panels(editor_state, dirty_mask);
    apply_damage_rects(editor_state, dirty_mask);
}

pub fn refresh_panels(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    update_toolbar(editor_state, dirty_mask);
    update_settings_panel(editor_state, dirty_mask);
    if editor_state.color_popover.open {
        update_color_popover(editor_state, dirty_mask);
        handle_color_popover_drag(editor_state, dirty_mask);
    }
    if editor_state.model_popover.is_visible() {
        update_model_popover(editor_state, dirty_mask);
    }
}

fn update_pointer(
    editor_state: &mut EditorState,
    monitor_idx: usize,
    local: (f64, f64),
    global: (f64, f64),
) {
    editor_state.input.pointer = PointerState::new(monitor_idx, local, global);
}

fn update_magnifier(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let now = Instant::now();
    if let Some(last) = editor_state.magnifier.last_update
        && now.duration_since(last) < anim::FRAME
    {
        return;
    }
    editor_state.magnifier.last_update = Some(now);

    let monitor_idx = editor_state.input.pointer.monitor_idx;
    let local = editor_state.input.pointer.local;

    if let Some(mag) = editor_state.magnifier.current.as_ref() {
        if mag.monitor_idx != monitor_idx {
            mark_dirty(dirty_mask, mag.monitor_idx);
        }
        editor_state.magnifier.prev = Some(MagnifierState {
            monitor_idx: mag.monitor_idx,
            pos: mag.pos,
        });
    } else {
        editor_state.magnifier.prev = None;
    }

    editor_state.magnifier.current = Some(MagnifierState {
        monitor_idx,
        pos: local,
    });
    mark_dirty(dirty_mask, monitor_idx);
}

fn handle_text_input(editor_state: &mut EditorState, ch: char, dirty_mask: &mut u32) {
    if editor_state.color_popover.fields.is_editing() {
        handle_color_field_text_input(editor_state, ch, dirty_mask);
        return;
    }
    if editor_state.settings_panel.is_editing() {
        handle_settings_text_input(editor_state, ch, dirty_mask);
        return;
    }
    dispatch_text(editor_state.selected_tool, editor_state, ch, dirty_mask);
    apply_damage_rects(editor_state, dirty_mask);
}

fn typing(editor_state: &EditorState) -> bool {
    editor_state.color_popover.fields.is_editing()
        || editor_state.settings_panel.is_editing()
        || (editor_state.selected_tool == Tool::Text && editor_state.text.editing.is_some())
}

fn special_key(chord: Chord) -> Option<SpecialKey> {
    Some(match chord.key {
        Key::Backspace => SpecialKey::Backspace,
        Key::Enter => SpecialKey::Enter,
        Key::Left => SpecialKey::Left,
        Key::Right => SpecialKey::Right,
        Key::Home => SpecialKey::Home,
        Key::End => SpecialKey::End,
        Key::Delete => SpecialKey::Delete,
        Key::Up => SpecialKey::Up,
        Key::Down => SpecialKey::Down,
        Key::Char('a') if chord.mods.ctrl => SpecialKey::KeyA,
        Key::Char('c') if chord.mods.ctrl => SpecialKey::KeyC,
        Key::Char('x') if chord.mods.ctrl => SpecialKey::KeyX,
        Key::Char('v') if chord.mods.ctrl => SpecialKey::KeyV,
        _ => return None,
    })
}

fn hovers_stepper(editor_state: &EditorState) -> bool {
    let local = editor_state.input.pointer.local;
    if editor_state.color_popover.open && hit_test_color_scroll_field(editor_state, local).is_some() {
        return true;
    }
    editor_state.settings_panel.visible
        && matches!(
            editor_state.settings_panel.hit_test(local).1
                .and_then(|idx| editor_state.settings_panel.widgets.get(idx)),
            Some(SettingsWidget::Stepper { .. })
        )
}

pub fn handle_key(
    editor_state: &mut EditorState,
    chord: Option<Chord>,
    text: Option<String>,
    dirty_mask: &mut u32,
) {
    let special = chord.and_then(special_key);
    let claimed = ((special.is_some() || text.is_some()) && typing(editor_state))
        || (matches!(special, Some(SpecialKey::Up | SpecialKey::Down)) && hovers_stepper(editor_state));
    if !claimed
        && let Some(action) = chord.and_then(|c| keys::map().lookup(c))
    {
        super::actions::run(editor_state, action, dirty_mask);
        return;
    }

    if let Some(key) = special {
        handle_key_press(editor_state, key, dirty_mask);
    } else if let Some(text) = text {
        for ch in text.chars() {
            handle_text_input(editor_state, ch, dirty_mask);
        }
    }
}

fn handle_key_press(editor_state: &mut EditorState, key: SpecialKey, dirty_mask: &mut u32) {
    // to not stack a lot of scroll events
    // any other action cancels the scroll in progress
    // so that user can do anything afterwards, wiithout waiting for the scroll to finish
    editor_state.settings_panel.cancel_scroll();
    editor_state.color_popover.cancel_scroll();

    if matches!(key, SpecialKey::Up | SpecialKey::Down) {
        let sign: i32 = if matches!(key, SpecialKey::Up) { 1 } else { -1 };

        if let Some(field) = editor_state
            .color_popover
            .fields
            .editing
            .as_ref()
            .map(|e| e.key)
        {
            step_color_field(editor_state, field, sign, dirty_mask);
            apply_damage_rects(editor_state, dirty_mask);
            return;
        }

        if let Some(widget_idx) = editor_state
            .settings_panel
            .fields
            .editing
            .as_ref()
            .map(|e| e.key)
        {
            let arrow = if sign > 0 {
                StepperArrow::Up
            } else {
                StepperArrow::Down
            };
            apply_stepper_arrow_step(editor_state, widget_idx, arrow, dirty_mask);
            update_settings_panel(editor_state, dirty_mask);
            sync_stepper_edit_text(editor_state, widget_idx);
            apply_damage_rects(editor_state, dirty_mask);
            return;
        }

        let local = editor_state.input.pointer.local;

        if editor_state.color_popover.open
            && let Some(field) = hit_test_color_scroll_field(editor_state, local)
        {
            step_color_field(editor_state, field, sign, dirty_mask);
            apply_damage_rects(editor_state, dirty_mask);
            return;
        }

        if editor_state.settings_panel.visible
            && let (_, Some(widget_idx)) = editor_state.settings_panel.hit_test(local)
            && matches!(
                editor_state.settings_panel.widgets.get(widget_idx),
                Some(SettingsWidget::Stepper { .. })
            )
        {
            let arrow = if sign > 0 {
                StepperArrow::Up
            } else {
                StepperArrow::Down
            };
            apply_stepper_arrow_step(editor_state, widget_idx, arrow, dirty_mask);
            update_settings_panel(editor_state, dirty_mask);
            apply_damage_rects(editor_state, dirty_mask);
            return;
        }
    }

    if editor_state.color_popover.fields.is_editing() {
        handle_color_field_key_press(editor_state, key, dirty_mask);
        return;
    }
    if editor_state.settings_panel.is_editing() {
        handle_settings_key_press(editor_state, key, dirty_mask);
        return;
    }
    dispatch_key(editor_state.selected_tool, editor_state, key, dirty_mask);
    apply_damage_rects(editor_state, dirty_mask);
}

// ──── scroll: hit-test first (color popover fields -> settings stepper), then apply ──────────
pub fn handle_scroll(
    editor_state: &mut EditorState,
    delta_x: f32,
    delta_y: f32,
    dirty_mask: &mut u32,
) {
    let local = editor_state.input.pointer.local;
    let delta_y = delta_y * SCROLL_SENSITIVITY;

    if editor_state.color_popover.open
        && let Some(field) = hit_test_color_scroll_field(editor_state, local) {
            handle_color_field_scroll(editor_state, field, delta_y, dirty_mask);
            apply_damage_rects(editor_state, dirty_mask);
            return;
        }

    if editor_state.settings_panel.visible
        && let (_, Some(widget_idx)) = editor_state.settings_panel.hit_test(local)
            && matches!(
                editor_state.settings_panel.widgets.get(widget_idx),
                Some(SettingsWidget::Stepper { .. })
            )
        {
            handle_stepper_scroll(editor_state, widget_idx, delta_y, dirty_mask);
            apply_damage_rects(editor_state, dirty_mask);
            return;
        }

    if matches!(hit_test_ui(editor_state, local), UiHit::None)
        && let Some(widget_idx) = editor_state
            .settings_panel
            .widgets
            .iter()
            .position(|w| matches!(w, SettingsWidget::Stepper { .. }))
    {
        handle_stepper_scroll(editor_state, widget_idx, delta_y, dirty_mask);
        let snapshot = editor_state.settings_panel.pre_edit_snapshot.take();
        editor_state.commit_snapshot(snapshot);
    }

    let _ = delta_x;
    apply_damage_rects(editor_state, dirty_mask);
}

fn hit_test_color_scroll_field(
    editor_state: &EditorState,
    local: (f64, f64),
) -> Option<ColorField> {
    if editor_state.color_popover.hex_field_hit(local) {
        return Some(ColorField::Hex);
    }
    editor_state.color_popover.rgba_field_hit(local)
}

fn languages_button_hit(editor_state: &EditorState) -> bool {
    let (_, widget_idx) = editor_state
        .settings_panel
        .hit_test(editor_state.input.pointer.local);
    matches!(
        widget_idx.and_then(|idx| editor_state.settings_panel.widgets.get(idx)),
        Some(SettingsWidget::Action {
            action: crate::ui::settings_panel::SettingsAction::OcrLanguages,
            ..
        })
    )
}