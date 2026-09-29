use super::{StoreError, merge, paths, records::*};

use crate::app::model::*;

use diesel::{connection::SimpleConnection, prelude::*};

use std::{collections::BTreeSet, path::Path};

use unicode_segmentation::UnicodeSegmentation;

pub(super) fn open(path: &Path) -> Result<SqliteConnection, StoreError> {
    if let Some(parent) = path.parent() {
        paths::private_dir(parent)?;
    }

    paths::private_file(path)?;

    let mut c = SqliteConnection::establish(path.to_str().ok_or(StoreError::InvalidData)?)?;
    super::search::register(&mut c)?;

    c.batch_execute("PRAGMA busy_timeout=5000; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;")?;

    #[derive(QueryableByName)]
    struct Version {
        #[diesel(sql_type=diesel::sql_types::Integer)]
        user_version: i32,
    }

    let version = diesel::sql_query("PRAGMA user_version")
        .get_result::<Version>(&mut c)?
        .user_version;

    if version > 2 {
        return Err(StoreError::InvalidData);
    }

    c.transaction::<_, StoreError, _>(|c| {
        if version == 0 {
            c.batch_execute(include_str!(
                "../../migrations/00000000000001_initial/up.sql"
            ))?;
        }

        if version < 2 {
            c.batch_execute(include_str!(
                "../../migrations/00000000000002_draft_barrier/up.sql"
            ))?;
        }

        Ok(())
    })?;

    Ok(c)
}

pub(super) fn default_chat(a: &AccountId, chat: &ChatId) -> ChatSummary {
    ChatSummary {
        account: a.clone(),
        chat: chat.clone(),
        name: chat.0.clone(),
        is_group: chat.0.ends_with("@g.us"),
        ..Default::default()
    }
}

pub(super) fn ensure_chat(
    c: &mut SqliteConnection,
    a: &AccountId,
    chat: &ChatId,
) -> Result<(), StoreError> {
    execute(
        c,
        "INSERT OR IGNORE INTO chats(account,chat,data) VALUES(?,?,?)",
        &[&a.0, &chat.0, &json(&default_chat(a, chat))?],
    )?;

    Ok(())
}

pub(super) fn draft(
    c: &mut SqliteConnection,
    a: &AccountId,
    chat: &ChatId,
) -> Result<Draft, StoreError> {
    let chat = ChatId(merge::canonical(c, a, &chat.0)?);

    Ok(rows(
        c,
        "SELECT data FROM drafts WHERE account=? AND chat=?",
        &[&a.0, &chat.0],
    )?
    .pop()
    .unwrap_or_default())
}

pub(super) fn save_draft(
    c: &mut SqliteConnection,
    a: &AccountId,
    chat: &ChatId,
    d: &Draft,
) -> Result<(), StoreError> {
    let chat = ChatId(merge::canonical(c, a, &chat.0)?);

    let mut d = d.clone();

    if let Some(q) = &mut d.reply {
        merge::quote(c, q)?;
    }

    if d.revision > i64::MAX as u64
        || d.reply
            .as_ref()
            .is_some_and(|q| q.key.account != *a || q.key.chat != chat)
    {
        return Err(StoreError::InvalidData);
    }

    ensure_chat(c, a, &chat)?;

    execute(
        c,
        "INSERT INTO drafts(account,chat,revision,data) VALUES(?,?,?,?) ON CONFLICT(account,chat) DO UPDATE SET revision=excluded.revision,data=excluded.data,cleared_from=NULL WHERE excluded.revision > drafts.revision OR (excluded.revision = drafts.revision AND drafts.cleared_from IS NOT NULL AND excluded.revision > drafts.cleared_from)",
        &[&a.0, &chat.0, &d.revision.to_string(), &json(&d)?],
    )?;

    Ok(())
}

