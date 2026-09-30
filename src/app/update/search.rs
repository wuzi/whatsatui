use super::*;
use crate::app::search::MessageSearch;

impl App {
    pub(super) fn highlighted_chat(&self) -> Option<ChatId> {
        if let Some(Overlay::Search { selected, .. }) = &self.view.overlay {
            self.search_results().get(*selected).map(|c| c.chat.clone())
        } else {
            None
        }
    }

    pub(super) fn restore_highlighted_chat(&mut self, highlighted: Option<ChatId>) {
        let results = self.search_results();
        if let Some(Overlay::Search { selected, .. }) = &mut self.view.overlay {
            *selected = highlighted
                .and_then(|id| results.iter().position(|c| c.chat == id))
                .unwrap_or_else(|| (*selected).min(results.len().saturating_sub(1)));
        }
    }

    pub(super) fn open_message_search(&mut self) {
        let (Some(account), Some(chat)) = (self.view.account.clone(), self.view.chat.clone())
        else {
            self.view.notice = Some("Select a conversation to find messages".into());
            return;
        };
        self.view.overlay = Some(Overlay::MessageSearch(Box::new(MessageSearch {
            account,
            chat,
            editor: Editor::default(),
            selected: 0,
            request: None,
            submitted: None,
            page: MessageSearchPage::default(),
            error: None,
        })));
    }

    pub(super) fn submit_or_open_message(&mut self, effects: &mut Vec<Effect>) {
        let Some(Overlay::MessageSearch(search)) = &self.view.overlay else {
            return;
        };
        if search.request.is_some() {
            return;
        }
        let query = search.editor.text().trim().to_owned();
        if query.is_empty() {
            return;
        }
        if search.submitted.as_ref() == Some(&query)
            && search.error.is_none()
            && let Some(hit) = search.page.hits.get(search.selected).cloned()
        {
            self.view.overlay = None;
            self.focus(Focus::Messages, effects);
            self.view.at_bottom = false;
            self.view.message_scroll = 0;
            self.view.message_scroll_max = 0;
            self.view.selected_message = Some(hit.key.clone());
            self.view.timeline_anchor = Some(hit.key.clone());
            self.visible_messages.clear();
            self.timeline_tail_rows = 0;
            self.load_chat(
                Some(PageCursor {
                    direction: PageDirection::AtOrBefore,
                    created_at_ms: hit.created_at_ms,
                    key: hit.key,
                }),
                effects,
            );
            return;
        }
        let request = self.request();
        let Some(Overlay::MessageSearch(search)) = &mut self.view.overlay else {
            return;
        };
        search.invalidate(None);
        search.request = Some(request);
        search.submitted = Some(query.clone());
        effects.push(Effect::SearchMessages {
            request,
            account: search.account.clone(),
            chat: search.chat.clone(),
            query,
        });
    }

    pub(super) fn complete_message_search(
        &mut self,
        request: RequestId,
        account: AccountId,
        chat: ChatId,
        query: String,
        result: Result<MessageSearchPage, String>,
    ) {
        let Some(Overlay::MessageSearch(search)) = &mut self.view.overlay else {
            return;
        };
        if self.view.account.as_ref() != Some(&account)
            || self.view.chat.as_ref() != Some(&chat)
            || search.account != account
            || search.chat != chat
            || search.request != Some(request)
        {
            return;
        }
        search.request = None;
        if search.submitted.as_ref() != Some(&query) || search.editor.text().trim() != query {
            return;
        }
        match result {
            Ok(page) => {
                search.page = page;
                search.error = None;
            }
            Err(error) => {
                search.page = MessageSearchPage::default();
                search.error = Some(error);
            }
        }
        search.selected = 0;
    }
}
