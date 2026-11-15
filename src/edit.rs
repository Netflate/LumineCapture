// editor window mode is a process of its own, like pin window;
// the editor itself is the overlay's, see app/window.rs
mod picture;
mod view;

use std::io::{IsTerminal, Read};

use tiny_skia::{Color, IntSize, Pixmap};

use crate::types::Placement;
pub use picture::Picture;
use picture::{Shown, whole_shift};
pub use view::View;

pub const EDIT_ARG: &str = "--edit";
pub const TITLE: &str = "LumineCapture";
const MAX_SHARE: f32 = 0.85;
/// logical pixels per wheel click, the touchpad moves the picture 1:1
pub const WHEEL_STEP: f32 = 100.0;
pub const ZOOM_STEP: f32 = 1.25;
/// touchpad pixels that zoom as much as one wheel click
pub const TOUCHPAD_ZOOM: f32 = 15.0;

pub fn cli(
    mut args: impl Iterator<Item = String>,
    mut launch: crate::app::Launch,
) -> Result<(), Box<dyn std::error::Error>> {
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
    let image = decode(&bytes)?;
    launch.mode = crate::types::Mode::Editor;
    let outputs = launch.outputs();
    tokio::runtime::Runtime::new()?.block_on(async {
        let conn = wayland_client::Connection::connect_to_env()
            .map_err(|e| format!("can't connect to Wayland: {e}"))?;
        crate::app::window::run(conn, image, outputs, launch.output).await
    })
}

pub fn spawn(
    png: &[u8],
    to: crate::types::Outputs,
    dir: Option<&std::path::Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::ffi::OsString;

    let mut args: Vec<OsString> = vec![
        EDIT_ARG.into(),
        "-".into(),
        "-t".into(),
        to.letters().into(),
        "--format".into(),
        crate::config::get().save.file_format.name().into(),
    ];
    if let Some(dir) = dir {
        args.extend(["-o".into(), dir.into()]);
    }
    let stdout = if to.stdout {
        std::process::Stdio::inherit()
    } else {
        std::process::Stdio::null()
    };
    crate::utils::spawn_self_stdout(&args, png, stdout)
}

