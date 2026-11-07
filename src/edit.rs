mod picture;
mod view;

use std::io::{IsTerminal, Read};

use tiny_skia::{IntSize, Pixmap};

use crate::backend::wayland::overlay::WaylandOverlay;
use crate::backend::{FRAME_FALLBACK, ScreenOverlay};
use crate::keys::{self, Action};
use crate::types::{CursorIcon, MouseButton, OverlayEvent, px_size};
use picture::Picture;
use view::View;

pub const EDIT_ARG: &str = "--edit";
const TITLE: &str = "LumineCapture";
const MAX_SHARE: f32 = 0.85;
const WHEEL_STEP: f32 = 100.0;
const ZOOM_STEP: f32 = 1.25;
const TOUCHPAD_ZOOM: f32 = 15.0;

pub fn cli(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn std::error::Error>> {
    let path = args.next();
    if let Some(extra) = args.next() {
        return Err(format!("unexpected argument {extra:?}").into());
    }
    let bytes = match path.as_deref() {
        None | Some("-") => {
            let mut stdin = std::io::stdin();
            if stdin.is_terminal() {
                return Err("no image: pass a file or pipe one in".into());
            }
            let mut bytes = Vec::new();
            stdin.read_to_end(&mut bytes)?;
            bytes
        }
        Some(path) => std::fs::read(path).map_err(|e| format!("can't read {path}: {e}"))?,
    };
    run(decode(&bytes)?)
}

fn decode(bytes: &[u8]) -> Result<Pixmap, Box<dyn std::error::Error>> {
    let rgba = image::load_from_memory(bytes)?.into_rgba8();
    let (w, h) = rgba.dimensions();
    let mut data = rgba.into_raw();
    crate::utils::premultiply(&mut data);
    let size = IntSize::from_wh(w, h).ok_or("empty image")?;
    Ok(Pixmap::from_vec(data, size).ok_or("invalid image")?)
}

struct Editor {
    picture: Picture,
    view: View,
    canvas: Option<Pixmap>,
    scale: f32,
    dirty: bool,
    pointer: (f32, f32),
    dragging: Option<(f32, f32)>,
    /// fractions not panned yet pixel : whole steps let `Picture` move the old frame
    pan_rest: (f32, f32),
    ctrl: bool,
    shift: bool,
}

pub fn run(image: Pixmap) -> Result<(), Box<dyn std::error::Error>> {
    let conn = wayland_client::Connection::connect_to_env()?;
    let mut window = WaylandOverlay::new(conn, false)?;
    let (size, scale) = initial_size(window.discovered_outputs(), (image.width(), image.height()));
    window.open_window(TITLE, size, scale)?;

    let picture = Picture::new(image);
    let mut editor = Editor {
        view: View::new(picture.size()),
        picture,
        canvas: None,
        scale,
        dirty: true,
        pointer: (0.0, 0.0),
        dragging: None,
        pan_rest: (0.0, 0.0),
        ctrl: false,
        shift: false,
    };
    let backdrop = crate::theme::color::panel().with_alpha(255).color();

    loop {
        let timeout = (editor.dirty && !window.ready(0)).then_some(FRAME_FALLBACK);
        match window.next_event(timeout)? {
            OverlayEvent::EscapePressed => break,
            OverlayEvent::Resized { .. } => {
                if let Some((size, scale)) = window.window_size() {
                    editor.resize(size, scale);
                }
            }
            OverlayEvent::Key {
                chord: Some(chord), ..
            } => {
                if keys::map().lookup(chord) == Some(Action::Cancel) {
                    break;
                }
                editor.key(chord);
            }
            ev => editor.handle(ev),
        }
        window.set_cursor(match editor.dragging {
            Some(_) => CursorIcon::Grabbing,
            None => CursorIcon::Default,
        });

        if editor.dirty
            && window.ready(0)
            && let Some(canvas) = editor.canvas.as_mut()
        {
            editor.picture.draw(canvas, &editor.view, backdrop);
            window.stage_frame(0, canvas.data(), None)?;
            window.flush()?;
            editor.dirty = false;
        }
    }
    Ok(())
}

impl Editor {
    fn resize(&mut self, size: (u32, u32), scale: f32) {
        let (w, h) = px_size(size, scale);
        self.view.resize((w, h), scale / self.scale);
        self.pointer = (
            self.pointer.0 * scale / self.scale,
            self.pointer.1 * scale / self.scale,
        );
        self.scale = scale;
        self.canvas = Pixmap::new(w, h);
        self.dirty = true;
    }

    fn handle(&mut self, ev: OverlayEvent) {
        match ev {
            OverlayEvent::PointerMove { x, y, .. } => {
                let at = (x as f32 * self.scale, y as f32 * self.scale);
                if let Some(from) = self.dragging {
                    self.pan((at.0 - from.0, at.1 - from.1));
                    self.dragging = Some(at);
                }
                self.pointer = at;
            }
            OverlayEvent::PointerButton {
                button: MouseButton::Middle,
                pressed,
            } => self.dragging = pressed.then_some(self.pointer),
            OverlayEvent::ModifiersChanged { ctrl, shift } => {
                self.ctrl = ctrl;
                self.shift = shift;
            }
            OverlayEvent::Scroll {
                delta_x,
                delta_y,
                wheel,
            } => self.scroll((delta_x, delta_y), wheel),
            _ => {}
        }
    }

    fn scroll(&mut self, delta: (f32, f32), wheel: Option<(f32, f32)>) {
        if self.ctrl {
            let clicks = wheel.map_or(delta.1 / TOUCHPAD_ZOOM, |w| w.1);
            self.dirty |= self.view.zoom_at(ZOOM_STEP.powf(clicks), self.pointer);
            return;
        }
        let (dx, dy) = match wheel {
            Some((x, y)) => (-x * WHEEL_STEP, y * WHEEL_STEP),
            None => (-delta.0, delta.1),
        };
        let (dx, dy) = if self.shift { (dy, dx) } else { (dx, dy) };
        self.pan((dx * self.scale, dy * self.scale));
    }

    fn pan(&mut self, (dx, dy): (f32, f32)) {
        let rest = (self.pan_rest.0 + dx, self.pan_rest.1 + dy);
        let whole = (rest.0.round(), rest.1.round());
        self.pan_rest = (rest.0 - whole.0, rest.1 - whole.1);
        self.dirty |= self.view.pan_by(whole);
    }

    fn key(&mut self, chord: keys::Chord) {
        let center = self.view.center();
        self.dirty |= match keys::map().lookup(chord) {
            Some(Action::ZoomIn) => self.view.zoom_at(ZOOM_STEP, center),
            Some(Action::ZoomOut) => self.view.zoom_at(1.0 / ZOOM_STEP, center),
            Some(Action::ZoomActual) => self.view.zoom_to(1.0, center),
            Some(Action::ZoomFit) => {
                self.view.fit();
                true
            }
            _ => false,
        };
    }
}

/// sized by the biggest monitor
/// window's real scale only comes once it's shown
fn initial_size(outputs: &[crate::types::Output], picture: (u32, u32)) -> ((u32, u32), f32) {
    let biggest = outputs
        .iter()
        .filter_map(|o| Some((o.info.logical_size?, o.scale())))
        .max_by_key(|((w, h), _)| w * h);
    let ((mw, mh), scale) = biggest.unwrap_or(((1280, 800), 1.0));
    let fit = |px: u32, max: i32| (px as f32 / scale).min(max as f32 * MAX_SHARE).round() as u32;
    ((fit(picture.0, mw), fit(picture.1, mh)), scale)
}
