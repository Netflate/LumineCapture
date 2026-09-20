use crate::config::ColorPicker;
use crate::types::Annotation;
use crate::interaction::ScrollAccumulator;
use crate::theme::{anim, font};
use crate::ui::panel::{AnimatedPanel, HoverablePanel, PanelItem, UiPanel};
use crate::ui::text_field::TextFieldGroup;
use crate::types::tool_settings::default_color;
use std::time::{Duration, Instant};
use tiny_skia::{Color, Mask, Pixmap, Rect};

fn cfg() -> &'static ColorPicker {
    &crate::config::get().color_picker
}

pub const RECENT_LABEL: &str = "── Palette & Values ───────────";

pub fn field_font_size() -> f32 {
    font::small()
}

pub fn width() -> f32 {
    let c = cfg();
    c.padding * 2.0 + c.sv_size + c.hue_gap + c.hue_width
}

pub fn height() -> f32 {
    rgba_row_offset() + cfg().field_height + cfg().padding
}

pub fn swatch_radius() -> f32 {
    cfg().swatch_size / 2.0
}

fn recent_row_offset() -> f32 {
    let c = cfg();
    c.padding + c.sv_size + c.label_gap + c.label_height + c.swatch_row_gap
}

fn hex_row_offset() -> f32 {
    recent_row_offset() + cfg().swatch_size + cfg().row_gap
}

fn rgba_row_offset() -> f32 {
    hex_row_offset() + cfg().field_height + cfg().row_gap
}

#[derive(Debug, Clone, Copy)]
pub enum ColorPickerItem {}