pub(super) fn get(
    c: &mut SqliteConnection,
    key: &MessageKey,
) -> Result<Option<MessageRecord>, StoreError> {
    let key = merge::canonical_key(c, key)?;

    Ok(rows(c, "SELECT data FROM messages WHERE key=?", &[&json(&key)?])?.pop())
}

pub(super) fn put(
    c: &mut SqliteConnection,
    m: &MessageRecord,
    unread: bool,
) -> Result<(), StoreError> {
    ensure_chat(c, &m.key.account, &m.key.chat)?;

    execute(
        c,
        "INSERT INTO messages(key,account,chat,sender,message_id,from_me,created_at_ms,data,unread) VALUES(?,?,?,?,?,?,?,?,?) ON CONFLICT(key) DO UPDATE SET data=excluded.data,created_at_ms=excluded.created_at_ms",
        &[
            &json(&m.key)?,
            &m.key.account.0,
            &m.key.chat.0,
            &m.key.sender.0,
            &m.key.id.0,
            if m.key.from_me { "1" } else { "0" },
            &m.created_at_ms.to_string(),
            &json(m)?,
            if unread { "1" } else { "0" },
        ],
    )?;

    Ok(())
}

pub(super) fn monotonic(old: Option<SendState>, new: SendState) -> SendState {
    let rank = |s| match s {
        SendState::Read => 4,
        SendState::Delivered => 3,
        SendState::Sent => 2,
        SendState::Failed => 1,
        _ => 0,
    };

    match old {
        Some(o) if new == SendState::Sending => o,
        Some(o) if rank(o) > rank(new) => o,
        _ => new,
    }
}

