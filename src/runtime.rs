use crate::{
    app::{App, ViewModel},
    config::{Config, Paths},
    storage::Store,
    whatsapp::{BackendError, BackendHandle},
};
use std::{future::Future, path::PathBuf};
#[derive(clap::Parser, Clone, Debug, Default)]
#[command(version, about = "Personal WhatsApp conversations in your terminal")]
pub struct Options {
    #[arg(long, conflicts_with = "data_dir")]
    pub demo: bool,
    #[arg(long)]
    pub config: Option<PathBuf>,
    #[arg(long)]
    pub data_dir: Option<PathBuf>,
}
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Config(#[from] crate::config::ConfigError),
    #[error("{0}")]
    Store(#[from] crate::storage::StoreError),
    #[error("{0}")]
    Backend(#[from] BackendError),
    #[error("Terminal input/output failed")]
    Terminal(#[from] std::io::Error),
    #[error("{0}")]
    Arguments(&'static str),
    #[error("{0}")]
    Media(String),
}
pub trait Screen {
    fn poll(&mut self) -> bool {
        false
    }
    fn stop_media(&mut self) {}
    fn draw(&mut self, view: &ViewModel, config: &Config) -> std::io::Result<()>;
    fn finish(&mut self) -> std::io::Result<()>;
    fn timeline_viewport(&self) -> Option<crate::app::TimelineViewport> {
        None
    }
    fn interaction_map(&self) -> Option<crate::ui::InteractionMap> {
        None
    }
}
use crate::{
    app::{Effect, Input, StoreCompletion},
    storage::paths::DataDirGuard,
    whatsapp::{BackendCommand, BackendEvent},
};
use futures_util::{Stream, StreamExt};
use std::collections::VecDeque;
use tokio::{
    sync::mpsc,
    task::JoinSet,
    time::{Duration, Instant},
};
pub struct Session {
    pub app: App,
    pub store: Store,
    pub backend: BackendHandle,
    path: PathBuf,
    _guard: DataDirGuard,
    _temporary: Option<tempfile::TempDir>,
}
impl Session {
    pub fn data_path(&self) -> &std::path::Path {
        &self.path
    }
    pub async fn run(self) -> Result<(), AppError> {
        run(self.app, self.store, self.backend).await
    }
}
pub async fn prepare_with<F, Fut>(
    options: Options,
    paths: Paths,
    factory: F,
) -> Result<Session, AppError>
where
    F: FnOnce(PathBuf, Store) -> Fut,
    Fut: Future<Output = Result<BackendHandle, BackendError>>,
{
    if options.demo && options.data_dir.is_some() {
        return Err(AppError::Arguments(
            "--demo cannot be combined with --data-dir",
        ));
    }
    let config = if let Some(path) = options.config {
        Config::load(&path)?
    } else if options.demo {
        Config::default()
    } else {
        Config::load(&paths.config)?
    };
    let temporary = if options.demo {
        Some(
            tempfile::Builder::new()
                .prefix("whatsapp-tui-demo-")
                .tempdir()?,
        )
    } else {
        None
    };
    let path = temporary
        .as_ref()
        .map(|d| d.path().to_owned())
        .or(options.data_dir)
        .unwrap_or(paths.data);
    let guard = DataDirGuard::acquire(&path)?;
    let store = Store::open(path.join("chat.sqlite3")).await?;
    crate::media::prune(store.clone())
        .await
        .map_err(AppError::Media)?;
    let backend = if options.demo {
        crate::whatsapp::demo::start(store.clone())
    } else {
        factory(path.join("session.sqlite3"), store.clone()).await?
    };
    Ok(Session {
        app: App::new(config),
        store,
        backend,
        path,
        _guard: guard,
        _temporary: temporary,
    })
}
struct TerminalScreen {
    avatars: crate::ui::Avatars,
    images: crate::ui::Images,
    terminal: ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    guard: crate::terminal::TerminalGuard,
    viewport: Option<crate::app::TimelineViewport>,
    interaction: Option<crate::ui::InteractionMap>,
}
impl Screen for TerminalScreen {
    fn poll(&mut self) -> bool {
        self.images.poll() | self.avatars.poll()
    }
    fn stop_media(&mut self) {
        self.images.stop();
        self.avatars.stop();
    }
    fn draw(&mut self, view: &ViewModel, config: &Config) -> std::io::Result<()> {
        self.terminal.draw(|frame| {
            self.viewport = crate::ui::timeline_viewport(frame.area(), view, config);
            self.interaction = Some(crate::ui::render_interactive(
                frame,
                view,
                config,
                &mut self.images,
                &mut self.avatars,
            ));
        })?;
        self.clean_images()?;
        Ok(())
    }
    fn finish(&mut self) -> std::io::Result<()> {
        self.images.stop();
        self.avatars.stop();
        self.clean_images()?;
        self.guard.restore()
    }
    fn timeline_viewport(&self) -> Option<crate::app::TimelineViewport> {
        self.viewport.clone()
    }
    fn interaction_map(&self) -> Option<crate::ui::InteractionMap> {
        self.interaction.clone()
    }
}
impl TerminalScreen {
    fn clean_images(&mut self) -> std::io::Result<()> {
        use std::io::Write;
        let cleanup = self.images.take_cleanup() + &self.avatars.take_cleanup();
        if !cleanup.is_empty() {
            std::io::stdout().write_all(cleanup.as_bytes())?;
            std::io::stdout().flush()?;
        }
        Ok(())
    }
}
impl Drop for TerminalScreen {
    fn drop(&mut self) {
        self.images.stop();
        self.avatars.stop();
        let _ = self.clean_images();
    }
}
pub async fn run(mut app: App, store: Store, backend: BackendHandle) -> Result<(), AppError> {
    let guard = crate::terminal::TerminalGuard::enter_with_mouse(app.config.ui.mouse)?;
    let terminal =
        ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))?;
    let truecolor = std::env::var("COLORTERM").is_ok_and(|s| s == "truecolor" || s == "24bit")
        || std::env::var("TERM").is_ok_and(|s| s.contains("direct"));
    app.set_truecolor(truecolor);
    let images = crate::ui::Images::new(
        store.clone(),
        backend.media.clone(),
        app.config.media.protocol,
    );
    let avatars = crate::ui::Avatars::new(
        store.data_dir().to_owned(),
        backend.profiles.clone(),
        &images,
    );
    let mut screen = TerminalScreen {
        avatars,
        images,
        terminal,
        guard,
        viewport: None,
        interaction: None,
    };
    run_with_screen(
        app,
        store,
        backend,
        &mut screen,
        crossterm::event::EventStream::new(),
    )
    .await
}
pub async fn execute(
    effect: Effect,
    store: Store,
    commands: mpsc::Sender<BackendCommand>,
) -> Option<Input> {
    let (_stop, cancel) = tokio::sync::watch::channel(false);
    execute_with_media(
        effect,
        store,
        commands,
        &crate::media::NativeDownloader,
        cancel,
    )
    .await
}
async fn execute_with_media(
    effect: Effect,
    store: Store,
    commands: mpsc::Sender<BackendCommand>,
    downloader: &dyn crate::media::Downloader,
    cancel: tokio::sync::watch::Receiver<bool>,
) -> Option<Input> {
    let event = match effect {
        Effect::Mutate {
            request,
            message,
            kind,
        } => {
            let account = message.key.account.clone();
            if commands
                .send(BackendCommand::Mutate {
                    request,
                    message,
                    kind,
                })
                .await
                .is_err()
            {
                return Some(Input::Backend(BackendEvent::MutationOutcome {
                    request,
                    account,
                    result: Err("WhatsApp service is unavailable; action was not sent".into()),
                }));
            }
            return None;
        }
        Effect::PasteClipboard {
            request,
            account,
            chat,
            sticker,
        } => {
            let result = clipboard_paste(store.data_dir().to_owned(), sticker, cancel).await;
            return Some(Input::ClipboardRead {
                request,
                account,
                chat,
                result,
            });
        }
        Effect::LoadStickers {
            request,
            account,
            chat,
        } => StoreCompletion::Stickers {
            request,
            account: account.clone(),
            chat,
            result: store
                .recent_stickers(account, chrono::Utc::now().timestamp_millis())
                .await
                .map_err(|e| e.to_string()),
        },
        Effect::ImportSticker {
            request,
            account,
            chat,
            message,
        } => {
            let result = if message.key.account != account {
                Err("Sticker belongs to a different account".into())
            } else {
                crate::media::stickers::import(*message, store, downloader, cancel).await
            };
            return Some(Input::StickerImported {
                request,
                account,
                chat,
                result,
            });
        }
        Effect::ImportImage {
            request,
            account,
            chat,
            path,
        } => {
            let root = store.data_dir().to_owned();
            let mut cancel = cancel;
            if *cancel.borrow() {
                return None;
            }
            let permit = tokio::select! {
                permit = crate::media::preview::DECODERS.acquire() => permit.expect("decoder semaphore stays open"),
                _ = cancel.changed() => return None,
            };
            let result = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                let path = if let Some(rest) = path.strip_prefix("~/") {
                    std::path::PathBuf::from(
                        std::env::var_os("HOME").ok_or("Home directory is unavailable")?,
                    )
                    .join(rest)
                } else {
                    std::path::PathBuf::from(path)
                };
                crate::media::outgoing::import(&path, &root)
            })
            .await
            .unwrap_or_else(|_| Err("Image preparation failed".into()));
            return Some(Input::ImageImported {
                request,
                account,
                chat,
                result,
            });
        }
        Effect::MediaAction {
            request,
            message,
            action,
        } => {
            let account = message.key.account.clone();
            let result = crate::media::execute(
                *message,
                action,
                store,
                downloader,
                &crate::desktop::NativeDesktop,
                cancel,
            )
            .await;
            return Some(Input::DesktopAction {
                request,
                account,
                result,
            });
        }
        Effect::DesktopAction {
            request,
            message,
            action,
        } => {
            let account = message.key.account.clone();
            let result =
                crate::desktop::execute(*message, action, store, &crate::desktop::NativeDesktop)
                    .await;
            return Some(Input::DesktopAction {
                request,
                account,
                result,
            });
        }
        Effect::SearchMessages {
            request,
            account,
            chat,
            query,
        } => StoreCompletion::MessageSearch {
            request,
            account: account.clone(),
            chat: chat.clone(),
            query: query.clone(),
            result: store
                .search_messages(account, chat, query, chrono::Utc::now().timestamp_millis())
                .await
                .map_err(|e| e.to_string()),
        },
        Effect::LoadChats { request, account } => StoreCompletion::Chats {
            request,
            account: account.clone(),
            result: store.list_chats(account).await.map_err(|e| e.to_string()),
        },
        Effect::LoadChat {
            request,
            account,
            chat,
            cursor,
        } => StoreCompletion::Chat {
            request,
            account: account.clone(),
            chat: chat.clone(),
            cursor: cursor.clone(),
            result: store
                .snapshot(account, chat, cursor)
                .await
                .map(Box::new)
                .map_err(|e| e.to_string()),
        },
        Effect::SaveDraft {
            request,
            account,
            chat,
            draft,
        } => StoreCompletion::DraftSaved {
            request,
            account: account.clone(),
            chat: chat.clone(),
            revision: draft.revision,
            result: store
                .save_draft(account, chat, draft)
                .await
                .map_err(|e| e.to_string()),
        },
        Effect::Prepare {
            request,
            chat,
            draft,
            ..
        } => {
            if commands
                .send(BackendCommand::PrepareText {
                    request,
                    chat,
                    draft,
                })
                .await
                .is_err()
            {
                return Some(Input::Backend(BackendEvent::PreparationFailed {
                    request,
                    reason: "WhatsApp service is unavailable; draft kept".into(),
                }));
            }
            return None;
        }
        Effect::Stage {
            request,
            message,
            preserve_draft,
        } => StoreCompletion::Staged {
            request,
            message: Box::new(message.clone()),
            result: if preserve_draft {
                store.stage_resend(message).await
            } else {
                store.stage_outgoing(message).await
            }
            .map_err(|e| e.to_string()),
        },
        Effect::Transmit(message) => {
            let message = match store.stored_outbound(message.clone()).await {
                Ok(message) => message,
                Err(e) => {
                    return Some(Input::Store(StoreCompletion::Changed {
                        account: message.key.account,
                        result: Err(e.to_string()),
                    }));
                }
            };
            if commands
                .send(BackendCommand::Transmit(message.clone()))
                .await
                .is_ok()
            {
                return None;
            }
            StoreCompletion::Changed {
                account: message.key.account.clone(),
                result: store
                    .set_send_state(message.key, crate::app::model::SendState::Unconfirmed)
                    .await
                    .map_err(|e| e.to_string()),
            }
        }
        Effect::PersistOutcome { key, state } => StoreCompletion::Changed {
            account: key.account.clone(),
            result: store
                .set_send_state(key, state)
                .await
                .map_err(|e| e.to_string()),
        },
        Effect::RecoverAccount(account) => {
            let result = async {
                store.recover_sends(account.clone()).await?;
                store
                    .expire(account.clone(), chrono::Utc::now().timestamp_millis())
                    .await?;
                let chats = store
                    .list_chats(account.clone())
                    .await?
                    .into_iter()
                    .map(|c| c.chat)
                    .collect();
                Ok(crate::app::model::StoreChange {
                    account: account.clone(),
                    chats,
                })
            }
            .await;
            StoreCompletion::Changed {
                account,
                result: result.map_err(|e: crate::storage::StoreError| e.to_string()),
            }
        }
        Effect::MarkRead {
            account,
            chat,
            keys,
        } => {
            let result = store
                .mark_read(account.clone(), chat.clone(), keys.clone())
                .await
                .map(|_| ())
                .map_err(|e| e.to_string());
            if result.is_ok() && !keys.is_empty() {
                let _ = commands.send(BackendCommand::MarkRead(keys.clone())).await;
            }
            StoreCompletion::Read {
                account,
                chat,
                keys,
                result,
            }
        }
        Effect::Expire { account, now_ms } => {
            let mut result = store
                .expire(account.clone(), now_ms)
                .await
                .map_err(|e| e.to_string());
            // Expiry runs once a second; scan managed downloads about twice a minute.
            if result.is_ok()
                && now_ms.div_euclid(1000) % 30 == 0
                && let Err(error) = crate::media::prune(store.clone()).await
            {
                result = Err(error);
            }
            StoreCompletion::Changed { account, result }
        }
        Effect::Shutdown => return None,
    };
    Some(Input::Store(event))
}
pub async fn run_with_screen<S, I>(
    mut app: App,
    store: Store,
    backend: BackendHandle,
    screen: &mut S,
    mut input: I,
) -> Result<(), AppError>
where
    S: Screen,
    I: Stream<Item = std::io::Result<crossterm::event::Event>> + Unpin,
{
    let BackendHandle {
        profiles: _,
        commands,
        mut events,
        control,
        media,
    } = backend;
    let (cancel_media, media_cancelled) = tokio::sync::watch::channel(false);
    let mut control = Some(control);
    let mut pending = VecDeque::new();
    let mut jobs = JoinSet::new();
    let mut command_jobs = 0usize;
    let mut closing = false;
    let mut input_closed = false;
    let mut events_closed = false;
    let mut shutdown_done = false;
    let mut shutdown: Option<futures_util::future::BoxFuture<'static, Result<(), BackendError>>> =
        None;
    let mut shutdown_error = None;
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut dirty = true;
    let mut last_second = chrono::Utc::now().timestamp();
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    loop {
        // Control is independent of effect slots. Reserve storage slots so a
        // full command channel can never prevent the pre-quit draft flush.
        if let Some(index) = pending.iter().position(|e| matches!(e, Effect::Shutdown)) {
            pending.remove(index);
            closing = true;
            let _ = cancel_media.send(true);
            screen.stop_media();
            if let Some(control) = control.take() {
                shutdown = Some(Box::pin(control.shutdown()));
            }
        }
        while jobs.len() < 16 {
            let Some(index) = pending
                .iter()
                .position(|e| command_jobs < 12 || !uses_commands(e))
            else {
                break;
            };
            let effect = pending.remove(index).expect("queued effect");
            let command = uses_commands(&effect);
            command_jobs += usize::from(command);
            let store = store.clone();
            let commands = commands.clone();
            let media = media.clone();
            let cancelled = media_cancelled.clone();
            jobs.spawn(async move {
                (
                    command,
                    execute_with_media(effect, store, commands, media.as_ref(), cancelled).await,
                )
            });
        }
        if dirty {
            screen.draw(&app.view(), &app.config)?;
            if let Some(map) = screen.interaction_map() {
                pending.extend(app.update(Input::Rendered(map), Instant::now()));
            }
            if let Some(viewport) = screen.timeline_viewport() {
                pending.extend(app.update(Input::TimelineViewport(viewport), Instant::now()));
            }
            dirty = false;
        }
        if closing && shutdown_done && events_closed && jobs.is_empty() && pending.is_empty() {
            if let Some(account) = app.view().account {
                store.recover_sends(account).await?;
            }
            store.flush().await?;
            screen.finish()?;
            return shutdown_error.map_or(Ok(()), |e| Err(AppError::Backend(e)));
        }
        let next = tokio::select! {
            event=input.next(),if !closing&&!input_closed=>match event{Some(Ok(event))=>Some(Input::Terminal(event)),Some(Err(e))=>return Err(e.into()),None=>{input_closed=true;pending.extend(app.request_shutdown());None}},
            event=events.recv(),if !events_closed&&(closing||pending.len()<48)=>match event{Some(event)=>Some(Input::Backend(event)),None=>{events_closed=true;Some(Input::Backend(BackendEvent::Stopped))}},
            completion=jobs.join_next(),if !jobs.is_empty()=>match completion{Some(Ok((command,event)))=>{command_jobs-=usize::from(command);event},Some(Err(_))=>return Err(AppError::Backend(BackendError::Stopped)),None=>None},
            result=async{shutdown.as_mut().expect("shutdown future exists").await},if closing&&!shutdown_done=>{if let Err(e)=result{shutdown_error=Some(e);}shutdown_done=true;None},
            _=tick.tick(),if !closing=>{dirty |= screen.poll();let epoch=chrono::Utc::now().timestamp_millis();if epoch/1000!=last_second{dirty=true;last_second=epoch/1000;}Some(Input::Tick(epoch))},
            _=tokio::signal::ctrl_c(),if !closing=>{pending.extend(app.request_shutdown());None},
            _=terminate.recv(),if !closing=>{pending.extend(app.request_shutdown());None},
        };
        if let Some(event) = next {
            if matches!(
                &event,
                Input::Terminal(crossterm::event::Event::Mouse(
                    crossterm::event::MouseEvent {
                        kind: crossterm::event::MouseEventKind::Moved
                            | crossterm::event::MouseEventKind::Up(_)
                            | crossterm::event::MouseEventKind::Drag(_),
                        ..
                    }
                ))
            ) {
                continue;
            }
            if !matches!(event, Input::Tick(_)) {
                dirty = true;
            }
            let effects = match event {
                Input::Terminal(event) if pending.len() >= 48 => {
                    app.input_under_pressure(event, Instant::now())
                }
                event => app.update(event, Instant::now()),
            };
            if !effects.is_empty() {
                dirty = true;
            }
            pending.extend(effects);
        }
        if input_closed && !app.quitting {
            return Err(AppError::Arguments(
                "Terminal input closed before drafts could be saved",
            ));
        }
    }
}
fn uses_commands(effect: &Effect) -> bool {
    matches!(
        effect,
        Effect::Prepare { .. }
            | Effect::Mutate { .. }
            | Effect::Transmit(_)
            | Effect::MarkRead { .. }
    )
}

