use crate::types::{CursorIcon, Placement, SelectionHandle, SignedRect};
use std::path::PathBuf;
use tiny_skia::Pixmap;
use tiny_skia::Rect;

#[inline]
pub fn make_rect(a: (f64, f64), b: (f64, f64)) -> Option<Rect> {
    let x = a.0.min(b.0) as f32;
    let y = a.1.min(b.1) as f32;
    let w = (a.0 - b.0).abs() as f32;
    let h = (a.1 - b.1).abs() as f32;
    if w < 1.0 || h < 1.0 {
        return None;
    }
    Rect::from_xywh(x, y, w, h)
}

pub fn global_point_to_local(
    placements: &[Placement],
    global: (f64, f64),
    fallback_idx: usize,
    fallback_local: (f64, f64),
) -> (usize, f64, f64) {
    let (gx, gy) = global;
    placements
        .iter()
        .enumerate()
        .find_map(|(idx, p)| {
            let (px, py) = p.position;
            let (w, h) = p.size;
            let inside =
                gx >= px as f64 && gx < (px + w) as f64 && gy >= py as f64 && gy < (py + h) as f64;
            inside.then_some((idx, gx - px as f64, gy - py as f64))
        })
        .unwrap_or((fallback_idx, fallback_local.0, fallback_local.1))
}

pub fn encode_png(pixmap: &Pixmap) -> Vec<u8> {
    use image::ImageEncoder;
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};

    let mut png_bytes = Vec::new();

    let rgba = pixmap.data();

    let encoder =
        PngEncoder::new_with_quality(&mut png_bytes, CompressionType::Fast, FilterType::Adaptive);

    encoder
        .write_image(
            rgba,
            pixmap.width(),
            pixmap.height(),
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();

    png_bytes
}

pub fn save_to_file(png_data: &[u8]) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let now = chrono::Local::now();
    let save = &crate::config::get().save;

    let dir = dirs::picture_dir()
        .or_else(|| dirs::home_dir().map(|home| home.join("Pictures")))
        .ok_or("can't find a pictures or home directory")?
        .join(&save.directory)
        .join(now.format(&save.month_format).to_string());

    std::fs::create_dir_all(&dir)?;

    let stem = now.format(&save.filename_format).to_string();
    Ok(write_unique(&dir, &stem, png_data)?)
}

