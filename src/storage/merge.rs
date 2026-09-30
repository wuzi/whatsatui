use super::{StoreError, records::*, worker};

use crate::app::model::*;

use diesel::prelude::*;

use serde::{Deserialize, Serialize};

use std::collections::BTreeSet;

#[derive(Default, Serialize, Deserialize)]
pub(super) struct ReadState {
    pub baseline: u32,
    pub baseline_at_ms: i64,
    pub initialized: bool,
    pub watermark: Option<(i64, String)>,
}

pub(super) fn read_state(
    c: &mut SqliteConnection,
    a: &AccountId,
    chat: &ChatId,
) -> Result<ReadState, StoreError> {
    Ok(rows(
        c,
        "SELECT data FROM read_state WHERE account=? AND chat=?",
        &[&a.0, &chat.0],
    )?
    .pop()
    .unwrap_or_default())
}

fn save_read(
    c: &mut SqliteConnection,
    a: &AccountId,
    chat: &ChatId,
    state: &ReadState,
) -> Result<(), StoreError> {
    execute(
        c,
        "INSERT INTO read_state(account,chat,data) VALUES(?,?,?) ON CONFLICT(account,chat) DO UPDATE SET data=excluded.data",
        &[&a.0, &chat.0, &json(state)?],
    )?;

    Ok(())
}

pub(super) fn canonical(
    c: &mut SqliteConnection,
    a: &AccountId,
    value: &str,
) -> Result<String, StoreError> {
    let mut value = value.to_owned();

    let mut seen = BTreeSet::new();

    while seen.insert(value.clone()) {
        let next: Option<String> = rows(
            c,
            "SELECT json_quote(canonical) AS data FROM aliases WHERE account=? AND alias=?",
            &[&a.0, &value],
        )?
        .pop();

        match next {
            Some(next) if next != value => value = next,
            _ => return Ok(value),
        }
    }

    Err(StoreError::InvalidData)
}

pub(super) fn canonical_key(
    c: &mut SqliteConnection,
    k: &MessageKey,
) -> Result<MessageKey, StoreError> {
    let mut key = k.clone();

    key.chat = canonical(c, &key.account, &key.chat.0)?.into();

    key.sender = canonical(c, &key.account, &key.sender.0)?.into();

    Ok(key)
}

pub(super) fn quote(c: &mut SqliteConnection, q: &mut Quote) -> Result<(), StoreError> {
    q.key = canonical_key(c, &q.key)?;

    use unicode_segmentation::UnicodeSegmentation;

    q.preview = q.preview.graphemes(true).take(160).collect();

    if let Some(m) = worker::get(c, &q.key)? {
        match m.body {
            MessageBody::Deleted => {
                q.preview.clear();

                q.availability = QuoteAvailability::Deleted;
            }

            MessageBody::Expired => {
                q.preview.clear();

                q.availability = QuoteAvailability::Expired;
            }

            _ => {}
        }
    }

    Ok(())
}

pub(super) fn unread(c: &mut SqliteConnection, m: &MessageRecord) -> Result<bool, StoreError> {
    if m.key.from_me || matches!(m.body, MessageBody::Deleted | MessageBody::Expired) {
        return Ok(false);
    }

    let state = read_state(c, &m.key.account, &m.key.chat)?;

    Ok(state
        .watermark
        .is_none_or(|w| (m.created_at_ms, json(&m.key).unwrap_or_default()) > w))
}

pub(super) fn baseline(c: &mut SqliteConnection, chat: &ChatSummary) -> Result<(), StoreError> {
    let mut state = read_state(c, &chat.account, &chat.chat)?;

    if !state.initialized && (chat.unread > 0 || chat.latest_at_ms > 0) {
        state.initialized = true;

        state.baseline = chat.unread;

        state.baseline_at_ms = chat.latest_at_ms;

        if state.baseline_at_ms == 0 {
            state.baseline_at_ms = chrono::Utc::now().timestamp_millis();
        }

        save_read(c, &chat.account, &chat.chat, &state)?;
    }

    Ok(())
}

