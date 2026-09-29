use super::{bridge, durability::DurableInbox, normalize, *};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use whatsapp_rust::wacore_binary::JidExt;
use whatsapp_rust::{
    Client,
    prelude::{Bot, Event, EventHandler, EventInterest, EventKind, SqliteStore},
    types::presence::ReceiptType,
};

pub(super) fn account(client: &Client) -> Option<AccountId> {
    client.pn().map(|j| AccountId(normalize::jid(&j)))
}
struct RawHandler(bridge::Bridge<Arc<Event>>);
impl EventHandler for RawHandler {
    fn handle_event(&self, event: Arc<Event>) {
        let _ = self.0.send(event);
    }
    fn interest(&self) -> EventInterest {
        EventInterest::of(&[
            EventKind::Connected,
            EventKind::Disconnected,
            EventKind::PairSuccess,
            EventKind::PairError,
            EventKind::LoggedOut,
            EventKind::PairingQrCode,
            EventKind::Messages,
            EventKind::Receipt,
            EventKind::ServerAck,
            EventKind::HistorySync,
            EventKind::ContactUpdate,
            EventKind::PushNameUpdate,
            EventKind::GroupUpdate,
            EventKind::OfflineSyncCompleted,
            EventKind::StreamReplaced,
            EventKind::ClientOutdated,
        ])
    }
}
async fn emit(tx: &mpsc::Sender<BackendEvent>, event: BackendEvent) -> Result<(), BackendError> {
    tx.send(event).await.map_err(|_| BackendError::Stopped)
}
fn storage_error(error: crate::storage::StoreError) -> BackendError {
    BackendError::Service(error.into())
}
pub(super) async fn start(
    session_path: PathBuf,
    store: Store,
) -> Result<BackendHandle, BackendError> {
    crate::storage::paths::private_file(&session_path).map_err(storage_error)?;
    let device = SqliteStore::new(session_path.to_str().ok_or(BackendError::InvalidIdentity)?)
        .await
        .map_err(|e| BackendError::Service(e.into()))?;
    let (bridge, raw) = bridge::bounded(128);
    let (notices, mut problems) = tokio::sync::watch::channel(None);
    let bot = Bot::builder()
        .with_backend(device)
        .with_event_handler(RawHandler(bridge))
        .with_inbound_durability_hook(DurableInbox(store.clone(), notices))
        .build()
        .await
        .map_err(|e| BackendError::Service(e.into()))?
        .spawn();
    let client = bot.client();
    let (events, rx) = mpsc::channel(256);
    let (commands, mut requests) = mpsc::channel(32);
    let (stop, mut stopping) = oneshot::channel();
    let finished = Arc::new(AtomicBool::new(false));
    let done = finished.clone();
    let (raw_tx, mut raw_rx) = mpsc::channel(32);
    let forwarding = tokio::task::spawn_blocking(move || {
        loop {
            match raw.recv_timeout(Duration::from_millis(20)) {
                Ok(event) => {
                    if raw_tx.blocking_send(event).is_err() {
                        break;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    if done.load(Ordering::Acquire) {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
    let event_client = client.clone();
    let event_store = store.clone();
    let event_tx = events.clone();
    let ingest = tokio::spawn(async move {
        if let Some(a) = account(&event_client) {
            emit(&event_tx, BackendEvent::AccountKnown(a)).await?;
        }
        while let Some(event) = raw_rx.recv().await {
            handle_event(&event_client, &event_store, &event_tx, &event).await?;
        }
        Ok::<_, BackendError>(())
    });
    let task = tokio::spawn(async move {
        let mut ingest = ingest;
        let mut result = Ok(());
        loop {
            tokio::select! {
                _=&mut stopping=>break,
                changed=problems.changed()=>{if changed.is_ok(){let notice=problems.borrow_and_update().clone();let _=emit(&events,BackendEvent::LocalError(notice)).await;}},
                r=&mut ingest=>{ result=r.unwrap_or(Err(BackendError::Stopped)); break; },
                command=requests.recv()=>{
                    let Some(command)=command else{break};
                    let key=if let BackendCommand::Transmit(ref o)=command {Some(o.key.clone())}else{None};
                    tokio::select! {
                        _=&mut stopping=>{
                            if let Some(key)=key{let _=emit(&events,BackendEvent::SendOutcome{key,state:SendState::Unconfirmed}).await;}break;
                        }
                        r=command_once(&client,&events,command)=>if let Err(e)=r{result=Err(e);break;}
                    }
                }
            }
        }
        // The event consumer remains active while the protocol flushes/disconnects.
        bot.shutdown().await;
        finished.store(true, Ordering::Release);
        drop(client);
        if !ingest.is_finished() {
            result = ingest.await.unwrap_or(Err(BackendError::Stopped));
        }
        let _ = forwarding.await;
        let _ = events.send(BackendEvent::Stopped).await;
        result
    });
    Ok(BackendHandle {
        commands,
        events: rx,
        control: BackendControl::new(stop, task),
    })
}
async fn command_once(
    client: &Client,
    tx: &mpsc::Sender<BackendEvent>,
    command: BackendCommand,
) -> Result<(), BackendError> {
    match command {
        BackendCommand::PrepareText {
            request,
            chat,
            draft,
        } => {
            if let Some(account) = account(client) {
                let message = OutboundText {
                    key: MessageKey {
                        sender: ParticipantId(account.0.clone()),
                        account,
                        chat,
                        id: client.generate_message_id().into(),
                        from_me: true,
                    },
                    draft,
                    created_at_ms: chrono::Utc::now().timestamp_millis(),
                };
                emit(tx, BackendEvent::Prepared { request, message }).await?;
            } else {
                emit(
                    tx,
                    BackendEvent::PreparationFailed {
                        request,
                        reason: "Account is not connected".into(),
                    },
                )
                .await?;
            }
        }
        BackendCommand::Transmit(message) => {
            let state = if account(client).as_ref() != Some(&message.key.account) {
                SendState::Failed
            } else {
                match encode::encode_text(&message) {
                    Err(_) => SendState::Failed,
                    Ok(encoded) => match tokio::time::timeout(
                        Duration::from_secs(30),
                        client.send_message_with_options(
                            encoded.to,
                            encoded.message,
                            encoded.options,
                        ),
                    )
                    .await
                    {
                        Ok(Ok(_)) => SendState::Sent,
                        Ok(Err(e)) => encode::classify_send_error(&e),
                        Err(_) => SendState::Unconfirmed,
                    },
                }
            };
            emit(
                tx,
                BackendEvent::SendOutcome {
                    key: message.key,
                    state,
                },
            )
            .await?;
        }
        BackendCommand::MarkRead(keys) => {
            let mut groups: BTreeMap<(ChatId, ParticipantId), Vec<String>> = BTreeMap::new();
            for key in keys {
                if !key.from_me && account(client).as_ref() == Some(&key.account) {
                    groups
                        .entry((key.chat, key.sender))
                        .or_default()
                        .push(key.id.0);
                }
            }
            for ((chat, sender), ids) in groups {
                if let (Ok(chat), Ok(sender)) = (
                    chat.0.parse::<whatsapp_rust::Jid>(),
                    sender.0.parse::<whatsapp_rust::Jid>(),
                ) {
                    let refs = ids.iter().map(String::as_str).collect::<Vec<_>>();
                    let _ = tokio::time::timeout(
                        Duration::from_secs(5),
                        client.mark_as_read(&chat, chat.is_group().then_some(&sender), &refs),
                    )
                    .await;
                }
            }
        }
    }
    Ok(())
}
fn chat(a: &AccountId, id: String, name: String) -> ChatSummary {
    ChatSummary {
        account: a.clone(),
        is_group: id.ends_with("@g.us"),
        phone: id.strip_suffix("@s.whatsapp.net").map(str::to_owned),
        name: if name.is_empty() { id.clone() } else { name },
        chat: ChatId(id),
        ..Default::default()
    }
}
async fn names(
    store: &Store,
    tx: &mpsc::Sender<BackendEvent>,
    a: &AccountId,
    chats: Vec<ChatSummary>,
) -> Result<(), BackendError> {
    let change = store
        .upsert_chats(a.clone(), chats)
        .await
        .map_err(storage_error)?;
    emit(tx, BackendEvent::StoreChanged(change)).await
}
async fn handle_event(
    client: &Client,
    store: &Store,
    tx: &mpsc::Sender<BackendEvent>,
    event: &Event,
) -> Result<(), BackendError> {
    match event {
        Event::PairingQrCode(q) => {
            return emit(
                tx,
                BackendEvent::PairingQr {
                    content: q.code.clone(),
                    expires_at: Instant::now() + q.timeout,
                },
            )
            .await;
        }
        Event::Connected(_) | Event::PairSuccess(_) => {
            if let Some(a) = account(client) {
                emit(tx, BackendEvent::AccountKnown(a)).await?;
            }
            return emit(
                tx,
                BackendEvent::ConnectionChanged {
                    state: if matches!(event, Event::Connected(_)) {
                        ConnectionState::Connected
                    } else {
                        ConnectionState::Connecting
                    },
                    reason: None,
                },
            )
            .await;
        }
        Event::Disconnected(_) => {
            return emit(
                tx,
                BackendEvent::ConnectionChanged {
                    state: ConnectionState::Reconnecting,
                    reason: Some("Connection interrupted".into()),
                },
            )
            .await;
        }
        Event::LoggedOut(_) => {
            return emit(
                tx,
                BackendEvent::ConnectionChanged {
                    state: ConnectionState::PairingRequired,
                    reason: Some("Linked session revoked; restart to link again".into()),
                },
            )
            .await;
        }
        Event::PairError(_) | Event::ClientOutdated(_) | Event::StreamReplaced(_) => {
            return emit(
                tx,
                BackendEvent::ConnectionChanged {
                    state: ConnectionState::Disconnected,
                    reason: Some("Session unavailable; restart or check linked devices".into()),
                },
            )
            .await;
        }
        _ => {}
    }
    let Some(a) = account(client) else {
        return Ok(());
    };
    match event {
        Event::Messages(batch) => {
            for chunk in batch.messages.chunks(100) {
                let change = super::durability::persist_normalized(
                    store,
                    normalize::message_batch(a.clone(), MessageSource::Live, chunk),
                )
                .await
                .map_err(storage_error)?;
                emit(tx, BackendEvent::StoreChanged(change)).await?;
            }
            for m in batch
                .iter()
                .filter(|m| !m.info.push_name.is_empty() && !m.info.source.is_from_me)
            {
                names(
                    store,
                    tx,
                    &a,
                    vec![chat(
                        &a,
                        normalize::jid(&m.info.source.sender),
                        m.info.push_name.clone(),
                    )],
                )
                .await?;
            }
        }
        Event::Receipt(r) => {
            let state = match r.r#type {
                ReceiptType::Read | ReceiptType::Played => ReceiptState::Read,
                ReceiptType::Delivered => ReceiptState::Delivered,
                _ => return Ok(()),
            };
            for id in &r.message_ids {
                let key = MessageKey {
                    account: a.clone(),
                    chat: normalize::jid(&r.source.chat).into(),
                    sender: a.0.clone().into(),
                    id: id.to_string().into(),
                    from_me: true,
                };
                let change = store
                    .record_receipt(Receipt {
                        key,
                        recipient: normalize::jid(&r.source.sender).into(),
                        state,
                        at_ms: r.timestamp.timestamp_millis(),
                    })
                    .await
                    .map_err(storage_error)?;
                emit(tx, BackendEvent::StoreChanged(change)).await?;
            }
        }
        Event::ServerAck(ack) if ack.class.as_deref() == Some("message") => {
            if let Some(to) = &ack.from {
                let key = MessageKey {
                    account: a.clone(),
                    chat: normalize::jid(to).into(),
                    sender: a.0.clone().into(),
                    id: ack.id.clone().into(),
                    from_me: true,
                };
                let state = if ack.error.is_some() {
                    SendState::Failed
                } else {
                    SendState::Sent
                };
                let change = store
                    .set_send_state(key, state)
                    .await
                    .map_err(storage_error)?;
                emit(tx, BackendEvent::StoreChanged(change)).await?;
            }
        }
        Event::ContactUpdate(c) => {
            names(
                store,
                tx,
                &a,
                vec![chat(
                    &a,
                    normalize::jid(&c.jid),
                    c.action
                        .full_name
                        .clone()
                        .or(c.action.first_name.clone())
                        .unwrap_or_default(),
                )],
            )
            .await?;
        }
        Event::PushNameUpdate(c) => {
            names(
                store,
                tx,
                &a,
                vec![chat(&a, normalize::jid(&c.jid), c.new_push_name.clone())],
            )
            .await?;
        }
        Event::GroupUpdate(g) => {
            if let whatsapp_rust::wacore::stanza::groups::GroupNotificationAction::Subject {
                subject,
                ..
            } = &g.action
            {
                names(
                    store,
                    tx,
                    &a,
                    vec![chat(&a, normalize::jid(&g.group_jid), subject.clone())],
                )
                .await?;
            }
        }
        Event::HistorySync(lazy) => {
            emit(tx, BackendEvent::HistoryProgress(None)).await?;
            if let Some(history) = lazy.get() {
                for c in &history.conversations {
                    if c.id.ends_with("@broadcast") || c.id.ends_with("@newsletter") {
                        continue;
                    }
                    let mut summary = chat(
                        &a,
                        c.id.clone(),
                        c.name
                            .clone()
                            .or(c.display_name.clone())
                            .unwrap_or_default(),
                    );
                    summary.unread = c.unread_count.unwrap_or(0);
                    names(store, tx, &a, vec![summary.clone()]).await?;
                    for chunk in c.messages.chunks(100) {
                        let changes = chunk
                            .iter()
                            .filter_map(|m| m.message.as_option())
                            .filter_map(|m| normalize::history_message(&a, &summary.chat, m))
                            .collect();
                        let change = store
                            .apply_batch(MessageBatch {
                                account: a.clone(),
                                source: MessageSource::History,
                                changes,
                            })
                            .await
                            .map_err(storage_error)?;
                        emit(tx, BackendEvent::StoreChanged(change)).await?;
                    }
                }
                for p in &history.pushnames {
                    if let Some(id) = &p.id {
                        names(
                            store,
                            tx,
                            &a,
                            vec![chat(&a, id.clone(), p.pushname.clone().unwrap_or_default())],
                        )
                        .await?;
                    }
                }
                emit(tx, BackendEvent::HistoryProgress(history.progress)).await?;
            }
        }
        Event::OfflineSyncCompleted(_) => {
            emit(tx, BackendEvent::HistoryProgress(Some(100))).await?;
        }
        _ => {}
    }
    Ok(())
}
