// ── pinned screenshot ────────────────────────────────────────────────────────
// separate process`--pin`, simple xdg-window without decorations (very similar to flameshot pin)
// initialliy i wanted to avoid xdg-window, to make it spawn exactly where was the capture, and its more than possible in wayland
// but it needs alot of code for window moving, specially when dragging it to one monitor to another
// and its still possible, but i couldn't fix desync between cursor and window position, specially between monitors
// so i kinda realized even after fixing this issue, it will be still unrealiable piece of junk that can break anytime
// maybe in the future would be implemented as a separate option, but def not the main one
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
    Shape, WpCursorShapeDeviceV1,
};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, Region};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{
    KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers, RepeatInfo,
};
use smithay_client_toolkit::seat::pointer::cursor_shape::CursorShapeManager;
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::xdg::{XdgShell, XdgSurface};
use smithay_client_toolkit::shell::xdg::window::{
    Window, WindowConfigure, WindowDecorations, WindowHandler,
};
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{delegate_dispatch2, delegate_registry, registry_handlers};
use tiny_skia::{Color, FillRule, IntSize, Mask, Pixmap, Rect, Transform};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::wp::viewporter::client::{wp_viewport, wp_viewporter};
use wayland_protocols::xdg::toplevel_icon::v1::client::{
    xdg_toplevel_icon_manager_v1, xdg_toplevel_icon_v1,
};

use crate::backend::initialize_clipboard;
use crate::backend::notify::{self, Notice};
use crate::backend::wayland::keysym;
use crate::backend::wayland::utils::input::wheel_clicks;
use crate::edit::{Picture, TOUCHPAD_ZOOM, ZOOM_STEP, step_zoom};
use crate::keys::{self, Action, Mods};
use crate::renderer::Painter;
use crate::renderer::paths::rounded_rect_path;
use crate::theme::radius;
use crate::types::Finish;
use crate::utils::{copy_swizzled, premultiply};

const BTN_LEFT: u32 = 0x110;
const APP_ID: &str = "lumine-pin";

/// Argv the overlay re-execs itself with to pin a finished capture.
pub const PIN_ARG: &str = "--pin";
pub const SCALE_ARG: &str = "--scale";

/// Handles `--pin [FILE]`. Reads the image from stdin if no file path is provided.
pub fn cli(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Read;

    let mut scale = 1.0_f32;
    let mut path = None;
    while let Some(arg) = args.next() {
        if arg == SCALE_ARG {
            scale = args
                .next()
                .and_then(|s| s.parse::<f32>().ok())
                .filter(|s| *s > 0.0)
                .ok_or("--scale needs a positive number")?;
        } else {
            path = Some(arg);
        }
    }

    let image = match path {
        Some(path) => std::fs::read(path)?,
        None => {
            let mut buf = Vec::new();
            std::io::stdin().read_to_end(&mut buf)?;
            buf
        }
    };
    run(&image, scale)
}

/// Re execs this binary to pin a finished capture.
pub fn spawn(png: &[u8], scale: f32) -> Result<(), Box<dyn std::error::Error>> {
    let scale = scale.to_string();
    let mut args = vec![PIN_ARG, SCALE_ARG, &scale];
    if !crate::config::get().notifications.enabled {
        args.push(notify::NO_NOTIFY_ARG);
    }
    crate::utils::spawn_self(&args, png)
}

