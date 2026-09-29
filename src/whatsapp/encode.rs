use super::BackendError;
use crate::app::model::*;
use whatsapp_rust::prelude::*;

pub struct EncodedText {
    pub to: Jid,
    pub message: wa::Message,
    pub options: SendOptions,
}
pub fn encode_text(outbound: &OutboundText) -> Result<EncodedText, BackendError> {
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
        let context = whatsapp_rust::wacore::proto_helpers::build_quote_context_with_info(
            quote.key.id.0.clone(),
            &sender,
            &to,
            &to,
            &wa::Message::text(preview),
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

#[cfg(test)]
mod tests {
    use super::*;
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
                text: "  hello\nworld  ".into(),
                reply: Some(Quote {
                    key,
                    preview: "previous text".into(),
                    availability: QuoteAvailability::Available,
                }),
                revision: 1,
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
