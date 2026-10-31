// Shot delivery: pin, save, copy to clipboard and stdout.

use std::path::Path;

use crate::app::render_final;
use crate::backend::initialize_clipboard;
use crate::backend::notify::{self, Notice};
use crate::editor::EditorState;
use crate::types::Outputs;
use crate::utils::{save_to_file, write_to_stdout};

pub async fn finish_capture(
    editor_state: &mut EditorState,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(outputs) = editor_state.finish else {
        return Ok(());
    };
    let Some((png, _, scale)) = render_final(editor_state) else {
        return Ok(());
    };
    deliver(
        png,
        scale,
        outputs | editor_state.to,
        editor_state.output.as_deref(),
    )
    .await;
    Ok(())
}

/// Pins, saves and copies the finished shot, then tells the user about it.
pub async fn deliver(png: Vec<u8>, scale: f32, outputs: Outputs, dir: Option<&Path>) {
    if outputs.pin
        && let Err(e) = crate::backend::wayland::pin::spawn(&png, scale)
    {
        log::error!("pin failed: {e}");
        notify::send(Notice::PinFailed(e.to_string())).await;
    }
    if outputs.stdout {
        to_stdout(&png, true).await;
    }
    let saved = if outputs.save {
        match save_to_file(&png, dir) {
            Ok(path) => Some(path),
            Err(e) => {
                log::error!("save failed: {e}");
                notify::send(Notice::SaveFailed(e.to_string())).await;
                None
            }
        }
    } else {
        None
    };
    if outputs.copy {
        match initialize_clipboard().copy_image_to_clipboard(&png) {
            Ok(()) => {
                let preview = saved.is_none().then_some(png.as_slice());
                notify::send(Notice::Copied(saved, preview)).await
            }
            Err(e) => {
                log::error!("copy failed: {e}");
                notify::send(Notice::CopyFailed(e.to_string())).await
            }
        }
    } else if outputs.save
        && let Some(path) = saved
    {
        notify::send(Notice::Saved(path)).await;
    }
}

pub(crate) async fn to_stdout(data: &[u8], binary: bool) {
    if let Err(e) = write_to_stdout(data, binary) {
        log::error!("can't write to stdout: {e}");
        notify::send(Notice::StdoutFailed(e.to_string())).await;
    }
}
