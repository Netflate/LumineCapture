use tiny_skia::Rect;
// will rename to ui, and put here everything related to panels like toolbars and stuff 
pub trait PanelItem {
    fn size(&self) -> f32;
    fn trailing_padding(&self) -> f32;
    fn is_button(&self) -> bool;
}

pub trait UiPanel {
    type Item: PanelItem;

    fn render_pos(&self) -> (f32, f32);
    fn size(&self) -> (f32, f32);
    fn items(&self) -> &[Self::Item];
    fn padding(&self) -> f32;

    fn rect(&self) -> Option<Rect> {
        let (x, y) = self.render_pos();
        let (w, h) = self.size();
        Rect::from_xywh(x, y, w, h)
    }

    fn width(&self) -> f32 {
        let mut total = self.padding() * 2.0;
        for item in self.items() {
            total += item.size() + item.trailing_padding();
        }
        total
    }
}