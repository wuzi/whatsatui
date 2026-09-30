use super::*;
use crate::ui::interaction::Target;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};

impl App {
    pub(super) fn mouse(&mut self, event: MouseEvent, effects: &mut Vec<Effect>) {
        if !self.config.ui.mouse {
            return;
        }
        let Some(map) = self.rendered.take().filter(|m| m.matches(&self.view)) else {
            return;
        };
        let Some(target) = map.hit(event.column, event.row).cloned() else {
            return;
        };
        if matches!(
            event.kind,
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown
        ) {
            let delta = if event.kind == MouseEventKind::ScrollUp {
                -3
            } else {
                3
            };
            self.last_click = None;
            if self.view.overlay.is_some() {
                if !matches!(self.view.overlay, Some(Overlay::Resend { .. })) {
                    self.move_selection(delta, effects);
                }
            } else {
                match target {
                    Target::Message(_)
                    | Target::Reactions(_)
                    | Target::Quote(_)
                    | Target::Pane(Focus::Messages) => self.scroll_timeline(delta, effects),
                    Target::Chat(_) | Target::Pane(Focus::Chats) => {
                        self.focus(Focus::Chats, effects);
                        self.move_selection(delta, effects);
                    }
                    _ => {}
                }
            }
            return;
        }
        let MouseEventKind::Down(button) = event.kind else {
            return;
        };
        if button != MouseButton::Left && button != MouseButton::Right {
            return;
        }
        let double = button == MouseButton::Left
            && self.last_click.as_ref().is_some_and(|(old, context, at)| {
                old == &target
                    && *context == map.context
                    && self.view.now.saturating_duration_since(*at)
                        <= std::time::Duration::from_millis(400)
            });
        self.last_click = if double || button != MouseButton::Left {
            None
        } else {
            Some((target.clone(), map.context, self.view.now))
        };
        match target {
            Target::Action(action) if button == MouseButton::Left => self.action(action, effects),
            Target::Chat(chat) if self.view.chats.iter().any(|c| c.chat == chat) => {
                self.select_chat(chat, effects);
                self.focus(
                    if double {
                        Focus::Composer
                    } else if map.area.width < 80 {
                        Focus::Messages
                    } else {
                        Focus::Chats
                    },
                    effects,
                );
            }
            Target::Message(key) if self.view.messages.iter().any(|m| m.key == key) => {
                self.focus(Focus::Messages, effects);
                if self.view.timeline_anchor.is_none() {
                    self.view.timeline_anchor = self.view.messages.last().map(|m| m.key.clone());
                }
                self.view.selected_message = Some(key);
                self.reading_position();
                if button == MouseButton::Right || double {
                    self.action(ActionId::MessageActions, effects);
                }
            }
            Target::Quote(key)
                if button == MouseButton::Left
                    && self.view.messages.iter().any(|m| m.key == key) =>
            {
                self.focus(Focus::Messages, effects);
                self.view.selected_message = Some(key);
                self.jump_to_quote(effects);
            }
            Target::Reactions(key) if self.view.messages.iter().any(|m| m.key == key) => {
                self.focus(Focus::Messages, effects);
                self.view.selected_message = Some(key);
                self.open_reactions();
            }
            Target::Composer(byte) if button == MouseButton::Left => {
                self.focus(Focus::Composer, effects);
                if let Some(editing) = &mut self.view.editing {
                    editing.editor.set_cursor(byte);
                } else {
                    self.editor.set_cursor(byte);
                }
            }
            Target::Query(byte) if button == MouseButton::Left => match &mut self.view.overlay {
                Some(
                    Overlay::Search { editor, .. }
                    | Overlay::Emoji { editor, .. }
                    | Overlay::Attachment { editor, .. },
                ) => editor.set_cursor(byte),
                Some(Overlay::MessageSearch(search)) => search.editor.set_cursor(byte),
                _ => {}
            },
            Target::Menu(index) if button == MouseButton::Left => {
                let current = match &self.view.overlay {
                    Some(
                        Overlay::Search { selected, .. }
                        | Overlay::Emoji { selected, .. }
                        | Overlay::Attachment { selected, .. },
                    ) => *selected,
                    Some(Overlay::MessageSearch(search)) => search.selected,
                    Some(Overlay::MessageActions(menu)) => menu.selected,
                    Some(Overlay::MessageLinks(links)) => links.selected,
                    Some(Overlay::Stickers(picker)) => picker.selected,
                    _ => return,
                };
                self.move_selection(index as isize - current as isize, effects);
                if double {
                    self.action(ActionId::Open, effects);
                }
            }
            Target::Pane(focus) if self.view.overlay.is_none() => self.focus(focus, effects),
            _ => {}
        }
    }
}
