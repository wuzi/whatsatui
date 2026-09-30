use super::encode::{self, EncodedText, UploadedImage};
use crate::{
    app::model::*,
    media::{outgoing, preview},
};
use std::{path::PathBuf, time::Duration};
use whatsapp_rust::{Client, prelude::SendError};

#[async_trait::async_trait]
trait Transport: Sync {
    fn account(&self) -> Option<AccountId>;
    async fn upload(&self, bytes: Vec<u8>) -> Result<UploadedImage, ()>;
    async fn send(&self, encoded: EncodedText) -> Result<(), SendError>;
}
struct Native<'a>(&'a Client);
#[async_trait::async_trait]
impl Transport for Native<'_> {
    fn account(&self) -> Option<AccountId> {
        super::native::account(self.0)
    }
    async fn upload(&self, bytes: Vec<u8>) -> Result<UploadedImage, ()> {
        self.0
            .upload(
                bytes,
                whatsapp_rust::wacore::download::MediaType::Image,
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
        let Some(local) = message.draft.attachment.clone() else {
            return SendState::Failed;
        };
        let expected = local.clone();
        let Ok(permit) = preview::DECODERS.acquire().await else {
            return SendState::Failed;
        };
        let prepared = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let bytes = outgoing::read(&local, &data_dir)?;
            let image = preview::decode(&bytes)?.thumbnail(160, 160).to_rgb8();
            let mut thumbnail = Vec::new();
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut thumbnail, 70)
                .encode_image(&image)
                .map_err(|_| "Cannot make thumbnail")?;
            Ok::<_, String>((bytes, thumbnail))
        })
        .await;
        let Ok(Ok((bytes, thumbnail))) = prepared else {
            return SendState::Failed;
        };
        let Ok(uploaded) = transport.upload(bytes).await else {
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
        let Ok(encoded) = encode::encode_image(&message, uploaded, thumbnail) else {
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
        stall_send: bool,
        switched: AtomicBool,
        switch_on_upload: bool,
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
        async fn upload(&self, bytes: Vec<u8>) -> Result<UploadedImage, ()> {
            use sha2::{Digest, Sha256};
            assert_eq!(
                image::guess_format(&bytes).unwrap(),
                image::ImageFormat::Jpeg
            );
            if self.fail {
                return Err(());
            }
            if self.switch_on_upload {
                self.switched.store(true, Ordering::SeqCst);
            }
            Ok(UploadedImage {
                url: "https://mmg.whatsapp.net/v/test".into(),
                media_key_timestamp: 123,
                attachment: crate::media::Attachment {
                    kind: crate::media::AttachmentKind::Image,
                    filename: None,
                    mime: Some("image/jpeg".into()),
                    caption: None,
                    size: bytes.len() as u64,
                    direct_path: "/v/test".into(),
                    media_key: [1; 32],
                    sha256: Sha256::digest(bytes).into(),
                    encrypted_sha256: [2; 32],
                },
            })
        }
        async fn send(&self, encoded: EncodedText) -> Result<(), SendError> {
            self.sent.lock().unwrap().push(encoded);
            if self.stall_send {
                std::future::pending().await
            } else {
                Ok(())
            }
        }
    }
    #[tokio::test]
    async fn verified_snapshot_uploads_once_and_failures_never_send_caption_as_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.webp");
        std::fs::write(&path, include_bytes!("../../tests/fixtures/sticker.webp")).unwrap();
        let local = outgoing::import(&path, dir.path()).unwrap();
        let message = OutboundText {
            key: MessageKey {
                account: "self@s.whatsapp.net".into(),
                chat: "120363000000001@g.us".into(),
                sender: "self@s.whatsapp.net".into(),
                id: "test-image-id".into(),
                from_me: true,
            },
            draft: Draft {
                text: "Caption 😀".into(),
                attachment: Some(local),
                ..Default::default()
            },
            created_at_ms: 0,
        };
        for (fail, stall, switch, expected, count) in [
            (false, false, false, SendState::Sending, 1),
            (true, false, false, SendState::Failed, 0),
            (false, true, false, SendState::Unconfirmed, 1),
            (false, false, true, SendState::Failed, 0),
        ] {
            let fake = Fake {
                sent: Mutex::new(vec![]),
                fail,
                stall_send: stall,
                switched: AtomicBool::new(false),
                switch_on_upload: switch,
            };
            assert_eq!(
                transmit(
                    &fake,
                    message.clone(),
                    dir.path().to_owned(),
                    Duration::from_millis(100)
                )
                .await,
                expected
            );
            let sent = fake.sent.lock().unwrap();
            assert_eq!(sent.len(), count);
            if let Some(sent) = sent.first() {
                assert_eq!(sent.options.message_id.as_deref(), Some("test-image-id"));
                assert_eq!(
                    sent.message
                        .image_message
                        .as_option()
                        .unwrap()
                        .caption
                        .as_deref(),
                    Some("Caption 😀")
                );
            }
        }
        std::fs::remove_file(
            outgoing::path(message.draft.attachment.as_ref().unwrap(), dir.path()).unwrap(),
        )
        .unwrap();
        let fake = Fake {
            sent: Mutex::new(vec![]),
            fail: false,
            stall_send: false,
            switched: AtomicBool::new(false),
            switch_on_upload: false,
        };
        assert_eq!(
            transmit(
                &fake,
                message,
                dir.path().to_owned(),
                Duration::from_secs(1)
            )
            .await,
            SendState::Failed
        );
        assert!(fake.sent.lock().unwrap().is_empty());
    }
}
