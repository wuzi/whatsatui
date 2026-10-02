//! Synthetic offline account. This module never constructs a network client.
use super::*;
use crate::app::model::*;
use crate::media::{Attachment, AttachmentKind};
use sha2::{Digest, Sha256};
mod scene;
const STICKER: &[u8] = include_bytes!("../../tests/fixtures/send-sticker.webp");
const IMAGE: &[u8] = include_bytes!("../../assets/demo/cafe.jpg");
const AUDIO: &[u8] = include_bytes!("../../tests/fixtures/voice.ogg");
const VIDEO: &[u8] = include_bytes!("../../tests/fixtures/video.mp4");
const GIF: &[u8] = include_bytes!("../../tests/fixtures/loop.mp4");
fn gif_attachment() -> Attachment {
    Attachment {
        kind: AttachmentKind::Gif,
        filename: Some("demo-loop.mp4".into()),
        caption: Some("GIF demo · loops inline".into()),
        size: GIF.len() as u64,
        direct_path: "/v/offline-gif".into(),
        sha256: Sha256::digest(GIF).into(),
        ..video_attachment()
    }
}
fn video_attachment() -> Attachment {
    Attachment {
        kind: AttachmentKind::Video,
        audio: None,
        filename: Some("demo-video.mp4".into()),
        mime: Some("video/mp4".into()),
        caption: Some("A synthetic video · try Play to open mpv".into()),
        size: VIDEO.len() as u64,
        direct_path: "/v/offline-video".into(),
        media_key: [0; 32],
        sha256: Sha256::digest(VIDEO).into(),
        encrypted_sha256: [0; 32],
    }
}
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
        filename: Some("saturday-coffee.jpg".into()),
        mime: Some("image/jpeg".into()),
        caption: Some("Found our Saturday spot ☕".into()),
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
pub fn start(store: Store) -> BackendHandle {
    let (commands, mut requests) = mpsc::channel(32);
    let (tx, events) = mpsc::channel(256);
    let (stop, mut stopping) = oneshot::channel();
    let task = tokio::spawn(async move {
        let change = scene::initialize(&store).await?;
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
        profiles: std::sync::Arc::new(scene::Profiles),
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
            && attachment != &video_attachment()
            && attachment != &gif_attachment()
        {
            return Err("Attachment is not an offline demo fixture".into());
        }
        std::fs::write(
            destination,
            if attachment.kind == AttachmentKind::Gif {
                GIF
            } else if attachment.kind == AttachmentKind::Video {
                VIDEO
            } else if attachment.kind == AttachmentKind::Audio {
                AUDIO
            } else if attachment.kind == AttachmentKind::Sticker {
                STICKER
            } else {
                IMAGE
            },
        )
        .map_err(|_| "Could not write the demo media".into())
    }
}
