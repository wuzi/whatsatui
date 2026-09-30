use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
#[derive(Clone, Debug, Default)]
pub struct Editor {
    text: String,
    cursor: usize,
    column: Option<usize>,
}
#[derive(Clone, Debug)]
pub enum EditAction {
    Insert(String),
    Newline,
    Clear,
    Backspace,
    Delete,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
}
impl Editor {
    pub fn new(text: String) -> Self {
        let cursor = text.len();
        Self {
            text,
            cursor,
            column: None,
        }
    }
    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    pub fn set_cursor(&mut self, byte: usize) {
        self.cursor = self
            .text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .chain(std::iter::once(self.text.len()))
            .take_while(|i| *i <= byte)
            .last()
            .unwrap_or(0);
        self.column = None;
    }
    fn previous(&self) -> usize {
        self.text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .take_while(|i| *i < self.cursor)
            .last()
            .unwrap_or(0)
    }
    fn next(&self) -> usize {
        self.text
            .grapheme_indices(true)
            .map(|(i, _)| i)
            .find(|i| *i > self.cursor)
            .unwrap_or(self.text.len())
    }
    fn line_start(&self) -> usize {
        self.text[..self.cursor].rfind('\n').map_or(0, |i| i + 1)
    }
    fn line_end(&self) -> usize {
        self.text[self.cursor..]
            .find('\n')
            .map_or(self.text.len(), |i| self.cursor + i)
    }
    pub fn apply(&mut self, action: EditAction) -> bool {
        let vertical = matches!(action, EditAction::Up | EditAction::Down);
        if !vertical {
            self.column = None;
        }
        match action {
            EditAction::Insert(text) => {
                if text.is_empty() {
                    return false;
                }
                self.text.insert_str(self.cursor, &text);
                self.cursor += text.len();
                if self.cursor < self.text.len()
                    && !self
                        .text
                        .grapheme_indices(true)
                        .any(|(i, _)| i == self.cursor)
                {
                    self.cursor = self.next();
                }
                true
            }
            EditAction::Newline => self.apply(EditAction::Insert("\n".into())),
            EditAction::Clear => {
                let changed = !self.text.is_empty();
                self.text.clear();
                self.cursor = 0;
                changed
            }
            EditAction::Backspace => {
                if self.cursor == 0 {
                    return false;
                }
                let start = self.previous();
                self.text.replace_range(start..self.cursor, "");
                self.cursor = start;
                true
            }
            EditAction::Delete => {
                if self.cursor == self.text.len() {
                    return false;
                }
                let end = self.next();
                self.text.replace_range(self.cursor..end, "");
                true
            }
            EditAction::Left => {
                self.cursor = self.previous();
                false
            }
            EditAction::Right => {
                self.cursor = self.next();
                false
            }
            EditAction::Home => {
                self.cursor = self.line_start();
                false
            }
            EditAction::End => {
                self.cursor = self.line_end();
                false
            }
            EditAction::Up | EditAction::Down => {
                let start = self.line_start();
                let end = self.line_end();
                let col = self
                    .column
                    .unwrap_or_else(|| self.text[start..self.cursor].width());
                self.column = Some(col);
                let (next_start, next_end) = if matches!(action, EditAction::Up) {
                    if start == 0 {
                        return false;
                    }
                    (
                        self.text[..start - 1].rfind('\n').map_or(0, |i| i + 1),
                        start - 1,
                    )
                } else {
                    if end == self.text.len() {
                        return false;
                    }
                    (
                        end + 1,
                        self.text[end + 1..]
                            .find('\n')
                            .map_or(self.text.len(), |i| end + 1 + i),
                    )
                };
                let mut width = 0;
                self.cursor = next_start;
                for g in self.text[next_start..next_end].graphemes(true) {
                    if width + g.width() > col {
                        break;
                    }
                    width += g.width();
                    self.cursor += g.len();
                }
                false
            }
        }
    }
}