pub fn run(png: &[u8], scale: f32) -> Result<(), Box<dyn std::error::Error>> {
    let image = decode(png)?;
    let picture = Picture::new(&image);

    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();

    let shm = Shm::bind(&globals, &qh)?;
    let viewporter = globals
        .bind::<wp_viewporter::WpViewporter, _, _>(&qh, 1..=1, ())
        .ok();
    let layout_scale = if viewporter.is_some() { scale } else { 1.0 };
    let layout = layout((image.width(), image.height()), 1.0, layout_scale);
    let pool = SlotPool::new((layout.buffer.0 * layout.buffer.1 * 4) as usize, &shm)?;
    let compositor = CompositorState::bind(&globals, &qh)?;
    let xdg_shell = XdgShell::bind(&globals, &qh)?;
    let icon_manager = globals
        .bind::<xdg_toplevel_icon_manager_v1::XdgToplevelIconManagerV1, _, _>(&qh, 1..=1, ())
        .ok();

    let surface = compositor.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::RequestClient, &qh);
    window.set_title(APP_ID);
    window.set_app_id(APP_ID);
    // buffer in native resolution, while the window uses logical size.
    // otherwise pinned pciture will be low quality when changed scales
    let viewport = viewporter.as_ref().map(|viewporter| {
        let viewport = viewporter.get_viewport(window.wl_surface(), &qh, ());
        viewport.set_destination(layout.dest.0 as i32, layout.dest.1 as i32);
        viewport
    });
    // xdg min/max size count the window geometry, not the padded surface
    window.set_min_size(Some(layout.visible));
    window.set_max_size(Some(layout.visible));
    window.commit();

    // no .desktop entry ties this app_id to an icon, so ask the compositor
    // directly instead of hoping a taskbar falls back to an icon-theme lookup
    if let Some(manager) = &icon_manager {
        let icon = manager.create_icon(&qh, ());
        icon.set_name(APP_ID.to_string());
        manager.set_icon(window.xdg_toplevel(), Some(&icon));
        icon.destroy();
    }

    let mut pin = Pin {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        seat: SeatState::new(&globals, &qh),
        cursor_shapes: CursorShapeManager::bind(&globals, &qh).ok(),
        copying: None,
        shm,
        pool,
        window,
        compositor,
        mapped: false,
        image,
        picture,
        scale: layout_scale,
        zoom: 1.0,
        frame: None,
        viewport,
        redraw: false,
        shade: false,
        opacity: 1.0,
        output: None,
        buffer: None,
        png: png.into(),
        pointer_seat: None,
        cursor: None,
        enter_serial: 0,
        mods: Mods::default(),
        exit: false,
    };

    while !pin.exit {
        queue.blocking_dispatch(&mut pin)?;
        if pin.mapped {
            if pin.redraw {
                pin.draw();
            } else if pin.shade {
                pin.reshade();
            }
        }
    }
    let copying = pin.copying.take();
    drop(pin);
    let _ = conn.flush();
    if let Some(handle) = copying {
        let _ = handle.join();
    }
    Ok(())
}

fn decode(png: &[u8]) -> Result<Pixmap, Box<dyn std::error::Error>> {
    let rgba = image::load_from_memory(png)?.into_rgba8();
    let (w, h) = rgba.dimensions();
    let mut data = rgba.into_raw();
    premultiply(&mut data);
    let size = IntSize::from_wh(w, h).ok_or("empty image")?;
    Pixmap::from_vec(data, size).ok_or_else(|| "invalid image".into())
}

fn fade(data: &mut [u8], opacity: f32) {
    if opacity >= 1.0 {
        return;
    }
    let k = (opacity.clamp(0.0, 1.0) * 255.0).round() as u32;
    for byte in data {
        *byte = ((*byte as u32 * k + 127) / 255) as u8;
    }
}

/// Pads image dimensions with transparent pixels
///
/// prevents KWin rendering artifacts when dragging on a monitor with scale less than 100%
fn whole_size(n: u32, scale: f32) -> u32 {
    let scale = scale as f64;
    (n..n + 256)
        .find(|&b| {
            let logical = b as f64 / scale;
            (logical - logical.round()).abs() < 1e-4
        })
        .unwrap_or(n)
}

// At first thought it will be fine to make unlimited size of pin windows
// but kwin is breaking if the window gets too big
// 3/4 of it still leaves room to move the window
const MAX_SHARE: f32 = 0.75;
const MIN_ZOOM: f32 = 0.25;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Layout {
    buffer: (u32, u32),
    dest: (u32, u32),
    visible: (u32, u32),
}

