use crate::utils::to_rgba;
use pipewire as pw;
use pw::{properties::properties, spa};
use spa::param::video::VideoFormat;
use spa::pod::Pod;
use std::os::fd::{BorrowedFd, OwnedFd};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::time::Duration;

struct UserData {
    format: spa::param::video::VideoInfoRaw,
}

pub struct PipewireFrame {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub crop: Option<(u32, u32, u32, u32)>,
}

const FRAME_TIMEOUT: Duration = Duration::from_secs(5);

type FrameResult = Result<PipewireFrame, String>;

/// `fd` is the portal's private PipeWire remote, `None` connects to the user's own instance
pub fn capture_frame(
    node_id: u32,
    fd: Option<BorrowedFd<'_>>,
) -> Result<PipewireFrame, Box<dyn std::error::Error>> {
    capture(node_id, fd, false)
}

/// Mutter's window stream
/// keeps alpha and reads the VideoCrop meta to `crop`
pub fn capture_window(
    node_id: u32,
    fd: Option<BorrowedFd<'_>>,
) -> Result<PipewireFrame, Box<dyn std::error::Error>> {
    capture(node_id, fd, true)
}

fn capture(
    node_id: u32,
    fd: Option<BorrowedFd<'_>>,
    window: bool,
) -> Result<PipewireFrame, Box<dyn std::error::Error>> {
    let (tx, rx) = mpsc::sync_channel::<FrameResult>(1);

    let owned_fd: Option<OwnedFd> = fd
        .map(|fd| fd.try_clone_to_owned())
        .transpose()
        .map_err(|e| format!("Failed to clone file descriptor: {}", e))?;

    std::thread::spawn(move || {
        if let Err(e) = run_stream(node_id, owned_fd, tx.clone(), window) {
            let _ = tx.try_send(Err(e));
        }
    });

    match rx.recv_timeout(FRAME_TIMEOUT) {
        Ok(frame) => frame.map_err(Into::into),
        Err(RecvTimeoutError::Timeout) => Err("PipeWire didn't deliver a frame in time".into()),
        Err(RecvTimeoutError::Disconnected) => {
            Err("PipeWire capture thread exited without a frame".into())
        }
    }
}

fn run_stream(
    node_id: u32,
    fd: Option<OwnedFd>,
    tx: SyncSender<FrameResult>,
    window: bool,
) -> Result<(), String> {
    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None)
        .map_err(|e| format!("Failed to create main loop: {e}"))?;
    let context = pw::context::ContextRc::new(&mainloop, None)
        .map_err(|e| format!("Failed to create context: {e}"))?;

    let core = match fd {
        Some(fd) => context.connect_fd_rc(fd, None),
        None => context.connect_rc(None),
    }
    .map_err(|e| format!("Failed to connect to PipeWire: {e}"))?;

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
    .map_err(|e| format!("Failed to create stream: {e}"))?;

    let mainloop_clone = mainloop.clone();
    let tx_clone = tx.clone();
    let mainloop_state = mainloop.clone();

    let _listener = stream
        .add_local_listener_with_user_data(data)
        .state_changed(move |_, _, _, new| {
            if let pw::stream::StreamState::Error(msg) = new {
                let _ = tx.try_send(Err(format!("PipeWire stream error: {msg}")));
                mainloop_state.quit();
            }
        })
        .param_changed(move |stream, user_data, id, param| {
            let Some(param) = param else { return };
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }

            let (media_type, media_subtype) = match spa::param::format_utils::parse_format(param) {
                Ok(v) => v,
                Err(_) => return,
            };

            if media_type != spa::param::format::MediaType::Video
                || media_subtype != spa::param::format::MediaSubtype::Raw
            {
                return;
            }

            let _ = user_data.format.parse(param);
            if window {
                let mut meta_buf = Vec::new();
                let meta_pod = build_meta_pod(&mut meta_buf);
                let _ = stream.update_params(&mut [meta_pod]);
            }
        })
        .process(move |stream, user_data| {
            if let Some(mut buffer) = stream.dequeue_buffer() {
                let crop = if window {
                    buffer
                        .find_meta::<spa::buffer::meta::MetaVideoCrop>()
                        .and_then(|meta| {
                            let position = meta.meta_region().position();
                            let size = meta.meta_region().size();
                            (size.width > 0 && size.height > 0).then_some((
                                position.x as u32,
                                position.y as u32,
                                size.width,
                                size.height,
                            ))
                        })
                } else {
                    None
                };
                if let Some(data) = buffer.datas_mut().first_mut() {
                    send_frame(
                        frame_from_data(data, user_data, crop, window),
                        &tx_clone,
                        &mainloop_clone,
                    );
                }
            }
        })
        .register()
        .map_err(|e| format!("Failed to register stream listener: {e}"))?;

    let mut params_buf = Vec::new();
    let pod = build_format_pod(&mut params_buf, window);

    stream
        .connect(
            spa::utils::Direction::Input,
            Some(node_id),
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut [pod],
        )
        .map_err(|e| format!("Failed to connect stream: {e}"))?;

    mainloop.run();
    Ok(())
}

