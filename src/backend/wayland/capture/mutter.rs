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
use log::warn;
use zbus::zvariant::{OwnedObjectPath, Value};
use zbus::{Connection, proxy};

use crate::backend::CaptureMethod;
use crate::backend::wayland::capture::portal::PortalMethod;
use crate::backend::wayland::capture::stream;
use crate::types::{CaptureResult, MonitorFrame, Output, StreamInfo};

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

#[async_trait]
impl CaptureMethod for MutterMethod {
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