/// Writes `<stem>.png`, or `<stem>_N.png` if taken, never overwriting a file.
fn write_unique(dir: &std::path::Path, stem: &str, data: &[u8]) -> std::io::Result<PathBuf> {
    let mut n = 0;
    loop {
        let filename = match n {
            0 => format!("{stem}.png"),
            n => format!("{stem}_{n}.png"),
        };
        let path = dir.join(filename);
        match std::fs::File::create_new(&path) {
            Ok(mut file) => {
                std::io::Write::write_all(&mut file, data)?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    }
}

#[inline]
pub fn rects_overlap(a: &Rect, b: &Rect) -> bool {
    a.left() < b.right() && a.right() > b.left() && a.top() < b.bottom() && a.bottom() > b.top()
}

// to render necessary monitors
#[inline]
pub fn get_overlapping_monitors(selection: &Rect, placements: &[crate::types::Placement]) -> u32 {
    let mut mask = 0u32;
    for (i, p) in placements.iter().enumerate() {
        let mx = p.position.0 as f32;
        let my = p.position.1 as f32;
        let overlaps = selection.left() < mx + p.size.0 as f32
            && selection.right() > mx
            && selection.top() < my + p.size.1 as f32
            && selection.bottom() > my;
        if overlaps {
            mask |= 1 << i;
        }
    }
    mask
}

// used for selecting/resizing annotations or selection
#[inline]
pub fn hit_test_rect_handle(sel: &Rect, pos: (f64, f64)) -> SelectionHandle {
    let (x, y) = pos;
    let (l, r, t, b) = (
        sel.left() as f64,
        sel.right() as f64,
        sel.top() as f64,
        sel.bottom() as f64,
    );
    let w = sel.width() as f64;
    let h = sel.height() as f64;

    let input = &crate::config::get().input;
    let (ratio, min, max) = (input.corner_ratio as f64, input.corner_min as f64, input.corner_max as f64);
    let corner_w = (w * ratio).clamp(min, max).min(w * 0.5);
    let corner_h = (h * ratio).clamp(min, max).min(h * 0.5);

    let half_pad = input.handle_hit_width as f64 / 2.0;

    // Top-Left
    let in_tl_horizontal = (y - t).abs() <= half_pad && x >= l - half_pad && x <= l + corner_w;
    let in_tl_vertical = (x - l).abs() <= half_pad && y >= t - half_pad && y <= t + corner_h;
    if in_tl_horizontal || in_tl_vertical {
        return SelectionHandle::TopLeft;
    }

    // Top-Right
    let in_tr_horizontal = (y - t).abs() <= half_pad && x >= r - corner_w && x <= r + half_pad;
    let in_tr_vertical = (x - r).abs() <= half_pad && y >= t - half_pad && y <= t + corner_h;
    if in_tr_horizontal || in_tr_vertical {
        return SelectionHandle::TopRight;
    }

    // Bottom-Left
    let in_bl_horizontal = (y - b).abs() <= half_pad && x >= l - half_pad && x <= l + corner_w;
    let in_bl_vertical = (x - l).abs() <= half_pad && y >= b - corner_h && y <= b + half_pad;
    if in_bl_horizontal || in_bl_vertical {
        return SelectionHandle::BottomLeft;
    }

    // Bottom-Right
    let in_br_horizontal = (y - b).abs() <= half_pad && x >= r - corner_w && x <= r + half_pad;
    let in_br_vertical = (x - r).abs() <= half_pad && y >= b - corner_h && y <= b + half_pad;
    if in_br_horizontal || in_br_vertical {
        return SelectionHandle::BottomRight;
    }

    // Top
    if (y - t).abs() <= half_pad && x > l + corner_w && x < r - corner_w {
        return SelectionHandle::Top;
    }
    // Bottom
    if (y - b).abs() <= half_pad && x > l + corner_w && x < r - corner_w {
        return SelectionHandle::Bottom;
    }
    // Left
    if (x - l).abs() <= half_pad && y > t + corner_h && y < b - corner_h {
        return SelectionHandle::Left;
    }
    // Right
    if (x - r).abs() <= half_pad && y > t + corner_h && y < b - corner_h {
        return SelectionHandle::Right;
    }

    if x >= l + half_pad && x <= r - half_pad && y >= t + half_pad && y <= b - half_pad {
        return SelectionHandle::Move;
    }

    SelectionHandle::None
}

pub fn cursor_for_handle(handle: SelectionHandle, dragging: bool) -> Option<CursorIcon> {
    Some(match handle {
        SelectionHandle::TopLeft | SelectionHandle::BottomRight => CursorIcon::NwseResize,
        SelectionHandle::TopRight | SelectionHandle::BottomLeft => CursorIcon::NeswResize,
        SelectionHandle::Top | SelectionHandle::Bottom => CursorIcon::NsResize,
        SelectionHandle::Left | SelectionHandle::Right => CursorIcon::EwResize,
        SelectionHandle::Move => {
            if dragging {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            }
        }
        SelectionHandle::None => return None,
    })
}

#[inline]
pub fn apply_handle_drag(orig: &Rect, handle: SelectionHandle, delta: (f64, f64)) -> SignedRect {
    let (dx, dy) = (delta.0 as f32, delta.1 as f32);
    let (mut l, mut r, mut t, mut b) = (orig.left(), orig.right(), orig.top(), orig.bottom());
    match handle {
        SelectionHandle::TopLeft => {
            l += dx;
            t += dy;
        }
        SelectionHandle::Top => {
            t += dy;
        }
        SelectionHandle::TopRight => {
            r += dx;
            t += dy;
        }
        SelectionHandle::Left => {
            l += dx;
        }
        SelectionHandle::Right => {
            r += dx;
        }
        SelectionHandle::BottomLeft => {
            l += dx;
            b += dy;
        }
        SelectionHandle::Bottom => {
            b += dy;
        }
        SelectionHandle::BottomRight => {
            r += dx;
            b += dy;
        }
        SelectionHandle::Move => {
            l += dx;
            r += dx;
            t += dy;
            b += dy;
        }
        SelectionHandle::None => {}
    }
    SignedRect {
        left: l,
        top: t,
        right: r,
        bottom: b,
    }
}

pub fn get_full_workspace_rect(placements: &[Placement]) -> Option<Rect> {
    if placements.is_empty() {
        return None;
    }

    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;

    for p in placements {
        min_x = min_x.min(p.position.0);
        min_y = min_y.min(p.position.1);
        max_x = max_x.max(p.position.0 + p.size.0);
        max_y = max_y.max(p.position.1 + p.size.1);
    }

    Rect::from_ltrb(min_x as f32, min_y as f32, max_x as f32, max_y as f32)
}

// pixels swap
#[inline]
pub fn to_rgba(pixels: &mut [u8], bgr: bool) {
    for chunk in pixels.chunks_exact_mut(4) {
        if bgr {
            chunk.swap(0, 2);
        }
        chunk[3] = 255;
    }
}

/// Copies into a shm canvas with R/B already swapped, in a single pass.
///
/// Copying first and swizzling the canvas afterwards leaves the buffer holding
/// un-swapped bytes for the length of the second pass, and there is no
/// wl_buffer.release tracking here - so a compositor sampling in that window
/// shows a frame with red and blue exchanged. One pass per pixel means the
/// buffer only ever holds correct colors.
pub fn copy_swizzled(dst: &mut [u8], src: &[u8]) {
    for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        d[0] = s[2];
        d[1] = s[1];
        d[2] = s[0];
        d[3] = s[3];
    }
}


/// Spawns a new instance of this binary and passes image bytes via stdin.
/// Runs in its own process group so Ctrl+C in the terminal won't kill pins or the clipboard handler.
pub fn spawn_self(args: &[&str], stdin: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    spawn_self_with(args, stdin, std::process::Stdio::null()).map(|_| ())
}

/// Like `spawn_self`, but waits for the child's first stdout line:
/// empty once it is ready, the error message otherwise.
pub fn spawn_self_ready(
    args: &[&str],
    stdin: &[u8],
    timeout: std::time::Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::BufRead;

    let stdout = spawn_self_with(args, stdin, std::process::Stdio::piped())?
        .ok_or("child has no stdout")?;

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let read = std::io::BufReader::new(stdout).read_line(&mut line);
        let _ = tx.send(read.map(|_| line));
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(line)) if line == "\n" => Ok(()),
        Ok(Ok(line)) if line.is_empty() => Err("helper process exited unexpectedly".into()),
        Ok(Ok(line)) => Err(line.trim_end().into()),
        Ok(Err(e)) => Err(e.into()),
        Err(_) => Err("helper process didn't respond in time".into()),
    }
}

