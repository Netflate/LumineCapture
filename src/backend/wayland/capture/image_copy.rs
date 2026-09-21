// ── Wayland ext-image-copy-capture backend ─────────────────────────────────
//
// Bypasses xdg-desktop-portal entirely, uses standard Wayland which is faster
// and doesn't ask for monitor choice as Portal does
use std::time::{Duration, Instant};

use async_trait::async_trait;
use rustix::event::{PollFd, PollFlags, poll};
use rustix::time::Timespec;
use smithay_client_toolkit::delegate_dispatch2;
use smithay_client_toolkit::shm::slot::{Buffer, SlotPool};
use smithay_client_toolkit::shm::{Shm, ShmHandler};
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::{wl_output, wl_registry, wl_shm};
use wayland_client::{Connection, Dispatch, EventQueue, QueueHandle, WEnum};
use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_image_capture_source_v1::ExtImageCaptureSourceV1,
    ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_frame_v1::{self, ExtImageCopyCaptureFrameV1},
    ext_image_copy_capture_manager_v1::{ExtImageCopyCaptureManagerV1, Options},
    ext_image_copy_capture_session_v1::{self, ExtImageCopyCaptureSessionV1},
};

use crate::backend::CaptureMethod;
use crate::types::{CaptureResult, MonitorFrame, Output, StreamInfo};
use crate::utils::to_rgba;

const FRAME_TIMEOUT: Duration = Duration::from_secs(5);

pub struct ImageCopyMethod {
    conn: Connection,
}

impl ImageCopyMethod {
    pub fn new(conn: Connection) -> Self {
        Self { conn }
    }
}

pub fn supported(conn: &Connection) -> bool {
    let Ok((globals, _)) = registry_queue_init::<State>(conn) else {
        return false;
    };
    globals.contents().with_list(|list| {
        let has = |name: &str| list.iter().any(|g| g.interface == name);
        has("ext_output_image_capture_source_manager_v1")
            && has("ext_image_copy_capture_manager_v1")
    })
}

struct Target {
    wl_output: wl_output::WlOutput,
    size: Option<(i32, i32)>,
    position: Option<(i32, i32)>,
}

#[derive(Default)]
struct Slot {
    size: Option<(u32, u32)>,
    formats: Vec<wl_shm::Format>,
    constraints_done: bool,
    transform: Option<wl_output::Transform>,
    result: Option<Result<(), String>>,
}

struct State {
    shm: Shm,
    slots: Vec<Slot>,
}

#[async_trait]
impl CaptureMethod for ImageCopyMethod {
    async fn capture_frame(
        &self,
        outputs: &[Output],
    ) -> Result<CaptureResult, Box<dyn std::error::Error>> {
        let conn = self.conn.clone();
        let targets: Vec<Target> = outputs
            .iter()
            .map(|o| Target {
                wl_output: o.wl_output.clone(),
                size: o.info.logical_size,
                position: o.info.logical_position,
            })
            .collect();

        let frames = tokio::task::spawn_blocking(move || capture_all(&conn, &targets)).await??;
        Ok(CaptureResult { frames })
    }
}

