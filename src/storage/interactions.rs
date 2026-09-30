use super::{StoreError, merge, records::*, worker};
use crate::app::model::*;
use diesel::SqliteConnection;
use unicode_segmentation::UnicodeSegmentation;

pub(super) fn react(c: &mut SqliteConnection, mut reaction: Reaction) -> Result<(), StoreError> {
    reaction.key = merge::canonical_key(c, &reaction.key)?;
    reaction.reactor = merge::canonical(c, &reaction.key.account, &reaction.reactor.0)?.into();
    if reaction.key.id.0.is_empty()
        || reaction.reactor.0.is_empty()
        || reaction.at_ms < 0
        || reaction.emoji.len() > 64
        || reaction.emoji.chars().any(char::is_control)
        || reaction.emoji.graphemes(true).count() > 1
    {
        return Ok(());
    }
    if worker::get(c, &reaction.key)?
        .is_some_and(|m| matches!(m.body, MessageBody::Deleted | MessageBody::Expired))
    {
        return Ok(());
    }
    let key = json(&reaction.key)?;
    let previous: Option<Reaction> = rows(
        c,
        "SELECT data FROM reactions WHERE key=? AND reactor=?",
        &[&key, &reaction.reactor.0],
    )?
    .pop();
    let order = |r: &Reaction| (r.at_ms, r.emoji.is_empty(), r.event_id.clone());
    if previous
        .as_ref()
        .is_some_and(|old| order(old) >= order(&reaction))
    {
        return Ok(());
    }
    execute(
        c,
        "INSERT OR REPLACE INTO reactions(key,account,reactor,data) VALUES(?,?,?,?)",
        &[
            &key,
            &reaction.key.account.0,
            &reaction.reactor.0,
            &json(&reaction)?,
        ],
    )?;
    Ok(())
}
pub(super) fn snapshot(
    c: &mut SqliteConnection,
    messages: &[MessageRecord],
) -> Result<MessageInteractions, StoreError> {
    let keys = messages
        .iter()
        .filter(|m| !matches!(m.body, MessageBody::Deleted | MessageBody::Expired))
        .map(|m| json(&m.key))
        .collect::<Result<Vec<_>, _>>()?;
    if keys.is_empty() {
        return Ok(MessageInteractions::default());
    }
    let placeholders = std::iter::repeat_n("?", keys.len())
        .collect::<Vec<_>>()
        .join(",");
    let params = keys.iter().map(String::as_str).collect::<Vec<_>>();
    let reactions: Vec<Reaction> = rows(
        c,
        &format!("SELECT data FROM reactions WHERE key IN ({placeholders}) ORDER BY key,reactor"),
        &params,
    )?;
    let mutations = rows(
        c,
        &format!("SELECT data FROM outgoing_mutations WHERE key IN ({placeholders})"),
        &params,
    )?;
    Ok(MessageInteractions {
        reactions: reactions
            .into_iter()
            .filter(|r| !r.emoji.is_empty())
            .collect(),
        mutations,
    })
}
pub(super) fn scrub(c: &mut SqliteConnection, key: &MessageKey) -> Result<(), StoreError> {
    for table in ["reactions", "outgoing_mutations"] {
        execute(
            c,
            &format!("DELETE FROM {table} WHERE key=?"),
            &[&json(key)?],
        )?;
    }
    Ok(())
}
pub(super) fn rekey(
    c: &mut SqliteConnection,
    account: &AccountId,
) -> Result<Vec<ChatId>, StoreError> {
    let reactions: Vec<Reaction> = rows(
        c,
        "SELECT data FROM reactions WHERE account=?",
        &[&account.0],
    )?;
    let mut affected = vec![];
    for reaction in reactions {
        let key = merge::canonical_key(c, &reaction.key)?;
        let reactor = merge::canonical(c, account, &reaction.reactor.0)?;
        if key != reaction.key || reactor != reaction.reactor.0 {
            execute(
                c,
                "DELETE FROM reactions WHERE key=? AND reactor=?",
                &[&json(&reaction.key)?, &reaction.reactor.0],
            )?;
            affected.push(key.chat);
            react(c, reaction)?;
        }
    }
    let attempts: Vec<MutationAttempt> = rows(
        c,
        "SELECT data FROM outgoing_mutations WHERE account=?",
        &[&account.0],
    )?;
    for mut attempt in attempts {
        let key = merge::canonical_key(c, &attempt.target.key)?;
        if key != attempt.target.key {
            execute(
                c,
                "DELETE FROM outgoing_mutations WHERE key=?",
                &[&json(&attempt.target.key)?],
            )?;
            attempt.target.key = key;
            let other: Option<MutationAttempt> = rows(
                c,
                "SELECT data FROM outgoing_mutations WHERE key=?",
                &[&json(&attempt.target.key)?],
            )?
            .pop();
            if other
                .as_ref()
                .is_none_or(|a| a.created_at_ms < attempt.created_at_ms)
            {
                if let Some(q) = &mut attempt.target.quote {
                    merge::quote(c, q)?;
                }
                affected.push(attempt.target.key.chat.clone());
                save_attempt(c, &attempt)?;
            }
        }
    }
    Ok(affected)
}

