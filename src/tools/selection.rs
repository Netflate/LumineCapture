use crate::editor::dirty::mark_all_dirty;
use crate::editor::{DamageZone, EditorState};
use crate::interaction::ClickTarget;
use crate::tools::ToolBehavior;
use crate::types::{CursorIcon, MouseButton, Placement, SelectionEdges, SelectionHandle};
use crate::utils::{apply_handle_drag, cursor_for_handle, hit_test_rect_handle, make_rect};
use tiny_skia::Rect;

pub struct SelectionTool;

pub fn global_selection_to_local(selection: &Rect, placement: &Placement) -> Option<Rect> {
    let (mx, my) = placement.offset();
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
    let (mx, my) = placement.offset();
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

impl ToolBehavior for SelectionTool {
    fn on_button(
        &self,
        state: &mut EditorState,
        button: MouseButton,
        pressed: bool,
        dirty_mask: &mut u32,
    ) {
        if matches!(button, MouseButton::Right) && pressed && !state.input.mouse_down {
            if state.selection.zone.take().is_some() {
                mark_all_dirty(dirty_mask, state.placements.len());
            }
            return;
        }
        if !matches!(button, MouseButton::Left) {
            return;
        }

        state.input.mouse_down = pressed;
        if pressed {
            state.tool_active = true;
            let handle = state
                .selection
                .zone
                .as_ref()
                .map(|sel| hit_test_rect_handle(sel, state.input.pointer.global))
                .unwrap_or(SelectionHandle::None);

            let pos = (state.input.pointer.global.0 as f32, state.input.pointer.global.1 as f32);
            // double click on selection is equal to saving the screenshot
            if handle == SelectionHandle::Move
                && state.input.clicks.register(ClickTarget::Selection, pos)
                && let Some(finish) = crate::config::get().general.double_click.finish()
            {
                state.input.mouse_down = false;
                state.tool_active = false;
                state.finish = Some(finish);
                return;
            }

            if handle != SelectionHandle::None {
                if let Some(sel) = state.selection.zone {
                    state
                        .selection
                        .set_drag(handle, Some(state.input.pointer.global), Some(sel));
                    state.input.drag_start = None;
                }
            } else {
                state.selection.set_drag(SelectionHandle::None, None, None);
                state.input.drag_start = Some(state.input.pointer.global);
            }
        } else {
            state.tool_active = false;

            state.input.drag_start = None;
            state.selection.set_drag(SelectionHandle::None, None, None);
        }
    }

    fn on_move(&self, state: &mut EditorState, global: (f64, f64), _dirty_mask: &mut u32) {
        let old_sel = state.selection.zone;
        let mut selection_changed = false;

        // handle drag (resize/move existing selection)
        if state.input.mouse_down && state.selection.active_handle != SelectionHandle::None {
            if let (Some(origin), Some(sel_start)) = (
                state.selection.drag_origin,
                state.selection.selection_at_drag_start,
            ) {
                let delta = (global.0 - origin.0, global.1 - origin.1);
                state.selection.zone =
                    apply_handle_drag(&sel_start, state.selection.active_handle, delta).to_rect();
                selection_changed = true;
            }
        } else if state.input.mouse_down {
            // new selection drag
            if let Some(start) = state.input.drag_start {
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

    fn cursor(&self, state: &EditorState) -> CursorIcon {
        if state.input.mouse_down && state.selection.active_handle != SelectionHandle::None {
            return cursor_for_handle(state.selection.active_handle, true)
                .unwrap_or(CursorIcon::Crosshair);
        }
        if let Some(sel) = state.selection.zone
            && let Some(icon) = cursor_for_handle(hit_test_rect_handle(&sel, state.input.pointer.global), false)
        {
            return icon;
        }
        CursorIcon::Crosshair
    }
}
