//! Visible-media scheduler and bounded terminal protocol cache.
use crate::{
    app::model::*,
    config::ImageProtocol,
    media::{self, Downloader},
    storage::Store,
};
use ratatui::{
    Frame,
    layout::{Rect, Size},
    widgets::Paragraph,
};
use ratatui_image::{
    FontSize, Resize,
    picker::Picker,
    protocol::kitty::Kitty,
    sliced::{SlicedImage, SlicedProtocol},
};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{oneshot, watch};

pub const PREVIEW_ROWS: u16 = 8;
const CAPACITY: usize = 8;

#[derive(Clone)]
struct Request {
    id: String,
    message: MessageRecord,
    size: Size,
}
struct Ready {
    protocol: Result<SlicedProtocol, String>,
    kitty_id: u32,
    retry_at: Instant,
}
struct Job {
    id: String,
    stop: watch::Sender<bool>,
    receive: oneshot::Receiver<Ready>,
}

pub struct Images {
    service: Option<(Store, Arc<dyn Downloader>)>,
    kitty: bool,
    font: FontSize,
    wanted: Vec<Request>,
    cache: HashMap<String, Ready>,
    active: Option<Job>,
    serial: u32,
    cleanup: String,
    stopped: bool,
}
impl Default for Images {
    fn default() -> Self {
        Self {
            service: None,
            kitty: false,
            font: FontSize {
                width: 10,
                height: 20,
            },
            wanted: vec![],
            cache: HashMap::new(),
            active: None,
            serial: 0,
            cleanup: String::new(),
            stopped: false,
        }
    }
}

pub fn use_kitty(protocol: ImageProtocol, term_program: &str, term: &str, indirect: bool) -> bool {
    !indirect
        && match protocol {
            ImageProtocol::Kitty => true,
            ImageProtocol::Halfblocks => false,
            ImageProtocol::Auto => {
                term_program.eq_ignore_ascii_case("ghostty")
                    || term.contains("kitty")
                    || term.contains("ghostty")
            }
        }
}

