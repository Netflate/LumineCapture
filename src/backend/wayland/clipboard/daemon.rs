// The clipboard helper process. Wayland hands the selection to a live process,
// so the overlay re-execs itself here and this one stays alive to serve the
// data until something else takes the clipboard over.

use std::io::{Read, Write};

use wl_clipboard_rs::copy::{MimeSource, MimeType, Options, Source};

use super::TEXT_ARG;

pub fn cli(mut args: impl Iterator<Item = String>) {
    let text = args.next().as_deref() == Some(TEXT_ARG);

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
