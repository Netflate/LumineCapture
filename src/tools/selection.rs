use crate::editor::{EditorState, DamageZone};
use crate::tools::ToolBehavior;
use crate::types::{MouseButton, Placement, SelectionEdges, SelectionHandle};
use crate::utils::{apply_handle_drag, hit_test_rect_handle, make_rect};
use tiny_skia::Rect;

pub struct SelectionTool;

pub fn global_selection_to_local(selection: &Rect, placement: &Placement) -> Option<Rect> {
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
    fn on_button(
        &self,
        state: &mut EditorState,
        button: MouseButton,
        pressed: bool,
        _dirty_mask: &mut u32,
    ) {
        if !matches!(button, MouseButton::Left) {
            return;
        }

        state.mouse_down_left = pressed;
        if pressed {
            state.tool_active = true;
            let handle = state
                .selection
                .zone
                .as_ref()
                .map(|sel| hit_test_rect_handle(sel, state.pointer.global))
                .unwrap_or(SelectionHandle::None);

            if handle != SelectionHandle::None {
                if let Some(sel) = state.selection.zone {
                    state
                        .selection
                        .set_drag(handle, Some(state.pointer.global), Some(sel));
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
    
    fn on_move(
        &self,
        state: &mut EditorState,
        global: (f64, f64),
        _dirty_mask: &mut u32, 
    ) {
        let old_sel = state.selection.zone;
        let mut selection_changed = false;

        // handle drag (resize/move existing selection)
        if state.mouse_down_left && state.selection.active_handle != SelectionHandle::None {
            if let (Some(origin), Some(sel_start)) = (
                state.selection.drag_origin,
                state.selection.selection_at_drag_start,
            ) {
                let delta = (global.0 - origin.0, global.1 - origin.1);
                state.selection.zone =
                    apply_handle_drag(&sel_start, state.selection.active_handle, delta).to_rect();
                selection_changed = true;
            }
        } else if state.mouse_down_left {
            // new selection drag
            if let Some(start) = state.drag_start {
                state.selection.zone = make_rect(start, global);
                selection_changed = true;
            }
        }

        if selection_changed {
            if let Some(sel) = old_sel {
                state.damage_rects.push(DamageZone::Global(sel));
            }
            if let Some(sel) = state.selection.zone {
                state.damage_rects.push(DamageZone::Global(sel));
            }
        }
    }
}

