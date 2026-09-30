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
