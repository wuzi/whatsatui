use super::*;
impl App {
    pub(super) fn scroll_timeline(&mut self, delta: isize, effects: &mut Vec<Effect>) {
        if self.view.messages.is_empty() || self.view.loading {
            return;
        }
        let current = self
            .view
            .message_scroll
            .saturating_add(self.timeline_tail_rows);
        let max = self
            .view
            .message_scroll_max
            .saturating_add(self.timeline_tail_rows);
        self.view.timeline_anchor = self.view.messages.last().map(|m| m.key.clone());
        self.view.message_scroll = current.saturating_add_signed(-delta).min(max);
        self.view.message_scroll_max = max;
        self.timeline_tail_rows = 0;
        self.reading_position();
        let boundary = if delta < 0 && current == max && self.view.has_older {
            self.view
                .messages
                .first()
                .map(|m| (PageDirection::Before, m))
        } else if delta > 0 && current == 0 && self.view.has_newer {
            self.view.messages.last().map(|m| (PageDirection::After, m))
        } else {
            None
        };
        if let Some((direction, message)) = boundary {
            self.load_chat(
                Some(PageCursor {
                    direction,
                    key: message.key.clone(),
                    created_at_ms: message.created_at_ms,
                }),
                effects,
            );
        }
    }
}
