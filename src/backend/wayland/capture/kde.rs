// ── KDE KWin ScreenShot2 DBus capture backend ──────────────────────────────
//
// Bypasses xdg-desktop-portal entirely and talks directly to kwin's private protocol

use std::collections::HashMap;
use std::error::Error;
use std::os::fd::{AsFd, OwnedFd};

use async_trait::async_trait;
use tokio::sync::OnceCell;
use zbus::{proxy, Connection};
use zbus::zvariant::{Fd, OwnedValue, Value};

use crate::backend::CaptureMethod;
use crate::types::{CaptureResult, MonitorFrame, Output, StreamInfo};

// async_trait requires the whole future graph to be Send; std::error::Error
// alone isn't Send, so all internal helpers use this bound instead and only
// convert to the trait's plain Box<dyn Error> at the outer return boundary
type BoxErr = Box<dyn Error + Send + Sync>;

#[proxy(
    interface = "org.kde.KWin.ScreenShot2",
    default_service = "org.kde.KWin",
    default_path = "/org/kde/KWin/ScreenShot2"
)]
trait ScreenShot2 {
    async fn capture_screen(
        &self,
        name: &str,
        options: HashMap<&str, Value<'_>>,
        pipe: Fd<'_>,
    ) -> zbus::Result<HashMap<String, OwnedValue>>;
}

pub struct KdeMethod {
    conn: OnceCell<Connection>, // session bus connected lazily once, reused across calls
}

impl KdeMethod {
    pub fn new() -> Self {
        Self { conn: OnceCell::new() }
    }
}

// captures a single output by name; returns tight (unpadded) pixels
// no channel swap needed here
async fn capture_one_screen(
    proxy: &ScreenShot2Proxy<'_>,
    output_name: &str,
    dimensions: Option<(usize, usize)>, 
) -> Result<(Vec<u8>, u32, u32), BoxErr> {
    let (read_fd, write_fd): (OwnedFd, OwnedFd) = nix::unistd::pipe()?;

    let read_task = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
        use std::io::Read;
        let mut buf = match dimensions {
            Some((w, h)) => Vec::with_capacity(w.saturating_mul(h).saturating_mul(4)),
            None => Vec::new(), 
        };
        std::fs::File::from(read_fd).read_to_end(&mut buf)?;
        Ok(buf)
    });

    let result = proxy
        .capture_screen(output_name, HashMap::new(), Fd::from(write_fd.as_fd()))
        .await;

    drop(write_fd);

    let metadata = result?;
    let mut raw = read_task.await??;

    let width = metadata.get("width").and_then(|v| u32::try_from(v.clone()).ok())
        .ok_or(format!("no 'width' for output '{output_name}'"))?;
    let height = metadata.get("height").and_then(|v| u32::try_from(v.clone()).ok())
        .ok_or(format!("no 'height' for output '{output_name}'"))?;
    let stride = metadata.get("stride").and_then(|v| u32::try_from(v.clone()).ok())
        .unwrap_or(width * 4);

    let row_bytes = (width * 4) as usize;
    let needed = stride as usize * height as usize;
    if raw.len() < needed {
        return Err(format!("short read for '{output_name}': have {} need {}", raw.len(), needed).into());
    }

    // if there is no padding, there is no point in allocations and copying
    if stride as usize == row_bytes {
        raw.truncate(row_bytes * height as usize);
        return Ok((raw, width, height));
    }

    // unless there is, we need to do heavy copy
    let mut tight = vec![0u8; row_bytes * height as usize];
    for row in 0..height as usize {
        let src = &raw[row * stride as usize..][..row_bytes];
        let dst = &mut tight[row * row_bytes..][..row_bytes];
        dst.copy_from_slice(src);
    }
    Ok((tight, width, height))
}

#[async_trait]
impl CaptureMethod for KdeMethod {
    async fn capture_frame(&self, outputs: &[Output]) -> Result<CaptureResult, Box<dyn Error>> {
        let inner = async {
            let conn = self.conn.get_or_try_init(|| async { Connection::session().await.map_err(BoxErr::from) }).await?;
            let proxy = ScreenShot2Proxy::new(conn).await?;

            // one CaptureScreen call per monitor, all concurrent 
            let futs = outputs.iter().map(|o| {
                let proxy = proxy.clone();
                let name = o.info.name.clone().unwrap_or_default();
                
                let dimensions = o.info.modes.iter()
                    .find(|m| m.current)
                    .or_else(|| o.info.modes.first())
                    .map(|m| (m.dimensions.0 as usize, m.dimensions.1 as usize));

                async move { capture_one_screen(&proxy, &name, dimensions).await }
            });
            futures::future::try_join_all(futs).await
        };

        let results: Vec<_> = inner.await.map_err(|e: BoxErr| -> Box<dyn Error> { e })?;

        // try_join_all preserves input order, so results[i] <-> outputs[i] —
        // no reconciliation step needed, unlike the portal backend
        let frames = results.into_iter().zip(outputs).map(|((pixels, w, h), o)| MonitorFrame {
            pixels,
            pw_width: w,
            pw_height: h,
            pw_stride: w * 4,
            info: StreamInfo { node_id: 0, size: o.info.logical_size, position: o.info.logical_position },
        }).collect();

        Ok(CaptureResult { frames })
    }
}