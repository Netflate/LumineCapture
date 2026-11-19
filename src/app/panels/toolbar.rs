// toolbar's animation and positioning logic

use crate::editor::EditorState;
use crate::theme::size;
use crate::ui::panel::UiPanel;
use crate::ui::panel::{sync_panel_hover, sync_panel_rect};
use crate::ui::toolbar::ToolbarPlacementKind;
use tiny_skia::Rect;

const MIN_SHRINK: f32 = 0.5;

fn fit_toolbar(editor_state: &EditorState) -> f32 {
    if editor_state.sheet.is_none() {
        return 1.0;
    }
    let room = editor_state.placements[0].size.0 as f32 - size::margin() * 2.0;
    (room / editor_state.toolbar.width()).clamp(MIN_SHRINK, 1.0)
}

pub fn update_toolbar(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let old_monitor = editor_state.toolbar.monitor_idx;
    let old_position = editor_state.toolbar.position;
    let old_rect = editor_state.toolbar.rect();
    let old_kind = editor_state.toolbar.placement_kind;
    let mut layout_changed = false;

    let fit = fit_toolbar(editor_state);
    if editor_state.toolbar.shrink != fit {
        editor_state.toolbar.set_shrink(fit);
        layout_changed = true;
    }

    let (target_monitor, target_position, hidden) = compute_toolbar_placement(editor_state);

    let new_kind = if hidden {
        ToolbarPlacementKind::Hidden
    } else if editor_state.selection.zone.is_none() || editor_state.sheet.is_some() {
        ToolbarPlacementKind::Idle
    } else {
        ToolbarPlacementKind::AtSelection
    };

    let entering_idle_fresh = new_kind == ToolbarPlacementKind::Idle
        && (old_kind != Some(ToolbarPlacementKind::Idle) || old_monitor != target_monitor);

    if entering_idle_fresh
        && editor_state.toolbar.render_pos != (target_position.0, target_position.1)
    {
        editor_state.toolbar.render_pos = (target_position.0, -editor_state.toolbar.size.1);
    } else if old_monitor != target_monitor || old_position != target_position {
        let start_pos = editor_state.toolbar.compute_transition_start(
            old_monitor,
            target_position,
            target_monitor,
            &editor_state.placements,
            crate::config::get().animation.toolbar_slide_distance,
        );
        editor_state.toolbar.render_pos = start_pos;
    }

    if old_monitor != target_monitor || old_position != target_position {
        editor_state.toolbar.monitor_idx = target_monitor;
        editor_state.toolbar.position = target_position;
        layout_changed = true;
    }
    if editor_state.toolbar.interferes != hidden {
        editor_state.toolbar.interferes = hidden;
        layout_changed = true;
    }
    if editor_state.toolbar.dirty {
        layout_changed = true;
    }

    editor_state.toolbar.placement_kind = Some(new_kind);

    if hidden && editor_state.scroll.is_some() {
        editor_state.toolbar.opacity = 0.0;
    }

    sync_panel_rect(
        &mut editor_state.toolbar,
        old_rect,
        old_monitor,
        layout_changed,
        &mut editor_state.damage_rects,
        dirty_mask,
    );

    if editor_state.toolbar.rect().is_some() {
        let (_, hovered) = editor_state
            .toolbar
            .hit_test(editor_state.input.pointer.local);
        sync_panel_hover(
            &mut editor_state.toolbar,
            hovered,
            &mut editor_state.damage_rects,
            dirty_mask,
        );
    }
}

pub fn min_window_size(toolbar: (f32, f32), client_frame: bool) -> (u32, u32) {
    let margin = size::margin();
    (
        (toolbar.0 + margin * 2.0).ceil() as u32,
        // header + two toolbars of the picture under it
        (toolbar.1 * 3.0
            + crate::ui::settings_panel::height()
            + if client_frame {
                crate::ui::window_controls::TITLE_HEIGHT
            } else {
                0.0
            })
        .ceil() as u32,
    )
}

pub fn docked_inset(editor_state: &EditorState) -> f32 {
    let scale = editor_state.placements[0].scale;
    let toolbar = editor_state.toolbar.natural_size().1 * fit_toolbar(editor_state);
    // panel starts where the toolbar's rounded edge does
    (toolbar * scale).round()
        + if editor_state.frame.client.is_some() {
            (crate::ui::window_controls::TITLE_HEIGHT * scale).round()
        } else {
            0.0
        }
        + (crate::ui::settings_panel::height() * scale).ceil()
}

