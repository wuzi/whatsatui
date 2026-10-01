mod support;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use support::*;
use tokio::sync::{Notify, watch};
use whatsapp_tui::{
    app::model::*,
    audio::{Phase, Playback, Player, Request, Speed},
    media::{Attachment, AttachmentKind, Downloader},
    storage::Store,
};

fn record(id: &str, bytes: &str) -> MessageRecord {
    let mut m = message(key("chat", "alice", id), "");
    m.body = MessageBody::Media(Box::new(Attachment {
        kind: AttachmentKind::Audio,
        audio: None,
        filename: None,
        mime: Some("audio/ogg".into()),
        caption: None,
        size: bytes.len() as u64,
        direct_path: format!("/v/{bytes}"),
        media_key: [1; 32],
        sha256: Sha256::digest(bytes.as_bytes()).into(),
        encrypted_sha256: [2; 32],
    }));
    m
}
struct Source {
    started: Notify,
}
#[async_trait::async_trait]
impl Downloader for Source {
    async fn download(
        &self,
        a: &Attachment,
        p: &Path,
        _: watch::Receiver<bool>,
    ) -> Result<(), String> {
        let mode = a.direct_path.strip_prefix("/v/").unwrap();
        if mode == "slow" {
            self.started.notify_one();
            std::future::pending::<()>().await;
        }
        tokio::fs::write(p, mode).await.map_err(|e| e.to_string())
    }
}
struct Fixture {
    dir: tempfile::TempDir,
    store: Store,
    source: Arc<Source>,
    exe: PathBuf,
}
impl Fixture {
    fn window(&self, event: &str) {
        let temporary = self.exe.with_extension("window-next");
        std::fs::write(&temporary, event).unwrap();
        std::fs::rename(temporary, self.exe.with_extension("window")).unwrap();
    }
    async fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("player.py");
        std::fs::write(&exe, include_str!("fixtures/audio_player.py")).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o700)).unwrap();
        let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
        Self {
            dir,
            store,
            source: Arc::new(Source {
                started: Notify::new(),
            }),
            exe,
        }
    }
    fn player(&self) -> Player {
        Player::start(self.store.clone(), self.source.clone(), self.exe.clone())
    }
    async fn request(&self, id: u64, mode: &str) -> Request {
        let m = record(&id.to_string(), mode);
        self.store
            .apply_batch(batch(vec![m.clone()]))
            .await
            .unwrap();
        Request {
            id: RequestId(id),
            revision: 0,
            message: m,
            paused: false,
            speed: Speed::Normal,
        }
    }
    async fn reaped(&self) {
        let pid = std::fs::read_to_string(self.exe.with_extension("pid")).unwrap();
        tokio::time::timeout(Duration::from_secs(2), async {
            while Path::new(&format!("/proc/{}", pid.trim())).exists() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("player child must be reaped");
    }
}
async fn observed(player: &mut Player, predicate: impl Fn(&Playback) -> bool) -> Playback {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(p) = player.events.borrow_and_update().clone()
                && predicate(&p)
            {
                return p;
            }
            player.events.changed().await.unwrap();
        }
    })
    .await
    .expect("playback observation")
}
#[tokio::test]
async fn repeated_desired_state_resumes_a_native_window_pause() {
    let f = Fixture::new().await;
    let mut player = f.player();
    let request = f.request(1, "normal").await;
    player.set(Some(request.clone()));
    observed(&mut player, |p| p.phase == Phase::Playing).await;
    f.window(r#"{"event":"property-change","name":"pause","data":true}"#);
    observed(&mut player, |p| p.phase == Phase::Paused).await;
    // Same request values as before, but an explicit user action must reach mpv again.
    player.set(Some(request));
    observed(&mut player, |p| p.phase == Phase::Playing).await;
    player.set(None);
    f.reaped().await;
}
#[tokio::test]
async fn video_opens_a_window_and_normal_close_reaps_the_player() {
    let f = Fixture::new().await;
    let mut request = f.request(1, "normal").await;
    request.message.key.id = "video".into();
    if let MessageBody::Media(a) = &mut request.message.body {
        a.kind = AttachmentKind::Video;
        a.mime = Some("video/mp4".into());
    }
    f.store
        .apply_batch(batch(vec![request.message.clone()]))
        .await
        .unwrap();
    let mut player = f.player();
    player.set(Some(request));
    let status = observed(&mut player, |p| {
        p.phase == Phase::Playing || p.phase == Phase::Failed
    })
    .await;
    assert_eq!(status.phase, Phase::Playing, "{:?}", status.error);
    let args: Vec<String> =
        serde_json::from_str(&std::fs::read_to_string(f.exe.with_extension("args")).unwrap())
            .unwrap();
    assert!(!args.iter().any(|a| a == "--no-video"));
    assert!(args.iter().any(|a| a == "--vo=gpu-next,gpu,wlshm,x11"));
    assert!(args.iter().any(|a| a == "--terminal=no"));
    f.window(r#"{"event":"end-file","reason":"quit"}"#);
    let ended = observed(&mut player, |p| {
        matches!(p.phase, Phase::Finished | Phase::Failed)
    })
    .await;
    assert_eq!(ended.phase, Phase::Finished, "{:?}", ended.error);
    f.reaped().await;
    let snapshot = std::fs::read_to_string(f.exe.with_extension("snapshot")).unwrap();
    assert!(!Path::new(&snapshot).exists());
}
#[tokio::test]
async fn normal_window_close_during_speed_command_finishes_the_control_batch() {
    for mode in ["quit-on-speed", "quit-after-observe"] {
        let f = Fixture::new().await;
        let mut request = f.request(1, mode).await;
        request.message.key.id = "closing-video".into();
        if let MessageBody::Media(a) = &mut request.message.body {
            a.kind = AttachmentKind::Video;
        }
        f.store
            .apply_batch(batch(vec![request.message.clone()]))
            .await
            .unwrap();
        let mut player = f.player();
        player.set(Some(request));
        let ended = observed(&mut player, |p| {
            matches!(p.phase, Phase::Finished | Phase::Failed)
        })
        .await;
        assert_eq!(ended.phase, Phase::Finished, "{mode}: {:?}", ended.error);
        f.reaped().await;
        let snapshot = std::fs::read_to_string(f.exe.with_extension("snapshot")).unwrap();
        assert!(!Path::new(&snapshot).exists());
    }
}
#[tokio::test]
async fn native_window_speed_changes_are_reported_as_observed_values() {
    let f = Fixture::new().await;
    let mut player = f.player();
    player.set(Some(f.request(1, "normal").await));
    observed(&mut player, |p| p.phase == Phase::Playing).await;
    f.window(r#"{"event":"property-change","name":"speed","data":1.25}"#);
    let changed = observed(&mut player, |p| p.speed_milli == 1250).await;
    assert_eq!(changed.speed_label(), "1.25x");
    assert_eq!(changed.request.speed, Speed::Normal); // observation is separate from intent
    player.set(None);
    f.reaped().await;
}
#[tokio::test]
async fn pause_speed_and_progress_follow_player_observations_and_stop_reaps_it() {
    let f = Fixture::new().await;
    let mut player = f.player();
    let mut request = f.request(1, "normal").await;
    player.set(Some(request.clone()));
    let p = observed(&mut player, |p| {
        p.phase == Phase::Playing && p.position_ms > 0
    })
    .await;
    assert_eq!(p.duration_ms, Some(20_000));
    let args = std::fs::read_to_string(f.exe.with_extension("args")).unwrap();
    assert!(args.contains("--no-video"), "audio must remain windowless");
    request.paused = true;
    player.set(Some(request.clone()));
    let paused = observed(&mut player, |p| p.phase == Phase::Paused).await;
    request.speed = Speed::OneHalf;
    player.set(Some(request.clone()));
    let fast = observed(&mut player, |p| {
        p.phase == Phase::Paused && p.request.speed == Speed::OneHalf
    })
    .await;
    assert_eq!(fast.position_ms, paused.position_ms);
    request.paused = false;
    player.set(Some(request));
    observed(&mut player, |p| {
        p.phase == Phase::Playing && p.position_ms > paused.position_ms
    })
    .await;
    player.set(None);
    f.reaped().await;
}
#[tokio::test]
async fn replacement_cancels_an_old_download_before_it_can_start_a_player() {
    let f = Fixture::new().await;
    let mut player = f.player();
    player.set(Some(f.request(1, "slow").await));
    f.source.started.notified().await;
    player.set(Some(f.request(2, "normal").await));
    let p = observed(&mut player, |p| p.phase == Phase::Playing).await;
    assert_eq!(p.request.id, RequestId(2));
    assert_eq!(
        std::fs::read_to_string(f.exe.with_extension("starts")).unwrap(),
        "normal\n"
    );
    drop(player);
    f.reaped().await;
}
#[tokio::test]
async fn malformed_stalled_rejected_and_crashed_players_fail_without_leaking() {
    for mode in ["malformed", "stall", "reject", "exit"] {
        let f = Fixture::new().await;
        let mut player = f.player();
        player.set(Some(f.request(1, mode).await));
        let p = observed(&mut player, |p| p.phase == Phase::Failed).await;
        assert!(!p.error.unwrap().is_empty());
        f.reaped().await;
    }
}
#[tokio::test]
async fn missing_player_produces_an_actionable_error() {
    let f = Fixture::new().await;
    let mut player = Player::start(
        f.store.clone(),
        f.source.clone(),
        f.dir.path().join("missing-mpv"),
    );
    player.set(Some(f.request(1, "normal").await));
    let p = observed(&mut player, |p| p.phase == Phase::Failed).await;
    assert!(p.error.unwrap().contains("mpv"));
}
#[tokio::test]
async fn post_start_stalls_fail_and_release_both_child_and_snapshot_even_while_paused() {
    for paused in [false, true] {
        let f = Fixture::new().await;
        let mut player = f.player();
        let mut request = f.request(1, "late-stall").await;
        request.paused = paused;
        player.set(Some(request));
        observed(&mut player, |p| {
            p.phase
                == if paused {
                    Phase::Paused
                } else {
                    Phase::Playing
                }
        })
        .await;
        let snapshot = std::fs::read_to_string(f.exe.with_extension("snapshot")).unwrap();
        assert!(Path::new(&snapshot).exists());
        let failure = observed(&mut player, |p| p.phase == Phase::Failed).await;
        assert!(failure.error.unwrap().contains("did not respond"));
        f.reaped().await;
        assert!(!Path::new(&snapshot).exists());
    }
}
#[tokio::test]
async fn eof_during_a_health_query_finishes_normally() {
    let f = Fixture::new().await;
    let mut player = f.player();
    player.set(Some(f.request(1, "eof-on-health").await));
    observed(&mut player, |p| p.phase == Phase::Finished).await;
    f.reaped().await;
}
#[tokio::test]
async fn eof_finishes_and_expired_offscreen_content_stops_the_player() {
    let f = Fixture::new().await;
    let mut player = f.player();
    player.set(Some(f.request(1, "eof").await));
    observed(&mut player, |p| p.phase == Phase::Finished).await;
    f.reaped().await;
    let request = f.request(2, "normal").await;
    player.set(Some(request.clone()));
    observed(&mut player, |p| p.phase == Phase::Playing).await;
    let mut expired = request.message;
    expired.body = MessageBody::Expired;
    f.store.apply_batch(batch(vec![expired])).await.unwrap();
    observed(&mut player, |p| p.phase == Phase::Failed).await;
    f.reaped().await;
}

#[tokio::test]
#[ignore = "Requires installed mpv; uses silent null audio output"]
async fn installed_mpv_decodes_opus_and_observes_pause_speed_and_eof() {
    struct Opus;
    #[async_trait::async_trait]
    impl Downloader for Opus {
        async fn download(
            &self,
            _: &Attachment,
            path: &Path,
            _: watch::Receiver<bool>,
        ) -> Result<(), String> {
            tokio::fs::write(path, include_bytes!("fixtures/voice.ogg"))
                .await
                .map_err(|e| e.to_string())
        }
    }
    let f = Fixture::new().await;
    std::fs::write(&f.exe, "#!/bin/sh\nexec mpv --ao=null \"$@\"\n").unwrap();
    let mut message = record("opus", "normal");
    let MessageBody::Media(a) = &mut message.body else {
        panic!()
    };
    a.size = include_bytes!("fixtures/voice.ogg").len() as u64;
    a.sha256 = Sha256::digest(include_bytes!("fixtures/voice.ogg")).into();
    f.store
        .apply_batch(batch(vec![message.clone()]))
        .await
        .unwrap();
    let mut player = Player::start(f.store.clone(), Arc::new(Opus), f.exe.clone());
    let mut request = Request {
        id: RequestId(1),
        revision: 0,
        message,
        paused: false,
        speed: Speed::Normal,
    };
    player.set(Some(request.clone()));
    let playing = observed(&mut player, |p| {
        p.phase == Phase::Playing && p.position_ms > 0
    })
    .await;
    assert!(
        playing
            .duration_ms
            .is_some_and(|ms| (7900..8100).contains(&ms))
    );
    request.paused = true;
    player.set(Some(request.clone()));
    observed(&mut player, |p| p.phase == Phase::Paused).await;
    request.speed = Speed::Double;
    request.paused = false;
    player.set(Some(request));
    observed(&mut player, |p| {
        p.phase == Phase::Playing && p.request.speed == Speed::Double
    })
    .await;
    observed(&mut player, |p| p.phase == Phase::Finished).await;
}

#[tokio::test]
#[ignore = "Requires installed mpv; uses null video and silent null audio output"]
async fn installed_mpv_decodes_video_and_observes_pause_speed_and_eof() {
    const VIDEO: &[u8] = include_bytes!("fixtures/video.mp4");
    struct Video;
    #[async_trait::async_trait]
    impl Downloader for Video {
        async fn download(
            &self,
            _: &Attachment,
            path: &Path,
            _: watch::Receiver<bool>,
        ) -> Result<(), String> {
            tokio::fs::write(path, VIDEO)
                .await
                .map_err(|e| e.to_string())
        }
    }
    let f = Fixture::new().await;
    // The production command is unchanged except for the output devices. This
    // exercises decoding and IPC without requiring a display or making sound.
    std::fs::write(&f.exe, "#!/usr/bin/python3\nimport os, sys\nargs = [a for a in sys.argv[1:] if not a.startswith(('--vo=', '--force-window='))]\nos.execvp('mpv', ['mpv', '--vo=null', '--ao=null', '--force-window=no'] + args)\n").unwrap();
    let mut message = record("video-codec", "normal");
    let MessageBody::Media(a) = &mut message.body else {
        panic!()
    };
    a.kind = AttachmentKind::Video;
    a.mime = Some("video/mp4".into());
    a.size = VIDEO.len() as u64;
    a.sha256 = Sha256::digest(VIDEO).into();
    f.store
        .apply_batch(batch(vec![message.clone()]))
        .await
        .unwrap();
    let mut player = Player::start(f.store.clone(), Arc::new(Video), f.exe.clone());
    let mut request = Request {
        id: RequestId(1),
        revision: 0,
        message,
        paused: false,
        speed: Speed::Normal,
    };
    player.set(Some(request.clone()));
    let playing = observed(&mut player, |p| {
        p.phase == Phase::Playing && p.position_ms > 0
    })
    .await;
    assert!(
        playing
            .duration_ms
            .is_some_and(|n| (2900..3100).contains(&n))
    );
    request.paused = true;
    player.set(Some(request.clone()));
    observed(&mut player, |p| p.phase == Phase::Paused).await;
    request.paused = false;
    request.speed = Speed::Double;
    player.set(Some(request));
    observed(&mut player, |p| {
        p.phase == Phase::Playing && p.speed_milli == 2000
    })
    .await;
    observed(&mut player, |p| p.phase == Phase::Finished).await;
}
