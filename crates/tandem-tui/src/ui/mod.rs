use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

pub mod matrix;
use crate::app::{App, AppState, PinPromptMode, SetupStep};

pub fn draw(f: &mut Frame, app: &App) {
    match &app.state {
        AppState::StartupAnimation { .. } => draw_startup(f, app),

        AppState::PinPrompt { input, error, mode } => {
            draw_pin_prompt(f, app, input, error.as_deref(), mode)
        }
        AppState::MainMenu => draw_main_menu(f, app),
        AppState::Chat { .. } => draw_chat(f, app),
        AppState::Connecting => draw_connecting(f, app),
        AppState::SetupWizard { .. } => draw_setup_wizard(f, app),
    }
}

fn draw_startup(f: &mut Frame, app: &App) {
    // Fill background with matrix
    let matrix = app.matrix.layer(true);
    f.render_widget(matrix, f.area());
}

fn draw_pin_prompt(
    f: &mut Frame,
    app: &App,
    input: &str,
    error: Option<&str>,
    mode: &PinPromptMode,
) {
    let matrix = app.matrix.layer(false);
    f.render_widget(matrix, f.area());

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(42),
            Constraint::Length(3), // Input box
            Constraint::Length(1), // Error msg
            Constraint::Length(2), // Hint
            Constraint::Percentage(38),
        ])
        .split(f.area());

    let input_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(37),
            Constraint::Length(26),
            Constraint::Percentage(37),
        ])
        .split(chunks[1]);

    let error_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(30),
            Constraint::Length(40),
            Constraint::Percentage(30),
        ])
        .split(chunks[2]);

    let masked_input = if input.is_empty() {
        " ".to_string()
    } else {
        input.chars().map(|_| '•').collect::<String>()
    };
    let title = match mode {
        PinPromptMode::UnlockExisting => "Unlock PIN",
        PinPromptMode::CreateNew => "Create PIN",
        PinPromptMode::ConfirmNew { .. } => "Confirm PIN",
    };
    let hint = match mode {
        PinPromptMode::UnlockExisting => "Enter your existing 4-8 digit PIN",
        PinPromptMode::CreateNew => "Create a new 4-8 digit PIN",
        PinPromptMode::ConfirmNew { .. } => "Re-enter the same PIN",
    };

    let input_widget = Paragraph::new(masked_input)
        .style(Style::default().fg(Color::Yellow))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).title(title));

    f.render_widget(input_widget, input_chunks[1]);

    if let Some(err) = error {
        let error_widget = Paragraph::new(err)
            .style(Style::default().fg(Color::Red))
            .alignment(Alignment::Center);
        f.render_widget(error_widget, error_chunks[1]);
    }

    let hint_widget = Paragraph::new(hint)
        .style(Style::default().fg(Color::Gray))
        .alignment(Alignment::Center);
    f.render_widget(hint_widget, chunks[3]);
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
                let content = format!("{} (ID: {})", s.title, &s.id[..8.min(s.id.len())]);
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

    draw_status_bar(f, app);
}

