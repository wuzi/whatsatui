use super::ConfigError;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Deserialize;
use std::collections::BTreeMap;
#[derive(Clone, Debug)]
pub struct Bindings {
    entries: Vec<Binding>,
}
#[derive(Clone, Debug)]
struct Binding {
    context: Context,
    action: ActionId,
    key: KeyEvent,
    label: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Context {
    Chats,
    Messages,
    Composer,
    Search,
    MessageSearch,
    Help,
    Resend,
    Global,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionId {
    FocusNext,
    FocusPrevious,
    Search,
    MessageSearch,
    Unread,
    ToggleUnread,
    Help,
    Quit,
    Next,
    Previous,
    Open,
    Send,
    Newline,
    RemoveReply,
    Back,
    Reply,
    Resend,
    PageUp,
    PageDown,
    Bottom,
    Confirm,
}
pub(super) type Overrides = BTreeMap<Context, BTreeMap<ActionId, Vec<String>>>;
impl ActionId {
    pub fn label(self) -> &'static str {
        match self {
            Self::FocusNext => "next pane",
            Self::FocusPrevious => "previous pane",
            Self::Search => "chats",
            Self::MessageSearch => "find messages",
            Self::Unread => "unread chats",
            Self::ToggleUnread => "all/unread",
            Self::Help => "help",
            Self::Quit => "quit",
            Self::Next => "down",
            Self::Previous => "up",
            Self::Open => "open",
            Self::Send => "send",
            Self::Newline => "newline",
            Self::RemoveReply => "remove reply",
            Self::Back => "back",
            Self::Reply => "reply",
            Self::Resend => "resend",
            Self::PageUp => "older",
            Self::PageDown => "newer",
            Self::Bottom => "latest",
            Self::Confirm => "confirm",
        }
    }
}
impl Default for Bindings {
    fn default() -> Self {
        use {ActionId as A, Context as C};
        let mut b = Self { entries: vec![] };
        b.add(C::Global, A::Quit, "ctrl-q");
        for c in [C::Chats, C::Messages, C::Composer] {
            for (a, k) in [
                (A::FocusNext, "tab"),
                (A::FocusPrevious, "shift-tab"),
                (A::Search, "ctrl-p"),
                (A::MessageSearch, "ctrl-f"),
                (A::Help, "f1"),
            ] {
                b.add(c, a, k);
            }
        }
        for c in [C::Chats, C::Messages] {
            for (a, k) in [
                (A::Next, "j"),
                (A::Next, "down"),
                (A::Previous, "k"),
                (A::Previous, "up"),
                (A::Search, "/"),
                (A::Help, "?"),
            ] {
                b.add(c, a, k);
            }
        }
        b.add(C::Chats, A::Open, "enter");
        b.add(C::Chats, A::Unread, "u");
        for (a, k) in [
            (A::Back, "esc"),
            (A::Reply, "r"),
            (A::Resend, "R"),
            (A::PageUp, "pageup"),
            (A::PageDown, "pagedown"),
            (A::Bottom, "end"),
        ] {
            b.add(C::Messages, a, k);
        }
        for (a, k) in [
            (A::Back, "esc"),
            (A::Send, "enter"),
            (A::Newline, "alt-enter"),
            (A::RemoveReply, "alt-r"),
        ] {
            b.add(C::Composer, a, k);
        }
        for (a, k) in [
            (A::Back, "esc"),
            (A::Open, "enter"),
            (A::Next, "down"),
            (A::Previous, "up"),
        ] {
            b.add(C::Search, a, k);
            b.add(C::MessageSearch, a, k);
        }
        b.add(C::Help, A::Back, "esc");
        b.add(C::Search, A::ToggleUnread, "ctrl-u");
        b.add(C::Resend, A::Back, "esc");
        b.add(C::Resend, A::Confirm, "enter");
        b
    }
}
impl Bindings {
    fn add(&mut self, context: Context, action: ActionId, label: &str) {
        self.entries.push(Binding {
            context,
            action,
            key: parse_key(label).expect("valid default key"),
            label: label.into(),
        });
    }
    pub(super) fn configured(overrides: Overrides) -> Result<Self, ConfigError> {
        let mut result = Self::default();
        for (context, actions) in overrides {
            for (action, keys) in actions {
                if !allowed(context, action) {
                    return Err(ConfigError(format!(
                        "bindings.{context:?}.{action:?} is not an action for this context"
                    )));
                }
                result
                    .entries
                    .retain(|b| b.context != context || b.action != action);
                for label in keys {
                    let key = parse_key(&label)?;
                    if matches!(
                        context,
                        Context::Composer | Context::Search | Context::MessageSearch
                    ) && matches!(key.code, KeyCode::Char(_))
                        && !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    {
                        return Err(ConfigError(format!(
                            "bindings.{context:?}: '{label}' must remain text"
                        )));
                    }
                    result.entries.push(Binding {
                        context,
                        action,
                        key,
                        label,
                    });
                }
            }
        }
        for context in [
            Context::Chats,
            Context::Messages,
            Context::Composer,
            Context::Search,
            Context::MessageSearch,
            Context::Help,
            Context::Resend,
            Context::Global,
        ] {
            let entries: Vec<_> = result
                .entries
                .iter()
                .filter(|b| b.context == context || b.context == Context::Global)
                .collect();
            for (i, a) in entries.iter().enumerate() {
                for b in &entries[i + 1..] {
                    if normalized(a.key) == normalized(b.key) {
                        return Err(ConfigError(format!(
                            "bindings.{context:?}: duplicate key '{}'",
                            a.label
                        )));
                    }
                }
            }
            let mut required = vec![ActionId::Quit];
            if matches!(
                context,
                Context::Chats | Context::Messages | Context::Composer
            ) {
                required.extend([ActionId::FocusNext, ActionId::FocusPrevious, ActionId::Help]);
            }
            for action in required {
                if !entries.iter().any(|b| b.action == action) {
                    return Err(ConfigError(format!(
                        "bindings.{context:?}: required action {action:?} has no key"
                    )));
                }
            }
        }
        Ok(result)
    }
    pub fn lookup(&self, context: Context, key: KeyEvent) -> Option<ActionId> {
        self.entries
            .iter()
            .find(|b| {
                (b.context == Context::Global || b.context == context)
                    && normalized(b.key) == normalized(key)
            })
            .map(|b| b.action)
    }
    pub fn help(&self, context: Context) -> Vec<(String, ActionId)> {
        self.entries
            .iter()
            .filter(|b| b.context == context || b.context == Context::Global)
            .map(|b| (b.label.clone(), b.action))
            .collect()
    }
}
fn allowed(c: Context, a: ActionId) -> bool {
    use {ActionId as A, Context as C};
    match c {
        C::Global => a == A::Quit,
        C::Chats => matches!(
            a,
            A::FocusNext
                | A::FocusPrevious
                | A::Search
                | A::MessageSearch
                | A::Unread
                | A::Help
                | A::Next
                | A::Previous
                | A::Open
        ),
        C::Messages => !matches!(
            a,
            A::Quit
                | A::Open
                | A::Send
                | A::Newline
                | A::RemoveReply
                | A::Confirm
                | A::Unread
                | A::ToggleUnread
        ),
        C::Composer => matches!(
            a,
            A::FocusNext
                | A::FocusPrevious
                | A::Search
                | A::MessageSearch
                | A::Help
                | A::Send
                | A::Newline
                | A::RemoveReply
                | A::Back
        ),
        C::Search => matches!(
            a,
            A::Back | A::Open | A::Next | A::Previous | A::ToggleUnread
        ),
        C::MessageSearch => matches!(a, A::Back | A::Open | A::Next | A::Previous),
        C::Help => a == A::Back,
        C::Resend => matches!(a, A::Back | A::Confirm),
    }
}
fn normalized(key: KeyEvent) -> (KeyCode, KeyModifiers) {
    let mut modifiers = key.modifiers;
    if matches!(key.code, KeyCode::Char(_)) {
        modifiers.remove(KeyModifiers::SHIFT);
    }
    (key.code, modifiers)
}
pub fn parse_key(label: &str) -> Result<KeyEvent, ConfigError> {
    let mut key = label;
    let mut modifiers = KeyModifiers::NONE;
    loop {
        if let Some(rest) = key.strip_prefix("ctrl-") {
            modifiers |= KeyModifiers::CONTROL;
            key = rest;
        } else if let Some(rest) = key.strip_prefix("alt-") {
            modifiers |= KeyModifiers::ALT;
            key = rest;
        } else if let Some(rest) = key.strip_prefix("shift-") {
            modifiers |= KeyModifiers::SHIFT;
            key = rest;
        } else {
            break;
        }
    }
    let code = match key {
        "tab" => {
            if modifiers.contains(KeyModifiers::SHIFT) {
                KeyCode::BackTab
            } else {
                KeyCode::Tab
            }
        }
        "enter" => KeyCode::Enter,
        "esc" => KeyCode::Esc,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "backspace" => KeyCode::Backspace,
        "delete" => KeyCode::Delete,
        "space" => KeyCode::Char(' '),
        s if s.starts_with('f') && s.len() > 1 => KeyCode::F(
            s[1..]
                .parse::<u8>()
                .ok()
                .filter(|n| (1..=24).contains(n))
                .ok_or_else(|| ConfigError(format!("invalid key '{label}'")))?,
        ),
        s if s.chars().count() == 1 => KeyCode::Char(s.chars().next().unwrap()),
        _ => return Err(ConfigError(format!("invalid key '{label}'"))),
    };
    Ok(KeyEvent::new(code, modifiers))
}
