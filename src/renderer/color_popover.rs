use crate::types::color_popover::{
    hex_field_geom, hex_label_pos, hsv_to_color, hue_handle_center_y, recent_label_rect,
    rgba_field_geom, rgba_slot_origin, swatch_center, ColorField, ColorPickerPopover,
    ColorPopoverElement, ColorSquareState, COLORPICKER_PADDING, COLOR_POPOVER_ITEM_BORDER,
    FIELD_FONT_SIZE, FIELD_HEIGHT, FIELD_LABEL_WIDTH, HUE_SLIDER_GAP, HUE_SLIDER_WIDTH,
    HUE_SLIDER_HEIGHT, HUE_SLIDER_RADIUS, MARKER_OUTLINE, MARKER_RADIUS, MARKER_STROKE,
    RECENT_LABEL, RECENT_LABEL_FONT_SIZE, RGBA_FIELDS, RGBA_LABEL_WIDTH, SV_SQUARE_SIZE,
    SV_SQUARE_RADIUS, SWATCH_RADIUS, COLORPICKER_RADIUS,
};
use crate::types::panel::{UiPanel, PANEL_COLOR, ICON_COLOR, DEFAULT_ITEM_BORDER_STROKE};
use super::paths::{rounded_rect_path, draw_panel_border, draw_item_border};
use super::text::{draw_aligned_text, draw_input_box, draw_line_edit, HAlign};
use tiny_skia::{
    BlendMode, Color, FillRule, FilterQuality, GradientStop, LinearGradient, Mask, Paint,
    PathBuilder, Pixmap, PixmapPaint, Point, Rect, SpreadMode, Stroke, Transform,
};
use cosmic_text::{FontSystem, SwashCache, Weight};

pub fn draw_color_popover(
    canvas: &mut Pixmap,
    color_popover: &mut ColorPickerPopover,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) {
    let Some(cp_rect) = color_popover.rect() else { return };

    let x = cp_rect.left();
    let y = cp_rect.top();
    let (w, h) = color_popover.size;
    let pw = w.ceil() as u32;
    let ph = h.ceil() as u32;

    let needs_resize = color_popover
        .colorpicker_pixmap
        .as_ref()
        .is_none_or(|p| p.width() != pw || p.height() != ph);

    if needs_resize {
        color_popover.colorpicker_pixmap = Pixmap::new(pw, ph);
    }

    let Some(mut popover_pixmap) = color_popover.colorpicker_pixmap.take() else {
        return;
    };

    if color_popover.dirty {
        popover_pixmap.fill(Color::TRANSPARENT);
        draw_color_popover_content(&mut popover_pixmap, color_popover, font_system, swash_cache);
    }

    canvas.draw_pixmap(
        x as i32,
        y as i32,
        popover_pixmap.as_ref(),
        &PixmapPaint {
            opacity: color_popover.opacity,
            blend_mode: BlendMode::SourceOver,
            quality: FilterQuality::Nearest,
        },
        Transform::identity(),
        None,
    );

    draw_panel_border(canvas, x, y, w, h, COLORPICKER_RADIUS, color_popover.opacity);

    color_popover.colorpicker_pixmap = Some(popover_pixmap);
}

fn build_rounded_clip_mask(
    canvas_w: f32,
    canvas_h: f32,
    item_x: f32,
    item_y: f32,
    item_w: f32,
    item_h: f32,
    radius: f32,
) -> Option<Mask> {
    let item_rect = Rect::from_xywh(
        item_x + 0.5, item_y + 0.5,
        (item_w - 1.0).max(0.1), (item_h - 1.0).max(0.1),
    )?;
    let path = rounded_rect_path(&item_rect, radius, true, true, true, true)?;
    let mut mask = Mask::new(canvas_w.ceil() as u32, canvas_h.ceil() as u32)?;
    mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
    Some(mask)
}

