// toolbar's animation and positioning logic 

use crate::editor::EditorState;
use crate::tools::selection::global_selection_to_local;
use crate::types::toolbar::{TOOLBAR_OFFSET, TOOLBAR_TRANSITION_OFFSET, ToolbarPlacementKind};
use crate::types::UiPanel;
use crate::types::panel::{sync_panel_rect, sync_panel_hover};

use tiny_skia::Rect;

pub fn update_toolbar(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let old_monitor = editor_state.toolbar.monitor_idx;
    let old_position = editor_state.toolbar.position;
    let old_rect = editor_state.toolbar.rect();
    let old_kind = editor_state.toolbar.placement_kind;
    let mut layout_changed = false;

    let (target_monitor, target_position, hidden) = compute_toolbar_placement(editor_state);

    let new_kind = if hidden {
        ToolbarPlacementKind::Hidden
    } else if editor_state.selection.zone.is_none() {
        ToolbarPlacementKind::Idle
    } else {
        ToolbarPlacementKind::AtSelection
    };

    let entering_idle_fresh = new_kind == ToolbarPlacementKind::Idle
        && (old_kind != Some(ToolbarPlacementKind::Idle) || old_monitor != target_monitor);

    if entering_idle_fresh && editor_state.toolbar.render_pos != (target_position.0, target_position.1)  {
        editor_state.toolbar.render_pos = (target_position.0, -editor_state.toolbar.size.1);
    } else if old_monitor != target_monitor || old_position != target_position {
        let start_pos = editor_state.toolbar.compute_transition_start(
            old_monitor,
            target_position,
            target_monitor,
            &editor_state.placements,
            TOOLBAR_TRANSITION_OFFSET,
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

    editor_state.toolbar.placement_kind = Some(new_kind);

    sync_panel_rect(
        &mut editor_state.toolbar,
        old_rect,
        old_monitor,
        layout_changed,
        &mut editor_state.damage_rects,
        dirty_mask,
    );

    if editor_state.toolbar.rect().is_some() {
        let hovered = editor_state.toolbar.hit_test(editor_state.pointer.local);
        sync_panel_hover(&mut editor_state.toolbar, hovered, &mut editor_state.damage_rects, dirty_mask);
    }
}

fn compute_toolbar_placement(editor_state: &EditorState) -> (usize, (f32, f32), bool) {
    if editor_state.tool_active {
        return (editor_state.toolbar.monitor_idx, editor_state.toolbar.position, true);
    }

    let Some(sel) = editor_state.selection.zone else {
        let monitor_idx = editor_state.pointer.monitor_idx;
        let placement = &editor_state.placements[monitor_idx];
        let mon_w = placement.size.0 as f32;
        let pos = ((mon_w - editor_state.toolbar.size.0) / 2.0, TOOLBAR_OFFSET);
        return (monitor_idx, pos, false);
    };

    let monitor_idx = editor_state.pointer.monitor_idx;
    let placement = &editor_state.placements[monitor_idx];

    let local_sel = global_selection_to_local(&sel, placement).unwrap_or_else(|| {
        Rect::from_xywh(0.0, 0.0, placement.size.0 as f32, placement.size.1 as f32).unwrap()
    });

    let tb_w = editor_state.toolbar.size.0;
    let tb_h = editor_state.toolbar.size.1;
    let margin = TOOLBAR_OFFSET;

    let sel_center_x = (local_sel.left() + local_sel.right()) / 2.0;
    let pos_x = (sel_center_x - tb_w / 2.0)
        .clamp(0.0, (placement.size.0 as f32 - tb_w).max(0.0));

    let above_y = local_sel.top() - tb_h - margin;
    let pos_y = if above_y >= 0.0 {
        above_y
    } else {
        local_sel.top() + margin
    };

    (monitor_idx, (pos_x, pos_y), false)
}