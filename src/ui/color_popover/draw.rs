use crate::config::ColorPicker;
use crate::renderer::Painter;
use crate::renderer::paths::rounded_rect_path;
use crate::renderer::text::TextStyle;
use crate::theme::{color, radius};
use crate::ui::color_popover::{
    ColorField, ColorPickerPopover, ColorPopoverElement, ColorSquareState, RECENT_LABEL,
    RGBA_FIELDS, eyedropper_center, field_font_size, hex_field_geom, hex_label_pos, hsv_to_color,
    hue_handle_center_y, hue_track_origin, recent_label_rect, rgba_field_geom, rgba_slot_origin,
    sv_square_origin, swatch_center, swatch_radius,
};
use crate::ui::panel::UiPanel;
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Mask, Paint, PathBuilder, Pixmap, PixmapPaint,
    Point, Rect, SpreadMode, Stroke, Transform,
};

fn cfg() -> &'static ColorPicker {
    &crate::config::get().color_picker
}

pub fn draw_color_popover(p: &mut Painter, color_popover: &mut ColorPickerPopover) {
    let Some(cp_rect) = color_popover.rect() else {
        return;
    };

    let mut cache = color_popover.colorpicker_pixmap.take();
    p.panel(
        &mut cache,
        (cp_rect.left(), cp_rect.top()),
        color_popover.size,
        color_popover.opacity,
        |p| draw_color_popover_content(p, color_popover),
    );
    color_popover.colorpicker_pixmap = cache;
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
        item_x + 0.5,
        item_y + 0.5,
        (item_w - 1.0).max(0.1),
        (item_h - 1.0).max(0.1),
    )?;
    let path = rounded_rect_path(&item_rect, radius, true, true, true, true)?;
    let mut mask = Mask::new(canvas_w.ceil() as u32, canvas_h.ceil() as u32)?;
    mask.fill_path(&path, FillRule::Winding, true, Transform::identity());
    Some(mask)
}

fn draw_color_popover_content(p: &mut Painter, color_popover: &mut ColorPickerPopover) {
    let (w, h) = color_popover.size;
    let Some(rect) = Rect::from_xywh(0.0, 0.0, w, h) else {
        return;
    };

    let cfg = cfg();
    p.fill_rounded(rect, radius::panel(), cfg.background.get());

    // ── sv square ────────────────────────────────────────
    let square_origin = sv_square_origin((0.0, 0.0));

    let mut sv_clip = color_popover.sv_clip_mask.take();
    if sv_clip.is_none() {
        sv_clip = build_rounded_clip_mask(
            w,
            h,
            square_origin.0,
            square_origin.1,
            cfg.sv_size,
            cfg.sv_size,
            cfg.sv_radius,
        );
    }

    let sv_pixmap = ensure_sv_pixmap(&mut color_popover.sv_square);
    blit_sv_square(p, sv_pixmap, square_origin, sv_clip.as_ref());
    color_popover.sv_clip_mask = sv_clip;

    let sv_hovered = matches!(color_popover.hovered, Some(ColorPopoverElement::SvSquare));
    p.item_border(
        square_origin.0,
        square_origin.1,
        cfg.sv_size,
        cfg.sv_size,
        cfg.sv_radius,
        cfg.border_width,
        sv_hovered,
        color_popover.sv_square.dragging,
    );

    let sv = color_popover.sv_square.sv;
    let sv_marker_cx = square_origin.0 + sv.0 * cfg.sv_size;
    let sv_marker_cy = square_origin.1 + (1.0 - sv.1) * cfg.sv_size;
    draw_selection_marker(
        p,
        sv_marker_cx,
        sv_marker_cy,
        color_popover.sv_square.color(),
    );

    // ── hue slider ───────────────────────────────────────
    let track_origin = hue_track_origin((0.0, 0.0));
    let hue = color_popover.sv_square.hue;

    let mut hue_clip = color_popover.hue_clip_mask.take();
    if hue_clip.is_none() {
        hue_clip = build_rounded_clip_mask(
            w,
            h,
            track_origin.0,
            track_origin.1,
            cfg.hue_width,
            cfg.sv_size,
            cfg.hue_radius,
        );
    }

    let hue_track = ensure_hue_track(color_popover);
    blit_hue_track(p, hue_track, track_origin, hue_clip.as_ref());
    color_popover.hue_clip_mask = hue_clip;

    let hue_hovered = matches!(color_popover.hovered, Some(ColorPopoverElement::HueSlider));
    p.item_border(
        track_origin.0,
        track_origin.1,
        cfg.hue_width,
        cfg.sv_size,
        cfg.hue_radius,
        cfg.border_width,
        hue_hovered,
        color_popover.hue_dragging,
    );

    let hue_marker_cx = track_origin.0 + cfg.hue_width / 2.0;
    let hue_marker_cy = hue_handle_center_y(track_origin.1, hue);
    draw_selection_marker(p, hue_marker_cx, hue_marker_cy, hsv_to_color(hue, 1.0, 1.0));

    draw_recent_colors(p, color_popover);
    draw_color_fields(p, color_popover);
}

