// Shots that were only copied without saving are still temporarly
// saved so the notification can preview them.
//
// Only for daemons that need a separate file for a proper preview,
// like Plasma one, the rest directly take the pixels
//
// set to Read-only, and kept in XDG_RUNTIME_DIR, which is wiped on logout.

use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;

pub fn write(png: &[u8]) -> io::Result<PathBuf> {
    let dir = dirs::runtime_dir()
        .ok_or_else(|| io::Error::other("XDG_RUNTIME_DIR is not set"))?
        .join("LumineCapture")
        .join("previews");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&dir)?;
    let format = &crate::config::get().save.filename_format;
    let stem = chrono::Local::now().format(format).to_string();
    crate::utils::write_unique(&dir, &stem, png, 0o400)
}
