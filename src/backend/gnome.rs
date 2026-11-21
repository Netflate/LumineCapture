use zbus::{Connection, proxy};

/// x, y, width, height
pub type Rect = (i32, i32, i32, i32);

#[derive(Debug, Clone, Copy)]
pub struct FocusWindow {
    pub id: u64,
    pub frame: Rect,
    pub buffer: Rect,
}

#[proxy(
    interface = "io.github.netflate.LumineCapture.Shell1",
    default_service = "io.github.netflate.LumineCapture.Shell",
    default_path = "/io/github/netflate/LumineCapture/Shell"
)]
trait Shell {
    fn get_focus_window(&self) -> zbus::Result<(u64, Rect, Rect)>;
    fn keep_above(&self) -> zbus::Result<()>;
    fn passthrough(&self) -> zbus::Result<()>;
    fn claim_recording(&self) -> zbus::Result<()>;
}

async fn proxy(conn: &Connection) -> zbus::Result<ShellProxy<'_>> {
    ShellProxy::builder(conn)
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
}

pub async fn focus_window(conn: &Connection) -> zbus::Result<FocusWindow> {
    let (id, frame, buffer) = proxy(conn).await?.get_focus_window().await?;
    Ok(FocusWindow { id, frame, buffer })
}

pub async fn claim_recording(conn: &Connection) {
    if let Err(error) = async { proxy(conn).await?.claim_recording().await }.await {
        log::debug!("GNOME recording claim failed: {error}");
    }
}

pub async fn passthrough() -> bool {
    let result = async {
        proxy(&Connection::session().await?)
            .await?
            .passthrough()
            .await
    }
    .await;
    match result {
        Ok(()) => {
            log::info!("GNOME overlay passthrough enabled");
            true
        }
        Err(error) => {
            log::debug!("GNOME overlay passthrough unavailable: {error}");
            false
        }
    }
}

pub fn keep_above() {
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    match result {
        Ok(runtime) => runtime.block_on(async {
            let result = async {
                proxy(&Connection::session().await?)
                    .await?
                    .keep_above()
                    .await
            }
            .await;
            if let Err(error) = result {
                log::debug!("GNOME keep-above request failed: {error}");
            }
        }),
        Err(error) => log::debug!("GNOME keep-above runtime failed: {error}"),
    }
}
