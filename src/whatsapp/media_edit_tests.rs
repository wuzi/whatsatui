use super::normalize::normalize;
use crate::{app::model::*, desktop::Desktop, media::*, storage::Store};
use sha2::{Digest, Sha256};
use whatsapp_rust::prelude::{MessageField, wa};

fn original(kind: AttachmentKind) -> MessageRecord {
    let key = MessageKey {
        account: "test".into(),
        chat: "group@g.us".into(),
        sender: "alice".into(),
        id: "original".into(),
        from_me: false,
    };
    MessageRecord {
        key,
        body: MessageBody::Media(Box::new(Attachment {
            audio: None,
            kind,
            filename: Some("test.txt".into()),
            mime: Some("text/plain".into()),
            caption: Some("original caption".into()),
            size: 6,
            direct_path: "/v/test".into(),
            media_key: [1; 32],
            sha256: Sha256::digest(b"hello\n").into(),
            encrypted_sha256: [3; 32],
        })),
        quote: None,
        created_at_ms: 0,
        edited_at_ms: None,
        expires_at_ms: None,
        send_state: None,
    }
}
fn edit(message: &MessageRecord, caption: &str, at: i64) -> MessageChange {
    let MessageBody::Media(attachment) = &message.body else {
        panic!()
    };
    // Caption edits need not repeat the original encrypted media references.
    let content = match attachment.kind {
        AttachmentKind::Sticker | AttachmentKind::Audio => {
            panic!("attachment has no editable caption")
        }
        AttachmentKind::Image => wa::Message {
            image_message: MessageField::some(wa::message::ImageMessage {
                caption: Some(caption.into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        AttachmentKind::Video | AttachmentKind::Gif => wa::Message {
            video_message: MessageField::some(wa::message::VideoMessage {
                caption: Some(caption.into()),
                ..Default::default()
            }),
            ..Default::default()
        },
        AttachmentKind::Document => wa::Message {
            document_message: MessageField::some(wa::message::DocumentMessage {
                caption: (!caption.is_empty()).then(|| caption.into()),
                ..Default::default()
            }),
            ..Default::default()
        },
    };
    let envelope = wa::Message {
        protocol_message: MessageField::some(wa::message::ProtocolMessage {
            r#type: Some(wa::message::protocol_message::Type::MessageEdit),
            key: MessageField::some(wa::MessageKey {
                id: Some(message.key.id.0.clone()),
                from_me: Some(false),
                participant: Some(message.key.sender.0.clone()),
                ..Default::default()
            }),
            edited_message: MessageField::some(content),
            timestamp_ms: Some(at),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut event_key = message.key.clone();
    event_key.id = format!("edit-event-{at}").into();
    normalize(event_key, &envelope, at, None)
}
async fn apply(store: &Store, changes: Vec<MessageChange>) {
    store
        .apply_batch(MessageBatch {
            account: "test".into(),
            source: MessageSource::Live,
            changes,
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn media_caption_edit_envelopes_preserve_references_and_mutation_order() {
    for kind in [
        AttachmentKind::Image,
        AttachmentKind::Document,
        AttachmentKind::Video,
        AttachmentKind::Gif,
    ] {
        for before_original in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let store = Store::open(dir.path().join("db")).await.unwrap();
            let original = original(kind);
            let change = edit(&original, "updated caption", 20);
            assert!(matches!(&change, MessageChange::Edit { key, .. } if key == &original.key));
            let mut changes = vec![MessageChange::Upsert(original.clone()), change];
            if before_original {
                changes.reverse();
            }
            apply(&store, changes).await;
            // Older edits and history replays cannot restore an old caption.
            apply(
                &store,
                vec![
                    edit(&original, "older", 10),
                    MessageChange::Upsert(original.clone()),
                ],
            )
            .await;
            let mut expected = original.clone();
            let MessageBody::Media(attachment) = &mut expected.body else {
                panic!()
            };
            attachment.caption = Some("updated caption".into());
            expected.edited_at_ms = Some(20);
            assert_eq!(
                store.get_message(original.key.clone()).await.unwrap(),
                Some(expected)
            );
            assert_eq!(
                store
                    .search_messages(
                        "test".into(),
                        original.key.chat.clone(),
                        "updated".into(),
                        0
                    )
                    .await
                    .unwrap()
                    .hits
                    .len(),
                1
            );
            apply(&store, vec![edit(&original, "", 30)]).await;
            let saved = store
                .get_message(original.key.clone())
                .await
                .unwrap()
                .unwrap();
            assert!(
                matches!(&saved.body, MessageBody::Media(a) if a.caption.as_deref() == Some(""))
            );
            apply(
                &store,
                vec![
                    MessageChange::Delete {
                        key: original.key.clone(),
                    },
                    edit(&original, "after deletion", 40),
                ],
            )
            .await;
            assert_eq!(
                store.get_message(original.key).await.unwrap().unwrap().body,
                MessageBody::Deleted
            );
        }
    }
}

#[tokio::test]
async fn media_caption_edit_envelope_during_transfer_prevents_publication() {
    struct EditingSource {
        store: Store,
        message: MessageRecord,
    }
    #[async_trait::async_trait]
    impl Downloader for EditingSource {
        async fn download(
            &self,
            _: &Attachment,
            destination: &std::path::Path,
            _: tokio::sync::watch::Receiver<bool>,
        ) -> Result<(), String> {
            std::fs::write(destination, b"hello\n").unwrap();
            apply(&self.store, vec![edit(&self.message, "new caption", 20)]).await;
            Ok(())
        }
    }
    struct UnusedDesktop;
    #[async_trait::async_trait]
    impl Desktop for UnusedDesktop {
        async fn copy(&self, _: &str) -> Result<(), String> {
            panic!("unexpected desktop action")
        }
        async fn open(&self, _: &str) -> Result<(), String> {
            panic!("unexpected desktop action")
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("db")).await.unwrap();
    let message = original(AttachmentKind::Image);
    apply(&store, vec![MessageChange::Upsert(message.clone())]).await;
    let source = EditingSource {
        store: store.clone(),
        message: message.clone(),
    };
    let (_stop, cancel) = tokio::sync::watch::channel(false);
    assert!(
        execute(
            message,
            MediaAction::Download,
            store,
            &source,
            &UnusedDesktop,
            cancel
        )
        .await
        .is_err()
    );
    assert!(
        std::fs::read_dir(dir.path().join("media"))
            .unwrap()
            .all(|e| e.unwrap().file_name() == ".lock")
    );
}
