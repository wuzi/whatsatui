use crate::media::{Attachment, AttachmentKind};
use whatsapp_rust::prelude::{MessageExt, wa};

pub(super) fn attachment(payload: &wa::Message) -> Option<Attachment> {
    if payload.is_view_once() {
        return None;
    }
    let base = payload.get_base_message();
    let attachment = if let Some(image) = base.image_message.as_option() {
        Attachment {
            audio: None,
            kind: AttachmentKind::Image,
            filename: None,
            mime: image.mimetype.clone(),
            caption: image.caption.clone(),
            size: image.file_length?,
            direct_path: image.direct_path.clone()?,
            media_key: image.media_key.as_deref()?.try_into().ok()?,
            sha256: image.file_sha256.as_deref()?.try_into().ok()?,
            encrypted_sha256: image.file_enc_sha256.as_deref()?.try_into().ok()?,
        }
    } else if let Some(sticker) = base.sticker_message.as_option() {
        Attachment {
            audio: None,
            kind: AttachmentKind::Sticker,
            filename: None,
            mime: sticker.mimetype.clone(),
            caption: None,
            size: sticker.file_length?,
            direct_path: sticker.direct_path.clone()?,
            media_key: sticker.media_key.as_deref()?.try_into().ok()?,
            sha256: sticker.file_sha256.as_deref()?.try_into().ok()?,
            encrypted_sha256: sticker.file_enc_sha256.as_deref()?.try_into().ok()?,
        }
    } else if let Some(audio) = base.audio_message.as_option() {
        Attachment {
            kind: AttachmentKind::Audio,
            audio: Some(crate::media::AudioMetadata {
                seconds: audio.seconds,
                voice: audio.ptt.unwrap_or(false),
            }),
            filename: None,
            mime: audio.mimetype.clone(),
            caption: None,
            size: audio.file_length?,
            direct_path: audio.direct_path.clone()?,
            media_key: audio.media_key.as_deref()?.try_into().ok()?,
            sha256: audio.file_sha256.as_deref()?.try_into().ok()?,
            encrypted_sha256: audio.file_enc_sha256.as_deref()?.try_into().ok()?,
        }
    } else {
        let document = base.document_message.as_option()?;
        Attachment {
            audio: None,
            kind: AttachmentKind::Document,
            filename: document.file_name.clone(),
            mime: document.mimetype.clone(),
            caption: document.caption.clone(),
            size: document.file_length?,
            direct_path: document.direct_path.clone()?,
            media_key: document.media_key.as_deref()?.try_into().ok()?,
            sha256: document.file_sha256.as_deref()?.try_into().ok()?,
            encrypted_sha256: document.file_enc_sha256.as_deref()?.try_into().ok()?,
        }
    };
    attachment.validate().ok()?;
    Some(attachment)
}

#[cfg(test)]
mod tests {
    use super::*;
    use whatsapp_rust::prelude::MessageField;
    #[test]
    fn voice_and_audio_payloads_keep_metadata_in_live_and_history_but_not_view_once() {
        use crate::app::model::*;
        for voice in [true, false] {
            let payload = wa::Message {
                audio_message: MessageField::some(wa::message::AudioMessage {
                    direct_path: Some("/v/voice".into()),
                    media_key: Some(vec![1; 32]),
                    file_sha256: Some(vec![2; 32]),
                    file_enc_sha256: Some(vec![3; 32]),
                    file_length: Some(200),
                    mimetype: Some("audio/ogg; codecs=opus".into()),
                    seconds: Some(42),
                    ptt: Some(voice),
                    ..Default::default()
                }),
                ..Default::default()
            };
            let a = attachment(&payload).expect("audio reference");
            assert_eq!(a.kind, AttachmentKind::Audio);
            assert_eq!(a.audio.as_ref().unwrap().seconds, Some(42));
            assert_eq!(a.audio.as_ref().unwrap().voice, voice);
            let key = MessageKey {
                account: "me".into(),
                chat: "peer".into(),
                sender: "peer".into(),
                id: "voice".into(),
                from_me: false,
            };
            assert!(matches!(
                super::super::normalize::normalize(key, &payload, 100, None),
                MessageChange::Upsert(MessageRecord {
                    body: MessageBody::Media(_),
                    ..
                })
            ));
            let web = wa::WebMessageInfo {
                key: MessageField::some(wa::MessageKey {
                    id: Some("voice".into()),
                    ..Default::default()
                }),
                message: MessageField::some(payload.clone()),
                message_timestamp: Some(1),
                ..Default::default()
            };
            assert!(matches!(
                &super::super::normalize::history_changes(&"me".into(), &"peer".into(), &web)[0],
                MessageChange::Upsert(MessageRecord {
                    body: MessageBody::Media(_),
                    ..
                })
            ));
            let once = wa::Message {
                view_once_message_v2: MessageField::some(wa::message::FutureProofMessage {
                    message: MessageField::some(payload),
                }),
                ..Default::default()
            };
            assert!(attachment(&once).is_none());
        }
    }
    #[test]
    fn complete_sticker_is_downloadable_but_view_once_is_not() {
        let payload = wa::Message {
            sticker_message: MessageField::some(wa::message::StickerMessage {
                direct_path: Some("/v/sticker".into()),
                media_key: Some(vec![1; 32]),
                file_sha256: Some(vec![2; 32]),
                file_enc_sha256: Some(vec![3; 32]),
                file_length: Some(128),
                mimetype: Some("image/webp".into()),
                is_animated: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        };
        let a = attachment(&payload).expect("complete sticker");
        assert_eq!(a.kind.label(), "sticker");
        assert_eq!(a.extension(), Some("webp"));
        let once = wa::Message {
            view_once_message_v2: MessageField::some(wa::message::FutureProofMessage {
                message: MessageField::some(payload),
            }),
            ..Default::default()
        };
        assert!(attachment(&once).is_none());
    }
}
