// edit mode in its own window: the overlay's editor with one monitor,
// the window, which shows the picture of `EditorState.sheet` under the ui
use std::collections::HashMap;
use std::path::PathBuf;

use cosmic_text::SwashCache;
use tiny_skia::{Pixmap, Rect};

use super::init;
use crate::backend::ScreenOverlay;
use crate::backend::wayland::overlay::WaylandOverlay;
use crate::edit::{Sheet, TITLE, TOUCHPAD_ZOOM, WHEEL_STEP, ZOOM_STEP};
use crate::editor::dirty::{apply_damage_rects, mark_dirty};
use crate::editor::{EditorState, Layers, TextState};
use crate::keys::Action;
use crate::types::{Capture, Outputs, Placement};

pub async fn run(
    conn: wayland_client::Connection,
    image: Pixmap,
    outputs: Outputs,
    output: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let icons = std::thread::spawn(init::load_icons_cache);
    let system_fonts = std::thread::spawn(crate::fonts::system);

    let mut window = WaylandOverlay::new(conn.clone(), false)?;
    let picture = (image.width(), image.height());
    let (size, monitor_scale) = crate::edit::initial_size(
        window.discovered_outputs(),
        picture,
        crate::theme::size::panel_height(),
    );
    window.open_window(TITLE, size, monitor_scale)?;
    let ((w, h), scale) = window.window_size().ok_or("the window got no size")?;

    let sheet = Sheet::new(&image, monitor_scale);
    let placement = Placement {
        position: (0, 0),
        size: (w as i32, h as i32),
        scale,
    };
    let (canvas, dimmed, annotations) = init::build_layers(&[placement]);
    let capture = Capture {
        scale: sheet.scale(),
        pixmap: image,
    };
    let mut editor_state = EditorState::new(
        Layers {
            captures: vec![capture],
            canvas,
            dimmed,
            annotations,
        },
        vec![placement],
        icons.join().map_err(|_| "icons thread panicked")?,
        TextState {
            font_system: crate::fonts::bundled(),
            system_fonts: Some(system_fonts),
            swash_cache: SwashCache::new(),
            editors: HashMap::new(),
            editing: None,
        },
        super::build_ocr_state(),
    );
    editor_state.toolbar.docked = true;
    editor_state.sheet = Some(sheet);
    let inset = super::panels::toolbar::docked_inset(&editor_state);
    if let Some(sheet) = editor_state.sheet.as_mut() {
        sheet.resize(placement.px_size(), inset, 1.0);
    }
    editor_state.selection.zone = editor_state.workspace();
    editor_state.to = outputs;
    editor_state.toolbar.to = outputs;
    editor_state.output = output;
    window.set_min_size(super::panels::toolbar::min_window_size(
        editor_state.toolbar.natural_size(),
    ));

    let mut overlay: Box<dyn ScreenOverlay> = Box::new(window);
    super::run_overlay(&conn, &mut editor_state, &mut overlay).await?;
    drop(overlay);
    let _ = conn.flush();

    crate::editor::saved::save(
        &editor_state.tool_settings,
        &editor_state.color_popover.history,
        &editor_state.color_popover.initial_history,
        !editor_state.magnifier.hidden,
    );
    editor_state.ocr.models.shutdown();
    super::finish::finish_capture(&mut editor_state).await
}

/// new size or scale: the buffer is new either way and has to be fully redrawn
pub fn resized(editor_state: &mut EditorState, size: (u32, u32), scale: f32, dirty_mask: &mut u32) {
    if editor_state.sheet.is_none() {
        return;
    }
    let placement = Placement {
        position: (0, 0),
        size: (size.0 as i32, size.1 as i32),
        scale,
    };
    let old = std::mem::replace(&mut editor_state.placements[0], placement);
    let inset = super::panels::toolbar::docked_inset(editor_state);
    let Some(sheet) = editor_state.sheet.as_mut() else {
        return;
    };
    if old != placement {
        let (w, h) = placement.px_size();
        sheet.resize((w, h), inset, scale / old.scale);
        for layer in [
            &mut editor_state.canvas[0],
            &mut editor_state.dimmed[0],
            &mut editor_state.annotations_layer[0],
        ] {
            if let Some(pixmap) = Pixmap::new(w, h) {
                *layer = pixmap;
            }
        }
    }
    view_changed(editor_state, dirty_mask);
    super::input::refresh_panels(editor_state, dirty_mask);
}

