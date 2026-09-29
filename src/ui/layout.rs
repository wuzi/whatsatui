use crate::app::Focus;
use ratatui::layout::Rect;
pub struct LayoutRegions {
    pub header: Rect,
    pub chats: Rect,
    pub messages: Rect,
    pub composer: Rect,
    pub footer: Rect,
    pub too_small: bool,
}
pub fn calculate(area: Rect, focus: Focus) -> LayoutRegions {
    let header = Rect::new(area.x, area.y, area.width, 1.min(area.height));
    let footer = Rect::new(
        area.x,
        area.bottom().saturating_sub(2),
        area.width,
        2.min(area.height),
    );
    let main = Rect::new(
        area.x,
        area.y + 1,
        area.width,
        area.height.saturating_sub(3),
    );
    let (chats, right) = if area.width >= 80 {
        let w = (area.width * 30 / 100).clamp(25, 40);
        (
            Rect::new(main.x, main.y, w, main.height),
            Rect::new(main.x + w, main.y, main.width - w, main.height),
        )
    } else if focus == Focus::Chats {
        (main, Rect::default())
    } else {
        (Rect::default(), main)
    };
    let composer_height = (right.height / 4).clamp(4, 8).min(right.height);
    LayoutRegions {
        header,
        chats,
        messages: Rect::new(
            right.x,
            right.y,
            right.width,
            right.height - composer_height,
        ),
        composer: Rect::new(
            right.x,
            right.bottom() - composer_height,
            right.width,
            composer_height,
        ),
        footer,
        too_small: area.width < 40 || area.height < 12,
    }
}
