// ── GNOME Mutter ScreenCast capture backend ────────────────────────────────
//
// Bypasses xdg-desktop-portal, talks to mutter's private org.gnome.Mutter.ScreenCast directly
// The interface has no stability promise and can be locked down
#[derive(Default)]
pub struct MutterMethod {
    hold: AtomicBool,
    held: Mutex<Option<SessionProxy<'static>>>,
}

use std::collections::HashMap;
use std::error::Error;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use futures::StreamExt;
use log::{debug, warn};
use zbus::zvariant::{OwnedObjectPath, Value};
use zbus::{Connection, proxy};

use crate::backend::CaptureMethod;
use crate::backend::gnome;
use crate::backend::wayland::capture::portal::PortalMethod;
use crate::backend::wayland::capture::stream;
use crate::types::{CaptureResult, MonitorFrame, Output, StreamInfo};
use tiny_skia::{IntSize, Pixmap};

type BoxErr = Box<dyn Error + Send + Sync>;

const NODE_TIMEOUT: Duration = Duration::from_secs(5);

#[proxy(
    interface = "org.gnome.Mutter.ScreenCast",
    default_service = "org.gnome.Mutter.ScreenCast",
    default_path = "/org/gnome/Mutter/ScreenCast"
)]
trait ScreenCast {
    fn create_session(&self, properties: HashMap<&str, Value<'_>>)
    -> zbus::Result<OwnedObjectPath>;
}

#[proxy(
    interface = "org.gnome.Mutter.ScreenCast.Session",
    default_service = "org.gnome.Mutter.ScreenCast"
)]
trait Session {
    fn start(&self) -> zbus::Result<()>;

    fn stop(&self) -> zbus::Result<()>;

    fn record_monitor(
        &self,
        connector: &str,
        properties: HashMap<&str, Value<'_>>,
    ) -> zbus::Result<OwnedObjectPath>;

    fn record_window(&self, properties: HashMap<&str, Value<'_>>) -> zbus::Result<OwnedObjectPath>;
}

#[proxy(
    interface = "org.gnome.Mutter.ScreenCast.Stream",
    default_service = "org.gnome.Mutter.ScreenCast"
)]
trait Stream {
    #[zbus(signal)]
    fn pipe_wire_stream_added(&self, node_id: u32) -> zbus::Result<()>;
}

async fn record(
    conn: &Connection,
    session: &SessionProxy<'_>,
    outputs: &[Output],
) -> Result<CaptureResult, BoxErr> {
    let mut listeners = Vec::with_capacity(outputs.len());

    for (idx, output) in outputs.iter().enumerate() {
        let connector = output
            .info
            .name
            .as_deref()
            .ok_or("the output has no connector name")?;

        // cursor-mode 0 is hidden, same as the portal path, where it stays out of the pixels
        //
        // is-recording swaps the sharing pill, which gnome-shell keeps for at least 5 s,
        // for a small dot that goes away with the session
        let props = HashMap::from([
            ("cursor-mode", Value::U32(0)),
            ("is-recording", Value::Bool(true)),
        ]);
        let path = session.record_monitor(connector, props).await?;
        let stream = StreamProxy::builder(conn).path(path)?.build().await?;

        // mutter announces the node once, right after Start, so listen first
        listeners.push((idx, stream.receive_pipe_wire_stream_added().await?));
    }

    gnome::claim_recording(conn).await;
    session.start().await?;

    let mut frames = Vec::with_capacity(listeners.len());
    for (idx, mut added) in listeners {
        let signal = tokio::time::timeout(NODE_TIMEOUT, added.next())
            .await
            .map_err(|_| "mutter didn't announce a PipeWire node in time")?
            .ok_or("mutter closed the stream before announcing a node")?;
        let node_id = signal.args()?.node_id;

        let frame = stream::capture_frame(node_id, None).map_err(|e| e.to_string())?;

        let o = &outputs[idx];
        frames.push(MonitorFrame {
            output: idx,
            pixels: frame.pixels,
            pw_width: frame.width,
            pw_height: frame.height,
            pw_stride: frame.stride,
            info: StreamInfo {
                node_id,
                size: o.info.logical_size,
                position: o.info.logical_position,
            },
        });
    }

    Ok(CaptureResult { frames })
}

async fn stop(session: &SessionProxy<'_>) {
    if let Err(e) = session.stop().await {
        warn!("Can't stop mutter screencast session: {e}");
    }
}

async fn capture(
    outputs: &[Output],
    hold: bool,
) -> Result<(CaptureResult, Option<SessionProxy<'static>>), BoxErr> {
    let conn = Connection::session().await?;
    // while this started session lives gnome-shell turns animations off
    // the app stops it once the window is gone
    let props = if hold {
        HashMap::from([("disable-animations", Value::Bool(true))])
    } else {
        HashMap::new()
    };
    let path = ScreenCastProxy::new(&conn)
        .await?
        .create_session(props)
        .await?;
    let session = SessionProxy::builder(&conn).path(path)?.build().await?;

    let result = record(&conn, &session, outputs).await;

    match result {
        Ok(frames) if hold => Ok((frames, Some(session))),
        result => {
            stop(&session).await;
            Ok((result?, None))
        }
    }
}