pub(super) fn apply(
    c: &mut SqliteConnection,
    batch: MessageBatch,
) -> Result<StoreChange, StoreError> {
    c.transaction::<_, StoreError, _>(|c| {
        let mut chats = BTreeSet::new();

        for change in batch.changes {
            let raw_key = match &change {
                MessageChange::Upsert(m) => &m.key,
                MessageChange::Edit { key, .. }
                | MessageChange::Delete { key }
                | MessageChange::Expire { key } => key,
            };

            if raw_key.account != batch.account {
                return Err(StoreError::InvalidData);
            }
            let key = merge::canonical_key(c, raw_key)?;
            chats.insert(key.chat.clone());

            match change {
                MessageChange::Upsert(mut m) => {
                    m.key = key.clone();
                    if let Some(q) = &mut m.quote {
                        if q.key.account != batch.account {
                            return Err(StoreError::InvalidData);
                        }
                        merge::quote(c, q)?;
                    }

                    if let Some(old) = get(c, &key)? {
                        if matches!(old.body, MessageBody::Expired)
                            || matches!(old.body, MessageBody::Deleted)
                                && !matches!(m.body, MessageBody::Expired)
                            || !matches!(m.body, MessageBody::Deleted | MessageBody::Expired)
                                && old.edited_at_ms > m.edited_at_ms
                        {
                            m.body = old.body;
                            m.edited_at_ms = old.edited_at_ms;
                        }

                        m.expires_at_ms = m.expires_at_ms.or(old.expires_at_ms);
                        m.send_state = m
                            .send_state
                            .map(|s| monotonic(old.send_state, s))
                            .or(old.send_state);
                    }

                    if let Some(MessageChange::Edit {
                        text, edited_at_ms, ..
                    }) = rows::<MessageChange>(
                        c,
                        "SELECT data FROM mutations WHERE key=?",
                        &[&json(&key)?],
                    )?
                    .pop()
                    {
                        if !matches!(m.body, MessageBody::Deleted | MessageBody::Expired)
                            && m.edited_at_ms.unwrap_or(0) < edited_at_ms
                        {
                            m.body = MessageBody::Text(text);
                            m.edited_at_ms = Some(edited_at_ms);
                        }

                        execute(c, "DELETE FROM mutations WHERE key=?", &[&json(&key)?])?;
                    }

                    let receipts: Vec<Receipt> =
                        rows(c, "SELECT data FROM receipts WHERE key=?", &[&json(&key)?])?;

                    for receipt in receipts {
                        let state = if key.chat.0.ends_with("@g.us") {
                            SendState::Sent
                        } else if receipt.state == ReceiptState::Read {
                            SendState::Read
                        } else {
                            SendState::Delivered
                        };
                        m.send_state = Some(monotonic(m.send_state, state));
                    }

                    if matches!(m.body, MessageBody::Deleted | MessageBody::Expired) {
                        m.quote = None;
                    }

                    let unread = batch.source == MessageSource::Live && merge::unread(c, &m)?;
                    put(c, &m, unread)?;

                    if matches!(m.body, MessageBody::Deleted | MessageBody::Expired) {
                        let state = if matches!(m.body, MessageBody::Expired) {
                            QuoteAvailability::Expired
                        } else {
                            QuoteAvailability::Deleted
                        };
                        chats.extend(merge::scrub_quotes(c, &batch.account, &key, state)?);
                    }
                }

                MessageChange::Edit {
                    text, edited_at_ms, ..
                } => {
                    if let Some(mut m) = get(c, &key)? {
                        if !matches!(m.body, MessageBody::Deleted | MessageBody::Expired)
                            && m.edited_at_ms.unwrap_or(0) < edited_at_ms
                        {
                            m.body = MessageBody::Text(text);
                            m.edited_at_ms = Some(edited_at_ms);
                            put(c, &m, false)?;
                        }
                    } else {
                        let previous = rows::<MessageChange>(
                            c,
                            "SELECT data FROM mutations WHERE key=?",
                            &[&json(&key)?],
                        )?
                        .pop();
                        if !matches!(previous,Some(MessageChange::Edit{
edited_at_ms:at,..}
)if at>=edited_at_ms)
                        {
                            execute(
                                c,
                                "INSERT OR REPLACE INTO mutations(key,account,data) VALUES(?,?,?)",
                                &[
                                    &json(&key)?,
                                    &batch.account.0,
                                    &json(&MessageChange::Edit {
                                        key: key.clone(),
                                        text,
                                        edited_at_ms,
                                    })?,
                                ],
                            )?;
                        }
                    }
                }

                MessageChange::Delete { .. } | MessageChange::Expire { .. } => {
                    let expired = matches!(change, MessageChange::Expire { .. });
                    let mut m = get(c, &key)?.unwrap_or(MessageRecord {
                        key: key.clone(),
                        body: MessageBody::Deleted,
                        quote: None,
                        created_at_ms: 0,
                        edited_at_ms: None,
                        expires_at_ms: None,
                        send_state: None,
                    });

                    m.body = if expired || matches!(m.body, MessageBody::Expired) {
                        MessageBody::Expired
                    } else {
                        MessageBody::Deleted
                    };
                    m.quote = None;
                    put(c, &m, false)?;
                    execute(
                        c,
                        "UPDATE messages SET unread=0 WHERE key=?",
                        &[&json(&key)?],
                    )?;

                    chats.extend(merge::scrub_quotes(
                        c,
                        &batch.account,
                        &key,
                        if matches!(m.body, MessageBody::Expired) {
                            QuoteAvailability::Expired
                        } else {
                            QuoteAvailability::Deleted
                        },
                    )?);
                }
            }
        }

        Ok(StoreChange {
            account: batch.account,
            chats: chats.into_iter().collect(),
        })
    })
}

