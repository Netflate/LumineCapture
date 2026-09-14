pub struct PortalMethod;

use log::warn;
use crate::backend::wayland::capture::stream;
use std::fs;
use std::os::fd::AsFd;
use std::path::PathBuf;

use crate::backend::CaptureMethod;
use crate::types::{CaptureResult, MonitorFrame, Output, StreamInfo};
use ashpd::desktop::{
    PersistMode, Session,
    screencast::{
        CursorMode, Screencast, SelectSourcesOptions, SourceType as AshpdSourceType, Streams,
    },
};
use async_trait::async_trait;

fn token_path() -> Option<PathBuf> {
    Some(dirs::state_dir()?.join("LumineCapture").join("token"))
}

// older builds kept the token in the config dir
fn legacy_token_path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("LumineCapture").join("token"))
}

fn read_token() -> Option<String> {
    [token_path(), legacy_token_path()]
        .into_iter()
        .flatten()
        .find_map(|path| fs::read_to_string(path).ok())
}

fn write_token(token: &str) {
    let Some(path) = token_path() else { return };
    let written = path
        .parent()
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(&path, token));
    match written {
        Ok(()) => {
            if let Some(legacy) = legacy_token_path() {
                let _ = fs::remove_file(legacy);
            }
        }
        Err(e) => warn!("Can't save portal token to {}: {}", path.display(), e),
    }
}

fn forget_token() {
    for path in [token_path(), legacy_token_path()].into_iter().flatten() {
        let _ = fs::remove_file(path);
    }
}

async fn start_session(
    proxy: &Screencast,
    token: Option<&str>,
) -> Result<(Session<Screencast>, Streams), ashpd::Error> {
    let session = proxy.create_session(Default::default()).await?;

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

    Ok((session, response))
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

    Ok(ordered
        .into_iter()
        .map(|s| s.expect("reconcile: slot left empty despite length check"))
        .collect())
}

#[async_trait]
impl CaptureMethod for PortalMethod {
    async fn capture_frame(
        &self,
        outputs: &[Output],
    ) -> Result<CaptureResult, Box<dyn std::error::Error>> {
        let proxy = Screencast::new().await?;

        let token = read_token();
        let (session, response) = match start_session(&proxy, token.as_deref()).await {
            Err(ashpd::Error::Portal(ashpd::PortalError::InvalidArgument(msg)))
                if token.is_some() =>
            {
                warn!("Portal rejected the saved restore token ({msg}), asking again");
                forget_token();
                start_session(&proxy, None).await?
            }
            res => res?,
        };

        let new_token = response.restore_token().map(str::to_owned);

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

        if let Some(new_token) = new_token {
            write_token(&new_token);
        }

        if let Err(e) = session.close().await {
            warn!("Can't close portal session: {e}");
        }

        Ok(CaptureResult { frames })
    }
}
