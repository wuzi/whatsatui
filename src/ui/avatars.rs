//! Small profile photos have a separate budget from message previews.
use crate::avatars::{Cache, Identity, Provider};
use ratatui::{
    Frame,
    layout::{Rect, Size},
    widgets::Paragraph,
};
use ratatui_image::{
    FontSize,
    sliced::{SlicedImage, SlicedProtocol},
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::oneshot;
use unicode_segmentation::UnicodeSegmentation;

const CAPACITY: usize = 32;
const REFRESH_INTERVAL: Duration = Duration::from_secs(60);
struct Ready {
    protocol: Option<SlicedProtocol>,
    fingerprint: Option<[u8; 32]>,
    id: u32,
    refresh: Instant,
}
enum Refresh {
    Unchanged,
    Replace(Ready),
}
struct Job {
    token: String,
    task: tokio::task::JoinHandle<()>,
    result: oneshot::Receiver<Result<Refresh, String>>,
}
pub struct Avatars {
    service: Option<(Cache, Arc<dyn Provider>)>,
    wanted: Vec<Identity>,
    cache: HashMap<String, Ready>,
    active: Option<Job>,
    kitty: bool,
    font: FontSize,
    serial: u32,
    cleanup: String,
    stopped: bool,
}
impl Default for Avatars {
    fn default() -> Self {
        Self {
            service: None,
            wanted: vec![],
            cache: HashMap::new(),
            active: None,
            kitty: false,
            font: FontSize {
                width: 10,
                height: 20,
            },
            serial: 0,
            cleanup: String::new(),
            stopped: false,
        }
    }
}
impl Avatars {
    pub fn new(root: PathBuf, source: Arc<dyn Provider>, images: &super::Images) -> Self {
        let (kitty, font) = images.settings();
        let mut result = Self::default();
        result.service = Some((Cache::new(root), source));
        result.kitty = kitty;
        result.font = font;
        result
    }
    pub fn begin_frame(&mut self) {
        self.wanted.clear();
    }
    pub fn draw(
        &mut self,
        frame: &mut Frame,
        identity: Identity,
        name: &str,
        area: Rect,
        skip: u16,
    ) {
        if area.is_empty() || self.stopped {
            return;
        }
        let token = identity.token();
        if self.wanted.len() < CAPACITY && !self.wanted.contains(&identity) {
            self.wanted.push(identity);
        }
        if let Some(protocol) = self.cache.get(&token).and_then(|r| r.protocol.as_ref()) {
            frame.render_widget(SlicedImage::new(protocol, (0, -(skip as i16)).into()), area);
        } else if skip == 0 {
            let safe = super::single(name);
            let words: Vec<_> = safe.split_whitespace().collect();
            let initials = if words.len() > 1 {
                words
                    .iter()
                    .take(2)
                    .filter_map(|s| s.graphemes(true).next())
                    .collect::<String>()
            } else {
                safe.graphemes(true).take(2).collect()
            };
            frame.render_widget(Paragraph::new(initials.to_uppercase()).centered(), area);
        }
    }
    pub fn end_frame(&mut self) {
        let wanted: Vec<_> = self.wanted.iter().map(Identity::token).collect();
        let removed: Vec<_> = self
            .cache
            .keys()
            .filter(|k| !wanted.contains(k))
            .cloned()
            .collect();
        for token in removed {
            self.remove(&token);
        }
        if self
            .active
            .as_ref()
            .is_some_and(|j| !wanted.contains(&j.token))
            && let Some(job) = self.active.take()
        {
            job.task.abort();
        }
        self.start_next();
    }
    fn remove(&mut self, token: &str) {
        if let Some(ready) = self.cache.remove(token)
            && self.kitty
            && ready.protocol.is_some()
        {
            self.cleanup
                .push_str(&format!("\x1b_Ga=d,d=I,i={},q=2;\x1b\\", ready.id));
        }
    }
    pub fn poll(&mut self) -> bool {
        let received = self
            .active
            .as_mut()
            .and_then(|j| match j.result.try_recv() {
                Ok(result) => Some(result),
                Err(oneshot::error::TryRecvError::Closed) => {
                    Some(Err("Photo preparation stopped".into()))
                }
                Err(oneshot::error::TryRecvError::Empty) => None,
            });
        let mut changed = false;
        if let Some(result) = received {
            let job = self.active.take().expect("finished avatar");
            if !self.stopped && self.wanted.iter().any(|i| i.token() == job.token) {
                match result {
                    Ok(Refresh::Replace(ready)) => {
                        // Keep the old image until its replacement is ready.
                        // Runtime draws the new frame before flushing cleanup.
                        self.remove(&job.token);
                        self.cache.insert(job.token, ready);
                        changed = true;
                    }
                    Ok(Refresh::Unchanged) | Err(_) => {
                        // A freshness check or transient failure must not
                        // discard the visible protocol (and its Kitty ID).
                        let ready = self.cache.entry(job.token).or_insert(Ready {
                            protocol: None,
                            fingerprint: None,
                            id: 0,
                            refresh: Instant::now(),
                        });
                        ready.refresh = Instant::now() + REFRESH_INTERVAL;
                    }
                }
            }
        }
        self.start_next();
        changed
    }
    fn start_next(&mut self) {
        if self.stopped || self.active.is_some() {
            return;
        }
        let Some((cache, source)) = &self.service else {
            return;
        };
        let Some(identity) = self
            .wanted
            .iter()
            .find(|i| {
                self.cache
                    .get(&i.token())
                    .is_none_or(|r| r.refresh <= Instant::now())
            })
            .cloned()
        else {
            return;
        };
        let token = identity.token();
        let previous = self.cache.get(&token).and_then(|r| r.fingerprint);
        let (cache, source) = (cache.clone(), source.clone());
        let (kitty, font) = (self.kitty, self.font);
        self.serial = self.serial.wrapping_add(1).max(1);
        let id = 0x7800_0000 | (self.serial & 0x00ff_ffff);
        let (send, result) = oneshot::channel();
        let task = tokio::spawn(async move {
            let result = async {
                let photo = cache
                    .load(
                        source.as_ref(),
                        &identity,
                        chrono::Utc::now().timestamp_millis(),
                    )
                    .await?;
                tokio::task::spawn_blocking(move || {
                    let fingerprint = photo
                        .as_ref()
                        .map(|image| Sha256::digest(image.to_rgba8().as_raw()).into());
                    if fingerprint == previous {
                        return Ok(Refresh::Unchanged);
                    }
                    let protocol = photo
                        .map(|image| {
                            super::images::prepare(
                                image,
                                Size::new(4, 2),
                                font,
                                kitty.then_some(id),
                            )
                        })
                        .transpose()?;
                    Ok(Refresh::Replace(Ready {
                        protocol,
                        fingerprint,
                        id,
                        refresh: Instant::now() + REFRESH_INTERVAL,
                    }))
                })
                .await
                .map_err(|_| "Cannot prepare profile photo".to_owned())?
            }
            .await;
            let _ = send.send(result);
        });
        self.active = Some(Job {
            token,
            task,
            result,
        });
    }
    pub fn take_cleanup(&mut self) -> String {
        std::mem::take(&mut self.cleanup)
    }
    pub fn stop(&mut self) {
        self.stopped = true;
        self.wanted.clear();
        if let Some(job) = self.active.take() {
            job.task.abort();
        }
        for key in self.cache.keys().cloned().collect::<Vec<_>>() {
            self.remove(&key);
        }
    }
}
impl Drop for Avatars {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests;
