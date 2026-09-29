use super::model::{AccountId, ChatId, MessageSearchPage, RequestId};
use super::{editor::Editor, model::ChatSummary};

#[derive(Clone, Debug)]
pub struct MessageSearch {
    pub account: AccountId,
    pub chat: ChatId,
    pub editor: Editor,
    pub selected: usize,
    pub request: Option<RequestId>,
    pub submitted: Option<String>,
    pub page: MessageSearchPage,
    pub error: Option<String>,
}
impl MessageSearch {
    pub fn invalidate(&mut self, reason: Option<&str>) {
        // Keep the running request until its response retires it. Its results
        // are invalid, but editing must not enqueue another whole-history scan.
        self.submitted = None;
        self.page = MessageSearchPage::default();
        self.selected = 0;
        self.error = reason.map(str::to_owned);
    }
}

pub fn normalize_query(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_whitespace() || c.is_control() {
                ' '
            } else {
                c
            }
        })
        .take(256)
        .collect()
}

pub fn edit_query(editor: &mut Editor, edit: super::editor::EditAction) -> bool {
    use super::editor::EditAction;
    let edit = match edit {
        EditAction::Insert(text) => {
            let remaining = 256usize.saturating_sub(editor.text().chars().count());
            EditAction::Insert(normalize_query(&text).chars().take(remaining).collect())
        }
        EditAction::Newline => EditAction::Insert(" ".into()),
        other => other,
    };
    editor.apply(edit)
}

/// Stable ranking keeps recency as the tie-breaker and empty-query ordering.
pub fn rank_chats(chats: &[ChatSummary], query: &str, unread_only: bool) -> Vec<ChatSummary> {
    let query = normalize_query(query).trim().to_lowercase();
    let numeric = query.chars().any(|c| c.is_ascii_digit())
        && query
            .chars()
            .all(|c| c.is_ascii_digit() || "+-() .".contains(c));
    let query = if numeric { digits(&query) } else { query };
    let tokens: Vec<_> = query.split_whitespace().collect();
    let mut scored: Vec<_> = chats
        .iter()
        .filter(|c| !unread_only || c.unread > 0)
        .filter_map(|chat| {
            let name = chat.name.to_lowercase();
            let phone = digits(chat.phone.as_deref().unwrap_or(""));
            let id = chat.chat.0.to_lowercase();
            let mut total = 0usize;
            for token in &tokens {
                let score = [(&name, 0), (&phone, 25), (&id, 50)]
                    .into_iter()
                    .filter_map(|(candidate, penalty)| score(candidate, token).map(|s| s + penalty))
                    .min()?;
                total = total.saturating_add(score);
            }
            Some((total, chat))
        })
        .collect();
    scored.sort_by_key(|(score, _)| *score);
    scored.into_iter().map(|(_, chat)| chat.clone()).collect()
}

fn digits(text: &str) -> String {
    text.chars().filter(char::is_ascii_digit).collect()
}

fn score(candidate: &str, token: &str) -> Option<usize> {
    if candidate == token {
        return Some(0);
    }
    if candidate.starts_with(token) {
        return Some(100);
    }
    if let Some(start) = candidate.find(token) {
        return Some(200 + start.min(99));
    }
    let mut wanted = token.chars();
    let mut next = wanted.next()?;
    let mut start = None;
    for (index, c) in candidate.chars().enumerate() {
        if c == next {
            start.get_or_insert(index);
            if let Some(c) = wanted.next() {
                next = c;
            } else {
                return Some(1000 + index.saturating_sub(start.unwrap()) + start.unwrap());
            }
        }
    }
    None
}
