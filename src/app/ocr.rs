// Everything related to -r ocr:
// dragging with no toolbar, immediate closure after choosing a region,
// with the result being scanned in the background,
// after which the user gets a notification about the outcome, successful or not
//
// maybe in the future it's worth implementing an option to make a very small
// toolbar appear in -r ocr, with the ability to choose an ocr model,
// but for now the user has to go to the editor to choose/download something specifically

use crate::backend::notify::{self, Notice};
use crate::editor::EditorState;
use crate::ocr::{self, StartOutcome};
use crate::theme::anim;
use log::{error, info};

use super::finish::deliver_text;

type Result = std::result::Result<(), Box<dyn std::error::Error>>;

pub async fn finish(editor_state: &mut EditorState) -> Result {
    let Some(outputs) = editor_state.finish else {
        return Ok(());
    };
    let Some(region) = editor_state.selection.zone else {
        return Ok(());
    };
    let Some(image) = ocr::composite_region(&editor_state.captures, editor_state.sources(), region)
    else {
        return Ok(());
    };

    let runtime = &mut editor_state.ocr.runtime;
    if let StartOutcome::Unavailable(e) = runtime.start(image) {
        error!("ocr: engine unavailable: {e}");
        notify::send(Notice::TextFailed(e)).await;
        return Ok(());
    }
    let result = loop {
        if let Some(result) = runtime.poll() {
            break result;
        }
        tokio::time::sleep(anim::frame()).await;
    };

    let text = match result {
        Ok(text) => text,
        Err(e) => {
            error!("ocr: recognition failed: {e}");
            notify::send(Notice::TextFailed(e)).await;
            return Ok(());
        }
    };
    if text.is_empty() {
        info!("ocr: no text found in the region");
        notify::send(Notice::NoText).await;
        return Ok(());
    }

    let view = &mut editor_state.ocr.view;
    view.set_lines(text.lines);
    info!("ocr: {} line(s)", view.lines.len());
    deliver_text(
        view.text_to_copy(),
        outputs,
        Notice::TextCopied,
        Notice::TextInPrimary,
    )
    .await;
    Ok(())
}
