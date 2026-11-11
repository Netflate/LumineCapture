// everything is in device pixels: 100% is one picture pixel per screen pixel,
// so a screenshot shows exactly as it was taken on its own monitor
const MAX_ZOOM: f32 = 32.0;
const MIN_ZOOM: f32 = 0.5;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    picture: (f32, f32),
    area: (f32, f32),
    inset: f32,
    pub zoom: f32,
    pub pan: (f32, f32),
    fitted: bool,
    untouched: bool,
}

impl View {
    pub fn new(picture: (u32, u32)) -> Self {
        Self {
            picture: (picture.0.max(1) as f32, picture.1.max(1) as f32),
            area: (0.0, 0.0),
            inset: 0.0,
            zoom: 1.0,
            pan: (0.0, 0.0),
            fitted: true,
            untouched: true,
        }
    }

    /// `ratio` is the new scale over the old one: the picture keeps its size on screen
    pub fn resize(&mut self, area: (u32, u32), inset: f32, ratio: f32) {
        let area = (area.0 as f32, area.1 as f32);
        if self.area == (0.0, 0.0) || self.untouched {
            self.area = area;
            self.inset = inset;
            self.fit();
            return;
        }
        let center = self.picture_point(self.center());
        self.area = area;
        self.inset = inset;
        self.zoom = if self.fitted {
            self.fit_zoom()
        } else {
            (self.zoom * ratio).clamp(self.min_zoom(), MAX_ZOOM)
        };
        let at = self.center();
        self.pan = (at.0 - center.0 * self.zoom, at.1 - center.1 * self.zoom);
        self.clamp();
    }

    pub fn fit(&mut self) {
        self.zoom = self.fit_zoom();
        self.pan = (0.0, self.inset);
        self.fitted = true;
        self.untouched = true;
        self.clamp();
    }

    pub fn zoom_at(&mut self, factor: f32, at: (f32, f32)) -> bool {
        let old = self.zoom;
        let mut zoom = (old * factor).clamp(self.min_zoom(), MAX_ZOOM);
        // 100% is never skipped over, it's the one pixel-exact level
        if (old < 1.0 && zoom > 1.0) || (old > 1.0 && zoom < 1.0) {
            zoom = 1.0;
        }
        self.zoom_to(zoom, at)
    }

    pub fn zoom_to(&mut self, zoom: f32, at: (f32, f32)) -> bool {
        let before = *self;
        let point = self.picture_point(at);
        self.zoom = zoom.clamp(self.min_zoom(), MAX_ZOOM);
        self.pan = (at.0 - point.0 * self.zoom, at.1 - point.1 * self.zoom);
        self.fitted = false;
        self.clamp();
        self.untouched &= self.zoom == before.zoom && self.pan == before.pan;
        *self != before
    }

    pub fn pan_by(&mut self, (dx, dy): (f32, f32)) -> bool {
        let before = self.pan;
        self.pan = (self.pan.0 + dx, self.pan.1 + dy);
        self.clamp();
        self.untouched &= self.pan == before;
        self.pan != before
    }

    pub fn center(&self) -> (f32, f32) {
        let (w, h) = self.viewport();
        (w / 2.0, self.inset + h / 2.0)
    }

    pub fn inset(&self) -> f32 {
        self.inset
    }

    fn viewport(&self) -> (f32, f32) {
        (self.area.0, (self.area.1 - self.inset).max(1.0))
    }

    pub fn picture_point(&self, (x, y): (f32, f32)) -> (f32, f32) {
        ((x - self.pan.0) / self.zoom, (y - self.pan.1) / self.zoom)
    }

    fn fit_zoom(&self) -> f32 {
        let whole = self.whole_zoom();
        let (w, h) = self.viewport();
        let across = (w / self.picture.0).max(h / self.picture.1);
        // for scrolled screenshots
        let zoom = if whole >= across / 2.0 { whole } else { across };
        zoom.min(1.0)
    }

    fn whole_zoom(&self) -> f32 {
        let (w, h) = self.viewport();
        (w / self.picture.0).min(h / self.picture.1)
    }

    fn min_zoom(&self) -> f32 {
        self.whole_zoom().clamp(f32::EPSILON, MIN_ZOOM)
    }

