use super::download::WorkerInput;
use super::{AttachmentKind, MAX_FILE_BYTES};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    sync::Arc,
};
use whatsapp_rust::download::{DownloadParams, DownloadWriter, MediaDownloader, MediaType};

// Only the parent launches this internal mode. It never opens an account store.
pub async fn run() -> Result<(), String> {
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read worker input")?;
    if bytes.len() > 64 * 1024 {
        return Err("Worker input exceeds its limit".into());
    }
    let input = serde_json::from_slice(&bytes).map_err(|_| "Invalid worker input")?;
    let downloader = MediaDownloader::with_default_hosts(
        Arc::new(
            whatsapp_rust::http::UreqHttpClient::new().with_max_body_bytes(MAX_FILE_BYTES + 26),
        ),
        Arc::new(whatsapp_rust::TokioRuntime),
    );
    receive(input, &downloader).await
}

async fn receive(input: WorkerInput, downloader: &MediaDownloader) -> Result<(), String> {
    let attachment = input.attachment;
    attachment.validate()?;
    if attachment.size > MAX_FILE_BYTES || !input.destination.is_absolute() {
        return Err("Unsupported attachment transfer".into());
    }
    let meta = fs::symlink_metadata(&input.destination).map_err(|_| "Missing temporary file")?;
    if !meta.is_file() {
        return Err("Invalid temporary file".into());
    }
    let file = OpenOptions::new()
        .write(true)
        .read(true)
        .open(&input.destination)
        .map_err(|_| "Cannot access temporary file")?;
    let writer = LimitedFile {
        file,
        limit: attachment.size,
    };
    let kind = match attachment.kind {
        AttachmentKind::Image => MediaType::Image,
        AttachmentKind::Document => MediaType::Document,
    };
    let params = DownloadParams::encrypted(
        attachment.direct_path.clone(),
        &attachment.media_key,
        &attachment.sha256,
        &attachment.encrypted_sha256,
        attachment.size,
        kind,
    );
    let writer = downloader
        .download_to_writer(&params, writer)
        .await
        .map_err(|_| "Attachment transfer failed")?;
    writer
        .file
        .sync_all()
        .map_err(|_| "Cannot save temporary file")?;
    super::cache::verify(input.destination, &attachment).await
}

struct LimitedFile {
    file: File,
    limit: u64,
}
impl Write for LimitedFile {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if self
            .file
            .stream_position()?
            .saturating_add(bytes.len() as u64)
            > self.limit
        {
            return Err(io::Error::other("Attachment exceeded its declared size"));
        }
        self.file.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}
impl Seek for LimitedFile {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.file.seek(position)
    }
}
impl DownloadWriter for LimitedFile {
    fn truncate(&mut self, length: u64) -> io::Result<()> {
        if length > self.limit {
            return Err(io::Error::other("Attachment exceeds its limit"));
        }
        self.file.set_len(length)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{Attachment, AttachmentKind};
    use std::{io::Cursor, sync::Arc};
    use whatsapp_rust::{
        download::{MediaHost, MediaRoute, MediaType},
        http::{HttpClient, HttpRequest, HttpResponse},
        wacore::net::StreamingHttpResponse,
    };
    struct Http(Vec<u8>);
    #[async_trait::async_trait]
    impl HttpClient for Http {
        async fn execute(&self, _: HttpRequest) -> anyhow::Result<HttpResponse> {
            panic!("must stream")
        }
        fn supports_streaming(&self) -> bool {
            true
        }
        fn execute_streaming(&self, request: HttpRequest) -> anyhow::Result<StreamingHttpResponse> {
            assert!(request.url.starts_with("https://fixture.invalid/"));
            Ok(StreamingHttpResponse {
                status_code: 200,
                body: Box::new(Cursor::new(self.0.clone())),
            })
        }
    }
    #[tokio::test]
    async fn receives_encrypted_images_and_documents_through_the_streaming_boundary() {
        for (kind, media_type) in [
            (AttachmentKind::Image, MediaType::Image),
            (AttachmentKind::Document, MediaType::Document),
        ] {
            let encrypted =
                whatsapp_rust::wacore::upload::encrypt_media(b"hello\n", media_type).unwrap();
            let attachment = Attachment {
                kind,
                filename: None,
                mime: None,
                caption: None,
                size: 6,
                direct_path: "/v/fixture".into(),
                media_key: encrypted.media_key,
                sha256: encrypted.file_sha256,
                encrypted_sha256: encrypted.file_enc_sha256,
            };
            let downloader = MediaDownloader::new(
                Arc::new(Http(encrypted.data_to_upload)),
                Arc::new(whatsapp_rust::TokioRuntime),
                MediaRoute::unauthenticated(vec![MediaHost::new("fixture.invalid")]),
            );
            let file = tempfile::NamedTempFile::new().unwrap();
            receive(
                WorkerInput {
                    attachment: attachment.clone(),
                    destination: file.path().into(),
                },
                &downloader,
            )
            .await
            .unwrap();
            assert_eq!(std::fs::read(file.path()).unwrap(), b"hello\n");
            let mut short = attachment;
            short.size = 1;
            assert!(
                receive(
                    WorkerInput {
                        attachment: short,
                        destination: file.path().into()
                    },
                    &downloader
                )
                .await
                .is_err()
            );
            assert_eq!(std::fs::metadata(file.path()).unwrap().len(), 0);
        }
    }
}
