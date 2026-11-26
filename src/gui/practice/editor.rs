//! Lightweight multi-line code editor buffer for the Daily Practice panel.
//!
//! Uses UTF-8 safe byte offsets into a single `String`, supports cursor movement,
//! full-buffer selection, auto-indent, smart Tab/Shift+Tab, word navigation,
//! undo/redo history, and bounded text size.

const MAX_UNDO_DEPTH: usize = 128;
pub const MAX_SOURCE_BYTES: usize = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    text: String,
    cursor: usize,
    anchor: Option<usize>,
}

#[derive(Clone, Debug)]
pub struct CodeEditor {
    text: String,
    /// Byte offset in `text` (always on a UTF-8 char boundary).
    cursor: usize,
    /// Selection anchor byte offset (if `Some`, range is `min(anchor, cursor)..max(anchor, cursor)`).
    anchor: Option<usize>,
    indent_spaces: usize,
    undo_stack: Vec<Snapshot>,
    redo_stack: Vec<Snapshot>,
}

impl Default for CodeEditor {
    fn default() -> Self {
        Self::new(String::new(), 4)
    }
}

impl CodeEditor {
    pub fn new(text: String, indent_spaces: usize) -> Self {
        let normalized = normalize_newlines(&text);
        let cursor = normalized.len();
        Self {
            text: normalized,
            cursor,
            anchor: None,
            indent_spaces: indent_spaces.clamp(2, 8),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn set_text(&mut self, text: &str, indent_spaces: usize) {
        self.text = normalize_newlines(text);
        self.indent_spaces = indent_spaces.clamp(2, 8);
        self.cursor = self.text.len();
        self.anchor = None;
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor_byte(&self) -> usize {
        self.cursor
    }

    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        if anchor == self.cursor {
            None
        } else {
            Some((anchor.min(self.cursor), anchor.max(self.cursor)))
        }
    }

    pub fn selected_text(&self) -> Option<&str> {
        let (start, end) = self.selection_range()?;
        self.text.get(start..end)
    }

    pub fn clear_selection(&mut self) -> bool {
        self.anchor.take().is_some()
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.text.len();
    }

    /// 0-based `(line, char_col)` of the cursor.
    pub fn cursor_line_col(&self) -> (usize, usize) {
        let before = &self.text[..self.cursor];
        let line = before.bytes().filter(|&b| b == b'\n').count();
        let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col = before[line_start..].chars().count();
        (line, col)
    }

    pub fn line_count(&self) -> usize {
        self.text.bytes().filter(|&b| b == b'\n').count() + 1
    }

    /// Byte ranges `(start, end)` for every line (excluding trailing `\n`).
    pub fn line_ranges(&self) -> Vec<(usize, usize)> {
        let mut ranges = Vec::new();
        let mut start = 0usize;
        for (idx, b) in self.text.bytes().enumerate() {
            if b == b'\n' {
                ranges.push((start, idx));
                start = idx + 1;
            }
        }
        ranges.push((start, self.text.len()));
        ranges
    }

    /// Move cursor to 0-based `(line, char_col)` clamped to valid bounds.
    pub fn set_cursor_line_col(&mut self, line: usize, col: usize, extend_selection: bool) {
        let ranges = self.line_ranges();
        let clamped_line = line.min(ranges.len().saturating_sub(1));
        let (start, end) = ranges[clamped_line];
        let slice = &self.text[start..end];
        let byte_col = slice
            .char_indices()
            .nth(col)
            .map(|(i, _)| i)
            .unwrap_or(slice.len());
        self.move_to(start + byte_col, extend_selection);
    }

    fn push_undo(&mut self) {
        self.redo_stack.clear();
        self.undo_stack.push(Snapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
        });
        if self.undo_stack.len() > MAX_UNDO_DEPTH {
            self.undo_stack.remove(0);
        }
    }

    pub fn undo(&mut self) -> bool {
        let Some(prev) = self.undo_stack.pop() else {
            return false;
        };
        self.redo_stack.push(Snapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
        });
        self.text = prev.text;
        self.cursor = clamp_char_boundary(&self.text, prev.cursor);
        self.anchor = prev.anchor.map(|a| clamp_char_boundary(&self.text, a));
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        self.undo_stack.push(Snapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
        });
        self.text = next.text;
        self.cursor = clamp_char_boundary(&self.text, next.cursor);
        self.anchor = next.anchor.map(|a| clamp_char_boundary(&self.text, a));
        true
    }

    fn delete_selection_raw(&mut self) -> bool {
        let Some((start, end)) = self.selection_range() else {
            self.anchor = None;
            return false;
        };
        self.text.replace_range(start..end, "");
        self.cursor = start;
        self.anchor = None;
        true
    }

    pub fn insert_str(&mut self, raw: &str) -> bool {
        let s = normalize_newlines(raw);
        if s.is_empty() {
            return false;
        }
        let sel_len = self.selection_range().map(|(a, b)| b - a).unwrap_or(0);
        if self.text.len().saturating_sub(sel_len) + s.len() > MAX_SOURCE_BYTES {
            return false;
        }
        self.push_undo();
        self.delete_selection_raw();
        self.text.insert_str(self.cursor, &s);
        self.cursor += s.len();
        true
    }

    pub fn insert_newline_auto_indent(&mut self) -> bool {
        let (line_start, _) = self.current_line_bounds();
        let before_cursor = &self.text[line_start..self.cursor];
        let existing_indent: String = before_cursor
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let trimmed = before_cursor.trim_end();
        let extra = if trimmed.ends_with('{')
            || trimmed.ends_with(':')
            || trimmed.ends_with("do")
            || trimmed.ends_with("then")
        {
            " ".repeat(self.indent_spaces)
        } else {
            String::new()
        };
        let insertion = format!("\n{existing_indent}{extra}");
        self.insert_str(&insertion)
    }

    pub fn insert_tab(&mut self) -> bool {
        let spaces = " ".repeat(self.indent_spaces);
        self.insert_str(&spaces)
    }

    pub fn unindent_line(&mut self) -> bool {
        let (line_start, line_end) = self.current_line_bounds();
        let line = &self.text[line_start..line_end];
        let removable = if line.starts_with('\t') {
            1
        } else {
            line.bytes()
                .take(self.indent_spaces)
                .take_while(|&b| b == b' ')
                .count()
        };
        if removable == 0 {
            return false;
        }
        self.push_undo();
        self.text.replace_range(line_start..line_start + removable, "");
        if self.cursor > line_start {
            self.cursor = self.cursor.saturating_sub(removable).max(line_start);
        }
        self.anchor = None;
        true
    }

    pub fn backspace(&mut self) -> bool {
        if self.selection_range().is_some() {
            self.push_undo();
            self.delete_selection_raw();
            return true;
        }
        if self.cursor == 0 {
            return false;
        }
        let prev = prev_char_boundary(&self.text, self.cursor);
        self.push_undo();
        self.text.replace_range(prev..self.cursor, "");
        self.cursor = prev;
        true
    }

    pub fn delete_forward(&mut self) -> bool {
        if self.selection_range().is_some() {
            self.push_undo();
            self.delete_selection_raw();
            return true;
        }
        if self.cursor >= self.text.len() {
            return false;
        }
        let next = next_char_boundary(&self.text, self.cursor);
        self.push_undo();
        self.text.replace_range(self.cursor..next, "");
        true
    }

    pub fn cut_selection(&mut self) -> Option<String> {
        let text = self.selected_text()?.to_string();
        self.push_undo();
        self.delete_selection_raw();
        Some(text)
    }

    pub fn move_left(&mut self, extend_selection: bool) {
        if !extend_selection {
            if let Some((start, _)) = self.selection_range() {
                self.cursor = start;
                self.anchor = None;
                return;
            }
        }
        let target = prev_char_boundary(&self.text, self.cursor);
        self.move_to(target, extend_selection);
    }

    pub fn move_right(&mut self, extend_selection: bool) {
        if !extend_selection {
            if let Some((_, end)) = self.selection_range() {
                self.cursor = end;
                self.anchor = None;
                return;
            }
        }
        let target = next_char_boundary(&self.text, self.cursor);
        self.move_to(target, extend_selection);
    }

    pub fn move_word_left(&mut self, extend_selection: bool) {
        let mut idx = self.cursor;
        while idx > 0 {
            let prev = prev_char_boundary(&self.text, idx);
            let ch = self.text[prev..idx].chars().next().unwrap_or(' ');
            idx = prev;
            if !ch.is_whitespace() {
                break;
            }
        }
        while idx > 0 {
            let prev = prev_char_boundary(&self.text, idx);
            let ch = self.text[prev..idx].chars().next().unwrap_or(' ');
            if !is_word_char(ch) {
                break;
            }
            idx = prev;
        }
        self.move_to(idx, extend_selection);
    }

    pub fn move_word_right(&mut self, extend_selection: bool) {
        let mut idx = self.cursor;
        let len = self.text.len();
        while idx < len {
            let next = next_char_boundary(&self.text, idx);
            let ch = self.text[idx..next].chars().next().unwrap_or(' ');
            if !is_word_char(ch) {
                break;
            }
            idx = next;
        }
        while idx < len {
            let next = next_char_boundary(&self.text, idx);
            let ch = self.text[idx..next].chars().next().unwrap_or(' ');
            if !ch.is_whitespace() {
                break;
            }
            idx = next;
        }
        self.move_to(idx, extend_selection);
    }

    pub fn move_up(&mut self, extend_selection: bool) {
        let (line, col) = self.cursor_line_col();
        if line == 0 {
            self.move_to(0, extend_selection);
        } else {
            self.set_cursor_line_col(line - 1, col, extend_selection);
        }
    }

    pub fn move_down(&mut self, extend_selection: bool) {
        let (line, col) = self.cursor_line_col();
        let max_line = self.line_count().saturating_sub(1);
        if line >= max_line {
            self.move_to(self.text.len(), extend_selection);
        } else {
            self.set_cursor_line_col(line + 1, col, extend_selection);
        }
    }

    pub fn move_home(&mut self, extend_selection: bool) {
        let (line_start, _) = self.current_line_bounds();
        self.move_to(line_start, extend_selection);
    }

    pub fn move_end(&mut self, extend_selection: bool) {
        let (_, line_end) = self.current_line_bounds();
        self.move_to(line_end, extend_selection);
    }

    fn current_line_bounds(&self) -> (usize, usize) {
        let start = self.text[..self.cursor]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let end = self.text[self.cursor..]
            .find('\n')
            .map(|rel| self.cursor + rel)
            .unwrap_or(self.text.len());
        (start, end)
    }

    fn move_to(&mut self, target: usize, extend_selection: bool) {
        let clamped = clamp_char_boundary(&self.text, target);
        if extend_selection {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = clamped;
    }
}

