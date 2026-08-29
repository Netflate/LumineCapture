/// another text engine, but unlike tool/text.rs, this is for simple single-line text input fields
/// so i couldn't use the same functions in both files, since tool one is for rich text and use editor
use std::collections::HashMap;
use std::hash::Hash;
use crate::types::SpecialKey;

pub const SCROLL_SENSITIVITY: f32 = 4.0;
#[derive(Debug, Clone, Default)]
pub struct LineEditState {
    pub text: String,
    pub cursor: usize,                  
    pub selection_anchor: Option<usize>, 
}

impl LineEditState {
    pub fn new(text: String) -> Self {
        let cursor = text.chars().count();
        Self { text, cursor, selection_anchor: None }
    }

    pub fn insert(&mut self, ch: char) {
        self.delete_selection();
        let byte_idx = self.byte_index(self.cursor);
        self.text.insert(byte_idx, ch);
        self.cursor += 1;
    }

    pub fn insert_str(&mut self, s: &str) {
        self.delete_selection();
        let byte_idx = self.byte_index(self.cursor);
        self.text.insert_str(byte_idx, s);
        self.cursor += s.chars().count();
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor == 0 {
            return;
        }
        let byte_idx = self.byte_index(self.cursor - 1);
        let next_byte_idx = self.byte_index(self.cursor);
        self.text.replace_range(byte_idx..next_byte_idx, "");
        self.cursor -= 1;
    }

    pub fn delete_forward(&mut self) {
        if self.delete_selection() {
            return;
        }
        let len = self.text.chars().count();
        if self.cursor >= len {
            return;
        }
        let byte_idx = self.byte_index(self.cursor);
        let next_byte_idx = self.byte_index(self.cursor + 1);
        self.text.replace_range(byte_idx..next_byte_idx, "");
    }

    // ── Moving cursor stuff ─────────────────────────────────

    pub fn move_left(&mut self, extend: bool) {
        self.begin_or_clear_selection(extend);
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn move_right(&mut self, extend: bool) {
        self.begin_or_clear_selection(extend);
        let len = self.text.chars().count();
        if self.cursor < len {
            self.cursor += 1;
        }
    }

    pub fn move_home(&mut self, extend: bool) {
        self.begin_or_clear_selection(extend);
        self.cursor = 0;
    }

    pub fn move_end(&mut self, extend: bool) {
        self.begin_or_clear_selection(extend);
        self.cursor = self.text.chars().count();
    }

    pub fn select_all(&mut self) {
        self.selection_anchor = Some(0);
        self.cursor = self.text.chars().count();
    }

    pub fn clear_selection(&mut self) {
        self.selection_anchor = None;
    }

    fn begin_or_clear_selection(&mut self, extend: bool) {
        if extend {
            if self.selection_anchor.is_none() {
                self.selection_anchor = Some(self.cursor);
            }
        } else {
            self.selection_anchor = None;
        }
    }

    // ── selection ─────────────────────────────────────────

    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.selection_anchor?;
        if anchor == self.cursor {
            return None;
        }
        Some((anchor.min(self.cursor), anchor.max(self.cursor)))
    }

    pub fn selection_byte_range(&self) -> Option<(usize, usize)> {
        let (a, b) = self.selection_range()?;
        Some((self.byte_index(a), self.byte_index(b)))
    }

    pub fn selected_text(&self) -> Option<String> {
        let (a, b) = self.selection_byte_range()?;
        Some(self.text[a..b].to_string())
    }

    fn delete_selection(&mut self) -> bool {
        let Some((start_char, _end_char)) = self.selection_range() else {
            return false;
        };
        let Some((start_byte, end_byte)) = self.selection_byte_range() else {
            return false;
        };
        self.text.replace_range(start_byte..end_byte, "");
        self.cursor = start_char;
        self.selection_anchor = None;
        true
    }

    pub fn cursor_byte(&self) -> usize {
        self.byte_index(self.cursor)
    }

    fn byte_index(&self, char_idx: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(self.text.len())
    }
    pub fn backspace_selection_only(&mut self) {
        self.delete_selection();
    }
}

pub fn is_stepper_char(ch: char) -> bool {
    ch.is_ascii_digit() || ch == '.' || ch == '-'
}