fn save_attempt(c: &mut SqliteConnection, attempt: &MutationAttempt) -> Result<(), StoreError> {
    execute(
        c,
        "INSERT INTO outgoing_mutations(key,account,id,data) VALUES(?,?,?,?) ON CONFLICT(key) DO UPDATE SET id=excluded.id,data=excluded.data",
        &[
            &json(&attempt.target.key)?,
            &attempt.target.key.account.0,
            &attempt.id,
            &json(attempt)?,
        ],
    )?;
    Ok(())
}
pub(super) fn stage(
    c: &mut SqliteConnection,
    mut attempt: MutationAttempt,
    now_ms: i64,
) -> Result<MutationAttempt, StoreError> {
    use diesel::Connection;
    c.transaction(|c| {
        let current = worker::get(c, &attempt.target.key)?
            .ok_or(StoreError::Mutation("Message is no longer available"))?;
        if current.body != attempt.target.body
            || current.edited_at_ms != attempt.target.edited_at_ms
            || current.created_at_ms != attempt.target.created_at_ms
        {
            return Err(StoreError::Mutation("Message changed; reopen its actions"));
        }
        if attempt.id.is_empty()
            || attempt.id == current.key.id.0
            || attempt.state != MutationState::Pending
        {
            return Err(StoreError::InvalidData);
        }
        match &attempt.kind {
            MutationKind::Reaction { emoji }
                if crate::message_actions::can_react(&current, now_ms)
                    && (emoji.is_empty() || emojis::get(emoji).is_some()) => {}
            MutationKind::Edit { text }
                if crate::message_actions::can_edit(&current, now_ms)
                    && !text.trim().is_empty()
                    && text.len() <= 65_536 => {}
            _ => {
                return Err(StoreError::Mutation(
                    "Action unavailable: check ownership, expiry, and the 15-minute edit limit",
                ));
            }
        }
        let old: Option<MutationAttempt> = rows(
            c,
            "SELECT data FROM outgoing_mutations WHERE key=?",
            &[&json(&current.key)?],
        )?
        .pop();
        if old.is_some_and(|a| a.state == MutationState::Pending) {
            return Err(StoreError::Mutation(
                "An action on this message is still pending",
            ));
        }
        attempt.target = current;
        save_attempt(c, &attempt)?;
        Ok(attempt)
    })
}
pub(super) fn finish(
    c: &mut SqliteConnection,
    account: &AccountId,
    id: &str,
    state: MutationState,
) -> Result<StoreChange, StoreError> {
    use diesel::Connection;
    c.transaction(|c| {
        let Some(mut attempt) = rows::<MutationAttempt>(
            c,
            "SELECT data FROM outgoing_mutations WHERE account=? AND id=?",
            &[&account.0, id],
        )?
        .pop() else {
            return Ok(StoreChange {
                account: account.clone(),
                chats: vec![],
            });
        };
        if attempt.state == MutationState::Sent {
            return Ok(StoreChange {
                account: account.clone(),
                chats: vec![attempt.target.key.chat],
            });
        }
        attempt.state = state;
        let key = attempt.target.key.clone();
        if state == MutationState::Sent {
            let change = match &attempt.kind {
                MutationKind::Reaction { emoji } => MessageChange::Reaction(Reaction {
                    key: key.clone(),
                    reactor: account.0.clone().into(),
                    emoji: emoji.clone(),
                    at_ms: attempt.created_at_ms,
                    event_id: attempt.id.clone().into(),
                }),
                MutationKind::Edit { text } => MessageChange::Edit {
                    key: key.clone(),
                    text: text.clone(),
                    edited_at_ms: attempt.created_at_ms,
                },
            };
            worker::apply(
                c,
                MessageBatch {
                    account: account.clone(),
                    source: MessageSource::History,
                    changes: vec![change],
                },
            )?;
        }
        save_attempt(c, &attempt)?;
        Ok(StoreChange {
            account: account.clone(),
            chats: vec![key.chat],
        })
    })
}
pub(super) fn recover(c: &mut SqliteConnection, account: &AccountId) -> Result<(), StoreError> {
    let pending: Vec<MutationAttempt> = rows(
        c,
        "SELECT data FROM outgoing_mutations WHERE account=? AND json_extract(data,'$.state')='Pending'",
        &[&account.0],
    )?;
    for mut attempt in pending {
        attempt.state = MutationState::Unconfirmed;
        save_attempt(c, &attempt)?;
    }
    Ok(())
}