fn compute_toolbar_placement(editor_state: &EditorState) -> (usize, (f32, f32), bool) {
    // in the window edit mode it stays on top while drawing, it's never in the way there
    let window = editor_state.sheet.is_some();
    if (editor_state.tool_active && !window) || editor_state.ui_hidden {
        return (
            editor_state.toolbar.monitor_idx,
            editor_state.toolbar.position,
            true,
        );
    }

    let zone = editor_state.selection.zone.filter(|_| !window);
    let Some(sel) = zone else {
        let monitor_idx = editor_state.input.pointer.monitor_idx;
        let placement = &editor_state.placements[monitor_idx];
        let mon_w = placement.size.0 as f32;
        let top = if editor_state.toolbar.docked && editor_state.frame.client.is_some() {
            crate::ui::window_controls::TITLE_HEIGHT
        } else if editor_state.toolbar.docked {
            0.0
        } else {
            size::margin()
        };
        let pos = ((mon_w - editor_state.toolbar.size.0) / 2.0, top);
        return (monitor_idx, pos, false);
    };

    let monitor_idx = match &editor_state.scroll {
        Some(scroll) => scroll.monitor_idx,
        None => editor_state.input.pointer.monitor_idx,
    };
    let placement = &editor_state.placements[monitor_idx];

    let local_sel = editor_state
        .selection_on(monitor_idx, &sel)
        .unwrap_or_else(|| {
            Rect::from_xywh(0.0, 0.0, placement.size.0 as f32, placement.size.1 as f32).unwrap()
        });

    let tb_w = editor_state.toolbar.size.0;
    let tb_h = editor_state.toolbar.size.1;
    let margin = size::margin();

    let sel_center_x = (local_sel.left() + local_sel.right()) / 2.0;
    let pos_x = (sel_center_x - tb_w / 2.0).clamp(0.0, (placement.size.0 as f32 - tb_w).max(0.0));

    let above_y = local_sel.top() - tb_h - margin;
    let pos_y = if above_y >= 0.0 {
        above_y
    } else {
        local_sel.top() + margin
    };

    // inside the live region it would end up in the frames
    let in_frames = editor_state.scroll.as_ref().is_some_and(|scroll| {
        scroll.grown()
            || Rect::from_xywh(pos_x, pos_y, tb_w, tb_h).is_some_and(|rect| {
                crate::utils::rects_overlap(&rect, &scroll.region)
                    || crate::utils::rects_overlap(&rect, &scroll.layout.rect)
            })
    });

    (monitor_idx, (pos_x, pos_y), in_frames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::testing::editor;
    use crate::tools::Tool;
    use crate::ui::settings_panel::height;

    fn window(width: i32) -> EditorState {
        let mut state = editor(true);
        state.placements[0].size = (width, 600);
        state.toolbar.docked = true;
        state.toolbar.set_items(crate::ui::toolbar::WINDOW_ITEMS);
        state
    }

    #[test]
    fn the_header_is_the_toolbar_and_a_row_for_the_settings() {
        let state = window(1600);
        let toolbar = state.toolbar.natural_size().1;
        assert_eq!(docked_inset(&state), toolbar + height());
    }

    #[test]
    fn a_shrunk_toolbar_leaves_the_settings_row_whole() {
        let mut state = window(300);
        let fit = fit_toolbar(&state);
        assert!(fit < 1.0);
        let toolbar = state.toolbar.natural_size().1 * fit;
        assert_eq!(docked_inset(&state), toolbar.round() + height());
        state.placements[0].scale = 2.0;
        assert_eq!(
            docked_inset(&state),
            (toolbar * 2.0).round() + height() * 2.0
        );
    }

    #[test]
    fn the_panel_ends_where_the_picture_begins_for_every_tool() {
        let mut state = window(1600);
        let inset = docked_inset(&state);
        let natural = state.toolbar.natural_size().1;
        for tool in [Tool::Selection, Tool::Pen, Tool::Text, Tool::Ocr] {
            state.selected_tool = tool;
            update_toolbar(&mut state, &mut 0);
            state.toolbar.render_pos = state.toolbar.position;
            crate::app::panels::settings::update_settings_panel(&mut state, &mut 0);
            let panel = &state.settings_panel;
            if panel.visible {
                assert_eq!(panel.render_pos.1 + panel.size.1, inset, "{tool:?}");
                assert_eq!(panel.size.1, height());
            }
            assert_eq!(docked_inset(&state), natural + height(), "{tool:?}");
        }
    }

    #[test]
    fn the_window_is_never_smaller_than_the_header_and_some_picture() {
        let toolbar = (600.0, 42.0);
        let (_, h) = min_window_size(toolbar, false);
        assert_eq!(h as f32, toolbar.1 * 3.0 + height());
    }
}
