use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{text::Text, Frame};

use crate::app::PagerOverlayState;

pub fn draw_overlay(
    f: &mut Frame,
    overlay: &PagerOverlayState,
    area: ratatui::layout::Rect,
    lines: Text<'static>,
) {
    let title = format!(" {} ", overlay.title);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        );
    let widget = Paragraph::new(lines)
        .block(block)
        .scroll((overlay.scroll as u16, 0))
        .wrap(Wrap { trim: false });
    f.render_widget(widget, area);
}
