mod app;
pub mod backend;
pub mod config;
pub mod logging;
pub mod editor;
pub mod interaction;
pub mod keys;
pub mod ocr;
pub mod profiler;
pub mod renderer;
pub mod theme;
pub mod tools;
pub mod types;
pub mod ui;
pub mod utils;

use backend::wayland::{clipboard, pin};
use std::path::PathBuf;

fn print_help() {
    println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    println!("{}", env!("CARGO_PKG_DESCRIPTION"));
    println!();
    println!("Usage: LumineCapture [OPTIONS]");
    println!("       LumineCapture --pin [--at X,Y] [--scale S] [FILE]");
    println!("       LumineCapture --clipboard-daemon [text]");
    println!("       LumineCapture --ocr-daemon [serve|status|stop|calibrate]");
    println!();
    println!("Options:");
    println!("  -h, --help              Print this help and exit");
    println!("  -V, --version           Print the version and exit");
    println!("      --one               Capture only the active monitor");
    println!("      --config <PATH>     Use this config file instead of the default location");
    println!("      --print-default-config");
    println!("                          Print the default config.toml and exit");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut pargs = pico_args::Arguments::from_env();

    if pargs.contains(["-h", "--help"]) {
        print_help();
        return Ok(());
    }
    if pargs.contains(["-V", "--version"]) {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if pargs.contains("--print-default-config") {
        print!("{}", config::TEMPLATE);
        return Ok(());
    }
    let one = pargs.contains("--one");
    let config_path: Option<PathBuf> = pargs
        .opt_value_from_os_str("--config", |s| Ok::<_, String>(PathBuf::from(s)))?;
    config::init(config_path);

    let mut args = pargs
        .finish()
        .into_iter()
        .map(|s| s.into_string().unwrap_or_default());
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
                Ok(conn) => app::make_screenshot(conn, one).await,
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
