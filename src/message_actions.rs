use crate::app::model::{MessageBody, MessageRecord};
use linkify::{LinkFinder, LinkKind};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DesktopAction {
    CopyText,
    OpenLink(String),
    CopyLink(String),
}
pub fn text(message: &MessageRecord, now_ms: i64) -> Option<&str> {
    if message.expires_at_ms.is_some_and(|at| at <= now_ms) {
        return None;
    }
    match &message.body {
        MessageBody::Text(text)
        | MessageBody::Unsupported {
            caption: Some(text),
            ..
        } if !text.is_empty() => Some(text),
        _ => None,
    }
}
pub fn web_links(text: &str) -> Vec<String> {
    let mut finder = LinkFinder::new();
    finder.kinds(&[LinkKind::Url]);
    let mut seen = HashSet::new();
    finder
        .links(text)
        .filter_map(|link| {
            let mut url = link.as_str();
            if let Some(marker) = text[..link.start()].chars().next_back()
                && matches!(marker, '*' | '_' | '~' | '`')
                && url.ends_with(marker)
            {
                url = &url[..url.len() - marker.len_utf8()];
            }
            let adjacent_control = text[link.end()..]
                .chars()
                .next()
                .is_some_and(|c| c.is_control() && !c.is_whitespace());
            (valid_web_link(url) && !adjacent_control && seen.insert(url.to_owned()))
                .then(|| url.to_owned())
        })
        .take(32)
        .collect()
}
pub(crate) fn valid_web_link(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        && !authority.is_empty() && !authority.contains('@') && url.len() <= 4096
        && !url.chars().any(|c| c.is_control() || c.is_whitespace()
            || matches!(c, '\\' | '<' | '>' | '"' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
}
