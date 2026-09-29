use linkify::{LinkFinder, LinkKind};
use std::{borrow::Cow, collections::BTreeMap, ops::Range};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Format {
    Bold,
    Italic,
    Strike,
    Code,
}

pub(crate) struct Mark {
    pub length: usize,
    pub format: Format,
    pub open: bool,
}

// The renderer and link discovery must agree about which bytes are markup.
// Only complete graphemes can be delimiters; a star keycap is ordinary text.
// A stack of three distinct emphasis kinds bounds nesting without recursion.
pub(crate) fn marks(text: &str) -> BTreeMap<usize, Mark> {
    let graphemes: Vec<_> = text.grapheme_indices(true).collect();
    let ticks: Vec<_> = graphemes
        .iter()
        .enumerate()
        .filter_map(|(i, (_, g))| (*g == "`").then_some(i))
        .collect();
    let triples: Vec<_> = graphemes
        .windows(3)
        .enumerate()
        .filter_map(|(i, window)| window.iter().all(|(_, g)| *g == "`").then_some(i))
        .collect();
    let links = link_ranges(text);
    let mut links = links
        .iter()
        .map(|range| {
            let trailing =
                range.start + text[range.clone()].trim_end_matches(['*', '_', '~']).len();
            (range, trailing)
        })
        .peekable();
    let mut result = BTreeMap::new();
    let mut stack: Vec<(&str, usize, Format)> = vec![];
    let mut index = 0;
    while index < graphemes.len() {
        let (i, grapheme) = graphemes[index];
        if grapheme == "`" {
            let (length, ends) = if triples.binary_search(&index).is_ok() {
                (3, &triples)
            } else {
                (1, &ticks)
            };
            if let Some(&end_index) = ends.get(ends.partition_point(|end| *end < index + length))
                && end_index > index + length
                && (length == 3 || !text[i + length..graphemes[end_index].0].contains('\n'))
            {
                for (at, open) in [(i, true), (graphemes[end_index].0, false)] {
                    result.insert(
                        at,
                        Mark {
                            length,
                            format: Format::Code,
                            open,
                        },
                    );
                }
                index = end_index + length;
            } else {
                index += length;
            }
            continue;
        }
        index += 1;
        let format = match grapheme {
            "*" => Format::Bold,
            "_" => Format::Italic,
            "~" => Format::Strike,
            _ => continue,
        };
        while links.peek().is_some_and(|(range, _)| range.end <= i) {
            links.next();
        }
        let in_url = links.peek().filter(|(range, _)| range.contains(&i));
        // URL paths can contain underscores and tildes. Only an already-open
        // emphasis span may close in a URL's trailing run of delimiters.
        if in_url.is_some_and(|(_, trailing)| i < *trailing) {
            continue;
        }
        let before = text[..i].chars().next_back();
        let after = text[i + 1..].chars().next();
        let ch = grapheme.as_bytes()[0] as char;
        let opens = in_url.is_none()
            && after.is_some_and(|c| !c.is_whitespace() && c != ch)
            && !before.is_some_and(|c| c.is_alphanumeric() || c == ch);
        let closes = before.is_some_and(|c| !c.is_whitespace() && c != ch)
            && !after.is_some_and(|c| c.is_alphanumeric() || c == ch);
        if closes && stack.last().is_some_and(|(g, _, _)| *g == grapheme) {
            let (_, start, format) = stack.pop().unwrap();
            for (at, open) in [(start, true), (i, false)] {
                result.insert(
                    at,
                    Mark {
                        length: 1,
                        format,
                        open,
                    },
                );
            }
        } else if opens && !stack.iter().any(|(g, _, _)| *g == grapheme) {
            stack.push((grapheme, i, format));
        }
    }
    result
}

pub(crate) fn links(text: &str) -> Vec<Range<usize>> {
    let mut literal = Cow::Borrowed(text);
    for (i, mark) in marks(text) {
        // All matched delimiters are ASCII; masking preserves source offsets
        // and prevents markup from being interpreted as part of a destination.
        literal
            .to_mut()
            .replace_range(i..i + mark.length, &" ".repeat(mark.length));
    }
    link_ranges(&literal)
}

fn link_ranges(text: &str) -> Vec<Range<usize>> {
    let mut finder = LinkFinder::new();
    finder.kinds(&[LinkKind::Url]);
    let mut scan = Cow::Borrowed(text);
    for link in finder.links(text) {
        if text.as_bytes().get(link.end()) == Some(&b'#') {
            // linkify 0.11 misses fragments immediately after the authority.
            // A same-width path separator lets its punctuation/quote handling
            // scan the suffix; returned ranges still select the original '#'.
            scan.to_mut().replace_range(link.end()..link.end() + 1, "/");
        }
    }
    finder
        .links(&scan)
        .map(|link| link.start()..link.end())
        .collect()
}