    fn clamp(&mut self) {
        let axis = |pan: f32, from: f32, area: f32, size: f32| {
            let shown = size * self.zoom;
            if shown <= area {
                from + (area - shown) / 2.0
            } else {
                pan.clamp(from + area - shown, from)
            }
        };
        let (w, h) = self.viewport();
        self.pan = (
            axis(self.pan.0, 0.0, w, self.picture.0),
            axis(self.pan.1, self.inset, h, self.picture.1),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(picture: (u32, u32), area: (u32, u32)) -> View {
        let mut view = View::new(picture);
        view.resize(area, 0.0, 1.0);
        view
    }

    #[test]
    fn screenshot_fits_whole_and_centered() {
        let view = view((1920, 1080), (1000, 1000));
        assert!((view.zoom - 1000.0 / 1920.0).abs() < 1e-6);
        assert!(view.pan.0.abs() < 1e-3);
        assert!((view.pan.1 - (1000.0 - 1080.0 * view.zoom) / 2.0).abs() < 1e-3);
    }

    #[test]
    fn long_capture_fits_its_width_from_the_top() {
        let view = view((1600, 30000), (1200, 800));
        assert_eq!(view.zoom, 0.75);
        assert_eq!(view.pan, (0.0, 0.0));
    }

    #[test]
    fn small_picture_is_not_blown_up() {
        let view = view((200, 100), (800, 600));
        assert_eq!(view.zoom, 1.0);
        assert_eq!(view.pan, (300.0, 250.0));
    }

    #[test]
    fn zoom_keeps_the_point_under_the_pointer() {
        let mut view = view((4000, 4000), (1000, 1000));
        view.zoom_to(1.0, (500.0, 500.0));
        let at = (300.0, 700.0);
        let before = view.picture_point(at);
        view.zoom_at(1.25, at);
        let after = view.picture_point(at);
        assert!((before.0 - after.0).abs() < 1e-3 && (before.1 - after.1).abs() < 1e-3);
    }

    #[test]
    fn zoom_stops_at_100_percent_on_the_way() {
        let mut view = view((4000, 4000), (1000, 1000));
        view.zoom_to(0.9, (0.0, 0.0));
        view.zoom_at(1.25, (0.0, 0.0));
        assert_eq!(view.zoom, 1.0);
        view.zoom_at(1.25, (0.0, 0.0));
        assert_eq!(view.zoom, 1.25);
    }

    #[test]
    fn panning_stops_at_the_edges() {
        let mut view = view((1600, 30000), (1200, 800));
        assert!(!view.pan_by((0.0, 100.0)));
        assert!(view.pan_by((0.0, -1e9)));
        assert_eq!(view.pan.1, 800.0 - 30000.0 * 0.75);
        assert!(!view.pan_by((50.0, 0.0)));
    }

    #[test]
    fn whole_long_capture_can_be_seen_zoomed_out() {
        let mut view = view((1600, 30000), (1200, 800));
        view.zoom_at(1e-6, (0.0, 0.0));
        assert!((view.zoom - 800.0 / 30000.0).abs() < 1e-6);
    }

    #[test]
    fn fitted_view_refits_and_keeps_its_place_on_resize() {
        let mut view = view((1600, 30000), (1200, 800));
        view.pan_by((0.0, -6000.0));
        let middle = view.picture_point(view.center());
        view.resize((800, 800), 0.0, 1.0);
        assert_eq!(view.zoom, 0.5);
        let moved = view.picture_point(view.center());
        assert!((middle.1 - moved.1).abs() < 1e-2);
    }

    #[test]
    fn untouched_long_capture_fits_again_from_the_top() {
        let mut view = view((1600, 30000), (1200, 800));
        view.resize((1200, 700), 0.0, 1.0);
        assert_eq!(view.pan, (0.0, 0.0));
        view.pan_by((0.0, -100.0));
        view.resize((1200, 600), 0.0, 1.0);
        assert_ne!(view.pan.1, 0.0);
    }

    #[test]
    fn picture_starts_under_the_inset() {
        let mut view = View::new((1000, 1000));
        view.resize((500, 600), 100.0, 1.0);
        assert_eq!(view.zoom, 0.5);
        assert_eq!(view.pan, (0.0, 100.0));
        view.zoom_to(1.0, view.center());
        assert!(view.pan_by((0.0, 1e9)));
        assert_eq!(view.pan.1, 100.0);
        assert!(view.pan_by((0.0, -1e9)));
        assert_eq!(view.pan.1, 600.0 - 1000.0);
    }

    #[test]
    fn small_picture_is_centered_below_the_inset() {
        let mut view = View::new((200, 100));
        view.resize((800, 600), 100.0, 1.0);
        assert_eq!(view.pan, (300.0, 100.0 + 200.0));
        assert_eq!(view.center(), (400.0, 350.0));
    }

    #[test]
    fn long_capture_refits_from_below_the_inset() {
        let mut view = View::new((1600, 30000));
        view.resize((1200, 900), 100.0, 1.0);
        assert_eq!(view.zoom, 0.75);
        assert_eq!(view.pan, (0.0, 100.0));
        view.resize((1200, 700), 80.0, 1.0);
        assert_eq!(view.pan, (0.0, 80.0));
        view.pan_by((0.0, -500.0));
        view.fit();
        assert_eq!(view.pan, (0.0, 80.0));
    }

    #[test]
    fn new_scale_keeps_the_size_on_screen() {
        let mut view = view((4000, 4000), (1000, 1000));
        view.zoom_to(1.0, (0.0, 0.0));
        view.resize((2000, 2000), 0.0, 2.0);
        assert_eq!(view.zoom, 2.0);
    }
}
