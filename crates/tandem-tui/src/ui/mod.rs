use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

pub mod components;
pub mod matrix;
use crate::app::{
    AgentStatus, App, AppState, ChatMessage, ContentBlock, ModalState, PendingRequestKind,
    PinPromptMode, SetupStep, UiMode,
};
use crate::ui::components::{flow::FlowList, task_list::TaskList};

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

    let masked_input = if input.is_empty() {
        " ".to_string()
    } else {
        input.chars().map(|_| '*').collect::<String>()
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

    let popup_h = if error.is_some() { 9 } else { 7 };
    let popup_area = centered_fixed_rect(52, popup_h, f.area());
    f.render_widget(Clear, popup_area);

    let popup_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(title);
    let inner = popup_block.inner(popup_area);
    f.render_widget(popup_block, popup_area);

    let mut rows = vec![Constraint::Length(3), Constraint::Length(1)];
    if error.is_some() {
        rows.push(Constraint::Length(1));
    }
    let content = Layout::default()
        .direction(Direction::Vertical)
        .constraints(rows)
        .split(inner);

    let input_widget = Paragraph::new(masked_input)
        .style(Style::default().fg(Color::Yellow))
        .alignment(Alignment::Center);
    f.render_widget(input_widget, content[0]);

    if let Some(err) = error {
        let error_widget = Paragraph::new(err)
            .style(Style::default().fg(Color::Red))
            .alignment(Alignment::Center);
        f.render_widget(error_widget, content[1]);
    }

    let hint_widget = Paragraph::new(hint)
        .style(Style::default().fg(Color::Gray))
        .alignment(Alignment::Center);
    let hint_idx = if error.is_some() { 2 } else { 1 };
    f.render_widget(hint_widget, content[hint_idx]);
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
        tasks,
        agents,
        active_agent_index,
        ui_mode,
        grid_page,
        modal,
        pending_requests,
        request_cursor,
        permission_choice,
        ..
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

        let content_area = chunks[0];
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

        // Split content area for tasks only in focus mode.
        let (messages_area, tasks_area) = if *ui_mode == UiMode::Focus && !tasks.is_empty() {
            let areas = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                .split(content_area);
            (areas[0], Some(areas[1]))
        } else {
            (content_area, None)
        };

        // Engine Status
        let (status_color, status_text) = match app.engine_health {
            crate::app::EngineConnectionStatus::Disconnected => (Color::DarkGray, " ○ Offline "),
            crate::app::EngineConnectionStatus::Connecting => (Color::Yellow, " ◌ Connecting "),
            crate::app::EngineConnectionStatus::Connected => (Color::Green, " ● Online "),
            crate::app::EngineConnectionStatus::Error => (Color::Red, " ✖ Error "),
        };
        let status_title = ratatui::widgets::block::Title::from(Span::styled(
            status_text,
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Right);

        if *ui_mode == UiMode::Focus {
            // Focus mode: active agent transcript only.
            if messages.is_empty() {
                let empty_msg = Paragraph::new("No messages yet. Type a prompt or /help for commands.\n\nTab/Shift+Tab switches active agent.")
                    .style(Style::default().fg(Color::DarkGray))
                    .alignment(Alignment::Center)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(chat_title.as_str())
                            .title(status_title.clone())
                            .border_style(Style::default().fg(Color::DarkGray)),
                    );
                f.render_widget(empty_msg, messages_area);
            } else {
                let flow_list = FlowList::new(messages).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .borders(Borders::ALL)
                        .title(chat_title.as_str())
                        .title(status_title)
                        .border_style(Style::default().fg(Color::DarkGray)),
                );

                let mut flow_state = crate::ui::components::flow::FlowListState {
                    offset: *scroll_from_bottom as usize,
                };
                f.render_stateful_widget(flow_list, messages_area, &mut flow_state);
            }
        } else {
            // Grid mode: up to 4 panes per page.
            let start = (*grid_page).saturating_mul(4);
            let end = (start + 4).min(agents.len());
            let page_agents = &agents[start..end];

            let pane_areas = match page_agents.len() {
                0 => Vec::new(),
                1 => vec![messages_area],
                2 => Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(messages_area)
                    .to_vec(),
                3 => {
                    let rows = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .split(messages_area);
                    let top = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .split(rows[0]);
                    vec![top[0], top[1], rows[1]]
                }
                _ => {
                    let rows = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .split(messages_area);
                    let top = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .split(rows[0]);
                    let bot = Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                        .split(rows[1]);
                    vec![top[0], top[1], bot[0], bot[1]]
                }
            };

            for (pane_idx, agent) in page_agents.iter().enumerate() {
                if pane_idx >= pane_areas.len() {
                    break;
                }
                let is_active = start + pane_idx == *active_agent_index;
                let status_label = match agent.status {
                    AgentStatus::Running | AgentStatus::Streaming => {
                        format!("{} Working", spinner_frame(app.tick_count))
                    }
                    AgentStatus::Cancelling => "Cancelling".to_string(),
                    AgentStatus::Done => "Done".to_string(),
                    AgentStatus::Error => "Error".to_string(),
                    AgentStatus::Closed => "Closed".to_string(),
                    AgentStatus::Idle => "Idle".to_string(),
                };
                let title = format!(
                    " {} {} [{}] ",
                    if is_active { ">" } else { " " },
                    agent.agent_id,
                    status_label
                );
                if agent.messages.is_empty() {
                    let empty = Paragraph::new("No output yet")
                        .style(Style::default().fg(Color::DarkGray))
                        .block(
                            Block::default()
                                .borders(Borders::ALL)
                                .title(title)
                                .border_style(if is_active {
                                    Style::default().fg(Color::Yellow)
                                } else {
                                    Style::default().fg(Color::DarkGray)
                                }),
                        );
                    f.render_widget(empty, pane_areas[pane_idx]);
                } else {
                    let flow_list = FlowList::new(&agent.messages).block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(title)
                            .border_style(if is_active {
                                Style::default().fg(Color::Yellow)
                            } else {
                                Style::default().fg(Color::DarkGray)
                            }),
                    );
                    let mut flow_state = crate::ui::components::flow::FlowListState {
                        offset: agent.scroll_from_bottom as usize,
                    };
                    f.render_stateful_widget(flow_list, pane_areas[pane_idx], &mut flow_state);
                }
            }
        }

        // Render Tasks (TaskList)
        if let Some(area) = tasks_area {
            let task_list = TaskList::new(tasks)
                .block(Block::default().borders(Borders::ALL).title(" Tasks "))
                .spinner_frame(app.tick_count);

            let mut task_state = crate::ui::components::task_list::TaskListState::default();
            f.render_stateful_widget(task_list, area, &mut task_state);
        }

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
        let active_label = agents
            .get(*active_agent_index)
            .map(|a| a.agent_id.as_str())
            .unwrap_or("A1");
        let active_activity = agents
            .get(*active_agent_index)
            .map(|a| match a.status {
                AgentStatus::Running | AgentStatus::Streaming => {
                    format!("{} Working", spinner_frame(app.tick_count))
                }
                AgentStatus::Cancelling => "Cancelling".to_string(),
                AgentStatus::Done => "Done".to_string(),
                AgentStatus::Error => "Error".to_string(),
                AgentStatus::Closed => "Closed".to_string(),
                AgentStatus::Idle => "Idle".to_string(),
            })
            .unwrap_or_else(|| "Idle".to_string());
        let context_chars = estimate_context_chars(messages);
        let context_limit = resolve_context_limit(app);
        let context_label = match context_limit {
            Some(limit) if limit > 0 => {
                let pct = ((context_chars as f64 / limit as f64) * 100.0).round() as i64;
                format!("Ctx~{}%", pct.max(0))
            }
            _ => format!("Ctx~{}ch", context_chars),
        };
        let compacting_label = if matches!(
            agents.get(*active_agent_index).map(|a| &a.status),
            Some(AgentStatus::Running | AgentStatus::Streaming)
        ) {
            if let Some(limit) = context_limit {
                if limit > 0 && context_chars.saturating_mul(100) >= (limit as usize * 90) {
                    Some(format!("{} Compacting", spinner_frame(app.tick_count)))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };
        let (active_req_count, background_req_count) = app.pending_request_counts();
        let status_text = format!(
            " Tandem TUI | {} | {} | {} | {} | Active: {} ({}) | {}{} | Req:{}/{} ",
            mode_str,
            provider_str,
            model_str,
            &session_id[..8.min(session_id.len())],
            active_label,
            active_activity,
            context_label,
            compacting_label
                .map(|v| format!(" | {}", v))
                .unwrap_or_default(),
            active_req_count,
            background_req_count
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

        if let Some(modal_state) = modal {
            let area = centered_rect(58, 34, f.area());
            f.render_widget(Clear, area);
            let text = match modal_state {
                ModalState::Help => "Keys:\nF1 Help\nTab/Shift+Tab switch agent\nAlt+1..9 jump agent\nCtrl+N new agent\nCtrl+W close agent\nCtrl+C cancel active run\nAlt+G toggle grid\nAlt+R open request center\nAlt+S / Alt+B demo streams\nShift+Enter newline\nEsc close modal/input\nCtrl+X quit",
                ModalState::ConfirmCloseAgent { target_agent_id } => {
                    if target_agent_id.is_empty() {
                        "Close active agent and discard draft? (Y/N)"
                    } else {
                        "Discard draft and close this agent? (Y/N)"
                    }
                }
                ModalState::RequestCenter => "Request center",
            };
            let popup_block = Block::default()
                .title(" Modal ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow));

            let popup = if matches!(modal_state, ModalState::RequestCenter) {
                if let Some(request) = pending_requests.get(*request_cursor) {
                    if let PendingRequestKind::Permission(permission) = &request.kind {
                        let args_preview = permission
                            .args
                            .as_ref()
                            .map(|args| args.to_string())
                            .unwrap_or_else(|| "{}".to_string());

                        let choice_chip =
                            |idx: usize, label: &str, active: usize| -> Span<'static> {
                                if idx == active {
                                    Span::styled(
                                        format!(" {} ", label),
                                        Style::default()
                                            .fg(Color::Black)
                                            .bg(Color::LightGreen)
                                            .add_modifier(Modifier::BOLD),
                                    )
                                } else {
                                    Span::styled(
                                        format!(" {} ", label),
                                        Style::default().fg(Color::Gray).bg(Color::DarkGray),
                                    )
                                }
                            };

                        let choice_line = Line::from(vec![
                            choice_chip(0, "1 Allow Once", *permission_choice),
                            Span::raw(" "),
                            choice_chip(1, "2 Allow Always", *permission_choice),
                            Span::raw(" "),
                            choice_chip(2, "3 Deny", *permission_choice),
                        ]);

                        let mode_name = format!("{:?}", app.current_mode);
                        let why = permission_reason(&permission.tool, &mode_name);
                        let mut lines = vec![
                            Line::from(format!(
                                "Pending request {}/{}",
                                request_cursor + 1,
                                pending_requests.len()
                            )),
                            Line::from(format!(
                                "Session: {} | Agent: {}",
                                request.session_id, request.agent_id
                            )),
                            Line::from("Type: Permission"),
                            Line::from(format!("Mode: {}", mode_name)),
                            Line::from(format!("Tool: {}", permission.tool)),
                            Line::from(format!("Request ID: {}", permission.id)),
                            Line::from(format!("Args: {}", args_preview)),
                            Line::from(format!("Why this permission: {}", why)),
                        ];

                        if permission.tool.eq_ignore_ascii_case("question") {
                            let prompts = extract_permission_questions(permission.args.as_ref());
                            if !prompts.is_empty() {
                                lines.push(Line::from(""));
                                lines.push(Line::from(format!(
                                    "This will ask you {} question(s):",
                                    prompts.len()
                                )));
                                for q in prompts.iter().take(4) {
                                    lines.push(Line::from(format!("  - {}", q)));
                                }
                                if prompts.len() > 4 {
                                    lines.push(Line::from("  - ..."));
                                }
                                lines.push(Line::from(
                                    "Tip: choose `Allow Once` to continue and answer them.",
                                ));
                            }
                        }

                        lines.push(Line::from(""));
                        lines.push(Line::from("Selected choice:"));
                        lines.push(choice_line);
                        lines.push(Line::from(""));
                        lines.push(Line::from("Keys: Up/Down request, Left/Right choice, 1..3 quick choice, Enter confirm, R reject, Esc close"));

                        let text = Text::from(lines);

                        Paragraph::new(text)
                            .wrap(Wrap { trim: true })
                            .block(popup_block)
                    } else {
                        let modal_text = if let PendingRequestKind::Question(question) =
                            &request.kind
                        {
                            if let Some(q) = question.questions.get(question.question_index) {
                                let mut options = String::new();
                                for (idx, option) in q.options.iter().enumerate() {
                                    let selected = if q.selected_options.contains(&idx) {
                                        "*"
                                    } else {
                                        " "
                                    };
                                    let cursor = if q.option_cursor == idx { ">" } else { " " };
                                    options.push_str(&format!(
                                        "{}{} {}. {} {}\n",
                                        cursor,
                                        selected,
                                        idx + 1,
                                        option.label,
                                        option.description
                                    ));
                                }
                                let custom_display = if q.custom_input.trim().is_empty() {
                                    "<none>"
                                } else {
                                    q.custom_input.trim()
                                };
                                format!(
                                    "Pending request {}/{}\nSession: {} | Agent: {}\nType: Question\nRequest ID: {}\n\nQuestion {}/{}: {}\n{}\n{}\nCustom: {}\n\nKeys: Up/Down request, Left/Right option, Space toggle, 1..9 quick toggle, type custom text, Backspace edit, Enter next/submit, R reject, Esc close",
                                    request_cursor + 1,
                                    pending_requests.len(),
                                    request.session_id,
                                    request.agent_id,
                                    question.id,
                                    question.question_index + 1,
                                    question.questions.len(),
                                    q.header,
                                    q.question,
                                    options,
                                    custom_display
                                )
                            } else {
                                "Question request has no prompts.".to_string()
                            }
                        } else {
                            "No pending requests.".to_string()
                        };
                        Paragraph::new(modal_text)
                            .wrap(Wrap { trim: true })
                            .block(popup_block)
                    }
                } else {
                    Paragraph::new("No pending requests.")
                        .wrap(Wrap { trim: true })
                        .block(popup_block)
                }
            } else {
                Paragraph::new(text)
                    .wrap(Wrap { trim: true })
                    .block(popup_block)
            };
            f.render_widget(popup, area);
        }
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
    let identity = app
        .sessions
        .get(app.selected_session_index)
        .map(|s| s.id.as_str())
        .or(app.engine_lease_id.as_deref())
        .unwrap_or("engine");
    let status_text = format!(
        " Tandem TUI | {} | {} | {} | {} ",
        mode_str,
        provider_str,
        model_str,
        &identity[..8.min(identity.len())]
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

fn permission_reason(tool: &str, mode: &str) -> &'static str {
    match tool {
        "question" => "The agent wants to ask you one or more clarification questions.",
        "task" | "todo_write" | "todowrite" | "update_todo_list" | "new_task" => {
            if mode.eq_ignore_ascii_case("Plan") {
                "Plan mode uses task/todo tools to propose or update structured steps."
            } else {
                "The agent wants to track or update a task/todo item."
            }
        }
        "bash" | "run_command" => "The agent wants to run a shell command.",
        "read" | "glob" | "grep" | "codesearch" | "search" | "ls" | "list" => {
            "The agent wants to inspect workspace files."
        }
        "websearch" | "webfetch" | "webfetch_document" => {
            "The agent wants to retrieve information from the web."
        }
        _ => "The agent requested a tool call that needs your approval.",
    }
}

fn extract_permission_questions(args: Option<&serde_json::Value>) -> Vec<String> {
    let Some(args) = args else {
        return Vec::new();
    };
    let Some(items) = args.get("questions").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| item.get("question").and_then(|q| q.as_str()))
        .map(|s| s.to_string())
        .collect()
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
    let connect_frames = ["Starting", "Starting.", "Starting..", "Starting..."];
    let connect_label = connect_frames[(app.tick_count / 10) % connect_frames.len()];
    lines.push(Line::from(vec![
        Span::styled(connect_label, Style::default().fg(Color::Cyan)),
        Span::raw(" "),
        Span::styled(
            &app.connection_status,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
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

fn centered_fixed_rect(
    width: u16,
    height: u16,
    area: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let w = width.min(area.width.max(1));
    let h = height.min(area.height.max(1));
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    ratatui::layout::Rect::new(x, y, w, h)
}

fn spinner_frame(tick: usize) -> &'static str {
    const FRAMES: [&str; 4] = ["|", "/", "-", "\\"];
    FRAMES[tick % FRAMES.len()]
}

fn resolve_context_limit(app: &App) -> Option<u32> {
    let provider_id = app.current_provider.as_ref()?;
    let model_id = app.current_model.as_ref()?;
    let catalog = app.provider_catalog.as_ref()?;
    let provider = catalog.all.iter().find(|p| &p.id == provider_id)?;
    provider
        .models
        .get(model_id)
        .and_then(|m| m.limit.as_ref())
        .and_then(|l| l.context)
}

fn estimate_context_chars(messages: &[ChatMessage]) -> usize {
    messages
        .iter()
        .map(|m| {
            m.content
                .iter()
                .map(|b| match b {
                    ContentBlock::Text(t) => t.len(),
                    ContentBlock::Code { language, code } => language.len() + code.len(),
                    ContentBlock::ToolCall(t) => t.name.len() + t.args.len() + t.id.len(),
                    ContentBlock::ToolResult(t) => t.len(),
                })
                .sum::<usize>()
        })
        .sum()
}
