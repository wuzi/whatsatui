//! Synthetic offline account. This module never constructs a network client.
use super::*;
use crate::app::model::*;
const ACCOUNT: &str = "you@demo";
const TIME: i64 = 1_790_640_000_000;
fn key(chat: &str, sender: &str, id: &str) -> MessageKey {
    MessageKey {
        account: ACCOUNT.into(),
        chat: chat.into(),
        sender: sender.into(),
        id: id.into(),
        from_me: sender == ACCOUNT,
    }
}
fn message(chat: &str, sender: &str, id: &str, text: &str, offset: i64) -> MessageRecord {
    MessageRecord {
        key: key(chat, sender, id),
        body: MessageBody::Text(text.into()),
        quote: None,
        created_at_ms: TIME + offset,
        edited_at_ms: None,
        expires_at_ms: None,
        send_state: (sender == ACCOUNT).then_some(SendState::Read),
    }
}
async fn initialize(store: &Store) -> Result<StoreChange, BackendError> {
    let chats = [
        ("alice@demo", "Alice", false),
        ("weekend@g.us", "Weekend plans", true),
        ("maya@demo", "Maya", false),
        ("leo@demo", "Leo", false),
    ]
    .into_iter()
    .map(|(id, name, is_group)| ChatSummary {
        account: ACCOUNT.into(),
        chat: id.into(),
        name: name.into(),
        phone: None,
        is_group,
        unread: match id {
            "weekend@g.us" => 3,
            "maya@demo" => 1,
            _ => 0,
        },
        latest_at_ms: match id {
            "weekend@g.us" => TIME + 150_000,
            "maya@demo" => TIME + 45_000,
            _ => 0,
        },
        ..Default::default()
    })
    .collect();
    store
        .upsert_chats(ACCOUNT.into(), chats)
        .await
        .map_err(|e| BackendError::Service(e.into()))?;
    let mut messages = vec![
        message(
            "alice@demo",
            "alice@demo",
            "a1",
            "Hey! How is the new terminal setup going?",
            0,
        ),
        message(
            "alice@demo",
            ACCOUNT,
            "a2",
            "Cyan borders, a good keyboard, and no lost drafts.",
            60_000,
        ),
        message(
            "alice@demo",
            "alice@demo",
            "a3",
            "Tab moves between panes. Ctrl-P finds chats; Ctrl-F searches this conversation. Try cyan.",
            120_000,
        ),
        message(
            "weekend@g.us",
            "maya@demo",
            "g1",
            "Saturday coffee? ☕",
            30_000,
        ),
        message(
            "weekend@g.us",
            "leo@demo",
            "g2",
            "Sounds good. I'll bring the book we talked about.",
            90_000,
        ),
        message(
            "maya@demo",
            "maya@demo",
            "m1",
            "A little Unicode test: café, 日本語, 👩‍💻",
            45_000,
        ),
    ];
    let mut photo = message("weekend@g.us", "maya@demo", "g3", "", 150_000);
    photo.body = MessageBody::Unsupported {
        kind: "image".into(),
        caption: Some("The café on the corner".into()),
    };
    messages.push(photo);
    store
        .apply_batch(MessageBatch {
            account: ACCOUNT.into(),
            source: MessageSource::History,
            changes: messages.into_iter().map(MessageChange::Upsert).collect(),
        })
        .await
        .map_err(|e| BackendError::Service(e.into()))
}
pub fn start(store: Store) -> BackendHandle {
    let (commands, mut requests) = mpsc::channel(32);
    let (tx, events) = mpsc::channel(256);
    let (stop, mut stopping) = oneshot::channel();
    let task = tokio::spawn(async move {
        let change = initialize(&store).await?;
        for event in [
            BackendEvent::AccountKnown(ACCOUNT.into()),
            BackendEvent::ConnectionChanged {
                state: ConnectionState::Connected,
                reason: Some("Offline demo · synthetic conversations".into()),
            },
            BackendEvent::StoreChanged(change),
        ] {
            if tx.send(event).await.is_err() {
                return Ok(());
            }
        }
        let mut counter = 0u64;
        loop {
            tokio::select! {_=&mut stopping=>break,command=requests.recv()=>{let Some(command)=command else{break};match command{
                BackendCommand::PrepareText{request,chat,draft}=>{counter+=1;let message=OutboundText{key:key(&chat.0,ACCOUNT,&format!("demo-{counter:06}")),draft,created_at_ms:TIME+180_000+counter as i64*1000};if tx.send(BackendEvent::Prepared{request,message}).await.is_err(){break;}},
                BackendCommand::Transmit(sent)=>{
                    if tx.send(BackendEvent::SendOutcome{key:sent.key.clone(),state:SendState::Sent}).await.is_err(){break;}
                    let sender=if sent.key.chat.0.ends_with("@g.us"){"maya@demo"}else{&sent.key.chat.0};
                    let reply=message(&sent.key.chat.0,sender,&format!("reply-{counter:06}"),"Message received. This is an offline demo response.",181_000+counter as i64*1000);
                    let change=store.apply_batch(MessageBatch{account:ACCOUNT.into(),source:MessageSource::Live,changes:vec![MessageChange::Upsert(reply)]}).await.map_err(|e|BackendError::Service(e.into()))?;
                    let _=tx.send(BackendEvent::StoreChanged(change)).await;
                    let change=store.record_receipt(Receipt{key:sent.key.clone(),recipient:sender.into(),state:ReceiptState::Read,at_ms:TIME+182_000+counter as i64*1000}).await.map_err(|e|BackendError::Service(e.into()))?;
                    let _=tx.send(BackendEvent::StoreChanged(change)).await;
                }
                BackendCommand::MarkRead(_)=>{}
            }}}
        }
        let _ = tx.send(BackendEvent::Stopped).await;
        Ok(())
    });
    BackendHandle {
        commands,
        events,
        control: BackendControl::new(stop, task),
    }
}
