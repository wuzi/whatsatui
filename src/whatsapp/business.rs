//! Readable business announcements projected onto the existing text/media UI.
//! Callback IDs and native-flow payloads are never treated as message text.
use crate::{
    app::model::MessageBody,
    media::{Attachment, AttachmentKind},
    message_actions::valid_web_link,
};
use wa::message::{interactive_message as im, template_message as tm};
use whatsapp_rust::prelude::{MessageExt, MessageField as F, wa};

const MAX_TEXT: usize = 32 * 1024;
const MAX_ITEMS: usize = 20;
const MAX_JSON: usize = 64 * 1024;

fn template(message: &wa::Message) -> Option<&wa::message::TemplateMessage> {
    message.template_message.as_option().or_else(|| {
        message
            .highly_structured_message
            .as_option()?
            .hydrated_hsm
            .as_option()
    })
}

pub(super) fn context(message: &wa::Message) -> Option<&wa::ContextInfo> {
    if let Some(template) = template(message) {
        return template
            .context_info
            .as_option()
            .or_else(|| match &template.format {
                Some(tm::Format::InteractiveMessageTemplate(value)) => {
                    value.context_info.as_option()
                }
                _ => None,
            });
    }
    message
        .interactive_message
        .as_option()
        .and_then(|m| m.context_info.as_option())
        .or_else(|| {
            message
                .buttons_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            message
                .list_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
}

pub(super) fn body(payload: &wa::Message) -> Option<MessageBody> {
    let content = content(payload)?;
    Some(if let Some(mut attachment) = content.media {
        attachment.caption = (!content.text.is_empty()).then_some(content.text);
        MessageBody::Media(Box::new(attachment))
    } else if !content.text.is_empty() {
        MessageBody::Text(content.text)
    } else {
        MessageBody::Unsupported {
            kind: "business message".into(),
            caption: None,
        }
    })
}

pub(super) fn quoted_summary(payload: &wa::Message) -> Option<(String, Option<AttachmentKind>)> {
    if payload.is_view_once() {
        return None;
    }
    let content = content(payload)?;
    if let Some(kind) = content.media_kind {
        Some((
            format!("[{}] {}", kind.label(), content.text)
                .trim_end()
                .into(),
            Some(kind),
        ))
    } else {
        (!content.text.is_empty()).then_some((content.text, None))
    }
}

fn content(payload: &wa::Message) -> Option<Content> {
    let message = payload.get_base_message();
    let mut content = Content {
        text: String::new(),
        full: false,
        media: None,
        media_kind: None,
        allow_media: !payload.is_view_once(),
    };
    if let Some(template) = template(message) {
        content.template(template);
    } else if let Some(interactive) = message.interactive_message.as_option() {
        content.interactive(interactive);
    } else if let Some(buttons) = message.buttons_message.as_option() {
        content.buttons(buttons);
    } else if let Some(list) = message.list_message.as_option() {
        content.list(list);
    } else if message.highly_structured_message.is_set() {
        // An unhydrated template only carries server-side localization IDs.
        // Its parameters are not a substitute for the missing announcement.
    } else {
        return None;
    }
    Some(content)
}

struct Content {
    text: String,
    full: bool,
    media: Option<Attachment>,
    // Quotes can describe media even when their abbreviated payload has no keys.
    media_kind: Option<AttachmentKind>,
    allow_media: bool,
}
impl Content {
    fn line(&mut self, parts: &[&str]) {
        if self.full || parts.iter().all(|p| p.trim().is_empty()) {
            return;
        }
        if self
            .text
            .len()
            .saturating_add(parts.iter().map(|p| p.len()).sum::<usize>())
            + 1
            > MAX_TEXT - 64
        {
            self.text.push_str("\n[More content available in WhatsApp]");
            self.full = true;
            return;
        }
        if !self.text.is_empty() {
            self.text.push('\n');
        }
        for part in parts {
            self.text.push_str(part);
        }
    }
    fn push(&mut self, value: Option<&str>) {
        if let Some(value) = value {
            self.line(&[value.trim()]);
        }
    }
    fn button(&mut self, label: Option<&str>, target: Option<&str>) {
        let label = label.filter(|s| !s.trim().is_empty());
        if label.is_none() && target.is_none() {
            return;
        }
        self.line(&[
            "• ",
            label.unwrap_or("Link"),
            if target.is_some() { ": " } else { "" },
            target.unwrap_or_default(),
        ]);
    }
    fn media(&mut self, payload: wa::Message, kind: AttachmentKind) {
        if self.allow_media && self.media_kind.is_none() && !payload.is_view_once() {
            self.push(payload.get_caption());
            self.media_kind = Some(kind);
            self.media = super::media::attachment(&payload);
        }
    }
    fn image(&mut self, image: &wa::message::ImageMessage) {
        self.media(
            wa::Message {
                image_message: F::some(image.clone()),
                ..Default::default()
            },
            AttachmentKind::Image,
        );
    }
    fn video(&mut self, video: &wa::message::VideoMessage) {
        self.media(
            wa::Message {
                video_message: F::some(video.clone()),
                ..Default::default()
            },
            if video.gif_playback == Some(true) {
                AttachmentKind::Gif
            } else {
                AttachmentKind::Video
            },
        );
    }
    fn document(&mut self, document: &wa::message::DocumentMessage) {
        self.media(
            wa::Message {
                document_message: F::some(document.clone()),
                ..Default::default()
            },
            AttachmentKind::Document,
        );
    }
    fn template(&mut self, template: &wa::message::TemplateMessage) {
        if let Some(hydrated) = template.hydrated_template.as_option() {
            return self.hydrated(hydrated);
        }
        match &template.format {
            Some(tm::Format::HydratedFourRowTemplate(value)) => self.hydrated(value),
            Some(tm::Format::InteractiveMessageTemplate(value)) => self.interactive(value),
            _ => {}
        }
    }
    fn hydrated(&mut self, value: &tm::HydratedFourRowTemplate) {
        use tm::hydrated_four_row_template::Title;
        match &value.title {
            Some(Title::HydratedTitleText(text)) => self.push(Some(text)),
            Some(Title::ImageMessage(image)) => self.image(image),
            Some(Title::VideoMessage(video)) => self.video(video),
            Some(Title::DocumentMessage(document)) => self.document(document),
            _ => {}
        }
        self.push(value.hydrated_content_text.as_deref());
        self.push(value.hydrated_footer_text.as_deref());
        for button in value.hydrated_buttons.iter().take(MAX_ITEMS) {
            use wa::hydrated_template_button::HydratedButton as B;
            match &button.hydrated_button {
                Some(B::UrlButton(b)) => self.button(
                    b.display_text.as_deref(),
                    b.url.as_deref().filter(|s| valid_web_link(s)),
                ),
                Some(B::QuickReplyButton(b)) => self.button(b.display_text.as_deref(), None),
                Some(B::CallButton(b)) => {
                    self.button(b.display_text.as_deref(), b.phone_number.as_deref())
                }
                _ => {}
            }
        }
    }
    fn interactive(&mut self, value: &wa::message::InteractiveMessage) {
        if let Some(header) = value.header.as_option() {
            self.push(header.title.as_deref());
            self.push(header.subtitle.as_deref());
            match &header.media {
                Some(im::header::Media::ImageMessage(image)) => self.image(image),
                Some(im::header::Media::VideoMessage(video)) => self.video(video),
                Some(im::header::Media::DocumentMessage(document)) => self.document(document),
                _ => {}
            }
        }
        self.push(value.body.as_option().and_then(|b| b.text.as_deref()));
        self.push(value.footer.as_option().and_then(|f| f.text.as_deref()));
        if let Some(im::InteractiveMessage::NativeFlowMessage(flow)) = &value.interactive_message {
            for button in flow.buttons.iter().take(MAX_ITEMS) {
                self.native_button(
                    button.name.as_deref(),
                    button.button_params_json.as_deref(),
                    None,
                );
            }
        }
    }
    fn buttons(&mut self, value: &wa::message::ButtonsMessage) {
        use wa::message::buttons_message::Header;
        match &value.header {
            Some(Header::Text(text)) => self.push(Some(text)),
            Some(Header::ImageMessage(image)) => self.image(image),
            Some(Header::VideoMessage(video)) => self.video(video),
            Some(Header::DocumentMessage(document)) => self.document(document),
            _ => {}
        }
        self.push(value.content_text.as_deref());
        self.push(value.footer_text.as_deref());
        for button in value.buttons.iter().take(MAX_ITEMS) {
            let label = button
                .button_text
                .as_option()
                .and_then(|b| b.display_text.as_deref());
            if let Some(flow) = button.native_flow_info.as_option() {
                self.native_button(flow.name.as_deref(), flow.params_json.as_deref(), label);
            } else {
                self.button(label, None);
            }
        }
    }
    fn list(&mut self, value: &wa::message::ListMessage) {
        self.push(value.title.as_deref());
        self.push(value.description.as_deref());
        self.push(value.footer_text.as_deref());
        self.push(value.button_text.as_deref());
        for section in value.sections.iter().take(MAX_ITEMS) {
            self.push(section.title.as_deref());
            for row in section.rows.iter().take(MAX_ITEMS) {
                self.button(row.title.as_deref(), None);
                self.push(row.description.as_deref());
            }
        }
    }
    fn native_button(
        &mut self,
        name: Option<&str>,
        json: Option<&str>,
        fallback_label: Option<&str>,
    ) {
        let Some(value) = json
            .filter(|s| s.len() <= MAX_JSON)
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        else {
            self.button(fallback_label, None);
            return;
        };
        let label = value["display_text"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| value["title"].as_str().filter(|s| !s.trim().is_empty()))
            .or(fallback_label);
        let target = match name {
            Some("cta_url") => value["url"].as_str().filter(|s| valid_web_link(s)),
            Some("cta_call") => value["phone_number"].as_str(),
            Some("cta_copy") => value["copy_code"].as_str(),
            _ => None,
        };
        self.button(label, target);
        if name == Some("single_select") {
            for section in value["sections"]
                .as_array()
                .into_iter()
                .flatten()
                .take(MAX_ITEMS)
            {
                self.push(section["title"].as_str());
                for row in section["rows"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(MAX_ITEMS)
                {
                    self.push(row["header"].as_str());
                    self.button(row["title"].as_str(), None);
                    self.push(row["description"].as_str());
                }
            }
        }
    }
}