fn draw_chat(f: &mut Frame, app: &App) {
    if let AppState::Chat {
        session_id,
        command_input,
        messages,
        scroll_from_bottom,
    } = &app.state
    {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(0),
                Constraint::Length(3),
                Constraint::Length(1),
            ])
            .split(f.area());

        let messages_chunk = chunks[0];
        let input_chunk = chunks[1];
        let status_chunk = chunks[2];

        // Find session title
        let session_title = app
            .sessions
            .iter()
            .find(|s| s.id == *session_id)
            .map(|s| s.title.as_str())
            .unwrap_or("New session");
        let chat_title = format!(" {} ", session_title);

        let message_widget = if messages.is_empty() {
            Paragraph::new("No messages yet. Type a prompt or /help for commands.\n\nPress Tab for command autocomplete.")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center)
                .block(Block::default().borders(Borders::ALL).title(chat_title.as_str()).border_style(Style::default().fg(Color::DarkGray)))
        } else {
            let lines: Vec<Line> = messages
                .iter()
                .flat_map(|msg| {
                    let (color, prefix) = match msg.role {
                        crate::app::MessageRole::User => (Color::Cyan, "you: "),
                        crate::app::MessageRole::Assistant => (Color::Green, "ai:  "),
                        crate::app::MessageRole::System => (Color::Yellow, "sys: "),
                    };
                    let mut message_lines = msg.content.lines();
                    let first = message_lines.next().unwrap_or("");
                    let mut rows = vec![Line::from(vec![
                        Span::styled(
                            prefix,
                            Style::default().fg(color).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(first.to_string(), Style::default().fg(color)),
                    ])];
                    let indent = "     ";
                    for line in message_lines {
                        rows.push(Line::from(vec![
                            Span::raw(indent),
                            Span::styled(line.to_string(), Style::default().fg(color)),
                        ]));
                    }
                    rows.push(Line::from(""));
                    rows
                })
                .collect();
            let line_count = lines.len();
            let visible = messages_chunk.height.saturating_sub(2) as usize;
            let max_scroll = line_count.saturating_sub(visible);
            let scroll = max_scroll.saturating_sub(*scroll_from_bottom as usize) as u16;
            Paragraph::new(Text::from(lines))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(chat_title.as_str())
                        .border_style(Style::default().fg(Color::DarkGray)),
                )
                .wrap(Wrap { trim: false })
                .scroll((scroll, 0))
        };
        f.render_widget(message_widget, messages_chunk);

        // Input box with cursor
        let input_style = if command_input.is_empty() {
            Style::default().fg(Color::DarkGray)
        } else if command_input.starts_with('/') {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::White)
        };
        let input_display = if command_input.is_empty() {
            "Type prompt or /command... (Tab for autocomplete)".to_string()
        } else {
            format!("{}|", command_input)
        };
        let input_widget = Paragraph::new(input_display).style(input_style).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Input ")
                .border_style(Style::default().fg(Color::Cyan)),
        );
        f.render_widget(input_widget, input_chunk);

        // Autocomplete popup
        if app.show_autocomplete && !app.autocomplete_items.is_empty() {
            let item_count = app.autocomplete_items.len().min(10);
            let popup_height = (item_count + 2) as u16;
            let popup_width = 50u16.min(f.area().width.saturating_sub(4));
            let popup_y = input_chunk.y.saturating_sub(popup_height);
            let popup_x = input_chunk.x + 1;
            let popup_area =
                ratatui::layout::Rect::new(popup_x, popup_y, popup_width, popup_height);
            f.render_widget(Clear, popup_area);
            let (title, prefix) = match app.autocomplete_mode {
                crate::app::AutocompleteMode::Command => (" Commands ", "/"),
                crate::app::AutocompleteMode::Provider => (" Providers ", " "),
                crate::app::AutocompleteMode::Model => (" Models ", " "),
            };
            let items: Vec<Line> = app
                .autocomplete_items
                .iter()
                .enumerate()
                .take(10)
                .map(|(i, (name, desc))| {
                    let sel = i == app.autocomplete_index;
                    let s = if sel {
                        Style::default()
                            .fg(Color::Black)
                            .bg(Color::Green)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    let d = if sel {
                        Style::default().fg(Color::Black).bg(Color::Green)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };
                    Line::from(vec![
                        Span::styled(format!(" {}{:<12}", prefix, name), s),
                        Span::styled(format!(" {}", desc), d),
                    ])
                })
                .collect();
            let popup = Paragraph::new(Text::from(items)).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green))
                    .title(title),
            );
            f.render_widget(popup, popup_area);
        }

        // Status bar
        let mode_str = format!("{:?}", app.current_mode);
        let provider_str = app.current_provider.as_deref().unwrap_or("not configured");
        let model_str = app.current_model.as_deref().unwrap_or("none");
        let status_text = format!(
            " {} | {} | {} | {} ",
            mode_str,
            provider_str,
            model_str,
            &session_id[..8.min(session_id.len())]
        );
        let status_widget = Paragraph::new(status_text)
            .style(
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Left)
            .block(Block::default().borders(Borders::NONE));
        f.render_widget(status_widget, status_chunk);
    }
}

