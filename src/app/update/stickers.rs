use super::*;

impl App {
    pub(super) fn open_stickers(&mut self, effects: &mut Vec<Effect>) {
        if self.view.loading {
            return;
        }
        let (Some(account), Some(chat)) = (self.view.account.clone(), self.view.chat.clone())
        else {
            return;
        };
        self.clipboard_request = None;
        let request = self.request();
        self.view.overlay = Some(Overlay::Stickers(Box::new(StickerPicker {
            items: vec![],
            selected: 0,
            loading: Some(request),
            reload: false,
            sending: None,
            error: None,
        })));
        effects.push(Effect::LoadStickers {
            request,
            account,
            chat,
        });
    }
    pub(super) fn stickers_loaded(
        &mut self,
        request: RequestId,
        account: AccountId,
        chat: ChatId,
        result: Result<Vec<MessageRecord>, String>,
        effects: &mut Vec<Effect>,
    ) {
        if self.quitting
            || self.view.account.as_ref() != Some(&account)
            || self.view.chat.as_ref() != Some(&chat)
        {
            return;
        }
        let Some(Overlay::Stickers(picker)) = &mut self.view.overlay else {
            return;
        };
        if picker.loading != Some(request) {
            return;
        }
        picker.loading = None;
        match result {
            Ok(items) => {
                let selected = picker
                    .items
                    .get(picker.selected)
                    .and_then(StickerChoice::content_id);
                picker
                    .items
                    .retain(|item| matches!(item, StickerChoice::Local(_)));
                picker.items.extend(
                    items
                        .into_iter()
                        .filter(|m| m.key.account == account)
                        .map(|m| StickerChoice::Recent(Box::new(m))),
                );
                picker.items.truncate(60);
                picker.selected = selected
                    .and_then(|s| {
                        picker
                            .items
                            .iter()
                            .position(|item| item.content_id().as_ref() == Some(&s))
                    })
                    .unwrap_or_else(|| picker.selected.min(picker.items.len().saturating_sub(1)));
                picker.error = None;
            }
            Err(reason) => {
                picker
                    .items
                    .retain(|item| matches!(item, StickerChoice::Local(_)));
                picker.selected = 0;
                picker.error = Some(reason);
            }
        }
        if picker.reload {
            picker.reload = false;
            self.refresh_stickers(effects);
        }
    }
    pub(super) fn refresh_stickers(&mut self, effects: &mut Vec<Effect>) {
        let Some(Overlay::Stickers(picker)) = &mut self.view.overlay else {
            return;
        };
        if picker.loading.is_some() {
            picker.reload = true;
            return;
        }
        let (Some(account), Some(chat)) = (self.view.account.clone(), self.view.chat.clone())
        else {
            return;
        };
        let request = self.request();
        if let Some(Overlay::Stickers(picker)) = &mut self.view.overlay {
            picker.loading = Some(request);
        }
        effects.push(Effect::LoadStickers {
            request,
            account,
            chat,
        });
    }
    pub(super) fn send_sticker(&mut self, effects: &mut Vec<Effect>) {
        let Some(Overlay::Stickers(picker)) = &self.view.overlay else {
            return;
        };
        if picker.loading.is_some() || picker.sending.is_some() || self.clipboard_request.is_some()
        {
            return;
        }
        let Some(choice) = picker.items.get(picker.selected).cloned() else {
            return;
        };
        match choice {
            StickerChoice::Local(image) => self.prepare_sticker(*image, effects),
            StickerChoice::Recent(message) => {
                let (Some(account), Some(chat)) =
                    (self.view.account.clone(), self.view.chat.clone())
                else {
                    return;
                };
                if message.key.account != account {
                    return;
                }
                let request = self.request();
                if let Some(Overlay::Stickers(picker)) = &mut self.view.overlay {
                    picker.sending = Some(request);
                    picker.error = None;
                }
                effects.push(Effect::ImportSticker {
                    request,
                    account,
                    chat,
                    message,
                });
            }
        }
    }
    pub(super) fn sticker_imported(
        &mut self,
        request: RequestId,
        account: AccountId,
        chat: ChatId,
        result: Result<crate::media::outgoing::LocalImage, String>,
        effects: &mut Vec<Effect>,
    ) {
        if self.quitting
            || self.view.account.as_ref() != Some(&account)
            || self.view.chat.as_ref() != Some(&chat)
        {
            return;
        }
        let Some(Overlay::Stickers(picker)) = &mut self.view.overlay else {
            return;
        };
        if picker.sending != Some(request) {
            return;
        }
        picker.sending = None;
        match result {
            Ok(image) => {
                let original = picker
                    .items
                    .get(picker.selected)
                    .and_then(StickerChoice::content_id);
                if original.as_ref() != Some(&image.id) {
                    picker
                        .items
                        .insert(0, StickerChoice::Local(Box::new(image)));
                    picker.items.truncate(60);
                    picker.selected = 0;
                    self.view.notice =
                        Some("Sticker optimized; review the preview, then send.".into());
                } else {
                    self.prepare_sticker(image, effects);
                }
            }
            Err(reason) => picker.error = Some(reason),
        }
    }
    fn prepare_sticker(
        &mut self,
        image: crate::media::outgoing::LocalImage,
        effects: &mut Vec<Effect>,
    ) {
        if image.sticker.is_none() {
            return;
        }
        let Some(chat) = self.view.chat.clone() else {
            return;
        };
        let before = effects.len();
        self.prepare(
            chat,
            Draft {
                attachment: Some(Box::new(image)),
                ..Default::default()
            },
            true,
            effects,
        );
        if let Some(Effect::Prepare { request, .. }) = effects.get(before) {
            if let Some(Overlay::Stickers(picker)) = &mut self.view.overlay {
                picker.sending = Some(*request);
                picker.error = None;
            }
            self.view.notice = None;
        }
    }

    /// Keep a pasted selection until the outgoing attempt is durably stored.
    pub(super) fn sticker_staged(&mut self, request: RequestId, result: &Result<(), String>) {
        let Some(Overlay::Stickers(picker)) = &mut self.view.overlay else {
            return;
        };
        if picker.sending != Some(request) {
            return;
        }
        picker.sending = None;
        match result {
            Ok(()) => self.view.overlay = None,
            Err(reason) => picker.error = Some(reason.clone()),
        }
    }
}
