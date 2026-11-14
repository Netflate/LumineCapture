// Shot delivery: pin, save, copy to clipboard and stdout.

use std::path::Path;

use crate::app::render_final;
use crate::backend::initialize_clipboard;
use crate::backend::notify::{self, Notice};
use crate::editor::EditorState;
use crate::types::Outputs;
use crate::ui::settings_panel::ValueField;
use crate::utils::format::for_clipboard;
use crate::utils::{copy_to_clipboard, copy_to_primary, save_to_file, write_to_stdout};

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

/// if the editor window can't start for some reason, screenshot is delivered like a plain finish
pub async fn hand_to_window(
    editor_state: &mut EditorState,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some((png, _, scale)) = render_final(editor_state) else {
        return Ok(());
    };
    if let Err(e) = crate::edit::spawn(&png, editor_state.to, editor_state.output.as_deref()) {
        log::error!("can't open the editor window: {e}");
        deliver(png, scale, editor_state.to, editor_state.output.as_deref()).await;
    }
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
    let extra = outputs.copy.then(|| for_clipboard(&png)).flatten();
    let extra = extra
        .as_ref()
        .map(|(format, data)| (*format, data.as_slice()));
    let copied = outputs
        .copy
        .then(|| initialize_clipboard().copy_image_to_clipboard(&png, extra));
    let saved = if outputs.save {
        match save_to_file(&png, extra, dir) {
            Ok((path, fell_back)) => {
                if fell_back {
                    notify::send(Notice::WebpTooBig).await;
                }
                Some(path)
            }
            Err(e) => {
                log::error!("save failed: {e}");
                notify::send(Notice::SaveFailed(e.to_string())).await;
                None
            }
        }
    } else {
        None
    };
    match copied {
        Some(Ok(())) => {
            let preview = saved.is_none().then_some(png.as_slice());
            notify::send(Notice::Copied(saved, preview)).await
        }
        Some(Err(e)) => {
            log::error!("copy failed: {e}");
            notify::send(Notice::CopyFailed(e.to_string())).await
        }
        None => {
            if let Some(path) = saved {
                notify::send(Notice::Saved(path)).await;
            }
        }
    }
}

pub(crate) async fn to_stdout(data: &[u8], binary: bool) {
    if let Err(e) = write_to_stdout(data, binary) {
        log::error!("can't write to stdout: {e}");
        notify::send(Notice::StdoutFailed(e.to_string())).await;
    }
}

pub async fn pick(editor_state: &EditorState) {
    let (Some(outputs), Some(color)) = (editor_state.finish, editor_state.picked) else {
        return;
    };
    let hex = ValueField::Hex.text(color);
    let rgb = ValueField::Rgb.text(color);
    let rgb2 = rgb.clone();
    deliver_text(
        hex,
        outputs,
        move |hex| Notice::ColorCopied(hex, rgb),
        move |hex| Notice::ColorInPrimary(hex, rgb2),
    )
    .await;
}

pub async fn deliver_text(
    text: String,
    outputs: Outputs,
    copied: impl FnOnce(String) -> Notice<'static>,
    in_primary: impl FnOnce(String) -> Notice<'static>,
) {
    if outputs.stdout {
        to_stdout(format!("{text}\n").as_bytes(), false).await;
    }
    let in_clipboard = outputs.copy && copy_ok(copy_to_clipboard(&text)).await;
    let in_primary_sel = outputs.primary && copy_ok(copy_to_primary(&text)).await;
    if in_clipboard {
        notify::send(copied(text)).await;
    } else if in_primary_sel {
        notify::send(in_primary(text)).await;
    }
}

async fn copy_ok(result: Result<(), Box<dyn std::error::Error>>) -> bool {
    if let Err(e) = &result {
        log::error!("copy failed: {e}");
        notify::send(Notice::TextCopyFailed(e.to_string())).await;
    }
    result.is_ok()
}
