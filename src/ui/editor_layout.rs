//! One wrapping model for drawn text, the cursor and mouse caret placement.
use super::*;
pub(super) struct TextLayout {
    pub lines: Vec<String>,
    carets: Vec<(usize, usize, usize)>,
    ends: Vec<usize>,
}
impl TextLayout {
    pub fn new(text: &str, width: usize) -> Self {
        let width = width.max(1);
        let mut result = Self {
            lines: vec![String::new()],
            carets: vec![],
            ends: vec![0],
        };
        let (mut row, mut column) = (0, 0);
        for (byte, g) in text.grapheme_indices(true) {
            if g == "\n" {
                result.carets.push((
                    byte,
                    row + usize::from(column >= width),
                    if column >= width { 0 } else { column },
                ));
                result.ends[row] = byte;
                result.lines.push(String::new());
                result.ends.push(byte + g.len());
                row += 1;
                column = 0;
                continue;
            }
            let safe = safe_text(g);
            let size = safe.width();
            if column > 0 && column + size > width {
                result.lines.push(String::new());
                result.ends.push(byte);
                row += 1;
                column = 0;
            }
            result.carets.push((byte, row, column));
            result.lines[row].push_str(&safe);
            column += size;
            result.ends[row] = byte + g.len();
        }
        if column >= width {
            result.lines.push(String::new());
            result.ends.push(text.len());
            row += 1;
            column = 0;
        }
        result.carets.push((text.len(), row, column));
        result
    }
    pub fn position(&self, byte: usize) -> (usize, usize) {
        self.carets
            .iter()
            .rev()
            .find(|(b, _, _)| *b <= byte)
            .map(|(_, r, c)| (*r, *c))
            .unwrap_or((0, 0))
    }
    pub fn byte_at(&self, row: usize, column: usize) -> usize {
        if self.lines.get(row).is_none_or(|s| column >= s.width()) {
            return self
                .ends
                .get(row)
                .copied()
                .unwrap_or_else(|| *self.ends.last().unwrap_or(&0));
        }
        self.carets
            .iter()
            .rev()
            .find(|(_, r, c)| *r == row && *c <= column)
            .map_or(0, |(b, _, _)| *b)
    }
}
