use crate::app::model::{MessageBody, MessageRecord};
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DesktopAction {
    CopyText,
    OpenLink(String),
    CopyLink(String),
}
pub fn available(message: &MessageRecord, now_ms: i64) -> Vec<crate::config::bindings::ActionId> {
    use crate::{app::model::SendState, config::bindings::ActionId as A};
    if message.expires_at_ms.is_some_and(|at| at <= now_ms) {
        return vec![];
    }
    let mut actions = vec![];
    if let MessageBody::Media(attachment) = &message.body
        && attachment.validate().is_ok()
    {
        actions.push(A::DownloadMedia);
        if attachment.extension().is_some() {
            actions.push(A::OpenMedia);
        }
    }
    if let Some(text) = text(message, now_ms) {
        actions.push(A::CopyText);
        if !web_links(text).is_empty() {
            actions.push(A::OpenLinks);
        }
    }
    if matches!(message.body, MessageBody::Text(_)) && !actions.is_empty() {
        actions.push(A::Reply);
        if message.key.from_me
            && matches!(
                message.send_state,
                Some(SendState::Failed | SendState::Unconfirmed)
            )
        {
            actions.push(A::Resend);
        }
    }
    if matches!(message.body, MessageBody::LocalImage { .. })
        && message.key.from_me
        && matches!(
            message.send_state,
            Some(SendState::Failed | SendState::Unconfirmed)
        )
    {
        actions.push(A::Resend);
    }
    actions
}
pub fn text(message: &MessageRecord, now_ms: i64) -> Option<&str> {
    if message.expires_at_ms.is_some_and(|at| at <= now_ms) {
        return None;
    }
    match &message.body {
        MessageBody::Media(attachment) => attachment.caption.as_deref().filter(|s| !s.is_empty()),
        MessageBody::Text(text)
        | MessageBody::LocalImage { caption: text, .. }
        | MessageBody::Unsupported {
            caption: Some(text),
            ..
        } if !text.is_empty() => Some(text),
        _ => None,
    }
}
pub fn web_links(text: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    crate::message_text::links(text)
        .into_iter()
        .filter_map(|range| {
            let url = &text[range.clone()];
            let adjacent_control = text[range.end..]
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
