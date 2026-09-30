mod support;
use crossterm::event::{Event, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use support::*;
use tokio::time::Instant;
use whatsapp_tui::{
    app::{model::*, *},
    audio::{Phase, Playback, Request, Speed},
    config::Config,
    ui::{self, interaction::Target},
    whatsapp::BackendEvent,
};

fn voice(id: &str) -> MessageRecord {
    let mut m = message(key("chat", "alice", id), "");
    m.body=serde_json::from_value(serde_json::json!({"Media":{"kind":"Audio","audio":{"voice":true,"seconds":42},"filename":null,"mime":"audio/ogg","caption":null,"size":5,"direct_path":"/v/audio","media_key":vec![1;32],"sha256":vec![2;32],"encrypted_sha256":vec![3;32]}})).unwrap();
    m
}
fn refresh(app: &mut App, messages: Vec<MessageRecord>) -> Vec<Effect> {
    let view = app.view();
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
    for effect in &effects {
        if let Effect::LoadChats { request, .. } = effect {
            let mut chats = view.chats.clone();
            if chats.len() == 1 {
                chats.push(ChatSummary {
                    account: account("test"),
                    chat: "other".into(),
                    name: "Bob".into(),
                    ..Default::default()
                });
            }
            app.update(
                Input::Store(StoreCompletion::Chats {
                    request: *request,
                    account: account("test"),
                    result: Ok(chats),
                }),
                Instant::now(),
            );
        }
    }
    app.update(
        Input::Store(StoreCompletion::Chat {
            request,
            account: account("test"),
            chat: "chat".into(),
            cursor: None,
            result: Ok(Box::new(ChatSnapshot {
                summary: view.chats[0].clone(),
                messages,
                draft: view.draft,
                receipts: vec![],
                interactions: Default::default(),
                has_older: false,
                has_newer: false,
            })),
        }),
        Instant::now(),
    )
}
fn app() -> App {
    let mut app = ready_app();
    refresh(&mut app, vec![voice("one")]);
    press(&mut app, "tab");
    app
}
fn request(effects: Vec<Effect>) -> Request {
    effects
        .into_iter()
        .find_map(|e| {
            if let Effect::Audio(Some(request)) = e {
                Some(request)
            } else {
                None
            }
        })
        .expect("audio play effect")
}
fn observe(app: &mut App, request: Request, phase: Phase) {
    let mut playback = Playback::loading(request);
    playback.phase = phase;
    playback.position_ms = 12_000;
    app.update(Input::Playback(playback), Instant::now());
}
#[test]
fn controls_keep_the_normal_draft_and_coalesce_rapid_changes_despite_old_observations() {
    let mut app = app();
    press(&mut app, "tab");
    app.update(
        Input::Terminal(Event::Paste("keep draft".into())),
        Instant::now(),
    );
    press(&mut app, "esc");
    let first = request(press(&mut app, "p"));
    observe(&mut app, first.clone(), Phase::Playing);
    let paused = request(press(&mut app, "p"));
    assert!(paused.paused);
    assert_eq!(paused.id, first.id);
    observe(&mut app, first.clone(), Phase::Playing); // delayed before pause was acknowledged
    assert!(!request(press(&mut app, "p")).paused);
    assert_eq!(request(press(&mut app, "s")).speed, Speed::OneHalf);
    observe(&mut app, first, Phase::Playing);
    assert_eq!(request(press(&mut app, "s")).speed, Speed::Double);
    assert!(
        press(&mut app, "x")
            .iter()
            .any(|e| matches!(e, Effect::Audio(None)))
    );
    assert!(app.view().playback.is_none());
    assert_eq!(app.view().draft.text, "keep draft");
    press(&mut app, "tab");
    press(&mut app, "p");
    press(&mut app, "s");
    press(&mut app, "x");
    assert_eq!(app.view().draft.text, "keep draftpsx");
}
#[test]
fn menu_target_is_stable_and_replaced_or_stopped_sessions_ignore_old_events() {
    let mut app = app();
    press(&mut app, "enter");
    let mut newer = voice("two");
    newer.created_at_ms += 1000;
    refresh(&mut app, vec![voice("one"), newer.clone()]);
    let old = request(press(&mut app, "p"));
    assert_eq!(old.message.key.id.0, "one");
    press(&mut app, "end");
    let current = request(press(&mut app, "p"));
    assert_eq!(current.message.key.id.0, "two");
    assert_ne!(old.id, current.id);
    observe(&mut app, old.clone(), Phase::Playing);
    assert_eq!(app.view().playback.unwrap().request.id, current.id);
    press(&mut app, "x");
    observe(&mut app, current, Phase::Playing);
    assert!(app.view().playback.is_none());
    observe(&mut app, old, Phase::Failed);
    assert!(app.view().playback.is_none());
}
#[test]
fn account_changes_deletion_and_quit_stop_playback_but_chat_switches_do_not() {
    for reason in ["account", "delete", "quit", "chat"] {
        let mut app = app();
        let playing = request(press(&mut app, "p"));
        observe(&mut app, playing.clone(), Phase::Playing);
        let effects = match reason {
            "account" => app.update(
                Input::Backend(BackendEvent::AccountKnown("other".into())),
                Instant::now(),
            ),
            "delete" => {
                let mut deleted = voice("one");
                deleted.body = MessageBody::Deleted;
                refresh(&mut app, vec![deleted])
            }
            "quit" => app.request_shutdown(),
            _ => {
                press(&mut app, "ctrl-p");
                app.update(Input::Terminal(Event::Paste("Bob".into())), Instant::now());
                press(&mut app, "enter")
            }
        };
        if reason == "chat" {
            assert!(app.view().playback.is_some());
            assert_eq!(app.view().chat, Some("other".into()));
            assert!(!effects.iter().any(|e| matches!(e, Effect::Audio(_))));
        } else {
            assert!(app.view().playback.is_none());
            assert!(effects.iter().any(|e| matches!(e, Effect::Audio(None))));
            observe(&mut app, playing, Phase::Playing);
            assert!(app.view().playback.is_none());
        }
    }
}
#[test]
fn remapped_play_control_works_and_failure_can_be_retried() {
    let mut app = app();
    app.config = Config::parse("[bindings.messages]\nplay_audio=['b']\n").unwrap();
    assert!(
        !press(&mut app, "p")
            .iter()
            .any(|e| matches!(e, Effect::Audio(_)))
    );
    let first = request(press(&mut app, "b"));
    let mut failed = Playback::loading(first.clone());
    failed.phase = Phase::Failed;
    failed.error = Some("Install mpv".into());
    app.update(Input::Playback(failed), Instant::now());
    assert!(app.view().notice.unwrap().contains("mpv"));
    let next = request(press(&mut app, "b"));
    assert_ne!(next.id, first.id);
}
#[test]
fn rendered_audio_rows_and_header_controls_have_scoped_clipped_mouse_targets() {
    for (w, h) in [(40, 12), (120, 34)] {
        let mut app = app();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        let mut map = None;
        terminal
            .draw(|f| {
                map = Some(ui::render_interactive(
                    f,
                    &app.view(),
                    &app.config,
                    &mut ui::Images::default(),
                    &mut ui::Avatars::default(),
                ))
            })
            .unwrap();
        let map = map.take().unwrap();
        let (x, y) = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .find(|(x, y)| matches!(map.hit(*x, *y), Some(Target::PlayAudio(_))))
            .unwrap();
        let symbol = terminal.backend().buffer()[(x, y)].symbol();
        assert!(!symbol.is_empty());
        app.update(Input::Rendered(map), Instant::now());
        let first = request(app.update(
            Input::Terminal(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            })),
            Instant::now(),
        ));
        observe(&mut app, first, Phase::Playing);
        let mut map = None;
        terminal
            .draw(|f| {
                map = Some(ui::render_interactive(
                    f,
                    &app.view(),
                    &app.config,
                    &mut ui::Images::default(),
                    &mut ui::Avatars::default(),
                ))
            })
            .unwrap();
        let map = map.unwrap();
        let (x, y) = (0..h)
            .flat_map(|y| (0..w).map(move |x| (x, y)))
            .find(|(x, y)| {
                matches!(
                    map.hit(*x, *y),
                    Some(Target::Playback(
                        _,
                        whatsapp_tui::config::bindings::ActionId::AudioSpeed
                    ))
                )
            })
            .unwrap();
        app.update(Input::Rendered(map), Instant::now());
        let speed = request(app.update(
            Input::Terminal(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            })),
            Instant::now(),
        ));
        assert_eq!(speed.speed, Speed::OneHalf);
    }
}

