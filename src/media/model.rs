use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttachmentKind {
    Image,
    Sticker,
    Document,
}

impl AttachmentKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Image => "image",
            Self::Sticker => "sticker",
            Self::Document => "document",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    pub kind: AttachmentKind,
    pub filename: Option<String>,
    pub mime: Option<String>,
    pub caption: Option<String>,
    pub size: u64,
    pub direct_path: String,
    pub media_key: [u8; 32],
    pub sha256: [u8; 32],
    pub encrypted_sha256: [u8; 32],
}

impl std::fmt::Debug for Attachment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Attachment")
            .field("kind", &self.kind)
            .field("filename", &self.filename)
            .field("mime", &self.mime)
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl Attachment {
    pub fn validate(&self) -> Result<(), String> {
        let path = self.direct_path.split('?').next().unwrap_or_default();
        if self.size == 0
            || self.direct_path.len() > 4096
            || !path.starts_with("/v/")
            || path.split('/').any(|p| matches!(p, "." | ".."))
            || self
                .direct_path
                .chars()
                .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '\\' | '#'))
            || self.filename.as_ref().is_some_and(|s| s.len() > 4096)
            || self.mime.as_ref().is_some_and(|s| s.len() > 128)
        {
            return Err("Attachment reference is incomplete or unsupported".into());
        }
        Ok(())
    }

    pub fn extension(&self) -> Option<&'static str> {
        match self
            .mime
            .as_deref()?
            .split(';')
            .next()?
            .trim()
            .to_ascii_lowercase()
            .as_str()
        {
            "image/jpeg" => Some("jpg"),
            "image/png" => Some("png"),
            "image/gif" => Some("gif"),
            "image/webp" => Some("webp"),
            "application/pdf" => Some("pdf"),
            "text/plain" => Some("txt"),
            "text/csv" => Some("csv"),
            "application/msword" => Some("doc"),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
                Some("docx")
            }
            "application/vnd.ms-excel" => Some("xls"),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => Some("xlsx"),
            "application/vnd.ms-powerpoint" => Some("ppt"),
            "application/vnd.openxmlformats-officedocument.presentationml.presentation" => {
                Some("pptx")
            }
            "application/vnd.oasis.opendocument.text" => Some("odt"),
            "application/vnd.oasis.opendocument.spreadsheet" => Some("ods"),
            _ => None,
        }
    }

    pub fn label(&self) -> String {
        let size = if self.size < 1024 {
            format!("{} B", self.size)
        } else if self.size < 1024 * 1024 {
            format!("{:.1} KiB", self.size as f64 / 1024.0)
        } else {
            format!("{:.1} MiB", self.size as f64 / (1024.0 * 1024.0))
        };
        format!(
            "[{}] {} · {size}",
            self.kind.label(),
            self.filename.as_deref().unwrap_or("Attachment")
        )
    }
}
