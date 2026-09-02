// notifications: simple text on the overlay, without any input or hit testing
// not implemented as panels, since panels are not a one time thing, and has wider
// functionality, as input fields, hit testing and etc
use std::time::{Duration, Instant};

use cosmic_text::FontSystem;
use tiny_skia::Rect;

use crate::editor::DamageZone;
use crate::types::panel::emit_panel_damage;

pub const TOAST_HEIGHT: f32 = 38.0;
pub const TOAST_PAD_X: f32 = 18.0;
pub const TOAST_RADIUS: f32 = 10.0;
pub const TOAST_FONT_SIZE: f32 = 14.0;

const TICK: Duration = Duration::from_millis(16);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    OcrPickRegion,
}

#[derive(Clone, Copy)]
pub enum ToastAnchor {
    CursorMonitorCenter,
}

#[derive(Clone, Copy)]
pub enum ToastLife {
    Manual,
    Timed(Duration),
}

pub struct ToastSpec {
    pub text: &'static str,
    pub anchor: ToastAnchor,
    pub life: ToastLife,
    /// in seconds
    pub fade_in: f32,
    pub fade_out: f32,
}

impl ToastKind {
    pub const fn spec(self) -> ToastSpec {
        match self {
            ToastKind::OcrPickRegion => ToastSpec {
                text: "Drag a box over the area you want to read",
                anchor: ToastAnchor::CursorMonitorCenter,
                life: ToastLife::Manual,
                fade_in: 0.18,
                fade_out: 0.14,
            },
        }
    }
}

pub struct Toast {
    pub kind: ToastKind,
    pub spec: ToastSpec,
    pub text_width: f32,
    pub monitor_idx: usize,
    pub rect: Option<Rect>,
    pub progress: f32,
    pub closing: bool,
    pub shown_at: Instant,
}

impl Toast {
    pub fn opacity(&self) -> f32 {
        let t = self.progress.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    fn layout(&mut self, place: &ToastPlace) {
        self.monitor_idx = place.monitor_idx;
        let w = self.text_width + TOAST_PAD_X * 2.0;
        self.rect = match self.spec.anchor {
            ToastAnchor::CursorMonitorCenter => Rect::from_xywh(
                ((place.size.0 - w) / 2.0).round(),
                ((place.size.1 - TOAST_HEIGHT) / 2.0).round(),
                w,
                TOAST_HEIGHT,
            ),
        };
    }
}

#[derive(Clone, Copy)]
pub struct ToastPlace {
    pub monitor_idx: usize,
    pub size: (f32, f32),
}

#[derive(Default)]
pub struct Toasts {
    pub items: Vec<Toast>,
    last_tick: Option<Instant>,
}

impl Toasts {
    pub fn show(&mut self, kind: ToastKind, font_system: &mut FontSystem) {
        if let Some(toast) = self.items.iter_mut().find(|t| t.kind == kind) {
            toast.closing = false;
            toast.shown_at = Instant::now();
            return;
        }

        let spec = kind.spec();
        let text_width =
            crate::renderer::measure_line_width(spec.text, TOAST_FONT_SIZE, font_system);
        self.items.push(Toast {
            kind,
            spec,
            text_width,
            monitor_idx: 0,
            rect: None,
            progress: 0.0,
            closing: false,
            shown_at: Instant::now(),
        });
    }

    pub fn dismiss(&mut self, kind: ToastKind) {
        for toast in self.items.iter_mut().filter(|t| t.kind == kind) {
            toast.closing = true;
        }
    }

    pub fn is_animating(&self) -> bool {
        self.items
            .iter()
            .any(|t| t.closing || t.progress < 1.0 || matches!(t.spec.life, ToastLife::Timed(_)))
    }

    pub fn tick(
        &mut self,
        place: ToastPlace,
        damage_rects: &mut Vec<DamageZone>,
        dirty_mask: &mut u32,
    ) {
        if self.items.is_empty() {
            return;
        }

        let now = Instant::now();
        let elapsed = self
            .last_tick
            .map(|t| now.duration_since(t))
            .unwrap_or(TICK);
        if elapsed < TICK {
            return;
        }
        self.last_tick = Some(now);
        let dt = elapsed.as_secs_f32().min(0.1);

        self.items.retain_mut(|toast| {
            let was = (toast.monitor_idx, toast.rect);

            if !toast.closing
                && let ToastLife::Timed(after) = toast.spec.life
                && toast.shown_at.elapsed() >= after
            {
                toast.closing = true;
            }

            let rate = if toast.closing {
                -1.0 / toast.spec.fade_out
            } else {
                1.0 / toast.spec.fade_in
            };
            let next = (toast.progress + rate * dt).clamp(0.0, 1.0);
            let faded = next != toast.progress;
            toast.progress = next;

            toast.layout(&place);

            if faded || was != (toast.monitor_idx, toast.rect) {
                if let Some(rect) = was.1 {
                    emit_panel_damage(rect, was.0, damage_rects, dirty_mask);
                }
                if let Some(rect) = toast.rect {
                    emit_panel_damage(rect, toast.monitor_idx, damage_rects, dirty_mask);
                }
            }

            !(toast.closing && toast.progress <= 0.0)
        });

        if self.items.is_empty() {
            self.last_tick = None;
        }
    }
}