fn draw_color_popover_content(
    canvas: &mut Pixmap,
    color_popover: &mut ColorPickerPopover,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) {
    let (w, h) = color_popover.size;
    let Some(rect) = Rect::from_xywh(0.0, 0.0, w, h) else { return };

    let Some(path) = rounded_rect_path(&rect, COLORPICKER_RADIUS, true, true, true, true) else {
        return;
    };

    let mut paint = Paint::default();
    paint.set_color(PANEL_COLOR);
    paint.anti_alias = true;
    canvas.fill_path(&path, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);

    // ── sv square ────────────────────────────────────────
    let square_origin = (COLORPICKER_PADDING, COLORPICKER_PADDING);

    let mut sv_clip = color_popover.sv_clip_mask.take();
    if sv_clip.is_none() {
        sv_clip = build_rounded_clip_mask(
            w, h, square_origin.0, square_origin.1, SV_SQUARE_SIZE, SV_SQUARE_SIZE, SV_SQUARE_RADIUS,
        );
    }

    let sv_pixmap = ensure_sv_pixmap(&mut color_popover.sv_square);
    blit_sv_square(canvas, sv_pixmap, square_origin, sv_clip.as_ref());
    color_popover.sv_clip_mask = sv_clip;

    let sv_hovered = matches!(color_popover.hovered, Some(ColorPopoverElement::SvSquare));
    draw_item_border(
        canvas, square_origin.0, square_origin.1, SV_SQUARE_SIZE, SV_SQUARE_SIZE,
        SV_SQUARE_RADIUS, COLOR_POPOVER_ITEM_BORDER, sv_hovered, color_popover.sv_square.dragging,
    );

    let sv = color_popover.sv_square.sv;
    let sv_marker_cx = square_origin.0 + sv.0 * SV_SQUARE_SIZE;
    let sv_marker_cy = square_origin.1 + (1.0 - sv.1) * SV_SQUARE_SIZE;
    draw_selection_marker(canvas, sv_marker_cx, sv_marker_cy, color_popover.sv_square.color());

    // ── hue slider ───────────────────────────────────────
    let track_origin = (
        COLORPICKER_PADDING + SV_SQUARE_SIZE + HUE_SLIDER_GAP,
        COLORPICKER_PADDING,
    );
    let hue = color_popover.sv_square.hue;

    let mut hue_clip = color_popover.hue_clip_mask.take();
    if hue_clip.is_none() {
        hue_clip = build_rounded_clip_mask(
            w, h, track_origin.0, track_origin.1, HUE_SLIDER_WIDTH, HUE_SLIDER_HEIGHT, HUE_SLIDER_RADIUS,
        );
    }

    let hue_track = ensure_hue_track(color_popover);
    blit_hue_track(canvas, hue_track, track_origin, hue_clip.as_ref());
    color_popover.hue_clip_mask = hue_clip;

    let hue_hovered = matches!(color_popover.hovered, Some(ColorPopoverElement::HueSlider));
    draw_item_border(
        canvas, track_origin.0, track_origin.1, HUE_SLIDER_WIDTH, HUE_SLIDER_HEIGHT,
        HUE_SLIDER_RADIUS, COLOR_POPOVER_ITEM_BORDER, hue_hovered, color_popover.hue_dragging,
    );

    let hue_marker_cx = track_origin.0 + HUE_SLIDER_WIDTH / 2.0;
    let hue_marker_cy = hue_handle_center_y(track_origin.1, hue);
    draw_selection_marker(canvas, hue_marker_cx, hue_marker_cy, hsv_to_color(hue, 1.0, 1.0));

    draw_recent_colors(canvas, color_popover, font_system, swash_cache);
    draw_color_fields(canvas, color_popover, font_system, swash_cache);
}

fn ensure_sv_pixmap(square: &mut ColorSquareState) -> &Pixmap {
    if square.sv_dirty || square.sv_pixmap.is_none() {
        let side = SV_SQUARE_SIZE as u32;
        square.sv_pixmap = Some(build_sv_pixmap(square.hue, side, side));
        square.sv_dirty = false;
    }
    square.sv_pixmap.as_ref().unwrap()
}