pub(super) fn mark_read(
    c: &mut SqliteConnection,
    a: &AccountId,
    chat: &ChatId,
    keys: Vec<MessageKey>,
) -> Result<StoreChange, StoreError> {
    c.transaction::<_, StoreError, _>(|c| {
        let chat = ChatId(canonical(c, a, &chat.0)?);
        let mut state = read_state(c, a, &chat)?;
        let mut watermark = state.watermark.take();
        // Only identities actually visible when the read action was captured may
        // advance the watermark. An empty capture must not read a later arrival.
        for key in &keys {
            let key = canonical_key(c, key)?;
            if key.account != *a || key.chat != chat {
                return Err(StoreError::InvalidData);
            }
            if let Some(m) = worker::get(c, &key)? {
                let next = (m.created_at_ms, json(&key)?);
                if watermark.as_ref().is_none_or(|old| &next > old) {
                    watermark = Some(next);
                }
            }
        }
        state.watermark = watermark;
        state.baseline = 0;
        state.initialized = true;
        save_read(c, a, &chat, &state)?;
        if let Some((at, key)) = &state.watermark {
            execute(
                c,
                "UPDATE messages SET unread=0 WHERE account=? AND chat=? AND (created_at_ms < ? OR (created_at_ms = ? AND key <= ?))",
                &[&a.0, &chat.0, &at.to_string(), &at.to_string(), key],
            )?;
        }
        Ok(StoreChange { account: a.clone(), chats: vec![chat] })
    })
}
pub(super) fn scrub_quotes(
    c: &mut SqliteConnection,
    a: &AccountId,
    target: &MessageKey,
    availability: QuoteAvailability,
) -> Result<Vec<ChatId>, StoreError> {
    let mut chats = BTreeSet::new();

    let messages: Vec<MessageRecord> = rows(
        c,
        "SELECT data FROM messages WHERE account=? AND json_extract(data,'$.quote.key.id')=?",
        &[&a.0, &target.id.0],
    )?;

    for mut m in messages {
        if m.quote.as_ref().is_some_and(|q| q.key == *target) {
            let q = m.quote.as_mut().unwrap();

            q.preview.clear();

            q.availability = availability.clone();

            worker::put(c, &m, false)?;

            chats.insert(m.key.chat);
        }
    }

    #[derive(QueryableByName)]
    struct DraftRow {
        #[diesel(sql_type=diesel::sql_types::Text)]
        chat: String,
        #[diesel(sql_type=diesel::sql_types::Text)]
        data: String,
    }

    let drafts = diesel::sql_query("SELECT chat,data FROM drafts WHERE account=?")
        .bind::<diesel::sql_types::Text, _>(&a.0)
        .load::<DraftRow>(c)?;

    for row in drafts {
        let mut draft: Draft = serde_json::from_str(&row.data)?;

        let mut changed = false;
        for q in draft.quotes_mut().filter(|q| q.key == *target) {
            q.preview.clear();
            q.availability = availability.clone();
            changed = true;
        }
        if changed {
            execute(
                c,
                "UPDATE drafts SET data=? WHERE account=? AND chat=?",
                &[&json(&draft)?, &a.0, &row.chat],
            )?;

            chats.insert(row.chat.into());
        }
    }

    execute(c, "DELETE FROM mutations WHERE key=?", &[&json(target)?])?;

    Ok(chats.into_iter().collect())
}

pub(super) fn expire(
    c: &mut SqliteConnection,
    a: &AccountId,
    now: i64,
) -> Result<StoreChange, StoreError> {
    c.transaction::<_, StoreError, _>(|c| expire_in_transaction(c, a, now))
}