fn decode(bytes: &[u8]) -> Result<Pixmap, Box<dyn std::error::Error>> {
    let rgba = image::load_from_memory(bytes)?.into_rgba8();
    let (w, h) = rgba.dimensions();
    let mut data = rgba.into_raw();
    crate::utils::premultiply(&mut data);
    let size = IntSize::from_wh(w, h).ok_or("empty image")?;
    Ok(Pixmap::from_vec(data, size).ok_or("invalid image")?)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Follow {
    Same,
    Shift(i32, i32),
    Redraw,
}

/// picture of the editor window and how it's shown
///
/// annotations are in document units, `scale` picture pixels each, like logical pixels of a monitor
pub struct Sheet {
    picture: Picture,
    pub view: View,
    placement: Placement,
    pub base: Pixmap,
    pub stale: bool,
    /// fractions not panned yet pixel : whole steps let `Picture` move the old frame
    pan_rest: (f32, f32),
    pub dragging: Option<(f64, f64)>,
    pub hand: bool,
    pub hand_grab: bool,
    layer: Option<Shown>,
}

impl Sheet {
    pub fn new(picture: &Pixmap, scale: f32) -> Self {
        let (w, h) = (picture.width(), picture.height());
        let scale = doc_scale((w, h), scale);
        Self {
            picture: Picture::new(picture),
            view: View::new((w, h)),
            placement: Placement {
                position: (0, 0),
                size: ((w as f32 / scale) as i32, (h as f32 / scale) as i32),
                scale,
            },
            base: Pixmap::new(1, 1).expect("1x1 pixmap"),
            stale: true,
            pan_rest: (0.0, 0.0),
            dragging: None,
            hand: false,
            hand_grab: false,
            layer: None,
        }
    }

    pub fn placement(&self) -> &Placement {
        &self.placement
    }

    pub fn scale(&self) -> f32 {
        self.placement.scale
    }

    pub fn origin(&self) -> (f32, f32) {
        self.origin_at(self.view.pan, self.view.zoom)
    }

    pub fn zoom(&self, window_scale: f32) -> f32 {
        self.zoom_at(self.view.zoom, window_scale)
    }

    /// view the annotations layer holds right now; whatever draws into it directly
    /// (pen segments, a finished shape) uses it, since the layer catches up with a pan only
    /// at the next frame
    pub fn layer_origin(&self) -> (f32, f32) {
        match self.layer {
            Some(shown) => self.origin_at(shown.pan, shown.zoom),
            None => self.origin(),
        }
    }

    pub fn layer_zoom(&self, window_scale: f32) -> f32 {
        match self.layer {
            Some(shown) => self.zoom_at(shown.zoom, window_scale),
            None => self.zoom(window_scale),
        }
    }

    fn origin_at(&self, pan: (f32, f32), zoom: f32) -> (f32, f32) {
        let k = zoom * self.scale();
        (-pan.0 / k, -pan.1 / k)
    }

    fn zoom_at(&self, zoom: f32, window_scale: f32) -> f32 {
        zoom * self.scale() / window_scale
    }

    pub fn resize(&mut self, px: (u32, u32), inset: f32, pad: f32, ratio: f32) {
        self.view.resize(px, inset, pad, ratio);
        if let Some(base) = Pixmap::new(px.0, px.1) {
            self.base = base;
        }
        self.stale = true;
        self.layer = None;
    }

    pub fn follow(&mut self, size: (u32, u32)) -> Follow {
        let now = Shown {
            zoom: self.view.zoom,
            pan: self.view.pan,
            size,
        };
        match self.layer.replace(now) {
            Some(was) if was.zoom == now.zoom && was.size == size => {
                if was.pan == now.pan {
                    Follow::Same
                } else {
                    whole_shift(was.pan, now.pan, size)
                        .map_or(Follow::Redraw, |(dx, dy)| Follow::Shift(dx, dy))
                }
            }
            _ => Follow::Redraw,
        }
    }

    pub fn pan(&mut self, (dx, dy): (f32, f32)) -> bool {
        let rest = (self.pan_rest.0 + dx, self.pan_rest.1 + dy);
        let whole = (rest.0.round(), rest.1.round());
        self.pan_rest = (rest.0 - whole.0, rest.1 - whole.1);
        let moved = self.view.pan_by(whole);
        self.moved(moved)
    }

    pub fn moved(&mut self, moved: bool) -> bool {
        self.stale |= moved;
        moved
    }

    pub fn paint(&mut self, full: &Pixmap, backdrop: Color) {
        self.picture
            .draw(full, &mut self.base, &self.view, backdrop);
    }
}

/// The monitor scale at which picture pixels align perfectly with whole
/// logical pixels, resembling a native screenshot taken on that display.
/// Otherwise, picture pixels serve as independent layout units.
fn doc_scale((w, h): (u32, u32), scale: f32) -> f32 {
    let whole = |s: f32| s >= 1.0 && (w as f32 / s).fract() == 0.0 && (h as f32 / s).fract() == 0.0;
    [scale, scale.floor()]
        .into_iter()
        .find(|&s| whole(s))
        .unwrap_or(1.0)
}

/// device pixels kept free around the screenshot
pub fn frame_pad(scale: f32) -> f32 {
    let cfg = crate::config::get();
    (cfg.selection
        .border_width
        .max(cfg.input.handle_hit_width / 2.0)
        * scale)
        .ceil()
}

/// sized by the biggest monitor
/// window's real scale only comes once it's shown
pub fn initial_size(
    outputs: &[crate::types::Output],
    picture: (u32, u32),
    top: f32,
    pad: f32,
) -> ((u32, u32), f32) {
    let biggest = outputs
        .iter()
        .filter_map(|o| Some((o.info.logical_size?, o.scale())))
        .max_by_key(|((w, h), _)| w * h);
    let ((mw, mh), scale) = biggest.unwrap_or(((1280, 800), 1.0));
    let fit = |px: f32, max: i32| px.min(max as f32 * MAX_SHARE).round() as u32;
    let (w, h) = (picture.0 as f32 / scale, picture.1 as f32 / scale);
    (
        (fit(w + pad * 2.0, mw), fit(h + top + pad * 2.0, mh)),
        scale,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_scale_keeps_whole_logical_pixels() {
        assert_eq!(doc_scale((2400, 1500), 1.25), 1.25);
        assert_eq!(doc_scale((2560, 1600), 2.0), 2.0);
        assert_eq!(doc_scale((2001, 1500), 1.25), 1.0);
        assert_eq!(doc_scale((1000, 602), 2.5), 2.0);
        assert_eq!(doc_scale((999, 600), 0.5), 1.0);
    }

    #[test]
    fn window_has_room_for_the_toolbar_over_the_picture() {
        let size = |picture| initial_size(&[], picture, 40.0, 0.0).0;
        assert_eq!(size((800, 400)), (800, 440));
        assert_eq!(size((4000, 4000)), (1088, 680));
    }

    #[test]
    fn the_layer_follows_the_view() {
        let image = Pixmap::new(300, 200).unwrap();
        let mut sheet = Sheet::new(&image, 1.0);
        sheet.resize((120, 90), 0.0, 0.0, 1.0);
        let size = (120, 90);
        let center = sheet.view.center();
        assert!(sheet.view.zoom_to(1.0, center));
        assert_eq!(sheet.follow(size), Follow::Redraw);
        assert_eq!(sheet.follow(size), Follow::Same);

        assert!(sheet.pan((7.0, 3.0)));
        assert_eq!(sheet.follow(size), Follow::Shift(7, 3));
        assert_eq!(sheet.follow(size), Follow::Same);

        assert!(sheet.view.zoom_at(1.25, center));
        assert_eq!(sheet.follow(size), Follow::Redraw);
        assert_eq!(sheet.follow(size), Follow::Same);

        assert_eq!(sheet.follow((121, 90)), Follow::Redraw);
        assert_eq!(sheet.follow((121, 90)), Follow::Same);

        sheet.resize((120, 90), 0.0, 0.0, 1.0);
        assert_eq!(sheet.follow(size), Follow::Redraw);

        assert!(sheet.pan((500.0, 0.0)));
        assert_eq!(sheet.follow(size), Follow::Redraw);
    }

    #[test]
    fn direct_writers_draw_in_the_layers_own_view() {
        let image = Pixmap::new(300, 200).unwrap();
        let mut sheet = Sheet::new(&image, 1.0);
        sheet.resize((120, 90), 0.0, 0.0, 1.0);
        let center = sheet.view.center();
        assert!(sheet.view.zoom_to(1.0, center));
        let size = (120, 90);
        assert_eq!(sheet.layer_origin(), sheet.origin());
        assert_eq!(sheet.layer_zoom(1.0), sheet.zoom(1.0));

        assert_eq!(sheet.follow(size), Follow::Redraw);
        let (origin, zoom) = (sheet.origin(), sheet.zoom(1.0));
        assert_eq!(sheet.layer_origin(), origin);

        assert!(sheet.pan((7.0, 3.0)));
        assert_ne!(sheet.origin(), origin);
        assert_eq!(sheet.layer_origin(), origin);
        assert_eq!(sheet.layer_zoom(1.0), zoom);

        assert_eq!(sheet.follow(size), Follow::Shift(7, 3));
        assert_eq!(sheet.layer_origin(), sheet.origin());

        assert!(sheet.view.zoom_at(1.25, center));
        assert_ne!(sheet.layer_zoom(1.0), sheet.zoom(1.0));
        assert_eq!(sheet.follow(size), Follow::Redraw);
        assert_eq!(sheet.layer_zoom(1.0), sheet.zoom(1.0));

        assert!(sheet.pan((5.0, 0.0)));
        sheet.resize((120, 90), 0.0, 0.0, 1.0);
        assert_eq!(sheet.layer_origin(), sheet.origin());
        assert_eq!(sheet.layer_zoom(1.0), sheet.zoom(1.0));
    }
}
