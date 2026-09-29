mod support;
use crossterm::event::Event;
use diesel::{prelude::*, sql_types::Text};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use support::*;
use tokio::time::{Duration, Instant};
use whatsapp_tui::{
    app::*,
    config::{Config, Paths, XdgDirs},
    runtime::{self, Options, Screen},
    storage::Store,
    whatsapp::{BackendControl, BackendError, BackendHandle},
};
fn backend() -> BackendHandle {
    let (commands, mut rx) = tokio::sync::mpsc::channel(32);
    let (tx, events) = tokio::sync::mpsc::channel(256);
    let (stop, wait) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        tokio::pin!(wait);
        loop {
            tokio::select! {_=&mut wait=>break,command=rx.recv()=>if command.is_none(){break;}}
        }
        drop(tx);
        Ok(())
    });
    BackendHandle {
        media: Arc::new(whatsapp_tui::whatsapp::demo::DemoDownloader),
        commands,
        events,
        control: BackendControl::new(stop, task),
    }
}
#[tokio::test]
async fn demo_uses_only_temporary_storage() {
    let real = tempfile::tempdir().unwrap();
    std::fs::write(real.path().join("sentinel"), "leave intact").unwrap();
    let paths = Paths::resolve(real.path(), &XdgDirs::default());
    let opts = Options {
        demo: true,
        ..Default::default()
    };
    let s = runtime::prepare_with(opts, paths, |_, _| async { Ok(backend()) })
        .await
        .unwrap();
    assert!(!s.data_path().starts_with(real.path()));
    assert_eq!(
        std::fs::read_to_string(real.path().join("sentinel")).unwrap(),
        "leave intact"
    );
    assert_eq!(std::fs::read_dir(real.path()).unwrap().count(), 1);
    s.backend.control.shutdown().await.unwrap();
}
#[tokio::test]
async fn demo_has_no_network_factory() {
    let real = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let capture = calls.clone();
    let s = runtime::prepare_with(
        Options {
            demo: true,
            ..Default::default()
        },
        Paths::resolve(real.path(), &XdgDirs::default()),
        move |_, _| async move {
            capture.fetch_add(1, Ordering::SeqCst);
            Ok(backend())
        },
    )
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    s.backend.control.shutdown().await.unwrap();
}
#[tokio::test(start_paused = true)]
async fn draft_tick_is_250_ms() {
    let mut a = ready_app();
    press(&mut a, "enter");
    let now = Instant::now();
    a.update(Input::Terminal(Event::Paste("draft".into())), now);
    let e = a.update(Input::Tick(0), now + Duration::from_millis(249));
    assert!(!e.iter().any(|e| matches!(e, Effect::SaveDraft { .. })));
    let e = a.update(Input::Tick(0), now + Duration::from_millis(250));
    assert_eq!(
        e.iter()
            .filter_map(|e| if let Effect::SaveDraft { draft, .. } = e {
                Some(draft.revision)
            } else {
                None
            })
            .collect::<Vec<_>>(),
        vec![1]
    );
}
struct CheckScreen {
    path: PathBuf,
    finished: bool,
}
impl Screen for CheckScreen {
    fn draw(&mut self, _: &ViewModel, _: &Config) -> std::io::Result<()> {
        Ok(())
    }
    fn finish(&mut self) -> std::io::Result<()> {
        #[derive(QueryableByName)]
        struct Row {
            #[diesel(sql_type=Text)]
            data: String,
        }
        let mut c = diesel::SqliteConnection::establish(self.path.to_str().unwrap()).unwrap();
        let rows = diesel::sql_query("SELECT data FROM drafts")
            .load::<Row>(&mut c)
            .unwrap();
        assert!(
            rows.iter().any(|r| r.data.contains("exit draft")),
            "terminal cleanup must follow draft persistence"
        );
        self.finished = true;
        Ok(())
    }
}
#[tokio::test]
async fn normal_exit_flushes_before_cleanup() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("db");
    let store = Store::open(path.clone()).await.unwrap();
    let input = futures_util::stream::iter(vec![
        Ok(Event::Key(
            whatsapp_tui::config::bindings::parse_key("enter").unwrap(),
        )),
        Ok(Event::Paste("exit draft".into())),
        Ok(Event::Key(
            whatsapp_tui::config::bindings::parse_key("ctrl-q").unwrap(),
        )),
    ]);
    let mut screen = CheckScreen {
        path,
        finished: false,
    };
    runtime::run_with_screen(ready_app(), store, backend(), &mut screen, input)
        .await
        .unwrap();
    assert!(screen.finished);
}
#[test]
fn backend_errors_are_sanitized() {
    assert!(
        !BackendError::Service(anyhow::anyhow!("PRIVATE_SENTINEL"))
            .to_string()
            .contains("PRIVATE_SENTINEL")
    );
}
#[tokio::test]
async fn media_download_allows_typing_and_is_cancelled_before_exit() {
    use whatsapp_tui::{app::model::*, media::*, whatsapp::BackendEvent};
    struct Waiting {
        started: tokio::sync::Notify,
        cancelled: AtomicUsize,
    }
    #[async_trait::async_trait]
    impl Downloader for Waiting {
        async fn download(
            &self,
            _: &Attachment,
            _: &std::path::Path,
            mut cancel: tokio::sync::watch::Receiver<bool>,
        ) -> Result<(), String> {
            self.started.notify_one();
            cancel.changed().await.unwrap();
            assert!(*cancel.borrow());
            self.cancelled.store(1, Ordering::SeqCst);
            Err("Cancelled".into())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let store = Store::open(path.clone()).await.unwrap();
    let mut app = ready_app();
    let mut view = app.view();
    view.messages[0].body = MessageBody::Media(Box::new(Attachment {
        kind: AttachmentKind::Document,
        filename: Some("test.txt".into()),
        mime: Some("text/plain".into()),
        caption: None,
        size: 6,
        direct_path: "/v/test".into(),
        media_key: [1; 32],
        sha256: [2; 32],
        encrypted_sha256: [3; 32],
    }));
    store
        .apply_batch(batch(view.messages.clone()))
        .await
        .unwrap();
    let effects = app.update(
        Input::Backend(BackendEvent::StoreChanged(StoreChange {
            account: account("test"),
            chats: vec!["chat".into()],
        })),
        Instant::now(),
    );
    let request = effects
        .iter()
        .find_map(|e| {
            if let Effect::LoadChat { request, .. } = e {
                Some(*request)
            } else {
                None
            }
        })
        .unwrap();
    app.update(
        Input::Store(StoreCompletion::Chat {
            request,
            account: account("test"),
            chat: "chat".into(),
            cursor: None,
            result: Ok(Box::new(ChatSnapshot {
                summary: view.chats[0].clone(),
                messages: view.messages,
                draft: view.draft,
                receipts: vec![],
                has_older: false,
                has_newer: false,
            })),
        }),
        Instant::now(),
    );
    let source = Arc::new(Waiting {
        started: tokio::sync::Notify::new(),
        cancelled: AtomicUsize::new(0),
    });
    let mut backend = backend();
    backend.media = source.clone();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let input = Box::pin(futures_util::stream::unfold(rx, |mut rx| async {
        rx.recv().await.map(|event| (Ok(event), rx))
    }));
    let send_key = |s| {
        tx.send(Event::Key(
            whatsapp_tui::config::bindings::parse_key(s).unwrap(),
        ))
        .unwrap()
    };
    send_key("tab");
    send_key("d");
    let interact = async {
        source.started.notified().await;
        send_key("tab");
        tx.send(Event::Paste("exit draft".into())).unwrap();
        send_key("ctrl-q");
    };
    let mut screen = CheckScreen {
        path,
        finished: false,
    };
    tokio::time::timeout(Duration::from_secs(2), async {
        let (result, ()) = tokio::join!(
            runtime::run_with_screen(app, store, backend, &mut screen, input),
            interact
        );
        result.unwrap();
    })
    .await
    .expect("quit must cancel the download promptly");
    assert_eq!(source.cancelled.load(Ordering::SeqCst), 1);
    assert!(screen.finished);
}
#[tokio::test]
async fn committed_demo_attempt_reaches_backend() {
    use whatsapp_tui::whatsapp::BackendEvent;
    let mut app = ready_app();
    press(&mut app, "enter");
    app.update(
        Input::Terminal(Event::Paste("hello".into())),
        Instant::now(),
    );
    let effects = press(&mut app, "enter");
    let (request, draft) = effects
        .iter()
        .find_map(|e| {
            if let Effect::Prepare { request, draft, .. } = e {
                Some((*request, draft.clone()))
            } else {
                None
            }
        })
        .unwrap();
    let message = outbound(key("chat", "test", "demo-attempt"), draft);
    let staged = app.update(
        Input::Backend(BackendEvent::Prepared {
            request,
            message: message.clone(),
        }),
        Instant::now(),
    );
    assert!(staged.iter().any(|e| matches!(e, Effect::Stage { .. })));
    assert!(!staged.iter().any(|e| matches!(e, Effect::Transmit(_))));
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("db")).await.unwrap();
    store.stage_outgoing(message.clone()).await.unwrap();
    let effects = app.update(
        Input::Store(StoreCompletion::Staged {
            request,
            message,
            result: Ok(()),
        }),
        Instant::now(),
    );
    assert!(effects.iter().any(|e| matches!(e, Effect::Transmit(_))));
    assert!(app.view().draft.text.is_empty());
}

#[test]
fn quit_waits_for_draft_commit_and_a_failure_keeps_the_composer() {
    let mut app = ready_app();
    press(&mut app, "enter");
    press(&mut app, "x");
    let effects = press(&mut app, "ctrl-q");
    assert!(
        !effects.iter().any(|e| matches!(e, Effect::Shutdown)),
        "must await the draft write"
    );
    let (request, account, chat, revision) = effects
        .into_iter()
        .find_map(|e| {
            if let Effect::SaveDraft {
                request,
                account,
                chat,
                draft,
            } = e
            {
                Some((request, account, chat, draft.revision))
            } else {
                None
            }
        })
        .unwrap();
    let effects = app.update(
        Input::Store(StoreCompletion::DraftSaved {
            request,
            account,
            chat,
            revision,
            result: Err("Cannot commit local data".into()),
        }),
        Instant::now(),
    );
    assert!(!effects.iter().any(|e| matches!(e, Effect::Shutdown)));
    assert_eq!(app.view().draft.text, "x");
    press(&mut app, "y");
    assert_eq!(app.view().draft.text, "xy");
    let effects = press(&mut app, "ctrl-q");
    let (request, account, chat, revision) = effects
        .into_iter()
        .find_map(|e| {
            if let Effect::SaveDraft {
                request,
                account,
                chat,
                draft,
            } = e
            {
                Some((request, account, chat, draft.revision))
            } else {
                None
            }
        })
        .unwrap();
    let effects = app.update(
        Input::Store(StoreCompletion::DraftSaved {
            request,
            account,
            chat,
            revision,
            result: Ok(()),
        }),
        Instant::now(),
    );
    assert!(effects.iter().any(|e| matches!(e, Effect::Shutdown)));
}
