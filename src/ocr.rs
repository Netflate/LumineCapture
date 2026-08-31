// Optical character recognition.
//
// `OcrBackend` hides the engine: input `OcrImage`, output `OcrText`. 
// implementation of new engine is a new module & one line in `default_backend`.
//
// layout  - detection boxes -> lines, blocks, reading order
// view    - selection over those lines
// runtime - recognition on a worker thread

pub mod layout;
pub mod paddle_backend;
pub mod runtime;
pub mod view;

use tiny_skia::{Pixmap, PixmapPaint, Rect, Transform};

pub use runtime::{OcrRuntime, StartOutcome};
pub use view::{BlockOverlay, LineSelection, OcrView};

use crate::types::Placement;

/// Tightly packed RGB8 pixels plus the global coordinate of their top-left
/// corner, so results can be mapped back onto the canvas. Owned: it moves to
/// the worker thread and into the backend without another copy.
pub struct OcrImage {
    pub rgb: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub origin: (f32, f32),
}

/// One recognized line. `bounds` is global. `char_x` is the global x of every
/// character boundary: `text.chars().count() + 1` values, non-decreasing,
/// running from `bounds.left()` to `bounds.right()`.
#[derive(Debug, Clone)]
pub struct OcrLine {
    pub text: String,
    pub bounds: Rect,
    pub char_x: Vec<f32>,
}

impl OcrLine {
    pub fn char_count(&self) -> usize {
        self.char_x.len().saturating_sub(1)
    }
}

/// Everything found in one image, lines in reading order.
#[derive(Debug, Clone, Default)]
pub struct OcrText {
    pub lines: Vec<OcrLine>,
}

impl OcrText {
    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|l| l.text.trim().is_empty())
    }
}

pub type OcrError = Box<dyn std::error::Error + Send + Sync>;

/// `Send` so recognition can run on a worker thread. Takes the image by value
/// to avoid copying the full screen buffer.
pub trait OcrBackend: Send {
    fn recognize(&self, image: OcrImage) -> Result<OcrText, OcrError>;
}

/// Build the backend the app ships with today.
pub fn default_backend() -> Result<Box<dyn OcrBackend>, OcrError> {
    Ok(Box::new(paddle_backend::PaddleBackend::new()?))
}

/// Composite the pixels covered by `region` (global coords) out of the
/// per-monitor `base` layers into one contiguous RGB8 buffer. Repeats 
/// base-compositing half of `app::render_final`, without annotations.
pub fn composite_region(
    base: &[Pixmap],
    placements: &[Placement],
    region: Rect,
) -> Option<OcrImage> {
    let left = region.left().floor() as i32;
    let top = region.top().floor() as i32;
    let right = region.right().ceil() as i32;
    let bottom = region.bottom().ceil() as i32;
    let w = (right - left).max(0) as u32;
    let h = (bottom - top).max(0) as u32;
    if w == 0 || h == 0 {
        return None;
    }

    let mut out = Pixmap::new(w, h)?;
    for (i, placement) in placements.iter().enumerate() {
        let Some(base_i) = base.get(i) else {
            continue;
        };
        out.draw_pixmap(
            placement.position.0 - left,
            placement.position.1 - top,
            base_i.as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            None,
        );
    }

    // RGBA -> RGB
    let rgba = out.data();
    let mut rgb = vec![0u8; (w as usize) * (h as usize) * 3];
    for (dst, src) in rgb.chunks_exact_mut(3).zip(rgba.chunks_exact(4)) {
        dst.copy_from_slice(&src[..3]);
    }

    Some(OcrImage {
        rgb,
        width: w,
        height: h,
        origin: (left as f32, top as f32),
    })
}

/// Write recognized text to a timestamped `.txt` in the current working
/// directory (temporary) 
pub fn write_text_file(text: &str) -> std::io::Result<std::path::PathBuf> {
    let name = chrono::Local::now()
        .format("ocr_%Y-%m-%d_%H-%M-%S.txt")
        .to_string();
    let path = std::env::current_dir()?.join(name);
    std::fs::write(&path, text)?;
    Ok(path)
}