fn layout(image: (u32, u32), zoom: f32, scale: f32) -> Layout {
    let side = |n: u32| ((n.max(1) as f32 * zoom).round() as u32).max(1);
    let shown = (side(image.0), side(image.1));
    let buffer = (whole_size(shown.0, scale), whole_size(shown.1, scale));
    Layout {
        buffer,
        dest: (
            (buffer.0 as f64 / scale as f64).round() as u32,
            (buffer.1 as f64 / scale as f64).round() as u32,
        ),
        visible: (
            (shown.0 as f32 / scale).ceil() as u32,
            (shown.1 as f32 / scale).ceil() as u32,
        ),
    }
}

fn zoom_limit(image: (u32, u32), scale: f32, monitor: Option<(u32, u32)>) -> f32 {
    let Some((mw, mh)) = monitor else {
        return 1.0;
    };
    let fit = |m: u32, n: u32| MAX_SHARE * m as f32 * scale / n.max(1) as f32;
    fit(mw, image.0).min(fit(mh, image.1)).max(1.0)
}

fn render(
    image: &Pixmap,
    picture: &mut Picture,
    layout: &Layout,
    zoom: f32,
    scale: f32,
) -> Option<Pixmap> {
    let mut canvas = Pixmap::new(layout.buffer.0, layout.buffer.1)?;
    picture.paint(image, &mut canvas, (0.0, 0.0), zoom, Color::TRANSPARENT);

    // border style and radius similar to panels one
    let fw = image.width() as f32 * zoom;
    let fh = image.height() as f32 * zoom;
    let rect = Rect::from_xywh(0.0, 0.0, fw, fh)?;
    // not scaled by zoom, the border keeps the look it has at 1x
    let radius = radius::panel() * scale;
    let shape = rounded_rect_path(&rect, radius, true, true, true, true)?;
    let mut mask = Mask::new(canvas.width(), canvas.height())?;
    mask.fill_path(&shape, FillRule::Winding, true, Transform::identity());
    canvas.apply_mask(&mask);
    Painter::new(&mut canvas).panel_border(0.0, 0.0, fw, fh, radius, 1.0);
    Some(canvas)
}

struct Pin {
    registry: RegistryState,
    outputs: OutputState,
    seat: SeatState,
    cursor_shapes: Option<CursorShapeManager>,
    copying: Option<std::thread::JoinHandle<()>>,
    shm: Shm,
    pool: SlotPool,

    window: Window,
    compositor: CompositorState,
    mapped: bool,
    image: Pixmap,
    picture: Picture,
    scale: f32,
    zoom: f32,
    frame: Option<Pixmap>,
    viewport: Option<wp_viewport::WpViewport>,
    redraw: bool,
    shade: bool,
    opacity: f32,
    output: Option<wl_output::WlOutput>,
    buffer: Option<Buffer>,
    png: std::sync::Arc<[u8]>,

    pointer_seat: Option<wl_seat::WlSeat>,
    cursor: Option<WpCursorShapeDeviceV1>,
    enter_serial: u32,
    mods: Mods,
    exit: bool,
}

impl Pin {
    fn set_cursor(&self, shape: Shape) {
        if let Some(device) = &self.cursor {
            device.set_shape(self.enter_serial, shape);
        }
    }

    fn monitor(&self) -> Option<(u32, u32)> {
        let size = |output: &wl_output::WlOutput| {
            let (w, h) = self.outputs.info(output)?.logical_size?;
            Some((w.max(0) as u32, h.max(0) as u32))
        };
        self.output.as_ref().and_then(size).or_else(|| {
            self.outputs
                .outputs()
                .filter_map(|o| size(&o))
                .max_by_key(|(w, h)| *w as u64 * *h as u64)
        })
    }

    fn zoom_limit(&self) -> f32 {
        let image = (self.image.width(), self.image.height());
        zoom_limit(image, self.scale, self.monitor())
    }

