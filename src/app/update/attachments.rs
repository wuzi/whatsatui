use super::*;

impl App {
    pub(super) fn open_attachment(&mut self) {
        self.clipboard_request = None;
        if self.view.loading || self.view.chat.is_none() || self.view.account.is_none() {
            return;
        }
        self.view.overlay = Some(Overlay::Attachment {
            editor: Editor::default(),
            selected: 0,
            importing: None,
            error: None,
        });
    }
    pub(super) fn import_attachment(&mut self, effects: &mut Vec<Effect>) {
        let Some(Overlay::Attachment {
            editor,
            selected,
            importing: None,
            ..
        }) = &self.view.overlay
        else {
            return;
        };
        let path = editor.text().trim().to_owned();
        if path.is_empty() {
            if let Some(chat) = &self.view.chat
                && self.view.draft.restore(*selected, chat)
            {
                self.editor = Editor::new(self.view.draft.text.clone());
                self.view.overlay = None;
                self.view.notice = Some("Saved draft restored; review before sending".into());
                self.remember();
            }
            return;
        }
        let (Some(account), Some(chat)) = (self.view.account.clone(), self.view.chat.clone())
        else {
            return;
        };
        let request = self.request();
        if let Some(Overlay::Attachment {
            importing, error, ..
        }) = &mut self.view.overlay
        {
            *importing = Some(request);
            *error = None;
        }
        effects.push(Effect::ImportImage {
            request,
            account,
            chat,
            path,
        });
    }
    pub(super) fn image_imported(
        &mut self,
        request: RequestId,
        account: AccountId,
        chat: ChatId,
        result: Result<crate::media::outgoing::LocalImage, String>,
    ) {
        if self.quitting
            || self.view.account.as_ref() != Some(&account)
            || self.view.chat.as_ref() != Some(&chat)
        {
            return;
        }
        let Some(Overlay::Attachment {
            importing, error, ..
        }) = &mut self.view.overlay
        else {
            return;
        };
        if *importing != Some(request) {
            return;
        }
        *importing = None;
        match result {
            Ok(image) => {
                self.view.draft.attachment = Some(Box::new(image));
                self.view.overlay = None;
                self.view.notice = None;
                self.remember();
            }
            Err(reason) => *error = Some(reason),
        }
    }
}

impl App {
    pub(super) fn paste_clipboard(&mut self, effects: &mut Vec<Effect>) {
        if self.view.loading || self.clipboard_request.is_some() {
            return;
        }
        let (Some(account), Some(chat)) = (self.view.account.clone(), self.view.chat.clone())
        else {
            return;
        };
        let sticker = matches!(self.view.overlay, Some(Overlay::Stickers(_)));
        if let Some(Overlay::Stickers(picker)) = &self.view.overlay
            && (picker.loading.is_some() || picker.sending.is_some())
        {
            return;
        }
        let request = self.request();
        self.clipboard_request = Some((request, account.clone(), chat.clone(), sticker));
        self.view.notice = Some("Reading clipboard…".into());
        effects.push(Effect::PasteClipboard {
            request,
            account,
            chat,
            sticker,
        });
    }
    pub(super) fn clipboard_read(
        &mut self,
        request: RequestId,
        account: AccountId,
        chat: ChatId,
        result: Result<crate::desktop::clipboard::Paste, String>,
    ) {
        let Some((pending, expected_account, expected_chat, sticker)) =
            self.clipboard_request.clone()
        else {
            return;
        };
        if self.quitting
            || pending != request
            || expected_account != account
            || expected_chat != chat
        {
            return;
        }
        self.clipboard_request = None;
        if self.view.account.as_ref() != Some(&account) || self.view.chat.as_ref() != Some(&chat) {
            return;
        }
        if sticker {
            self.view.notice = None;
            let Some(Overlay::Stickers(picker)) = &mut self.view.overlay else {
                return;
            };
            match result {
                Ok(crate::desktop::clipboard::Paste::Image(image)) if image.sticker.is_some() => {
                    picker.items.insert(0, StickerChoice::Local(image));
                    picker.items.truncate(60);
                    picker.selected = 0;
                    picker.error = None;
                    self.view.notice = Some("Sticker ready · Enter sends · Esc cancels".into());
                }
                Ok(_) => picker.error = Some("Copy an image to create a sticker".into()),
                Err(reason) => picker.error = Some(reason),
            }
            return;
        }
        match result {
            Ok(crate::desktop::clipboard::Paste::Image(image)) => {
                self.view.draft.attachment = Some(image);
                self.remember();
                self.view.notice =
                    Some("Image attached · type a caption, then Enter to send".into());
            }
            Ok(crate::desktop::clipboard::Paste::Text(text)) => {
                self.edit_current(EditAction::Insert(text));
                self.view.notice = None;
            }
            Err(reason) => self.view.notice = Some(reason),
        }
    }
}
