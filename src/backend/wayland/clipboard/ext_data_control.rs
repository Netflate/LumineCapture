use crate::backend::ClipboardProvider;

pub struct ClipboardMethod;

impl ClipboardProvider for ClipboardMethod {
    fn copy_image_to_clipboard(&self, png_data: Vec<u8>) -> Result<(), Box<dyn std::error::Error>> {
        crate::utils::spawn_self(&["--clipboard-daemon"], &png_data)
    }
}