fn expire_in_transaction(
    c: &mut SqliteConnection,
    a: &AccountId,
    now: i64,
) -> Result<StoreChange, StoreError> {
    let expired: Vec<MessageRecord> = rows(
        c,
        "SELECT data FROM messages WHERE account=? AND json_extract(data,'$.expires_at_ms') <= CAST(? AS INTEGER) AND json_extract(data,'$.body') != 'Expired'",
        &[&a.0, &now.to_string()],
    )?;

    let changes = expired
        .into_iter()
        .map(|m| MessageChange::Expire { key: m.key })
        .collect();
    let mut change = worker::apply(
        c,
        MessageBatch {
            account: a.clone(),
            source: MessageSource::History,
            changes,
        },
    )?;
    // One runtime tick handles acknowledgement deadlines without a task per send.
    let pending: Vec<MessageRecord> = rows(
        c,
        "SELECT data FROM messages WHERE account=? AND created_at_ms <= ? AND json_extract(data,'$.send_state')='Sending'",
        &[&a.0, &now.saturating_sub(30_000).to_string()],
    )?;
    for message in pending {
        change
            .chats
            .extend(worker::set_state(c, &message.key, SendState::Unconfirmed)?.chats);
    }
    change.chats.sort();
    change.chats.dedup();
    Ok(change)
}

pub(super) fn merge_alias(
    c: &mut SqliteConnection,
    a: &AccountId,
    alias: &ParticipantId,
    target: &ParticipantId,
) -> Result<StoreChange, StoreError> {
    c.transaction::<_, StoreError, _>(|c| merge_alias_in_transaction(c, a, alias, target))
}

