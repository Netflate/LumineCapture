use cosmic_text::{Action, Cursor, Edit, Editor, FontSystem};
use tiny_skia::Rect;

use super::{sync_content_from_editor, text_line_height};
use crate::editor::dirty::apply_damage_rects;
use crate::editor::{DamageZone, EditorState};
use crate::tools::Tool;
use crate::types::AnnotationShape;

/// composing text sits inside the annotation, so it is cut out by its range
/// a click may have moved the cursor away meanwhile
pub struct Preedit {
    id: u64,
    from: Cursor,
    to: Cursor,
}

fn editing(state: &EditorState) -> Option<u64> {
    if state.selected_tool != Tool::Text {
        return None;
    }
    state.text.editing.as_ref().map(|edit| edit.annotation_id)
}

fn damage(state: &mut EditorState, id: u64) {
    if let Some(ann) = state.annotations.iter().find(|a| a.id == id) {
        state.layer_damage_rects.push(ann.damage_bbox(false));
        state
            .damage_rects
            .push(DamageZone::Global(ann.damage_bbox(true)));
    }
}

fn drop_preedit(state: &mut EditorState) {
    let Some(preedit) = state.text.preedit.take() else {
        return;
    };
    let Some(editor) = state.text.editors.get_mut(&preedit.id) else {
        return;
    };
    editor.set_cursor(preedit.to);
    editor.set_selection(cosmic_text::Selection::Normal(preedit.from));
    editor.delete_selection();
}

fn clear(state: &mut EditorState) {
    let Some(id) = state.text.preedit.as_ref().map(|p| p.id) else {
        return;
    };
    damage(state, id);
    drop_preedit(state);
    if let Some(editor) = state.text.editors.get_mut(&id) {
        sync_content_from_editor(
            id,
            editor,
            &mut state.annotations,
            &mut state.text.font_system,
        );
    }
    damage(state, id);
}

fn insert(editor: &mut Editor<'static>, font_system: &mut FontSystem, text: &str) {
    if editor.selection_bounds().is_some() {
        editor.delete_selection();
    }
    for ch in text.chars().filter(|ch| *ch != '\r') {
        editor.action(font_system, Action::Insert(ch));
    }
}

pub fn apply(state: &mut EditorState, preedit: Option<String>, commit: Option<String>) {
    let Some(id) = editing(state) else {
        return;
    };
    if state.text.preedit.as_ref().is_some_and(|p| p.id != id) {
        clear(state);
    }
    damage(state, id);
    drop_preedit(state);

    let Some(editor) = state.text.editors.get_mut(&id) else {
        return;
    };
    if let Some(text) = commit {
        insert(editor, &mut state.text.font_system, &text);
    }
    if let Some(text) = preedit {
        if editor.selection_bounds().is_some() {
            editor.delete_selection();
        }
        let from = editor.cursor();
        insert(editor, &mut state.text.font_system, &text);
        state.text.preedit = Some(Preedit {
            id,
            from,
            to: editor.cursor(),
        });
    }
    sync_content_from_editor(
        id,
        editor,
        &mut state.annotations,
        &mut state.text.font_system,
    );
    damage(state, id);
}

pub fn area(state: &EditorState) -> Option<(i32, i32, i32, i32)> {
    let id = editing(state)?;
    let ann = state.annotations.iter().find(|a| a.id == id)?;
    let AnnotationShape::Text {
        start, font_size, ..
    } = &ann.shape
    else {
        return None;
    };
    let (x, y) = state
        .text
        .editors
        .get(&id)?
        .cursor_position()
        .unwrap_or((0, 0));
    let height = font_size * text_line_height();
    let caret = Rect::from_xywh(start.0 + x as f32, start.1 + y as f32, 1.0, height)?;
    let local = state.to_screen(state.input.pointer.monitor_idx, &caret)?;
    Some((
        local.left().floor() as i32,
        local.top().floor() as i32,
        local.width().ceil().max(1.0) as i32,
        local.height().ceil().max(1.0) as i32,
    ))
}

/// every loop pass: drops a preedit left behind when editing ends by any path
pub fn sync(state: &mut EditorState, dirty_mask: &mut u32) -> Option<(i32, i32, i32, i32)> {
    let id = editing(state);
    if state
        .text
        .preedit
        .as_ref()
        .is_some_and(|p| Some(p.id) != id)
    {
        clear(state);
        apply_damage_rects(state, dirty_mask);
    }
    area(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::testing::editor;
    use crate::tools::ToolBehavior;
    use crate::tools::text::TextTool;
    use crate::types::MouseButton;

    fn typing() -> EditorState {
        let mut state = editor(false);
        state.selected_tool = Tool::Text;
        state.input.pointer.global = (40.0, 50.0);
        TextTool.on_button(&mut state, MouseButton::Left, true, &mut 0);
        TextTool.on_button(&mut state, MouseButton::Left, false, &mut 0);
        state
    }

    fn content(state: &EditorState) -> String {
        match &state.annotations[0].shape {
            AnnotationShape::Text { content, .. } => content.clone(),
            _ => unreachable!(),
        }
    }

    #[test]
    fn preedit_is_replaced_by_the_next_one() {
        let mut state = typing();
        apply(&mut state, Some("n".into()), None);
        assert_eq!(content(&state), "n");
        apply(&mut state, Some("ni".into()), None);
        assert_eq!(content(&state), "ni");
        apply(&mut state, Some("ni h".into()), None);
        assert_eq!(content(&state), "ni h");
    }

    #[test]
    fn commit_takes_the_place_of_the_preedit() {
        let mut state = typing();
        apply(&mut state, Some("nihao".into()), None);
        apply(&mut state, None, Some("你好".into()));
        assert_eq!(content(&state), "你好");
        assert!(state.text.preedit.is_none());
    }

    #[test]
    fn commit_and_new_preedit_in_one_batch() {
        let mut state = typing();
        apply(&mut state, Some("ni".into()), None);
        apply(&mut state, Some("hao".into()), Some("你".into()));
        assert_eq!(content(&state), "你hao");
        apply(&mut state, None, Some("好".into()));
        assert_eq!(content(&state), "你好");
    }

    #[test]
    fn empty_batch_removes_the_preedit() {
        let mut state = typing();
        apply(&mut state, None, Some("ab".into()));
        apply(&mut state, Some("ni".into()), None);
        apply(&mut state, None, None);
        assert_eq!(content(&state), "ab");
    }

    #[test]
    fn preedit_goes_when_editing_ends() {
        let mut state = typing();
        apply(&mut state, Some("ni".into()), None);
        state.text.editing = None;
        let area = sync(&mut state, &mut 0);
        assert_eq!(content(&state), "");
        assert!(state.text.preedit.is_none());
        assert_eq!(area, None);
    }

    #[test]
    fn nothing_is_typed_without_a_field() {
        let mut state = editor(false);
        state.selected_tool = Tool::Text;
        apply(&mut state, None, Some("a".into()));
        assert!(state.annotations.is_empty());
        assert_eq!(sync(&mut state, &mut 0), None);
    }

    #[test]
    fn caret_area_follows_the_text() {
        let mut state = typing();
        let empty = sync(&mut state, &mut 0).unwrap();
        apply(&mut state, None, Some("hello".into()));
        let typed = sync(&mut state, &mut 0).unwrap();
        assert_eq!(empty.0, 40);
        assert!(typed.0 > empty.0);
        assert_eq!(typed.1, empty.1);
        assert!(typed.3 > 1);
    }
}
