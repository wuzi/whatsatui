//! Preserve complete image drafts when WhatsApp joins two contact identities.
use super::model::*;

impl Draft {
    pub fn has_content(&self) -> bool {
        self.has_active_content() || !self.recovered.is_empty()
    }

    fn has_active_content(&self) -> bool {
        !self.text.is_empty() || self.attachment.is_some() || self.reply.is_some()
    }

    fn active(&self, fallback: &ChatId) -> RecoveredDraft {
        RecoveredDraft {
            origin: self.origin.clone().unwrap_or_else(|| fallback.clone()),
            text: self.text.clone(),
            attachment: self.attachment.clone(),
            reply: self.reply.clone(),
        }
    }

    fn activate(&mut self, draft: RecoveredDraft) {
        self.origin = Some(draft.origin);
        self.text = draft.text;
        self.attachment = draft.attachment;
        self.reply = draft.reply;
    }

    /// `edited` gives local edits precedence, including an explicitly empty image.
    pub(crate) fn merge_from(&mut self, other: &Draft, chat: &ChatId, edited: bool) {
        self.merge_content(other.active(chat), edited);
        for recovered in &other.recovered {
            self.merge_content(recovered.clone(), edited);
        }
    }

    /// Accept newly merged composers without overwriting this composer's edits.
    pub(crate) fn recover_from(&mut self, stored: &Draft, chat: &ChatId) {
        if stored.origin.is_none() && stored.recovered.is_empty() {
            return;
        }
        let origin = self.origin.get_or_insert_with(|| chat.clone()).clone();
        let active = stored.active(chat);
        if active.origin != origin {
            self.merge_content(active, false);
        }
        for recovered in &stored.recovered {
            if recovered.origin != origin {
                self.merge_content(recovered.clone(), false);
            }
        }
    }

    fn merge_content(&mut self, other: RecoveredDraft, edited: bool) {
        let other_has_content =
            !other.text.is_empty() || other.attachment.is_some() || other.reply.is_some();
        if let Some(index) = self.recovered.iter().position(|d| d.origin == other.origin) {
            if edited {
                if other_has_content {
                    self.recovered[index] = other;
                } else {
                    self.recovered.remove(index);
                }
            }
            return;
        }
        // Keep the previous text-only merge behavior. Images must keep their own
        // captions and quotes instead of acquiring another composer's text.
        if self.attachment.is_none() && other.attachment.is_none() && self.recovered.is_empty() {
            retain_text(&mut self.text, &other.text);
            if self.reply.is_none() || (edited && self.origin.as_ref() == Some(&other.origin)) {
                self.reply = other.reply;
            }
        } else if self.origin.as_ref() == Some(&other.origin) {
            if edited {
                self.activate(other);
            }
        } else if !self.has_active_content() {
            self.activate(other);
        } else if other_has_content {
            self.recovered.push(other);
        }
    }

    /// Restore without sending; keep the active draft available for a later swap.
    pub(crate) fn restore(&mut self, index: usize, chat: &ChatId) -> bool {
        if index >= self.recovered.len() {
            return false;
        }
        let restored = self.recovered.remove(index);
        if self.has_active_content() {
            self.recovered.push(self.active(chat));
        }
        self.activate(restored);
        true
    }

    pub(crate) fn quotes_mut(&mut self) -> impl Iterator<Item = &mut Quote> {
        self.reply
            .iter_mut()
            .chain(self.recovered.iter_mut().filter_map(|d| d.reply.as_mut()))
    }
}

/// Preserve distinct complete draft texts. A store merge may already contain
/// one local draft as a complete blank-line-delimited component.
fn retain_text(target: &mut String, text: &str) {
    if text.is_empty()
        || target == text
        || target.starts_with(&format!("{text}\n\n"))
        || target.ends_with(&format!("\n\n{text}"))
        || target.contains(&format!("\n\n{text}\n\n"))
    {
        return;
    }
    if !target.is_empty() {
        target.push_str("\n\n");
    }
    target.push_str(text);
}
