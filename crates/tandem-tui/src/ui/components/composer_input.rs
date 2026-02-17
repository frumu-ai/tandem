// Adapted from Codex TUI composer/textarea interaction patterns
// (codex/codex-rs/tui/src/public_widgets/composer_input.rs and
// codex/codex-rs/tui/src/bottom_pane/textarea.rs), rewritten for tandem-tui.
use ratatui::layout::Rect;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ComposerInputState {
    text: String,
    cursor: usize,
}

impl ComposerInputState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn set_text(&mut self, text: String) {
        self.text = text;
        self.cursor = self.clamp_boundary(self.cursor.min(self.text.len()));
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub fn insert_char(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    pub fn insert_str(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        self.text.insert_str(self.cursor, s);
        self.cursor += s.len();
        self.cursor = self.clamp_boundary(self.cursor);
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let prev = self.prev_boundary(self.cursor);
        self.text.drain(prev..self.cursor);
        self.cursor = prev;
    }

    pub fn delete_forward(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }
        let next = self.next_boundary(self.cursor);
        self.text.drain(self.cursor..next);
    }

    pub fn move_left(&mut self) {
        self.cursor = self.prev_boundary(self.cursor);
    }

    pub fn move_right(&mut self) {
        self.cursor = self.next_boundary(self.cursor);
    }

    pub fn move_home(&mut self) {
        self.cursor = self.line_start(self.cursor);
    }

    pub fn move_end(&mut self) {
        self.cursor = self.line_end(self.cursor);
    }

    pub fn move_line_up(&mut self) {
        let col = self.column_chars(self.cursor);
        let line_start = self.line_start(self.cursor);
        if line_start == 0 {
            return;
        }
        let prev_line_end = line_start.saturating_sub(1);
        let prev_line_start = self.line_start(prev_line_end);
        self.cursor = self.byte_at_col(prev_line_start, prev_line_end, col);
    }

    pub fn move_line_down(&mut self) {
        let col = self.column_chars(self.cursor);
        let line_end = self.line_end(self.cursor);
        if line_end >= self.text.len() {
            return;
        }
        let next_line_start = line_end + 1;
        let next_line_end = self.line_end(next_line_start);
        self.cursor = self.byte_at_col(next_line_start, next_line_end, col);
    }

    pub fn desired_height(&self, width: u16) -> u16 {
        if width <= 2 {
            return 3;
        }
        let inner_width = (width - 2) as usize;
        let mut rows = 1usize;
        for line in self.text.split('\n') {
            let chars = line.chars().count().max(1);
            rows += (chars - 1) / inner_width;
        }
        let content_rows = rows.clamp(1, 6) as u16;
        (content_rows + 2).clamp(3, 8)
    }

    pub fn cursor_screen_pos(&self, area: Rect) -> (u16, u16) {
        let inner_x = area.x.saturating_add(1);
        let inner_y = area.y.saturating_add(1);
        let inner_w = area.width.saturating_sub(2).max(1);
        let (line_idx, col_chars) = self.line_and_col(self.cursor);
        let wrapped_row = (col_chars as u16) / inner_w;
        let wrapped_col = (col_chars as u16) % inner_w;
        let y = inner_y
            .saturating_add(line_idx as u16)
            .saturating_add(wrapped_row);
        let y_max = area.y.saturating_add(area.height.saturating_sub(2));
        let clamped_y = y.min(y_max);
        let x = inner_x.saturating_add(wrapped_col);
        (x, clamped_y)
    }

    fn line_and_col(&self, pos: usize) -> (usize, usize) {
        let mut line = 0usize;
        let mut col = 0usize;
        for (idx, ch) in self.text.char_indices() {
            if idx >= pos {
                break;
            }
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        (line, col)
    }

    fn line_start(&self, pos: usize) -> usize {
        let p = self.clamp_boundary(pos.min(self.text.len()));
        self.text[..p].rfind('\n').map(|i| i + 1).unwrap_or(0)
    }

    fn line_end(&self, pos: usize) -> usize {
        let p = self.clamp_boundary(pos.min(self.text.len()));
        self.text[p..]
            .find('\n')
            .map(|i| p + i)
            .unwrap_or(self.text.len())
    }

    fn column_chars(&self, pos: usize) -> usize {
        let start = self.line_start(pos);
        self.text[start..pos].chars().count()
    }

    fn byte_at_col(&self, line_start: usize, line_end: usize, target_col: usize) -> usize {
        let mut col = 0usize;
        for (off, ch) in self.text[line_start..line_end].char_indices() {
            if col >= target_col {
                return line_start + off;
            }
            col += 1;
            if col == target_col {
                return line_start + off + ch.len_utf8();
            }
        }
        line_end
    }

    fn clamp_boundary(&self, pos: usize) -> usize {
        let p = pos.min(self.text.len());
        if self.text.is_char_boundary(p) {
            p
        } else {
            self.prev_boundary(p)
        }
    }

    fn prev_boundary(&self, pos: usize) -> usize {
        if pos == 0 {
            return 0;
        }
        let mut i = pos.saturating_sub(1);
        while i > 0 && !self.text.is_char_boundary(i) {
            i = i.saturating_sub(1);
        }
        i
    }

    fn next_boundary(&self, pos: usize) -> usize {
        if pos >= self.text.len() {
            return self.text.len();
        }
        let mut i = pos.saturating_add(1).min(self.text.len());
        while i < self.text.len() && !self.text.is_char_boundary(i) {
            i += 1;
        }
        i
    }
}

#[cfg(test)]
mod tests {
    use super::ComposerInputState;

    #[test]
    fn insert_and_backspace_middle() {
        let mut c = ComposerInputState::new();
        c.insert_str("hello");
        c.move_left();
        c.move_left();
        c.insert_char('X');
        assert_eq!(c.text(), "helXlo");
        c.backspace();
        assert_eq!(c.text(), "hello");
    }

    #[test]
    fn move_between_lines_preserves_column() {
        let mut c = ComposerInputState::new();
        c.insert_str("abc\ndefg");
        c.move_home();
        c.move_right();
        c.move_right();
        c.move_line_down();
        c.insert_char('X');
        assert_eq!(c.text(), "abc\ndeXfg");
    }

    #[test]
    fn desired_height_clamped() {
        let mut c = ComposerInputState::new();
        c.insert_str("line1\nline2\nline3\nline4\nline5\nline6\nline7");
        let h = c.desired_height(40);
        assert!(h <= 8);
        assert!(h >= 3);
    }
}
