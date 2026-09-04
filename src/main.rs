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
            run_clipboard_daemon();
            Ok(())
        }
        Some("--pin") => run_pin(args),
        _ => tokio::runtime::Runtime::new()?.block_on(async {
            let wayland_ = wayland_client::Connection::connect_to_env().ok();
            app::make_screenshot(wayland_).await
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

fn run_clipboard_daemon() {
    use std::io::Read;
    use wl_clipboard_rs::copy::{MimeSource, MimeType, Options, Source};

    let mut buf = Vec::new();
    std::io::stdin().read_to_end(&mut buf).expect("read stdin");

    let mut opts = Options::new();
    opts.foreground(true);

    let result = opts.copy_multi(vec![
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
    ]);

    if let Err(e) = result {
        eprintln!("clipboard daemon error: {e}");
        std::process::exit(1);
    }
}
