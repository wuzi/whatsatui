use crate::app::model::*;
use unicode_segmentation::UnicodeSegmentation;
use whatsapp_rust::prelude::MessageExt;
use whatsapp_rust::wacore_binary::JidExt;
use whatsapp_rust::{prelude::wa, types::events::InboundMessage};

pub(super) fn jid(value: &whatsapp_rust::Jid) -> String {
    value.with_device(0).to_string()
}
pub(super) fn identity_aliases(messages: &[InboundMessage]) -> Vec<(ParticipantId, ParticipantId)> {
    let mut pairs = std::collections::BTreeSet::new();
    for message in messages {
        let source = &message.info.source;
        for (first, second) in [
            (&source.sender, source.sender_alt.as_ref()),
            (&source.chat, source.recipient_alt.as_ref()),
        ] {
            if let Some(second) = second {
                let first = jid(first);
                let second = jid(second);
                if first.ends_with("@lid") && second.ends_with("@s.whatsapp.net") {
                    pairs.insert((first.into(), second.into()));
                } else if second.ends_with("@lid") && first.ends_with("@s.whatsapp.net") {
                    pairs.insert((second.into(), first.into()));
                }
            }
        }
    }
    pairs.into_iter().collect()
}
pub(super) fn message_batch(
    account: AccountId,
    source: MessageSource,
    messages: &[InboundMessage],
) -> MessageBatch {
    let changes = messages
        .iter()
        .filter(|m| {
            !m.info.source.chat.is_status_broadcast() && !m.info.source.chat.is_newsletter()
        })
        .map(|m| {
            let info = &m.info;
            let from_me = info.source.is_from_me;
            let sender = if from_me {
                account.0.clone()
            } else {
                jid(info
                    .source
                    .sender_alt
                    .as_ref()
                    .filter(|j| j.to_string().ends_with("@s.whatsapp.net"))
                    .unwrap_or(&info.source.sender))
            };
            let chat = if !info.source.is_group && !from_me {
                sender.clone()
            } else {
                jid(&info.source.chat)
            };
            let key = MessageKey {
                account: account.clone(),
                chat: chat.into(),
                sender: sender.into(),
                id: info.id.to_string().into(),
                from_me,
            };
            normalize(
                key,
                &m.message,
                info.timestamp.timestamp_millis(),
                info.ephemeral_expiration.filter(|s| *s > 0).map(|seconds| {
                    info.timestamp
                        .timestamp_millis()
                        .saturating_add(i64::from(seconds) * 1000)
                }),
            )
        })
        .collect();
    MessageBatch {
        account,
        source,
        changes,
    }
}
fn context(message: &wa::Message) -> Option<&wa::ContextInfo> {
    message
        .extended_text_message
        .as_option()
        .and_then(|m| m.context_info.as_option())
        .or_else(|| {
            message
                .image_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            message
                .video_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            message
                .audio_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            message
                .document_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
        .or_else(|| {
            message
                .sticker_message
                .as_option()
                .and_then(|m| m.context_info.as_option())
        })
}
pub(super) fn normalize(
    key: MessageKey,
    payload: &wa::Message,
    at: i64,
    expiry: Option<i64>,
) -> MessageChange {
    let message = payload.get_base_message();
    if let Some(protocol) = message.protocol_message.as_option() {
        if let Some(target) = protocol.key.as_option() {
            let from_me = target.from_me.unwrap_or(false);
            let target_key = MessageKey {
                account: key.account.clone(),
                chat: key.chat.clone(),
                sender: if from_me {
                    ParticipantId(key.account.0.clone())
                } else {
                    target
                        .participant
                        .clone()
                        .map(ParticipantId)
                        .unwrap_or_else(|| key.sender.clone())
                },
                id: target.id.clone().unwrap_or_default().into(),
                from_me,
            };
            if protocol.r#type == Some(wa::message::protocol_message::Type::Revoke.into()) {
                return MessageChange::Delete { key: target_key };
            }
            if protocol.r#type == Some(wa::message::protocol_message::Type::MessageEdit.into()) {
                if let Some(text) = protocol
                    .edited_message
                    .as_option()
                    .and_then(|m| m.text_content())
                {
                    return MessageChange::Edit {
                        key: target_key,
                        text: text.into(),
                        edited_at_ms: protocol.timestamp_ms.unwrap_or(at),
                    };
                }
            }
        }
    }
    let ctx = context(message);
    let quote = ctx.and_then(|ctx| {
        ctx.stanza_id.as_ref().map(|id| {
            let sender = ctx
                .participant
                .clone()
                .unwrap_or_else(|| key.sender.0.clone());
            let text = ctx
                .quoted_message
                .as_option()
                .and_then(|m| m.text_content());
            Quote {
                key: MessageKey {
                    account: key.account.clone(),
                    chat: ctx
                        .remote_jid
                        .clone()
                        .map(ChatId)
                        .unwrap_or_else(|| key.chat.clone()),
                    sender: sender.clone().into(),
                    id: id.as_str().into(),
                    from_me: sender == key.account.0,
                },
                preview: text.unwrap_or("").graphemes(true).take(160).collect(),
                availability: if text.is_some() {
                    QuoteAvailability::Available
                } else if ctx.quoted_message.is_set() {
                    QuoteAvailability::Unsupported
                } else {
                    QuoteAvailability::Missing
                },
            }
        })
    });
    let body = if let Some(text) = message.text_content() {
        MessageBody::Text(text.into())
    } else {
        let kind = if message.image_message.is_set() {
            "image"
        } else if message.video_message.is_set() {
            "video"
        } else if message.audio_message.is_set() {
            "audio"
        } else if message.document_message.is_set() {
            "document"
        } else if message.sticker_message.is_set() {
            "sticker"
        } else if message.reaction_message.is_set() {
            "reaction"
        } else {
            "unsupported message"
        };
        MessageBody::Unsupported {
            kind: kind.into(),
            caption: message.get_caption().map(str::to_owned),
        }
    };
    let expires_at_ms = expiry.or_else(|| {
        ctx.and_then(|c| c.expiration)
            .filter(|n| *n > 0)
            .map(|n| at.saturating_add(i64::from(n) * 1000))
    });
    let send_state = key.from_me.then_some(SendState::Sent);
    MessageChange::Upsert(MessageRecord {
        key,
        body,
        quote,
        created_at_ms: at,
        edited_at_ms: None,
        expires_at_ms,
        send_state,
    })
}
pub(super) fn history_message(
    account: &AccountId,
    chat: &ChatId,
    web: &wa::WebMessageInfo,
) -> Option<MessageChange> {
    let k = web.key.as_option()?;
    let from_me = k.from_me.unwrap_or(false);
    let sender = if from_me {
        account.0.clone()
    } else {
        k.participant
            .clone()
            .or(web.participant.clone())
            .unwrap_or_else(|| chat.0.clone())
    };
    let key = MessageKey {
        account: account.clone(),
        chat: chat.clone(),
        sender: sender.into(),
        id: k.id.clone()?.into(),
        from_me,
    };
    let at = i64::try_from(web.message_timestamp?)
        .ok()?
        .saturating_mul(1000);
    let expiry = web.ephemeral_duration.filter(|s| *s > 0).map(|s| {
        web.ephemeral_start_timestamp
            .and_then(|n| i64::try_from(n).ok())
            .unwrap_or(at / 1000)
            .saturating_add(i64::from(s))
            .saturating_mul(1000)
    });
    Some(normalize(key, web.message.as_option()?, at, expiry))
}
#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::sync::Arc;
    use whatsapp_rust::prelude::{MessageBuilderExt, MessageField, MessageInfo};
    pub fn fixture() -> InboundMessage {
        let mut info = MessageInfo::default();
        info.id = "incoming".into();
        info.source.chat = "120363000000001@g.us".parse().unwrap();
        info.source.sender = "551100000001@s.whatsapp.net".parse().unwrap();
        info.source.is_group = true;
        info.timestamp = chrono::DateTime::from_timestamp(1_790_640_000, 0).unwrap();
        info.ephemeral_expiration = Some(60);
        let context = wa::ContextInfo {
            stanza_id: Some("quoted".into()),
            participant: Some("551100000002@s.whatsapp.net".into()),
            quoted_message: MessageField::some(wa::Message::text("previous")),
            ..Default::default()
        };
        let message = wa::Message {
            image_message: MessageField::some(wa::message::ImageMessage {
                caption: Some("picture caption".into()),
                context_info: MessageField::some(context),
                ..Default::default()
            }),
            ..Default::default()
        };
        InboundMessage::builder()
            .message(Arc::new(message))
            .info(Arc::new(info))
            .build()
    }
    #[test]
    fn alternate_identifiers_only_map_authoritative_person_pairs() {
        let mut inbound = fixture();
        let info = Arc::make_mut(&mut inbound.info);
        info.source.sender = "123@lid".parse().unwrap();
        info.source.sender_alt = Some("551100000001@s.whatsapp.net".parse().unwrap());
        assert_eq!(
            identity_aliases(&[inbound]),
            vec![("123@lid".into(), "551100000001@s.whatsapp.net".into())]
        );
        assert!(identity_aliases(&[fixture()]).is_empty());
    }
    #[test]
    fn normalizes_group_quote_and_media_caption() {
        let batch = message_batch(
            "self@s.whatsapp.net".into(),
            MessageSource::Live,
            &[fixture()],
        );
        let MessageChange::Upsert(m) = &batch.changes[0] else {
            panic!("missing message")
        };
        assert_eq!(
            m.body,
            MessageBody::Unsupported {
                kind: "image".into(),
                caption: Some("picture caption".into())
            }
        );
        assert_eq!(m.key.sender.0, "551100000001@s.whatsapp.net");
        assert_eq!(
            m.quote.as_ref().unwrap().key.sender.0,
            "551100000002@s.whatsapp.net"
        );
        assert_eq!(m.quote.as_ref().unwrap().preview, "previous");
        assert_eq!(m.expires_at_ms, Some(1_790_640_060_000));
    }
}
