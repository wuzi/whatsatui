use super::{Phase, Playback, Request, mpv::Mpv};
use crate::{
    media::{self, Downloader},
    storage::Store,
};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::watch;

pub(super) async fn run(
    store: Store,
    downloader: Arc<dyn Downloader>,
    executable: PathBuf,
    mut desired: watch::Receiver<Option<Request>>,
    events: watch::Sender<Option<Playback>>,
) {
    loop {
        let request = desired.borrow_and_update().clone();
        let Some(request) = request else {
            if desired.changed().await.is_err() {
                return;
            }
            continue;
        };
        let mut result = Box::pin(session(
            &request,
            &store,
            downloader.as_ref(),
            &executable,
            desired.clone(),
            &events,
        ));
        loop {
            tokio::select! {
                biased;
                change=desired.changed()=>{
                    if change.is_err(){return;}
                    if desired.borrow().as_ref().is_none_or(|r|!request.same_source(r)) {break;}
                }
                outcome=&mut result=>{
                    if let Err(error)=outcome {
                        let mut status=events.borrow().clone().filter(|p|p.request.same_source(&request)).unwrap_or_else(||Playback::loading(request.clone()));
                        status.phase=Phase::Failed;status.error=Some(error);events.send_replace(Some(status));
                    }
                    // Finished/failed audio restarts only on a new generation.
                    loop {
                        if desired.changed().await.is_err(){return;}
                        if desired.borrow().as_ref().is_none_or(|r|!request.same_source(r)){break;}
                    }
                    break;
                }
            }
        }
        // Dropping the session cancels preparation and closes/kills the child
        // before the next generation is allowed to start.
    }
}
async fn session(
    initial: &Request,
    store: &Store,
    downloader: &dyn Downloader,
    executable: &Path,
    mut desired: watch::Receiver<Option<Request>>,
    events: &watch::Sender<Option<Playback>>,
) -> Result<(), String> {
    let mut playback = Playback::loading(initial.clone());
    events.send_replace(Some(playback.clone()));
    let (_cancel, cancelled) = watch::channel(false);
    let snapshot = media::audio::prepare(&initial.message, store, downloader, cancelled).await?;
    let mut player = Mpv::start(executable, snapshot.path(), initial.is_video()).await?;
    let mut request = desired
        .borrow_and_update()
        .clone()
        .filter(|r| r.same_source(initial))
        .ok_or("Playback canceled")?;
    apply_controls(&mut player, &request).await?;
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut validation = tokio::time::interval(Duration::from_secs(1));
    validation.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut health = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_secs(2),
        Duration::from_secs(2),
    );
    health.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let startup = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        playback.request = request.clone();
        playback.position_ms = player.state.position;
        playback.speed_milli = player.state.speed_milli;
        playback.duration_ms = player.state.duration.or(playback.duration_ms);
        playback.phase = if player.state.finished {
            Phase::Finished
        } else if !player.state.loaded {
            Phase::Loading
        } else if player.state.paused {
            Phase::Paused
        } else {
            Phase::Playing
        };
        if player.state.finished {
            events.send_replace(Some(playback));
            return Ok(());
        }
        tokio::select! {
            change=desired.changed()=>{
                change.map_err(|_|"Playback canceled")?;
                let next=desired.borrow_and_update().clone().filter(|r|r.same_source(initial)).ok_or("Playback canceled")?;
                apply_controls(&mut player,&next).await?;request=next;
            }
            value=player.read()=>{value?;}
            _=tick.tick()=>{
                if !player.state.loaded && tokio::time::Instant::now()>=startup {return Err("Media player could not load this file".into());}
                events.send_replace(Some(playback.clone()));
            }
            _=validation.tick()=>{media::current(&initial.message,store).await?;}
            _=health.tick()=>{
                // Paused audio legitimately emits no progress. Require an IPC
                // reply instead, using the same bounded deadline as controls.
                player.command(serde_json::json!(["get_property", "pause"])).await?;
            }
        }
    }
}
async fn apply_controls(player: &mut Mpv, request: &Request) -> Result<(), String> {
    player
        .command(serde_json::json!([
            "set_property",
            "speed",
            request.speed.value()
        ]))
        .await?;
    player
        .command(serde_json::json!(["set_property", "pause", request.paused]))
        .await
}
