use tiny_skia::Rect;
use crate::types::{Placement, SelectionEdges, SelectionHandle, HANDLE_RADIUS};

pub fn make_rect(a: (f64, f64), b: (f64, f64)) -> Option<Rect> {
    let x = a.0.min(b.0) as f32;
    let y = a.1.min(b.1) as f32;
    let w = (a.0 - b.0).abs() as f32;
    let h = (a.1 - b.1).abs() as f32;
    if w < 1.0 || h < 1.0 { return None; }
    Rect::from_xywh(x, y, w, h)
}

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

pub fn global_point_to_local(
    placements: &[Placement],
    global: (f64, f64),
    fallback_idx: usize,
    fallback_local: (f64, f64),
) -> (usize, f64, f64) {
    let (gx, gy) = global;
    placements
        .iter()
        .enumerate()
        .find_map(|(idx, p)| {
            let (px, py) = p.position;
            let (w, h) = p.size;
            let inside = gx >= px as f64 && gx < (px + w) as f64
                && gy >= py as f64 && gy < (py + h) as f64;
            inside.then(|| (idx, gx - px as f64, gy - py as f64))
        })
        .unwrap_or((fallback_idx, fallback_local.0, fallback_local.1))
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

pub fn selection_handle_points(sel: &Rect) -> [(f32, f32); 8] {
    [
        (sel.left(), sel.top()),
        ((sel.left() + sel.right()) / 2.0, sel.top()),
        (sel.right(), sel.top()),
        (sel.left(), (sel.top() + sel.bottom()) / 2.0),
        (sel.right(), (sel.top() + sel.bottom()) / 2.0),
        (sel.left(), sel.bottom()),
        ((sel.left() + sel.right()) / 2.0, sel.bottom()),
        (sel.right(), sel.bottom()),
    ]
}

pub fn hit_test_selection(sel: &Rect, pos: (f64, f64)) -> SelectionHandle {
    let (x, y) = pos;
    let (l, r, t, b) = (
        sel.left() as f64, sel.right() as f64,
        sel.top() as f64, sel.bottom() as f64,
    );
    let mid_x = (l + r) / 2.0;
    let mid_y = (t + b) / 2.0;

    let near = |a: f64, b: f64| (a - b).abs() < HANDLE_RADIUS;
    let on_left   = near(x, l);
    let on_right  = near(x, r);
    let on_top    = near(y, t);
    let on_bottom = near(y, b);
    let on_mid_x  = near(x, mid_x);
    let on_mid_y  = near(y, mid_y);

    match (on_left || on_right, on_top || on_bottom) {
        _ if on_left  && on_top    => SelectionHandle::TopLeft,
        _ if on_right && on_top    => SelectionHandle::TopRight,
        _ if on_left  && on_bottom => SelectionHandle::BottomLeft,
        _ if on_right && on_bottom => SelectionHandle::BottomRight,
        _ if on_mid_x && on_top    => SelectionHandle::Top,
        _ if on_mid_x && on_bottom => SelectionHandle::Bottom,
        _ if on_left  && on_mid_y  => SelectionHandle::Left,
        _ if on_right && on_mid_y  => SelectionHandle::Right,
        _ => {
            let inside = x >= l && x <= r && y >= t && y <= b;
            if inside { SelectionHandle::Move } else { SelectionHandle::None }
        }
    }
}


