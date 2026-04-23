// FIXME: Raw and EXPERIMENTAL implementation of ext-data-control. 
// suspicion of Undefined Behavior regarding process forking and wayland proxy lifecycle
// requires a complete audit of the background worker logic

use crate::backend::ClipboardProvider;

use std::io::Write;
use std::os::fd::{FromRawFd, IntoRawFd};
use std::fs::File;

use wayland_client::{
    protocol::{wl_registry, wl_seat},
    Connection, Dispatch, QueueHandle, Proxy,
};
use wayland_protocols::ext::data_control::v1::client::{
    ext_data_control_offer_v1::{self, ExtDataControlOfferV1},
    ext_data_control_device_v1::{self, ExtDataControlDeviceV1},
    ext_data_control_manager_v1::{self, ExtDataControlManagerV1},
    ext_data_control_source_v1::{self, ExtDataControlSourceV1},
};

pub struct ClipboardMethod {
    pub connection: wayland_client::Connection,
}

impl ClipboardProvider for ClipboardMethod {
    fn copy_image_to_clipboard(
        &self,
        png_data: Vec<u8>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        copy_image_to_clipboard(png_data, &self.connection)
    }
}

struct ClipboardState {
    seat: Option<wl_seat::WlSeat>,
    manager: Option<ExtDataControlManagerV1>,
    device: Option<ExtDataControlDeviceV1>,
    png_data: Vec<u8>,
    cancelled: bool,
}

impl Dispatch<wl_registry::WlRegistry, ()> for ClipboardState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global { name, interface, version } => {
                if interface == wl_seat::WlSeat::interface().name {
                    state.seat = Some(registry.bind(name, version, qh, ()));
                } else if interface == ExtDataControlManagerV1::interface().name {
                    state.manager = Some(registry.bind(name, version, qh, ()));
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtDataControlOfferV1, ()> for ClipboardState {
    fn event(_: &mut Self, _: &ExtDataControlOfferV1, _: ext_data_control_offer_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<wl_seat::WlSeat, ()> for ClipboardState {
    fn event(_: &mut Self, _: &wl_seat::WlSeat, _: wl_seat::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ExtDataControlManagerV1, ()> for ClipboardState {
    fn event(_: &mut Self, _: &ExtDataControlManagerV1, _: ext_data_control_manager_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<ExtDataControlDeviceV1, ()> for ClipboardState {
    fn event(_: &mut Self, _: &ExtDataControlDeviceV1, _: ext_data_control_device_v1::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {}
    
    wayland_client::event_created_child!(ClipboardState, ExtDataControlDeviceV1, [
        ext_data_control_device_v1::EVT_DATA_OFFER_OPCODE => (ExtDataControlOfferV1, ())
    ]);
}

impl Dispatch<ExtDataControlSourceV1, ()> for ClipboardState {
    fn event(
        state: &mut Self,
        _: &ExtDataControlSourceV1,
        event: ext_data_control_source_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_data_control_source_v1::Event::Send { mime_type, fd } => {
                if mime_type == "image/png"
                    || mime_type == "application/x-qt-image"
                    || mime_type == "x-kde-force-image-copy"
                {
                    let mut f = unsafe { File::from_raw_fd(fd.into_raw_fd()) };
                    let _ = f.write_all(&state.png_data);
                }
            }
            ext_data_control_source_v1::Event::Cancelled => {
                state.cancelled = true;
            }
            _ => {}
        }
    }
}

pub fn copy_image_to_clipboard(
    png_data: Vec<u8>,
    wayland_connection: &wayland_client::Connection,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = wayland_connection;
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();

    let display = conn.display();
    display.get_registry(&qh, ());

    let mut state = ClipboardState {
        seat: None,
        manager: None,
        device: None,
        png_data,
        cancelled: false,
    };

    event_queue.roundtrip(&mut state)?;

    let seat = state.seat.as_ref().ok_or("no wl_seat")?;
    let manager = state.manager.as_ref().ok_or("no ext_data_control_manager")?;

    let source = manager.create_data_source(&qh, ());
    source.offer("image/png".to_string());
    source.offer("application/x-qt-image".to_string());
    source.offer("x-kde-force-image-copy".to_string());

    let device = manager.get_data_device(seat, &qh, ());
    device.set_selection(Some(&source));
    state.device = Some(device);

    unsafe {
        match nix::unistd::fork()? {
            nix::unistd::ForkResult::Parent { .. } => return Ok(()),
            nix::unistd::ForkResult::Child => {}
        }
    }

    while !state.cancelled {
        event_queue.blocking_dispatch(&mut state)?;
    }

    Ok(())
}