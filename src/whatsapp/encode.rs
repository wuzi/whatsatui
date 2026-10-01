use super::BackendError;
use crate::app::model::*;
use whatsapp_rust::prelude::*;

pub struct EncodedText {
    pub to: Jid,
    pub message: wa::Message,
    pub options: SendOptions,
}
pub fn encode_text(outbound: &OutboundText) -> Result<EncodedText, BackendError> {
    if outbound.draft.attachment.is_some() {
        return Err(BackendError::InvalidIdentity);
    }
    encode_base(outbound)
}
fn encode_base(outbound: &OutboundText) -> Result<EncodedText, BackendError> {
    let to: Jid = outbound
        .key
        .chat
        .0
        .parse()
        .map_err(|_| BackendError::InvalidIdentity)?;
    let message = if let Some(quote) = &outbound.draft.reply {
        if quote.key.account != outbound.key.account || quote.key.chat != outbound.key.chat {
            return Err(BackendError::InvalidIdentity);
        }
        let sender: Jid = quote
            .key
            .sender
            .0
            .parse()
            .map_err(|_| BackendError::InvalidIdentity)?;
        let preview = if quote.availability == QuoteAvailability::Available {
            quote.preview.as_str()
        } else {
            "[message unavailable]"
        };
        let quoted = quoted_body(quote, preview);
        let context = whatsapp_rust::wacore::proto_helpers::build_quote_context_with_info(
            quote.key.id.0.clone(),
            &sender,
            &to,
            &to,
            &quoted,
        );
        wa::Message::text_with_context(outbound.draft.text.clone(), context)
    } else {
        wa::Message::text(outbound.draft.text.clone())
    };
    Ok(EncodedText {
        to,
        message,
        options: SendOptions::default().with_message_id(outbound.key.id.0.clone()),
    })
}
pub struct UploadedImage {
    pub attachment: crate::media::Attachment,
    pub url: String,
    pub media_key_timestamp: i64,
}
impl From<whatsapp_rust::upload::UploadResponse> for UploadedImage {
    fn from(r: whatsapp_rust::upload::UploadResponse) -> Self {
        Self {
            url: r.url,
            media_key_timestamp: r.media_key_timestamp,
            attachment: crate::media::Attachment {
                audio: None,
                kind: crate::media::AttachmentKind::Image,
                filename: None,
                caption: None,
                mime: Some("image/jpeg".into()),
                size: r.file_length,
                direct_path: r.direct_path,
                media_key: r.media_key,
                sha256: r.file_sha256,
                encrypted_sha256: r.file_enc_sha256,
            },
        }
    }
}
pub fn encode_image(
    outbound: &OutboundText,
    uploaded: UploadedImage,
    thumbnail: Vec<u8>,
) -> Result<EncodedText, BackendError> {
    let local = outbound
        .draft
        .attachment
        .as_ref()
        .ok_or(BackendError::InvalidIdentity)?;
    if local.sticker.is_some() {
        return Err(BackendError::InvalidIdentity);
    }
    let mut encoded = encode_base(outbound)?;
    let context_info = encoded
        .message
        .extended_text_message
        .as_option_mut()
        .and_then(|m| m.context_info.take());
    let a = uploaded.attachment;
    encoded.message = wa::Message {
        image_message: MessageField::some(wa::message::ImageMessage {
            url: Some(uploaded.url),
            direct_path: Some(a.direct_path),
            mimetype: Some("image/jpeg".into()),
            caption: Some(outbound.draft.text.clone()),
            file_length: Some(a.size),
            width: Some(local.width),
            height: Some(local.height),
            media_key: Some(a.media_key.to_vec()),
            file_sha256: Some(a.sha256.to_vec()),
            file_enc_sha256: Some(a.encrypted_sha256.to_vec()),
            media_key_timestamp: Some(uploaded.media_key_timestamp),
            jpeg_thumbnail: Some(thumbnail),
            context_info: context_info.into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    Ok(encoded)
}
pub fn encode_sticker(
    outbound: &OutboundText,
    uploaded: UploadedImage,
) -> Result<EncodedText, BackendError> {
    let local = outbound
        .draft
        .attachment
        .as_ref()
        .ok_or(BackendError::InvalidIdentity)?;
    let info = local
        .sticker
        .as_ref()
        .ok_or(BackendError::InvalidIdentity)?;
    if !outbound.draft.text.is_empty() || local.width != 512 || local.height != 512 {
        return Err(BackendError::InvalidIdentity);
    }
    let mut encoded = encode_base(outbound)?;
    let context_info = encoded
        .message
        .extended_text_message
        .as_option_mut()
        .and_then(|m| m.context_info.take());
    let a = uploaded.attachment;
    encoded.message = wa::Message {
        sticker_message: MessageField::some(wa::message::StickerMessage {
            url: Some(uploaded.url),
            direct_path: Some(a.direct_path),
            mimetype: Some("image/webp".into()),
            file_length: Some(a.size),
            width: Some(local.width),
            height: Some(local.height),
            media_key: Some(a.media_key.to_vec()),
            file_sha256: Some(a.sha256.to_vec()),
            file_enc_sha256: Some(a.encrypted_sha256.to_vec()),
            media_key_timestamp: Some(uploaded.media_key_timestamp),
            is_animated: Some(info.animated),
            context_info: context_info.into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    Ok(encoded)
}
pub fn classify_send_error(error: &SendError) -> SendState {
    match error {
        SendError::NotLoggedIn | SendError::InvalidRequest(_) => SendState::Failed,
        _ => SendState::Unconfirmed,
    }
}
pub fn classify_transport_result<T>(result: &Result<T, SendError>) -> SendState {
    // Upstream success means the stanza was written; ServerAck is separate.
    match result {
        Ok(_) => SendState::Sending,
        Err(e) => classify_send_error(e),
    }
}

fn quoted_body(quote: &Quote, preview: &str) -> wa::Message {
    use crate::media::AttachmentKind;
    if quote.availability != QuoteAvailability::Available {
        return wa::Message::text(preview);
    }
    match quote.media_kind {
        Some(AttachmentKind::Video) => wa::Message {
            video_message: MessageField::some(wa::message::VideoMessage {
                caption: Some(preview.into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        Some(AttachmentKind::Audio) => wa::Message {
            audio_message: MessageField::some(wa::message::AudioMessage::default()),
            ..Default::default()
        },
        Some(AttachmentKind::Image) => wa::Message {
            image_message: MessageField::some(wa::message::ImageMessage {
                caption: Some(preview.into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        Some(AttachmentKind::Sticker) => wa::Message {
            sticker_message: MessageField::some(wa::message::StickerMessage::default()),
            ..Default::default()
        },
        Some(AttachmentKind::Document) => wa::Message {
            document_message: MessageField::some(wa::message::DocumentMessage {
                title: Some(preview.into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        None => wa::Message::text(preview),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_encoding_preserves_caption_quote_and_message_id() {
        let mut outbound = outgoing("120363000000001@g.us");
        outbound.draft.attachment = Some(Box::new(crate::media::outgoing::LocalImage {
            sticker: None,
            id: "0".repeat(64),
            filename: "photo.png".into(),
            size: 42,
            width: 300,
            height: 200,
        }));
        let uploaded = UploadedImage {
            url: "https://mmg.whatsapp.net/v/photo".into(),
            media_key_timestamp: 123,
            attachment: crate::media::Attachment {
                audio: None,
                kind: crate::media::AttachmentKind::Image,
                filename: None,
                caption: None,
                mime: Some("image/jpeg".into()),
                size: 42,
                direct_path: "/v/photo".into(),
                media_key: [1; 32],
                sha256: [2; 32],
                encrypted_sha256: [3; 32],
            },
        };
        assert!(
            encode_text(&outbound).is_err(),
            "an attached image must never fall back to text"
        );
        let encoded = encode_image(&outbound, uploaded, vec![4, 5]).unwrap();
        assert_eq!(encoded.options.message_id.as_deref(), Some("3EB0TEST0001"));
        assert!(encoded.message.conversation.is_none());
        let image = encoded.message.image_message.as_option().unwrap();
        assert_eq!(image.caption.as_deref(), Some("  hello\nworld  "));
        assert_eq!(image.mimetype.as_deref(), Some("image/jpeg"));
        assert_eq!(image.direct_path.as_deref(), Some("/v/photo"));
        assert_eq!(image.file_length, Some(42));
        assert_eq!(image.width, Some(300));
        assert_eq!(
            image.context_info.as_option().unwrap().stanza_id.as_deref(),
            Some("original")
        );
    }
    fn outgoing(chat: &str) -> OutboundText {
        let mut key = MessageKey {
            account: "self@s.whatsapp.net".into(),
            chat: chat.into(),
            sender: "self@s.whatsapp.net".into(),
            id: "3EB0TEST0001".into(),
            from_me: true,
        };
        let outgoing_key = key.clone();
        key.id = "original".into();
        key.sender = "551100000001@s.whatsapp.net".into();
        key.from_me = false;
        OutboundText {
            key: outgoing_key,
            draft: Draft {
                attachment: None,
                text: "  hello\nworld  ".into(),
                reply: Some(Quote {
                    media_kind: None,
                    key,
                    preview: "previous text".into(),
                    availability: QuoteAvailability::Available,
                }),
                revision: 1,
                ..Default::default()
            },
            created_at_ms: 123,
        }
    }
    #[test]
    fn preserves_outbound_id_and_quote() {
        for chat in ["551100000002@s.whatsapp.net", "120363000000001@g.us"] {
            let encoded = encode_text(&outgoing(chat)).unwrap();
            assert_eq!(encoded.options.message_id.as_deref(), Some("3EB0TEST0001"));
            assert_eq!(encoded.message.text_content(), Some("  hello\nworld  "));
            let ctx = encoded
                .message
                .extended_text_message
                .as_option()
                .unwrap()
                .context_info
                .as_option()
                .unwrap();
            assert_eq!(ctx.stanza_id.as_deref(), Some("original"));
            assert_eq!(
                ctx.participant.as_deref(),
                Some("551100000001@s.whatsapp.net")
            );
        }
    }
    #[test]
    fn only_definite_rejections_are_failed() {
        assert_eq!(
            classify_send_error(&SendError::NotLoggedIn),
            SendState::Failed
        );
        assert_eq!(
            classify_send_error(&SendError::InvalidRequest("bad".into())),
            SendState::Failed
        );
        assert_eq!(
            classify_send_error(&SendError::Internal(anyhow::anyhow!("opaque"))),
            SendState::Unconfirmed
        );
        let timeout = std::io::Error::new(std::io::ErrorKind::TimedOut, "transport");
        assert_eq!(
            classify_send_error(&SendError::Internal(timeout.into())),
            SendState::Unconfirmed
        );
    }
    #[test]
    fn socket_write_does_not_prove_server_acceptance() {
        assert_eq!(
            classify_transport_result(&Ok(String::from("locally-written-id"))),
            SendState::Sending
        );
    }
}