    // opacity re-copies the last frame, premultiplied pixels scale in all four channels alike
    fn reshade(&mut self) {
        self.shade = false;
        let Some(frame) = &self.frame else {
            return;
        };
        let (w, h) = (frame.width() as i32, frame.height() as i32);
        let Ok((buffer, canvas)) = self
            .pool
            .create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)
        else {
            self.exit = true;
            return;
        };
        copy_swizzled(canvas, frame.data());
        fade(canvas, self.opacity);
        self.window.attach(Some(buffer.wl_buffer()), 0, 0);
        self.window.wl_surface().damage_buffer(0, 0, w, h);
        self.window.commit();
        self.buffer = Some(buffer);
    }

    fn draw(&mut self) {
        self.redraw = false;
        self.shade = false;
        let scale = self.scale;
        let layout = layout((self.image.width(), self.image.height()), self.zoom, scale);
        let Some(frame) = render(&self.image, &mut self.picture, &layout, self.zoom, scale) else {
            self.exit = true;
            return;
        };
        let (w, h) = (frame.width() as i32, frame.height() as i32);
        let Ok((buffer, canvas)) = self
            .pool
            .create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)
        else {
            self.exit = true;
            return;
        };
        copy_swizzled(canvas, frame.data());
        fade(canvas, self.opacity);
        self.frame = Some(frame);

        let surface = self.window.wl_surface();
        self.window.set_min_size(Some(layout.visible));
        self.window.set_max_size(Some(layout.visible));
        if let Ok(region) = Region::new(&self.compositor) {
            region.add(0, 0, layout.visible.0 as i32, layout.visible.1 as i32);
            surface.set_input_region(Some(region.wl_region()));
        }
        // the padding stays outside the window, snapping and focus borders follow the picture
        // it sure thing looks like a unelegant workaround, but currently it looks like the only way
        // to fix strecthed pin on weird fractional scales
        self.window
            .set_window_geometry(0, 0, layout.visible.0, layout.visible.1);
        if let Some(viewport) = &self.viewport {
            viewport.set_destination(layout.dest.0 as i32, layout.dest.1 as i32);
        }
        self.window.attach(Some(buffer.wl_buffer()), 0, 0);
        surface.damage_buffer(0, 0, w, h);
        self.window.commit();
        self.buffer = Some(buffer);
    }
}

impl WindowHandler for Pin {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &Window,
        _: WindowConfigure,
        _: u32,
    ) {
        if self.mapped {
            return;
        }
        self.draw();
        self.mapped = !self.exit;
    }
}

impl PointerHandler for Pin {
    fn pointer_frame(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_pointer::WlPointer,
        events: &[PointerEvent],
    ) {
        let mut clicks = 0.0_f32;
        for event in events {
            if self.window.wl_surface() != &event.surface {
                continue;
            }
            match event.kind {
                PointerEventKind::Enter { serial } => {
                    self.enter_serial = serial;
                    self.set_cursor(Shape::Grab);
                }
                // now draggong and etc none of our business, fully handled by wayland window manager
                PointerEventKind::Press {
                    button: BTN_LEFT,
                    serial,
                    ..
                } => {
                    if let Some(seat) = &self.pointer_seat {
                        self.window.move_(seat, serial);
                    }
                }
                PointerEventKind::Axis { vertical, .. } => {
                    let mut axis = -wheel_clicks(&vertical);
                    if axis == 0.0 {
                        let sensitivity = crate::config::get().input.scroll_sensitivity;
                        axis = -(vertical.absolute as f32) * sensitivity / TOUCHPAD_ZOOM;
                    }
                    clicks += axis;
                }
                _ => {}
            }
        }
        if clicks != 0.0 && self.mods.ctrl {
            let opacity = (self.opacity + 0.1 * clicks).clamp(0.1, 1.0);
            self.shade |= opacity != self.opacity;
            self.opacity = opacity;
        } else if clicks != 0.0 {
            let limit = self.zoom_limit();
            let mut zoom = step_zoom(self.zoom, ZOOM_STEP.powf(clicks), MIN_ZOOM, limit);
            if clicks > 0.0 {
                zoom = zoom.max(self.zoom);
            }
            self.redraw |= zoom != self.zoom;
            self.zoom = zoom;
        }
    }
}

