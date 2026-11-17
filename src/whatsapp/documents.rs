use super::encode::{self, EncodedText, UploadedDocument};
use crate::{app::model::*, media::outgoing};
use std::{path::PathBuf, time::Duration};
use whatsapp_rust::{Client, prelude::SendError};

#[async_trait::async_trait]
trait Transport: Sync {
    fn account(&self) -> Option<AccountId>;
    async fn upload(&self, bytes: Vec<u8>, media: MediaKind) -> Result<UploadedDocument, ()>;
    async fn send(&self, encoded: EncodedText) -> Result<(), SendError>;
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum MediaKind {
    Audio,
    Video,
    Document,
}
struct Native<'a>(&'a Client);
#[async_trait::async_trait]
impl Transport for Native<'_> {
    fn account(&self) -> Option<AccountId> {
        super::native::account(self.0)
    }
    async fn upload(&self, bytes: Vec<u8>, media: MediaKind) -> Result<UploadedDocument, ()> {
        self.0
            .upload(
                bytes,
                match media {
                    MediaKind::Audio => whatsapp_rust::wacore::download::MediaType::Audio,
                    MediaKind::Video => whatsapp_rust::wacore::download::MediaType::Video,
                    MediaKind::Document => whatsapp_rust::wacore::download::MediaType::Document,
                },
                whatsapp_rust::upload::UploadOptions::default(),
            )
            .await
            .map(Into::into)
            .map_err(|_| ())
    }
    async fn send(&self, encoded: EncodedText) -> Result<(), SendError> {
        self.0
            .send_message_with_options(encoded.to, encoded.message, encoded.options)
            .await
            .map(|_| ())
    }
}
pub(super) async fn send(client: &Client, message: OutboundText, data_dir: PathBuf) -> SendState {
    transmit(&Native(client), message, data_dir, Duration::from_secs(60)).await
}
async fn transmit(
    transport: &impl Transport,
    message: OutboundText,
    data_dir: PathBuf,
    deadline: Duration,
) -> SendState {
    let mut sending = false;
    let result = tokio::time::timeout(deadline, async {
        if transport.account().as_ref() != Some(&message.key.account) {
            return SendState::Failed;
        }
        let Some(local) = message.draft.document.clone() else {
            return SendState::Failed;
        };
        let expected = local.clone();
        let root = data_dir.clone();
        let prepared =
            tokio::task::spawn_blocking(move || outgoing::read_document(&local, &root)).await;
        let Ok(Ok(bytes)) = prepared else {
            return SendState::Failed;
        };
        if bytes.len() as u64 != expected.size {
            return SendState::Failed;
        }
        // Sound files without a caption chosen as playable go out as audio
        // messages; chosen videos go out as video messages. Anything else
        // keeps the document form (which supports captions).
        let media = if expected.is_audio()
            && message.draft.document_as_media
            && message.draft.text.is_empty()
        {
            MediaKind::Audio
        } else if expected.is_video() && message.draft.document_as_media {
            MediaKind::Video
        } else {
            MediaKind::Document
        };
        let video_meta = if media == MediaKind::Video {
            let path = outgoing::document_path(&expected, &data_dir).unwrap_or_else(|_| {
                data_dir
                    .join("outgoing")
                    .join(format!("{}.bin", expected.id))
            });
            probe_video(&path).await
        } else {
            None
        };
        let Ok(uploaded) = transport.upload(bytes, media).await else {
            return SendState::Failed;
        };
        if transport.account().as_ref() != Some(&message.key.account)
            || uploaded.attachment.size != expected.size
            || uploaded
                .attachment
                .sha256
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
                != expected.id
        {
            return SendState::Failed;
        }
        let Ok(encoded) = (match media {
            MediaKind::Audio => encode::encode_audio(&message, uploaded),
            MediaKind::Video => {
                let (width, height, seconds) = video_meta
                    .map(|m| (m.width, m.height, m.seconds))
                    .unwrap_or((None, None, None));
                encode::encode_video(&message, uploaded, width, height, seconds)
            }
            MediaKind::Document => encode::encode_document(&message, uploaded),
        }) else {
            return SendState::Failed;
        };
        sending = true;
        encode::classify_transport_result(&transport.send(encoded).await)
    })
    .await;
    result.unwrap_or(if sending {
        SendState::Unconfirmed
    } else {
        SendState::Failed
    })
}

struct VideoMeta {
    width: Option<u32>,
    height: Option<u32>,
    seconds: Option<u32>,
}

