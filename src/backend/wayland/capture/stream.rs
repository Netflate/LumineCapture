use pipewire as pw;
use pw::{properties::properties, spa};
use spa::pod::Pod;
use std::sync::mpsc;
use std::os::fd::{BorrowedFd, OwnedFd};

struct UserData {
    format: spa::param::video::VideoInfoRaw,
}

pub struct PipewireFrame {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
}

pub fn capture_frame(
    node_id: u32,
    fd: BorrowedFd<'_>,
) -> Result<PipewireFrame, Box<dyn std::error::Error>> {
    let (tx, rx) = mpsc::sync_channel::<PipewireFrame>(1);

    let owned_fd: OwnedFd = fd.try_clone_to_owned()
        .map_err(|e| format!("Failed to clone file descriptor: {}", e))?;

    std::thread::spawn(move || {
        pw::init();

        let mainloop = pw::main_loop::MainLoopRc::new(None).expect("Failed to create main loop");
        let context = pw::context::ContextRc::new(&mainloop, None).expect("Failed to create context");

        let core = context.connect_fd_rc(owned_fd, None).expect("Failed to connect via FD");

        let data = UserData {
            format: Default::default(),
        };

        let stream = pw::stream::StreamRc::new(
            core.clone(), 
            "lumine-capture",
            properties! {
                *pw::keys::MEDIA_TYPE => "Video",
                *pw::keys::MEDIA_CATEGORY => "Capture",
                *pw::keys::MEDIA_ROLE => "Screen",
            },
        )
        .expect("Failed to create stream");

        let mainloop_clone = mainloop.clone();
        let tx_clone = tx.clone();

        let _listener = stream
            .add_local_listener_with_user_data(data)
            .param_changed(|_, user_data, id, param| {
                let Some(param) = param else { return };
                if id != spa::param::ParamType::Format.as_raw() {
                    return;
                }

                let (media_type, media_subtype) =
                    match spa::param::format_utils::parse_format(param) {
                        Ok(v) => v,
                        Err(_) => return,
                    };

                if media_type != spa::param::format::MediaType::Video
                    || media_subtype != spa::param::format::MediaSubtype::Raw
                {
                    return;
                }

                user_data.format.parse(param).expect("Failed to parse format");
            })
            .process(move |stream, user_data| {
                if let Some(mut buffer) = stream.dequeue_buffer() {
                    let datas = buffer.datas_mut();
                    if let Some(data) = datas.first_mut() {
                        let (size, chunk_stride) = {
                            let chunk = data.chunk();
                            (chunk.size() as usize, chunk.stride())
                        };
                        if size == 0 {
                            return;
                        }

                        if let Some(bytes) = data.data() {
                            let width = user_data.format.size().width;
                            let height = user_data.format.size().height;
                            if width == 0 || height == 0 {
                                return;
                            }

                            let default_stride = width.saturating_mul(4);
                            let stride = match chunk_stride {
                                s if s > 0 => s as u32,
                                _ => default_stride,
                            };

                            let frame_data = PipewireFrame {
                                pixels: bytes[..size].to_vec(),
                                width,
                                height,
                                stride,
                            };
                            let _ = tx_clone.send(frame_data);
                            mainloop_clone.quit();
                        }
                    }
                }
            })
            .register()
            .expect("Failed to register stream listener");

        let mut params_buf = Vec::new();
        let pod = build_format_pod(&mut params_buf);

        stream
            .connect(
                spa::utils::Direction::Input,
                Some(node_id),
                pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
                &mut [pod],
            )
            .expect("Failed to connect stream");

        mainloop.run();
    });

    rx.recv().map_err(|e| format!("Failed to receive frame from thread: {}", e).into())
}

fn build_format_pod<'a>(buffer: &'a mut Vec<u8>) -> &'a Pod {
    use spa::pod::serialize::PodSerializer;
    use spa::pod::{Object, Property, PropertyFlags, Value};
    use spa::sys::*;
    use spa::utils::Id;

    PodSerializer::serialize(
        std::io::Cursor::new(&mut *buffer),
        &Value::Object(Object {
            type_: SPA_TYPE_OBJECT_Format,
            id: SPA_PARAM_EnumFormat,
            properties: vec![
                Property {
                    key: SPA_FORMAT_mediaType,
                    flags: PropertyFlags::empty(),
                    value: Value::Id(Id(SPA_MEDIA_TYPE_video)),
                },
                Property {
                    key: SPA_FORMAT_mediaSubtype,
                    flags: PropertyFlags::empty(),
                    value: Value::Id(Id(SPA_MEDIA_SUBTYPE_raw)),
                },
                Property {                                                                  // TODO: I've heard some sources may not be able to give an BGRA frame
                    key: SPA_FORMAT_VIDEO_format,                                           // so we need to support most of the possible formats I guess? 
                    flags: PropertyFlags::empty(),                                          // at some point will be necessary to make a research about it  
                    value: Value::Id(Id(spa::param::video::VideoFormat::BGRA.as_raw())),    // to find out if it's really an issue or not
                },
            ],
        }),
    )
    .unwrap();

    Pod::from_bytes(buffer).unwrap()
}
