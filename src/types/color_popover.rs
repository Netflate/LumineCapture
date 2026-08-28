use crate::types::panel::{AnimatedPanel, HoverablePanel, PanelItem, UiPanel};
use crate::types::text_field::TextFieldGroup;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tiny_skia::{Color, Mask, Pixmap, Rect};

pub const COLORPICKER_WIDTH: f32 = 230.0;
pub const COLORPICKER_OFFSET: f32 = 5.0;
pub const COLORPICKER_PADDING: f32 = 15.0;
pub const COLORPICKER_RADIUS: f32 = 15.0;

pub const COLORPICKER_ANIM_INTERVAL: Duration = Duration::from_millis(16);
pub const COLORPICKER_ANIM_DT: f32 = 0.016;

pub const SV_SQUARE_SIZE: f32 = 170.0;
pub const SV_SQUARE_RADIUS: f32 = 10.0;
pub const MARKER_RADIUS: f32 = 10.0;
pub const MARKER_STROKE: f32 = 2.0;
pub const MARKER_OUTLINE: f32 = 0.1;

pub const COLOR_POPOVER_ITEM_BORDER: f32 = 2.0; 
// ^ USED only for sv-square, hue-slider, swatches, not for hex/rgba fields
// input and etc use usual item border constant from types/panel.rs
pub const HUE_SLIDER_GAP: f32 = 12.0;
pub const HUE_SLIDER_WIDTH: f32 = 17.0;
pub const HUE_SLIDER_RADIUS: f32 = SV_SQUARE_RADIUS - 5.0;
pub const HUE_SLIDER_HEIGHT: f32 = SV_SQUARE_SIZE; 

// ── recent colors ───────────────────────────────────────
pub const RECENT_LABEL: &str = "Recent Colors";
pub const RECENT_LABEL_GAP: f32 = 10.0;
pub const RECENT_LABEL_HEIGHT: f32 = 14.0;
pub const RECENT_LABEL_FONT_SIZE: f32 = 12.0;
pub const RECENT_ROW_GAP: f32 = 6.0;
pub const SWATCH_DIAMETER: f32 = 22.0;
pub const SWATCH_RADIUS: f32 = SWATCH_DIAMETER / 2.0;
pub const SWATCH_GAP: f32 = 8.0;
pub const SWATCH_BORDER: f32 = 1.5;
pub const MAX_RECENT_COLORS: usize = 6;

// ── hex / rgba input fields ──────────────────────────────
pub const FIELD_ROW_GAP: f32 = 10.0;
pub const FIELD_LABEL_WIDTH: f32 = 28.0;
pub const RGBA_LABEL_WIDTH: f32 = 14.0;
pub const FIELD_HEIGHT: f32 = 24.0;
pub const FIELD_FONT_SIZE: f32 = 12.0;
pub const FIELD_GAP: f32 = 6.0;

const RECENT_ROW_OFFSET: f32 =
    COLORPICKER_PADDING + SV_SQUARE_SIZE + RECENT_LABEL_GAP + RECENT_LABEL_HEIGHT + RECENT_ROW_GAP;
const HEX_ROW_OFFSET: f32 = RECENT_ROW_OFFSET + SWATCH_DIAMETER + FIELD_ROW_GAP;
const RGBA_ROW_OFFSET: f32 = HEX_ROW_OFFSET + FIELD_HEIGHT + FIELD_ROW_GAP;

pub const COLORPICKER_HEIGHT: f32 = RGBA_ROW_OFFSET + FIELD_HEIGHT + COLORPICKER_PADDING;

#[derive(Debug, Clone, Copy)]
pub enum ColorPickerItem {}

impl PanelItem for ColorPickerItem {
    fn size(&self) -> f32 { match *self {} }
    fn trailing_padding(&self) -> f32 { match *self {} }
    fn is_button(&self) -> bool { match *self {} }
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

