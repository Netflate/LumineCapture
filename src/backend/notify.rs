pub mod freedesktop;

use log::warn;
use std::error::Error;
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;

use crate::backend::{Notifier, initialize_notifier};

/// duration to wait for the desktop notification daemon via DBus.
const TIMEOUT: Duration = Duration::from_millis(500);

pub struct Notification {
    pub summary: &'static str,
    pub body: String,
    pub error: bool,
    pub file: Option<PathBuf>,
}

pub enum Notice {
    Saved(PathBuf),
    /// Contains `Some(PathBuf)` 
    /// if the copied image was also auto-saved to disk.
    Copied(Option<PathBuf>),
    SaveFailed(String),
    CopyFailed(String),
    PinFailed(String),
    Failed(String),
}

impl Notice {
    pub fn spec(self) -> Notification {
        let (summary, body, error, file) = match self {
            Notice::Saved(path) => ("Screenshot saved", path.display().to_string(), false, Some(path)),
            Notice::Copied(Some(path)) => (
                "Screenshot copied",
                format!("Also saved to {}", path.display()),
                false,
                Some(path),
            ),
            Notice::Copied(None) => ("Screenshot copied", String::new(), false, None),
            Notice::SaveFailed(e) => ("Couldn't save the screenshot", e, true, None),
            Notice::CopyFailed(e) => ("Couldn't copy the screenshot", e, true, None),
            Notice::PinFailed(e) => ("Couldn't pin the screenshot", e, true, None),
            Notice::Failed(e) => ("Screenshot failed", e, true, None),
        };
        Notification { summary, body, error, file }
    }
}

pub struct StderrNotifier;

#[async_trait]
impl Notifier for StderrNotifier {
    async fn notify(&self, n: &Notification) -> Result<(), Box<dyn Error + Send + Sync>> {
        eprintln!("{}: {}", n.summary, n.body);
        Ok(())
    }
}

pub async fn send(notice: Notice) {
    if !crate::config::get().notifications.enabled {
        return;
    }
    let n = notice.spec();
    let sent = tokio::time::timeout(TIMEOUT, initialize_notifier().notify(&n)).await;
    let reason = match sent {
        Ok(Ok(())) => return,
        Ok(Err(e)) => e.to_string(),
        Err(_) => "timed out".to_string(),
    };
    warn!("notification failed: {reason}");
    let _ = StderrNotifier.notify(&n).await;
}

/// Dispatches notifications synchronously for non Tokio worker processes 
/// Panics if invoked inside an active Tokio context. Avoids `zbus::block_on` to prevent
/// spawning per-core threads.
pub fn send_blocking(notice: Notice) {
    if !crate::config::get().notifications.enabled {
        return;
    }
    match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt.block_on(send(notice)),
        Err(_) => {
            let n = notice.spec();
            eprintln!("{}: {}", n.summary, n.body);
        }
    }
}
