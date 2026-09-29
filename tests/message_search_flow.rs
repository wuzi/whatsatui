mod support;
use crossterm::event::Event;
use ratatui::{Terminal, backend::TestBackend};
use support::*;
use tokio::{sync::mpsc, time::Instant};
use whatsapp_tui::{
    app::{model::*, *},
    config::Config,
    runtime,
    storage::Store,
    ui,
    whatsapp::BackendEvent,
};

fn paste(app: &mut App, text: &str) -> Vec<Effect> {
    app.update(Input::Terminal(Event::Paste(text.into())), Instant::now())
}
fn hit(id: &str, preview: &str) -> MessageSearchHit {
    MessageSearchHit {
        key: key("chat", "alice", id),
        created_at_ms: 1_790_640_000_000,
        preview: preview.into(),
    }
}
fn response(effect: &Effect, result: Result<MessageSearchPage, String>) -> Input {
    let Effect::SearchMessages {
        request,
        account,
        chat,
        query,
    } = effect
    else {
        panic!("expected search effect")
    };
    Input::Store(StoreCompletion::MessageSearch {
        request: *request,
        account: account.clone(),
        chat: chat.clone(),
        query: query.clone(),
        result,
    })
}
fn page(id: &str) -> MessageSearchPage {
    MessageSearchPage {
        hits: vec![hit(id, "found text")],
        has_more: false,
    }
}
fn request(app: &mut App, query: &str) -> Effect {
    press(app, "ctrl-f");
    paste(app, query);
    press(app, "enter")
        .into_iter()
        .find(|e| matches!(e, Effect::SearchMessages { .. }))
        .expect("search request")
}
fn screen(app: &App, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| ui::render(f, &app.view(), &app.config)).unwrap();
    let position = t.get_cursor_position().unwrap();
    assert!(position.x < w && position.y < h);
    t.backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}

#[test]
fn finder_submits_only_on_enter() {
    let mut app = ready_app();
    assert!(press(&mut app, "ctrl-f").is_empty());
    assert!(matches!(
        app.view().overlay,
        Some(Overlay::MessageSearch(_))
    ));
    assert!(paste(&mut app, "needle").is_empty());
    let effects = press(&mut app, "enter");
    assert_eq!(
        effects
            .iter()
            .filter(|e| matches!(e, Effect::SearchMessages { .. }))
            .count(),
        1
    );
    assert!(press(&mut app, "enter").is_empty());
    assert_eq!(app.view().focus, Focus::Chats);
}

#[test]
fn editing_an_inflight_search_cannot_queue_more_scans() {
    let mut app = ready_app();
    let first = request(&mut app, "needle");
    paste(&mut app, " changed");
    assert!(press(&mut app, "enter").is_empty());
    app.update(response(&first, Ok(page("obsolete"))), Instant::now());
    let Some(Overlay::MessageSearch(search)) = app.view().overlay else {
        panic!()
    };
    assert!(search.page.hits.is_empty());
    assert!(search.request.is_none());
    assert!(
        press(&mut app, "enter")
            .iter()
            .any(|e| matches!(e, Effect::SearchMessages { .. }))
    );
}

#[test]
fn alias_snapshot_invalidates_search_without_leaving_it_busy() {
    let mut app = ready_app();
    let first = request(&mut app, "needle");
    let effects = app.update(
        Input::Backend(BackendEvent::StoreChanged(StoreChange {
            account: account("test"),
            chats: vec!["chat".into()],
        })),
        Instant::now(),
    );
    let load = effects
        .iter()
        .find_map(|e| match e {
            Effect::LoadChat { request, .. } => Some(*request),
            _ => None,
        })
        .unwrap();
    app.update(
        Input::Store(StoreCompletion::Chat {
            request: load,
            account: account("test"),
            chat: "chat".into(),
            cursor: None,
            result: Ok(Box::new(ChatSnapshot {
                summary: ChatSummary {
                    account: account("test"),
                    chat: "canonical".into(),
                    ..Default::default()
                },
                messages: vec![],
                draft: Draft::default(),
                receipts: vec![],
                has_older: false,
                has_newer: false,
            })),
        }),
        Instant::now(),
    );
    app.update(response(&first, Ok(page("alias-stale"))), Instant::now());
    let Some(Overlay::MessageSearch(search)) = app.view().overlay else {
        panic!()
    };
    assert_eq!(search.chat.0, "canonical");
    assert!(search.page.hits.is_empty());
    assert!(
        press(&mut app, "enter")
            .iter()
            .any(|e| matches!(e, Effect::SearchMessages { chat, .. } if chat.0 == "canonical"))
    );
}