impl KeyboardHandler for Pin {
    fn press_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        event: KeyEvent,
    ) {
        // uses the same bind as our usual overlay
        let chord = keysym::chord(event.keysym, event.raw_code, self.mods);
        match chord.and_then(|c| keys::map().lookup(c)) {
            Some(Action::Cancel) => self.exit = true,
            Some(Action::Finish(Finish::Copy)) => {
                if self.copying.as_ref().is_some_and(|h| !h.is_finished()) {
                    return;
                }
                let png = self.png.clone();
                self.copying = Some(std::thread::spawn(move || {
                    let extra = crate::utils::format::for_clipboard(&png);
                    let extra = extra.as_ref().map(|(f, d)| (*f, d.as_slice()));
                    let notice = match initialize_clipboard().copy_image_to_clipboard(&png, extra) {
                        Ok(()) => Notice::Copied(None, Some(&png)),
                        Err(e) => {
                            log::error!("copy failed: {e}");
                            Notice::CopyFailed(e.to_string())
                        }
                    };
                    notify::send_blocking(notice);
                }));
            }
            _ => {}
        }
    }

    fn update_modifiers(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        modifiers: Modifiers,
        _: RawModifiers,
        _: u32,
    ) {
        self.mods = keysym::mods(&modifiers);
    }

    fn repeat_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
        _: &[u32],
        _: &[Keysym],
    ) {
    }
    fn leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: &wl_surface::WlSurface,
        _: u32,
    ) {
        self.mods = Mods::default();
    }
    fn release_key(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: u32,
        _: KeyEvent,
    ) {
    }
    fn update_repeat_info(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_keyboard::WlKeyboard,
        _: RepeatInfo,
    ) {
    }
}

impl SeatHandler for Pin {
    fn seat_state(&mut self) -> &mut SeatState {
        &mut self.seat
    }

    fn new_capability(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        seat: wl_seat::WlSeat,
        capability: Capability,
    ) {
        match capability {
            Capability::Pointer => {
                let Ok(pointer) = self.seat.get_pointer(qh, &seat) else {
                    return;
                };
                self.cursor = self
                    .cursor_shapes
                    .as_ref()
                    .map(|shapes| shapes.get_shape_device(&pointer, qh));
                self.pointer_seat = Some(seat);
            }
            Capability::Keyboard => {
                let _ = self.seat.get_keyboard(qh, &seat, None);
            }
            _ => {}
        }
    }

    fn new_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
    fn remove_capability(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: wl_seat::WlSeat,
        _: Capability,
    ) {
    }
    fn remove_seat(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_seat::WlSeat) {}
}

