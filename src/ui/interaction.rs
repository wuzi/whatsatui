use super::*;
use crate::{app::model::*, config::bindings::ActionId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Pane(Focus),
    Chat(ChatId),
    Message(MessageKey),
    Composer(usize),
    Query(usize),
    Menu(usize),
    Action(ActionId),
    Popup,
}
#[derive(Clone, Debug)]
pub struct InteractionMap {
    pub area: Rect,
    pub account: Option<AccountId>,
    pub chat: Option<ChatId>,
    pub context: Context,
    pub help_max_scroll: usize,
    pub list_offsets: std::collections::BTreeMap<Context, usize>,
    regions: Vec<(Rect, Target)>,
}
pub fn context(view: &ViewModel) -> Context {
    match &view.overlay {
        Some(Overlay::Stickers(_)) => Context::Stickers,
        Some(Overlay::Emoji { .. }) => Context::Emoji,
        Some(Overlay::Attachment { .. }) => Context::Attachment,
        Some(Overlay::MessageActions(_)) => Context::MessageActions,
        Some(Overlay::MessageLinks(_)) => Context::MessageLinks,
        Some(Overlay::MessageSearch(_)) => Context::MessageSearch,
        Some(Overlay::Search { .. }) => Context::Search,
        Some(Overlay::Help) => Context::Help,
        Some(Overlay::Resend { .. }) => Context::Resend,
        None => match view.focus {
            Focus::Chats => Context::Chats,
            Focus::Messages => Context::Messages,
            Focus::Composer => Context::Composer,
        },
    }
}
impl InteractionMap {
    pub fn new(area: Rect, view: &ViewModel) -> Self {
        Self {
            area,
            account: view.account.clone(),
            chat: view.chat.clone(),
            context: context(view),
            help_max_scroll: 0,
            list_offsets: Default::default(),
            regions: vec![],
        }
    }
    pub fn matches(&self, view: &ViewModel) -> bool {
        self.account == view.account && self.chat == view.chat && self.context == context(view)
    }
    pub fn hit(&self, x: u16, y: u16) -> Option<&Target> {
        self.regions
            .iter()
            .rev()
            .find(|(rect, _)| rect.contains((x, y).into()))
            .map(|(_, target)| target)
    }
    pub(super) fn push(&mut self, rect: Rect, target: Target) {
        let rect = rect.intersection(self.area);
        if !rect.is_empty() {
            self.regions.push((rect, target));
        }
    }
    pub(super) fn clear(&mut self) {
        self.regions.clear();
    }
    pub(super) fn list(
        &mut self,
        context: Context,
        rect: Rect,
        offset: usize,
        heights: impl IntoIterator<Item = usize>,
        mut target: impl FnMut(usize) -> Target,
    ) {
        self.list_offsets.insert(context, offset);
        let mut y = rect.y;
        for (index, height) in heights.into_iter().enumerate().skip(offset) {
            let height = height.min(rect.bottom().saturating_sub(y) as usize) as u16;
            if height == 0 {
                break;
            }
            self.push(Rect::new(rect.x, y, rect.width, height), target(index));
            y += height;
        }
    }
    pub(super) fn popup(
        &mut self,
        frame: &mut Frame,
        rect: Rect,
        view: &ViewModel,
        config: &Config,
    ) {
        self.push(rect, Target::Popup);
        let close = Rect::new(rect.right().saturating_sub(4), rect.y, 3.min(rect.width), 1);
        frame.render_widget(
            Paragraph::new("[×]").style(style(config, view, ThemeRole::Accent)),
            close,
        );
        self.push(close, Target::Action(ActionId::Back));
    }
}
