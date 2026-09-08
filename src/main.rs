mod app;
pub mod backend;
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    // supplementary processes start before initializing Tokio to avoid inheriting the runtime or its worker threads.
    match args.next().as_deref() {
        // wayland clipboard requires the source process to stay alive to serve data.
        // we spawn a short-lived daemon so clipboard managers can fetch the capture
        Some("--clipboard-daemon") => {
            run_clipboard_daemon(args.next().as_deref() == Some("text"));
            Ok(())
        }
        Some("--pin") => {
            let result = run_pin(args);
            if let Err(e) = &result {
                backend::notify::send_blocking(backend::notify::Notice::PinFailed(e.to_string()));
            }
            result
        }
        Some("--ocr-daemon") => ocr::daemon::cli(args),
        _ => tokio::runtime::Runtime::new()?.block_on(async {
            let result = match wayland_client::Connection::connect_to_env() {
                Ok(conn) => app::make_screenshot(conn).await,
                Err(e) => Err(format!("can't connect to Wayland: {e}").into()),
            };
            if let Err(e) = &result {
                backend::notify::send(backend::notify::Notice::Failed(e.to_string())).await;
            }
            result
        }),
    }
}

/// Handles `--pin [--at X,Y] [FILE]`. Reads the image from stdin if no file path is provided.
fn run_pin(mut args: impl Iterator<Item = String>) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Read;

    let mut at = None;
    let mut file = None;
    while let Some(arg) = args.next() {
        if arg == "--at" {
            at = args.next().and_then(|v| {
                let (x, y) = v.split_once(',')?;
                Some((x.parse().ok()?, y.parse().ok()?))
            });
        } else {
            file = Some(arg);
        }
    }

    let image = match file {
        Some(path) => std::fs::read(path)?,
        None => {
            let mut buf = Vec::new();
            std::io::stdin().read_to_end(&mut buf)?;
            buf
        }
    };
    backend::wayland::pin::run(&image, at)
}

fn run_clipboard_daemon(text: bool) {
    use std::io::{Read, Write};
    use wl_clipboard_rs::copy::{MimeSource, MimeType, Options, Source};

    let mut buf = Vec::new();
    if let Err(e) = std::io::stdin().read_to_end(&mut buf) {
        println!("can't read clipboard data: {e}");
        std::process::exit(1);
    }

    let mut opts = Options::new();
    opts.foreground(true);

    let sources = if text {
        vec![MimeSource {
            source: Source::Bytes(buf.into()),
            mime_type: MimeType::Text,
        }]
    } else {
        vec![
            MimeSource {
                source: Source::Bytes(buf.clone().into()),
                mime_type: MimeType::Specific("image/png".to_string()),
            },
            MimeSource {
                source: Source::Bytes(buf.clone().into()),
                mime_type: MimeType::Specific("application/x-qt-image".to_string()),
            },
            MimeSource {
                source: Source::Bytes(buf.into()),
                mime_type: MimeType::Specific("x-kde-force-image-copy".to_string()),
            },
        ]
    };

    // the parent reads this line: empty once the selection is ours
    let mut stdout = std::io::stdout();
    match opts.prepare_copy_multi(sources) {
        Ok(copy) => {
            let _ = writeln!(stdout);
            let _ = stdout.flush();
            if copy.serve().is_err() {
                std::process::exit(1);
            }
        }
        Err(e) => {
            let _ = writeln!(stdout, "{}", e.to_string().replace('\n', " "));
            std::process::exit(1);
        }
    }
}