pub fn wheel(
    editor_state: &mut EditorState,
    delta: (f32, f32),
    wheel: Option<(f32, f32)>,
    dirty_mask: &mut u32,
) -> bool {
    let input = &editor_state.input;
    let (ctrl, shift, alt) = (input.ctrl, input.shift, input.alt);
    let scale = editor_state.placements[0].scale;
    let pointer = device(input.pointer.local, scale);
    let Some(sheet) = editor_state.sheet.as_mut().filter(|_| !alt) else {
        return false;
    };
    let moved = if ctrl {
        let clicks = wheel.map_or(delta.1 / TOUCHPAD_ZOOM, |w| w.1);
        let moved = sheet.view.zoom_at(ZOOM_STEP.powf(clicks), pointer);
        sheet.moved(moved)
    } else {
        let (dx, dy) = match wheel {
            Some((x, y)) => (-x * WHEEL_STEP, y * WHEEL_STEP),
            None => (-delta.0, delta.1),
        };
        let (dx, dy) = if shift { (dy, dx) } else { (dx, dy) };
        sheet.pan((dx * scale, dy * scale))
    };
    if moved {
        view_changed(editor_state, dirty_mask);
    }
    true
}

pub fn zoom_key(editor_state: &mut EditorState, action: Action, dirty_mask: &mut u32) {
    let Some(sheet) = editor_state.sheet.as_mut() else {
        return;
    };
    let center = sheet.view.center();
    let moved = match action {
        Action::ZoomIn => sheet.view.zoom_at(ZOOM_STEP, center),
        Action::ZoomOut => sheet.view.zoom_at(1.0 / ZOOM_STEP, center),
        Action::ZoomActual => sheet.view.zoom_to(1.0, center),
        Action::ZoomFit => {
            sheet.view.fit();
            true
        }
        _ => false,
    };
    if sheet.moved(moved) {
        view_changed(editor_state, dirty_mask);
    }
}

pub fn grab(editor_state: &mut EditorState, pressed: bool) -> bool {
    let local = editor_state.input.pointer.local;
    let Some(sheet) = editor_state.sheet.as_mut() else {
        return false;
    };
    sheet.dragging = pressed.then_some(local);
    true
}

pub fn drag(editor_state: &mut EditorState, at: (f64, f64), dirty_mask: &mut u32) {
    let scale = editor_state.placements[0].scale;
    let Some(sheet) = editor_state.sheet.as_mut() else {
        return;
    };
    let Some(from) = sheet.dragging else {
        return;
    };
    sheet.dragging = Some(at);
    let (from, to) = (device(from, scale), device(at, scale));
    if sheet.pan((to.0 - from.0, to.1 - from.1)) {
        view_changed(editor_state, dirty_mask);
    }
}

pub fn hand(editor_state: &mut EditorState, held: bool) -> bool {
    let Some(sheet) = editor_state.sheet.as_mut() else {
        return false;
    };
    sheet.hand = held;
    true
}

pub fn hand_button(editor_state: &mut EditorState, pressed: bool) -> bool {
    let grabbing = editor_state
        .sheet
        .as_ref()
        .is_some_and(|sheet| sheet.hand_grab);
    let start = pressed && super::input::hand_over_picture(editor_state);
    if !(start || (grabbing && !pressed)) {
        return false;
    }
    grab(editor_state, pressed);
    if let Some(sheet) = editor_state.sheet.as_mut() {
        sheet.hand_grab = pressed;
    }
    true
}

pub fn hand_held(editor_state: &EditorState) -> bool {
    editor_state.sheet.as_ref().is_some_and(|sheet| sheet.hand)
}

pub fn grabbing(editor_state: &EditorState) -> bool {
    editor_state
        .sheet
        .as_ref()
        .is_some_and(|sheet| sheet.dragging.is_some())
}

pub fn paint(editor_state: &mut EditorState) -> bool {
    let Some(sheet) = editor_state.sheet.as_mut().filter(|sheet| sheet.stale) else {
        return false;
    };
    let backdrop = crate::theme::color::panel().with_alpha(255).color();
    sheet.paint(&editor_state.captures[0].pixmap, backdrop);
    sheet.stale = false;
    true
}

/// everything on the picture moved: the annotations layer is drawn anew,
/// and the tool sees the pointer over another point of the picture
fn view_changed(editor_state: &mut EditorState, dirty_mask: &mut u32) {
    let (w, h) = editor_state.placements[0].size;
    if let Some(whole) = Rect::from_xywh(0.0, 0.0, w as f32, h as f32) {
        editor_state.damage_local(0, whole);
    }
    mark_dirty(dirty_mask, 0);
    let (x, y) = editor_state.input.pointer.local;
    super::input::handle_pointer_move(editor_state, 0, x, y, dirty_mask);
    apply_damage_rects(editor_state, dirty_mask);
}

fn device((x, y): (f64, f64), scale: f32) -> (f32, f32) {
    (x as f32 * scale, y as f32 * scale)
}
