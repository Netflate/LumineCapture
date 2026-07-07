mod app;
pub mod backend;
pub mod editor;
pub mod renderer;
pub mod tools;
pub mod types;
pub mod utils;

#[tokio::main]

async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    // wayland clipboard requires the source process to stay alive to serve data.
    // we spawn a short-lived daemon so clipboard managers can fetch the capture
    if args.get(1).map(String::as_str) == Some("--clipboard-daemon") {
        run_clipboard_daemon();
        return Ok(());
    }

    let wayland_conn = wayland_client::Connection::connect_to_env().ok();
    app::make_screenshot(wayland_conn).await?;
    Ok(())
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