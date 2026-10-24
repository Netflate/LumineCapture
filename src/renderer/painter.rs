// Painter is a wrapper over pixmap plus drawing context
// viewport (origin and scale), icons, and fonts.
//
// Shape helpers (fill_path, fill_rounded, etc) automatically apply viewport
// transform.
// (draw_pixmap() and canvas() draw directly in raw pixels without it)
use std::collections::HashMap;

use cosmic_text::{FontSystem, SwashCache};
use tiny_skia::{
    BlendMode, Color, FillRule, FilterQuality, Paint, Path, PathBuilder, Pixmap, PixmapPaint, Rect,
    Stroke, Transform,
};
use usvg::Tree;

use super::paths::rounded_rect_path;
use crate::theme::radius;

pub type Icons = HashMap<&'static str, Tree>;

pub(super) struct Fonts<'a> {
    pub(super) system: &'a mut FontSystem,
    pub(super) cache: &'a mut SwashCache,
}

pub struct Painter<'a> {
    pub(super) canvas: &'a mut Pixmap,
    scale: f32,
    // top-left corner position in the coorindates callers drawn in
    pub(super) origin: (f32, f32),
    pub(super) icons: Option<&'a Icons>,
    pub(super) fonts: Option<Fonts<'a>>,
}

impl<'a> Painter<'a> {
    pub fn new(canvas: &'a mut Pixmap) -> Self {
        Self {
            canvas,
            scale: 1.0,
            origin: (0.0, 0.0),
            icons: None,
            fonts: None,
        }
    }

    pub fn with_icons(mut self, icons: &'a Icons) -> Self {
        self.icons = Some(icons);
        self
    }

    pub fn with_text(mut self, system: &'a mut FontSystem, cache: &'a mut SwashCache) -> Self {
        self.fonts = Some(Fonts { system, cache });
        self
    }

    pub fn with_origin(mut self, origin: (f32, f32)) -> Self {
        self.origin = origin;
        self
    }

    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    pub fn canvas(&mut self) -> &mut Pixmap {
        self.canvas
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn size(&self) -> (f32, f32) {
        (
            self.canvas.width() as f32 / self.scale,
            self.canvas.height() as f32 / self.scale,
        )
    }

    pub fn to_px(&self, x: f32, y: f32) -> (f32, f32) {
        (
            (x - self.origin.0) * self.scale,
            (y - self.origin.1) * self.scale,
        )
    }

    pub fn transform(&self) -> Transform {
        Transform::from_translate(-self.origin.0, -self.origin.1).post_scale(self.scale, self.scale)
    }

    pub fn transform_shifted(&self, dx: f32, dy: f32) -> Transform {
        Transform::from_translate(-self.origin.0 + dx, -self.origin.1 + dy)
            .post_scale(self.scale, self.scale)
    }

    /// paints into another pixmap with the same stuff (icons, fonts, and scale)
    /// keep in mind, origin get reset; new canvas has its own coordinates
    /// use .with_origin afterwards if the new canvas is offset from it
    pub fn sub<'b>(&'b mut self, canvas: &'b mut Pixmap) -> Painter<'b> {
        Painter {
            canvas,
            scale: self.scale,
            origin: (0.0, 0.0),
            icons: self.icons,
            fonts: self.fonts.as_mut().map(|fonts| Fonts {
                system: &mut *fonts.system,
                cache: &mut *fonts.cache,
            }),
        }
    }

    pub fn fill_path(&mut self, path: &Path, color: impl Into<Color>) {
        let mut paint = Paint::default();
        paint.set_color(color.into());
        paint.anti_alias = true;
        let transform = self.transform();
        self.canvas
            .fill_path(path, &paint, FillRule::Winding, transform, None);
    }

    pub fn stroke_path(&mut self, path: &Path, color: impl Into<Color>, stroke: &Stroke) {
        let mut paint = Paint::default();
        paint.set_color(color.into());
        paint.anti_alias = true;
        let transform = self.transform();
        self.canvas
            .stroke_path(path, &paint, stroke, transform, None);
    }

    pub fn fill_rounded(&mut self, rect: Rect, radius: f32, color: impl Into<Color>) {
        if let Some(path) = rounded_rect_path(&rect, radius, true, true, true, true) {
            self.fill_path(&path, color);
        }
    }

    pub fn fill_circle(&mut self, cx: f32, cy: f32, r: f32, color: impl Into<Color>) {
        let mut pb = PathBuilder::new();
        pb.push_circle(cx, cy, r);
        if let Some(path) = pb.finish() {
            self.fill_path(&path, color);
        }
    }

    /// ignores origin & scale
    pub fn draw_pixmap(&mut self, x: i32, y: i32, pixmap: &Pixmap) {
        self.canvas.draw_pixmap(
            x,
            y,
            pixmap.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }

    /// uses sub, since to correclty apply opacity in draws them
    /// in a separate pixmap, and then apply on it as a whole
    /// otherwise icons wouldn't be transparent
    pub fn panel(
        &mut self,
        cache: &mut Option<Pixmap>,
        pos: (f32, f32),
        size: (f32, f32),
        opacity: f32,
        redraw: bool,
        content: impl FnOnce(&mut Painter),
    ) {
        let s = self.scale;
        let (px, py) = self.to_px(pos.0, pos.1);
        // rounding is obligatory to avoid desync and visual glitches
        let x = px.round();
        let y = py.round();
        let (w, h) = size;
        let (pw, ph) = ((w * s).ceil() as u32, (h * s).ceil() as u32);

        let fresh = cache
            .as_ref()
            .is_none_or(|p| p.width() != pw || p.height() != ph);
        if fresh {
            *cache = Pixmap::new(pw, ph);
        }
        let Some(pixmap) = cache.as_mut() else {
            return;
        };

        if fresh || redraw {
            pixmap.fill(Color::TRANSPARENT);
            content(&mut self.sub(pixmap));
        }

        self.canvas.draw_pixmap(
            x as i32,
            y as i32,
            pixmap.as_ref(),
            &PixmapPaint {
                opacity,
                blend_mode: BlendMode::SourceOver,
                quality: FilterQuality::Nearest,
            },
            Transform::identity(),
            None,
        );

        // (border follows where the pixmap went after rounding)
        let (bx, by) = (x / s + self.origin.0, y / s + self.origin.1);
        self.panel_border(bx, by, w, h, radius::panel(), opacity);
    }
}
