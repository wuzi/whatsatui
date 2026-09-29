use crate::media::{Attachment, AttachmentKind};
use whatsapp_rust::prelude::{MessageExt, wa};

pub(super) fn attachment(payload: &wa::Message) -> Option<Attachment> {
    if payload.is_view_once() {
        return None;
    }
    let base = payload.get_base_message();
    let attachment = if let Some(image) = base.image_message.as_option() {
        Attachment {
            kind: AttachmentKind::Image,
            filename: None,
            mime: image.mimetype.clone(),
            caption: image.caption.clone(),
            size: image.file_length?,
            direct_path: image.direct_path.clone()?,
            media_key: image.media_key.as_deref()?.try_into().ok()?,
            sha256: image.file_sha256.as_deref()?.try_into().ok()?,
            encrypted_sha256: image.file_enc_sha256.as_deref()?.try_into().ok()?,
        }
    } else if let Some(document) = base.document_message.as_option() {
        Attachment {
            kind: AttachmentKind::Document,
            filename: document.file_name.clone(),
            mime: document.mimetype.clone(),
            caption: document.caption.clone(),
            size: document.file_length?,
            direct_path: document.direct_path.clone()?,
            media_key: document.media_key.as_deref()?.try_into().ok()?,
            sha256: document.file_sha256.as_deref()?.try_into().ok()?,
            encrypted_sha256: document.file_enc_sha256.as_deref()?.try_into().ok()?,
        }
    } else {
        return None;
    };
    attachment.validate().ok()?;
    Some(attachment)
}
