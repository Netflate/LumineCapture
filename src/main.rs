mod app;
pub mod backend;
pub mod config;
pub mod editor;
pub mod fonts;
pub mod interaction;
pub mod keys;
pub mod logging;
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
use std::time::{Duration, Instant};

const HELP: &str = "\
Usage: {name} [MODE] [OPTIONS]
       {name} --ocr-daemon [serve|status|stop|calibrate]

Modes (without one, the editor opens):
  -f, --full           Capture the screen right away, no editor
  -w, --window         Capture the active window right away (KDE Plasma only)
  -r, --region         Drag a region, releasing the mouse takes the shot

Options:
  -m, --monitor        Only the monitor under the pointer
  -d, --delay <SECONDS>
                       Take the shot this long after the launch, e.g. -d 3 or -d 0.5
  -t, --to <LETTERS>   What to do with the shot: c copy, p pin, s save, o write the PNG
                       to stdout, in any order and mix, e.g. -t pc. Defaults to general.to
                       in the config. In the editor, Enter and a double click do this, and
                       any other finish adds it. A bare -t means nothing by default.
                       Only for -r ocr: r copies the text to the primary selection (middle
                       click). Screenshots ignore r, apps paste only text with the middle click
  -o, --output <DIR>   Save the shot into this directory instead of the configured
                       one, creating it if needed; implies s in --to
  -n, --nonotify       Disable notifications for this launch
  -s, --speed          Print how long each startup step took to stderr
      --config <PATH>  Use this config file instead of the default location
      --print-default-config
                       Print the default config.toml and exit
  -h, --help           Print this help and exit
  -V, --version        Print the version and exit
";

fn print_help() {
    println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    println!("{}", env!("CARGO_PKG_DESCRIPTION"));
    println!();
    print!("{}", HELP.replace("{name}", env!("CARGO_PKG_NAME")));
}

fn usage_error(e: impl std::fmt::Display) -> ! {
    eprintln!("{}: {e}", env!("CARGO_PKG_NAME"));
    eprintln!("See --help");
    std::process::exit(2);
}

fn parse_delay(s: &str) -> Result<Duration, &'static str> {
    s.parse::<f64>()
        .ok()
        .and_then(|secs| Duration::try_from_secs_f64(secs).ok())
        .ok_or("expected seconds, e.g. 3 or 0.5")
}

fn parse_launch(
    pargs: &mut pico_args::Arguments,
    started: Instant,
) -> Result<app::Launch, Box<dyn std::error::Error>> {
    let modes = [
        (pargs.contains(["-f", "--full"]), app::Mode::Full),
        (pargs.contains(["-w", "--window"]), app::Mode::Window),
        (pargs.contains(["-r", "--region"]), app::Mode::Region),
    ];
    let mut picked = modes.iter().filter(|(on, _)| *on).map(|(_, mode)| *mode);
    let mode = picked.next().unwrap_or_default();
    if picked.next().is_some() {
        return Err("pick one mode: --full, --window or --region".into());
    }
    let one_monitor = pargs.contains(["-m", "--monitor"]);
    let output =
        pargs.opt_value_from_os_str(["-o", "--output"], |s| Ok::<_, String>(PathBuf::from(s)))?;
    let speed = pargs.contains(["-s", "--speed"]);
    let shot_at = match pargs.opt_value_from_fn(["-d", "--delay"], parse_delay)? {
        Some(delay) => Some(started.checked_add(delay).ok_or("the delay is too long")?),
        None => None,
    };
    let to = match pargs.opt_value_from_fn(["-t", "--to"], types::Outputs::parse) {
        Err(pico_args::Error::OptionWithoutAValue(_)) => {
            pargs.contains(["-t", "--to"]);
            Some(types::Outputs::default())
        }
        to => to?,
    };
    Ok(app::Launch {
        mode,
        one_monitor,
        to,
        output,
        speed,
        shot_at,
    })
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let started = Instant::now();
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
    let config_path: Option<PathBuf> =
        pargs.opt_value_from_os_str("--config", |s| Ok::<_, String>(PathBuf::from(s)))?;
    let no_notify = pargs.contains(["-n", backend::notify::NO_NOTIFY_ARG]);
    let launch = match parse_launch(&mut pargs, started) {
        Ok(launch) => launch,
        Err(e) => usage_error(e),
    };
    config::init(config_path, |config| {
        if no_notify {
            config.notifications.enabled = false;
        }
    });

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
        Some(other) => usage_error(format!("unknown argument {other:?}")),
        None => tokio::runtime::Runtime::new()?.block_on(async {
            logging::init(logging::Process::Overlay);
            let result = match wayland_client::Connection::connect_to_env() {
                Ok(conn) => app::run(conn, launch).await,
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