fn build_sv_pixmap(hue: f32, w: u32, h: u32) -> Pixmap {
    let mut pm = Pixmap::new(w.max(1), h.max(1)).expect("sv pixmap dims");
    let rect = Rect::from_xywh(0.0, 0.0, w as f32, h as f32).unwrap();
    let identity = Transform::identity();

    let mut paint = Paint::default();
    paint.set_color(hsv_to_color(hue, 1.0, 1.0));
    pm.fill_rect(rect, &paint, identity, None);

    let sat = LinearGradient::new(
        Point::from_xy(0.0, 0.0),
        Point::from_xy(w as f32, 0.0),
        vec![
            GradientStop::new(0.0, Color::WHITE),
            GradientStop::new(1.0, Color::from_rgba8(255, 255, 255, 0)),
        ],
        SpreadMode::Pad,
        identity,
    ).expect("sat gradient");
    let mut paint = Paint::default();
    paint.shader = sat;
    pm.fill_rect(rect, &paint, identity, None);

    let val = LinearGradient::new(
        Point::from_xy(0.0, 0.0),
        Point::from_xy(0.0, h as f32),
        vec![
            GradientStop::new(0.0, Color::from_rgba8(0, 0, 0, 0)),
            GradientStop::new(1.0, Color::BLACK),
        ],
        SpreadMode::Pad,
        identity,
    ).expect("val gradient");
    let mut paint = Paint::default();
    paint.shader = val;
    pm.fill_rect(rect, &paint, identity, None);

    pm
}

fn ensure_hue_track(color_popover: &mut ColorPickerPopover) -> &Pixmap {
    if color_popover.hue_track_pixmap.is_none() {
        let w = HUE_SLIDER_WIDTH.ceil() as u32;
        let h = HUE_SLIDER_HEIGHT.ceil() as u32;
        color_popover.hue_track_pixmap = Some(build_hue_track_pixmap(w, h));
    }
    color_popover.hue_track_pixmap.as_ref().unwrap()
}

fn build_hue_track_pixmap(w: u32, h: u32) -> Pixmap {
    let mut pm = Pixmap::new(w.max(1), h.max(1)).expect("hue track pixmap dims");
    let rect = Rect::from_xywh(0.0, 0.0, w as f32, h as f32).unwrap();

    let stops: Vec<GradientStop> = (0..=6)
        .map(|i| {
            let hue = i as f32 * 60.0;
            GradientStop::new(i as f32 / 6.0, hsv_to_color(hue, 1.0, 1.0))
        })
        .collect();

    let gradient = LinearGradient::new(
        Point::from_xy(0.0, 0.0),
        Point::from_xy(0.0, h as f32),
        stops,
        SpreadMode::Pad,
        Transform::identity(),
    ).expect("hue track gradient");

    let mut paint = Paint::default();
    paint.shader = gradient;
    pm.fill_rect(rect, &paint, Transform::identity(), None);

    pm
}

fn draw_selection_marker(pm: &mut Pixmap, cx: f32, cy: f32, fill: Color) {
    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, MARKER_RADIUS);
    let Some(disc) = pb.finish() else { return };

    let mut fill_paint = Paint::default();
    fill_paint.set_color(fill);
    fill_paint.anti_alias = true;
    pm.fill_path(&disc, &fill_paint, tiny_skia::FillRule::Winding, Transform::identity(), None);

    let mut ring_paint = Paint::default();
    ring_paint.set_color(Color::WHITE);
    ring_paint.anti_alias = true;
    let ring_stroke = Stroke { width: MARKER_STROKE, ..Default::default() };
    pm.stroke_path(&disc, &ring_paint, &ring_stroke, Transform::identity(), None);

    let mut pb2 = PathBuilder::new();
    pb2.push_circle(cx, cy, MARKER_RADIUS + MARKER_STROKE);
    let Some(outline) = pb2.finish() else { return };
    let mut outline_paint = Paint::default();
    outline_paint.set_color(Color::from_rgba8(0, 0, 0, 160));
    outline_paint.anti_alias = true;
    let outline_stroke = Stroke { width: MARKER_OUTLINE, ..Default::default() };
    pm.stroke_path(&outline, &outline_paint, &outline_stroke, Transform::identity(), None);
}

fn blit_sv_square(pm: &mut Pixmap, sv_pixmap: &Pixmap, square_origin: (f32, f32), clip: Option<&Mask>) {
    pm.draw_pixmap(
        square_origin.0 as i32,
        square_origin.1 as i32,
        sv_pixmap.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        clip,
    );
}

fn blit_hue_track(pm: &mut Pixmap, track_pixmap: &Pixmap, track_origin: (f32, f32), clip: Option<&Mask>) {
    pm.draw_pixmap(
        track_origin.0 as i32,
        track_origin.1 as i32,
        track_pixmap.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        clip,
    );
}

