use tiny_skia::Rect;
use crate::types::Placement;

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