fn spawn_self_with(
    args: &[&str],
    stdin: &[u8],
    stdout: std::process::Stdio,
) -> Result<Option<std::process::ChildStdout>, Box<dyn std::error::Error>> {
    use std::io::Write;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let mut child = Command::new(std::env::current_exe()?)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(stdout)
        .process_group(0)
        .spawn()?;

    child.stdin.take().ok_or("child has no stdin")?.write_all(stdin)?;
    let stdout = child.stdout.take();

    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(stdout)
}

pub fn copy_to_clipboard(text: &str) -> Result<(), Box<dyn std::error::Error>> {
    use crate::backend::wayland::clipboard::{DAEMON_ARG, READY_TIMEOUT, TEXT_ARG};

    spawn_self_ready(&[DAEMON_ARG, TEXT_ARG], text.as_bytes(), READY_TIMEOUT)
}

pub fn paste_from_clipboard() -> Option<String> {
    use std::io::Read;
    use wl_clipboard_rs::paste::{ClipboardType, MimeType, Seat, get_contents};

    let (mut pipe, _) = get_contents(ClipboardType::Regular, Seat::Unspecified, MimeType::Text).ok()?;
    let mut text = String::new();
    pipe.read_to_string(&mut text).ok()?;
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::write_unique;

    #[test]
    fn saving_twice_with_the_same_name_keeps_both_files() {
        let dir = std::env::temp_dir().join(format!("lumine-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let first = write_unique(&dir, "shot", b"one").unwrap();
        let second = write_unique(&dir, "shot", b"two").unwrap();

        assert_eq!(first.file_name().unwrap(), "shot.png");
        assert_eq!(second.file_name().unwrap(), "shot_1.png");
        assert_eq!(std::fs::read(&first).unwrap(), b"one");
        assert_eq!(std::fs::read(&second).unwrap(), b"two");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
