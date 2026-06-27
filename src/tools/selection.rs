use crate::tools::ToolBehavior;
use crate::types::{MouseButton, Placement, SelectionEdges, SelectionHandle};
use crate::utils::{make_rect, get_overlapping_monitors, hit_test_rect_handle};
use crate::renderer::apply_handle_drag;
use crate::editor::EditorState;
use tiny_skia::Rect;

pub struct SelectionTool;

pub fn global_selection_to_local(
    selection: &Rect,
    placement: &Placement,
) -> Option<Rect> {
    let mx = placement.position.0 as f32;
    let my = placement.position.1 as f32;
    let mw = placement.size.0 as f32;
    let mh = placement.size.1 as f32;

    let ix = selection.left().max(mx);
    let iy = selection.top().max(my);
    let ix2 = selection.right().min(mx + mw);
    let iy2 = selection.bottom().min(my + mh);

    if ix2 <= ix || iy2 <= iy {
        return None;
    }

    Rect::from_ltrb(ix - mx, iy - my, ix2 - mx, iy2 - my)
}

pub fn selection_edges_for_monitor(sel: &Rect, placement: &Placement) -> SelectionEdges {
    let (mx, my) = (placement.position.0 as f32, placement.position.1 as f32);
    let (mw, mh) = (placement.size.0 as f32, placement.size.1 as f32);
    let limit_x = mx + mw;
    let limit_y = my + mh;

    SelectionEdges {
        left: sel.left() >= mx && sel.left() < limit_x,
        right: sel.right() > mx && sel.right() <= limit_x,
        top: sel.top() >= my && sel.top() < limit_y,
        bottom: sel.bottom() > my && sel.bottom() <= limit_y,
    }
}

pub fn point_in_monitor(p: (f32, f32), placement: &Placement) -> bool {
    let (x, y) = p;
    let mx = placement.position.0 as f32;
    let my = placement.position.1 as f32;
    let mw = placement.size.0 as f32;
    let mh = placement.size.1 as f32;

    x >= mx && x < mx + mw && y >= my && y < my + mh
}

impl ToolBehavior for SelectionTool {
    fn on_button(&self, state: &mut EditorState, button: MouseButton, pressed: bool, _dirty_mask: &mut u32) {
        if !matches!(button, MouseButton::Left) {
            return;
        }

        state.mouse_down_left = pressed;
        if pressed {
            state.tool_active = true;
            let handle = state.selection.zone.as_ref()
                .map(|sel| hit_test_rect_handle(sel, state.pointer.global))
                .unwrap_or(SelectionHandle::None);

            if handle != SelectionHandle::None {
                if let Some(sel) = state.selection.zone {
                    state.selection.set_drag(handle, Some(state.pointer.global), Some(sel));
                    state.drag_start = None;
                }
            } else {
                state.selection.set_drag(SelectionHandle::None, None, None);
                state.drag_start = Some(state.pointer.global);
            }
        } else {
            state.tool_active = false;

            state.drag_start = None;
            state.selection.set_drag(SelectionHandle::None, None, None);
        }
    }

    fn on_move(&self, state: &mut EditorState, global: (f64, f64), selection_dirty: &mut bool, dirty_mask: &mut u32) {
        let old_sel = state.selection.zone;

        // handle drag (resizon_movee/move existing selection)
        if state.mouse_down_left && state.selection.active_handle != SelectionHandle::None {
            if let (Some(origin), Some(sel_start)) = (
                state.selection.drag_origin,
                state.selection.selection_at_drag_start,
            ) {
                let delta = (global.0 - origin.0, global.1 - origin.1);
                state.selection.zone = apply_handle_drag(&sel_start, state.selection.active_handle, delta);
                state.selection.prev_zone = old_sel;
                apply_selection_dirty(old_sel, state.selection.zone, &state.placements, dirty_mask, selection_dirty);
            }
            return;
        }

        // new selection drag
        if !state.mouse_down_left {
            return;
        }
        if let Some(start) = state.drag_start {
            state.selection.zone = make_rect(start, global);
            state.selection.prev_zone = old_sel;
            apply_selection_dirty(old_sel, state.selection.zone, &state.placements, dirty_mask, selection_dirty);
        }
    }
}

fn apply_selection_dirty(
    old_sel: Option<Rect>,
    new_sel: Option<Rect>,
    placements: &[crate::types::Placement],
    dirty_mask: &mut u32,
    selection_dirty: &mut bool,
) {
    *selection_dirty = true;
    if let Some(sel) = old_sel { *dirty_mask |= get_overlapping_monitors(&sel, placements); }
    if let Some(sel) = new_sel { *dirty_mask |= get_overlapping_monitors(&sel, placements); }
}