#[test]
fn finder_ignores_stale_responses() {
    let mut app = ready_app();
    let first = request(&mut app, "first");
    paste(&mut app, " edit");
    app.update(response(&first, Ok(page("stale-edit"))), Instant::now());
    let Some(Overlay::MessageSearch(search)) = app.view().overlay else {
        panic!()
    };
    assert!(search.page.hits.is_empty());
    let second = press(&mut app, "enter").remove(0);
    press(&mut app, "esc");
    let third = request(&mut app, "third");
    app.update(response(&second, Ok(page("stale-reopen"))), Instant::now());
    let Some(Overlay::MessageSearch(search)) = app.view().overlay else {
        panic!()
    };
    assert!(search.page.hits.is_empty());
    app.update(response(&third, Ok(page("fresh"))), Instant::now());
    let Some(Overlay::MessageSearch(search)) = app.view().overlay else {
        panic!()
    };
    assert_eq!(search.page.hits[0].key.id.0, "fresh");
    app.update(
        Input::Backend(BackendEvent::AccountKnown(account("different"))),
        Instant::now(),
    );
    app.update(response(&third, Ok(page("wrong-account"))), Instant::now());
    assert!(app.view().overlay.is_none());
}

#[test]
fn finder_failure_can_retry() {
    let mut app = ready_app();
    let first = request(&mut app, "needle");
    app.update(
        response(&first, Err("Local storage unavailable".into())),
        Instant::now(),
    );
    let Some(Overlay::MessageSearch(search)) = app.view().overlay else {
        panic!()
    };
    assert_eq!(search.editor.text(), "needle");
    assert!(search.error.is_some());
    let second = press(&mut app, "enter").remove(0);
    let (Effect::SearchMessages { request: a, .. }, Effect::SearchMessages { request: b, .. }) =
        (&first, &second)
    else {
        panic!()
    };
    assert_ne!(a, b);
    app.update(response(&second, Ok(page("one"))), Instant::now());
    assert!(screen(&app, 80, 24).contains("found text"));
}

#[test]
fn conversation_change_invalidates_results() {
    let mut app = ready_app();
    let first = request(&mut app, "needle");
    app.update(response(&first, Ok(page("old"))), Instant::now());
    app.update(
        Input::Backend(BackendEvent::StoreChanged(StoreChange {
            account: account("test"),
            chats: vec!["chat".into()],
        })),
        Instant::now(),
    );
    let Some(Overlay::MessageSearch(search)) = app.view().overlay else {
        panic!()
    };
    assert!(search.page.hits.is_empty());
    app.update(response(&first, Ok(page("deleted"))), Instant::now());
    let Some(Overlay::MessageSearch(search)) = app.view().overlay else {
        panic!()
    };
    assert!(search.page.hits.is_empty());
    assert!(
        press(&mut app, "enter")
            .iter()
            .any(|e| matches!(e, Effect::SearchMessages { .. }))
    );
}

#[test]
fn finder_cancel_keeps_draft_focus_and_query_is_bounded() {
    let mut app = ready_app();
    press(&mut app, "enter");
    paste(&mut app, "draft words");
    press(&mut app, "ctrl-f");
    paste(&mut app, &format!("needle\n{}", "界".repeat(300)));
    let Some(Overlay::MessageSearch(search)) = app.view().overlay else {
        panic!()
    };
    assert_eq!(search.editor.text().chars().count(), 256);
    assert!(search.editor.text().starts_with("needle "));
    for (w, h) in [(40, 12), (120, 40)] {
        screen(&app, w, h);
    }
    press(&mut app, "esc");
    assert_eq!(app.view().draft.text, "draft words");
    assert_eq!(app.view().focus, Focus::Composer);
    assert_eq!(app.view().selected_message.unwrap().id.0, "one");
    let mut empty = App::new(Config::default());
    assert!(press(&mut empty, "ctrl-f").is_empty());
    assert!(empty.view().overlay.is_none());
    assert!(empty.view().notice.is_some());
}