fn capture_all(conn: &Connection, targets: &[Target]) -> Result<Vec<MonitorFrame>, String> {
    // separate registry queue from overlay backend, but still the same connection
    let (globals, mut queue) =
        registry_queue_init::<State>(conn).map_err(|e| format!("wayland registry: {e}"))?;
    let qh = queue.handle();

    let sources_mgr: ExtOutputImageCaptureSourceManagerV1 = globals
        .bind(&qh, 1..=1, ())
        .map_err(|e| format!("ext_output_image_capture_source_manager_v1: {e}"))?;
    let copy_mgr: ExtImageCopyCaptureManagerV1 = globals
        .bind(&qh, 1..=1, ())
        .map_err(|e| format!("ext_image_copy_capture_manager_v1: {e}"))?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| format!("wl_shm: {e}"))?;

    let mut state = State {
        shm,
        slots: targets.iter().map(|_| Slot::default()).collect(),
    };
    let deadline = Instant::now() + FRAME_TIMEOUT;

    let sessions: Vec<(ExtImageCaptureSourceV1, ExtImageCopyCaptureSessionV1)> = targets
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let source = sources_mgr.create_source(&t.wl_output, &qh, ());
            let session = copy_mgr.create_session(&source, Options::empty(), &qh, i);
            (source, session)
        })
        .collect();

    dispatch_until(&mut queue, &mut state, deadline, |s| {
        s.slots
            .iter()
            .all(|slot| slot.constraints_done || slot.result.is_some())
    })?;
    check_failed(&state)?;

    let total: usize = state
        .slots
        .iter()
        .filter_map(|slot| slot.size)
        .map(|(w, h)| w as usize * h as usize * 4)
        .sum();
    let mut pool = SlotPool::new(total.max(1), &state.shm).map_err(|e| format!("shm pool: {e}"))?;

    let mut pending: Vec<(Buffer, bool, ExtImageCopyCaptureFrameV1)> = Vec::new();
    for (i, ((_, session), slot)) in sessions.iter().zip(&state.slots).enumerate() {
        let (w, h) = slot
            .size
            .ok_or_else(|| format!("output {i}: no buffer size"))?;
        let (format, bgr) = pick_format(&slot.formats)
            .ok_or_else(|| format!("output {i}: no usable shm format in {:?}", slot.formats))?;
        let (buffer, _) = pool
            .create_buffer(w as i32, h as i32, w as i32 * 4, format)
            .map_err(|e| format!("output {i}: shm buffer: {e}"))?;

        let frame = session.create_frame(&qh, i);
        frame.attach_buffer(buffer.wl_buffer());
        frame.damage_buffer(0, 0, w as i32, h as i32);
        frame.capture();
        pending.push((buffer, bgr, frame));
    }

    dispatch_until(&mut queue, &mut state, deadline, |s| {
        s.slots.iter().all(|slot| slot.result.is_some())
    })?;
    check_failed(&state)?;

    let mut frames = Vec::with_capacity(targets.len());
    for (i, ((buffer, bgr, frame), target)) in pending.into_iter().zip(targets).enumerate() {
        let slot = &state.slots[i];
        let (w, h) = slot.size.unwrap_or_default();

        let canvas = buffer
            .canvas(&mut pool)
            .ok_or_else(|| format!("output {i}: shm buffer is busy"))?;
        let mut pixels = canvas[..w as usize * h as usize * 4].to_vec();
        to_rgba(&mut pixels, bgr);
        let (pixels, w, h) = untransform(
            pixels,
            w,
            h,
            slot.transform.unwrap_or(wl_output::Transform::Normal),
        );

        frame.destroy();
        frames.push(MonitorFrame {
            output: i,
            pixels,
            pw_width: w,
            pw_height: h,
            pw_stride: w * 4,
            info: StreamInfo {
                node_id: 0,
                size: target.size,
                position: target.position,
            },
        });
    }

    for (source, session) in sessions {
        session.destroy();
        source.destroy();
    }
    copy_mgr.destroy();
    sources_mgr.destroy();
    let _ = queue.flush();

    Ok(frames)
}

/// returning pixels to its normal orientation
/// Wayland returns pixel orientated as in the settings
///  
/// while it seems okay, its a problem, since everything else
/// work with the screenshot as if its not orientated
/// it wasn't a conscious choice, its implemented like that
/// because both portal and kde screenshot protocol returns
/// pixels without orientating them
fn untransform(
    pixels: Vec<u8>,
    w: u32,
    h: u32,
    transform: wl_output::Transform,
) -> (Vec<u8>, u32, u32) {
    use wl_output::Transform as T;
    let (flip, quarters) = match transform {
        T::_90 => (false, 3),
        T::_180 => (false, 2),
        T::_270 => (false, 1),
        T::Flipped => (true, 0),
        T::Flipped90 => (true, 1),
        T::Flipped180 => (true, 2),
        T::Flipped270 => (true, 3),
        _ => return (pixels, w, h),
    };

    let (w, h) = (w as usize, h as usize);
    let (dw, dh) = if quarters % 2 == 1 { (h, w) } else { (w, h) };
    let mut out = vec![0u8; pixels.len()];
    for y in 0..h {
        for x in 0..w {
            let fx = if flip { w - 1 - x } else { x };
            let (dx, dy) = match quarters {
                1 => (y, w - 1 - fx),
                2 => (w - 1 - fx, h - 1 - y),
                3 => (h - 1 - y, fx),
                _ => (fx, y),
            };
            let src = (y * w + x) * 4;
            let dst = (dy * dw + dx) * 4;
            out[dst..dst + 4].copy_from_slice(&pixels[src..src + 4]);
        }
    }
    (out, dw as u32, dh as u32)
}

fn pick_format(formats: &[wl_shm::Format]) -> Option<(wl_shm::Format, bool)> {
    [
        (wl_shm::Format::Xrgb8888, true),
        (wl_shm::Format::Argb8888, true),
        (wl_shm::Format::Xbgr8888, false),
        (wl_shm::Format::Abgr8888, false),
    ]
    .into_iter()
    .find(|(f, _)| formats.contains(f))
}

fn check_failed(state: &State) -> Result<(), String> {
    for (i, slot) in state.slots.iter().enumerate() {
        if let Some(Err(e)) = &slot.result {
            return Err(format!("output {i}: {e}"));
        }
    }
    Ok(())
}