    let s = if max <= f32::EPSILON { 0.0 } else { delta / max };
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
        format!("{:02X}{:02X}{:02X}{:02X}", c8.red(), c8.green(), c8.blue(), c8.alpha())
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

/// default palette when there isn't selection history 
fn default_palette() -> &'static [Color] {
    static PALETTE: OnceLock<Vec<Color>> = OnceLock::new();
    PALETTE.get_or_init(|| {
        vec![
            Color::from_rgba8(0, 0, 0, 255),
            Color::from_rgba8(255, 255, 255, 255),
            Color::from_rgba8(255, 59, 48, 255),
            Color::from_rgba8(255, 149, 0, 255),
            Color::from_rgba8(255, 214, 10, 255),
            Color::from_rgba8(52, 199, 89, 255),
            //Color::from_rgba8(0, 122, 255, 255),
            //Color::from_rgba8(175, 82, 222, 255),
        ]
    })
}

// ─── SV-square state (Content of the pixmap is built by the render) ──────────────
pub struct ColorSquareState {
    pub hue: f32,
    pub sv: (f32, f32),
    pub sv_pixmap: Option<Pixmap>, 
    pub sv_dirty: bool,           
    pub dragging: bool,
}

impl ColorSquareState {
    pub fn new() -> Self {
        Self {
            hue: 0.0,
            sv: (1.0, 1.0),
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
        hsv_to_color(self.hue, self.sv.0, self.sv.1)
    }
}

// ── hue slider geometry ─────────────────────────────────────────
fn marker_visual_radius() -> f32 {
    MARKER_RADIUS + MARKER_STROKE + MARKER_OUTLINE
}

pub fn hue_handle_center_y(track_top: f32, hue: f32) -> f32 {
    let inset = marker_visual_radius();
    let usable = (HUE_SLIDER_HEIGHT - 2.0 * inset).max(1.0);
    let t = hue.rem_euclid(360.0) / 360.0;
    track_top + inset + t * usable
}

pub fn hue_from_pointer_y(track_top: f32, y: f32) -> f32 {
    let inset = marker_visual_radius();
    let usable = (HUE_SLIDER_HEIGHT - 2.0 * inset).max(1.0);
    let t = ((y - track_top - inset) / usable).clamp(0.0, 1.0);
    (t * 360.0).min(359.999)
}

// ── recent colors geometry ──────────────────────────────

pub fn recent_label_origin(content_origin: (f32, f32)) -> (f32, f32) {
    (
        content_origin.0 + COLORPICKER_PADDING,
        content_origin.1 + COLORPICKER_PADDING + SV_SQUARE_SIZE + RECENT_LABEL_GAP,
    )
}

pub fn recent_label_rect(content_origin: (f32, f32)) -> Rect {
    let (x, y) = recent_label_origin(content_origin);
    let width = COLORPICKER_WIDTH - COLORPICKER_PADDING * 2.0;
    Rect::from_xywh(x, y, width, RECENT_LABEL_HEIGHT).expect("recent label rect")
}

pub fn swatch_row_top(content_origin_y: f32) -> f32 {
    content_origin_y + RECENT_ROW_OFFSET
}

pub fn swatch_center(content_origin: (f32, f32), idx: usize) -> (f32, f32) {
    let row_top = swatch_row_top(content_origin.1);
    let cx = content_origin.0
        + COLORPICKER_PADDING
        + SWATCH_RADIUS
        + idx as f32 * (SWATCH_DIAMETER + SWATCH_GAP);
    let cy = row_top + SWATCH_RADIUS;
    (cx, cy)
}

// ── hex / rgba fields geometry ───────────────────────────

pub fn hex_row_top(content_origin_y: f32) -> f32 {
    content_origin_y + HEX_ROW_OFFSET
}

pub fn rgba_row_top(content_origin_y: f32) -> f32 {
    content_origin_y + RGBA_ROW_OFFSET
}

pub fn hex_label_pos(content_origin: (f32, f32)) -> (f32, f32) {
    (content_origin.0 + COLORPICKER_PADDING, hex_row_top(content_origin.1))
}

pub fn hex_field_geom(content_origin: (f32, f32)) -> Rect {
    let x = content_origin.0 + COLORPICKER_PADDING + FIELD_LABEL_WIDTH;
    let y = hex_row_top(content_origin.1);
    let width = COLORPICKER_WIDTH - COLORPICKER_PADDING * 2.0 - FIELD_LABEL_WIDTH;
    Rect::from_xywh(x, y, width, FIELD_HEIGHT).expect("hex field rect")
}

fn rgba_field_total_width() -> f32 {
    (COLORPICKER_WIDTH - COLORPICKER_PADDING * 2.0 - 3.0 * FIELD_GAP) / 4.0
}

pub fn rgba_slot_origin(content_origin: (f32, f32), idx: usize) -> (f32, f32) {
    let total_w = rgba_field_total_width();
    let x = content_origin.0 + COLORPICKER_PADDING + idx as f32 * (total_w + FIELD_GAP);
    let y = rgba_row_top(content_origin.1);
    (x, y)
}

pub fn rgba_field_geom(content_origin: (f32, f32), idx: usize) -> Rect {
    let (slot_x, slot_y) = rgba_slot_origin(content_origin, idx);
    let total_w = rgba_field_total_width();
    Rect::from_xywh(slot_x + RGBA_LABEL_WIDTH, slot_y, (total_w - RGBA_LABEL_WIDTH).max(1.0), FIELD_HEIGHT)
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

    pub recent_colors: Vec<Color>,

    pub fields: TextFieldGroup<ColorField>,
}

impl ColorPickerPopover {
    pub fn new() -> Self {
        Self {
            colorpicker_pixmap: None,
            position: (0.0, 0.0),
            size: (COLORPICKER_WIDTH, COLORPICKER_HEIGHT),
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
            recent_colors: default_palette().to_vec(),
            fields: TextFieldGroup::new(),
        }
    }