#[derive(Debug, Clone, Copy)]
pub enum CursorInit {
    End,
    SelectAll,
    At(usize), 
}

pub struct FieldEdit<K> {
    pub key: K,
    pub field: LineEditState,
}

pub struct TextFieldGroup<K: Eq + Hash + Copy> {
    pub values: HashMap<K, String>,
    pub editing: Option<FieldEdit<K>>,
}

impl<K: Eq + Hash + Copy> TextFieldGroup<K> {
    pub fn new() -> Self {
        Self { values: HashMap::new(), editing: None }
    }

    pub fn begin_edit(&mut self, key: K, initial_text: String, cursor: CursorInit) {
        let mut field = LineEditState::new(initial_text);
        match cursor {
            CursorInit::End => field.move_end(false),
            CursorInit::SelectAll => field.select_all(),
            CursorInit::At(idx) => {
                let len = field.text.chars().count();
                field.cursor = idx.min(len);
                field.selection_anchor = None;
            }
        }
        self.editing = Some(FieldEdit { key, field });
    }

    pub fn cancel_edit(&mut self) -> bool {
        self.editing.take().is_some()
    }

    pub fn commit_edit(&mut self) -> Option<(K, String)> {
        let edit = self.editing.take()?;
        Some((edit.key, edit.field.text))
    }

    pub fn is_editing(&self) -> bool {
        self.editing.is_some()
    }

    pub fn is_editing_key(&self, key: K) -> bool {
        self.editing.as_ref().map(|e| e.key) == Some(key)
    }

    pub fn insert_char(&mut self, ch: char, allowed: impl Fn(char) -> bool) -> bool {
        let Some(edit) = self.editing.as_mut() else { return false };
        if !allowed(ch) {
            return false;
        }
        edit.field.insert(ch);
        true
    }

    pub fn handle_key(
        &mut self,
        key: SpecialKey,
        ctrl: bool,
        shift: bool,
        allowed: impl Fn(char) -> bool,
    ) -> (bool, bool) {
        let Some(edit) = self.editing.as_mut() else { return (false, false) };

        if ctrl {
            match key {
                SpecialKey::KeyA => {
                    edit.field.select_all();
                    return (true, false);
                }
                SpecialKey::KeyC | SpecialKey::KeyX => {
                    if let Some(sel) = edit.field.selected_text() {
                        crate::utils::copy_to_clipboard(&sel);
                    }
                    if matches!(key, SpecialKey::KeyX) {
                        edit.field.backspace_selection_only();
                        return (true, false);
                    }
                    return (false, false);
                }
                SpecialKey::KeyV => {
                    if let Some(text) = crate::utils::paste_from_clipboard() {
                        let filtered: String = text.chars().filter(|c| allowed(*c)).collect();
                        if !filtered.is_empty() {
                            edit.field.insert_str(&filtered);
                            return (true, false);
                        }
                    }
                    return (false, false);
                }
                _ => {}
            }
        }

        match key {
            SpecialKey::Enter => (false, true),
            SpecialKey::Left => { edit.field.move_left(shift); (true, false) }
            SpecialKey::Right => { edit.field.move_right(shift); (true, false) }
            SpecialKey::Home => { edit.field.move_home(shift); (true, false) }
            SpecialKey::End => { edit.field.move_end(shift); (true, false) }
            SpecialKey::Backspace => { edit.field.backspace(); (true, false) }
            SpecialKey::Delete => { edit.field.delete_forward(); (true, false) }
            _ => (false, false),
        }
    }

    pub fn sync_value(&mut self, key: K, text: String) {
        if self.is_editing_key(key) {
            return;
        }
        self.values.insert(key, text);
    }

    pub fn value(&self, key: K) -> Option<&String> {
        self.values.get(&key)
    }

    // to overwite text while editing
    // obligatory when changing text using arrows & editing is true
    pub fn set_editing_text(&mut self, key: K, text: String) {
        if let Some(edit) = self.editing.as_mut() {
            if edit.key == key {
                edit.field = LineEditState::new(text);
            }
        }
    }
}

pub fn is_hex_char(ch: char) -> bool {
    ch.is_ascii_hexdigit() || ch == '#'
}

pub fn is_rgba_channel_char(ch: char) -> bool {
    ch.is_ascii_digit()
}