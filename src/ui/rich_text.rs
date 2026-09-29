use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
struct Mark {
    length: usize,
    modifier: Modifier,
    code: bool,
    open: bool,
}

// Only matched delimiters become marks. A stack of the three emphasis kinds
// bounds nesting and keeps malformed input literal without recursive parsing.
fn marks(text: &str) -> BTreeMap<usize, Mark> {
    let mut result = BTreeMap::new();
    let mut stack: Vec<(char, usize, Modifier)> = vec![];
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        let ch = rest.chars().next().unwrap();
        if ch == '`' {
            let length = if rest.starts_with("```") { 3 } else { 1 };
            let delimiter = &rest[..length];
            if let Some(offset) = rest[length..].find(delimiter)
                && offset > 0
                && (length == 3 || !rest[length..length + offset].contains('\n'))
            {
                let end = i + length + offset;
                for (at, open) in [(i, true), (end, false)] {
                    result.insert(
                        at,
                        Mark {
                            length,
                            modifier: Modifier::DIM,
                            code: true,
                            open,
                        },
                    );
                }
                i = end + length;
                continue;
            }
            i += length;
            continue;
        }
        let modifier = match ch {
            '*' => Modifier::BOLD,
            '_' => Modifier::ITALIC,
            '~' => Modifier::CROSSED_OUT,
            _ => {
                i += ch.len_utf8();
                continue;
            }
        };
        let before = text[..i].chars().next_back();
        let after = rest[1..].chars().next();
        let opens = after.is_some_and(|c| !c.is_whitespace() && c != ch)
            && !before.is_some_and(|c| c.is_alphanumeric() || c == ch);
        let closes = before.is_some_and(|c| !c.is_whitespace() && c != ch)
            && !after.is_some_and(|c| c.is_alphanumeric() || c == ch);
        if closes && stack.last().is_some_and(|(c, _, _)| *c == ch) {
            let (_, start, modifier) = stack.pop().unwrap();
            for (at, open) in [(start, true), (i, false)] {
                result.insert(
                    at,
                    Mark {
                        length: 1,
                        modifier,
                        code: false,
                        open,
                    },
                );
            }
        } else if opens && !stack.iter().any(|(c, _, _)| *c == ch) {
            stack.push((ch, i, modifier));
        }
        i += 1;
    }
    result
}

pub(super) fn lines(source: &str, width: usize, accent: Style, muted: Style) -> Vec<Line<'static>> {
    let text = safe_text(source);
    let marks = marks(&text);
    let mut output = vec![];
    let mut row = Vec::new();
    let mut columns = 0;
    let mut modifier = Modifier::empty();
    let mut code = false;
    let mut quote = false;
    let mut line_start = true;
    let mut skip_until = 0;
    for (i, grapheme) in text.grapheme_indices(true) {
        if i < skip_until {
            continue;
        }
        if let Some(mark) = marks.get(&i) {
            if mark.open {
                modifier.insert(mark.modifier);
            } else {
                modifier.remove(mark.modifier);
            }
            if mark.code {
                code = mark.open;
            }
            skip_until = i + mark.length;
            continue;
        }
        if grapheme == "\n" {
            output.push(Line::from(std::mem::take(&mut row)));
            columns = 0;
            quote = false;
            line_start = true;
            continue;
        }
        let mut display = grapheme;
        if line_start && !code {
            let rest = &text[i..];
            if rest.starts_with("> ") {
                quote = true;
                display = "│ ";
                skip_until = i + 2;
            } else if rest.starts_with("- ") || rest.starts_with("* ") {
                display = "• ";
                skip_until = i + 2;
            }
        }
        line_start = false;
        let style = if code {
            accent
        } else if quote {
            muted
        } else {
            Style::default()
        }
        .add_modifier(modifier);
        for g in display.graphemes(true) {
            if columns + g.width() > width.max(1) && columns > 0 {
                output.push(Line::from(std::mem::take(&mut row)));
                columns = 0;
            }
            append(&mut row, g, style);
            columns += g.width();
        }
    }
    output.push(Line::from(row));
    output
}

fn append(row: &mut Vec<Span<'static>>, text: &str, style: Style) {
    if let Some(last) = row.last_mut()
        && last.style == style
    {
        last.content.to_mut().push_str(text);
    } else {
        row.push(Span::styled(text.to_owned(), style));
    }
}
