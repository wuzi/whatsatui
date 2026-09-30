//! Synthetic offline account. This module never constructs a network client.
use super::*;
use crate::app::model::*;
use crate::media::{Attachment, AttachmentKind};
use sha2::{Digest, Sha256};
const STICKER: &[u8] = include_bytes!("../../tests/fixtures/send-sticker.webp");
const IMAGE: &[u8] = include_bytes!("demo-image.png");
const AUDIO: &[u8] = include_bytes!("../../tests/fixtures/voice.ogg");
fn audio_attachment() -> Attachment {
    Attachment {
        kind: AttachmentKind::Audio,
        audio: Some(crate::media::AudioMetadata {
            voice: true,
            seconds: Some(8),
        }),
        filename: None,
        mime: Some("audio/ogg; codecs=opus".into()),
        caption: None,
        size: AUDIO.len() as u64,
        direct_path: "/v/offline-audio".into(),
        media_key: [0; 32],
        sha256: Sha256::digest(AUDIO).into(),
        encrypted_sha256: [0; 32],
    }
}
fn image_attachment() -> Attachment {
    Attachment {
        audio: None,
        kind: AttachmentKind::Image,
        filename: Some("demo-cyan.png".into()),
        mime: Some("image/png".into()),
        caption: Some("The café on the corner · synthetic image demo".into()),
        size: IMAGE.len() as u64,
        direct_path: "/v/offline-demo".into(),
        media_key: [0; 32],
        sha256: Sha256::digest(IMAGE).into(),
        encrypted_sha256: [0; 32],
    }
}
fn sticker_attachment() -> Attachment {
    Attachment {
        audio: None,
        kind: AttachmentKind::Sticker,
        filename: Some("hello.webp".into()),
        mime: Some("image/webp".into()),
        caption: None,
        size: STICKER.len() as u64,
        direct_path: "/v/offline-sticker".into(),
        media_key: [0; 32],
        sha256: Sha256::digest(STICKER).into(),
        encrypted_sha256: [0; 32],
    }
}
const ACCOUNT: &str = "you@demo";
const TIME: i64 = 1_790_640_000_000;
fn key(chat: &str, sender: &str, id: &str) -> MessageKey {
    MessageKey {
        account: ACCOUNT.into(),
        chat: chat.into(),
        sender: sender.into(),
        id: id.into(),
        from_me: sender == ACCOUNT,
    }
}
fn message(chat: &str, sender: &str, id: &str, text: &str, offset: i64) -> MessageRecord {
    MessageRecord {
        key: key(chat, sender, id),
        body: MessageBody::Text(text.into()),
        quote: None,
        created_at_ms: TIME + offset,
        edited_at_ms: None,
        expires_at_ms: None,
        send_state: (sender == ACCOUNT).then_some(SendState::Read),
    }
}
async fn initialize(store: &Store) -> Result<StoreChange, BackendError> {
    let chats = [
        ("alice@demo", "Alice", false),
        ("weekend@g.us", "Weekend plans", true),
        ("maya@demo", "Maya", false),
        ("leo@demo", "Leo", false),
    ]
    .into_iter()
    .map(|(id, name, is_group)| ChatSummary {
        account: ACCOUNT.into(),
        chat: id.into(),
        name: name.into(),
        phone: None,
        is_group,
        unread: match id {
            "weekend@g.us" => 3,
            "maya@demo" => 1,
            _ => 0,
        },
        latest_at_ms: match id {
            "weekend@g.us" => TIME + 150_000,
            "maya@demo" => TIME + 45_000,
            _ => 0,
        },
        ..Default::default()
    })
    .collect();
    store
        .upsert_chats(ACCOUNT.into(), chats)
        .await
        .map_err(|e| BackendError::Service(e.into()))?;
    let mut messages = vec![
        message(
            "alice@demo",
            "alice@demo",
            "a1",
            "Hey! How is the new terminal setup going?",
            0,
        ),
        message(
            "alice@demo",
            ACCOUNT,
            "a2",
            "Cyan borders, a good keyboard, and no lost drafts.",
            60_000,
        ),
        message(
            "alice@demo",
            "alice@demo",
            "a3",
            "Tab moves between panes. Ctrl-P finds chats; Ctrl-F searches this conversation. Try cyan.\n*Message actions*: Enter in Messages, y to copy, o for links.\n_Italic_ ~old~ `code`\nhttps://example.org/whatsapp-tui",
            120_000,
        ),
        message(
            "weekend@g.us",
            "maya@demo",
            "g1",
            "Saturday coffee? ☕",
            30_000,
        ),
        message(
            "weekend@g.us",
            "leo@demo",
            "g2",
            "Sounds good. I'll bring the book we talked about.",
            90_000,
        ),
        message(
            "maya@demo",
            "maya@demo",
            "m1",
            "A little Unicode test: café, 日本語, 👩‍💻",
            45_000,
        ),
    ];
    let mut photo = message("weekend@g.us", "maya@demo", "g3", "", 150_000);
    photo.body = MessageBody::Media(Box::new(image_attachment()));
    messages.push(photo);
    let mut sticker = message("leo@demo", "leo@demo", "l-sticker", "", 35_000);
    sticker.body = MessageBody::Media(Box::new(sticker_attachment()));
    messages.push(sticker);
    let mut audio = message("maya@demo", "maya@demo", "m-audio", "", 50_000);
    audio.body = MessageBody::Media(Box::new(audio_attachment()));
    messages.push(audio);
    let mut changes = messages
        .into_iter()
        .map(MessageChange::Upsert)
        .collect::<Vec<_>>();
    for (reactor, emoji) in [(ACCOUNT, "👍"), ("alice@demo", "👍")] {
        changes.push(MessageChange::Reaction(Reaction {
            key: key("alice@demo", "alice@demo", "a3"),
            reactor: reactor.into(),
            emoji: emoji.into(),
            at_ms: TIME + 121_000,
            event_id: format!("demo-reaction-{reactor}").into(),
        }));
    }
    store
        .apply_batch(MessageBatch {
            account: ACCOUNT.into(),
            source: MessageSource::History,
            changes,
        })
        .await
        .map_err(|e| BackendError::Service(e.into()))
}
pub fn start(store: Store) -> BackendHandle {
    let (commands, mut requests) = mpsc::channel(32);
    let (tx, events) = mpsc::channel(256);
    let (stop, mut stopping) = oneshot::channel();
    let task = tokio::spawn(async move {
        let change = initialize(&store).await?;
        for event in [
            BackendEvent::AccountKnown(ACCOUNT.into()),
            BackendEvent::ConnectionChanged {
                state: ConnectionState::Connected,
                reason: Some("Offline demo · synthetic conversations".into()),
            },
            BackendEvent::StoreChanged(change),
        ] {
            if tx.send(event).await.is_err() {
                return Ok(());
            }
        }
        let mut counter = 0u64;
        loop {
            tokio::select! {_=&mut stopping=>break,command=requests.recv()=>{let Some(command)=command else{break};match command{
                BackendCommand::PrepareText{request,chat,draft}=>{counter+=1;let message=OutboundText{key:key(&chat.0,ACCOUNT,&format!("demo-{counter:06}")),draft,created_at_ms:chrono::Utc::now().timestamp_millis()};if tx.send(BackendEvent::Prepared {request,message: Box::new(message)}).await.is_err(){break;}},
                BackendCommand::Transmit(sent)=>{
                    if tx.send(BackendEvent::SendOutcome{key:sent.key.clone(),state:SendState::Sent}).await.is_err(){break;}
                    let sender=if sent.key.chat.0.ends_with("@g.us"){"maya@demo"}else{&sent.key.chat.0};
                    let mut reply=message(&sent.key.chat.0,sender,&format!("reply-{counter:06}"),"Message received. This is an offline demo response.",181_000+counter as i64*1000);
                    reply.created_at_ms=sent.created_at_ms+1;
                    let change=store.apply_batch(MessageBatch{account:ACCOUNT.into(),source:MessageSource::Live,changes:vec![MessageChange::Upsert(reply)]}).await.map_err(|e|BackendError::Service(e.into()))?;
                    let _=tx.send(BackendEvent::StoreChanged(change)).await;
                    let change=store.record_receipt(Receipt{key:sent.key.clone(),recipient:sender.into(),state:ReceiptState::Read,at_ms:TIME+182_000+counter as i64*1000}).await.map_err(|e|BackendError::Service(e.into()))?;
                    let _=tx.send(BackendEvent::StoreChanged(change)).await;
                }
                BackendCommand::Mutate { request, message, kind } => {
                    counter += 1;
                    let account=message.key.account.clone(); let chat=message.key.chat.clone();
                    let result=super::interactions::execute(&store,&super::interactions::Demo,MutationAttempt { id:format!("demo-mutation-{counter}-{}",chrono::Utc::now().timestamp_millis()),target:*message,kind,created_at_ms:chrono::Utc::now().timestamp_millis(),state:MutationState::Pending }).await;
                    let _=tx.send(BackendEvent::StoreChanged(StoreChange { account:account.clone(),chats:vec![chat] })).await;
                    let _=tx.send(BackendEvent::MutationOutcome { request,account,result }).await;
                }
                BackendCommand::MarkRead(_)=>{}
            }}}
        }
        let _ = tx.send(BackendEvent::Stopped).await;
        Ok(())
    });
    BackendHandle {
        profiles: std::sync::Arc::new(crate::avatars::Unavailable),
        media: std::sync::Arc::new(DemoDownloader),
        commands,
        events,
        control: BackendControl::new(stop, task),
    }
}

pub struct DemoDownloader;
#[async_trait::async_trait]
impl crate::media::Downloader for DemoDownloader {
    async fn download(
        &self,
        attachment: &Attachment,
        destination: &std::path::Path,
        cancel: tokio::sync::watch::Receiver<bool>,
    ) -> Result<(), String> {
        if *cancel.borrow() || cancel.has_changed().is_err() {
            return Err("Demo download canceled".into());
        }
        if attachment != &image_attachment()
            && attachment != &sticker_attachment()
            && attachment != &audio_attachment()
        {
            return Err("Attachment is not an offline demo fixture".into());
        }
        std::fs::write(
            destination,
            if attachment.kind == AttachmentKind::Audio {
                AUDIO
            } else if attachment.kind == AttachmentKind::Sticker {
                STICKER
            } else {
                IMAGE
            },
        )
        .map_err(|_| "Could not write the demo image".into())
    }
}