fn dispatch_until(
    queue: &mut EventQueue<State>,
    state: &mut State,
    deadline: Instant,
    done: impl Fn(&State) -> bool,
) -> Result<(), String> {
    queue.flush().map_err(|e| e.to_string())?;
    loop {
        queue.dispatch_pending(state).map_err(|e| e.to_string())?;
        if done(state) {
            return Ok(());
        }

        let Some(guard) = queue.prepare_read() else {
            continue;
        };
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err("compositor didn't deliver the capture in time".into());
        }
        let timeout = Timespec {
            tv_sec: left.as_secs() as i64,
            tv_nsec: left.subsec_nanos() as i64,
        };
        let fd = guard.connection_fd();
        let mut fds = [PollFd::new(&fd, PollFlags::IN)];
        poll(&mut fds, Some(&timeout)).map_err(|e| e.to_string())?;
        if fds[0].revents().contains(PollFlags::IN) {
            match guard.read() {
                Ok(_) => {}
                Err(wayland_client::backend::WaylandError::Io(e))
                    if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.to_string()),
            }
        }
    }
}

impl ShmHandler for State {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_dispatch2!(State);

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for State {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtOutputImageCaptureSourceManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ExtOutputImageCaptureSourceManagerV1,
        _: <ExtOutputImageCaptureSourceManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtImageCaptureSourceV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ExtImageCaptureSourceV1,
        _: <ExtImageCaptureSourceV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtImageCopyCaptureManagerV1, ()> for State {
    fn event(
        _: &mut Self,
        _: &ExtImageCopyCaptureManagerV1,
        _: <ExtImageCopyCaptureManagerV1 as wayland_client::Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtImageCopyCaptureSessionV1, usize> for State {
    fn event(
        state: &mut Self,
        _: &ExtImageCopyCaptureSessionV1,
        event: ext_image_copy_capture_session_v1::Event,
        &idx: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_session_v1::Event;
        let slot = &mut state.slots[idx];
        match event {
            Event::BufferSize { width, height } => slot.size = Some((width, height)),
            Event::ShmFormat {
                format: WEnum::Value(format),
            } => slot.formats.push(format),
            Event::Done => slot.constraints_done = true,
            Event::Stopped => {
                slot.result
                    .get_or_insert(Err("capture session stopped".into()));
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtImageCopyCaptureFrameV1, usize> for State {
    fn event(
        state: &mut Self,
        _: &ExtImageCopyCaptureFrameV1,
        event: ext_image_copy_capture_frame_v1::Event,
        &idx: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use ext_image_copy_capture_frame_v1::Event;
        let slot = &mut state.slots[idx];
        match event {
            Event::Transform {
                transform: WEnum::Value(t),
            } => slot.transform = Some(t),
            Event::Ready => slot.result = Some(Ok(())),
            Event::Failed { reason } => {
                slot.result = Some(Err(format!("capture failed: {reason:?}")))
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(ids: &[u8]) -> Vec<u8> {
        ids.iter().flat_map(|&id| [id, id, id, 255]).collect()
    }

    fn ids(pixels: &[u8]) -> Vec<u8> {
        pixels.chunks_exact(4).map(|p| p[0]).collect()
    }

    #[test]
    fn rotated_frames_come_back_upright() {
        use wl_output::Transform as T;

        // 2 wide, 3 tall: a square source would hide every w/h and dw/dh mix-up
        //   1 2
        //   3 4
        //   5 6
        let source = [1, 2, 3, 4, 5, 6];

        // the mapping is inverted on purpose: T::_90 means the compositor already
        // turned the content, so undoing it takes the opposite quarter-turn
        let cases = [
            (T::Normal, [1, 2, 3, 4, 5, 6], (2, 3)),
            (T::_90, [5, 3, 1, 6, 4, 2], (3, 2)),
            (T::_180, [6, 5, 4, 3, 2, 1], (2, 3)),
            (T::_270, [2, 4, 6, 1, 3, 5], (3, 2)),
            (T::Flipped, [2, 1, 4, 3, 6, 5], (2, 3)),
            (T::Flipped90, [1, 3, 5, 2, 4, 6], (3, 2)),
            (T::Flipped180, [5, 6, 3, 4, 1, 2], (2, 3)),
            (T::Flipped270, [6, 4, 2, 5, 3, 1], (3, 2)),
        ];

        for (transform, expected, size) in cases {
            let (pixels, w, h) = untransform(img(&source), 2, 3, transform);
            assert_eq!(ids(&pixels), expected, "{transform:?}");
            assert_eq!((w, h), size, "{transform:?}");
            assert_eq!(
                pixels.len(),
                source.len() * 4,
                "{transform:?}: pixels went missing"
            );
        }
    }

    #[test]
    fn the_best_shm_format_wins_over_the_advertised_order() {
        use wl_shm::Format;

        let all = [
            Format::Abgr8888,
            Format::Xbgr8888,
            Format::Argb8888,
            Format::Xrgb8888,
        ];
        assert_eq!(pick_format(&all), Some((Format::Xrgb8888, true)));

        assert_eq!(
            pick_format(&[Format::Abgr8888, Format::Argb8888]),
            Some((Format::Argb8888, true))
        );
        assert_eq!(
            pick_format(&[Format::Xbgr8888]),
            Some((Format::Xbgr8888, false))
        );
        assert_eq!(pick_format(&[]), None);
        assert_eq!(pick_format(&[Format::Rgb565]), None);
    }
}
