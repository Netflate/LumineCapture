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
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{
    KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers, RepeatInfo,
};
use smithay_client_toolkit::seat::pointer::cursor_shape::CursorShapeManager;
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::xdg::XdgShell;
use smithay_client_toolkit::shell::xdg::window::{
    Window, WindowConfigure, WindowDecorations, WindowHandler,
};
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{
    delegate_compositor, delegate_keyboard, delegate_output, delegate_pointer, delegate_registry,
    delegate_seat, delegate_shm, delegate_xdg_shell, delegate_xdg_window, registry_handlers,
};
use tiny_skia::{FillRule, IntSize, Mask, Pixmap, Rect, Transform};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols::xdg::toplevel_icon::v1::client::{
    xdg_toplevel_icon_manager_v1, xdg_toplevel_icon_v1,
};

use crate::backend::notify::{self, Notice};
use crate::backend::wayland::keysym;
use crate::backend::{ClipboardProvider, initialize_clipboard};
use crate::keys::{self, Action, Mods};
use crate::renderer::paths::{draw_panel_border, rounded_rect_path};
use crate::theme::radius;
use crate::types::Finish;
use crate::utils::copy_swizzled;

const BTN_LEFT: u32 = 0x110;
const APP_ID: &str = "lumine-pin";

/// Argv the overlay re-execs itself with to pin a finished capture.
pub const PIN_ARG: &str = "--pin";

/// Handles `--pin [FILE]`. Reads the image from stdin if no file path is provided.
pub fn cli(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Read;

    let image = match args.next() {
        Some(path) => std::fs::read(path)?,
        None => {
            let mut buf = Vec::new();
            std::io::stdin().read_to_end(&mut buf)?;
            buf
        }
    };
    run(&image)
}

/// Re execs this binary to pin a finished capture.
pub fn spawn(png: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    crate::utils::spawn_self(&[PIN_ARG], png)
}

pub fn run(image: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    let frame = render_pin(image)?;
    let size = (frame.width(), frame.height());

    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();

    let shm = Shm::bind(&globals, &qh)?;
    let pool = SlotPool::new((size.0 * size.1 * 4) as usize, &shm)?;
    let compositor = CompositorState::bind(&globals, &qh)?;
    let xdg_shell = XdgShell::bind(&globals, &qh)?;
    let icon_manager = globals
        .bind::<xdg_toplevel_icon_manager_v1::XdgToplevelIconManagerV1, _, _>(&qh, 1..=1, ())
        .ok();

    let surface = compositor.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::RequestClient, &qh);
    window.set_title(APP_ID);
    window.set_app_id(APP_ID);
    window.set_min_size(Some(size));
    window.set_max_size(Some(size));
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
        clipboard: initialize_clipboard(),
        shm,
        pool,
        window,
        mapped: false,
        frame: Some(frame),
        size,
        buffer: None,
        png: image.to_vec(),
        pointer_seat: None,
        cursor: None,
        enter_serial: 0,
        mods: Mods::default(),
        exit: false,
    };

    while !pin.exit {
        queue.blocking_dispatch(&mut pin)?;
    }
    Ok(())
}

fn render_pin(image: &[u8]) -> Result<Pixmap, Box<dyn std::error::Error>> {
    let rgba = image::load_from_memory(image)?.into_rgba8();
    let (w, h) = rgba.dimensions();
    let mut data = rgba.into_raw();
    for px in data.chunks_exact_mut(4) {
        let a = px[3] as u16;
        if a != 255 {
            for c in &mut px[..3] {
                *c = ((*c as u16 * a + 127) / 255) as u8;
            }
        }
    }
    let size = IntSize::from_wh(w, h).ok_or("empty image")?;
    let mut pin = Pixmap::from_vec(data, size).ok_or("invalid image")?;

    // border style and radius similar to panels one
    let (fw, fh) = (w as f32, h as f32);
    let rect = Rect::from_xywh(0.0, 0.0, fw, fh).ok_or("empty image")?;
    let shape =
        rounded_rect_path(&rect, radius::PANEL, true, true, true, true).ok_or("empty image")?;
    let mut mask = Mask::new(w, h).ok_or("image is too large")?;
    mask.fill_path(&shape, FillRule::Winding, true, Transform::identity());
    pin.apply_mask(&mask);
    draw_panel_border(&mut pin, 0.0, 0.0, fw, fh, radius::PANEL, 1.0);
    Ok(pin)
}

struct Pin {
    registry: RegistryState,
    outputs: OutputState,
    seat: SeatState,
    cursor_shapes: Option<CursorShapeManager>,
    clipboard: Box<dyn ClipboardProvider>,
    shm: Shm,
    pool: SlotPool,

    window: Window,
    mapped: bool,
    frame: Option<Pixmap>,
    size: (u32, u32),
    buffer: Option<Buffer>,
    png: Vec<u8>,

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
}

impl WindowHandler for Pin {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.exit = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        window: &Window,
        _: WindowConfigure,
        _: u32,
    ) {
        if self.mapped {
            return;
        }
        if self.buffer.is_none() {
            let Some(frame) = self.frame.take() else {
                return;
            };
            let (w, h) = (frame.width() as i32, frame.height() as i32);
            let Ok((buffer, canvas)) =
                self.pool
                    .create_buffer(w, h, w * 4, wl_shm::Format::Argb8888)
            else {
                self.exit = true;
                return;
            };
            copy_swizzled(canvas, frame.data());
            self.buffer = Some(buffer);
        }
        let Some(buffer) = &self.buffer else {
            return;
        };
        window.attach(Some(buffer.wl_buffer()), 0, 0);
        window
            .wl_surface()
            .damage_buffer(0, 0, self.size.0 as i32, self.size.1 as i32);
        window.commit();
        self.mapped = true;
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
                PointerEventKind::Press { button: BTN_LEFT, serial, .. } => {
                    if let Some(seat) = &self.pointer_seat {
                        self.window.move_(seat, serial);
                    }
                }
                _ => {}
            }
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
                let notice = match self.clipboard.copy_image_to_clipboard(self.png.clone()) {
                    Ok(()) => Notice::Copied(None),
                    Err(e) => Notice::CopyFailed(e.to_string()),
                };
                notify::send_blocking(notice);
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
        _: &wl_output::WlOutput,
    ) {
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

delegate_compositor!(Pin);
delegate_output!(Pin);
delegate_shm!(Pin);
delegate_seat!(Pin);
delegate_keyboard!(Pin);
delegate_pointer!(Pin);
delegate_xdg_shell!(Pin);
delegate_xdg_window!(Pin);
delegate_registry!(Pin);