    pub fn hit_test(&self, local: (f64, f64)) -> bool {
        let Some(rect) = self.rect() else { return false };
        point_in_rect(local, rect)
    }

    pub fn sv_square_rect(&self) -> Option<Rect> {
        let rect = self.rect()?;
        Rect::from_xywh(
            rect.left() + COLORPICKER_PADDING,
            rect.top() + COLORPICKER_PADDING,
            SV_SQUARE_SIZE,
            SV_SQUARE_SIZE,
        )
    }

    pub fn sv_square_hit(&self, local: (f64, f64)) -> bool {
        self.sv_square_rect().is_some_and(|r| point_in_rect(local, r))
    }

    pub fn set_sv_from_local(&mut self, local: (f64, f64)) {
        let Some(rect) = self.sv_square_rect() else { return };
        let px = local.0 as f32;
        let py = local.1 as f32;
        let s = ((px - rect.left()) / rect.width()).clamp(0.0, 1.0);
        let v = 1.0 - ((py - rect.top()) / rect.height()).clamp(0.0, 1.0);
        self.sv_square.sv = (s, v);
    }

    pub fn hue_slider_rect(&self) -> Option<Rect> {
        let rect = self.rect()?;
        let x = rect.left() + COLORPICKER_PADDING + SV_SQUARE_SIZE + HUE_SLIDER_GAP;
        let y = rect.top() + COLORPICKER_PADDING;
        Rect::from_xywh(x, y, HUE_SLIDER_WIDTH, HUE_SLIDER_HEIGHT)
    }

    pub fn hue_slider_hit(&self, local: (f64, f64)) -> bool {
        let Some(rect) = self.hue_slider_rect() else { return false };
        let px = local.0 as f32;
        let py = local.1 as f32;
        let bleed = (marker_visual_radius() - HUE_SLIDER_WIDTH / 2.0).max(0.0);
        px >= rect.left() - bleed && px <= rect.right() + bleed
            && py >= rect.top() && py <= rect.bottom()
    }

    pub fn set_hue_from_local(&mut self, local: (f64, f64)) {
        let Some(rect) = self.hue_slider_rect() else { return };
        let hue = hue_from_pointer_y(rect.top(), local.1 as f32);
        self.sv_square.set_hue(hue);
    }

    pub fn palette(&self) -> &[Color] {
        &self.recent_colors
    }

    pub fn record_used_color(&mut self, color: Color) {
        let bytes = color_bytes(color);
        self.recent_colors.retain(|c| color_bytes(*c) != bytes);
        self.recent_colors.insert(0, color);
        self.recent_colors.truncate(MAX_RECENT_COLORS);
    }

