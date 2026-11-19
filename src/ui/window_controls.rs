// client side frame of the editor window, drawn only when the compositor gave none
// (GNOME): a close button in the header and invisible resize edges around the window
use crate::renderer::Painter;
use crate::renderer::text::TextStyle;
use crate::theme::color;
use crate::types::{CursorIcon, Edge};
use tiny_skia::Rect;

pub const CLOSE_SIZE: f32 = 24.0;
pub const TITLE_HEIGHT: f32 = 47.0;
const ICON_SIZE: f32 = 16.0;
pub const GRAB: f32 = 12.0;
const CORNER: f32 = 24.0;

pub fn close_rect(window_width: f32) -> Option<Rect> {
    Rect::from_xywh(
        window_width - (TITLE_HEIGHT - CLOSE_SIZE) / 2.0 - CLOSE_SIZE,
        (TITLE_HEIGHT - CLOSE_SIZE) / 2.0,
        CLOSE_SIZE,
        CLOSE_SIZE,
    )
}

/// the text area between the close slots, redrawing never wipes the button
pub fn title_rect(window_width: f32) -> Option<Rect> {
    let side = (TITLE_HEIGHT - CLOSE_SIZE) / 2.0 + CLOSE_SIZE;
    Rect::from_xywh(side, 0.0, window_width - side * 2.0, TITLE_HEIGHT)
}

pub fn draw_title(p: &mut Painter, rect: Rect, text: &str) {
    p.text(
        rect,
        text,
        TextStyle::new(14.0, color::foreground().color())
            .center()
            .weight(cosmic_text::Weight::BOLD),
    );
}

pub fn contains(rect: Rect, (x, y): (f64, f64)) -> bool {
    let (x, y) = (x as f32, y as f32);
    x >= rect.left() && x < rect.right() && y >= rect.top() && y < rect.bottom()
}

pub fn edge_at(pos: (f64, f64), size: (f32, f32)) -> Option<Edge> {
    let (x, y) = (pos.0 as f32, pos.1 as f32);
    let (w, h) = size;
    let outside_x = x < 0.0 || x >= w;
    let outside_y = y < 0.0 || y >= h;
    if (!outside_x && !outside_y) || x < -GRAB || x >= w + GRAB || y < -GRAB || y >= h + GRAB {
        return None;
    }
    let top = y < 0.0 || (outside_x && y < CORNER);
    let bottom = y >= h || (outside_x && y >= h - CORNER);
    let left = x < 0.0 || (outside_y && x < CORNER);
    let right = x >= w || (outside_y && x >= w - CORNER);
    Some(match (top, bottom, left, right) {
        (true, _, true, _) => Edge::TopLeft,
        (true, _, _, true) => Edge::TopRight,
        (_, true, true, _) => Edge::BottomLeft,
        (_, true, _, true) => Edge::BottomRight,
        (true, ..) => Edge::Top,
        (_, true, ..) => Edge::Bottom,
        (.., true, _) => Edge::Left,
        (.., true) => Edge::Right,
        _ => return None,
    })
}

pub fn edge_cursor(edge: Edge) -> CursorIcon {
    match edge {
        Edge::Top | Edge::Bottom => CursorIcon::NsResize,
        Edge::Left | Edge::Right => CursorIcon::EwResize,
        Edge::TopLeft | Edge::BottomRight => CursorIcon::NwseResize,
        Edge::TopRight | Edge::BottomLeft => CursorIcon::NeswResize,
    }
}

pub fn draw_close(p: &mut Painter, rect: Rect, hovered: bool) {
    let (cx, cy) = (
        rect.left() + CLOSE_SIZE / 2.0,
        rect.top() + CLOSE_SIZE / 2.0,
    );
    let fill = color::foreground().with_alpha(if hovered { 38 } else { 26 });
    let tint = color::foreground();
    p.fill_circle(cx, cy, CLOSE_SIZE / 2.0, fill);
    p.icon(
        crate::ui::icons::CLOSE,
        ICON_SIZE,
        rect.left() + (CLOSE_SIZE - ICON_SIZE) / 2.0,
        rect.top() + (CLOSE_SIZE - ICON_SIZE) / 2.0,
        tint.usvg(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (f32, f32) = (400.0, 300.0);

    #[test]
    fn edges_corners_and_the_inside() {
        assert_eq!(edge_at((200.0, -3.0), SIZE), Some(Edge::Top));
        assert_eq!(edge_at((200.0, 303.0), SIZE), Some(Edge::Bottom));
        assert_eq!(edge_at((-3.0, 150.0), SIZE), Some(Edge::Left));
        assert_eq!(edge_at((403.0, 150.0), SIZE), Some(Edge::Right));
        assert_eq!(edge_at((-3.0, -3.0), SIZE), Some(Edge::TopLeft));
        assert_eq!(edge_at((10.0, -5.0), SIZE), Some(Edge::TopLeft));
        assert_eq!(edge_at((390.0, -5.0), SIZE), Some(Edge::TopRight));
        assert_eq!(edge_at((-5.0, 290.0), SIZE), Some(Edge::BottomLeft));
        assert_eq!(edge_at((405.0, 305.0), SIZE), Some(Edge::BottomRight));
        assert_eq!(edge_at((200.0, 150.0), SIZE), None);
        assert_eq!(edge_at((200.0, 1.0), SIZE), None);
        assert_eq!(edge_at((413.0, 150.0), SIZE), None);
    }
}
