//! Логика раскладки и анимации Toolbar (позиционирование относительно
//! выделения/монитора, hover, opacity/позиция при появлении и
//! скрытии). Отрисовка пикселей — в renderer/toolbar.rs, это разные
//! слои и здесь им не место.

use crate::editor::{EditorState, DamageZone};
use crate::tools::selection::global_selection_to_local;
use crate::types::toolbar::{
    TOOLBAR_ANIM_DT, TOOLBAR_ANIM_INTERVAL, TOOLBAR_OFFSET, TOOLBAR_TRANSITION_OFFSET,
    ToolbarPlacementKind
};
use crate::types::UiPanel;
use crate::editor::dirty::mark_dirty;

use std::time::Instant;
use tiny_skia::Rect;

pub fn update_toolbar(editor_state: &mut EditorState) {
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

    let new_rect = editor_state.toolbar.rect();
    let new_monitor = editor_state.toolbar.monitor_idx;

    if layout_changed || old_rect != new_rect || old_monitor != new_monitor {
        editor_state.toolbar.dirty = true;
        if let Some(rect) = old_rect {
            editor_state.damage_rects.push(DamageZone::Local { monitor_idx: old_monitor, rect });
        }
        if let Some(rect) = new_rect {
            editor_state.damage_rects.push(DamageZone::Local { monitor_idx: new_monitor, rect });
        }
    }

    if let Some(tb_rect) = new_rect {
        let button = editor_state.toolbar.hit_test(editor_state.pointer.local);
        if button != editor_state.toolbar.hovered {
            editor_state.toolbar.dirty = true;
            editor_state.toolbar.hovered = button;
            editor_state.damage_rects.push(DamageZone::Local { monitor_idx: new_monitor, rect: tb_rect });
        }
    }
}

pub fn tick_toolbar_anim(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let now = Instant::now();
    let tb = &mut editor_state.toolbar;

    let elapsed = tb.last_tick
        .map(|t| now.duration_since(t))
        .unwrap_or(TOOLBAR_ANIM_INTERVAL);

    if elapsed < TOOLBAR_ANIM_INTERVAL {
        return;
    }

    let steps = ((elapsed.as_secs_f32() / TOOLBAR_ANIM_DT).floor() as u32).clamp(1, 4);
    tb.last_tick = Some(now);

    let old_render_pos = tb.render_pos;
    let mut changed = false;

    for _ in 0..steps {
        let target_opacity = if tb.interferes { 0.0 } else { 1.0 };
        if (tb.opacity - target_opacity).abs() > 0.001 {
            let delta = 5.0 * TOOLBAR_ANIM_DT;
            tb.opacity += (target_opacity - tb.opacity).signum() * delta;
            tb.opacity = tb.opacity.clamp(0.0, 1.0);
            changed = true;
        }

        let target = tb.position;
        let dx = target.0 - tb.render_pos.0;
        let dy = target.1 - tb.render_pos.1;
        if dx.abs() > 0.5 || dy.abs() > 0.5 {
            let t = (12.0 * TOOLBAR_ANIM_DT).min(1.0);
            tb.render_pos.0 += dx * t;
            tb.render_pos.1 += dy * t;
            changed = true;
        } else if tb.render_pos != target {
            tb.render_pos = target;
            changed = true;
        }
    }

    if changed {
        tb.dirty = true;
        let monitor_idx = tb.monitor_idx;
        mark_dirty(dirty_mask, monitor_idx);

        let old_rect = tiny_skia::Rect::from_xywh(old_render_pos.0, old_render_pos.1, tb.size.0, tb.size.1);
        let new_rect = tb.rect();

        let union = match (old_rect, new_rect) {
            (Some(a), Some(b)) => tiny_skia::Rect::from_ltrb(
                a.left().min(b.left()), a.top().min(b.top()),
                a.right().max(b.right()), a.bottom().max(b.bottom()),
            ),
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        };

        if let Some(rect) = union {
            editor_state.damage_rects.push(DamageZone::Local { monitor_idx, rect });
        }
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