pub(super) fn stage(
    c: &mut SqliteConnection,
    o: OutboundText,
    consume_draft: bool,
) -> Result<(), StoreError> {
    c.transaction::<_, StoreError, _>(|c| {
        let mut o = o;

        o.key = merge::canonical_key(c, &o.key)?;

        if let Some(q) = &mut o.draft.reply {
            merge::quote(c, q)?;
        }

        if !o.key.from_me || o.draft.text.trim().is_empty() || get(c, &o.key)?.is_some() {
            return Err(StoreError::InvalidData);
        }

        // First materialize the captured revision. A newer saved draft wins.
        if consume_draft {
            save_draft(c, &o.key.account, &o.key.chat, &o.draft)?;
        }

        let current = draft(c, &o.key.account, &o.key.chat)?;

        let m = MessageRecord {
            key: o.key.clone(),
            body: MessageBody::Text(o.draft.text),
            quote: o.draft.reply,
            created_at_ms: o.created_at_ms,
            edited_at_ms: None,
            expires_at_ms: None,
            send_state: Some(SendState::Sending),
        };

        put(c, &m, false)?;

        if consume_draft && current.revision == o.draft.revision {
            save_draft(
                c,
                &o.key.account,
                &o.key.chat,
                &Draft {
                    revision: current
                        .revision
                        .checked_add(1)
                        .ok_or(StoreError::InvalidData)?,
                    ..Default::default()
                },
            )?;

            execute(
                c,
                "UPDATE drafts SET cleared_from=? WHERE account=? AND chat=?",
                &[
                    &current.revision.to_string(),
                    &o.key.account.0,
                    &o.key.chat.0,
                ],
            )?;
        }

        Ok(())
    })
}

pub(super) fn set_state(
    c: &mut SqliteConnection,
    key: &MessageKey,
    state: SendState,
) -> Result<StoreChange, StoreError> {
    let key = merge::canonical_key(c, key)?;

    if let Some(mut m) = get(c, &key)? {
        m.send_state = Some(monotonic(m.send_state, state));

        put(c, &m, false)?;
    }

    Ok(StoreChange {
        account: key.account.clone(),
        chats: vec![key.chat.clone()],
    })
}

pub(super) fn recover(c: &mut SqliteConnection, a: &AccountId) -> Result<(), StoreError> {
    c.transaction::<_,StoreError,_>(|c| {

        let pending: Vec<MessageRecord> = rows(c,"SELECT data FROM messages WHERE account=? AND json_extract(data,'$.send_state')='Sending'", &[&a.0])?;

        for m in pending {
 set_state(c,&m.key,SendState::Unconfirmed)?;
 }
 Ok(())
    }
)
}

pub(super) fn receipt(
    c: &mut SqliteConnection,
    mut receipt: Receipt,
) -> Result<StoreChange, StoreError> {
    c.transaction::<_, StoreError, _>(|c| {
        receipt.key = merge::canonical_key(c, &receipt.key)?;

        receipt.recipient = merge::canonical(c, &receipt.key.account, &receipt.recipient.0)?.into();

        let old: Option<Receipt> = rows(
            c,
            "SELECT data FROM receipts WHERE key=? AND recipient=?",
            &[&json(&receipt.key)?, &receipt.recipient.0],
        )?
        .pop();

        if let Some(old) = old
            && old.state > receipt.state
        {
            receipt = old;
        }

        execute(
            c,
            "INSERT OR REPLACE INTO receipts(key,account,chat,recipient,data) VALUES(?,?,?,?,?)",
            &[
                &json(&receipt.key)?,
                &receipt.key.account.0,
                &receipt.key.chat.0,
                &receipt.recipient.0,
                &json(&receipt)?,
            ],
        )?;

        let state = if receipt.key.chat.0.ends_with("@g.us") {
            SendState::Sent
        } else if receipt.state == ReceiptState::Read {
            SendState::Read
        } else {
            SendState::Delivered
        };

        set_state(c, &receipt.key, state)
    })
}

pub(super) fn upsert_chats(
    c: &mut SqliteConnection,
    a: &AccountId,
    chats: Vec<ChatSummary>,
) -> Result<StoreChange, StoreError> {
    c.transaction::<_, StoreError, _>(|c| upsert_chats_in_transaction(c, a, chats))
}

