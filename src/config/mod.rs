pub mod bindings;
pub mod theme;
pub use bindings::Bindings;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
pub use theme::Theme;
#[derive(Debug, thiserror::Error)]
#[error("Invalid configuration: {0}")]
pub struct ConfigError(pub String);
#[derive(Clone, Debug, Default)]
pub struct Config {
    pub ui: UiConfig,
    pub theme: Theme,
    pub bindings: Bindings,
    pub media: MediaConfig,
    pub audio: AudioConfig,
    pub notifications: NotificationConfig,
    /// Raw binding overrides, kept so saving settings preserves keybindings.
    pub(crate) bindings_source: bindings::Overrides,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct NotificationConfig {
    pub enabled: bool,
    pub previews: bool,
}
impl Default for NotificationConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            previews: true,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct AudioConfig {
    pub player: PathBuf,
}
impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            player: "mpv".into(),
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    pub mouse: bool,
}
impl Default for UiConfig {
    fn default() -> Self {
        Self { mouse: true }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageProtocol {
    #[default]
    Auto,
    Kitty,
    Halfblocks,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct MediaConfig {
    pub inline: bool,
    pub avatars: bool,
    pub protocol: ImageProtocol,
    /// Image/sticker/gif previews fetch on their own when visible.
    pub auto_photos: bool,
    /// Videos download in the background once visible.
    pub auto_videos: bool,
    /// Voice notes and audio download in the background once visible.
    pub auto_audio: bool,
    /// Documents download in the background once visible.
    pub auto_documents: bool,
}
impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            inline: true,
            avatars: true,
            protocol: ImageProtocol::Auto,
            auto_photos: true,
            auto_videos: false,
            auto_audio: false,
            auto_documents: false,
        }
    }
}
impl MediaConfig {
    /// Whether this attachment kind may download without an explicit action.
    pub fn auto_download(&self, kind: crate::media::AttachmentKind) -> bool {
        match kind {
            crate::media::AttachmentKind::Image
            | crate::media::AttachmentKind::Sticker
            | crate::media::AttachmentKind::Gif => self.auto_photos,
            crate::media::AttachmentKind::Video => self.auto_videos,
            crate::media::AttachmentKind::Audio => self.auto_audio,
            crate::media::AttachmentKind::Document => self.auto_documents,
        }
    }
}
#[derive(Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
struct FileConfig {
    ui: UiConfig,
    theme: Theme,
    bindings: bindings::Overrides,
    media: MediaConfig,
    audio: AudioConfig,
    notifications: NotificationConfig,
}
impl Config {
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let file: FileConfig = toml::from_str(text).map_err(|e| ConfigError(e.to_string()))?;
        file.theme.validate()?;
        Ok(Self {
            ui: file.ui,
            theme: file.theme,
            bindings: Bindings::configured(file.bindings.clone())?,
            media: file.media,
            audio: file.audio,
            notifications: file.notifications,
            bindings_source: file.bindings,
        })
    }
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(s) => Self::parse(&s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(_) => Err(ConfigError(format!("cannot read {}", path.display()))),
        }
    }
    /// Persist current settings, preserving the loaded keybindings.
    /// The file is rewritten, so hand-written comments are not kept.
    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|_| ConfigError(format!("cannot write {}", path.display())))?;
        }
        let file = FileConfig {
            ui: self.ui.clone(),
            theme: self.theme.clone(),
            bindings: self.bindings_source.clone(),
            media: self.media.clone(),
            audio: self.audio.clone(),
            notifications: self.notifications.clone(),
        };
        let text = toml::to_string_pretty(&file).map_err(|e| ConfigError(e.to_string()))?;
        std::fs::write(path, text)
            .map_err(|_| ConfigError(format!("cannot write {}", path.display())))?;
        Ok(())
    }
}
#[derive(Default)]
pub struct XdgDirs {
    pub config: Option<PathBuf>,
    pub data: Option<PathBuf>,
    pub state: Option<PathBuf>,
}
pub struct Paths {
    pub config: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
}
impl Paths {
    pub fn resolve(home: &Path, xdg: &XdgDirs) -> Self {
        let base = |value: &Option<PathBuf>, fallback: &str| {
            value
                .as_ref()
                .filter(|p| p.is_absolute())
                .cloned()
                .unwrap_or_else(|| home.join(fallback))
                .join("whatsapp-tui")
        };
        Self {
            config: base(&xdg.config, ".config").join("config.toml"),
            data: base(&xdg.data, ".local/share"),
            state: base(&xdg.state, ".local/state"),
        }
    }
}