impl Images {
    pub fn new(store: Store, downloader: Arc<dyn Downloader>, protocol: ImageProtocol) -> Self {
        let mut result = Self::default();
        result.service = Some((store, downloader));
        result.kitty = use_kitty(
            protocol,
            &std::env::var("TERM_PROGRAM").unwrap_or_default(),
            &std::env::var("TERM").unwrap_or_default(),
            std::env::var_os("TMUX").is_some() || std::env::var_os("SSH_CONNECTION").is_some(),
        );
        if let Ok(size) = crossterm::terminal::window_size()
            && size.columns > 0
            && size.rows > 0
            && size.width >= size.columns
            && size.height >= size.rows
        {
            result.font = FontSize {
                width: (size.width / size.columns).clamp(1, 64),
                height: (size.height / size.rows).clamp(1, 128),
            };
        }
        result
    }
    pub fn begin_frame(&mut self) {
        self.wanted.clear();
    }
    pub fn draw(
        &mut self,
        frame: &mut Frame,
        message: &MessageRecord,
        area: Rect,
        skip: u16,
        full_size: Size,
    ) {
        if area.is_empty() || self.stopped {
            return;
        }
        let size = Size::new(full_size.width.clamp(1, 40), full_size.height.max(1));
        let id = format!(
            "{:x}-{}x{}",
            Sha256::digest(
                serde_json::to_vec(&(&message.key, &message.body)).expect("media identity")
            ),
            size.width,
            size.height
        );
        if self.wanted.len() < CAPACITY && !self.wanted.iter().any(|r| r.id == id) {
            self.wanted.push(Request {
                id: id.clone(),
                message: message.clone(),
                size,
            });
        }
        match self.cache.get(&id).map(|r| &r.protocol) {
            Some(Ok(protocol)) => {
                frame.render_widget(SlicedImage::new(protocol, (0, -(skip as i16)).into()), area)
            }
            state if skip == 0 => {
                let label = match state {
                    Some(Err(error)) => format!("[{}]", super::single(error)),
                    _ => "[Loading preview…]".into(),
                };
                frame.render_widget(Paragraph::new(label), Rect { height: 1, ..area });
            }
            _ => {}
        }
    }
    pub fn end_frame(&mut self) {
        let removed: Vec<_> = self
            .cache
            .keys()
            .filter(|id| !self.wanted.iter().any(|r| &r.id == *id))
            .cloned()
            .collect();
        for id in removed {
            self.remove(&id);
        }
        if let Some(job) = &self.active
            && !self.wanted.iter().any(|r| r.id == job.id)
        {
            let _ = job.stop.send(true);
        }
        self.start_next();
    }
    fn remove(&mut self, id: &str) {
        if let Some(ready) = self.cache.remove(id)
            && self.kitty
            && ready.protocol.is_ok()
        {
            self.cleanup
                .push_str(&format!("\x1b_Ga=d,d=I,i={},q=2;\x1b\\", ready.kitty_id));
        }
    }
    pub fn take_cleanup(&mut self) -> String {
        std::mem::take(&mut self.cleanup)
    }
    pub fn poll(&mut self) -> bool {
        let result = self
            .active
            .as_mut()
            .and_then(|j| match j.receive.try_recv() {
                Ok(ready) => Some(Some(ready)),
                Err(oneshot::error::TryRecvError::Closed) => Some(None),
                Err(oneshot::error::TryRecvError::Empty) => None,
            });
        let mut changed = false;
        if let Some(result) = result {
            let job = self.active.take().expect("completed preview");
            if let Some(ready) = result
                && self.wanted.iter().any(|r| r.id == job.id)
                && !self.stopped
                && !*job.stop.borrow()
            {
                self.cache.insert(job.id, ready);
            }
            changed = true;
        }
        let retry: Vec<_> = self
            .cache
            .iter()
            .filter(|(_, r)| r.protocol.is_err() && Instant::now() >= r.retry_at)
            .map(|(id, _)| id.clone())
            .collect();
        for id in retry {
            self.remove(&id);
            changed = true;
        }
        self.start_next();
        changed
    }
    fn start_next(&mut self) {
        if self.active.is_some() || self.stopped {
            return;
        }
        let Some((store, downloader)) = &self.service else {
            return;
        };
        let Some(request) = self
            .wanted
            .iter()
            .find(|r| !self.cache.contains_key(&r.id))
            .cloned()
        else {
            return;
        };
        let store = store.clone();
        let downloader = downloader.clone();
        let kitty = self.kitty;
        let font = self.font;
        self.serial = self.serial.wrapping_add(1).max(1);
        let kitty_id = 0x7700_0000 | (self.serial & 0x00ff_ffff);
        let (stop, cancel) = watch::channel(false);
        let (send, receive) = oneshot::channel();
        self.active = Some(Job {
            id: request.id,
            stop,
            receive,
        });
        tokio::spawn(async move {
            let result =
                media::preview::load(request.message, store, downloader.as_ref(), cancel).await;
            let protocol = match result {
                Ok(image) => tokio::task::spawn_blocking(move || {
                    prepare(image, request.size, font, kitty.then_some(kitty_id))
                })
                .await
                .unwrap_or_else(|_| Err("Preview preparation failed".into())),
                Err(error) => Err(error),
            };
            let delay = if protocol.as_ref().is_err_and(|e| e.contains("busy")) {
                1
            } else {
                60
            };
            let _ = send.send(Ready {
                protocol,
                kitty_id,
                retry_at: Instant::now() + Duration::from_secs(delay),
            });
        });
    }
    pub fn stop(&mut self) {
        self.stopped = true;
        self.wanted.clear();
        if let Some(job) = &self.active {
            let _ = job.stop.send(true);
        }
        let ids: Vec<_> = self.cache.keys().cloned().collect();
        for id in ids {
            self.remove(&id);
        }
    }
}
impl Drop for Images {
    fn drop(&mut self) {
        self.stop();
    }
}

fn prepare(
    image: image::DynamicImage,
    size: Size,
    font: FontSize,
    kitty_id: Option<u32>,
) -> Result<SlicedProtocol, String> {
    let resize = Resize::Scale(Some(image::imageops::FilterType::Triangle));
    if let Some(id) = kitty_id {
        let actual = resize.size_for(&image, font, size);
        let image = resize.resize(&image, font, actual, None);
        Kitty::new(image, actual, id, false, false)
            .map(SlicedProtocol::Kitty)
            .map_err(|_| "Cannot prepare terminal image".into())
    } else {
        SlicedProtocol::new_with_resize(&Picker::halfblocks(), image, size, resize)
            .map_err(|_| "Cannot prepare terminal image".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ghostty_is_automatic_without_terminal_queries() {
        assert!(use_kitty(
            ImageProtocol::Auto,
            "ghostty",
            "xterm-256color",
            false
        ));
        assert!(!use_kitty(
            ImageProtocol::Auto,
            "ghostty",
            "xterm-256color",
            true
        ));
        assert!(!use_kitty(
            ImageProtocol::Halfblocks,
            "ghostty",
            "xterm-256color",
            false
        ));
        assert!(!use_kitty(ImageProtocol::Auto, "", "xterm", false));
    }
}
