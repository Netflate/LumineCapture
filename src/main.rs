mod app;
pub mod backend;
pub mod logging;
pub mod editor;
pub mod interaction;
pub mod ocr;
pub mod profiler;
pub mod renderer;
pub mod theme;
pub mod tools;
pub mod types;
pub mod ui;
pub mod utils;

use backend::wayland::{clipboard, pin};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    // supplementary processes start before initializing Tokio to avoid inheriting the runtime or its worker threads
    match args.next().as_deref() {
        // wayland clipboard requires the source process to stay alive to serve data.
        // we spawn a short-lived daemon so clipboard managers can fetch the capture
        Some(clipboard::DAEMON_ARG) => {
            logging::init(logging::Process::Clipboard);
            clipboard::daemon::cli(args);
            Ok(())
        }
        Some(pin::PIN_ARG) => {
            logging::init(logging::Process::Pin);
            let result = pin::cli(args);
            if let Err(e) = &result {
                log::error!("pin failed: {e}");
                backend::notify::send_blocking(backend::notify::Notice::PinFailed(e.to_string()));
            }
            result
        }
        Some(ocr::daemon::DAEMON_ARG) => {
            logging::init(logging::Process::OcrDaemon);
            ocr::daemon::cli(args)
        }
        _ => tokio::runtime::Runtime::new()?.block_on(async {
            logging::init(logging::Process::Overlay);
            let result = match wayland_client::Connection::connect_to_env() {
                Ok(conn) => app::make_screenshot(conn).await,
                Err(e) => Err(format!("can't connect to Wayland: {e}").into()),
            };
            if let Err(e) = &result {
                log::error!("screenshot failed: {e}");
                backend::notify::send(backend::notify::Notice::Failed(e.to_string())).await;
            }
            result
        }),
    }
}