impl PanelItem for ColorPickerItem {
    fn size(&self) -> f32 {
        match *self {}
    }
    fn trailing_padding(&self) -> f32 {
        match *self {}
    }
    fn is_button(&self) -> bool {
        match *self {}
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColorField {
    Hex,
    R,
    G,
    B,
    A,
}

pub const RGBA_FIELDS: [ColorField; 4] =
    [ColorField::R, ColorField::G, ColorField::B, ColorField::A];

fn rgba_field_index(field: ColorField) -> Option<usize> {
    RGBA_FIELDS.iter().position(|f| *f == field)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorPopoverElement {
    SvSquare,
    HueSlider,
    Swatch(usize),
    Eyedropper,
    Field(ColorField),
}

/// h: 0.0..=360.0, s/v: 0.0..=1.0
pub fn hsv_to_color(h: f32, s: f32, v: f32) -> Color {
    let h = h.rem_euclid(360.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as u32 {
        0..=59 => (c, x, 0.0),
        60..=119 => (x, c, 0.0),
        120..=179 => (0.0, c, x),
        180..=239 => (0.0, x, c),
        240..=299 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Color::from_rgba(r + m, g + m, b + m, 1.0).unwrap()
}

pub fn color_to_hsv(c: Color) -> (f32, f32, f32) {
    let r = c.red();
    let g = c.green();
    let b = c.blue();
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let h = if delta <= f32::EPSILON {
        0.0
    } else if max == r {
        60.0 * (((g - b) / delta).rem_euclid(6.0))
    } else if max == g {
        60.0 * (((b - r) / delta) + 2.0)
    } else {
        60.0 * (((r - g) / delta) + 4.0)
    };

    let s = if max <= f32::EPSILON {
        0.0
    } else {
        delta / max
    };
    let v = max;

    (h.rem_euclid(360.0), s, v)
}

fn color_bytes(c: Color) -> (u8, u8, u8, u8) {
    let c8 = c.to_color_u8();
    (c8.red(), c8.green(), c8.blue(), c8.alpha())
}

pub fn color_channel_u8(c: Color, field: ColorField) -> u8 {
    let c8 = c.to_color_u8();
    match field {
        ColorField::R => c8.red(),
        ColorField::G => c8.green(),
        ColorField::B => c8.blue(),
        ColorField::A => c8.alpha(),
        ColorField::Hex => 0,
    }
}

pub fn color_with_channel(c: Color, field: ColorField, value: u8) -> Color {
    let c8 = c.to_color_u8();
    let (r, g, b, a) = (c8.red(), c8.green(), c8.blue(), c8.alpha());
    let (r, g, b, a) = match field {
        ColorField::R => (value, g, b, a),
        ColorField::G => (r, value, b, a),
        ColorField::B => (r, g, value, a),
        ColorField::A => (r, g, b, value),
        ColorField::Hex => (r, g, b, a),
    };
    Color::from_rgba8(r, g, b, a)
}

pub fn color_to_hex_string(c: Color) -> String {
    let c8 = c.to_color_u8();
    if c8.alpha() == 255 {
        format!("{:02X}{:02X}{:02X}", c8.red(), c8.green(), c8.blue())
    } else {
        format!(
            "{:02X}{:02X}{:02X}{:02X}",
            c8.red(),
            c8.green(),
            c8.blue(),
            c8.alpha()
        )
    }
}

pub fn parse_hex_color(s: &str) -> Option<Color> {
    let s = s.trim().trim_start_matches('#');
    let (r, g, b, a) = match s.len() {
        6 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            (r, g, b, 255)
        }
        8 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            let a = u8::from_str_radix(&s[6..8], 16).ok()?;
            (r, g, b, a)
        }
        _ => return None,
    };
    Some(Color::from_rgba8(r, g, b, a))
}

pub fn step_hex_text(text: &str, steps: i32) -> String {
    let trimmed = text.trim().trim_start_matches('#');
    let width = if trimmed.len() == 8 { 8usize } else { 6usize };
    let current = u32::from_str_radix(trimmed, 16).unwrap_or(0);
    let max: u32 = if width == 8 { 0xFFFF_FFFF } else { 0x00FF_FFFF };

    let new_value = if steps >= 0 {
        current.saturating_add(steps as u32).min(max)
    } else {
        current.saturating_sub(steps.unsigned_abs())
    };

    format!("{:0width$X}", new_value, width = width)
}

fn palette_row(history: &[Color]) -> Vec<Color> {
    let mut row = history.to_vec();
    // default palette when there isn't selection history
    for color in cfg().palette.iter().map(|c| c.color()) {
        if !row.iter().any(|c| color_bytes(*c) == color_bytes(color)) {
            row.push(color);
        }
    }
    row.truncate(cfg().max_recent);
    row
}

// ─── SV-square state (Content of the pixmap is built by the render) ──────────────
pub struct ColorSquareState {
    pub hue: f32,
    pub sv: (f32, f32),
    pub alpha: u8,
    pub sv_pixmap: Option<Pixmap>,
    pub sv_dirty: bool,
    pub dragging: bool,
}

impl Default for ColorSquareState {
    fn default() -> Self {
        Self::new()
    }
}

impl ColorSquareState {
    pub fn new() -> Self {
        let (h, s, v) = color_to_hsv(default_color().color());
        let alpha = default_color().alpha();
        Self {
            hue: h,
            sv: (s, v),
            alpha,
            sv_pixmap: None,
            sv_dirty: true,
            dragging: false,
        }
    }

    pub fn set_hue(&mut self, hue: f32) {
        let hue = hue.rem_euclid(360.0);
        if (self.hue - hue).abs() > f32::EPSILON {
            self.hue = hue;
            self.sv_dirty = true;
        }
    }

    pub fn color(&self) -> Color {
        let rgb = hsv_to_color(self.hue, self.sv.0, self.sv.1).to_color_u8();
        Color::from_rgba8(rgb.red(), rgb.green(), rgb.blue(), self.alpha)
    }
}

// ── hue slider geometry ─────────────────────────────────────────
fn marker_visual_radius() -> f32 {
    let c = cfg();
    c.marker_radius + c.marker_stroke + c.marker_outline
}

pub fn hue_handle_center_y(track_top: f32, hue: f32) -> f32 {
    let inset = marker_visual_radius();
    let usable = (cfg().sv_size - 2.0 * inset).max(1.0);
    let t = hue.rem_euclid(360.0) / 360.0;
    track_top + inset + t * usable
}

pub fn hue_from_pointer_y(track_top: f32, y: f32) -> f32 {
    let inset = marker_visual_radius();
    let usable = (cfg().sv_size - 2.0 * inset).max(1.0);
    let t = ((y - track_top - inset) / usable).clamp(0.0, 1.0);
    (t * 360.0).min(359.999)
}

pub fn sv_square_origin(content_origin: (f32, f32)) -> (f32, f32) {
    let pad = cfg().padding;
    (content_origin.0 + pad, content_origin.1 + pad)
}

pub fn hue_track_origin(content_origin: (f32, f32)) -> (f32, f32) {
    let c = cfg();
    (
        content_origin.0 + c.padding + c.sv_size + c.hue_gap,
        content_origin.1 + c.padding,
    )
}

// ── recent colors geometry ──────────────────────────────

pub fn recent_label_origin(content_origin: (f32, f32)) -> (f32, f32) {
    let c = cfg();
    (
        content_origin.0 + c.padding,
        content_origin.1 + c.padding + c.sv_size + c.label_gap,
    )
}

pub fn recent_label_rect(content_origin: (f32, f32)) -> Rect {
    let (x, y) = recent_label_origin(content_origin);
    let width = (width() - cfg().padding * 2.0).max(1.0);
    Rect::from_xywh(x, y, width, cfg().label_height.max(1.0)).expect("recent label rect")
}

pub fn swatch_row_top(content_origin_y: f32) -> f32 {
    content_origin_y + recent_row_offset()
}

pub fn swatch_center(content_origin: (f32, f32), idx: usize) -> (f32, f32) {
    let row_top = swatch_row_top(content_origin.1);
    let c = cfg();
    let cx = content_origin.0
        + c.padding
        + swatch_radius()
        + idx as f32 * (c.swatch_size + c.swatch_gap);
    let cy = row_top + swatch_radius();
    (cx, cy)
}

pub fn eyedropper_center(content_origin: (f32, f32)) -> (f32, f32) {
    let row_top = swatch_row_top(content_origin.1);
    (
        content_origin.0 + width() - cfg().padding - swatch_radius(),
        row_top + swatch_radius(),
    )
}

// ── hex / rgba fields geometry ───────────────────────────

pub fn hex_row_top(content_origin_y: f32) -> f32 {
    content_origin_y + hex_row_offset()
}

pub fn rgba_row_top(content_origin_y: f32) -> f32 {
    content_origin_y + rgba_row_offset()
}

pub fn hex_label_pos(content_origin: (f32, f32)) -> (f32, f32) {
    (
        content_origin.0 + cfg().padding,
        hex_row_top(content_origin.1),
    )
}

pub fn hex_field_geom(content_origin: (f32, f32)) -> Rect {
    let c = cfg();
    let x = content_origin.0 + c.padding + c.hex_label_width;
    let y = hex_row_top(content_origin.1);
    let width = (width() - c.padding * 2.0 - c.hex_label_width).max(1.0);
    Rect::from_xywh(x, y, width, c.field_height.max(1.0)).expect("hex field rect")
}

fn rgba_field_total_width() -> f32 {
    let c = cfg();
    (width() - c.padding * 2.0 - 3.0 * c.field_gap) / 4.0
}

pub fn rgba_slot_origin(content_origin: (f32, f32), idx: usize) -> (f32, f32) {
    let total_w = rgba_field_total_width();
    let x = content_origin.0 + cfg().padding + idx as f32 * (total_w + cfg().field_gap);
    let y = rgba_row_top(content_origin.1);
    (x, y)
}

pub fn rgba_field_geom(content_origin: (f32, f32), idx: usize) -> Rect {
    let (slot_x, slot_y) = rgba_slot_origin(content_origin, idx);
    let total_w = rgba_field_total_width();
    let c = cfg();
    Rect::from_xywh(
        slot_x + c.rgba_label_width,
        slot_y,
        (total_w - c.rgba_label_width).max(1.0),
        c.field_height.max(1.0),
    )
    .expect("rgba field rect")
}

fn point_in_rect(local: (f64, f64), rect: Rect) -> bool {
    let px = local.0 as f32;
    let py = local.1 as f32;
    px >= rect.left() && px <= rect.right() && py >= rect.top() && py <= rect.bottom()
}

pub struct ColorPickerPopover {
    pub colorpicker_pixmap: Option<Pixmap>,

    pub position: (f32, f32),
    pub size: (f32, f32),
    pub opacity: f32,
    pub monitor_idx: usize,

    pub open: bool,
    pub dirty: bool,

    pub hovered: Option<ColorPopoverElement>,

    pub last_tick: Option<Instant>,
    pub render_pos: (f32, f32),

    pub sv_square: ColorSquareState,

    pub hue_track_pixmap: Option<Pixmap>, // content is built by render, built once
    pub hue_dragging: bool,

    pub sv_clip_mask: Option<Mask>,
    pub hue_clip_mask: Option<Mask>,

    pub picking: bool,
    pub recent_colors: Vec<Color>,
    pub history: Vec<Color>,
    pub initial_history: Vec<Color>,
    pub fields: TextFieldGroup<ColorField>,
    pub pre_edit_snapshot: Option<Vec<Annotation>>,

    /// Scroll accumulator for scrollable fields (hex/rgba)
    /// Keeps scroll fractional state separate from raw events to avoid
    /// processing every single scroll event, trackpad or mouse wheel held
    /// will flood thousands of events that would freeze 
    pub scroll: ScrollAccumulator<ColorField>,
}

impl Default for ColorPickerPopover {
    fn default() -> Self {
        Self::new()
    }
}

impl ColorPickerPopover {
    pub fn new() -> Self {
        let history: Vec<Color> = crate::editor::saved::get()
            .recent_colors
            .iter()
            .map(|c| c.color())
            .take(cfg().max_recent)
            .collect();
        Self {
            colorpicker_pixmap: None,
            position: (0.0, 0.0),
            size: (width(), height()),
            opacity: 0.0,
            monitor_idx: 0,
            open: false,
            dirty: false,
            hovered: None,
            last_tick: None,
            render_pos: (0.0, 0.0),
            sv_square: ColorSquareState::new(),
            hue_track_pixmap: None,
            hue_dragging: false,
            sv_clip_mask: None,
            hue_clip_mask: None,
            picking: false,
            recent_colors: palette_row(&history),
            initial_history: history.clone(),
            history,
            fields: TextFieldGroup::new(),
            scroll: ScrollAccumulator::new(),
            pre_edit_snapshot: None,
        }
    }

    pub fn hit_test(&self, local: (f64, f64)) -> bool {
        let Some(rect) = self.rect() else {
            return false;
        };
        point_in_rect(local, rect)
    }

    pub fn sv_square_rect(&self) -> Option<Rect> {
        let rect = self.rect()?;
        let (x, y) = sv_square_origin((rect.left(), rect.top()));
        let side = cfg().sv_size;
        Rect::from_xywh(x, y, side, side)
    }

    pub fn sv_square_hit(&self, local: (f64, f64)) -> bool {
        self.sv_square_rect()
            .is_some_and(|r| point_in_rect(local, r))
    }

    pub fn set_sv_from_local(&mut self, local: (f64, f64)) {
        let Some(rect) = self.sv_square_rect() else {
            return;
        };
        let px = local.0 as f32;
        let py = local.1 as f32;
        let s = ((px - rect.left()) / rect.width()).clamp(0.0, 1.0);
        let v = 1.0 - ((py - rect.top()) / rect.height()).clamp(0.0, 1.0);
        self.sv_square.sv = (s, v);
    }

    pub fn hue_slider_rect(&self) -> Option<Rect> {
        let rect = self.rect()?;
        let (x, y) = hue_track_origin((rect.left(), rect.top()));
        Rect::from_xywh(x, y, cfg().hue_width, cfg().sv_size)
    }

    pub fn hue_slider_hit(&self, local: (f64, f64)) -> bool {
        let Some(rect) = self.hue_slider_rect() else {
            return false;
        };
        let px = local.0 as f32;
        let py = local.1 as f32;
        let bleed = (marker_visual_radius() - cfg().hue_width / 2.0).max(0.0);
        px >= rect.left() - bleed
            && px <= rect.right() + bleed
            && py >= rect.top()
            && py <= rect.bottom()
    }

    pub fn set_hue_from_local(&mut self, local: (f64, f64)) {
        let Some(rect) = self.hue_slider_rect() else {
            return;
        };
        let hue = hue_from_pointer_y(rect.top(), local.1 as f32);
        self.sv_square.set_hue(hue);
    }

    pub fn palette(&self) -> &[Color] {
        &self.recent_colors
    }

    pub fn record_used_color(&mut self, color: Color) {
        let bytes = color_bytes(color);
        self.history.retain(|c| color_bytes(*c) != bytes);
        self.history.insert(0, color);
        self.history.truncate(cfg().max_recent);
        self.recent_colors = palette_row(&self.history);
    }

    pub fn select_color(&mut self, color: Color) {
        let (h, s, v) = color_to_hsv(color);
        self.sv_square.set_hue(h);
        self.sv_square.sv = (s, v);
        self.sv_square.alpha = color.to_color_u8().alpha();
    }

    pub fn swatch_hit(&self, local: (f64, f64)) -> Option<usize> {
        let rect = self.rect()?;
        let origin = (rect.left(), rect.top());

        for idx in 0..self.palette().len() {
            let (cx, cy) = swatch_center(origin, idx);
            let dx = local.0 as f32 - cx;
            let dy = local.1 as f32 - cy;
            if dx * dx + dy * dy <= swatch_radius() * swatch_radius() {
                return Some(idx);
            }
        }
        None
    }

    pub fn eyedropper_hit(&self, local: (f64, f64)) -> bool {
        let Some(rect) = self.rect() else {
            return false;
        };
        let (cx, cy) = eyedropper_center((rect.left(), rect.top()));
        let (dx, dy) = (local.0 as f32 - cx, local.1 as f32 - cy);
        dx * dx + dy * dy <= swatch_radius() * swatch_radius()
    }

    // ── hex / rgba fields ────────────────────────────────

    pub fn hex_field_rect(&self) -> Option<Rect> {
        let rect = self.rect()?;
        Some(hex_field_geom((rect.left(), rect.top())))
    }

    pub fn hex_field_hit(&self, local: (f64, f64)) -> bool {
        self.hex_field_rect()
            .is_some_and(|r| point_in_rect(local, r))
    }

    pub fn rgba_field_rect(&self, field: ColorField) -> Option<Rect> {
        let rect = self.rect()?;
        let idx = rgba_field_index(field)?;
        Some(rgba_field_geom((rect.left(), rect.top()), idx))
    }

    pub fn rgba_field_hit(&self, local: (f64, f64)) -> Option<ColorField> {
        RGBA_FIELDS.into_iter().find(|f| {
            self.rgba_field_rect(*f)
                .is_some_and(|r| point_in_rect(local, r))
        })
    }

    pub fn field_rect(&self, field: ColorField) -> Option<Rect> {
        match field {
            ColorField::Hex => self.hex_field_rect(),
            _ => self.rgba_field_rect(field),
        }
    }

    pub fn field_text(&self, field: ColorField) -> String {
        if let Some(edit) = self.fields.editing.as_ref()
            && edit.key == field {
                return edit.field.text.clone();
            }
        self.fields.value(field).cloned().unwrap_or_default()
    }

    pub fn confirmed_field_text(&self, field: ColorField) -> String {
        let color = self.sv_square.color();
        match field {
            ColorField::Hex => color_to_hex_string(color),
            _ => color_channel_u8(color, field).to_string(),
        }
    }

    pub fn sync_field_values(&mut self) {
        let color = self.sv_square.color();
        self.fields
            .sync_value(ColorField::Hex, color_to_hex_string(color));
        for field in RGBA_FIELDS {
            self.fields
                .sync_value(field, color_channel_u8(color, field).to_string());
        }
    }

    pub fn try_apply_hex_text(&mut self, text: &str) -> Option<Color> {
        let color = parse_hex_color(text)?;
        self.select_color(color);
        Some(color)
    }

    pub fn try_apply_rgba_text(&mut self, field: ColorField, text: &str) -> Option<Color> {
        let value: u8 = text.trim().parse().ok()?;
        let current = self.sv_square.color();
        let color = color_with_channel(current, field, value);
        self.select_color(color);
        Some(color)
    }

    /// Accumulate a fractional scroll `delta` (already in "steps", not raw
    /// pixels) for the given field and return how many whole steps have
    /// accumulated since the last call (may be negative). Switching fields
    /// discards the leftover from the previous one.
    ///
    /// Delegates to [`ScrollAccumulator`], see its docs for the two
    /// flood-prevention safeguards (rate limit + step cap).
    pub fn scroll_step(&mut self, field: ColorField, delta: f32) -> i32 {
        self.scroll.step(field, delta)
    }

    /// scroll reset. Call on any non-scroll user action
    /// (click, key press) so stale queued events don't trigger afterwards
    pub fn cancel_scroll(&mut self) {
        self.scroll.cancel();
    }
}

impl UiPanel for ColorPickerPopover {
    type Item = ColorPickerItem;

    fn render_pos(&self) -> (f32, f32) {
        self.render_pos
    }
    fn size(&self) -> (f32, f32) {
        self.size
    }
    fn items(&self) -> &[Self::Item] {
        &[]
    }
    fn padding(&self) -> f32 {
        cfg().padding
    }
    fn monitor_idx(&self) -> usize {
        self.monitor_idx
    }
    fn set_dirty(&mut self) {
        self.dirty = true;
    }
    fn is_dirty(&self) -> bool {
        self.dirty
    }
    fn is_visible(&self) -> bool {
        self.open || self.opacity > 0.0
    }

    fn rect(&self) -> Option<Rect> {
        if self.opacity <= 0.0 {
            return None;
        }
        let (x, y) = self.render_pos();
        let (w, h) = self.size();
        Rect::from_xywh(x, y, w, h)
    }
}

impl HoverablePanel for ColorPickerPopover {
    type Hover = Option<ColorPopoverElement>;
    fn hovered(&self) -> Self::Hover {
        self.hovered
    }
    fn set_hovered(&mut self, hover: Self::Hover) {
        self.hovered = hover;
    }
}

impl AnimatedPanel for ColorPickerPopover {
    fn last_tick(&self) -> Option<Instant> {
        self.last_tick
    }
    fn set_last_tick(&mut self, at: Instant) {
        self.last_tick = Some(at);
    }

    fn anim_interval(&self) -> Duration {
        anim::frame()
    }
    fn anim_dt(&self) -> f32 {
        anim::dt()
    }

    fn is_animating(&self) -> bool {
        let target = if self.open { 1.0 } else { 0.0 };
        (self.opacity - target).abs() > anim::OPACITY_EPSILON
    }

    fn animate_step(&mut self, dt: f32) -> bool {
        let target = if self.open { 1.0 } else { 0.0 };
        if (self.opacity - target).abs() > anim::OPACITY_EPSILON {
            let delta = anim::popover_fade() * crate::config::get().animation.speed * dt;
            self.opacity += (target - self.opacity).signum() * delta;
            self.opacity = self.opacity.clamp(0.0, 1.0);
            true
        } else {
            false
        }
    }
}