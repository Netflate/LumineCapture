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
        if matches!(button, MouseButton::Right) && pressed {
            let drawing = state.input.mouse_down && state.input.drag_start.is_some();
            if drawing {
                state.input.mouse_down = false;
                state.tool_active = false;
                state.input.drag_start = None;
            }
            if !state.input.mouse_down && state.selection.zone.take().is_some() {
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
                .map(|sel| {
                    hit_test_rect_handle(sel, state.input.pointer.global, state.pointer_zoom())
                })
                .unwrap_or(SelectionHandle::None);

            let pos = (
                state.input.pointer.global.0 as f32,
                state.input.pointer.global.1 as f32,
            );
            // double click on selection is equal to saving the screenshot
            if state.sheet.is_none()
                && handle == SelectionHandle::Move
                && state.input.clicks.register(ClickTarget::Selection, pos)
                && !state.to.is_empty()
            {
                state.input.mouse_down = false;
                state.tool_active = false;
                state.finish = Some(state.to);
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

            let fresh = state.input.drag_start.is_some();
            state.input.drag_start = None;
            state.selection.set_drag(SelectionHandle::None, None, None);
            if state.region() && fresh && state.selection.zone.is_some() {
                state.finish = Some(state.to);
            }
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
                let delta = ((global.0 - origin.0).round(), (global.1 - origin.1).round());
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
            if let (Some(sel), Some(bounds)) = (
                state.selection.zone,
                state.sheet.as_ref().and(state.workspace()),
            ) {
                let moving = state.selection.active_handle == SelectionHandle::Move;
                state.selection.zone = keep_inside(sel, bounds, moving);
            }
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
            && let Some(icon) = cursor_for_handle(
                hit_test_rect_handle(&sel, state.input.pointer.global, state.pointer_zoom()),
                false,
            )
        {
            return icon;
        }
        CursorIcon::Crosshair
    }
}

fn keep_inside(sel: Rect, bounds: Rect, moving: bool) -> Option<Rect> {
    let sel = if moving {
        let back = |lo: f32, hi: f32, min: f32, max: f32| (min - lo).max(0.0) + (max - hi).min(0.0);
        let dx = back(sel.left(), sel.right(), bounds.left(), bounds.right());
        let dy = back(sel.top(), sel.bottom(), bounds.top(), bounds.bottom());
        Rect::from_xywh(sel.x() + dx, sel.y() + dy, sel.width(), sel.height())?
    } else {
        sel
    };
    sel.intersect(&bounds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_stays_on_the_picture() {
        let bounds = Rect::from_xywh(0.0, 0.0, 100.0, 80.0).unwrap();
        let past = Rect::from_xywh(70.0, -10.0, 50.0, 30.0).unwrap();
        assert_eq!(
            keep_inside(past, bounds, true),
            Rect::from_xywh(50.0, 0.0, 50.0, 30.0)
        );
        assert_eq!(
            keep_inside(past, bounds, false),
            Rect::from_xywh(70.0, 0.0, 30.0, 20.0)
        );
        assert_eq!(keep_inside(bounds, bounds, true), Some(bounds));
    }
}
