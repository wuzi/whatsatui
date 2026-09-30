use super::ConfigError;
use ratatui::style::Color;
use serde::Deserialize;
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    pub background: String,
    pub text: String,
    pub focus: String,
    pub accent: String,
    pub own: String,
    pub inactive: String,
    pub hints: String,
    pub error: String,
    pub error_border: String,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            background: "default".into(),
            text: "default".into(),
            focus: "#00b4b4".into(),
            accent: "#00c8c8".into(),
            own: "#6fdca3".into(),
            inactive: "#808080".into(),
            hints: "#c8c800".into(),
            error: "#ff6464".into(),
            error_border: "#c80000".into(),
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum ThemeRole {
    Background,
    Text,
    Focus,
    Accent,
    Own,
    Inactive,
    Hints,
    Error,
    ErrorBorder,
}
impl Theme {
    pub fn color(&self, role: ThemeRole, truecolor: bool) -> Color {
        let (value, ansi) = match role {
            ThemeRole::Background => (&self.background, Color::Reset),
            ThemeRole::Text => (&self.text, Color::Reset),
            ThemeRole::Focus => (&self.focus, Color::Cyan),
            ThemeRole::Accent => (&self.accent, Color::Cyan),
            ThemeRole::Own => (&self.own, Color::Green),
            ThemeRole::Inactive => (&self.inactive, Color::Gray),
            ThemeRole::Hints => (&self.hints, Color::Yellow),
            ThemeRole::Error => (&self.error, Color::Red),
            ThemeRole::ErrorBorder => (&self.error_border, Color::Red),
        };
        if value == "default" {
            Color::Reset
        } else if !truecolor {
            ansi
        } else {
            parse_color(value).unwrap_or(ansi)
        }
    }
    pub(super) fn validate(&self) -> Result<(), ConfigError> {
        for (key, value) in [
            ("background", &self.background),
            ("text", &self.text),
            ("focus", &self.focus),
            ("accent", &self.accent),
            ("own", &self.own),
            ("inactive", &self.inactive),
            ("hints", &self.hints),
            ("error", &self.error),
            ("error_border", &self.error_border),
        ] {
            parse_color(value)
                .map_err(|_| ConfigError(format!("theme.{key} must be 'default' or #RRGGBB")))?;
        }
        Ok(())
    }
}
fn parse_color(s: &str) -> Result<Color, ()> {
    if s == "default" {
        return Ok(Color::Reset);
    }
    if s.len() != 7 || !s.starts_with('#') || !s.is_ascii() {
        return Err(());
    }
    let n = u32::from_str_radix(&s[1..], 16).map_err(|_| ())?;
    Ok(Color::Rgb((n >> 16) as u8, (n >> 8) as u8, n as u8))
}