fn upsert_chats_in_transaction(
    c: &mut SqliteConnection,
    a: &AccountId,
    chats: Vec<ChatSummary>,
) -> Result<StoreChange, StoreError> {
    let mut ids = Vec::new();
    for mut chat in chats {
        if chat.account != *a {
            return Err(StoreError::InvalidData);
        }
        let original = chat.chat.clone();
        chat.chat = merge::canonical(c, a, &chat.chat.0)?.into();

        if let Some(old) = rows::<ChatSummary>(
            c,
            "SELECT data FROM chats WHERE account=? AND chat=?",
            &[&a.0, &chat.chat.0],
        )?
        .pop()
        {
            if chat.name.is_empty()
                || chat.name == original.0
                || chat.name_priority < old.name_priority
            {
                chat.name = old.name;
                chat.name_priority = old.name_priority;
            }
            chat.phone = chat.phone.or(old.phone);
        }

        merge::baseline(c, &chat)?;
        chat.unread = 0;
        execute(
            c,
            "INSERT INTO chats(account,chat,data) VALUES(?,?,?) ON CONFLICT(account,chat) DO UPDATE SET data=excluded.data",
            &[&a.0, &chat.chat.0, &json(&chat)?],
        )?;
        ids.push(chat.chat);
    }
    Ok(StoreChange {
        account: a.clone(),
        chats: ids,
    })
}

pub(super) fn preview(body: &MessageBody) -> String {
    let text = match body {
        MessageBody::Text(t) => t.clone(),
        MessageBody::Media(attachment) => format!(
            "[{}] {}",
            attachment.kind.label(),
            attachment
                .caption
                .as_deref()
                .or(attachment.filename.as_deref())
                .unwrap_or("")
        ),
        MessageBody::Unsupported { kind, caption } => {
            format!("[{kind}] {}", caption.as_deref().unwrap_or(""))
        }

        MessageBody::Deleted => "[deleted]".into(),
        MessageBody::Expired => "[expired]".into(),
    };

    text.graphemes(true).take(160).collect()
}

pub(super) fn summary(
    c: &mut SqliteConnection,
    a: &AccountId,
    chat: &ChatId,
) -> Result<ChatSummary, StoreError> {
    let chat = ChatId(merge::canonical(c, a, &chat.0)?);

    let mut s = rows(
        c,
        "SELECT data FROM chats WHERE account=? AND chat=?",
        &[&a.0, &chat.0],
    )?
    .pop()
    .unwrap_or_else(|| default_chat(a, &chat));

    if let Some(m)=rows::<MessageRecord>(c,"SELECT data FROM messages WHERE account=? AND chat=? ORDER BY created_at_ms DESC,key DESC LIMIT 1", &[&a.0,&chat.0])?.pop(){
s.preview=preview(&m.body);
s.latest_at_ms=m.created_at_ms;
}

    let state = merge::read_state(c, a, &chat)?;

    let count: Vec<u32> = rows(
        c,
        "SELECT CAST(COUNT(*) AS TEXT) AS data FROM messages WHERE account=? AND chat=? AND unread=1 AND created_at_ms > ?",
        &[
            &a.0,
            &chat.0,
            &if state.baseline > 0 {
                state.baseline_at_ms
            } else {
                i64::MIN
            }
            .to_string(),
        ],
    )?;

    s.unread = state.baseline.saturating_add(count[0]);

    let d = draft(c, a, &chat)?;

    s.has_draft = !d.text.is_empty() || d.reply.is_some();

    Ok(s)
}

