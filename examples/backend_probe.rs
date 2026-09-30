//! User-operated protocol exercise. No automatic messages are sent.
use clap::Parser;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use whatsapp_tui::{
    app::model::*,
    storage::{Store, paths::DataDirGuard},
    whatsapp::{self, BackendCommand, BackendEvent},
};
#[derive(Parser)]
struct Args {
    #[arg(long)]
    data_dir: PathBuf,
}
#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("Probe stopped: {error}");
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let _guard = DataDirGuard::acquire(&args.data_dir)?;
    let store = Store::open(args.data_dir.join("chat.sqlite3")).await?;
    let backend = whatsapp::start(args.data_dir.join("session.sqlite3"), store.clone()).await?;
    let whatsapp::BackendHandle {
        commands,
        mut events,
        control,
        ..
    } = backend;
    println!(
        "Commands: chats | send CHAT_JID TEXT | reply CHAT_JID MESSAGE_ID SENDER_JID TEXT | quit"
    );
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut account: Option<AccountId> = None;
    let mut serial = 0;
    loop {
        tokio::select! {
            line=lines.next_line()=>{
                let Some(line)=line?else{break};
                if line.trim()=="quit"{break;}
                if line.trim()=="chats"{if let Some(a)=&account{for c in store.list_chats(a.clone()).await?{println!("{}  {}",inert(&c.chat.0),inert(&c.name));}}continue;}
                let Some(a)=&account else{println!("Pair and wait for Connected first.");continue;};
                let mut parts=line.splitn(3,' ');let action=parts.next().unwrap_or("");let chat=parts.next().unwrap_or("");let rest=parts.next().unwrap_or("");
                let (text,reply)=if action=="reply"{let mut p=rest.splitn(3,' ');let id=p.next().unwrap_or("");let sender=p.next().unwrap_or("");let text=p.next().unwrap_or("");(text,Some(Quote{key:MessageKey{account:a.clone(),chat:chat.into(),sender:sender.into(),id:id.into(),from_me:sender==a.0},preview:String::new(),availability:QuoteAvailability::Missing}))}else if action=="send"{(rest,None)}else{println!("Unknown command");continue;};
                if chat.is_empty()||text.trim().is_empty(){println!("Conversation and text required");continue;}
                serial+=1;commands.send(BackendCommand::PrepareText{request:RequestId(serial),chat:chat.into(),draft:Draft{text:text.into(),attachment:None,reply,revision:serial,..Default::default()}}).await?;
            }
            event=events.recv()=>match event{
                Some(BackendEvent::AccountKnown(a))=>{store.recover_sends(a.clone()).await?;account=Some(a);println!("Account available");}
                Some(BackendEvent::ConnectionChanged{state,reason})=>println!("{state:?}: {}",reason.unwrap_or_default()),
                Some(BackendEvent::PairingQr{content,..})=>{let qr=qrcode::QrCode::new(content)?;println!("Link from WhatsApp > Linked devices:\n{}",qr.render::<qrcode::render::unicode::Dense1x2>().quiet_zone(true).build());}
                Some(BackendEvent::Prepared{message,..})=>{let message=*message;store.stage_outgoing(message.clone()).await?;commands.send(BackendCommand::Transmit(message)).await?;}
                Some(BackendEvent::PreparationFailed{reason,..})=>println!("{reason}"),
                Some(BackendEvent::SendOutcome{key,state})=>{store.set_send_state(key,state).await?;println!("Send: {state:?}");}
                Some(BackendEvent::StoreChanged(change))=>println!("Updated {} conversation(s)",change.chats.len()),
                Some(BackendEvent::HistoryProgress(p))=>println!("History progress: {p:?}"),
                Some(BackendEvent::LocalError(Some(reason)))=>println!("{reason}"),
                Some(BackendEvent::LocalError(None))=>{},
                Some(BackendEvent::Stopped)|None=>break,
            }
        }
    }
    let shutdown = control.shutdown();
    tokio::pin!(shutdown);
    loop {
        tokio::select! {result=&mut shutdown=>{result?;break;},event=events.recv()=>{if let Some(BackendEvent::SendOutcome{key,state})=event{store.set_send_state(key,state).await?;}else if event.is_none(){shutdown.await?;break;}}}
    }
    if let Some(a) = account {
        store.recover_sends(a).await?;
    }
    store.flush().await?;
    Ok(())
}
fn inert(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { '�' } else { c })
        .collect()
}
