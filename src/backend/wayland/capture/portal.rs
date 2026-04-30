// Current implementation relies on XDG Desktop Portals
// While it should work, theoretically, on x11, it is optimized and tested for Wayland (KDE/Sway)
// and if x11 full support will be ever added, its really necessary to use direct screenshot tools and not these slow portal+pipewire

// TODO: Implement token validation logic. If the restore_token is expired or invalid, clear the cache and reprompt the user for stream selection

pub struct PortalMethod;

use crate::backend::wayland::capture::stream;
use std::os::fd::AsFd;

use crate::backend::CaptureMethod;
use crate::types::{CaptureResult, StreamInfo, MonitorFrame};
use ashpd::desktop::{
    PersistMode,
    screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType as AshpdSourceType},
};
use async_trait::async_trait;


#[async_trait]
impl CaptureMethod for PortalMethod {
    async fn capture_frame(&self) -> Result<CaptureResult, Box<dyn std::error::Error>> {
        let proxy = Screencast::new().await?;
        let session = proxy.create_session(Default::default()).await?;

        std::fs::create_dir_all("/home/Netflate/.config/LumineCapture/")
            .unwrap_or_else(|e| eprintln!("can't create directory: {}", e));

        let path = "/home/Netflate/.config/LumineCapture/token"; // TOFIX: hard coded  

        let token_string = std::fs::read_to_string(path).ok();
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

        let streams_data: Vec<StreamInfo> = response
            .streams()
            .iter()
            .map(|s| StreamInfo {
                node_id: s.pipe_wire_node_id(),
                size: s.size(),
                position: s.position(),
            })
            .collect();

        if token.is_none() {
            if let Some(rt) = response.restore_token() {
                std::fs::write(path, rt)?;
            }
        }

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