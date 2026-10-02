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
        .filter(not_reaction_placeholder)
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
    if let Some(reaction) = message.reaction_message.as_option()
        && let Some(target) = reaction.key.as_option()
        && let Some(target_key) = reaction_target(&key, target)
        && let Some(emoji) = &reaction.text
    {
        return MessageChange::Reaction(Reaction {
            key: target_key,
            reactor: key.sender.clone(),
            emoji: emoji.clone(),
            at_ms: reaction.sender_timestamp_ms.unwrap_or(at),
            event_id: key.id.clone(),
        });
    }
    if let Some(protocol) = message.protocol_message.as_option()
        && let Some(target) = protocol.key.as_option()
    {
        // Protocol target ownership is relative to the envelope sender.
        let sender = if target.from_me.unwrap_or(false) {
            key.sender.clone()
        } else {
            target
                .participant
                .clone()
                .map(ParticipantId)
                .unwrap_or_else(|| key.sender.clone())
        };
        let target_key = MessageKey {
            account: key.account.clone(),
            chat: key.chat.clone(),
            from_me: sender.0 == key.account.0,
            sender,
            id: target.id.clone().unwrap_or_default().into(),
        };
        if protocol.r#type == Some(wa::message::protocol_message::Type::Revoke) {
            return MessageChange::Delete { key: target_key };
        }
        if protocol.r#type == Some(wa::message::protocol_message::Type::MessageEdit)
            && let Some(text) = protocol.edited_message.as_option().and_then(edited_text)
        {
            return MessageChange::Edit {
                key: target_key,
                text: text.into(),
                edited_at_ms: protocol.timestamp_ms.unwrap_or(at),
            };
        }
    }
    let ctx = context(message);
    let quote = ctx.and_then(|ctx| {
        ctx.stanza_id.as_ref().map(|id| {
            let sender = ctx
                .participant
                .clone()
                .unwrap_or_else(|| key.sender.0.clone());
            let summary = ctx.quoted_message.as_option().and_then(quoted_summary);
            Quote {
                media_kind: summary.as_ref().and_then(|(_, kind)| *kind),
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
                preview: summary
                    .as_ref()
                    .map(|(text, _)| text.as_str())
                    .unwrap_or("")
                    .graphemes(true)
                    .take(160)
                    .collect(),
                availability: if summary.is_some() {
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
    } else if let Some(attachment) = super::media::attachment(payload) {
        MessageBody::Media(Box::new(attachment))
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
        } else if message.reaction_message.is_set() || message.enc_reaction_message.is_set() {
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
fn edited_text(message: &wa::Message) -> Option<&str> {
    let base = message.get_base_message();
    base.text_content()
        .or_else(|| {
            base.video_message
                .as_option()
                .map(|m| m.caption.as_deref().unwrap_or(""))
        })
        .or_else(|| {
            base.image_message
                .as_option()
                .map(|m| m.caption.as_deref().unwrap_or(""))
        })
        .or_else(|| {
            base.document_message
                .as_option()
                .map(|m| m.caption.as_deref().unwrap_or(""))
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
        let mut info = MessageInfo {
            id: "incoming".into(),
            ..Default::default()
        };
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

    #[test]
    fn phone_image_with_current_cdn_path_keeps_its_preview_reference() {
        let mut inbound = fixture();
        let image = Arc::make_mut(&mut inbound.message)
            .image_message
            .as_option_mut()
            .unwrap();
        image.direct_path = Some("/o1/v/t24/f2/m232/synthetic-image?ccb=9-4&oh=fake".into());
        image.media_key = Some(vec![1; 32]);
        image.file_sha256 = Some(vec![2; 32]);
        image.file_enc_sha256 = Some(vec![3; 32]);
        image.file_length = Some(128);
        image.mimetype = Some("image/jpeg".into());
        let payload = wa::Message {
            device_sent_message: MessageField::some(wa::message::DeviceSentMessage {
                message: MessageField::some((*inbound.message).clone()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let key = MessageKey {
            account: "self@s.whatsapp.net".into(),
            chat: "chat@s.whatsapp.net".into(),
            sender: "self@s.whatsapp.net".into(),
            id: "phone-image".into(),
            from_me: true,
        };
        let MessageChange::Upsert(message) = normalize(key, &payload, 0, None) else {
            panic!()
        };
        assert!(
            matches!(message.body, MessageBody::Media(ref a) if a.kind == crate::media::AttachmentKind::Image)
        );
    }
    #[test]
    fn retains_complete_media_references_from_live_and_history() {
        let mut inbound = fixture();
        let image = Arc::make_mut(&mut inbound.message)
            .image_message
            .as_option_mut()
            .unwrap();
        image.direct_path = Some("/v/t62.7118-24/example?hash=test".into());
        image.media_key = Some(vec![1; 32]);
        image.file_sha256 = Some(vec![2; 32]);
        image.file_enc_sha256 = Some(vec![3; 32]);
        image.file_length = Some(8);
        image.mimetype = Some("image/jpeg".into());
        let batch = message_batch("test".into(), MessageSource::Live, &[inbound.clone()]);
        let MessageChange::Upsert(m) = &batch.changes[0] else {
            panic!("missing")
        };
        let body = serde_json::to_value(&m.body).unwrap();
        assert_eq!(body["Media"]["size"], 8);
        assert_eq!(body["Media"]["caption"], "picture caption");
        let web = wa::WebMessageInfo {
            key: MessageField::some(wa::MessageKey {
                id: Some("history-image".into()),
                from_me: Some(false),
                ..Default::default()
            }),
            message: MessageField::some((*inbound.message).clone()),
            message_timestamp: Some(1_790_640_000),
            ..Default::default()
        };
        let Some(MessageChange::Upsert(history)) =
            history_message(&"test".into(), &"chat".into(), &web)
        else {
            panic!("missing history")
        };
        assert_eq!(history.body, m.body);
    }

    #[test]
    fn incomplete_and_view_once_media_remain_placeholders() {
        let mut payload = wa::Message {
            document_message: MessageField::some(wa::message::DocumentMessage {
                direct_path: Some("/v/document".into()),
                media_key: Some(vec![1; 32]),
                file_sha256: Some(vec![2; 32]),
                file_enc_sha256: Some(vec![3; 32]),
                file_length: Some(12),
                file_name: Some("../invoice.pdf".into()),
                mimetype: Some("application/pdf".into()),
                caption: Some("invoice".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let key = MessageKey {
            account: "test".into(),
            chat: "chat".into(),
            sender: "alice".into(),
            id: "doc".into(),
            from_me: false,
        };
        let MessageChange::Upsert(m) = normalize(key.clone(), &payload, 0, None) else {
            panic!("missing")
        };
        assert_eq!(
            serde_json::to_value(m.body).unwrap()["Media"]["filename"],
            "../invoice.pdf"
        );
        for bad in [
            "https://evil.invalid/file",
            "//evil.invalid/file",
            "/v/file\n",
            "/v/../file",
            "/o1/v/../file",
            "/o1/v/file#fragment",
        ] {
            payload
                .document_message
                .as_option_mut()
                .unwrap()
                .direct_path = Some(bad.into());
            let MessageChange::Upsert(m) = normalize(key.clone(), &payload, 0, None) else {
                panic!("missing")
            };
            assert!(matches!(m.body, MessageBody::Unsupported { .. }));
        }
        payload
            .document_message
            .as_option_mut()
            .unwrap()
            .direct_path = Some("/v/document".into());
        payload.document_message.as_option_mut().unwrap().media_key = Some(vec![1; 31]);
        let MessageChange::Upsert(m) = normalize(key.clone(), &payload, 0, None) else {
            panic!("missing")
        };
        assert!(matches!(m.body, MessageBody::Unsupported { .. }));
        payload.document_message.as_option_mut().unwrap().media_key = Some(vec![1; 32]);
        let once = wa::Message {
            ephemeral_message: MessageField::some(wa::message::FutureProofMessage {
                message: MessageField::some(wa::Message {
                    view_once_message_v2: MessageField::some(wa::message::FutureProofMessage {
                        message: MessageField::some(payload),
                    }),
                    ..Default::default()
                }),
            }),
            ..Default::default()
        };
        let MessageChange::Upsert(m) = normalize(key, &once, 0, None) else {
            panic!("missing")
        };
        assert!(matches!(m.body, MessageBody::Unsupported { .. }));
    }
}

#[cfg(test)]
mod reaction_tests {
    use super::*;
    use whatsapp_rust::prelude::{MessageBuilderExt, MessageField};
    fn envelope(group: bool, from_me: bool) -> MessageKey {
        MessageKey {
            account: "111@s.whatsapp.net".into(),
            chat: if group {
                "123@g.us"
            } else {
                "222@s.whatsapp.net"
            }
            .into(),
            sender: if from_me {
                "111@s.whatsapp.net"
            } else {
                "222@s.whatsapp.net"
            }
            .into(),
            id: "envelope".into(),
            from_me,
        }
    }
    #[test]
    fn undecrypted_reactions_never_become_live_or_history_messages() {
        use whatsapp_rust::types::message::{MessageInfo, MessageSource as NativeSource};
        // The native dispatcher forwards these envelopes when the parent secret
        // is missing, or when ciphertext cannot be decoded.
        for encrypted in [
            wa::message::EncReactionMessage {
                target_message_key: MessageField::some(wa::MessageKey {
                    id: Some("missing-parent".into()),
                    from_me: Some(false),
                    participant: Some("111@s.whatsapp.net".into()),
                    ..Default::default()
                }),
                enc_payload: Some(vec![7; 32]),
                enc_iv: Some(vec![8; 12]),
            },
            wa::message::EncReactionMessage {
                enc_payload: Some(vec![0]),
                ..Default::default()
            },
        ] {
            let payload = wa::Message {
                enc_reaction_message: MessageField::some(encrypted),
                ..Default::default()
            };
            let info = MessageInfo {
                source: NativeSource {
                    chat: "123@g.us".parse().unwrap(),
                    sender: "222@s.whatsapp.net".parse().unwrap(),
                    is_group: true,
                    ..Default::default()
                },
                id: "encrypted-reaction".into(),
                ..Default::default()
            };
            let incoming = InboundMessage::builder()
                .message(std::sync::Arc::new(payload.clone()))
                .info(std::sync::Arc::new(info))
                .build();
            assert!(
                message_batch(
                    "111@s.whatsapp.net".into(),
                    MessageSource::Live,
                    &[incoming]
                )
                .changes
                .is_empty()
            );
            let web = wa::WebMessageInfo {
                key: MessageField::some(wa::MessageKey {
                    id: Some("encrypted-reaction".into()),
                    participant: Some("222@s.whatsapp.net".into()),
                    ..Default::default()
                }),
                message: MessageField::some(payload),
                message_timestamp: Some(100),
                ..Default::default()
            };
            assert!(
                history_changes(&"111@s.whatsapp.net".into(), &"123@g.us".into(), &web).is_empty()
            );
        }
    }
    #[test]
    fn reactions_resolve_sender_relative_targets_in_direct_and_group_chats() {
        for (group, outer_mine, target_mine, participant, expected, mine) in [
            (false, false, false, None, "111@s.whatsapp.net", true),
            (false, false, true, None, "222@s.whatsapp.net", false),
            (false, true, false, None, "222@s.whatsapp.net", false),
            (
                true,
                false,
                false,
                Some("333@s.whatsapp.net"),
                "333@s.whatsapp.net",
                false,
            ),
            (true, false, true, None, "222@s.whatsapp.net", false),
            (
                true,
                false,
                false,
                Some("111@s.whatsapp.net"),
                "111@s.whatsapp.net",
                true,
            ),
        ] {
            let payload = whatsapp_rust::wacore::proto_helpers::build_reaction_message(
                wa::MessageKey {
                    id: Some("original".into()),
                    from_me: Some(target_mine),
                    participant: participant.map(str::to_owned),
                    ..Default::default()
                },
                "👍",
                1234,
            );
            let value =
                serde_json::to_value(normalize(envelope(group, outer_mine), &payload, 5678, None))
                    .unwrap();
            assert_eq!(value["Reaction"]["key"]["sender"], expected);
            assert_eq!(value["Reaction"]["key"]["from_me"], mine);
            assert_eq!(value["Reaction"]["key"]["id"], "original");
            assert_eq!(value["Reaction"]["at_ms"], 1234);
        }
    }
    #[test]
    fn history_summary_reactions_are_separate_from_original_message() {
        let web = wa::WebMessageInfo {
            key: MessageField::some(wa::MessageKey {
                id: Some("original".into()),
                from_me: Some(true),
                ..Default::default()
            }),
            message: MessageField::some(wa::Message::text("hello")),
            message_timestamp: Some(100),
            reactions: vec![wa::Reaction {
                key: MessageField::some(wa::MessageKey {
                    id: Some("reaction".into()),
                    participant: Some("222@s.whatsapp.net".into()),
                    from_me: Some(false),
                    ..Default::default()
                }),
                text: Some("❤️".into()),
                sender_timestamp_ms: Some(101_000),
                ..Default::default()
            }],
            ..Default::default()
        };
        let changes = history_changes(&"111@s.whatsapp.net".into(), &"group@g.us".into(), &web);
        assert_eq!(changes.len(), 2);
        let v = serde_json::to_value(&changes[1]).unwrap();
        assert_eq!(v["Reaction"]["reactor"], "222@s.whatsapp.net");
        assert_eq!(v["Reaction"]["key"]["sender"], "111@s.whatsapp.net");
    }
}

fn not_reaction_placeholder(change: &MessageChange) -> bool {
    !matches!(change, MessageChange::Upsert(MessageRecord { body: MessageBody::Unsupported { kind, .. }, .. }) if kind == "reaction")
}
fn reaction_target(envelope: &MessageKey, target: &wa::MessageKey) -> Option<MessageKey> {
    let id = target.id.as_ref().filter(|id| !id.is_empty())?;
    let sender = if target.from_me.unwrap_or(false) {
        envelope.sender.0.clone()
    } else if envelope.chat.0.ends_with("@g.us") {
        if target
            .remote_jid
            .as_ref()
            .is_some_and(|jid| jid != &envelope.chat.0)
        {
            return None;
        }
        target.participant.clone().filter(|s| !s.is_empty())?
    } else if envelope.from_me {
        envelope.chat.0.clone()
    } else {
        envelope.account.0.clone()
    };
    let sender = sender
        .parse::<whatsapp_rust::Jid>()
        .map(|j| jid(&j))
        .unwrap_or(sender);
    Some(MessageKey {
        account: envelope.account.clone(),
        chat: envelope.chat.clone(),
        from_me: sender == envelope.account.0,
        sender: sender.into(),
        id: id.clone().into(),
    })
}
pub(super) fn history_changes(
    account: &AccountId,
    chat: &ChatId,
    web: &wa::WebMessageInfo,
) -> Vec<MessageChange> {
    let Some(change) = history_message(account, chat, web).filter(not_reaction_placeholder) else {
        return vec![];
    };
    let MessageChange::Upsert(message) = &change else {
        return vec![change];
    };
    let key = message.key.clone();
    let mut changes = vec![change];
    for r in &web.reactions {
        let Some(k) = r.key.as_option() else {
            continue;
        };
        let (Some(emoji), Some(at_ms)) = (&r.text, r.sender_timestamp_ms) else {
            continue;
        };
        let reactor = if k.from_me.unwrap_or(false) {
            account.0.clone()
        } else if chat.0.ends_with("@g.us") {
            let Some(p) = k.participant.clone() else {
                continue;
            };
            p
        } else {
            chat.0.clone()
        };
        changes.push(MessageChange::Reaction(Reaction {
            key: key.clone(),
            reactor: reactor.into(),
            emoji: emoji.clone(),
            at_ms,
            event_id: k.id.clone().unwrap_or_default().into(),
        }));
    }
    changes
}

#[cfg(test)]
mod edit_identity_tests {
    use super::*;
    use whatsapp_rust::prelude::{MessageBuilderExt, MessageField};
    #[test]
    fn peers_edit_targets_their_message_not_our_account() {
        let key = MessageKey {
            account: "111@s.whatsapp.net".into(),
            chat: "222@s.whatsapp.net".into(),
            sender: "222@s.whatsapp.net".into(),
            id: "edit-envelope".into(),
            from_me: false,
        };
        let payload = wa::Message {
            protocol_message: MessageField::some(wa::message::ProtocolMessage {
                key: MessageField::some(wa::MessageKey {
                    id: Some("original".into()),
                    from_me: Some(true),
                    ..Default::default()
                }),
                r#type: Some(wa::message::protocol_message::Type::MessageEdit),
                edited_message: MessageField::some(wa::Message::text("edited")),
                timestamp_ms: Some(101),
                ..Default::default()
            }),
            ..Default::default()
        };
        let MessageChange::Edit { key, .. } = normalize(key, &payload, 101, None) else {
            panic!()
        };
        assert_eq!(key.sender.0, "222@s.whatsapp.net");
        assert!(!key.from_me);
    }
}

fn quoted_summary(payload: &wa::Message) -> Option<(String, Option<crate::media::AttachmentKind>)> {
    use crate::media::AttachmentKind as K;
    if payload.is_view_once() {
        return None;
    }
    let base = payload.get_base_message();
    if let Some(text) = base.text_content() {
        return Some((text.into(), None));
    }
    let (kind, caption) = if let Some(image) = base.image_message.as_option() {
        (K::Image, image.caption.as_deref())
    } else if let Some(video) = base.video_message.as_option() {
        (
            if video.gif_playback == Some(true) {
                K::Gif
            } else {
                K::Video
            },
            video.caption.as_deref(),
        )
    } else if base.sticker_message.is_set() {
        (K::Sticker, None)
    } else if base.audio_message.is_set() {
        (K::Audio, None)
    } else {
        let document = base.document_message.as_option()?;
        (
            K::Document,
            document
                .caption
                .as_deref()
                .or(document.file_name.as_deref())
                .or(document.title.as_deref()),
        )
    };
    let label = format!("[{}]", kind.label());
    let text = match caption.filter(|s| !s.is_empty()) {
        Some(s) if s.starts_with(&label) => s.to_owned(),
        Some(s) => format!("{label} {s}"),
        None => label,
    };
    Some((text, Some(kind)))
}

#[cfg(test)]
mod media_quote_tests {
    use super::*;
    use whatsapp_rust::prelude::{MessageBuilderExt, MessageField};
    #[test]
    fn gif_quote_retains_its_kind_without_full_media_references() {
        let payload = wa::Message {
            video_message: MessageField::some(wa::message::VideoMessage {
                gif_playback: Some(true),
                caption: Some("A loop".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        assert_eq!(
            quoted_summary(&payload),
            Some((
                "[gif] A loop".into(),
                Some(crate::media::AttachmentKind::Gif)
            ))
        );
    }
    #[test]
    fn incoming_captionless_sticker_quote_keeps_identity_and_kind() {
        let context = wa::ContextInfo {
            stanza_id: Some("sticker-original".into()),
            participant: Some("222@s.whatsapp.net".into()),
            quoted_message: MessageField::some(wa::Message {
                sticker_message: MessageField::some(wa::message::StickerMessage::default()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let key = MessageKey {
            account: "111@s.whatsapp.net".into(),
            chat: "group@g.us".into(),
            sender: "333@s.whatsapp.net".into(),
            id: "reply".into(),
            from_me: false,
        };
        let MessageChange::Upsert(m) = normalize(
            key,
            &wa::Message::text_with_context("nice sticker", context),
            1,
            None,
        ) else {
            panic!()
        };
        let q = m.quote.unwrap();
        assert_eq!(q.preview, "[sticker]");
        assert_eq!(q.key.id.0, "sticker-original");
        assert_eq!(q.key.sender.0, "222@s.whatsapp.net");
        assert_eq!(q.availability, QuoteAvailability::Available);
        assert_eq!(q.media_kind, Some(crate::media::AttachmentKind::Sticker));
    }
}
