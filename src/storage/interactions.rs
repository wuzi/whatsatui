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
    // Outgoing journal rekeying is handled with the mutation APIs.
    Ok(affected)
}