impl CompositorHandler for Pin {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        output: &wl_output::WlOutput,
    ) {
        self.output = Some(output.clone());
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for Pin {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for Pin {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl Dispatch<xdg_toplevel_icon_manager_v1::XdgToplevelIconManagerV1, ()> for Pin {
    fn event(
        _: &mut Self,
        _: &xdg_toplevel_icon_manager_v1::XdgToplevelIconManagerV1,
        _: xdg_toplevel_icon_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wp_viewporter::WpViewporter, ()> for Pin {
    fn event(
        _: &mut Self,
        _: &wp_viewporter::WpViewporter,
        _: wp_viewporter::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<wp_viewport::WpViewport, ()> for Pin {
    fn event(
        _: &mut Self,
        _: &wp_viewport::WpViewport,
        _: wp_viewport::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<xdg_toplevel_icon_v1::XdgToplevelIconV1, ()> for Pin {
    fn event(
        _: &mut Self,
        _: &xdg_toplevel_icon_v1::XdgToplevelIconV1,
        _: xdg_toplevel_icon_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl ProvidesRegistryState for Pin {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }
    registry_handlers![OutputState, SeatState];
}

delegate_registry!(Pin);
delegate_dispatch2!(Pin);

#[cfg(test)]
mod tests {
    use super::*;

    fn pixmap(w: u32, h: u32) -> Pixmap {
        let mut image = Pixmap::new(w, h).unwrap();
        image.fill(Color::from_rgba8(10, 20, 30, 255));
        image
    }

    #[test]
    fn layout_at_100_percent_pads_to_whole_pixels() {
        let old = |n: u32, scale: f32| {
            let target = whole_size(n, scale);
            (
                target,
                (target as f32 / scale).round() as u32,
                (n as f32 / scale).ceil() as u32,
            )
        };
        for scale in [1.0, 1.35, 1.25, 1.5] {
            let l = layout((406, 203), 1.0, scale);
            let (bw, dw, vw) = old(406, scale);
            let (bh, dh, vh) = old(203, scale);
            assert_eq!(l.buffer, (bw, bh));
            assert_eq!(l.dest, (dw, dh));
            assert_eq!(l.visible, (vw, vh));
        }
        let l = layout((406, 203), 1.0, 1.0);
        assert_eq!(
            (l.buffer, l.dest, l.visible),
            ((406, 203), (406, 203), (406, 203))
        );
        let l = layout((406, 203), 1.0, 1.35);
        assert_eq!(l.buffer, (whole_size(406, 1.35), whole_size(203, 1.35)));
        assert!(l.buffer.0 > 406);
    }

    #[test]
    fn destination_is_whole_in_both_units() {
        for scale in [1.0f32, 1.25, 1.35, 1.5, 1.75] {
            for zoom in [0.25, 0.5, 0.77, 1.0, 1.25, 3.3] {
                let l = layout((406, 203), zoom, scale);
                for (b, d) in [(l.buffer.0, l.dest.0), (l.buffer.1, l.dest.1)] {
                    let logical = b as f64 / scale as f64;
                    assert!((logical - d as f64).abs() < 1e-4, "{scale} {zoom}");
                }
                assert!(l.visible.0 <= l.dest.0 && l.visible.1 <= l.dest.1);
            }
        }
    }

    #[test]
    fn small_pin_can_grow_to_three_quarters_of_the_monitor() {
        let limit = zoom_limit((406, 203), 1.0, Some((1920, 1080)));
        let (by_width, by_height) = (0.75_f32 * 1920.0 / 406.0, 0.75_f32 * 1080.0 / 203.0);
        assert!(by_width < by_height);
        assert_eq!(limit, by_width);
    }

    #[test]
    fn big_pins_cannot_zoom_in() {
        assert_eq!(zoom_limit((1920, 1080), 1.0, Some((1920, 1080))), 1.0);
        assert_eq!(zoom_limit((1080, 20000), 1.0, Some((1920, 1080))), 1.0);
        assert_eq!(zoom_limit((406, 203), 1.0, None), 1.0);
    }

    #[test]
    fn zoom_limit_follows_the_scale() {
        let limit = zoom_limit((406, 203), 1.35, Some((1422, 800)));
        let expected = (0.75_f32 * 800.0 * 1.35 / 203.0).min(0.75 * 1422.0 * 1.35 / 406.0);
        assert_eq!(limit, expected);
    }

    #[test]
    fn render_keeps_padding_transparent() {
        let image = pixmap(406, 203);
        let mut picture = Picture::new(&image);
        let l = layout((406, 203), 1.0, 1.35);
        let frame = render(&image, &mut picture, &l, 1.0, 1.35).unwrap();
        assert_eq!((frame.width(), frame.height()), l.buffer);
        assert!(frame.width() > 406);
        let (w, h) = (frame.width() as usize, frame.height() as usize);
        assert_eq!(frame.data()[(h - 1) * w * 4 + (w - 1) * 4 + 3], 0);
        assert_eq!(&frame.data()[(100 * w + 200) * 4..][..4], [10, 20, 30, 255]);
    }

    #[test]
    fn fade_scales_every_channel() {
        let mut data = vec![200, 100, 50, 255, 0, 10, 20, 30];
        fade(&mut data, 0.5);
        assert_eq!(data, [100, 50, 25, 128, 0, 5, 10, 15]);
        let mut same = vec![1, 2, 3, 4];
        fade(&mut same, 1.0);
        assert_eq!(same, [1, 2, 3, 4]);
    }

    #[test]
    fn zoomed_render_fills_the_picture_rect() {
        let image = pixmap(100, 50);
        let mut picture = Picture::new(&image);
        let l = layout((100, 50), 2.0, 1.0);
        let frame = render(&image, &mut picture, &l, 2.0, 1.0).unwrap();
        assert_eq!((frame.width(), frame.height()), (200, 100));
        let w = frame.width() as usize;
        assert_eq!(&frame.data()[(50 * w + 100) * 4..][..4], [10, 20, 30, 255]);
    }
}
