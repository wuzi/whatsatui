use super::Effect;
use crate::{
    app::{App, view_model::Overlay},
    config::Config,
};

/// Toggleable settings shown in the settings overlay, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingRow {
    InlineImages,
    Avatars,
    Protocol,
    AutoPhotos,
    AutoVideos,
    AutoAudio,
    AutoDocuments,
    Notifications,
    Previews,
    Mouse,
}

impl SettingRow {
    pub(crate) const ALL: [SettingRow; 10] = [
        SettingRow::InlineImages,
        SettingRow::Avatars,
        SettingRow::Protocol,
        SettingRow::AutoPhotos,
        SettingRow::AutoVideos,
        SettingRow::AutoAudio,
        SettingRow::AutoDocuments,
        SettingRow::Notifications,
        SettingRow::Previews,
        SettingRow::Mouse,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            SettingRow::InlineImages => "Inline images",
            SettingRow::Avatars => "Profile photos",
            SettingRow::Protocol => "Image protocol",
            SettingRow::AutoPhotos => "Auto-download photos",
            SettingRow::AutoVideos => "Auto-download videos",
            SettingRow::AutoAudio => "Auto-download audio",
            SettingRow::AutoDocuments => "Auto-download documents",
            SettingRow::Notifications => "Notifications",
            SettingRow::Previews => "Notification previews",
            SettingRow::Mouse => "Mouse",
        }
    }

    pub(crate) fn value(self, config: &Config) -> String {
        let on = |enabled: bool| {
            if enabled { "on".into() } else { "off".into() }
        };
        match self {
            SettingRow::InlineImages => on(config.media.inline),
            SettingRow::Avatars => on(config.media.avatars),
            SettingRow::Protocol => match config.media.protocol {
                crate::config::ImageProtocol::Auto => "auto".into(),
                crate::config::ImageProtocol::Kitty => "kitty".into(),
                crate::config::ImageProtocol::Halfblocks => "halfblocks".into(),
            },
            SettingRow::AutoPhotos => on(config.media.auto_photos),
            SettingRow::AutoVideos => on(config.media.auto_videos),
            SettingRow::AutoAudio => on(config.media.auto_audio),
            SettingRow::AutoDocuments => on(config.media.auto_documents),
            SettingRow::Notifications => on(config.notifications.enabled),
            SettingRow::Previews => on(config.notifications.previews),
            SettingRow::Mouse => on(config.ui.mouse),
        }
    }

    /// Extra hint shown for rows that need more than a toggle.
    pub(crate) fn hint(self) -> Option<&'static str> {
        match self {
            SettingRow::Protocol => Some("needs restart"),
            _ => None,
        }
    }
}

/// Flip the selected row, persist, and report what changed.
pub(super) fn toggle(app: &mut App, index: usize, effects: &mut Vec<Effect>) {
    let Some(row) = SettingRow::ALL.get(index).copied() else {
        return;
    };
    let changed = match row {
        SettingRow::InlineImages => {
            app.config.media.inline = !app.config.media.inline;
            "Inline images"
        }
        SettingRow::Avatars => {
            app.config.media.avatars = !app.config.media.avatars;
            "Profile photos"
        }
        SettingRow::Protocol => {
            app.config.media.protocol = match app.config.media.protocol {
                crate::config::ImageProtocol::Auto => crate::config::ImageProtocol::Kitty,
                crate::config::ImageProtocol::Kitty => crate::config::ImageProtocol::Halfblocks,
                crate::config::ImageProtocol::Halfblocks => crate::config::ImageProtocol::Auto,
            };
            "Image protocol (restart to apply)"
        }
        SettingRow::Notifications => {
            app.config.notifications.enabled = !app.config.notifications.enabled;
            "Notifications"
        }
        SettingRow::AutoPhotos => {
            app.config.media.auto_photos = !app.config.media.auto_photos;
            "Auto-download photos"
        }
        SettingRow::AutoVideos => {
            app.config.media.auto_videos = !app.config.media.auto_videos;
            "Auto-download videos"
        }
        SettingRow::AutoAudio => {
            app.config.media.auto_audio = !app.config.media.auto_audio;
            "Auto-download audio"
        }
        SettingRow::AutoDocuments => {
            app.config.media.auto_documents = !app.config.media.auto_documents;
            "Auto-download documents"
        }
        SettingRow::Previews => {
            app.config.notifications.previews = !app.config.notifications.previews;
            "Notification previews"
        }
        SettingRow::Mouse => {
            app.config.ui.mouse = !app.config.ui.mouse;
            effects.push(Effect::SetMouse(app.config.ui.mouse));
            "Mouse"
        }
    };
    if let Some(Overlay::Settings { selected }) = &mut app.view.overlay {
        *selected = index.min(SettingRow::ALL.len().saturating_sub(1));
    }
    if app.config_path.is_some() {
        app.view.notice = Some(format!("{changed} saved"));
    } else {
        app.view.notice = Some(format!("{changed} (session only)"));
    }
    persist(app);
}

fn persist(app: &mut App) {
    let Some(path) = app.config_path.clone() else {
        return;
    };
    if let Err(error) = app.config.save(&path) {
        app.view.notice = Some(format!("{error}; kept for this session"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_cover_unique_labels_and_values() {
        let config = Config::default();
        let mut labels = std::collections::BTreeSet::new();
        for row in SettingRow::ALL {
            assert!(labels.insert(row.label()), "duplicate row");
            assert!(!row.value(&config).is_empty());
        }
        assert_eq!(SettingRow::ALL.len(), labels.len());
    }

    #[test]
    fn protocol_cycles_through_all_modes() {
        let mut config = Config::default();
        let mut seen = vec![];
        for _ in 0..3 {
            config.media.protocol = match config.media.protocol {
                crate::config::ImageProtocol::Auto => crate::config::ImageProtocol::Kitty,
                crate::config::ImageProtocol::Kitty => crate::config::ImageProtocol::Halfblocks,
                crate::config::ImageProtocol::Halfblocks => crate::config::ImageProtocol::Auto,
            };
            seen.push(SettingRow::Protocol.value(&config));
        }
        seen.sort();
        assert_eq!(seen, ["auto", "halfblocks", "kitty"]);
    }
}