fn draw_status_bar(f: &mut Frame, app: &App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);

    let status_chunk = chunks[0];
    let mode_str = format!("{:?}", app.current_mode);
    let provider_str = app.current_provider.as_deref().unwrap_or("not configured");
    let model_str = app.current_model.as_deref().unwrap_or("none");
    let status_text = format!(
        " Tandem TUI | {} | {} | {} | Sessions: {} ",
        mode_str,
        provider_str,
        model_str,
        app.sessions.len()
    );
    let status_widget = Paragraph::new(status_text)
        .style(
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Left)
        .block(Block::default().borders(Borders::NONE));
    f.render_widget(status_widget, status_chunk);
}

fn draw_connecting(f: &mut Frame, app: &App) {
    // Matrix rain background
    let matrix = app.matrix.layer(false);
    f.render_widget(matrix, f.area());

    // Engine Animations
    let engine_frames = vec![
        vec![
            "    _    _    ",
            "   | |  | |   ",
            "   |_|  |_|   ",
            "    \\    /    ",
            "     \\__/     ",
        ],
        vec![
            "     _    _   ",
            "    | |  | |  ",
            "    |_|  |_|  ",
            "     \\    /   ",
            "      \\__/    ",
        ],
        vec![
            "    _    _    ",
            "   | |  | |   ",
            "   |_|  |_|   ",
            "    \\    /    ",
            "     \\__/     ",
        ],
        vec![
            "   _      _   ",
            "  | |    | |  ",
            "  |_|    |_|  ",
            "   \\      /   ",
            "    \\____/    ",
        ],
    ];

    let speed_mod = if app.tick_count % 50 > 25 { 2 } else { 4 };
    let frame_idx = (app.tick_count / speed_mod) % engine_frames.len();
    let current_frame = &engine_frames[frame_idx];

    // RPM Gauge
    let cycle = 20;
    let step = app.tick_count % cycle;
    let rev_level = if step < cycle / 2 { step } else { cycle - step };
    let bar_width = 15;
    let filled = (rev_level * bar_width) / 10;
    let gauge = format!("[{:<15}]", "=".repeat(filled));
    let gauge_color = if filled > 10 {
        Color::Red
    } else {
        Color::Green
    };

    let mut lines = Vec::new();
    lines.push(Line::from(""));
    for line in current_frame {
        lines.push(Line::from(vec![Span::styled(
            *line,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        &app.connection_status,
        Style::default()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    )]));
    lines.push(Line::from(vec![
        Span::styled("RPM: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            gauge,
            Style::default()
                .fg(gauge_color)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(""));

    let content = Paragraph::new(lines).alignment(Alignment::Center).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(" Engine Start "),
    );

    let area = centered_rect(50, 40, f.area());
    f.render_widget(Clear, area);
    f.render_widget(content, area);
}

fn draw_setup_wizard(f: &mut Frame, app: &App) {
    let area = f.area();
    let title = Paragraph::new("Tandem Setup Wizard")
        .style(Style::default().fg(Color::Cyan))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::ALL).title("Welcome"));

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(area);

    f.render_widget(title, layout[0]);

    let content = match &app.state {
        AppState::SetupWizard { step, provider_catalog, selected_provider_index, selected_model_index, api_key_input, model_input } => {
            match step {
                SetupStep::Welcome => {
                    Paragraph::new(
                        "Welcome to Tandem AI!\n\nPress ENTER to get started.\n\nUse j/k or Up/Down to navigate, Enter to select.",
                    )
                    .style(Style::default().fg(Color::White))
                    .alignment(Alignment::Center)
                    .block(Block::default().borders(Borders::ALL))
                }
                SetupStep::SelectProvider => {
                    let mut text = "Select a Provider:\n\n".to_string();
                    if let Some(ref catalog) = provider_catalog {
                        for (i, provider) in catalog.all.iter().enumerate() {
                            let marker = if i == *selected_provider_index { ">" } else { " " };
                            text.push_str(&format!("{} {}\n", marker, provider.id));
                        }
                    } else {
                        text.push_str(" Loading providers...\n");
                    }
                    text.push_str("\nPress ENTER to continue.");
                    Paragraph::new(text)
                        .style(Style::default().fg(Color::White))
                        .block(Block::default().borders(Borders::ALL).title("Select Provider"))
                }
                SetupStep::EnterApiKey => {
                    let masked_key = "*".repeat(api_key_input.len());
                    Paragraph::new(
                        format!("Enter API Key for provider:\n\n{}\n\nPress ENTER when done.", masked_key),
                    )
                    .style(Style::default().fg(Color::White))
                    .block(Block::default().borders(Borders::ALL).title("API Key"))
                }
                SetupStep::SelectModel => {
                    let mut text = "Select a Model:\n\n".to_string();
                    if model_input.trim().is_empty() {
                        text.push_str("Filter: (type to filter)\n\n");
                    } else {
                        text.push_str(&format!("Filter: {}\n\n", model_input.trim()));
                    }
                    if let Some(ref catalog) = provider_catalog {
                        if *selected_provider_index < catalog.all.len() {
                            let provider = &catalog.all[*selected_provider_index];
                            let mut model_ids: Vec<String> = provider.models.keys().cloned().collect();
                            model_ids.sort();
                            let query = model_input.trim().to_lowercase();
                            let filtered: Vec<String> = if query.is_empty() {
                                model_ids
                            } else {
                                model_ids
                                    .into_iter()
                                    .filter(|m| m.to_lowercase().contains(&query))
                                    .collect()
                            };
                            let total = filtered.len();
                            let visible_rows = 14usize;
                            let start = if total <= visible_rows {
                                0
                            } else {
                                selected_model_index
                                    .saturating_sub(visible_rows / 2)
                                    .min(total.saturating_sub(visible_rows))
                            };
                            let end = (start + visible_rows).min(total);
                            if total == 0 {
                                text.push_str("  No matches.\n");
                            } else {
                                if start > 0 {
                                    text.push_str("  ...\n");
                                }
                                for (i, model_id) in filtered[start..end].iter().enumerate() {
                                    let absolute_index = start + i;
                                    let marker = if absolute_index == *selected_model_index {
                                        ">"
                                    } else {
                                        " "
                                    };
                                    text.push_str(&format!("{} {}\n", marker, model_id));
                                }
                                if end < total {
                                    text.push_str("  ...\n");
                                }
                            }
                        }
                    }
                    text.push_str("\nPress ENTER to complete setup.");
                    Paragraph::new(text)
                        .style(Style::default().fg(Color::White))
                        .block(Block::default().borders(Borders::ALL).title("Select Model"))
                }
                SetupStep::Complete => {
                    Paragraph::new("Setup Complete!\n\nPress ENTER to continue to the main menu.")
                        .style(Style::default().fg(Color::Green))
                        .alignment(Alignment::Center)
                        .block(Block::default().borders(Borders::ALL))
                }
            }
        }
        _ => Paragraph::new(""),
    };

    f.render_widget(content, layout[1]);

    let help = Paragraph::new("j/k: Navigate | ENTER: Select | ESC: Quit")
        .style(Style::default().fg(Color::DarkGray))
        .alignment(Alignment::Center);
    f.render_widget(help, layout[2]);
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