fn build_format_pod(buffer: &mut Vec<u8>, window: bool) -> &Pod {
    use spa::pod::serialize::PodSerializer;
    use spa::pod::{ChoiceValue, Object, Property, PropertyFlags, Value};
    use spa::sys::*;
    use spa::utils::{Choice, ChoiceEnum, ChoiceFlags, Id};

    let formats = if window {
        [
            VideoFormat::BGRA,
            VideoFormat::RGBA,
            VideoFormat::BGRx,
            VideoFormat::RGBx,
        ]
    } else {
        [
            VideoFormat::BGRx,
            VideoFormat::BGRA,
            VideoFormat::RGBx,
            VideoFormat::RGBA,
        ]
    };

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
                Property {
                    // previously it supported only BGRA
                    // added some of the others since cosmic didn't return BGRA
                    key: SPA_FORMAT_VIDEO_format,
                    flags: PropertyFlags::empty(),
                    value: Value::Choice(ChoiceValue::Id(Choice(
                        ChoiceFlags::empty(),
                        ChoiceEnum::Enum {
                            default: Id(formats[0].as_raw()),
                            alternatives: formats.iter().map(|f| Id(f.as_raw())).collect(),
                        },
                    ))),
                },
            ],
        }),
    )
    .unwrap();

    Pod::from_bytes(buffer).unwrap()
}

fn build_meta_pod(buffer: &mut Vec<u8>) -> &Pod {
    use spa::pod::serialize::PodSerializer;
    use spa::pod::{Object, Property, PropertyFlags, Value};
    use spa::sys::*;
    use spa::utils::Id;
    PodSerializer::serialize(
        std::io::Cursor::new(&mut *buffer),
        &Value::Object(Object {
            type_: SPA_TYPE_OBJECT_ParamMeta,
            id: SPA_PARAM_Meta,
            properties: vec![
                Property {
                    key: SPA_PARAM_META_type,
                    flags: PropertyFlags::empty(),
                    value: Value::Id(Id(SPA_META_VideoCrop)),
                },
                Property {
                    key: SPA_PARAM_META_size,
                    flags: PropertyFlags::empty(),
                    value: Value::Int(std::mem::size_of::<spa_meta_region>() as i32),
                },
            ],
        }),
    )
    .unwrap();
    Pod::from_bytes(buffer).unwrap()
}

fn send_frame(
    result: Result<Option<PipewireFrame>, String>,
    tx: &SyncSender<FrameResult>,
    mainloop: &pw::main_loop::MainLoopRc,
) {
    match result {
        Ok(Some(frame)) => {
            let _ = tx.try_send(Ok(frame));
            mainloop.quit();
        }
        Ok(None) => {}
        Err(error) => {
            let _ = tx.try_send(Err(error));
            mainloop.quit();
        }
    }
}

fn frame_from_data(
    data: &mut spa::buffer::Data,
    user_data: &UserData,
    crop: Option<(u32, u32, u32, u32)>,
    keep_alpha: bool,
) -> Result<Option<PipewireFrame>, String> {
    let chunk = data.chunk();
    let (offset, size, chunk_stride) = (
        chunk.offset() as usize,
        chunk.size() as usize,
        chunk.stride(),
    );
    if size == 0 {
        return Ok(None);
    }
    let bytes = data
        .data()
        .ok_or_else(|| "PipeWire sent a buffer that can't be mapped (DMA-BUF?)".to_string())?;
    let dims = user_data.format.size();
    let (width, height) = (dims.width, dims.height);
    if width == 0 || height == 0 {
        return Ok(None);
    }
    let Some(end) = offset.checked_add(size) else {
        return Ok(None);
    };
    let Some(slice) = bytes.get(offset..end) else {
        return Ok(None);
    };
    let format = user_data.format.format();
    let bgr = match format {
        VideoFormat::BGRA | VideoFormat::BGRx => true,
        VideoFormat::RGBA | VideoFormat::RGBx => false,
        other => return Err(format!("PipeWire picked unsupported format {other:?}")),
    };
    let stride = if chunk_stride > 0 {
        chunk_stride as u32
    } else {
        width.saturating_mul(4)
    };
    let keep_alpha = keep_alpha && matches!(format, VideoFormat::BGRA | VideoFormat::RGBA);
    let mut pixels = slice.to_vec();
    to_rgba(&mut pixels, bgr, keep_alpha);
    Ok(Some(PipewireFrame {
        pixels,
        width,
        height,
        stride,
        crop,
    }))
}