fn rendered(app: &App, width: u16) -> ui::interaction::InteractionMap {
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 24)).unwrap();
    let mut map = None;
    terminal
        .draw(|f| {
            map = Some(ui::render_interactive(
                f,
                &app.view(),
                &app.config,
                &mut ui::Images::default(),
                &mut ui::Avatars::default(),
            ))
        })
        .unwrap();
    map.unwrap()
}
fn coordinate(
    map: &ui::interaction::InteractionMap,
    target: impl Fn(&Target) -> bool,
) -> (u16, u16) {
    (0..map.area.height)
        .flat_map(|y| (0..map.area.width).map(move |x| (x, y)))
        .find(|&(x, y)| map.hit(x, y).is_some_and(&target))
        .unwrap()
}
fn click(app: &mut App, point: (u16, u16), button: MouseButton) -> Vec<Effect> {
    app.update(
        Input::Terminal(Event::Mouse(MouseEvent {
            kind: MouseEventKind::Down(button),
            column: point.0,
            row: point.1,
            modifiers: KeyModifiers::NONE,
        })),
        Instant::now(),
    )
}
#[test]
fn right_clicking_audio_keeps_message_actions_available() {
    let mut app = app();
    let map = rendered(&app, 120);
    let point = coordinate(&map, |target| matches!(target, Target::PlayAudio(_)));
    app.update(Input::Rendered(map), Instant::now());
    let effects = click(&mut app, point, MouseButton::Right);
    assert!(!effects.iter().any(|e| matches!(e, Effect::Audio(_))));
    assert!(matches!(
        app.view().overlay,
        Some(Overlay::MessageActions(_))
    ));
}
#[test]
fn covered_clipped_and_stale_controls_cannot_change_playback() {
    use whatsapp_tui::config::bindings::ActionId;
    let mut app = app();
    let first = request(press(&mut app, "p"));
    observe(&mut app, first, Phase::Playing);
    let map = rendered(&app, 120);
    let point = coordinate(&map, |target| {
        matches!(target, Target::Playback(_, ActionId::AudioStop))
    });
    // A still-delivered old frame must not stop a replacement request.
    press(&mut app, "x");
    let current = request(press(&mut app, "p"));
    app.update(Input::Rendered(map), Instant::now());
    assert!(click(&mut app, point, MouseButton::Left).is_empty());
    assert_eq!(app.view().playback.unwrap().request.id, current.id);

    press(&mut app, "f1");
    let map = rendered(&app, 120);
    app.update(Input::Rendered(map), Instant::now());
    let effects = click(&mut app, point, MouseButton::Left);
    assert!(!effects.iter().any(|e| matches!(e, Effect::Audio(_))));
    assert!(app.view().playback.is_some());
    press(&mut app, "esc");

    let mut long = Playback::loading(current);
    long.phase = Phase::Playing;
    long.position_ms = 3_600_000;
    long.duration_ms = Some(7_200_000);
    app.update(Input::Playback(long), Instant::now());
    let map = rendered(&app, 40);
    // A long recording leaves only part of Stop before Help at width 40.
    assert!(!(0..40).any(|x| matches!(
        map.hit(x, 0),
        Some(Target::Playback(_, ActionId::AudioStop))
    )));
}
