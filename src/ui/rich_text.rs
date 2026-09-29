use super::*;
use crate::message_text::{Format, marks};

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
            let style = match mark.format {
                Format::Bold => Modifier::BOLD,
                Format::Italic => Modifier::ITALIC,
                Format::Strike => Modifier::CROSSED_OUT,
                Format::Code => Modifier::DIM,
            };
            if mark.open {
                modifier.insert(style);
            } else {
                modifier.remove(style);
            }
            if mark.format == Format::Code {
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
            // Prefix replacement must not consume a space's combining marks.
            let plain_space = rest.graphemes(true).nth(1) == Some(" ");
            if grapheme == ">" && plain_space {
                quote = true;
                display = "│ ";
                skip_until = i + 2;
            } else if matches!(grapheme, "-" | "*") && plain_space {
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