    pub fn select_color(&mut self, color: Color) {
        let (h, s, v) = color_to_hsv(color);
        self.sv_square.set_hue(h);
        self.sv_square.sv = (s, v);
    }

    pub fn swatch_hit(&self, local: (f64, f64)) -> Option<usize> {
        let rect = self.rect()?;
        let origin = (rect.left(), rect.top());

        for idx in 0..self.palette().len() {
            let (cx, cy) = swatch_center(origin, idx);
            let dx = local.0 as f32 - cx;
            let dy = local.1 as f32 - cy;
            if dx * dx + dy * dy <= SWATCH_RADIUS * SWATCH_RADIUS {
                return Some(idx);
            }
        }
        None
    }

    // ── hex / rgba fields ────────────────────────────────

    pub fn hex_field_rect(&self) -> Option<Rect> {
        let rect = self.rect()?;
        Some(hex_field_geom((rect.left(), rect.top())))
    }

    pub fn hex_field_hit(&self, local: (f64, f64)) -> bool {
        self.hex_field_rect().is_some_and(|r| point_in_rect(local, r))
    }

    pub fn rgba_field_rect(&self, field: ColorField) -> Option<Rect> {
        let rect = self.rect()?;
        let idx = rgba_field_index(field)?;
        Some(rgba_field_geom((rect.left(), rect.top()), idx))
    }

    pub fn rgba_field_hit(&self, local: (f64, f64)) -> Option<ColorField> {
        RGBA_FIELDS
            .into_iter()
            .find(|f| self.rgba_field_rect(*f).is_some_and(|r| point_in_rect(local, r)))
    }

    pub fn field_rect(&self, field: ColorField) -> Option<Rect> {
        match field {
            ColorField::Hex => self.hex_field_rect(),
            _ => self.rgba_field_rect(field),
        }
    }

    pub fn field_text(&self, field: ColorField) -> String {
        if let Some(edit) = self.fields.editing.as_ref() {
            if edit.key == field {
                return edit.field.text.clone();
            }
        }
        self.fields.value(field).cloned().unwrap_or_default()
    }

    pub fn sync_field_values(&mut self) {
        let color = self.sv_square.color();
        self.fields.sync_value(ColorField::Hex, color_to_hex_string(color));
        for field in RGBA_FIELDS {
            self.fields.sync_value(field, color_channel_u8(color, field).to_string());
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
}

impl UiPanel for ColorPickerPopover {
    type Item = ColorPickerItem;

    fn render_pos(&self) -> (f32, f32) { self.render_pos }
    fn size(&self) -> (f32, f32) { self.size }
    fn items(&self) -> &[Self::Item] { &[] }
    fn padding(&self) -> f32 { COLORPICKER_PADDING }
    fn monitor_idx(&self) -> usize { self.monitor_idx }
    fn set_dirty(&mut self) { self.dirty = true; }

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
    fn hovered(&self) -> Self::Hover { self.hovered }
    fn set_hovered(&mut self, hover: Self::Hover) { self.hovered = hover; }
}

impl AnimatedPanel for ColorPickerPopover {
    fn last_tick(&self) -> Option<Instant> { self.last_tick }
    fn set_last_tick(&mut self, at: Instant) { self.last_tick = Some(at); }

    fn anim_interval(&self) -> Duration { COLORPICKER_ANIM_INTERVAL }
    fn anim_dt(&self) -> f32 { COLORPICKER_ANIM_DT }

    fn is_animating(&self) -> bool {
        let target = if self.open { 1.0 } else { 0.0 };
        (self.opacity - target).abs() > 0.001
    }

    fn animate_step(&mut self, dt: f32) -> bool {
        let target = if self.open { 1.0 } else { 0.0 };
        if (self.opacity - target).abs() > 0.001 {
            let delta = 8.0 * dt;
            self.opacity += (target - self.opacity).signum() * delta;
            self.opacity = self.opacity.clamp(0.0, 1.0);
            true
        } else {
            false
        }
    }
}