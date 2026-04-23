// Current implementation relies on XDG Desktop Portals
// While it should work, theoretically, on x11, it is optimized and tested for Wayland (KDE/Sway)
// and if x11 full support will be ever added, its really necessary to use direct screenshot tools and not these slow portal+pipewire

// TODO: Implement token validation logic. If the restore_token is expired or invalid, clear the cache and reprompt the user for stream selection

pub struct PortalMethod;

use crate::backend::wayland::stream;

use ashpd::enumflags2::_internal::RawBitFlags;
use crate::backend::CaptureMethod;
use crate::types::CaptureResult;
use crate::types::StreamInfo;
use ashpd::desktop::{
    PersistMode,
    screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType as AshpdSourceType},
};
use crate::types::SourceType;
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
                    .set_multiple(false)
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
                source_type: match s.source_type().map(|st| st.bits()) {
                    Some(1) => SourceType::Monitor,
                    Some(2) => SourceType::Window,
                    Some(4) => SourceType::Virtual,
                    _ => SourceType::Monitor,
                },
            })
            .collect();
        response.streams().iter().for_each(|stream| {
            println!("node id : {}", stream.pipe_wire_node_id());
            println!("size : {:?}", stream.size());
            println!("position : {:?}", stream.position());
        });

        println!("token : {:?} ", token);
        if token.is_none() {
            if let Some(rt) = response.restore_token() {
                std::fs::write(path, rt)?;
            }
        }
        let t0 = std::time::Instant::now();
        let fd = proxy
            .open_pipe_wire_remote(&session, Default::default())
            .await?;
        println!("open_pipe_wire_remote: {}ms", t0.elapsed().as_millis());

        let node_id = response.streams()[0].pipe_wire_node_id();


        let frame = stream::capture_frame(node_id, fd)
            .map_err(|e| ashpd::Error::Zbus(ashpd::zbus::Error::Failure(e.to_string())))?;

        session.close().await?; 

        Ok(CaptureResult {
            frame,
            streams: streams_data,
        })
    }
}

