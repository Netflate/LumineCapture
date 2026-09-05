// ── pinned screenshot ────────────────────────────────────────────────────────
// separate process `--pin`, use top level of layer-shell (not overlay)
use smithay_client_toolkit::reexports::protocols::wp::cursor_shape::v1::client::wp_cursor_shape_device_v1::{
    Shape, WpCursorShapeDeviceV1,
};
use smithay_client_toolkit::reexports::protocols::wp::relative_pointer::zv1::client::zwp_relative_pointer_v1::ZwpRelativePointerV1;
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::seat::keyboard::{
    KeyEvent, KeyboardHandler, Keysym, Modifiers, RawModifiers, RepeatInfo,
};
use smithay_client_toolkit::seat::pointer::cursor_shape::CursorShapeManager;
use smithay_client_toolkit::seat::pointer::{PointerEvent, PointerEventKind, PointerHandler};
use smithay_client_toolkit::seat::relative_pointer::{
    RelativeMotionEvent, RelativePointerHandler, RelativePointerState,
};
use smithay_client_toolkit::seat::{Capability, SeatHandler, SeatState};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use smithay_client_toolkit::{
    delegate_compositor, delegate_keyboard, delegate_layer, delegate_output, delegate_pointer,
    delegate_registry, delegate_relative_pointer, delegate_seat, delegate_shm, registry_handlers,
};
use tiny_skia::{FillRule, IntSize, Mask, Pixmap, Rect, Transform};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface};
use wayland_client::{Connection, QueueHandle};

use crate::backend::notify::{self, Notice};
use crate::backend::{ClipboardProvider, initialize_clipboard};
use crate::renderer::paths::{draw_panel_border, rounded_rect_path};
use crate::theme::radius;
use crate::utils::copy_swizzled;

const BTN_LEFT: u32 = 0x110;
const KEY_C: u32 = 46;

