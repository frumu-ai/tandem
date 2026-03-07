use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

pub fn tool_call_lines(tool_name: &str, args_preview: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::from(vec![
        Span::styled(
            " TOOL ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            tool_name.to_string(),
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    for line in args_preview.lines().take(8) {
        lines.push(Line::from(vec![
            Span::styled("   ", Style::default().fg(Color::DarkGray)),
            Span::styled(line.to_string(), Style::default().fg(Color::Gray)),
        ]));
    }
    if args_preview.lines().count() > 8 {
        lines.push(Line::from("   ..."));
    }
    lines
}

pub fn tool_result_lines(output: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    lines.push(Line::from(vec![Span::styled(
        " RESULT ",
        Style::default()
            .fg(Color::Black)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )]));
    for line in output.lines().take(10) {
        lines.push(Line::from(vec![
            Span::styled("   ", Style::default().fg(Color::DarkGray)),
            Span::styled(line.to_string(), Style::default().fg(Color::Gray)),
        ]));
    }
    if output.lines().count() > 10 {
        lines.push(Line::from("   ..."));
    }
    lines
}
