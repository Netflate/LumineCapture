// The clipboard helper process. Wayland hands the selection to a live process,
// so the overlay re-execs itself here and this one stays alive to serve the
// data until something else takes the clipboard over.

use std::cell::Cell;
use std::io::{Read, Write};
use std::sync::Arc;
use std::time::Instant;

use log::{error, info};
use wl_clipboard_rs::copy::{ClipboardType, MimeSource, MimeType, Options, Source};

use super::mutter;
use super::{EXTRA_ARG, PRIMARY_ARG, TEXT_ARG};

type Split = (Option<(String, Vec<u8>)>, Vec<u8>);

fn split_extra(args: &[String], mut buf: Vec<u8>) -> Result<Split, String> {
    let Some(at) = args.iter().position(|a| a == EXTRA_ARG) else {
        return Ok((None, buf));
    };
    let (Some(mime), Some(len)) = (args.get(at + 1), args.get(at + 2)) else {
        return Err("the extra image needs a mime and a length".into());
    };
    let len: usize = len.parse().map_err(|_| format!("bad extra length {len}"))?;
    if len > buf.len() {
        return Err(format!(
            "extra length {len} is past the {} bytes read",
            buf.len()
        ));
    }
    let png = buf.split_off(len);
    Ok((Some((mime.clone(), buf)), png))
}

pub fn cli(args: impl Iterator<Item = String>) {
    let args: Vec<String> = args.collect();
    let text = args.iter().any(|a| a == TEXT_ARG);
    let primary = args.iter().any(|a| a == PRIMARY_ARG);

    let mut buf = Vec::new();
    if let Err(e) = std::io::stdin().read_to_end(&mut buf) {
        println!("can't read clipboard data: {e}");
        std::process::exit(1);
    }

    let (extra, buf) = match split_extra(&args, buf) {
        Ok(split) => split,
        Err(e) => {
            println!("{e}");
            std::process::exit(1);
        }
    };

    let mut opts = Options::new();
    opts.foreground(true);
    opts.clipboard(if primary {
        ClipboardType::Primary
    } else {
        ClipboardType::Regular
    });

    let len = buf.len();
    let kind = if text { "text" } else { "image" };
    let target = if primary { "primary" } else { "clipboard" };

    if crate::backend::desktop_is("GNOME") {
        if primary {
            println!("the primary selection isn't supported on GNOME");
            std::process::exit(1);
        }
        let offers: Vec<(String, Arc<[u8]>)> = if text {
            let data: Arc<[u8]> = buf.into();
            mutter::TEXT_MIMES
                .iter()
                .map(|m| (m.to_string(), data.clone()))
                .collect()
        } else {
            let png: Arc<[u8]> = buf.into();
            let mut offers: Vec<(String, Arc<[u8]>)> = extra
                .into_iter()
                .map(|(mime, data)| (mime, data.into()))
                .collect();
            offers.extend(
                [
                    "image/png",
                    "application/x-qt-image",
                    "x-kde-force-image-copy",
                ]
                .map(|m| (m.to_string(), png.clone())),
            );
            offers
        };

        let ready = Cell::new(false);
        let started = Instant::now();
        match mutter::serve(offers, || {
            ready.set(true);
            let mut out = std::io::stdout();
            let _ = writeln!(out);
            let _ = out.flush();
            info!("serving {len} bytes of {kind} to {target}");
        }) {
            Ok(()) => info!("taken over after {:.1?}", started.elapsed()),
            Err(e) if !ready.get() => {
                println!("{}", e.to_string().replace('\n', " "));
                std::process::exit(1);
            }
            Err(e) => {
                error!("stopped serving after {:.1?}: {e}", started.elapsed());
                std::process::exit(1);
            }
        }
        return;
    }

    let sources = if text {
        vec![MimeSource {
            source: Source::Bytes(buf.into()),
            mime_type: MimeType::Text,
        }]
    } else {
        let mut sources: Vec<MimeSource> = extra
            .into_iter()
            .map(|(mime, data)| MimeSource {
                source: Source::Bytes(data.into()),
                mime_type: MimeType::Specific(mime),
            })
            .collect();
        sources.extend([
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
        sources
    };

    // the parent reads this line: empty once the selection is ours
    let mut stdout = std::io::stdout();
    match opts.prepare_copy_multi(sources) {
        Ok(copy) => {
            let _ = writeln!(stdout);
            let _ = stdout.flush();
            info!("serving {len} bytes of {kind} to {target}");
            let started = Instant::now();
            match copy.serve() {
                Ok(()) => info!("taken over after {:.1?}", started.elapsed()),
                Err(e) => {
                    error!("stopped serving after {:.1?}: {e:?}", started.elapsed());
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            let _ = writeln!(stdout, "{}", e.to_string().replace('\n', " "));
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::split_extra;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn no_extra_keeps_all_of_stdin() {
        let (extra, png) = split_extra(&args(&["--clipboard-daemon"]), b"png".to_vec()).unwrap();
        assert_eq!((extra, png), (None, b"png".to_vec()));
    }

    #[test]
    fn extra_comes_first_in_stdin() {
        let a = args(&["--clipboard-daemon", "extra", "image/avif", "3"]);
        let (extra, png) = split_extra(&a, b"avfPNG".to_vec()).unwrap();
        assert_eq!(extra, Some(("image/avif".to_string(), b"avf".to_vec())));
        assert_eq!(png, b"PNG");
    }

    #[test]
    fn bad_extra_args_are_errors() {
        for list in [
            &["extra"][..],
            &["extra", "image/avif"],
            &["extra", "image/avif", "x"],
            &["extra", "image/avif", "-1"],
            &["extra", "image/avif", "99"],
        ] {
            assert!(split_extra(&args(list), b"abc".to_vec()).is_err());
        }
    }
}