/// Best-effort video dimensions and duration via ffprobe. Missing tools or
/// unreadable files simply omit the fields; the message still sends.
async fn probe_video(path: &std::path::Path) -> Option<VideoMeta> {
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        tokio::process::Command::new("ffprobe")
            .arg("-v")
            .arg("quiet")
            .arg("-print_format")
            .arg("json")
            .arg("-show_format")
            .arg("-show_streams")
            .arg(path)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    let stream = json
        .get("streams")?
        .as_array()?
        .iter()
        .find(|s| s.get("codec_type").and_then(|c| c.as_str()) == Some("video"))?;
    let width = stream.get("width")?.as_u64()? as u32;
    let height = stream.get("height")?.as_u64()? as u32;
    let seconds = json
        .get("format")?
        .get("duration")?
        .as_str()?
        .parse::<f32>()
        .ok()
        .map(|d| d.max(0.0) as u32);
    Some(VideoMeta {
        width: Some(width),
        height: Some(height),
        seconds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    };
    struct Fake {
        sent: Mutex<Vec<EncodedText>>,
        fail: bool,
        switched: AtomicBool,
    }
    #[async_trait::async_trait]
    impl Transport for Fake {
        fn account(&self) -> Option<AccountId> {
            Some(
                if self.switched.load(Ordering::SeqCst) {
                    "other"
                } else {
                    "self@s.whatsapp.net"
                }
                .into(),
            )
        }
        async fn upload(&self, bytes: Vec<u8>, media: MediaKind) -> Result<UploadedDocument, ()> {
            use sha2::{Digest, Sha256};
            if self.fail {
                return Err(());
            }
            Ok(UploadedDocument {
                url: "https://mmg.whatsapp.net/v/doc".into(),
                media_key_timestamp: 7,
                attachment: crate::media::Attachment {
                    audio: None,
                    kind: match media {
                        MediaKind::Audio => crate::media::AttachmentKind::Audio,
                        MediaKind::Video => crate::media::AttachmentKind::Video,
                        MediaKind::Document => crate::media::AttachmentKind::Document,
                    },
                    filename: None,
                    caption: None,
                    mime: None,
                    size: bytes.len() as u64,
                    direct_path: "/v/doc".into(),
                    media_key: [1; 32],
                    sha256: Sha256::digest(&bytes).into(),
                    encrypted_sha256: [2; 32],
                },
            })
        }
        async fn send(&self, encoded: EncodedText) -> Result<(), SendError> {
            self.sent.lock().unwrap().push(encoded);
            Ok(())
        }
    }
    #[tokio::test]
    async fn document_upload_sends_document_stanza_with_filename_and_caption() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("app.apk");
        std::fs::write(&source, b"fake-apk-bytes").unwrap();
        let local = outgoing::import_document(&source, dir.path()).unwrap();
        assert_eq!(local.mime, "application/vnd.android.package-archive");
        let message = OutboundText {
            key: MessageKey {
                account: "self@s.whatsapp.net".into(),
                chat: "551100000002@s.whatsapp.net".into(),
                sender: "self@s.whatsapp.net".into(),
                id: "doc-id".into(),
                from_me: true,
            },
            draft: Draft {
                text: "see attached".into(),
                document: Some(Box::new(local)),
                ..Default::default()
            },
            created_at_ms: 0,
        };
        let fake = Fake {
            sent: Mutex::new(vec![]),
            fail: false,
            switched: AtomicBool::new(false),
        };
        assert_eq!(
            transmit(
                &fake,
                message,
                dir.path().to_owned(),
                Duration::from_secs(5)
            )
            .await,
            SendState::Sending
        );
        let sent = fake.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        let doc = sent[0].message.document_message.as_option().unwrap();
        assert_eq!(doc.file_name.as_deref(), Some("app.apk"));
        assert_eq!(
            doc.mimetype.as_deref(),
            Some("application/vnd.android.package-archive")
        );
        assert_eq!(doc.caption.as_deref(), Some("see attached"));
        assert_eq!(sent[0].options.message_id.as_deref(), Some("doc-id"));
    }
    #[tokio::test]
    async fn music_without_caption_sends_playable_audio_stanza() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("song.mp3");
        std::fs::write(&source, b"fake-mp3-bytes").unwrap();
        let local = outgoing::import_document(&source, dir.path()).unwrap();
        assert!(local.is_audio());
        let message = OutboundText {
            key: MessageKey {
                account: "self@s.whatsapp.net".into(),
                chat: "551100000002@s.whatsapp.net".into(),
                sender: "self@s.whatsapp.net".into(),
                id: "audio-id".into(),
                from_me: true,
            },
            draft: Draft {
                document: Some(Box::new(local)),
                document_as_media: true,
                ..Default::default()
            },
            created_at_ms: 0,
        };
        let fake = Fake {
            sent: Mutex::new(vec![]),
            fail: false,
            switched: AtomicBool::new(false),
        };
        assert_eq!(
            transmit(
                &fake,
                message,
                dir.path().to_owned(),
                Duration::from_secs(5)
            )
            .await,
            SendState::Sending
        );
        let sent = fake.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert!(sent[0].message.document_message.as_option().is_none());
        let audio = sent[0].message.audio_message.as_option().unwrap();
        assert_eq!(audio.mimetype.as_deref(), Some("audio/mpeg"));
        assert_eq!(audio.ptt, Some(false));
        assert_eq!(audio.file_length, Some(14));
    }
    #[tokio::test]
    async fn captioned_music_keeps_the_document_form() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("song.mp3");
        std::fs::write(&source, b"fake-mp3-bytes").unwrap();
        let local = outgoing::import_document(&source, dir.path()).unwrap();
        let message = OutboundText {
            key: MessageKey {
                account: "self@s.whatsapp.net".into(),
                chat: "551100000002@s.whatsapp.net".into(),
                sender: "self@s.whatsapp.net".into(),
                id: "audio-doc-id".into(),
                from_me: true,
            },
            draft: Draft {
                text: "listen".into(),
                document: Some(Box::new(local)),
                ..Default::default()
            },
            created_at_ms: 0,
        };
        let fake = Fake {
            sent: Mutex::new(vec![]),
            fail: false,
            switched: AtomicBool::new(false),
        };
        assert_eq!(
            transmit(
                &fake,
                message,
                dir.path().to_owned(),
                Duration::from_secs(5)
            )
            .await,
            SendState::Sending
        );
        let sent = fake.sent.lock().unwrap();
        assert!(sent[0].message.audio_message.as_option().is_none());
        let doc = sent[0].message.document_message.as_option().unwrap();
        assert_eq!(doc.caption.as_deref(), Some("listen"));
    }
    #[tokio::test]
    async fn chosen_video_sends_playable_video_stanza_with_caption() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("clip.mp4");
        std::fs::write(&source, b"fake-mp4-bytes").unwrap();
        let local = outgoing::import_document(&source, dir.path()).unwrap();
        assert!(local.is_video());
        let message = OutboundText {
            key: MessageKey {
                account: "self@s.whatsapp.net".into(),
                chat: "551100000002@s.whatsapp.net".into(),
                sender: "self@s.whatsapp.net".into(),
                id: "video-id".into(),
                from_me: true,
            },
            draft: Draft {
                text: "watch".into(),
                document: Some(Box::new(local)),
                document_as_media: true,
                ..Default::default()
            },
            created_at_ms: 0,
        };
        let fake = Fake {
            sent: Mutex::new(vec![]),
            fail: false,
            switched: AtomicBool::new(false),
        };
        assert_eq!(
            transmit(
                &fake,
                message,
                dir.path().to_owned(),
                Duration::from_secs(5)
            )
            .await,
            SendState::Sending
        );
        let sent = fake.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert!(sent[0].message.document_message.as_option().is_none());
        let video = sent[0].message.video_message.as_option().unwrap();
        assert_eq!(video.mimetype.as_deref(), Some("video/mp4"));
        assert_eq!(video.caption.as_deref(), Some("watch"));
    }
    #[tokio::test]
    async fn audio_without_an_explicit_choice_stays_a_document() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("song.mp3");
        std::fs::write(&source, b"fake-mp3-bytes").unwrap();
        let local = outgoing::import_document(&source, dir.path()).unwrap();
        let message = OutboundText {
            key: MessageKey {
                account: "self@s.whatsapp.net".into(),
                chat: "551100000002@s.whatsapp.net".into(),
                sender: "self@s.whatsapp.net".into(),
                id: "audio-doc-id".into(),
                from_me: true,
            },
            draft: Draft {
                document: Some(Box::new(local)),
                ..Default::default()
            },
            created_at_ms: 0,
        };
        let fake = Fake {
            sent: Mutex::new(vec![]),
            fail: false,
            switched: AtomicBool::new(false),
        };
        assert_eq!(
            transmit(
                &fake,
                message,
                dir.path().to_owned(),
                Duration::from_secs(5)
            )
            .await,
            SendState::Sending
        );
        let sent = fake.sent.lock().unwrap();
        assert!(sent[0].message.audio_message.as_option().is_none());
        assert!(sent[0].message.document_message.as_option().is_some());
    }
}
