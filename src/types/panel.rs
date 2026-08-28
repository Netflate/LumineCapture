use tiny_skia::Rect;
use std::time::{Duration, Instant};

use crate::editor::DamageZone;
use crate::editor::dirty::mark_dirty;

pub trait PanelItem {
    fn size(&self) -> f32;
    fn trailing_padding(&self) -> f32;
    fn is_button(&self) -> bool;
}

// UI Layout Constants used in both toolbar and settings panel, and probably in other panels too 
const fn unit(v: u8) -> f32 {
    v as f32 / 255.0
}

pub const PANEL_COLOR: tiny_skia::Color = unsafe {
    tiny_skia::Color::from_rgba_unchecked(unit(13), unit(13), unit(23), unit(250))
};
pub const SEPARATOR_COLOR: tiny_skia::Color = unsafe {
    tiny_skia::Color::from_rgba_unchecked(unit(255), unit(255), unit(255), unit(255))
};
pub const BUTTON_HOVERED: tiny_skia::Color = unsafe {
    tiny_skia::Color::from_rgba_unchecked(unit(159), unit(48), unit(215), unit(255))
};
pub const BUTTON_SELECTED: tiny_skia::Color = unsafe {
    tiny_skia::Color::from_rgba_unchecked(unit(215), unit(132), unit(255), unit(255))
};

pub const ICON_COLOR: usvg::Color = usvg::Color { red: 255, green: 255, blue: 255 };
pub const ICON_HOVERED: usvg::Color = usvg::Color { red: 159, green: 48, blue: 215 };
pub const ICON_SELECTED: usvg::Color = usvg::Color { red: 215, green: 132, blue: 255 };

pub const DEFAULT_ITEM_BORDER_STROKE: f32 = 1.0;
pub trait UiPanel {
    type Item: PanelItem;

    fn render_pos(&self) -> (f32, f32);
    fn size(&self) -> (f32, f32);
    fn items(&self) -> &[Self::Item];
    fn padding(&self) -> f32;

    /// Which monitor this panel is currently placed on. Every real panel
    /// in this app is monitor-anchored
    fn monitor_idx(&self) -> usize;

    fn set_dirty(&mut self);

    /// Panel's rect right now, or None if there's nothing to show/
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

// ==========================================
// Shared damage/dirty primitives
// ==========================================
//
pub fn emit_panel_damage(
    rect: Rect,
    monitor_idx: usize,
    damage_rects: &mut Vec<DamageZone>,
    dirty_mask: &mut u32,
) {
    damage_rects.push(DamageZone::Local { monitor_idx, rect });
    mark_dirty(dirty_mask, monitor_idx);
}

pub fn sync_panel_rect<P: UiPanel>(
    panel: &mut P,
    old_rect: Option<Rect>,
    old_monitor: usize,
    extra_changed: bool,
    damage_rects: &mut Vec<DamageZone>,
    dirty_mask: &mut u32,
) -> bool {
    let new_rect = panel.rect();
    let new_monitor = panel.monitor_idx();

    let changed = extra_changed || old_rect != new_rect || old_monitor != new_monitor;
    if !changed {
        return false;
    }

    panel.set_dirty();
    if let Some(rect) = old_rect {
        emit_panel_damage(rect, old_monitor, damage_rects, dirty_mask);
    }
    if let Some(rect) = new_rect {
        emit_panel_damage(rect, new_monitor, damage_rects, dirty_mask);
    }
    true
}

/// A panel that tracks what's hovered. 
/// Toolbar's hover is a button index; SettingsPanel's is a
/// button index and a stepper arrow, both fit as 'Self::Hover', so
/// 'sync_panel_hover' works for anything without caring what type it is
pub trait HoverablePanel: UiPanel {
    type Hover: PartialEq + Copy;
    fn hovered(&self) -> Self::Hover;
    fn set_hovered(&mut self, hover: Self::Hover);
}

/// Same idea as 'sync_panel_rect', but for hover state
/// this handles "did it change, and if so redraw".
pub fn sync_panel_hover<P: HoverablePanel>(
    panel: &mut P,
    new_hover: P::Hover,
    damage_rects: &mut Vec<DamageZone>,
    dirty_mask: &mut u32,
) -> bool {
    if new_hover == panel.hovered() {
        return false;
    }
    panel.set_hovered(new_hover);
    panel.set_dirty();

    let monitor_idx = panel.monitor_idx();
    if let Some(rect) = panel.rect() {
        emit_panel_damage(rect, monitor_idx, damage_rects, dirty_mask);
    }
    true
}

// ==========================================
// Generic animation tick
// ==========================================
//
// One abstract function 'tick_panel_animation' for every animated panel.
// answers to how much time passed, how many fixed steps to simulate this frame, 
// and what damage rect to emit if anything moved

pub trait AnimatedPanel: UiPanel {
    fn last_tick(&self) -> Option<Instant>;
    fn set_last_tick(&mut self, at: Instant);

    fn anim_interval(&self) -> Duration {
        Duration::from_millis(16)
    }
    fn anim_dt(&self) -> f32 {
        0.016
    }

    /// Advance the animation by exactly one fixed step of 'dt' seconds.
    /// Return `true` if any visible state changed this step.
    /// Default: nothing to animate.
    fn animate_step(&mut self, _dt: f32) -> bool {
        false
    }

    fn is_animating(&self) -> bool {
        false
    }
}

pub fn tick_panel_animation<P: AnimatedPanel>(
    panel: &mut P,
    damage_rects: &mut Vec<DamageZone>,
    dirty_mask: &mut u32,
) {
    let now = Instant::now();
    let interval = panel.anim_interval();
    let dt = panel.anim_dt();

    let elapsed = panel
        .last_tick()
        .map(|t| now.duration_since(t))
        .unwrap_or(interval);

    if elapsed < interval {
        return;
    }

    let steps = ((elapsed.as_secs_f32() / dt).floor() as u32).clamp(1, 4);
    panel.set_last_tick(now);

    let old_render_pos = panel.render_pos();
    let mut changed = false;

    for _ in 0..steps {
        if panel.animate_step(dt) {
            changed = true;
        }
    }

    if !changed {
        return;
    }

    panel.set_dirty();
    let monitor_idx = panel.monitor_idx();
    mark_dirty(dirty_mask, monitor_idx);

    let (w, h) = panel.size();
    let old_rect = Rect::from_xywh(old_render_pos.0, old_render_pos.1, w, h);
    let new_rect = panel.rect();

    let union = match (old_rect, new_rect) {
        (Some(a), Some(b)) => Rect::from_ltrb(
            a.left().min(b.left()), a.top().min(b.top()),
            a.right().max(b.right()), a.bottom().max(b.bottom()),
        ),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => None,
    };

    if let Some(rect) = union {
        damage_rects.push(DamageZone::Local { monitor_idx, rect });
    }
}