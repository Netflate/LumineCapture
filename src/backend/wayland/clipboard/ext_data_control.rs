use crate::backend::ClipboardProvider;
use std::io::Write;
use std::process::{Command, Stdio};

pub struct ClipboardMethod;

impl ClipboardProvider for ClipboardMethod {
    fn copy_image_to_clipboard(&self, png_data: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
        let exe = std::env::current_exe()?;
        let mut child = Command::new(exe)
            .arg("--clipboard-daemon")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        child.stdin.take().unwrap().write_all(&png_data)?;

        std::thread::spawn(move || {
            let _ = child.wait();
        });

        Ok(())
    }
}
