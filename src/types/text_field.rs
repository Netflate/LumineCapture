/// another text engine, but unlike tool/text.rs, this is for simple single-line text input fields
/// so i couldn't use the same functions in both files, since tool one is for rich text and use editor

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