fn normalize_newlines(s: &str) -> String {
    s.replace("\r\n", "\n").replace('\r', "\n")
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn prev_char_boundary(s: &str, idx: usize) -> usize {
    if idx == 0 {
        return 0;
    }
    let mut i = idx.min(s.len()) - 1;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn next_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx + 1;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

fn clamp_char_boundary(s: &str, idx: usize) -> usize {
    let mut i = idx.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editing_auto_indent_tab_and_undo_redo_work() {
        let mut ed = CodeEditor::new("def solve(n):".into(), 4);
        assert_eq!(ed.cursor_line_col(), (0, 13));

        assert!(ed.insert_newline_auto_indent());
        assert_eq!(ed.text(), "def solve(n):\n    ");
        assert_eq!(ed.cursor_line_col(), (1, 4));

        assert!(ed.insert_str("return n + 1"));
        assert_eq!(ed.text(), "def solve(n):\n    return n + 1");

        assert!(ed.unindent_line());
        assert_eq!(ed.text(), "def solve(n):\nreturn n + 1");

        assert!(ed.undo());
        assert_eq!(ed.text(), "def solve(n):\n    return n + 1");

        assert!(ed.redo());
        assert_eq!(ed.text(), "def solve(n):\nreturn n + 1");
    }

    #[test]
    fn select_all_replace_and_utf8_navigation_are_safe() {
        let mut ed = CodeEditor::new("héllo 世界".into(), 4);
        ed.move_left(false);
        assert!(ed.backspace());
        assert_eq!(ed.text(), "héllo 界");

        ed.select_all();
        assert_eq!(ed.selected_text(), Some("héllo 界"));
        assert!(ed.insert_str("ok"));
        assert_eq!(ed.text(), "ok");
        assert_eq!(ed.cursor_line_col(), (0, 2));
    }

    #[test]
    fn rejects_payload_exceeding_max_source_bytes() {
        let mut ed = CodeEditor::new(String::new(), 4);
        let huge = "a".repeat(MAX_SOURCE_BYTES + 1);
        assert!(!ed.insert_str(&huge));
        assert!(ed.text().is_empty());
    }
}
