use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

pub mod matrix;
use crate::app::{App, AppState};

pub fn draw(f: &mut Frame, app: &App) {
    match &app.state {
        AppState::StartupAnimation { .. } => draw_startup(f, app),

        AppState::PinPrompt { input, error } => draw_pin_prompt(f, app, input, error.as_deref()),
        AppState::MainMenu => draw_main_menu(f, app),
        AppState::Connecting => draw_connecting(f),
        _ => {}
    }
}

fn draw_startup(f: &mut Frame, app: &App) {
    // Fill background with matrix
    let matrix = app.matrix.layer(true);
    f.render_widget(matrix, f.area());
}

fn draw_pin_prompt(f: &mut Frame, app: &App, input: &str, error: Option<&str>) {
    let matrix = app.matrix.layer(false);
    f.render_widget(matrix, f.area());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(40),
            Constraint::Length(3), // Input box
            Constraint::Length(1), // Error msg
            Constraint::Percentage(40),
        ])
        .split(f.area());

    let input_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
        ])
        .split(chunks[1]);

    let error_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Percentage(40),
            Constraint::Percentage(30),
        ])
        .split(chunks[2]);

    let masked_input: String = input.chars().map(|_| '*').collect();

    let input_widget = Paragraph::new(masked_input)
        .style(Style::default().fg(Color::Cyan))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Enter PIN (1234)"),
        );

    f.render_widget(input_widget, input_chunks[1]);

    if let Some(err) = error {
        let error_widget = Paragraph::new(err)
            .style(Style::default().fg(Color::Red))
            .alignment(Alignment::Center);
        f.render_widget(error_widget, error_chunks[1]);
    }
}

fn draw_main_menu(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(f.area());

    let title = Paragraph::new("Tandem TUI")
        .style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL));

    f.render_widget(title, chunks[0]);

    if app.sessions.is_empty() {
        let content =
            Paragraph::new("No sessions found. Press 'n' to create one.\n(Polling Engine...)")
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::NONE));
        f.render_widget(content, chunks[1]);
    } else {
        use ratatui::widgets::{List, ListItem};
        let items: Vec<ListItem> = app
            .sessions
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let content = format!("{} (ID: {})", s.name.as_deref().unwrap_or("Untitled"), s.id);
                let style = if i == app.selected_session_index {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };
                ListItem::new(content).style(style)
            })
            .collect();

        let list = List::new(items).block(Block::default().borders(Borders::ALL).title("Sessions"));

        f.render_widget(list, chunks[1]);
    }
}

fn draw_connecting(f: &mut Frame) {
    let block = Block::default().borders(Borders::ALL).title("Status");
    let paragraph = Paragraph::new("Connecting to Tandem Engine...")
        .style(Style::default().fg(Color::Yellow))
        .alignment(Alignment::Center)
        .block(block);

    // Center it
    let area = centered_rect(60, 20, f.area());
    f.render_widget(Clear, area); // Clear background
    f.render_widget(paragraph, area);
}

fn centered_rect(
    percent_x: u16,
    percent_y: u16,
    r: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