fn window_rect(
    frame: (i32, i32, i32, i32),
    buffer: (i32, i32, i32, i32),
    crop: (u32, u32, u32, u32),
) -> Result<(u32, u32, u32, u32, f64), BoxErr> {
    // Mutter may clip either crop axis to the monitor-sized stream.
    let k = (crop.2 as f64 / buffer.2 as f64).max(crop.3 as f64 / buffer.3 as f64);
    if !k.is_finite() || k <= 0.0 || buffer.2 <= 0 || buffer.3 <= 0 {
        return Err("invalid Mutter window geometry".into());
    }
    let x = crop.0 as f64 + ((frame.0 - buffer.0) as f64 * k).round();
    let y = crop.1 as f64 + ((frame.1 - buffer.1) as f64 * k).round();
    let right = x + (frame.2 as f64 * k).round();
    let bottom = y + (frame.3 as f64 * k).round();
    let left = x.max(crop.0 as f64).max(0.0);
    let top = y.max(crop.1 as f64).max(0.0);
    let right = right.min((crop.0 + crop.2) as f64);
    let bottom = bottom.min((crop.1 + crop.3) as f64);
    if right <= left || bottom <= top {
        return Err("active window frame is empty".into());
    }
    Ok((
        left as u32,
        top as u32,
        right as u32 - left as u32,
        bottom as u32 - top as u32,
        k,
    ))
}

async fn capture_window() -> Result<crate::types::Capture, BoxErr> {
    let conn = Connection::session().await?;
    let focused = gnome::focus_window(&conn).await.map_err(|e| {
        if matches!(
            &e,
            zbus::Error::MethodError(name, _, _)
                if name.as_str() == "org.freedesktop.DBus.Error.ServiceUnknown"
        ) {
            debug!("{e}");
            "capturing the active window on GNOME needs the LumineCapture GNOME Shell extension. If you just installed the package, log out and back in, then run:\n  gnome-extensions enable lumine-capture@netflate.github.io".to_string()
        } else {
            format!("GNOME Shell extension call failed: {e}")
        }
    })?;
    let path = ScreenCastProxy::new(&conn)
        .await?
        .create_session(HashMap::new())
        .await?;
    let session = SessionProxy::builder(&conn).path(path)?.build().await?;
    let result = async {
        let stream_path = session
            .record_window(HashMap::from([
                ("window-id", Value::U64(focused.id)),
                ("cursor-mode", Value::U32(0)),
                ("is-recording", Value::Bool(true)),
            ]))
            .await?;
        let stream_proxy = StreamProxy::builder(&conn)
            .path(stream_path)?
            .build()
            .await?;
        let mut added = stream_proxy.receive_pipe_wire_stream_added().await?;
        gnome::claim_recording(&conn).await;
        session.start().await?;
        let signal = tokio::time::timeout(NODE_TIMEOUT, added.next())
            .await
            .map_err(|_| "mutter didn't announce a PipeWire node in time")?
            .ok_or("mutter closed the stream before announcing a node")?;
        let frame =
            stream::capture_window(signal.args()?.node_id, None).map_err(|e| e.to_string())?;
        let crop = frame.crop.ok_or("Mutter didn't report the window size")?;
        let (x, y, width, height, k) = window_rect(focused.frame, focused.buffer, crop)?;
        let size = IntSize::from_wh(width, height).ok_or("empty active window image")?;
        let mut pixmap = Pixmap::new(size.width(), size.height())
            .ok_or("couldn't allocate active window image")?;
        let row_bytes = size.width() as usize * 4;
        for (dst, row) in pixmap.data_mut().chunks_exact_mut(row_bytes).zip(0..) {
            let start = (y as usize + row) * frame.stride as usize + x as usize * 4;
            let src = frame
                .pixels
                .get(start..start + row_bytes)
                .ok_or("PipeWire window row is truncated")?;
            dst.copy_from_slice(src);
        }
        Ok(crate::types::Capture {
            pixmap,
            scale: k as f32,
        })
    }
    .await;
    stop(&session).await;
    result
}

#[async_trait]
impl CaptureMethod for MutterMethod {
    async fn capture_active_window(
        &self,
    ) -> Result<crate::types::Capture, Box<dyn std::error::Error>> {
        capture_window().await.map_err(|e| e.to_string().into())
    }
    async fn capture_frame(
        &self,
        outputs: &[Output],
    ) -> Result<CaptureResult, Box<dyn std::error::Error>> {
        let hold = self.hold.swap(false, Ordering::Relaxed);
        match capture(outputs, hold).await {
            Ok((result, held)) => {
                *self.held.lock().unwrap() = held;
                Ok(result)
            }
            Err(e) => {
                warn!("Mutter screencast failed ({e}), falling back to the portal");
                PortalMethod.capture_frame(outputs).await
            }
        }
    }

    fn hold_session(&self) {
        self.hold.store(true, Ordering::Relaxed);
    }

    async fn release(&self) {
        let held = self.held.lock().unwrap().take();
        if let Some(session) = held {
            stop(&session).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::window_rect;

    #[test]
    fn window_rect_removes_client_side_shadow_and_scales_crop() {
        let frame = (114, 214, 972, 772);
        let buffer = (90, 190, 1020, 820);
        assert_eq!(
            window_rect(frame, buffer, (0, 0, 1020, 820)).unwrap(),
            (24, 24, 972, 772, 1.0)
        );
        assert_eq!(
            window_rect(frame, buffer, (0, 0, 2040, 1640)).unwrap(),
            (48, 48, 1944, 1544, 2.0)
        );
    }

    #[test]
    fn window_rect_uses_unclipped_axis_for_scale() {
        assert_eq!(
            window_rect(
                (10, 206, 1900, 700),
                (-13, 183, 1946, 746),
                (0, 0, 1920, 746)
            )
            .unwrap(),
            (23, 23, 1897, 700, 1.0)
        );
    }
}
