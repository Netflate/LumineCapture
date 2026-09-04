use cosmic_text::{FontSystem, SwashCache};
use tiny_skia::{Color, Paint, Pixmap, Rect, Transform};

use crate::renderer::paths::{draw_panel_border, rounded_rect_path};
use crate::theme::{color, font};
use crate::renderer::text::{HAlign, draw_aligned_text};
use crate::ui::toast::{RADIUS, Toasts};
/// clip is area of the screen cleared for current frame
/// draw somewhat transparent toast only if it intersects with clip, otherwise 
/// the background will be drawn oveer the old frame a second time and darken it 
pub fn draw_toasts(
    canvas: &mut Pixmap,
    toasts: &Toasts,
    monitor_idx: usize,
    clip: Option<&Rect>,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) {
    for toast in &toasts.items {
        if toast.monitor_idx != monitor_idx {
            continue;
        }
        let Some(rect) = toast.rect else { continue };

        let opacity = toast.opacity();
        if opacity <= 0.0 {
            continue;
        }
        if let Some(clip) = clip
            && (clip.left() >= rect.right()
                || clip.right() <= rect.left()
                || clip.top() >= rect.bottom()
                || clip.bottom() <= rect.top())
        {
            continue;
        }

        if let Some(path) = rounded_rect_path(&rect, RADIUS, true, true, true, true) {
            let mut paint = Paint::default();
            paint.set_color(color::PANEL.fade(opacity));
            paint.anti_alias = true;
            canvas.fill_path(
                &path,
                &paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }

        draw_panel_border(
            canvas,
            rect.left(),
            rect.top(),
            rect.width(),
            rect.height(),
            RADIUS,
            opacity,
        );

        draw_aligned_text(
            canvas,
            toast.text.as_ref(),
            font_system,
            swash_cache,
            rect,
            font::LABEL,
            Color::from_rgba(1.0, 1.0, 1.0, opacity).unwrap_or(Color::WHITE),
            HAlign::Center,
            (0.0, 0.0),
            cosmic_text::Weight::NORMAL,
            cosmic_text::Style::Normal,
        );
    }
}