pub(super) fn list(
    c: &mut SqliteConnection,
    a: &AccountId,
) -> Result<Vec<ChatSummary>, StoreError> {
    let chats: Vec<ChatSummary> = rows(c, "SELECT data FROM chats WHERE account=?", &[&a.0])?;

    let mut chats = chats
        .into_iter()
        .map(|s| summary(c, a, &s.chat))
        .collect::<Result<Vec<_>, _>>()?;

    chats.sort_by(|a, b| {
        b.latest_at_ms
            .cmp(&a.latest_at_ms)
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(chats)
}

pub(super) fn snapshot(
    c: &mut SqliteConnection,
    a: &AccountId,
    chat: &ChatId,
    cursor: Option<PageCursor>,
) -> Result<ChatSnapshot, StoreError> {
    let chat = ChatId(merge::canonical(c, a, &chat.0)?);

    let after = cursor
        .as_ref()
        .is_some_and(|p| p.direction == PageDirection::After);

    let mut messages: Vec<MessageRecord> = if let Some(cursor) = cursor {
        let key = merge::canonical_key(c, &cursor.key)?;

        let sql = match cursor.direction {
            PageDirection::Before => {
                "SELECT data FROM messages WHERE account=? AND chat=? AND (created_at_ms < ? OR (created_at_ms = ? AND key < ?)) ORDER BY created_at_ms DESC,key DESC LIMIT 100"
            }

            PageDirection::AtOrBefore => {
                "SELECT data FROM messages WHERE account=? AND chat=? AND (created_at_ms < ? OR (created_at_ms = ? AND key <= ?)) ORDER BY created_at_ms DESC,key DESC LIMIT 100"
            }

            PageDirection::After => {
                "SELECT data FROM messages WHERE account=? AND chat=? AND (created_at_ms > ? OR (created_at_ms = ? AND key > ?)) ORDER BY created_at_ms,key LIMIT 100"
            }
        };

        rows(
            c,
            sql,
            &[
                &a.0,
                &chat.0,
                &cursor.created_at_ms.to_string(),
                &cursor.created_at_ms.to_string(),
                &json(&key)?,
            ],
        )?
    } else {
        rows(
            c,
            "SELECT data FROM messages WHERE account=? AND chat=? ORDER BY created_at_ms DESC,key DESC LIMIT 100",
            &[&a.0, &chat.0],
        )?
    };

    if !after {
        messages.reverse();
    }

    let bound = |c: &mut SqliteConnection,
                 m: &MessageRecord,
                 older: bool|
     -> Result<bool, StoreError> {
        let sql = if older {
            "SELECT CASE WHEN EXISTS(SELECT 1 FROM messages WHERE account=? AND chat=? AND (created_at_ms < ? OR (created_at_ms = ? AND key < ?))) THEN 'true' ELSE 'false' END AS data"
        } else {
            "SELECT CASE WHEN EXISTS(SELECT 1 FROM messages WHERE account=? AND chat=? AND (created_at_ms > ? OR (created_at_ms = ? AND key > ?))) THEN 'true' ELSE 'false' END AS data"
        };

        Ok(rows::<bool>(
            c,
            sql,
            &[
                &a.0,
                &chat.0,
                &m.created_at_ms.to_string(),
                &m.created_at_ms.to_string(),
                &json(&m.key)?,
            ],
        )?[0])
    };

    let has_older = messages
        .first()
        .map(|m| bound(c, m, true))
        .transpose()?
        .unwrap_or(false);

    let has_newer = messages
        .last()
        .map(|m| bound(c, m, false))
        .transpose()?
        .unwrap_or(false);

    let receipts = if messages.is_empty() {
        vec![]
    } else {
        let keys = messages
            .iter()
            .map(|m| json(&m.key))
            .collect::<Result<Vec<_>, _>>()?;

        let sql = format!(
            "SELECT data FROM receipts WHERE key IN ({})",
            std::iter::repeat_n("?", keys.len())
                .collect::<Vec<_>>()
                .join(",")
        );

        let params = keys.iter().map(String::as_str).collect::<Vec<_>>();

        rows(c, &sql, &params)?
    };

    Ok(ChatSnapshot {
        summary: summary(c, a, &chat)?,
        messages,
        draft: draft(c, a, &chat)?,
        receipts,
        has_older,
        has_newer,
    })
}
