use std::collections::HashMap;

use tiny_skia::Pixmap;

use crate::editor::{EditorState, Layers, OcrState, TextState};
use crate::types::{Capture, Placement};

pub fn editor(window: bool) -> EditorState {
    let image = Pixmap::new(600, 400).unwrap();
    let sheet = crate::edit::Sheet::new(&image, 1.0);
    let placement = Placement {
        position: (0, 0),
        size: (300, 200),
        scale: 1.0,
    };
    let layer = || vec![Pixmap::new(300, 200).unwrap()];
    let mut state = EditorState::new(
        Layers {
            captures: vec![Capture {
                scale: 1.0,
                pixmap: image,
            }],
            canvas: layer(),
            dimmed: layer(),
            annotations: layer(),
        },
        vec![placement],
        HashMap::new(),
        TextState {
            font_system: crate::fonts::bundled(),
            system_fonts: None,
            swash_cache: cosmic_text::SwashCache::new(),
            editors: HashMap::new(),
            editing: None,
        },
        OcrState {
            runtime: crate::ocr::OcrRuntime::new(crate::ocr::settings::Mode::OnDemand),
            models: crate::ocr::models::OcrModels::load(),
            view: crate::ocr::OcrView::default(),
            redrag: false,
            redrag_from: None,
            await_region: false,
            scan_started: None,
        },
    );
    if window {
        state.sheet = Some(sheet);
    }
    state
}
