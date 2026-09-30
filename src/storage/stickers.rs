use super::{StoreError, records::rows};
use crate::app::model::*;
use diesel::SqliteConnection;

pub(super) fn recent(
    c: &mut SqliteConnection,
    account: &AccountId,
    now_ms: i64,
) -> Result<Vec<MessageRecord>, StoreError> {
    let candidates: Vec<MessageRecord> = rows(c,
        "SELECT data FROM messages WHERE account=? AND
         (json_extract(data,'$.body.Media.kind')='Sticker' OR json_type(data,'$.body.LocalImage.image.sticker')='object')
         AND (json_extract(data,'$.expires_at_ms') IS NULL OR json_extract(data,'$.expires_at_ms') > CAST(? AS INTEGER))
         ORDER BY created_at_ms DESC, key DESC LIMIT 512", &[&account.0, &now_ms.to_string()])?;
    let mut seen = std::collections::HashSet::new();
    Ok(candidates
        .into_iter()
        .filter(|m| {
            if &m.key.account != account {
                return false;
            }
            let id = match &m.body {
                MessageBody::Media(a) => a
                    .sha256
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
                MessageBody::LocalImage { image, .. } => image.id.clone(),
                _ => return false,
            };
            seen.insert(id)
        })
        .take(60)
        .collect())
}