fn draw_recent_colors(
    canvas: &mut Pixmap,
    color_popover: &ColorPickerPopover,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) {
    let origin = (0.0, 0.0);
    let label_color = Color::from_rgba8(200, 200, 205, 160);

    draw_aligned_text(
        canvas, RECENT_LABEL, font_system, swash_cache,
        recent_label_rect(origin), RECENT_LABEL_FONT_SIZE, label_color,
        HAlign::Left, (0.0, 0.0), Weight::NORMAL, 
        cosmic_text::Style::Oblique
    );

    for (idx, color) in color_popover.palette().iter().enumerate() {
        let (cx, cy) = swatch_center(origin, idx);
        let is_hovered = matches!(color_popover.hovered, Some(ColorPopoverElement::Swatch(i)) if i == idx);
        draw_swatch(canvas, cx, cy, *color, is_hovered);
    }
}

fn draw_swatch(pm: &mut Pixmap, cx: f32, cy: f32, color: Color, is_hovered: bool) {
    let radius = if is_hovered { SWATCH_RADIUS + 3.0 } else { SWATCH_RADIUS };

    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, radius);
    let Some(fill) = pb.finish() else { return };

    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    pm.fill_path(&fill, &paint, tiny_skia::FillRule::Winding, Transform::identity(), None);
}

fn draw_color_fields(
    canvas: &mut Pixmap,
    color_popover: &ColorPickerPopover,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) {
    let origin = (0.0, 0.0);
    let label_color = Color::from_rgba8(200, 200, 205, 160);

    let (label_x, label_y) = hex_label_pos(origin);
    let hex_label_rect = Rect::from_xywh(label_x, label_y, FIELD_LABEL_WIDTH, FIELD_HEIGHT).unwrap();
    draw_aligned_text(
        canvas, "Hex", font_system, swash_cache, hex_label_rect,
        FIELD_FONT_SIZE, label_color, HAlign::Left, (0.0, 0.0), Weight::NORMAL,
        cosmic_text::Style::Normal, 
    );

    let hex_rect = hex_field_geom(origin);
    draw_color_input_field(canvas, color_popover, ColorField::Hex, hex_rect, font_system, swash_cache);

    for (idx, field) in RGBA_FIELDS.into_iter().enumerate() {
        let (lx, ly) = rgba_slot_origin(origin, idx);
        let label_rect = Rect::from_xywh(lx, ly, RGBA_LABEL_WIDTH, FIELD_HEIGHT).unwrap();
        draw_aligned_text(
            canvas, field_label(field), font_system, swash_cache, label_rect,
            FIELD_FONT_SIZE, label_color, HAlign::Left, (0.0, 0.0), Weight::NORMAL,
            cosmic_text::Style::Normal, 
        );

        let field_rect = rgba_field_geom(origin, idx);
        draw_color_input_field(canvas, color_popover, field, field_rect, font_system, swash_cache);
    }
}

fn draw_color_input_field(
    canvas: &mut Pixmap,
    color_popover: &ColorPickerPopover,
    field: ColorField,
    box_rect: Rect,
    font_system: &mut FontSystem,
    swash_cache: &mut SwashCache,
) {
    draw_input_box(canvas, box_rect, 4.0);

    let is_hovered = matches!(color_popover.hovered, Some(ColorPopoverElement::Field(f)) if f == field);
    let is_editing = color_popover.fields.is_editing_key(field);
    draw_item_border(
        canvas, box_rect.left(), box_rect.top(), box_rect.width(), box_rect.height(),
        4.0, DEFAULT_ITEM_BORDER_STROKE, is_hovered, is_editing,
    );

    let inset_rect = Rect::from_xywh(
        box_rect.left() + 6.0, box_rect.top(),
        (box_rect.width() - 8.0).max(1.0), box_rect.height(),
    ).unwrap_or(box_rect);

    let editing = color_popover.fields.editing.as_ref()
        .filter(|e| e.key == field)
        .map(|e| &e.field);

    let text_color = Color::from_rgba8(ICON_COLOR.red, ICON_COLOR.green, ICON_COLOR.blue, 255);

    draw_line_edit(
        canvas, inset_rect, &color_popover.field_text(field), editing,
        font_system, swash_cache, FIELD_FONT_SIZE, text_color, cosmic_text::Weight::BOLD,
    );
}

fn field_label(field: ColorField) -> &'static str {
    match field {
        ColorField::Hex => "Hex",
        ColorField::R => "R",
        ColorField::G => "G",
        ColorField::B => "B",
        ColorField::A => "A",
    }
}