pub fn run(image: &[u8], at: Option<(i32, i32)>) -> Result<(), Box<dyn std::error::Error>> {
    let frame = render_pin(image)?;
    let size = (frame.width(), frame.height());

    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();

    let shm = Shm::bind(&globals, &qh)?;
    let pool = SlotPool::new((size.0 * size.1 * 4) as usize, &shm)?;

    let mut pin = Pin {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        seat: SeatState::new(&globals, &qh),
        cursor_shapes: CursorShapeManager::bind(&globals, &qh).ok(),
        relative: RelativePointerState::bind(&globals, &qh),
        clipboard: initialize_clipboard(conn.clone()),
        compositor: CompositorState::bind(&globals, &qh)?,
        layer_shell: LayerShell::bind(&globals, &qh)?,
        shm,
        pool,
        layer: None,
        output: None,
        origin: (0, 0),
        mapped: false,
        frame: Some(frame),
        size,
        buffer: None,
        png: image.to_vec(),
        margin: (0, 0),
        drag: None,
        cursor: None,
        _relative_pointer: None,
        enter_serial: 0,
        ctrl: false,
        exit: false,
    };

    // two roundtrips: first gets globals
    // second waits for logical monitor positions
    queue.roundtrip(&mut pin)?;
    queue.roundtrip(&mut pin)?;

    let (output, origin, margin) = pin
        .place(at, (size.0 as i32, size.1 as i32), None)
        .ok_or("no monitors found")?;
    pin.attach_to(&qh, output, origin, margin);

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

type Spot = (wl_output::WlOutput, (i32, i32), (i32, i32));

struct Drag {
    from: (i32, i32),
    moved: (f64, f64),
}

struct Pin {
    registry: RegistryState,
    outputs: OutputState,
    seat: SeatState,
    cursor_shapes: Option<CursorShapeManager>,
    relative: RelativePointerState,
    clipboard: Box<dyn ClipboardProvider>,
    compositor: CompositorState,
    layer_shell: LayerShell,
    shm: Shm,
    pool: SlotPool,

    layer: Option<LayerSurface>,
    output: Option<wl_output::WlOutput>,
    origin: (i32, i32),
    mapped: bool,
    frame: Option<Pixmap>,
    size: (u32, u32),
    buffer: Option<Buffer>,
    png: Vec<u8>,

    margin: (i32, i32),
    drag: Option<Drag>,
    cursor: Option<WpCursorShapeDeviceV1>,
    _relative_pointer: Option<ZwpRelativePointerV1>,
    enter_serial: u32,
    ctrl: bool,
    exit: bool,
}

impl Pin {
    /// specefically made it appear exactly where it was on the screen
    fn place(
        &self,
        at: Option<(i32, i32)>,
        size: (i32, i32),
        skip: Option<&wl_output::WlOutput>,
    ) -> Option<Spot> {
        let screens: Vec<_> = self
            .outputs
            .outputs()
            .filter(|output| Some(output) != skip)
            .filter_map(|output| {
                let info = self.outputs.info(&output)?;
                Some((output, info.logical_position?, info.logical_size?))
            })
            .collect();

        if let Some((x, y)) = at
            && let Some((output, pos, _)) = screens
                .iter()
                .find(|(_, p, s)| x >= p.0 && x < p.0 + s.0 && y >= p.1 && y < p.1 + s.1)
        {
            return Some((output.clone(), *pos, (x - pos.0, y - pos.1)));
        }

        let (output, pos, area) = screens.into_iter().next()?;
        let margin = match at {
            Some((x, y)) => (
                (x - pos.0).clamp(0, (area.0 - size.0).max(0)),
                (y - pos.1).clamp(0, (area.1 - size.1).max(0)),
            ),
            None => ((area.0 - size.0) / 2, (area.1 - size.1) / 2),
        };
        Some((output, pos, margin))
    }

    // we need new surface only in case if original (where pin created) monitor was changed
    fn attach_to(
        &mut self,
        qh: &QueueHandle<Self>,
        output: wl_output::WlOutput,
        origin: (i32, i32),
        margin: (i32, i32),
    ) {
        let surface = self.compositor.create_surface(qh);
        let layer = self.layer_shell.create_layer_surface(
            qh,
            surface,
            Layer::Top,
            Some("lumine-pin"),
            Some(&output),
        );
        layer.set_anchor(Anchor::TOP | Anchor::LEFT);
        layer.set_size(self.size.0, self.size.1);
        layer.set_exclusive_zone(-1);
        layer.set_keyboard_interactivity(KeyboardInteractivity::OnDemand);
        layer.set_margin(margin.1, 0, 0, margin.0);
        layer.wl_surface().commit();

        self.layer = Some(layer);
        self.output = Some(output);
        self.origin = origin;
        self.margin = margin;
        self.mapped = false;
        self.drag = None;
    }

    fn set_cursor(&self, shape: Shape) {
        if let Some(device) = &self.cursor {
            device.set_shape(self.enter_serial, shape);
        }
    }
}

impl LayerShellHandler for Pin {
    // when monitor disconnected
    fn closed(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &LayerSurface) {
        let at = (self.origin.0 + self.margin.0, self.origin.1 + self.margin.1);
        let size = (self.size.0 as i32, self.size.1 as i32);
        match self.place(Some(at), size, self.output.as_ref()) {
            Some((output, origin, margin)) => self.attach_to(qh, output, origin, margin),
            None => self.exit = true,
        }
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        _: LayerSurfaceConfigure,
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
        layer.attach(Some(buffer.wl_buffer()), 0, 0);
        layer
            .wl_surface()
            .damage_buffer(0, 0, self.size.0 as i32, self.size.1 as i32);
        layer.wl_surface().commit();
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
            match event.kind {
                PointerEventKind::Enter { serial } => {
                    self.enter_serial = serial;
                    let shape = if self.drag.is_some() { Shape::Grabbing } else { Shape::Grab };
                    self.set_cursor(shape);
                }
                PointerEventKind::Press { button: BTN_LEFT, .. } => {
                    self.drag = Some(Drag {
                        from: self.margin,
                        moved: (0.0, 0.0),
                    });
                    self.set_cursor(Shape::Grabbing);
                }
                PointerEventKind::Release { button: BTN_LEFT, .. } => {
                    self.drag = None;
                    self.set_cursor(Shape::Grab);
                }
                _ => {}
            }
        }
    }
}

impl RelativePointerHandler for Pin {
    fn relative_pointer_motion(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &ZwpRelativePointerV1,
        _: &wl_pointer::WlPointer,
        event: RelativeMotionEvent,
    ) {
        let Some(drag) = self.drag.as_mut() else {
            return;
        };
        drag.moved.0 += event.delta.0;
        drag.moved.1 += event.delta.1;
        let margin = (
            drag.from.0 + drag.moved.0.round() as i32,
            drag.from.1 + drag.moved.1.round() as i32,
        );
        if margin == self.margin {
            return;
        }
        self.margin = margin;
        if let Some(layer) = &self.layer {
            layer.set_margin(margin.1, 0, 0, margin.0);
            layer.wl_surface().commit();
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
        let is_c = matches!(event.keysym, Keysym::c | Keysym::C) || event.raw_code == KEY_C;
        if event.keysym == Keysym::Escape {
            self.exit = true;
        } else if self.ctrl && is_c {
            let notice = match self.clipboard.copy_image_to_clipboard(self.png.clone()) {
                Ok(()) => Notice::Copied(None),
                Err(e) => Notice::CopyFailed(e.to_string()),
            };
            notify::send_blocking(notice);
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
        self.ctrl = modifiers.ctrl;
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
        self.ctrl = false;
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
                self._relative_pointer = self.relative.get_relative_pointer(&pointer, qh).ok();
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
delegate_relative_pointer!(Pin);
delegate_layer!(Pin);
delegate_registry!(Pin);
