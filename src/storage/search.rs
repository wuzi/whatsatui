use super::{StoreError, merge, records::rows};
use crate::app::model::*;
use diesel::{SqliteConnection, sql_types::Text};
use unicode_segmentation::UnicodeSegmentation;

#[diesel::declare_sql_function]
extern "SQL" {
    fn wt_lower(text: Text) -> Text;
}

pub(super) fn register(connection: &mut SqliteConnection) -> Result<(), StoreError> {
    wt_lower_utils::register_impl(connection, |text: String| text.to_lowercase())?;
    Ok(())
}

pub(super) fn messages(
    connection: &mut SqliteConnection,
    account: &AccountId,
    chat: &ChatId,
    query: &str,
    now_ms: i64,
) -> Result<MessageSearchPage, StoreError> {
    if query.chars().count() > 256 {
        return Err(StoreError::InvalidData);
    }
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Ok(MessageSearchPage::default());
    }
    let chat = merge::canonical(connection, account, &chat.0)?;
    // Read current bodies directly: edits, alias merges, and tombstones cannot
    // leave a separate search index containing obsolete message text.
    let mut messages: Vec<MessageRecord> = rows(connection,
        "SELECT data FROM messages WHERE account=? AND chat=?
         AND (json_extract(data,'$.expires_at_ms') IS NULL OR json_extract(data,'$.expires_at_ms') > CAST(? AS INTEGER))
         AND instr(wt_lower(coalesce(json_extract(data,'$.body.Text'), json_extract(data,'$.body.Unsupported.caption'), json_extract(data,'$.body.Media.caption'), '')), ?) > 0
         ORDER BY created_at_ms DESC,key DESC LIMIT 51",
        &[&account.0, &chat, &now_ms.to_string(), &query])?;
    let has_more = messages.len() > 50;
    messages.truncate(50);
    let hits = messages
        .into_iter()
        .filter_map(|message| {
            let text = match message.body {
                MessageBody::Text(text) => text,
                MessageBody::Media(attachment) => attachment.caption?,
                MessageBody::Unsupported {
                    caption: Some(text),
                    ..
                } => text,
                _ => return None,
            };
            let (preview, match_grapheme) = excerpt(&text, &query);
            Some(MessageSearchHit {
                key: message.key,
                created_at_ms: message.created_at_ms,
                preview,
                match_grapheme,
            })
        })
        .collect();
    Ok(MessageSearchPage { hits, has_more })
}

fn excerpt(text: &str, query: &str) -> (String, usize) {
    let match_byte = text.to_lowercase().find(query).unwrap_or(0);
    let graphemes: Vec<_> = text.graphemes(true).collect();
    let mut folded_offset = 0;
    let match_index = graphemes
        .iter()
        .position(|g| {
            folded_offset += g.to_lowercase().len();
            folded_offset > match_byte
        })
        .unwrap_or(0);
    let start = match_index.saturating_sub(36);
    let end = (start + 160).min(graphemes.len());
    let preview = format!(
        "{}{}{}",
        if start > 0 { "…" } else { "" },
        graphemes[start..end].concat(),
        if end < graphemes.len() { "…" } else { "" }
    );
    (preview, match_index - start + usize::from(start > 0))
}