fn merge_alias_in_transaction(
    c: &mut SqliteConnection,
    a: &AccountId,
    alias: &ParticipantId,
    target: &ParticipantId,
) -> Result<StoreChange, StoreError> {
    if alias.0.ends_with("@g.us") || target.0.ends_with("@g.us") {
        return Err(StoreError::InvalidData);
    }

    let old = canonical(c, a, &alias.0)?;
    let target = canonical(c, a, &target.0)?;
    if old == target {
        return Ok(StoreChange {
            account: a.clone(),
            chats: vec![],
        });
    }

    // Only upstream-authoritative identity mappings call this operation.
    let old_draft = worker::draft(c, a, &ChatId(old.clone()))?;
    let target_draft = worker::draft(c, a, &ChatId(target.clone()))?;

    let old_read = read_state(c, a, &ChatId(old.clone()))?;
    let mut target_read = read_state(c, a, &ChatId(target.clone()))?;

    let metadata: Vec<ChatSummary> = rows(
        c,
        "SELECT data FROM chats WHERE account=? AND chat=?",
        &[&a.0, &old],
    )?;

    let messages: Vec<MessageRecord> = rows(
        c,
        "SELECT data FROM messages WHERE account=? AND (chat=? OR sender=? OR json_extract(data,'$.quote.key.chat')=? OR json_extract(data,'$.quote.key.sender')=?)",
        &[&a.0, &old, &old, &old, &old],
    )?;

    let receipts: Vec<Receipt> = rows(c, "SELECT data FROM receipts WHERE account=?", &[&a.0])?;

    let mutations: Vec<MessageChange> =
        rows(c, "SELECT data FROM mutations WHERE account=?", &[&a.0])?;

    execute(
        c,
        "INSERT INTO aliases(account,alias,canonical) VALUES(?,?,?) ON CONFLICT(account,alias) DO UPDATE SET canonical=excluded.canonical",
        &[&a.0, &old, &target],
    )?;

    let mut affected = BTreeSet::from([ChatId(old.clone()), ChatId(target.clone())]);

    for mut m in messages {
        let old_key = json(&m.key)?;
        let was_unread: Vec<bool> = rows(
            c,
            "SELECT CASE WHEN unread=1 THEN 'true' ELSE 'false' END AS data FROM messages WHERE key=?",
            &[&old_key],
        )?;

        m.key = canonical_key(c, &m.key)?;
        if let Some(q) = &mut m.quote {
            quote(c, q)?;
        }

        execute(c, "DELETE FROM messages WHERE key=?", &[&old_key])?;

        let key = m.key.clone();
        worker::apply(
            c,
            MessageBatch {
                account: a.clone(),
                source: MessageSource::History,
                changes: vec![MessageChange::Upsert(m)],
            },
        )?;

        if was_unread.first() == Some(&true)
            && let Some(m) = worker::get(c, &key)?
            && unread(c, &m)?
        {
            execute(
                c,
                "UPDATE messages SET unread=1 WHERE key=?",
                &[&json(&key)?],
            )?;
        }

        affected.insert(key.chat);
    }

    // Rekey receipts and mutations after installing the mapping; duplicate keys merge monotonically.
    for mut r in receipts {
        let old_key = json(&r.key)?;
        let old_recipient = r.recipient.0.clone();
        r.key = canonical_key(c, &r.key)?;
        r.recipient = canonical(c, a, &r.recipient.0)?.into();
        if json(&r.key)? != old_key || r.recipient.0 != old_recipient {
            execute(
                c,
                "DELETE FROM receipts WHERE key=? AND recipient=?",
                &[&old_key, &old_recipient],
            )?;
            worker::receipt(c, r)?;
        }
    }

    for mutation in mutations {
        let old_key = match &mutation {
            MessageChange::Edit { key, .. }
            | MessageChange::Delete { key }
            | MessageChange::Expire { key } => key,
            MessageChange::Upsert(m) => &m.key,
        };
        let new_key = canonical_key(c, old_key)?;
        if new_key != *old_key {
            execute(c, "DELETE FROM mutations WHERE key=?", &[&json(old_key)?])?;
            worker::apply(
                c,
                MessageBatch {
                    account: a.clone(),
                    source: MessageSource::History,
                    changes: vec![mutation],
                },
            )?;
        }
    }

    for mut chat in metadata {
        chat.chat = target.clone().into();
        worker::upsert_chats(c, a, vec![chat])?;
    }

    let mut merged = target_draft.clone();
    merged.origin.get_or_insert_with(|| ChatId(target.clone()));
    merged.merge_from(&old_draft, &ChatId(old.clone()), false);
    merged.revision = old_draft
        .revision
        .max(target_draft.revision)
        .saturating_add(1);
    for q in merged.quotes_mut() {
        quote(c, q)?;
    }

    if merged.has_content() {
        worker::save_draft(c, a, &target.clone().into(), &merged)?;
    }

    // Canonicalize quoted identities in every draft, including unrelated group composers.
    #[derive(QueryableByName)]
    struct DraftRow {
        #[diesel(sql_type=diesel::sql_types::Text)]
        chat: String,
        #[diesel(sql_type=diesel::sql_types::Text)]
        data: String,
    }

    for row in diesel::sql_query("SELECT chat,data FROM drafts WHERE account=?")
        .bind::<diesel::sql_types::Text, _>(&a.0)
        .load::<DraftRow>(c)?
    {
        let mut d: Draft = serde_json::from_str(&row.data)?;
        let before = d.clone();
        for q in d.quotes_mut() {
            quote(c, q)?;
        }
        if d != before {
            affected.insert(ChatId(row.chat.clone()));
            execute(
                c,
                "UPDATE drafts SET data=? WHERE account=? AND chat=?",
                &[&json(&d)?, &a.0, &row.chat],
            )?;
        }
    }

    if old_read.watermark > target_read.watermark {
        target_read.watermark = old_read.watermark;
    }
    target_read.baseline = target_read.baseline.max(old_read.baseline);
    target_read.baseline_at_ms = target_read.baseline_at_ms.max(old_read.baseline_at_ms);
    target_read.initialized |= old_read.initialized;

    if let Some((_, serialized)) = &mut target_read.watermark {
        let key: MessageKey = serde_json::from_str(serialized)?;
        *serialized = json(&canonical_key(c, &key)?)?;
    }

    save_read(c, a, &target.clone().into(), &target_read)?;

    execute(
        c,
        "DELETE FROM chats WHERE account=? AND chat=?",
        &[&a.0, &old],
    )?;
    execute(
        c,
        "DELETE FROM drafts WHERE account=? AND chat=?",
        &[&a.0, &old],
    )?;
    execute(
        c,
        "DELETE FROM read_state WHERE account=? AND chat=?",
        &[&a.0, &old],
    )?;

    Ok(StoreChange {
        account: a.clone(),
        chats: affected.into_iter().collect(),
    })
}