async fn clipboard_paste(
    root: PathBuf,
    sticker: bool,
    mut cancel: tokio::sync::watch::Receiver<bool>,
) -> Result<crate::desktop::clipboard::Paste, String> {
    use crate::desktop::clipboard::{self, Content, Paste};
    let content = clipboard::read(cancel.clone()).await?;
    if let Content::Text(text) = content {
        return Ok(Paste::Text(text));
    }
    if *cancel.borrow() {
        return Err("Clipboard read cancelled".into());
    }
    let permit = tokio::select! {
        permit = crate::media::preview::DECODERS.acquire() => permit.map_err(|_| "Image preparation unavailable")?,
        _ = cancel.changed() => return Err("Clipboard read cancelled".into()),
    };
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let image = match content {
            Content::Image { bytes, filename } => {
                if sticker {
                    crate::media::outgoing::import_sticker(&bytes, &root, false)?
                } else {
                    crate::media::outgoing::import_bytes(&bytes, filename, &root)?
                }
            }
            Content::File(path) => {
                if sticker {
                    crate::media::outgoing::import_sticker(
                        &crate::media::preview::read_bounded(&path)?,
                        &root,
                        false,
                    )?
                } else {
                    crate::media::outgoing::import(&path, &root)?
                }
            }
            Content::Text(_) => unreachable!(),
        };
        Ok(Paste::Image(Box::new(image)))
    })
    .await
    .unwrap_or_else(|_| Err("Image preparation failed".into()))
}