fn ensure_sv_pixmap(square: &mut ColorSquareState) -> &Pixmap {
    if square.sv_dirty || square.sv_pixmap.is_none() {
        let side = cfg().sv_size as u32;
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
    )
    .expect("sat gradient");
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
    )
    .expect("val gradient");
    let mut paint = Paint::default();
    paint.shader = val;
    pm.fill_rect(rect, &paint, identity, None);

    pm
}

fn ensure_hue_track(color_popover: &mut ColorPickerPopover) -> &Pixmap {
    if color_popover.hue_track_pixmap.is_none() {
        let w = cfg().hue_width.ceil() as u32;
        let h = cfg().sv_size.ceil() as u32;
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
    )
    .expect("hue track gradient");

    let mut paint = Paint::default();
    paint.shader = gradient;
    pm.fill_rect(rect, &paint, Transform::identity(), None);

    pm
}

fn draw_selection_marker(p: &mut Painter, cx: f32, cy: f32, fill: Color) {
    let cfg = cfg();
    let mut pb = PathBuilder::new();
    pb.push_circle(cx, cy, cfg.marker_radius);
    let Some(disc) = pb.finish() else { return };

    p.fill_path(&disc, fill);

    let ring_stroke = Stroke {
        width: cfg.marker_stroke,
        ..Default::default()
    };
    p.stroke_path(&disc, cfg.marker.get(), &ring_stroke);

    let mut pb2 = PathBuilder::new();
    pb2.push_circle(cx, cy, cfg.marker_radius + cfg.marker_stroke);
    let Some(outline) = pb2.finish() else { return };
    let outline_stroke = Stroke {
        width: cfg.marker_outline,
        ..Default::default()
    };
    p.stroke_path(&outline, cfg.marker_outline_color, &outline_stroke);
}

fn blit_sv_square(
    p: &mut Painter,
    sv_pixmap: &Pixmap,
    square_origin: (f32, f32),
    clip: Option<&Mask>,
) {
    p.canvas().draw_pixmap(
        square_origin.0 as i32,
        square_origin.1 as i32,
        sv_pixmap.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        clip,
    );
}

fn blit_hue_track(
    p: &mut Painter,
    track_pixmap: &Pixmap,
    track_origin: (f32, f32),
    clip: Option<&Mask>,
) {
    p.canvas().draw_pixmap(
        track_origin.0 as i32,
        track_origin.1 as i32,
        track_pixmap.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        clip,
    );
}

