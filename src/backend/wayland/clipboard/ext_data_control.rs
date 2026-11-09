use crate::backend::ClipboardProvider;
use crate::utils::format::FileFormat;

pub struct ClipboardMethod;

impl ClipboardProvider for ClipboardMethod {
    fn copy_image_to_clipboard(
        &self,
        png_data: &[u8],
        extra: Option<(FileFormat, &[u8])>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some((format, data)) = extra else {
            return crate::utils::spawn_self_ready(
                &[super::DAEMON_ARG],
                png_data,
                super::READY_TIMEOUT,
            );
        };
        let len = data.len().to_string();
        let stdin = [data, png_data].concat();
        crate::utils::spawn_self_ready(
            &[super::DAEMON_ARG, super::EXTRA_ARG, format.mime(), &len],
            &stdin,
            super::READY_TIMEOUT,
        )
    }
}
