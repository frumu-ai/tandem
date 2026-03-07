use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub fn style_diff_line(line: &str) -> Line<'static> {
    let style = if line.starts_with("diff --git") {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else if line.starts_with("@@") {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else if line.starts_with('+') && !line.starts_with("+++") {
        Style::default().fg(Color::Green)
    } else if line.starts_with('-') && !line.starts_with("---") {
        Style::default().fg(Color::Red)
    } else if line.starts_with("index ") || line.starts_with("--- ") || line.starts_with("+++ ") {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::Gray)
    };
    Line::from(vec![Span::styled(line.to_string(), style)])
}

#[cfg(test)]
mod tests {
    use super::style_diff_line;
    use ratatui::style::Color;

    #[test]
    fn styles_added_and_removed_lines() {
        let add = style_diff_line("+hello");
        let del = style_diff_line("-hello");
        assert_eq!(add.spans[0].style.fg, Some(Color::Green));
        assert_eq!(del.spans[0].style.fg, Some(Color::Red));
    }
}