#[tokio::test]
async fn opening_old_hit_keeps_draft_and_unread() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    let messages = (0..150)
        .map(|i| {
            let mut m = message(
                key("chat", "alice", &format!("m{i:03}")),
                if i == 0 { "needle in history" } else { "later" },
            );
            m.created_at_ms += i;
            m
        })
        .collect();
    store.apply_batch(batch(messages)).await.unwrap();
    let mut app = ready_app();
    press(&mut app, "enter");
    paste(&mut app, "unsent 👩‍💻");
    press(&mut app, "esc");
    let effect = request(&mut app, "needle");
    let (commands, _rx) = mpsc::channel(8);
    let response = runtime::execute(effect, store.clone(), commands.clone())
        .await
        .unwrap();
    assert!(
        app.update(response, Instant::now())
            .iter()
            .all(|e| !matches!(e, Effect::MarkRead { .. }))
    );
    let effects = press(&mut app, "enter");
    assert_eq!(app.view().focus, Focus::Messages);
    assert!(
        effects
            .iter()
            .all(|e| !matches!(e, Effect::MarkRead { .. }))
    );
    let load = effects
        .into_iter()
        .find(|e| {
            matches!(
                e,
                Effect::LoadChat {
                    cursor: Some(PageCursor {
                        direction: PageDirection::AtOrBefore,
                        ..
                    }),
                    ..
                }
            )
        })
        .unwrap();
    let snapshot = runtime::execute(load, store.clone(), commands.clone())
        .await
        .unwrap();
    assert!(
        app.update(snapshot, Instant::now())
            .iter()
            .all(|e| !matches!(e, Effect::MarkRead { .. }))
    );
    assert_eq!(
        app.view().selected_message.unwrap(),
        key("chat", "alice", "m000")
    );
    assert!(!app.view().at_bottom);
    assert_eq!(app.view().draft.text, "unsent 👩‍💻");
    assert_eq!(
        store.list_chats(account("test")).await.unwrap()[0].unread,
        150
    );
    let latest = press(&mut app, "end")
        .into_iter()
        .find(|e| matches!(e, Effect::LoadChat { cursor: None, .. }))
        .unwrap();
    let snapshot = runtime::execute(latest, store, commands).await.unwrap();
    app.update(snapshot, Instant::now());
    assert_eq!(app.view().selected_message.unwrap().id.0, "m149");
    assert_eq!(app.view().draft.text, "unsent 👩‍💻");
}

#[test]
fn finder_rendering_is_adaptive() {
    let mut app = ready_app();
    app.config =
        Config::parse("[bindings.message_search]\nopen=['ctrl-y']\nback=['ctrl-b']").unwrap();
    press(&mut app, "ctrl-f");
    paste(&mut app, "needle");
    for (w, h) in [(40, 12), (120, 40)] {
        let text = screen(&app, w, h);
        for expected in ["Cached", "ctrl-y", "ctrl-b"] {
            assert!(text.contains(expected), "{expected} at {w}x{h}");
        }
    }
    let first = press(&mut app, "ctrl-y").remove(0);
    assert!(screen(&app, 40, 12).contains("Searching"));
    app.update(
        response(&first, Ok(MessageSearchPage::default())),
        Instant::now(),
    );
    assert!(screen(&app, 40, 12).contains("No matches"));
    let next = press(&mut app, "ctrl-y").remove(0);
    app.update(
        response(&next, Err("Cannot read local data".into())),
        Instant::now(),
    );
    assert!(screen(&app, 40, 12).contains("Cannot read"));
    let next = press(&mut app, "ctrl-y").remove(0);
    app.update(
        response(
            &next,
            Ok(MessageSearchPage {
                hits: (0..50)
                    .map(|i| hit(&i.to_string(), "needle\u{1b}]52;secret\u{7}"))
                    .collect(),
                has_more: true,
            }),
        ),
        Instant::now(),
    );
    for (w, h) in [(40, 12), (120, 40)] {
        let text = screen(&app, w, h);
        assert!(text.contains("50+"));
        assert!(!text.contains('\u{1b}'));
        assert!(!text.contains('\u{7}'));
    }
}
