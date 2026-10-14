// Sends notifications through org.freedesktop.Notifications via D-Bus
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;

use async_trait::async_trait;
use log::warn;
use zbus::zvariant::Value;
use zbus::{Connection, proxy};

use crate::backend::Notifier;
use crate::backend::notify::{Notification, temp};

const APP_NAME: &str = "LumineCapture";

#[proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    async fn get_capabilities(&self) -> zbus::Result<Vec<String>>;

    async fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[&str],
        hints: HashMap<&str, Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;
}

pub struct FreedesktopNotifier;

/// Pnv previw in the notification
enum Preview {
    /// with png in the icon slot, it gets squiched on kde and looks terrible
    /// so we have to use `x-kde-urls` that gives a full preview with actions
    /// (it works with files)
    KdeUrls,
    Icon,
}

impl Preview {
    fn pick(caps: &[String]) -> Self {
        if caps.iter().any(|c| c == "x-kde-urls") {
            Preview::KdeUrls
        } else {
            Preview::Icon
        }
    }
}

#[async_trait]
impl Notifier for FreedesktopNotifier {
    async fn notify(&self, n: &Notification<'_>) -> Result<(), Box<dyn Error + Send + Sync>> {
        let conn = Connection::session().await?;
        let dbus = zbus::fdo::DBusProxy::new(&conn).await?;
        if !dbus
            .name_has_owner("org.freedesktop.Notifications".try_into()?)
            .await?
        {
            return Err("no notification daemon running".into());
        }
        let proxy = NotificationsProxy::new(&conn).await?;

        let caps = proxy.get_capabilities().await?;
        // Escape markup special characters (like `&`) if the daemon supports `body-markup` to prevent parsing errors.
        let markup = caps.iter().any(|c| c == "body-markup");
        let body = if markup {
            escape_markup(&n.body)
        } else {
            n.body.clone()
        };

        let mut hints: HashMap<&str, Value> = HashMap::new();
        hints.insert("urgency", Value::U8(1));
        match Preview::pick(&caps) {
            Preview::KdeUrls => {
                if let Some(url) = shot_url(n) {
                    hints.insert("x-kde-urls", Value::from(vec![url]));
                }
            }
            Preview::Icon => {
                if let Some(file) = &n.file {
                    hints.insert("image-path", Value::from(file_url(file)));
                }
            }
        }

        proxy
            .notify(
                APP_NAME,
                0,
                "lumine-capture",
                n.summary,
                &body,
                &[],
                hints,
                -1,
            )
            .await?;
        Ok(())
    }
}

fn shot_url(n: &Notification<'_>) -> Option<String> {
    let path = match (&n.file, &n.png) {
        (Some(file), _) => file.clone(),
        (None, Some(png)) => temp::write(png)
            .inspect_err(|e| warn!("can't write the preview: {e}"))
            .ok()?,
        (None, None) => return None,
    };
    Some(file_url(&path))
}

fn escape_markup(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn file_url(path: &Path) -> String {
    use std::fmt::Write;
    use std::os::unix::ffi::OsStrExt;

    let mut url = String::from("file://");
    for &b in path.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            url.push(b as char);
        } else {
            let _ = write!(url, "%{b:02X}");
        }
    }
    url
}
