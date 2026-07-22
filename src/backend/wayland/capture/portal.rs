pub struct PortalMethod;

use crate::backend::wayland::capture::stream;
use std::fs;
use std::os::fd::AsFd;
use std::path::PathBuf;

use crate::backend::CaptureMethod;
use crate::types::{CaptureResult, MonitorFrame, Output, StreamInfo};
use ashpd::desktop::{
    PersistMode,
    screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType as AshpdSourceType},
};
use async_trait::async_trait;

fn get_token_path() -> PathBuf {
    let mut path = dirs::config_dir().expect("Could not find config directory");
    path.push("LumineCapture");

    if let Err(e) = fs::create_dir_all(&path) {
        eprintln!("Can't create directory {}: {}", path.display(), e);
    }

    path.push("token");
    path
}

// reconcile portal-reported monitor streams against the wayland output list.
// the portal is a separate, independent source of truth from wayland, so unlike
// present() in the overlay (which trusts its own outputs), here a mismatch is a
// real, expected situation (e.g. the user deselected a monitor in the portal's
// picker dialog) and must be surfaced as an explicit error rather than silently
// falling back to a guess
fn reconcile_streams(
    streams: Vec<StreamInfo>,
    outputs: &[Output],
) -> Result<Vec<StreamInfo>, Box<dyn std::error::Error>> {
    if streams.len() != outputs.len() {
        return Err(format!(
            "portal returned {} monitor stream(s), but wayland reports {} output(s) — \
             did you deselect a monitor in the portal picker?",
            streams.len(),
            outputs.len()
        )
        .into());
    }

    let mut used = vec![false; outputs.len()];
    let mut ordered: Vec<Option<StreamInfo>> = (0..outputs.len()).map(|_| None).collect();

    for stream in streams {
        let pos = stream.position.unwrap_or((0, 0));

        let idx = outputs
            .iter()
            .enumerate()
            .filter(|(i, _)| !used[*i])
            .find(|(_, o)| o.info.logical_position == Some(pos))
            .map(|(i, _)| i)
            .ok_or_else(|| {
                format!(
                    "portal stream at {:?} doesn't match any known wayland output (known positions: {:?})",
                    pos,
                    outputs.iter().map(|o| o.info.logical_position).collect::<Vec<_>>()
                )
            })?;

        used[idx] = true;
        ordered[idx] = Some(stream);
    }

    Ok(ordered.into_iter().map(|s| s.expect("reconcile: slot left empty despite length check")).collect())
}

#[async_trait]
impl CaptureMethod for PortalMethod {
    async fn capture_frame(&self, outputs: &[Output]) -> Result<CaptureResult, Box<dyn std::error::Error>> {
        let proxy = Screencast::new().await?;
        let session = proxy.create_session(Default::default()).await?;

        let token_path = get_token_path();

        let token_string = fs::read_to_string(&token_path).ok();
        let token = token_string.as_deref();

        proxy
            .select_sources(
                &session,
                SelectSourcesOptions::default()
                    .set_cursor_mode(CursorMode::Metadata)
                    .set_sources(Some(AshpdSourceType::Monitor.into()))
                    .set_multiple(true)
                    .set_restore_token(token)
                    .set_persist_mode(PersistMode::ExplicitlyRevoked),
            )
            .await?;

        let response = proxy
            .start(&session, None, Default::default())
            .await?
            .response()?;

        if let Some(new_token) = response.restore_token() {
            fs::write(&token_path, new_token)?;
        }

        let streams_data: Vec<StreamInfo> = response
            .streams()
            .iter()
            .map(|s| StreamInfo {
                node_id: s.pipe_wire_node_id(),
                size: s.size(),
                position: s.position(),
            })
            .collect();

        // reconcile against the wayland outputs BEFORE opening the PipeWire remote,
        // so a mismatch fails fast instead of wasting time on the fd handoff
        let streams_data = reconcile_streams(streams_data, outputs)?;

        let fd = proxy
            .open_pipe_wire_remote(&session, Default::default())
            .await?;

        let mut frames = Vec::new();

        for stream_info in streams_data {
            let frame = stream::capture_frame(stream_info.node_id, fd.as_fd())
                .map_err(|e| ashpd::Error::Zbus(ashpd::zbus::Error::Failure(e.to_string())))?;

            frames.push(MonitorFrame {
                pixels: frame.pixels,
                pw_width: frame.width,
                pw_height: frame.height,
                pw_stride: frame.stride,
                info: stream_info,
            });
        }

        session.close().await?;

        Ok(CaptureResult { frames })
    }
}