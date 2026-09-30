use super::*;

impl App {
    pub(super) fn open_attachment(&mut self) {
        if self.view.loading || self.view.chat.is_none() || self.view.account.is_none() {
            return;
        }
        self.view.overlay = Some(Overlay::Attachment {
            editor: Editor::default(),
            importing: None,
            error: None,
        });
    }
    pub(super) fn import_attachment(&mut self, effects: &mut Vec<Effect>) {
        let Some(Overlay::Attachment {
            editor,
            importing: None,
            ..
        }) = &self.view.overlay
        else {
            return;
        };
        let path = editor.text().trim().to_owned();
        if path.is_empty() {
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
                self.view.draft.attachment = Some(image);
                self.view.overlay = None;
                self.view.notice = None;
                self.remember();
            }
            Err(reason) => *error = Some(reason),
        }
    }
}