fn draw_recent_colors(p: &mut Painter, color_popover: &ColorPickerPopover) {
    let origin = (0.0, 0.0);
    let label_color = cfg().label.get().color();

    p.text(
        recent_label_rect(origin),
        RECENT_LABEL,
        TextStyle::new(cfg().label_font_size, label_color).style(cosmic_text::Style::Oblique),
    );

    for (idx, color) in color_popover.palette().iter().enumerate() {
        let (cx, cy) = swatch_center(origin, idx);
        let is_hovered =
            matches!(color_popover.hovered, Some(ColorPopoverElement::Swatch(i)) if i == idx);
        draw_swatch(p, cx, cy, *color, is_hovered);
    }

    let (cx, cy) = eyedropper_center(origin);
    let is_hovered = color_popover.hovered == Some(ColorPopoverElement::Eyedropper);
    draw_eyedropper(p, cx, cy, is_hovered, color_popover.picking);
}

fn draw_eyedropper(p: &mut Painter, cx: f32, cy: f32, is_hovered: bool, is_picking: bool) {
    let fill = if is_picking {
        color::hover()
    } else {
        color::field()
    };
    p.fill_circle(cx, cy, swatch_radius(), fill);

    let tint = if is_picking || is_hovered {
        color::foreground()
    } else {
        color::muted()
    };
    let icon = cfg().eyedropper_icon_size;
    p.icon(
        crate::ui::icons::EYEDROPPER,
        icon,
        cx - icon / 2.0,
        cy - icon / 2.0,
        tint.usvg(),
    );
}

fn draw_swatch(p: &mut Painter, cx: f32, cy: f32, color: Color, is_hovered: bool) {
    let radius = if is_hovered {
        swatch_radius() + cfg().swatch_hover_grow
    } else {
        swatch_radius()
    };

    p.fill_circle(cx, cy, radius, color);
}

fn draw_color_fields(p: &mut Painter, color_popover: &ColorPickerPopover) {
    let cfg = cfg();
    let origin = (0.0, 0.0);
    let label_color = cfg.label.get().color();

    let (label_x, label_y) = hex_label_pos(origin);
    let hex_label_rect =
        Rect::from_xywh(label_x, label_y, cfg.hex_label_width, cfg.field_height).unwrap();
    p.text(
        hex_label_rect,
        "Hex",
        TextStyle::new(field_font_size(), label_color),
    );

    let hex_rect = hex_field_geom(origin);
    draw_color_input_field(p, color_popover, ColorField::Hex, hex_rect);

    for (idx, field) in RGBA_FIELDS.into_iter().enumerate() {
        let (lx, ly) = rgba_slot_origin(origin, idx);
        let label_rect = Rect::from_xywh(lx, ly, cfg.rgba_label_width, cfg.field_height).unwrap();
        p.text(
            label_rect,
            field_label(field),
            TextStyle::new(field_font_size(), label_color),
        );

        let field_rect = rgba_field_geom(origin, idx);
        draw_color_input_field(p, color_popover, field, field_rect);
    }
}

fn draw_color_input_field(
    p: &mut Painter,
    color_popover: &ColorPickerPopover,
    field: ColorField,
    box_rect: Rect,
) {
    let cfg = cfg();
    p.input_box(box_rect, cfg.field_radius);

    let is_hovered =
        matches!(color_popover.hovered, Some(ColorPopoverElement::Field(f)) if f == field);
    let is_editing = color_popover.fields.is_editing_key(field);
    p.item_border(
        box_rect.left(),
        box_rect.top(),
        box_rect.width(),
        box_rect.height(),
        cfg.field_radius,
        cfg.border_width,
        is_hovered,
        is_editing,
    );

    let inset_rect = Rect::from_xywh(
        box_rect.left() + cfg.field_text_inset,
        box_rect.top(),
        (box_rect.width() - cfg.field_text_inset - 2.0).max(1.0),
        box_rect.height(),
    )
    .unwrap_or(box_rect);

    let editing = color_popover
        .fields
        .editing
        .as_ref()
        .filter(|e| e.key == field)
        .map(|e| &e.field);

    let text_color = cfg.text.get().color();

    p.line_edit(
        inset_rect,
        &color_popover.field_text(field),
        editing,
        field_font_size(),
        text_color,
        cosmic_text::Weight::BOLD,
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
