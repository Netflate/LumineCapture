// GNOME has no data-control protocol, so the clipboard goes through mutter's
// RemoteDesktop API. No Start on the session: it would show the screen-sharing
// indicator. The session dies with our D-Bus connection.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use log::warn;
use nix::fcntl::{FcntlArg, OFlag, fcntl};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};
use zbus::{Connection, proxy};

type BoxErr = Box<dyn std::error::Error + Send + Sync>;

pub const TEXT_MIMES: [&str; 5] = [
    "text/plain;charset=utf-8",
    "text/plain",
    "UTF8_STRING",
    "STRING",
    "TEXT",
];
const READ_MIMES: [&str; 3] = ["text/plain;charset=utf-8", "text/plain", "UTF8_STRING"];
const READ_TIMEOUT: Duration = Duration::from_secs(2);

#[proxy(
    interface = "org.gnome.Mutter.RemoteDesktop",
    default_service = "org.gnome.Mutter.RemoteDesktop",
    default_path = "/org/gnome/Mutter/RemoteDesktop"
)]
trait RemoteDesktop {
    fn create_session(&self) -> zbus::Result<OwnedObjectPath>;
}

#[proxy(
    interface = "org.gnome.Mutter.RemoteDesktop.Session",
    default_service = "org.gnome.Mutter.RemoteDesktop"
)]
trait Session {
    fn enable_clipboard(&self, options: HashMap<&str, Value<'_>>) -> zbus::Result<()>;

    fn set_selection(&self, options: HashMap<&str, Value<'_>>) -> zbus::Result<()>;

    fn selection_write(&self, serial: u32) -> zbus::Result<zbus::zvariant::OwnedFd>;

    fn selection_write_done(&self, serial: u32, success: bool) -> zbus::Result<()>;

    fn selection_read(&self, mime_type: &str) -> zbus::Result<zbus::zvariant::OwnedFd>;

    #[zbus(signal)]
    fn selection_transfer(&self, mime_type: String, serial: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    fn selection_owner_changed(&self, options: HashMap<String, OwnedValue>) -> zbus::Result<()>;
}

async fn new_session(conn: &Connection) -> Result<SessionProxy<'static>, BoxErr> {
    let path = RemoteDesktopProxy::new(conn)
        .await?
        .create_session()
        .await?;
    Ok(SessionProxy::builder(conn).path(path)?.build().await?)
}

// mutter may hand out non-blocking fds
fn blocking_file(fd: OwnedFd) -> std::io::Result<File> {
    let flags = OFlag::from_bits_truncate(fcntl(&fd, FcntlArg::F_GETFL)?);
    fcntl(&fd, FcntlArg::F_SETFL(flags & !OFlag::O_NONBLOCK))?;
    Ok(File::from(fd))
}

pub fn serve(
    offers: Vec<(String, Arc<[u8]>)>,
    on_ready: impl FnOnce(),
) -> Result<(), Box<dyn std::error::Error>> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result: Result<(), BoxErr> = rt.block_on(serve_async(offers, on_ready));
    result.map_err(|e| e as Box<dyn std::error::Error>)
}

async fn serve_async(
    offers: Vec<(String, Arc<[u8]>)>,
    on_ready: impl FnOnce(),
) -> Result<(), BoxErr> {
    let conn = Connection::session().await?;
    let session = new_session(&conn).await?;

    // subscribe before SetSelection, or the first transfer request can be missed
    let mut transfers = session.receive_selection_transfer().await?;
    let mut owners = session.receive_selection_owner_changed().await?;

    session.enable_clipboard(HashMap::new()).await?;
    let mimes: Vec<&str> = offers.iter().map(|(m, _)| m.as_str()).collect();
    session
        .set_selection(HashMap::from([("mime-types", Value::from(mimes))]))
        .await?;
    on_ready();

    let mut owned = false;
    loop {
        tokio::select! {
            transfer = transfers.next() => {
                let Some(transfer) = transfer else {
                    return Err("mutter closed the clipboard session".into());
                };
                let args = transfer.args()?;
                let (serial, mime) = (args.serial, args.mime_type.to_owned());
                let Some((_, data)) = offers.iter().find(|(m, _)| *m == mime) else {
                    warn!("no data for {mime}");
                    if let Err(e) = session.selection_write_done(serial, false).await {
                        warn!("can't refuse {mime}: {e}");
                    }
                    continue;
                };
                let fd = match session.selection_write(serial).await {
                    Ok(fd) => fd,
                    Err(e) => {
                        warn!("can't get a write fd for {mime}: {e}");
                        continue;
                    }
                };
                let (proxy, data) = (session.clone(), data.clone());
                tokio::spawn(async move {
                    let fd: OwnedFd = fd.into();
                    let written = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
                        let mut file = blocking_file(fd)?;
                        file.write_all(&data)
                    })
                    .await;
                    let ok = match written {
                        Ok(Ok(())) => true,
                        Ok(Err(e)) => {
                            warn!("writing {mime} failed: {e}");
                            false
                        }
                        Err(e) => {
                            warn!("writing {mime} failed: {e}");
                            false
                        }
                    };
                    if let Err(e) = proxy.selection_write_done(serial, ok).await {
                        warn!("can't finish the {mime} transfer: {e}");
                    }
                });
            }
            change = owners.next() => {
                let Some(change) = change else {
                    return Err("mutter closed the clipboard session".into());
                };
                let args = change.args()?;
                let is_owner = args
                    .options
                    .get("session-is-owner")
                    .and_then(|v| bool::try_from(v).ok())
                    .unwrap_or(false);
                if is_owner {
                    owned = true;
                } else if owned {
                    return Ok(());
                }
            }
        }
    }
}

// own thread: the caller may already be inside a tokio runtime
pub fn read_text() -> Option<String> {
    let handle = std::thread::spawn(|| -> Result<Option<String>, BoxErr> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        rt.block_on(async {
            tokio::time::timeout(READ_TIMEOUT, read_async())
                .await
                .map_err(|_| "reading the clipboard timed out")?
        })
    });
    match handle.join() {
        Ok(Ok(text)) => text,
        Ok(Err(e)) => {
            warn!("can't read the clipboard through mutter: {e}");
            None
        }
        Err(_) => {
            warn!("the clipboard reader thread panicked");
            None
        }
    }
}

async fn read_async() -> Result<Option<String>, BoxErr> {
    let conn = Connection::session().await?;
    let session = new_session(&conn).await?;
    session.enable_clipboard(HashMap::new()).await?;

    for mime in READ_MIMES {
        let Ok(fd) = session.selection_read(mime).await else {
            continue;
        };
        let fd: OwnedFd = fd.into();
        let data = tokio::task::spawn_blocking(move || -> std::io::Result<Vec<u8>> {
            let mut file = blocking_file(fd)?;
            let mut data = Vec::new();
            file.read_to_end(&mut data)?;
            Ok(data)
        })
        .await??;
        return Ok(Some(String::from_utf8_lossy(&data).into_owned()));
    }
    Ok(None)
}
