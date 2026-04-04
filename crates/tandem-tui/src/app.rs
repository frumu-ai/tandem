use crate::ui::components::composer_input::ComposerInputState;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use std::collections::{HashMap, HashSet, VecDeque};

mod agent_management;
mod agent_team;
mod commands;
mod overlay_actions;
mod paste_actions;
mod plan_helpers;
mod prompt_actions;
mod state_sync;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Tick,
    Quit,
    CtrlCPressed,
    EnterPin(char),
    SubmitPin,
    CreateSession,
    LoadSessions,
    SessionsLoaded(Vec<Session>),
    SelectSession,
    DeleteSelectedSession,
    NewSession,
    NextSession,
    PreviousSession,
    SkipAnimation,
    CommandInput(char),
    SubmitCommand,
    ClearCommand,
    BackspaceCommand,
    DeleteForwardCommand,
    InsertNewline,
    MoveCursorLeft,
    MoveCursorRight,
    MoveCursorHome,
    MoveCursorEnd,
    MoveCursorUp,
    MoveCursorDown,
    PasteInput(String),
    PasteFromClipboard,
    SwitchToChat,
    Autocomplete,
    AutocompleteNext,
    AutocompletePrev,
    AutocompleteAccept,
    AutocompleteDismiss,
    BackToMenu,
    SetupNextStep,
    SetupPrevItem,
    SetupNextItem,
    SetupInput(char),
    SetupBackspace,
    ScrollUp,
    ScrollDown,
    PageUp,
    PageDown,
    ToggleTaskPin(String),
    PromptSuccess {
        session_id: String,
        agent_id: String,
        messages: Vec<ChatMessage>,
    },
    PromptDelta {
        session_id: String,
        agent_id: String,
        delta: String,
    },
    PromptInfo {
        session_id: String,
        agent_id: String,
        message: String,
    },
    PromptToolDelta {
        session_id: String,
        agent_id: String,
        tool_call_id: String,
        tool_name: String,
        args_delta: String,
        args_preview: String,
    },
    PromptTodoUpdated {
        session_id: String,
        todos: Vec<Value>,
    },
    PromptAgentTeamEvent {
        session_id: String,
        agent_id: String,
        event: crate::net::client::StreamAgentTeamEvent,
    },
    PromptRequest {
        session_id: String,
        agent_id: String,
        request: PendingRequestKind,
    },
    PromptMalformedQuestion {
        session_id: String,
        agent_id: String,
        request_id: String,
    },
    PromptRequestResolved {
        session_id: String,
        agent_id: String,
        request_id: String,
        reply: String,
    },
    PromptFailure {
        session_id: String,
        agent_id: String,
        error: String,
    },
    PromptRunStarted {
        session_id: String,
        agent_id: String,
        run_id: Option<String>,
    },
    NewAgent,
    CloseActiveAgent,
    SwitchAgentNext,
    SwitchAgentPrev,
    SelectAgentByNumber(usize),
    ToggleUiMode,
    GridPageNext,
    GridPagePrev,
    CycleMode,
    ShowHelpModal,
    CloseModal,
    OpenRequestCenter,
    OpenFileSearch,
    OpenDiffOverlay,
    OpenExternalEditor,
    ToggleRequestPanelExpand,
    OverlayScrollUp,
    OverlayScrollDown,
    OverlayPageUp,
    OverlayPageDown,
    FileSearchInput(char),
    FileSearchBackspace,
    FileSearchSelectNext,
    FileSearchSelectPrev,
    FileSearchConfirm,
    RequestSelectNext,
    RequestSelectPrev,
    RequestOptionNext,
    RequestOptionPrev,
    RequestToggleCurrent,
    RequestConfirm,
    RequestDigit(u8),
    RequestInput(char),
    RequestBackspace,
    RequestReject,
    PlanWizardNextField,
    PlanWizardPrevField,
    PlanWizardInput(char),
    PlanWizardBackspace,
    PlanWizardSubmit,
    ConfirmCloseAgent(bool),
    ConfirmStartPlanAgents {
        confirmed: bool,
        count: usize,
    },
    CancelActiveAgent,
    StartDemoStream,
    SpawnBackgroundDemo,
    OpenDocs,
    CopyLastAssistant,
    QueueSteeringFromComposer,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::{backend::TestBackend, Terminal};

    fn chat_app() -> App {
        let mut app = App::new();
        let session_id = "s-test".to_string();
        let agent = App::make_agent_pane("A1".to_string(), session_id.clone());
        app.state = AppState::Chat {
            session_id,
            command_input: ComposerInputState::new(),
            messages: Vec::new(),
            scroll_from_bottom: 0,
            tasks: Vec::new(),
            active_task_id: None,
            agents: vec![agent],
            active_agent_index: 0,
            ui_mode: UiMode::Focus,
            grid_page: 0,
            modal: None,
            pending_requests: Vec::new(),
            request_cursor: 0,
            permission_choice: 0,
            plan_wizard: PlanFeedbackWizardState::default(),
            last_plan_task_fingerprint: Vec::new(),
            plan_awaiting_approval: false,
            plan_multi_agent_prompt: None,
            plan_waiting_for_clarification_question: false,
            request_panel_expanded: false,
        };
        app
    }

    fn two_agent_app() -> App {
        let mut app = chat_app();
        if let AppState::Chat {
            agents,
            active_agent_index,
            ..
        } = &mut app.state
        {
            agents.push(App::make_agent_pane("A2".to_string(), "s-test".to_string()));
            *active_agent_index = 0;
        }
        app
    }

    fn render_to_text(app: &App) -> String {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).expect("terminal");
        terminal
            .draw(|f| crate::ui::draw(f, app))
            .expect("draw frame");
        let buffer = terminal.backend().buffer();
        let mut lines: Vec<String> = Vec::new();
        for y in 0..buffer.area.height {
            let mut line = String::new();
            for x in 0..buffer.area.width {
                line.push_str(buffer.get(x, y).symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        lines.join("\n")
    }

    #[test]
    fn render_activity_strip_groups_exploration_calls() {
        let mut app = chat_app();
        if let AppState::Chat { agents, .. } = &mut app.state {
            let agent = &mut agents[0];
            agent.status = AgentStatus::Streaming;
            agent.live_tool_calls.insert(
                "read-1".to_string(),
                LiveToolCall {
                    tool_name: "Read".to_string(),
                    args_preview: "/workspace/src/main.rs".to_string(),
                },
            );
            agent.live_tool_calls.insert(
                "search-1".to_string(),
                LiveToolCall {
                    tool_name: "SearchCodebase".to_string(),
                    args_preview: "find rollback preview".to_string(),
                },
            );
        }

        let rendered = render_to_text(&app);
        let summary = app.active_activity_summary().expect("activity summary");
        assert!(rendered.contains("Activity"));
        assert!(rendered.contains("Exploring"));
        assert!(summary.detail.contains("read"));
        assert!(summary.detail.contains("search"));
    }

    #[test]
    fn render_activity_strip_surfaces_pending_requests() {
        let mut app = chat_app();
        if let AppState::Chat {
            pending_requests, ..
        } = &mut app.state
        {
            pending_requests.push(PendingRequest {
                session_id: "s-test".to_string(),
                agent_id: "A1".to_string(),
                kind: PendingRequestKind::Permission(PendingPermissionRequest {
                    id: "perm-1".to_string(),
                    tool: "RunCommand".to_string(),
                    args: None,
                    args_source: None,
                    args_integrity: None,
                    query: None,
                    status: None,
                }),
            });
        }

        let rendered = render_to_text(&app);
        assert!(rendered.contains("Attention"));
        assert!(rendered.contains("Waiting for approval"));
    }

    #[test]
    fn keymap_cursor_and_edit_actions_in_chat() {
        let app = chat_app();
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
            Some(Action::MoveCursorLeft)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
            Some(Action::MoveCursorRight)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)),
            Some(Action::MoveCursorHome)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)),
            Some(Action::MoveCursorEnd)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE)),
            Some(Action::DeleteForwardCommand)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
            Some(Action::BackspaceCommand)
        );
    }

    #[test]
    fn keymap_line_nav_and_newline_shortcuts() {
        let app = chat_app();
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL)),
            Some(Action::MoveCursorUp)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL)),
            Some(Action::MoveCursorDown)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)),
            Some(Action::InsertNewline)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT)),
            Some(Action::InsertNewline)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Action::SubmitCommand)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)),
            Some(Action::SubmitCommand)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT)),
            Some(Action::QueueSteeringFromComposer)
        );
    }

    #[test]
    fn keymap_coding_overlay_shortcuts() {
        let app = chat_app();
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::ALT)),
            Some(Action::OpenFileSearch)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::ALT)),
            Some(Action::OpenDiffOverlay)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::ALT)),
            Some(Action::OpenExternalEditor)
        );
    }

    #[test]
    fn keymap_file_search_modal_controls() {
        let mut app = chat_app();
        if let AppState::Chat { modal, .. } = &mut app.state {
            *modal = Some(ModalState::FileSearch);
        }
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            Some(Action::FileSearchSelectNext)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE)),
            Some(Action::FileSearchBackspace)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Action::FileSearchConfirm)
        );
    }

    #[test]
    fn autocomplete_mode_keeps_cursor_keymap() {
        let mut app = chat_app();
        app.show_autocomplete = true;
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE)),
            Some(Action::MoveCursorLeft)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)),
            Some(Action::MoveCursorRight)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Delete, KeyModifiers::NONE)),
            Some(Action::DeleteForwardCommand)
        );
    }

    #[test]
    fn setup_wizard_accepts_paste_shortcuts() {
        let mut app = App::new();
        app.state = AppState::SetupWizard {
            step: SetupStep::EnterApiKey,
            provider_catalog: None,
            selected_provider_index: 0,
            selected_model_index: 0,
            api_key_input: String::new(),
            model_input: String::new(),
        };
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL)),
            Some(Action::PasteFromClipboard)
        );
        assert_eq!(
            app.handle_key_event(KeyEvent::new(KeyCode::Insert, KeyModifiers::SHIFT)),
            Some(Action::PasteFromClipboard)
        );
    }

    #[tokio::test]
    async fn setup_wizard_paste_input_appends_api_key() {
        let mut app = App::new();
        app.state = AppState::SetupWizard {
            step: SetupStep::EnterApiKey,
            provider_catalog: None,
            selected_provider_index: 0,
            selected_model_index: 0,
            api_key_input: String::new(),
            model_input: String::new(),
        };
        app.update(Action::PasteInput("sk-test-key\n".to_string()))
            .await
            .expect("paste update");
        if let AppState::SetupWizard { api_key_input, .. } = &app.state {
            assert_eq!(api_key_input, "sk-test-key");
        } else {
            panic!("expected setup wizard state");
        }
    }

    fn chat_assistant_text(app: &App) -> String {
        let AppState::Chat { messages, .. } = &app.state else {
            return String::new();
        };
        messages
            .iter()
            .filter(|m| matches!(m.role, MessageRole::Assistant))
            .flat_map(|m| m.content.iter())
            .filter_map(|b| match b {
                ContentBlock::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    #[tokio::test]
    async fn reducer_stream_roundtrip_success_flushes_tail() {
        let mut app = chat_app();
        let session_id = "s-test".to_string();
        let agent_id = "A1".to_string();
        let source = "line1\nline2\ntail";

        app.update(Action::PromptRunStarted {
            session_id: session_id.clone(),
            agent_id: agent_id.clone(),
            run_id: Some("r1".to_string()),
        })
        .await
        .unwrap();

        for chunk in ["li", "ne1\nl", "ine2", "\n", "ta", "il"] {
            app.update(Action::PromptDelta {
                session_id: session_id.clone(),
                agent_id: agent_id.clone(),
                delta: chunk.to_string(),
            })
            .await
            .unwrap();
        }

        let partial = chat_assistant_text(&app);
        assert_eq!(partial, "line1\nline2\n");

        app.update(Action::PromptSuccess {
            session_id: session_id.clone(),
            agent_id: agent_id.clone(),
            messages: vec![],
        })
        .await
        .unwrap();

        let final_text = chat_assistant_text(&app);
        assert_eq!(final_text, source);

        if let AppState::Chat { agents, .. } = &app.state {
            assert!(agents
                .iter()
                .find(|a| a.agent_id == agent_id)
                .and_then(|a| a.stream_collector.as_ref())
                .is_none());
        }
    }

    #[tokio::test]
    async fn reducer_stream_roundtrip_failure_flushes_tail_before_error() {
        let mut app = chat_app();
        let session_id = "s-test".to_string();
        let agent_id = "A1".to_string();
        let source = "alpha\nbeta\ngamma";

        app.update(Action::PromptRunStarted {
            session_id: session_id.clone(),
            agent_id: agent_id.clone(),
            run_id: Some("r2".to_string()),
        })
        .await
        .unwrap();

        for chunk in ["alpha\nbe", "ta\ng", "amma"] {
            app.update(Action::PromptDelta {
                session_id: session_id.clone(),
                agent_id: agent_id.clone(),
                delta: chunk.to_string(),
            })
            .await
            .unwrap();
        }

        app.update(Action::PromptFailure {
            session_id: session_id.clone(),
            agent_id: agent_id.clone(),
            error: "boom".to_string(),
        })
        .await
        .unwrap();

        let final_text = chat_assistant_text(&app);
        assert_eq!(final_text, source);

        if let AppState::Chat {
            messages, agents, ..
        } = &app.state
        {
            assert!(messages.iter().any(|m| {
                matches!(m.role, MessageRole::System)
                    && m.content.iter().any(
                        |b| matches!(b, ContentBlock::Text(t) if t.contains("Prompt failed: boom")),
                    )
            }));
            assert!(agents
                .iter()
                .find(|a| a.agent_id == agent_id)
                .and_then(|a| a.stream_collector.as_ref())
                .is_none());
        }
    }

    #[tokio::test]
    async fn reducer_stream_roundtrip_utf8_chunks() {
        let mut app = chat_app();
        let session_id = "s-test".to_string();
        let agent_id = "A1".to_string();
        let source = "🙂🙂🙂\n汉字漢字\nA\u{0304}B";

        app.update(Action::PromptRunStarted {
            session_id: session_id.clone(),
            agent_id: agent_id.clone(),
            run_id: Some("r3".to_string()),
        })
        .await
        .unwrap();

        for chunk in ["🙂", "🙂🙂\n汉", "字漢", "字\nA", "\u{0304}", "B"] {
            app.update(Action::PromptDelta {
                session_id: session_id.clone(),
                agent_id: agent_id.clone(),
                delta: chunk.to_string(),
            })
            .await
            .unwrap();
        }

        app.update(Action::PromptSuccess {
            session_id,
            agent_id,
            messages: vec![],
        })
        .await
        .unwrap();

        let final_text = chat_assistant_text(&app);
        assert_eq!(final_text, source);
    }

    #[test]
    fn paste_markers_expand_to_original_payload() {
        let mut app = chat_app();
        let payload = "line1\nline2\nline3\nline4\nline5\nline6\nline7\nline8\nline9\nline10";
        if let AppState::Chat {
            agents,
            active_agent_index,
            ..
        } = &mut app.state
        {
            let marker = App::register_collapsed_paste(&mut agents[*active_agent_index], payload);
            let expanded = App::expand_paste_markers(&marker, &agents[*active_agent_index]);
            assert_eq!(expanded, payload);
        } else {
            panic!("expected chat state");
        }
    }

    #[test]
    fn collapse_paste_only_for_more_than_two_lines() {
        assert!(!App::should_collapse_paste("single line"));
        assert!(!App::should_collapse_paste("line1\nline2"));
        assert!(!App::should_collapse_paste("line1\nline2\n"));
        assert!(App::should_collapse_paste("line1\nline2\nline3"));
    }

    #[tokio::test]
    async fn chat_paste_input_inserts_small_payload_directly() {
        let mut app = chat_app();
        app.update(Action::PasteInput("alpha\nbeta".to_string()))
            .await
            .expect("paste update");
        if let AppState::Chat { command_input, .. } = &app.state {
            assert_eq!(command_input.text(), "alpha\nbeta");
            assert!(!command_input.text().contains("[Pasted "));
        } else {
            panic!("expected chat state");
        }
    }

    #[tokio::test]
    async fn chat_paste_input_collapses_large_payload() {
        let mut app = chat_app();
        app.update(Action::PasteInput("a\nb\nc".to_string()))
            .await
            .expect("paste update");
        if let AppState::Chat { command_input, .. } = &app.state {
            assert!(command_input.text().contains("[Pasted "));
        } else {
            panic!("expected chat state");
        }
    }

    #[tokio::test]
    async fn chat_paste_input_normalizes_crlf_for_small_payload() {
        let mut app = chat_app();
        app.update(Action::PasteInput("alpha\r\nbeta".to_string()))
            .await
            .expect("paste update");
        if let AppState::Chat { command_input, .. } = &app.state {
            assert_eq!(command_input.text(), "alpha\nbeta");
            assert!(!command_input.text().contains('\r'));
        } else {
            panic!("expected chat state");
        }
    }

    #[test]
    fn non_active_agent_followup_dispatches_on_completion() {
        let mut app = two_agent_app();
        let session = "s-test".to_string();
        let target = "A2".to_string();
        if let AppState::Chat { agents, .. } = &mut app.state {
            let a2 = agents
                .iter_mut()
                .find(|a| a.agent_id == target)
                .expect("A2 exists");
            a2.follow_up_queue.push_back("queued follow-up".to_string());
            a2.status = AgentStatus::Done;
        }

        app.maybe_dispatch_queued_for_agent(&session, &target);

        if let AppState::Chat {
            agents,
            active_agent_index,
            ..
        } = &app.state
        {
            assert_eq!(
                *active_agent_index, 0,
                "active agent should remain unchanged"
            );
            let a2 = agents
                .iter()
                .find(|a| a.agent_id == target)
                .expect("A2 exists");
            assert!(
                a2.follow_up_queue.is_empty(),
                "follow-up should be consumed"
            );
            assert!(
                a2.messages.iter().any(|m| {
                    matches!(m.role, MessageRole::User)
                        && m.content
                            .iter()
                            .any(|b| matches!(b, ContentBlock::Text(t) if t == "queued follow-up"))
                }),
                "queued message should be appended to non-active agent transcript"
            );
        } else {
            panic!("expected chat state");
        }
    }

    #[test]
    fn steering_dispatch_clears_followups_and_wins_priority() {
        let mut app = two_agent_app();
        let session = "s-test".to_string();
        let target = "A2".to_string();
        if let AppState::Chat { agents, .. } = &mut app.state {
            let a2 = agents
                .iter_mut()
                .find(|a| a.agent_id == target)
                .expect("A2 exists");
            a2.follow_up_queue.push_back("followup-1".to_string());
            a2.follow_up_queue.push_back("followup-2".to_string());
            a2.steering_message = Some("steer-now".to_string());
            a2.status = AgentStatus::Done;
        }

        app.maybe_dispatch_queued_for_agent(&session, &target);

        if let AppState::Chat { agents, .. } = &app.state {
            let a2 = agents
                .iter()
                .find(|a| a.agent_id == target)
                .expect("A2 exists");
            assert!(
                a2.follow_up_queue.is_empty(),
                "steering should clear queued follow-ups"
            );
            assert!(a2.steering_message.is_none());
            assert!(
                a2.messages.iter().any(|m| {
                    matches!(m.role, MessageRole::User)
                        && m.content
                            .iter()
                            .any(|b| matches!(b, ContentBlock::Text(t) if t == "steer-now"))
                }),
                "steering message should be dispatched first"
            );
        } else {
            panic!("expected chat state");
        }
    }

    #[test]
    fn recipient_normalization_supports_agent_aliases() {
        assert_eq!(
            App::normalize_recipient_agent_id("A2").as_deref(),
            Some("A2")
        );
        assert_eq!(
            App::normalize_recipient_agent_id("a9").as_deref(),
            Some("A9")
        );
        assert_eq!(
            App::normalize_recipient_agent_id("agent-3").as_deref(),
            Some("A3")
        );
        assert_eq!(App::normalize_recipient_agent_id("agent-x"), None);
    }

    #[test]
    fn resolve_recipient_prefers_exact_match_then_normalized_alias() {
        let agents = vec![
            App::make_agent_pane("A1".to_string(), "s1".to_string()),
            App::make_agent_pane("A2".to_string(), "s2".to_string()),
            App::make_agent_pane("A3".to_string(), "s3".to_string()),
        ];

        let direct = App::resolve_agent_target_for_recipient(&agents, "A2");
        assert_eq!(direct, Some(("s2".to_string(), "A2".to_string())));

        let alias = App::resolve_agent_target_for_recipient(&agents, "agent-3");
        assert_eq!(alias, Some(("s3".to_string(), "A3".to_string())));
    }

    #[test]
    fn member_name_match_accepts_normalized_aliases() {
        assert!(App::member_name_matches_recipient("A2", "a2"));
        assert!(App::member_name_matches_recipient("A2", "agent-2"));
        assert!(App::member_name_matches_recipient("agent-3", "A3"));
        assert!(!App::member_name_matches_recipient("A2", "A4"));
    }

    #[test]
    fn render_plan_feedback_wizard_includes_guidance_text() {
        let mut app = chat_app();
        app.test_mode = true;
        if let AppState::Chat {
            modal, plan_wizard, ..
        } = &mut app.state
        {
            *modal = Some(ModalState::PlanFeedbackWizard);
            plan_wizard.task_preview = vec![
                "Draft milestones".to_string(),
                "Define acceptance checks".to_string(),
            ];
        }
        let rendered = render_to_text(&app);
        assert!(rendered.contains("Guided feedback for newly proposed plan tasks"));
        assert!(rendered.contains("Task preview:"));
        assert!(rendered.contains("Plan name (optional):"));
    }

    #[test]
    fn render_request_center_question_shows_prompt_and_keys() {
        let mut app = chat_app();
        app.test_mode = true;
        if let AppState::Chat {
            modal,
            pending_requests,
            ..
        } = &mut app.state
        {
            *modal = Some(ModalState::RequestCenter);
            pending_requests.push(PendingRequest {
                session_id: "s-test".to_string(),
                agent_id: "A1".to_string(),
                kind: PendingRequestKind::Question(PendingQuestionRequest {
                    id: "q-1".to_string(),
                    questions: vec![QuestionDraft {
                        header: "Approval".to_string(),
                        question: "Proceed with plan execution?".to_string(),
                        options: vec![
                            crate::net::client::QuestionChoice {
                                label: "Yes".to_string(),
                                description: "Continue".to_string(),
                            },
                            crate::net::client::QuestionChoice {
                                label: "No".to_string(),
                                description: "Revise first".to_string(),
                            },
                        ],
                        multiple: false,
                        custom: true,
                        selected_options: vec![],
                        custom_input: String::new(),
                        option_cursor: 0,
                    }],
                    question_index: 0,
                    permission_request_id: None,
                }),
            });
        }
        let rendered = render_to_text(&app);
        assert!(rendered.contains("AI asks: Proceed with plan execution?"));
        assert!(rendered.contains("Choices:"));
        assert!(rendered.contains("1. Yes Continue"));
        assert!(rendered.contains("Answer:"));
    }

    #[tokio::test]
    async fn plan_mode_prompt_todo_updated_opens_wizard_and_sets_approval_guard() {
        let mut app = chat_app();
        app.current_mode = TandemMode::Plan;

        app.update(Action::PromptTodoUpdated {
            session_id: "s-test".to_string(),
            todos: vec![serde_json::json!({
                "content": "Create architecture draft",
                "status": "pending"
            })],
        })
        .await
        .expect("todo update");

        if let AppState::Chat {
            modal,
            plan_wizard,
            plan_awaiting_approval,
            tasks,
            ..
        } = &app.state
        {
            assert!(matches!(modal, Some(ModalState::PlanFeedbackWizard)));
            assert!(*plan_awaiting_approval);
            assert_eq!(tasks.len(), 1);
            assert_eq!(
                plan_wizard.task_preview,
                vec!["Create architecture draft".to_string()]
            );
        } else {
            panic!("expected chat state");
        }
    }

    #[tokio::test]
    async fn plan_mode_duplicate_all_pending_todo_update_is_ignored_while_awaiting_approval() {
        let mut app = chat_app();
        app.current_mode = TandemMode::Plan;

        let todos = vec![serde_json::json!({
            "content": "Draft implementation checklist",
            "status": "pending"
        })];

        app.update(Action::PromptTodoUpdated {
            session_id: "s-test".to_string(),
            todos: todos.clone(),
        })
        .await
        .expect("first todo update");

        let (messages_before, preview_before) = if let AppState::Chat {
            messages,
            plan_wizard,
            ..
        } = &app.state
        {
            (messages.len(), plan_wizard.task_preview.clone())
        } else {
            panic!("expected chat state");
        };

        app.update(Action::PromptTodoUpdated {
            session_id: "s-test".to_string(),
            todos,
        })
        .await
        .expect("second todo update");

        if let AppState::Chat {
            messages,
            plan_wizard,
            ..
        } = &app.state
        {
            assert_eq!(
                messages.len(),
                messages_before,
                "guarded duplicate update should not append new system notes"
            );
            assert_eq!(
                plan_wizard.task_preview, preview_before,
                "guarded duplicate update should not mutate plan preview"
            );
        } else {
            panic!("expected chat state");
        }
    }

    #[tokio::test]
    async fn malformed_question_retry_prompt_is_dispatched_once_per_request_id() {
        let mut app = chat_app();

        if let AppState::Chat {
            pending_requests, ..
        } = &mut app.state
        {
            pending_requests.push(PendingRequest {
                session_id: "s-test".to_string(),
                agent_id: "A1".to_string(),
                kind: PendingRequestKind::Question(PendingQuestionRequest {
                    id: "req-1".to_string(),
                    questions: vec![QuestionDraft {
                        header: "Question".to_string(),
                        question: "Choose one".to_string(),
                        options: vec![],
                        multiple: false,
                        custom: true,
                        selected_options: vec![],
                        custom_input: String::new(),
                        option_cursor: 0,
                    }],
                    question_index: 0,
                    permission_request_id: None,
                }),
            });
        } else {
            panic!("expected chat state");
        }

        for _ in 0..2 {
            app.update(Action::PromptMalformedQuestion {
                session_id: "s-test".to_string(),
                agent_id: "A1".to_string(),
                request_id: "req-1".to_string(),
            })
            .await
            .expect("malformed question handling");
        }

        if let AppState::Chat {
            pending_requests,
            messages,
            ..
        } = &app.state
        {
            assert!(
                pending_requests.is_empty(),
                "malformed request should be removed from queue"
            );
            let retry_prompt_count = messages
                .iter()
                .filter(|m| matches!(m.role, MessageRole::User))
                .flat_map(|m| m.content.iter())
                .filter(|block| {
                    matches!(
                        block,
                        ContentBlock::Text(t)
                            if t.contains(
                                "Your last `question` tool call had invalid or empty arguments."
                            )
                    )
                })
                .count();
            assert_eq!(
                retry_prompt_count, 1,
                "retry guidance prompt should be dispatched only once per malformed request id"
            );
        } else {
            panic!("expected chat state");
        }
    }
}

use crate::net::client::Session;

#[derive(Debug, Clone, PartialEq)]
pub enum PinPromptMode {
    UnlockExisting,
    CreateNew,
    ConfirmNew { first_pin: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum EngineConnectionStatus {
    Disconnected,
    Connecting,
    Connected,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiMode {
    Focus,
    Grid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Running,
    Streaming,
    Cancelling,
    Done,
    Error,
    Closed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModalState {
    Help,
    ConfirmCloseAgent { target_agent_id: String },
    RequestCenter,
    PlanFeedbackWizard,
    StartPlanAgents { count: usize },
    FileSearch,
    Pager,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PagerOverlayState {
    pub title: String,
    pub lines: Vec<String>,
    pub scroll: usize,
    pub is_diff: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileSearchState {
    pub query: String,
    pub matches: Vec<String>,
    pub cursor: usize,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PlanFeedbackWizardState {
    pub plan_name: String,
    pub scope: String,
    pub constraints: String,
    pub priorities: String,
    pub notes: String,
    pub cursor_step: usize,
    pub source_request_id: Option<String>,
    pub task_preview: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QuestionDraft {
    pub header: String,
    pub question: String,
    pub options: Vec<crate::net::client::QuestionChoice>,
    pub multiple: bool,
    pub custom: bool,
    pub selected_options: Vec<usize>,
    pub custom_input: String,
    pub option_cursor: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingQuestionRequest {
    pub id: String,
    pub questions: Vec<QuestionDraft>,
    pub question_index: usize,
    pub permission_request_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingPermissionRequest {
    pub id: String,
    pub tool: String,
    pub args: Option<Value>,
    pub args_source: Option<String>,
    pub args_integrity: Option<String>,
    pub query: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PendingRequestKind {
    Permission(PendingPermissionRequest),
    Question(PendingQuestionRequest),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingRequest {
    pub session_id: String,
    pub agent_id: String,
    pub kind: PendingRequestKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentPane {
    pub agent_id: String,
    pub session_id: String,
    pub draft: ComposerInputState,
    pub stream_collector: Option<crate::ui::markdown_stream::MarkdownStreamCollector>,
    pub messages: Vec<ChatMessage>,
    pub scroll_from_bottom: u16,
    pub tasks: Vec<Task>,
    pub active_task_id: Option<String>,
    pub status: AgentStatus,
    pub active_run_id: Option<String>,
    pub bound_context_run_id: Option<String>,
    pub follow_up_queue: VecDeque<String>,
    pub steering_message: Option<String>,
    pub paste_registry: HashMap<u32, String>,
    pub next_paste_id: u32,
    pub live_tool_calls: HashMap<String, LiveToolCall>,
    pub exploration_batch: Option<crate::activity::ExplorationBatch>,
    pub live_activity_message: Option<String>,
    pub delegated_worker: bool,
    pub delegated_team_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LiveToolCall {
    pub tool_name: String,
    pub args_preview: String,
}
pub use crate::activity::{ActivitySummary, ActivityTone};

#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    StartupAnimation {
        frame: usize,
    },

    PinPrompt {
        input: String,
        error: Option<String>,
        mode: PinPromptMode,
    },
    MainMenu,
    Chat {
        session_id: String,
        command_input: ComposerInputState,
        messages: Vec<ChatMessage>,
        scroll_from_bottom: u16,
        tasks: Vec<Task>,
        active_task_id: Option<String>,
        agents: Vec<AgentPane>,
        active_agent_index: usize,
        ui_mode: UiMode,
        grid_page: usize,
        modal: Option<ModalState>,
        pending_requests: Vec<PendingRequest>,
        request_cursor: usize,
        permission_choice: usize,
        plan_wizard: PlanFeedbackWizardState,
        last_plan_task_fingerprint: Vec<String>,
        plan_awaiting_approval: bool,
        plan_multi_agent_prompt: Option<usize>,
        plan_waiting_for_clarification_question: bool,
        request_panel_expanded: bool,
    },
    Connecting,
    SetupWizard {
        step: SetupStep,
        provider_catalog: Option<crate::net::client::ProviderCatalog>,
        selected_provider_index: usize,
        selected_model_index: usize,
        api_key_input: String,
        model_input: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SetupStep {
    Welcome,
    SelectProvider,
    EnterApiKey,
    SelectModel,
    Complete,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ContentBlock {
    Text(String),
    Code { language: String, code: String },
    ToolCall(ToolCallInfo),
    ToolResult(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallInfo {
    pub id: String,
    pub name: String,
    pub args: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub id: String,
    pub description: String,
    pub status: TaskStatus,
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TaskStatus {
    Pending,
    Working,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutocompleteMode {
    Command,
    Provider,
    Model,
}

use crate::command_catalog::COMMAND_HELP;
use crate::net::client::EngineClient;
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::env;
use tandem_wire::WireSessionMessage;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::time::timeout;

use crate::crypto::{
    keystore::SecureKeyStore,
    vault::{EncryptedVaultKey, MAX_PIN_LENGTH},
};
use anyhow::anyhow;
use std::fs;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command as StdCommand, Stdio};
use std::time::Instant;
use tandem_core::{
    load_or_create_engine_api_token, migrate_legacy_storage_if_needed, resolve_shared_paths,
    DEFAULT_ENGINE_HOST, DEFAULT_ENGINE_PORT,
};

pub struct App {
    pub state: AppState,
    pub matrix: crate::ui::matrix::MatrixEffect,
    pub should_quit: bool,
    pub test_mode: bool,
    pub tick_count: usize,
    pub config_dir: Option<PathBuf>,
    pub vault_key: Option<EncryptedVaultKey>,
    pub keystore: Option<SecureKeyStore>,
    pub engine_process: Option<Child>,
    pub engine_binary_path: Option<PathBuf>,
    pub engine_download_retry_at: Option<Instant>,
    pub engine_download_last_error: Option<String>,
    pub engine_download_total_bytes: Option<u64>,
    pub engine_downloaded_bytes: u64,
    pub engine_download_active: bool,
    pub engine_download_phase: Option<String>,
    pub startup_engine_bootstrap_done: bool,
    pub client: Option<EngineClient>,
    pub sessions: Vec<Session>,
    pub selected_session_index: usize,
    pub current_mode: TandemMode,
    pub current_provider: Option<String>,
    pub current_model: Option<String>,
    pub provider_catalog: Option<crate::net::client::ProviderCatalog>,
    pub connection_status: String,
    pub engine_health: EngineConnectionStatus,
    pub engine_lease_id: Option<String>,
    pub engine_lease_last_renewed: Option<Instant>,
    pub engine_api_token: Option<String>,
    pub engine_api_token_backend: Option<String>,
    pub engine_base_url_override: Option<String>,
    pub engine_connection_source: EngineConnectionSource,
    pub engine_spawned_at: Option<Instant>,
    pub local_engine_build_attempted: bool,
    pub pending_model_provider: Option<String>,
    pub recent_commands: VecDeque<String>,
    pub autocomplete_items: Vec<(String, String)>,
    pub autocomplete_index: usize,
    pub autocomplete_mode: AutocompleteMode,
    pub show_autocomplete: bool,
    pub action_tx: Option<tokio::sync::mpsc::UnboundedSender<Action>>,
    pub quit_armed_at: Option<Instant>,
    pub paste_activity_until: Option<Instant>,
    pub malformed_question_retries: HashSet<String>,
    pub pager_overlay: Option<PagerOverlayState>,
    pub file_search: FileSearchState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineStalePolicy {
    AutoReplace,
    Fail,
    Warn,
}

impl EngineStalePolicy {
    fn from_env() -> Self {
        match std::env::var("TANDEM_ENGINE_STALE_POLICY")
            .ok()
            .map(|v| v.trim().to_ascii_lowercase())
            .as_deref()
        {
            Some("fail") => Self::Fail,
            Some("warn") => Self::Warn,
            _ => Self::AutoReplace,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Self::AutoReplace => "auto_replace",
            Self::Fail => "fail",
            Self::Warn => "warn",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineConnectionSource {
    Unknown,
    SharedAttached,
    ManagedLocal,
}

impl EngineConnectionSource {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::SharedAttached => "shared-attached",
            Self::ManagedLocal => "managed-local",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum TandemMode {
    #[default]
    Plan,
    Coder,
    Explore,
    Immediate,
    Orchestrate,
    Ask,
}

const SCROLL_LINE_STEP: u16 = 3;
const SCROLL_PAGE_STEP: u16 = 20;
const MAX_RECENT_COMMANDS: usize = 8;
const MIN_ENGINE_BINARY_SIZE: u64 = 100 * 1024;
const ENGINE_REPO: &str = "frumu-ai/tandem";
const GITHUB_API: &str = "https://api.github.com";

#[derive(Debug, Deserialize, Clone)]
struct GitHubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<GitHubAsset>,
}

#[derive(Debug, Deserialize, Clone)]
struct GitHubAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

impl TandemMode {
    pub fn as_agent(&self) -> &'static str {
        match self {
            TandemMode::Ask => "general",
            TandemMode::Coder => "build",
            TandemMode::Explore => "explore",
            TandemMode::Immediate => "immediate",
            TandemMode::Orchestrate => "orchestrate",
            TandemMode::Plan => "plan",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "ask" => Some(TandemMode::Ask),
            "coder" => Some(TandemMode::Coder),
            "explore" => Some(TandemMode::Explore),
            "immediate" => Some(TandemMode::Immediate),
            "orchestrate" => Some(TandemMode::Orchestrate),
            "plan" => Some(TandemMode::Plan),
            _ => None,
        }
    }

    pub fn all_modes() -> Vec<(&'static str, &'static str)> {
        vec![
            (
                "plan",
                "Planning mode with write restrictions - uses plan agent",
            ),
            (
                "immediate",
                "Execute without confirmation - uses immediate agent",
            ),
            ("coder", "Code assistance - uses build agent"),
            ("ask", "General Q&A - uses general agent"),
            ("explore", "Read-only exploration - uses explore agent"),
            (
                "orchestrate",
                "Multi-agent orchestration - uses orchestrate agent",
            ),
        ]
    }

    pub fn next(&self) -> Self {
        match self {
            TandemMode::Plan => TandemMode::Immediate,
            TandemMode::Immediate => TandemMode::Coder,
            TandemMode::Coder => TandemMode::Ask,
            TandemMode::Ask => TandemMode::Explore,
            TandemMode::Explore => TandemMode::Orchestrate,
            TandemMode::Orchestrate => TandemMode::Plan,
        }
    }
}

impl App {
    fn test_mode_enabled() -> bool {
        std::env::var("TANDEM_TUI_TEST_MODE")
            .ok()
            .map(|v| {
                let normalized = v.trim().to_ascii_lowercase();
                !(normalized.is_empty()
                    || normalized == "0"
                    || normalized == "false"
                    || normalized == "off")
            })
            .unwrap_or(false)
    }

    fn test_skip_engine_enabled() -> bool {
        std::env::var("TANDEM_TUI_TEST_SKIP_ENGINE")
            .ok()
            .map(|v| {
                let normalized = v.trim().to_ascii_lowercase();
                !(normalized.is_empty()
                    || normalized == "0"
                    || normalized == "false"
                    || normalized == "off")
            })
            .unwrap_or(false)
    }

    fn is_paste_shortcut(key: &KeyEvent) -> bool {
        let is_ctrl_v = matches!(key.code, KeyCode::Char('v') | KeyCode::Char('V'))
            && key.modifiers.contains(KeyModifiers::CONTROL);
        let is_shift_insert =
            matches!(key.code, KeyCode::Insert) && key.modifiers.contains(KeyModifiers::SHIFT);
        is_ctrl_v || is_shift_insert
    }

    fn sanitize_provider_catalog(
        mut catalog: crate::net::client::ProviderCatalog,
    ) -> crate::net::client::ProviderCatalog {
        catalog.all.retain(|p| p.id != "local");
        catalog.connected.retain(|id| id != "local");
        catalog
    }

    fn provider_is_connected(&self, provider_id: &str) -> bool {
        self.provider_catalog
            .as_ref()
            .map(|c| c.connected.iter().any(|p| p == provider_id))
            .unwrap_or(false)
    }

    fn record_recent_command(&mut self, cmd: &str) {
        let normalized = cmd.trim();
        if normalized.is_empty()
            || !normalized.starts_with('/')
            || normalized.starts_with("/recent")
        {
            return;
        }
        if let Some(existing) = self
            .recent_commands
            .iter()
            .position(|row| row == normalized)
        {
            self.recent_commands.remove(existing);
        }
        self.recent_commands.push_front(normalized.to_string());
        self.recent_commands.truncate(MAX_RECENT_COMMANDS);
    }

    fn recent_commands_snapshot(&self) -> Vec<String> {
        self.recent_commands.iter().cloned().collect()
    }

    fn clear_recent_commands(&mut self) -> usize {
        let cleared = self.recent_commands.len();
        self.recent_commands.clear();
        cleared
    }

    fn open_key_wizard_for_provider(&mut self, provider_id: &str) -> bool {
        let mut selected_provider_index = 0usize;
        let mut found = false;
        if let Some(catalog) = &self.provider_catalog {
            if let Some(idx) = catalog.all.iter().position(|p| p.id == provider_id) {
                selected_provider_index = idx;
                found = true;
            }
        }
        if !found {
            return false;
        }
        self.state = AppState::SetupWizard {
            step: SetupStep::EnterApiKey,
            provider_catalog: self.provider_catalog.clone(),
            selected_provider_index,
            selected_model_index: 0,
            api_key_input: String::new(),
            model_input: String::new(),
        };
        true
    }

    async fn sync_keystore_keys_to_engine(&self, client: &EngineClient) -> usize {
        let Some(keystore) = &self.keystore else {
            return 0;
        };
        let mut synced = 0usize;
        for key_name in keystore.list_keys() {
            if let Ok(Some(api_key)) = keystore.get(&key_name) {
                if api_key.trim().is_empty() {
                    continue;
                }
                let provider_id = Self::normalize_provider_id_from_keystore_key(&key_name);
                if client.set_auth(&provider_id, &api_key).await.is_ok() {
                    synced += 1;
                }
            }
        }
        synced
    }

    fn normalize_provider_id_from_keystore_key(key: &str) -> String {
        let trimmed = key.trim();
        if let Some(rest) = trimmed.strip_prefix("opencode_") {
            if let Some(provider) = rest.strip_suffix("_api_key") {
                return provider.to_string();
            }
        }
        if let Some(provider) = trimmed.strip_suffix("_api_key") {
            return provider.to_string();
        }
        if let Some(provider) = trimmed.strip_suffix("_key") {
            return provider.to_string();
        }
        trimmed.to_string()
    }

    fn save_provider_key_local(&mut self, provider_id: &str, api_key: &str) {
        let Some(keystore) = &mut self.keystore else {
            return;
        };
        if keystore.set(provider_id, api_key.to_string()).is_ok() {
            if let Some(config_dir) = &self.config_dir {
                let _ = keystore.save(config_dir.join("tandem.keystore"));
            }
        }
    }

    fn shared_engine_mode_enabled() -> bool {
        std::env::var("TANDEM_SHARED_ENGINE_MODE")
            .ok()
            .map(|v| {
                let normalized = v.trim().to_ascii_lowercase();
                !(normalized == "0" || normalized == "false" || normalized == "off")
            })
            .unwrap_or(true)
    }

    fn configured_engine_port() -> u16 {
        std::env::var("TANDEM_ENGINE_PORT")
            .ok()
            .and_then(|raw| raw.trim().parse::<u16>().ok())
            .filter(|port| *port != 0)
            .unwrap_or(DEFAULT_ENGINE_PORT)
    }

    fn configured_engine_base_url() -> String {
        if let Ok(raw) = std::env::var("TANDEM_ENGINE_URL") {
            let trimmed = raw.trim().trim_end_matches('/');
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        format!(
            "http://{}:{}",
            DEFAULT_ENGINE_HOST,
            Self::configured_engine_port()
        )
    }

    fn engine_target_base_url(&self) -> String {
        self.engine_base_url_override
            .clone()
            .unwrap_or_else(Self::configured_engine_base_url)
    }

    fn engine_base_url_for_port(port: u16) -> String {
        format!("http://{}:{}", DEFAULT_ENGINE_HOST, port)
    }

    fn pick_spawn_port() -> u16 {
        let configured = Self::configured_engine_port();
        if TcpListener::bind((DEFAULT_ENGINE_HOST, configured)).is_ok() {
            return configured;
        }
        TcpListener::bind((DEFAULT_ENGINE_HOST, 0))
            .ok()
            .and_then(|listener| listener.local_addr().ok().map(|addr| addr.port()))
            .filter(|port| *port != 0)
            .unwrap_or(configured)
    }

    fn masked_engine_api_token(token: &str) -> String {
        let trimmed = token.trim();
        if trimmed.is_empty() || trimmed.len() <= 8 {
            return "****".to_string();
        }
        format!("{}****{}", &trimmed[..4], &trimmed[trimmed.len() - 4..])
    }

    fn resolve_engine_api_token() -> Option<(String, String)> {
        if let Ok(raw) = std::env::var("TANDEM_API_TOKEN") {
            let token = raw.trim();
            if !token.is_empty() {
                return Some((token.to_string(), "env".to_string()));
            }
        }
        let token_material = load_or_create_engine_api_token();
        Some((token_material.token, token_material.backend))
    }

    pub fn new() -> Self {
        let test_mode = Self::test_mode_enabled();
        let test_skip_engine = test_mode && Self::test_skip_engine_enabled();
        let config_dir = Self::find_or_create_config_dir();
        let (engine_api_token, engine_api_token_backend) = Self::resolve_engine_api_token()
            .map(|(token, backend)| (Some(token), Some(backend)))
            .unwrap_or((None, None));

        let vault_key = if let Some(dir) = &config_dir {
            let path = dir.join("vault.key");
            if path.exists() {
                EncryptedVaultKey::load(&path).ok()
            } else {
                None
            }
        } else {
            None
        };

        let test_session_id = "test-session".to_string();
        let test_agent = Self::make_agent_pane("A1".to_string(), test_session_id.clone());

        Self {
            state: if test_skip_engine {
                AppState::Chat {
                    session_id: test_session_id,
                    command_input: ComposerInputState::new(),
                    messages: vec![ChatMessage {
                        role: MessageRole::System,
                        content: vec![ContentBlock::Text(
                            "Test mode active: engine bootstrap skipped.".to_string(),
                        )],
                    }],
                    scroll_from_bottom: 0,
                    tasks: Vec::new(),
                    active_task_id: None,
                    agents: vec![test_agent],
                    active_agent_index: 0,
                    ui_mode: UiMode::Focus,
                    grid_page: 0,
                    modal: None,
                    pending_requests: Vec::new(),
                    request_cursor: 0,
                    permission_choice: 0,
                    plan_wizard: PlanFeedbackWizardState::default(),
                    last_plan_task_fingerprint: Vec::new(),
                    plan_awaiting_approval: false,
                    plan_multi_agent_prompt: None,
                    plan_waiting_for_clarification_question: false,
                    request_panel_expanded: false,
                }
            } else if test_mode {
                AppState::Connecting
            } else {
                AppState::StartupAnimation { frame: 0 }
            },
            matrix: crate::ui::matrix::MatrixEffect::new(0, 0),

            should_quit: false,
            test_mode,
            tick_count: 0,
            config_dir,
            vault_key,
            keystore: None,
            engine_process: None,
            engine_binary_path: None,
            engine_download_retry_at: None,
            engine_download_last_error: None,
            engine_download_total_bytes: None,
            engine_downloaded_bytes: 0,
            engine_download_active: false,
            engine_download_phase: None,
            startup_engine_bootstrap_done: test_mode,
            client: None,
            sessions: Vec::new(),
            selected_session_index: 0,
            current_mode: TandemMode::default(),
            current_provider: None,
            current_model: None,
            provider_catalog: None,
            connection_status: if test_skip_engine {
                "Test mode: engine skipped.".to_string()
            } else if test_mode {
                "Test mode: deterministic UI enabled.".to_string()
            } else {
                "Initializing...".to_string()
            },
            engine_health: EngineConnectionStatus::Disconnected,
            engine_lease_id: None,
            engine_lease_last_renewed: None,
            engine_api_token,
            engine_api_token_backend,
            engine_base_url_override: None,
            engine_connection_source: EngineConnectionSource::Unknown,
            engine_spawned_at: None,
            local_engine_build_attempted: false,
            pending_model_provider: None,
            recent_commands: VecDeque::new(),
            autocomplete_items: Vec::new(),
            autocomplete_index: 0,
            autocomplete_mode: AutocompleteMode::Command,
            show_autocomplete: false,
            action_tx: None,
            quit_armed_at: None,
            paste_activity_until: None,
            malformed_question_retries: HashSet::new(),
            pager_overlay: None,
            file_search: FileSearchState::default(),
        }
    }

    fn make_agent_pane(agent_id: String, session_id: String) -> AgentPane {
        AgentPane {
            agent_id,
            session_id,
            draft: ComposerInputState::new(),
            stream_collector: None,
            messages: Vec::new(),
            scroll_from_bottom: 0,
            tasks: Vec::new(),
            active_task_id: None,
            status: AgentStatus::Idle,
            active_run_id: None,
            bound_context_run_id: None,
            follow_up_queue: VecDeque::new(),
            steering_message: None,
            paste_registry: HashMap::new(),
            next_paste_id: 1,
            live_tool_calls: HashMap::new(),
            exploration_batch: None,
            live_activity_message: None,
            delegated_worker: false,
            delegated_team_name: None,
        }
    }

    async fn finalize_connecting(&mut self, client: &EngineClient) -> bool {
        if self.engine_lease_id.is_none() {
            self.acquire_engine_lease().await;
            let synced = self.sync_keystore_keys_to_engine(client).await;
            if synced > 0 {
                self.connection_status = format!("Synced {} provider key(s)...", synced);
            }
        }

        let providers = match client.list_providers().await {
            Ok(providers) => {
                let providers = Self::sanitize_provider_catalog(providers);
                self.provider_catalog = Some(providers.clone());
                providers
            }
            Err(_err) => {
                self.connection_status = "Connected. Loading providers...".to_string();
                return false;
            }
        };

        let needs_first_key_setup = self
            .keystore
            .as_ref()
            .map(|keystore| keystore.list_keys().is_empty())
            .unwrap_or(false);

        if providers.connected.is_empty() || needs_first_key_setup {
            self.state = AppState::SetupWizard {
                step: SetupStep::Welcome,
                provider_catalog: Some(providers),
                selected_provider_index: 0,
                selected_model_index: 0,
                api_key_input: String::new(),
                model_input: String::new(),
            };
            return true;
        }

        let config = client.config_providers().await.ok();
        self.apply_provider_defaults(config.as_ref());

        match client.list_sessions().await {
            Ok(sessions) => {
                self.sessions = sessions;
                self.connection_status = "Engine ready. Loading sessions...".to_string();
                self.state = AppState::MainMenu;
                true
            }
            Err(_err) => {
                self.connection_status = "Connected. Loading sessions...".to_string();
                false
            }
        }
    }

    async fn cancel_agent_if_running(&mut self, agent_index: usize) {
        let (session_id, run_id) = if let AppState::Chat { agents, .. } = &self.state {
            if let Some(agent) = agents.get(agent_index) {
                (agent.session_id.clone(), agent.active_run_id.clone())
            } else {
                return;
            }
        } else {
            return;
        };

        if let Some(client) = &self.client {
            if let Some(run_id) = run_id.as_deref() {
                let _ = client.cancel_run_by_id(&session_id, run_id).await;
            } else {
                let _ = client.abort_session(&session_id).await;
            }
        }
    }

    fn update_autocomplete_for_input(&mut self, input: &str) {
        if !input.starts_with('/') {
            self.show_autocomplete = false;
            self.autocomplete_items.clear();
            return;
        }
        if let Some(rest) = input.strip_prefix("/provider") {
            let query = rest.trim_start().to_lowercase();
            if let Some(catalog) = &self.provider_catalog {
                let mut providers: Vec<String> = catalog.all.iter().map(|p| p.id.clone()).collect();
                providers.sort();
                let filtered: Vec<String> = if query.is_empty() {
                    providers
                } else {
                    providers
                        .into_iter()
                        .filter(|p| p.to_lowercase().contains(&query))
                        .collect()
                };
                self.autocomplete_items = filtered
                    .into_iter()
                    .map(|p| (p, "provider".to_string()))
                    .collect();
                self.autocomplete_index = 0;
                self.autocomplete_mode = AutocompleteMode::Provider;
                self.show_autocomplete = !self.autocomplete_items.is_empty();
                return;
            }
        }
        if let Some(rest) = input.strip_prefix("/model") {
            let query = rest.trim_start().to_lowercase();
            if let Some(catalog) = &self.provider_catalog {
                let provider_id = self.current_provider.as_deref().unwrap_or("");
                if let Some(provider) = catalog.all.iter().find(|p| p.id == provider_id) {
                    let mut model_ids: Vec<String> = provider.models.keys().cloned().collect();
                    model_ids.sort();
                    let filtered: Vec<String> = if query.is_empty() {
                        model_ids
                    } else {
                        model_ids
                            .into_iter()
                            .filter(|m| m.to_lowercase().contains(&query))
                            .collect()
                    };
                    self.autocomplete_items = filtered
                        .into_iter()
                        .map(|m| (m, "model".to_string()))
                        .collect();
                    self.autocomplete_index = 0;
                    self.autocomplete_mode = AutocompleteMode::Model;
                    self.show_autocomplete = !self.autocomplete_items.is_empty();
                    return;
                }
            }
        }
        let cmd_part = input.trim_start_matches('/').to_lowercase();
        self.autocomplete_items = COMMAND_HELP
            .iter()
            .filter(|(name, _)| name.starts_with(&cmd_part))
            .map(|(name, desc)| (name.to_string(), desc.to_string()))
            .collect();
        self.autocomplete_index = 0;
        self.autocomplete_mode = AutocompleteMode::Command;
        self.show_autocomplete = !self.autocomplete_items.is_empty();
    }

    fn model_ids_for_provider(
        provider_catalog: &crate::net::client::ProviderCatalog,
        provider_index: usize,
    ) -> Vec<String> {
        if provider_index >= provider_catalog.all.len() {
            return Vec::new();
        }
        let provider = &provider_catalog.all[provider_index];
        let mut model_ids: Vec<String> = provider.models.keys().cloned().collect();
        model_ids.sort();
        model_ids
    }

    fn filtered_model_ids(
        provider_catalog: &crate::net::client::ProviderCatalog,
        provider_index: usize,
        model_input: &str,
    ) -> Vec<String> {
        let model_ids = Self::model_ids_for_provider(provider_catalog, provider_index);
        if model_input.trim().is_empty() {
            return model_ids;
        }
        let query = model_input.trim().to_lowercase();
        model_ids
            .into_iter()
            .filter(|m| m.to_lowercase().contains(&query))
            .collect()
    }

    fn find_or_create_config_dir() -> Option<PathBuf> {
        if let Ok(paths) = resolve_shared_paths() {
            let _ = std::fs::create_dir_all(&paths.canonical_root);
            if let Ok(report) = migrate_legacy_storage_if_needed(&paths) {
                tracing::info!(
                    "TUI storage migration status: reason={} performed={} copied={} skipped={} errors={}",
                    report.reason,
                    report.performed,
                    report.copied.len(),
                    report.skipped.len(),
                    report.errors.len()
                );
            }
            return Some(paths.canonical_root);
        }
        None
    }

    fn engine_binary_name() -> &'static str {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        return "tandem-engine.exe";

        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        return "tandem-engine";

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        return "tandem-engine";

        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        return "tandem-engine";

        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        return "tandem-engine";
    }

    fn engine_asset_name() -> &'static str {
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        return "tandem-engine-windows-x64.zip";

        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        return "tandem-engine-darwin-x64.zip";

        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        return "tandem-engine-darwin-arm64.zip";

        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        return "tandem-engine-linux-x64.tar.gz";

        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        return "tandem-engine-linux-arm64.tar.gz";
    }

    fn engine_asset_matches(asset_name: &str) -> bool {
        if !asset_name.starts_with("tandem-engine-") {
            return false;
        }
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            return asset_name.contains("windows") && asset_name.contains("x64");
        }
        #[cfg(all(target_os = "macos", target_arch = "x86_64"))]
        {
            return asset_name.contains("darwin") && asset_name.contains("x64");
        }
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            return asset_name.contains("darwin") && asset_name.contains("arm64");
        }
        #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
        {
            return asset_name.contains("linux") && asset_name.contains("x64");
        }
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        {
            return asset_name.contains("linux") && asset_name.contains("arm64");
        }
    }

    fn shared_binaries_dir() -> Option<PathBuf> {
        resolve_shared_paths()
            .ok()
            .map(|paths| paths.canonical_root.join("binaries"))
            .or_else(|| {
                #[cfg(target_os = "windows")]
                {
                    std::env::var_os("APPDATA")
                        .map(PathBuf::from)
                        .map(|d| d.join("tandem").join("binaries"))
                }
                #[cfg(not(target_os = "windows"))]
                {
                    std::env::var_os("XDG_DATA_HOME")
                        .map(PathBuf::from)
                        .or_else(|| {
                            std::env::var_os("HOME")
                                .map(PathBuf::from)
                                .map(|h| h.join(".local").join("share"))
                        })
                        .map(|d| d.join("tandem").join("binaries"))
                }
            })
    }

    fn find_desktop_bundled_engine_binary() -> Option<PathBuf> {
        let binary_name = Self::engine_binary_name();
        let mut candidates: Vec<PathBuf> = Vec::new();

        #[cfg(target_os = "windows")]
        {
            let mut roots: Vec<PathBuf> = Vec::new();
            if let Some(v) = std::env::var_os("ProgramFiles").map(PathBuf::from) {
                roots.push(v);
            }
            if let Some(v) = std::env::var_os("ProgramW6432").map(PathBuf::from) {
                if !roots.contains(&v) {
                    roots.push(v);
                }
            }
            if let Some(v) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
                roots.push(v.join("Programs"));
            }

            for root in roots {
                let app_dir = root.join("Tandem");
                candidates.push(app_dir.join("binaries").join(binary_name));
                candidates.push(app_dir.join("resources").join("binaries").join(binary_name));
                candidates.push(
                    app_dir
                        .join("resources")
                        .join("resources")
                        .join("binaries")
                        .join(binary_name),
                );
            }
        }

        #[cfg(target_os = "macos")]
        {
            let app_dir = PathBuf::from("/Applications/Tandem.app")
                .join("Contents")
                .join("Resources");
            candidates.push(app_dir.join("binaries").join(binary_name));
            candidates.push(app_dir.join("resources").join("binaries").join(binary_name));
        }

        #[cfg(target_os = "linux")]
        {
            let roots = [
                PathBuf::from("/opt/tandem"),
                PathBuf::from("/usr/lib/tandem"),
            ];
            for root in roots {
                candidates.push(root.join("binaries").join(binary_name));
                candidates.push(root.join("resources").join("binaries").join(binary_name));
                candidates.push(
                    root.join("resources")
                        .join("resources")
                        .join("binaries")
                        .join(binary_name),
                );
            }
        }

        for candidate in candidates {
            if candidate
                .metadata()
                .map(|m| m.len() >= MIN_ENGINE_BINARY_SIZE)
                .unwrap_or(false)
            {
                return Some(candidate);
            }
        }

        None
    }

    fn find_dev_engine_binary() -> Option<PathBuf> {
        let Ok(current_dir) = env::current_dir() else {
            return None;
        };
        let binary_name = Self::engine_binary_name();
        let candidates = [
            current_dir.join("target").join("debug").join(binary_name),
            current_dir
                .join("..")
                .join("target")
                .join("debug")
                .join(binary_name),
            current_dir
                .join("src-tauri")
                .join("..")
                .join("target")
                .join("debug")
                .join(binary_name),
            current_dir.join("binaries").join(binary_name),
            current_dir
                .join("src-tauri")
                .join("binaries")
                .join(binary_name),
        ];
        for candidate in candidates {
            if candidate.exists() {
                return Some(candidate);
            }
        }
        None
    }

    fn try_build_local_dev_engine_binary(&self) -> Option<PathBuf> {
        if !cfg!(debug_assertions) {
            return None;
        }
        let Ok(current_dir) = env::current_dir() else {
            return None;
        };
        let output = StdCommand::new("cargo")
            .arg("build")
            .arg("-p")
            .arg("tandem-ai")
            .current_dir(&current_dir)
            .output()
            .ok()?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let summary = stderr
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .unwrap_or("cargo build failed");
            tracing::warn!("TUI local engine rebuild failed: {}", summary);
            return None;
        }
        Self::find_dev_engine_binary().filter(|path| {
            path.metadata()
                .map(|m| m.len() >= MIN_ENGINE_BINARY_SIZE)
                .unwrap_or(false)
        })
    }

    fn find_extracted_binary(dir: &std::path::Path, binary_name: &str) -> anyhow::Result<PathBuf> {
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                if let Ok(found) = Self::find_extracted_binary(&path, binary_name) {
                    return Ok(found);
                }
            } else if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.eq_ignore_ascii_case(binary_name) {
                    return Ok(path);
                }
            }
        }
        Err(anyhow!("Extracted engine binary not found"))
    }

    fn parse_semver_triplet(raw: &str) -> Option<(u64, u64, u64)> {
        let token = raw
            .split_whitespace()
            .find(|part| part.chars().filter(|c| *c == '.').count() >= 2)?;
        let core = token.trim_start_matches('v');
        let mut parts = core.split('.');
        let major = parts.next()?.parse::<u64>().ok()?;
        let minor = parts.next()?.parse::<u64>().ok()?;
        let patch_str = parts.next()?;
        let patch_digits = patch_str
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect::<String>();
        if patch_digits.is_empty() {
            return None;
        }
        let patch = patch_digits.parse::<u64>().ok()?;
        Some((major, minor, patch))
    }

    fn format_semver_triplet(version: (u64, u64, u64)) -> String {
        format!("{}.{}.{}", version.0, version.1, version.2)
    }

    fn parse_capability_csv(raw: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for part in raw.split(',') {
            let item = part.trim();
            if item.is_empty() {
                continue;
            }
            if seen.insert(item.to_string()) {
                out.push(item.to_string());
            }
        }
        out
    }

    fn parse_required_optional_segments(raw: &str) -> (Vec<String>, Vec<String>) {
        let mut required = Vec::new();
        let mut optional = Vec::new();
        for segment in raw.split("::") {
            let trimmed = segment.trim();
            if let Some(value) = trimmed.strip_prefix("required=") {
                required = Self::parse_capability_csv(value);
            } else if let Some(value) = trimmed.strip_prefix("optional=") {
                optional = Self::parse_capability_csv(value);
            } else {
                for token in trimmed.split_whitespace() {
                    if let Some(value) = token.strip_prefix("required=") {
                        required = Self::parse_capability_csv(value);
                    } else if let Some(value) = token.strip_prefix("optional=") {
                        optional = Self::parse_capability_csv(value);
                    }
                }
            }
        }
        optional.retain(|cap| !required.iter().any(|req| req == cap));
        (required, optional)
    }

    fn caps_from_value(value: Option<&Value>) -> Vec<String> {
        match value {
            Some(Value::Array(items)) => {
                let joined = items
                    .iter()
                    .filter_map(|item| item.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                Self::parse_capability_csv(&joined)
            }
            Some(Value::String(text)) => Self::parse_capability_csv(text),
            _ => Vec::new(),
        }
    }

    fn normalize_automation_tasks(tasks: &Value) -> Result<Vec<Value>, String> {
        let Some(items) = tasks.as_array() else {
            return Err("tasks JSON must be an array of objects".to_string());
        };
        let mut normalized = Vec::new();
        for item in items {
            let Some(obj) = item.as_object() else {
                return Err("each task in tasks JSON must be an object".to_string());
            };
            let required = Self::caps_from_value(obj.get("required"));
            let optional = Self::caps_from_value(obj.get("optional"))
                .into_iter()
                .filter(|cap| !required.iter().any(|req| req == cap))
                .collect::<Vec<_>>();
            let id = obj
                .get("id")
                .and_then(|v| v.as_str())
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| format!("step_{}", normalized.len() + 1));
            let agent_id = obj
                .get("agent_id")
                .or_else(|| obj.get("agent_preset"))
                .and_then(|v| v.as_str())
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty());
            normalized.push(json!({
                "id": id,
                "agent_id": agent_id,
                "required": required,
                "optional": optional,
            }));
        }
        Ok(normalized)
    }

    fn automation_override_yaml(id: &str, tasks: &[Value], summary: &Value) -> String {
        let required = summary
            .get("automation")
            .and_then(|v| v.get("required"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let optional = summary
            .get("automation")
            .and_then(|v| v.get("optional"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut lines = vec![
            format!("id: {}", id),
            "version: 0.1.0".to_string(),
            "publisher: local.user".to_string(),
            "description: Project override automation preset".to_string(),
            "tags:".to_string(),
            "  - override".to_string(),
            "tasks:".to_string(),
        ];
        if tasks.is_empty() {
            lines.push("  - id: step_1".to_string());
            lines.push("    agent_preset:".to_string());
            lines.push("    capabilities:".to_string());
            lines.push("      required: []".to_string());
            lines.push("      optional: []".to_string());
        } else {
            for task in tasks {
                let task_id = task
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("step")
                    .trim();
                let agent = task
                    .get("agent_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim();
                lines.push(format!("  - id: {}", task_id));
                lines.push(format!("    agent_preset: {}", agent));
                lines.push("    capabilities:".to_string());
                lines.push("      required:".to_string());
                if let Some(req) = task.get("required").and_then(|v| v.as_array()) {
                    if req.is_empty() {
                        lines.push("        -".to_string());
                    } else {
                        for cap in req {
                            let cap = cap.as_str().unwrap_or("").trim();
                            if cap.is_empty() {
                                continue;
                            }
                            lines.push(format!("        - {}", cap));
                        }
                    }
                } else {
                    lines.push("        -".to_string());
                }
                lines.push("      optional:".to_string());
                if let Some(opt) = task.get("optional").and_then(|v| v.as_array()) {
                    if opt.is_empty() {
                        lines.push("        -".to_string());
                    } else {
                        for cap in opt {
                            let cap = cap.as_str().unwrap_or("").trim();
                            if cap.is_empty() {
                                continue;
                            }
                            lines.push(format!("        - {}", cap));
                        }
                    }
                } else {
                    lines.push("        -".to_string());
                }
            }
        }
        lines.push("capabilities:".to_string());
        lines.push("  required:".to_string());
        if required.is_empty() {
            lines.push("    -".to_string());
        } else {
            for cap in required {
                let cap = cap.as_str().unwrap_or("").trim();
                if !cap.is_empty() {
                    lines.push(format!("    - {}", cap));
                }
            }
        }
        lines.push("  optional:".to_string());
        if optional.is_empty() {
            lines.push("    -".to_string());
        } else {
            for cap in optional {
                let cap = cap.as_str().unwrap_or("").trim();
                if !cap.is_empty() {
                    lines.push(format!("    - {}", cap));
                }
            }
        }
        lines.push(String::new());
        lines.join("\n")
    }

    fn desired_engine_version() -> Option<(u64, u64, u64)> {
        Self::parse_semver_triplet(env!("CARGO_PKG_VERSION"))
    }

    fn installed_engine_version(path: &std::path::Path) -> Option<(u64, u64, u64)> {
        let output = StdCommand::new(path).arg("--version").output().ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Self::parse_semver_triplet(&stdout).or_else(|| Self::parse_semver_triplet(&stderr))
    }

    fn engine_binary_is_stale(path: &std::path::Path) -> bool {
        let Some(desired) = Self::desired_engine_version() else {
            return false;
        };
        let Some(installed) = Self::installed_engine_version(path) else {
            // If we cannot determine a version, keep existing behavior and accept it.
            return false;
        };
        installed < desired
    }

    async fn ensure_engine_binary(&mut self) -> anyhow::Result<Option<PathBuf>> {
        if let Some(path) = &self.engine_binary_path {
            if path
                .metadata()
                .map(|m| m.len() >= MIN_ENGINE_BINARY_SIZE)
                .unwrap_or(false)
            {
                if Self::engine_binary_is_stale(path) {
                    self.engine_download_phase =
                        Some("Cached engine is stale; refreshing binary".to_string());
                    self.engine_binary_path = None;
                } else {
                    return Ok(Some(path.clone()));
                }
            } else {
                self.engine_binary_path = None;
            }
        }

        if cfg!(debug_assertions) {
            if let Some(path) = Self::find_dev_engine_binary() {
                if Self::engine_binary_is_stale(&path) {
                    if !self.local_engine_build_attempted {
                        self.local_engine_build_attempted = true;
                        self.engine_download_phase =
                            Some("Local dev engine is stale; rebuilding local engine".to_string());
                        if let Some(rebuilt) = self.try_build_local_dev_engine_binary() {
                            if !Self::engine_binary_is_stale(&rebuilt) {
                                self.engine_binary_path = Some(rebuilt.clone());
                                self.engine_download_active = false;
                                self.engine_download_total_bytes = None;
                                self.engine_downloaded_bytes = 0;
                                self.engine_download_phase =
                                    Some("Using rebuilt local dev engine binary".to_string());
                                return Ok(Some(rebuilt));
                            }
                        }
                    }
                    self.engine_download_phase =
                        Some("Local dev engine is stale; using newer managed binary".to_string());
                } else {
                    self.engine_binary_path = Some(path.clone());
                    self.engine_download_active = false;
                    self.engine_download_total_bytes = None;
                    self.engine_downloaded_bytes = 0;
                    self.engine_download_phase = Some("Using local dev engine binary".to_string());
                    return Ok(Some(path.clone()));
                }
            }
        }

        if let Some(path) = Self::find_desktop_bundled_engine_binary() {
            if Self::engine_binary_is_stale(&path) {
                self.engine_download_phase = Some(
                    "Desktop bundled engine is stale; using updated sidecar binary".to_string(),
                );
            } else {
                self.engine_binary_path = Some(path.clone());
                self.engine_download_active = false;
                self.engine_download_total_bytes = None;
                self.engine_downloaded_bytes = 0;
                self.engine_download_phase =
                    Some("Using desktop bundled engine binary".to_string());
                return Ok(Some(path));
            }
        }

        let Some(binaries_dir) = Self::shared_binaries_dir() else {
            return Err(anyhow!(
                "Unable to resolve Tandem binaries directory for engine download"
            ));
        };
        let binary_path = binaries_dir.join(Self::engine_binary_name());
        if binary_path
            .metadata()
            .map(|m| m.len() >= MIN_ENGINE_BINARY_SIZE)
            .unwrap_or(false)
        {
            if Self::engine_binary_is_stale(&binary_path) {
                self.engine_download_phase =
                    Some("Local sidecar engine is stale; downloading latest".to_string());
            } else {
                self.engine_binary_path = Some(binary_path.clone());
                self.engine_download_active = false;
                self.engine_download_total_bytes = None;
                self.engine_downloaded_bytes = 0;
                self.engine_download_phase = Some("Using cached engine binary".to_string());
                return Ok(Some(binary_path));
            }
        }

        fs::create_dir_all(&binaries_dir)?;
        self.connection_status = "Downloading engine...".to_string();
        let path = self
            .download_engine_binary(&binaries_dir, &binary_path)
            .await?;
        self.engine_binary_path = Some(path.clone());
        self.engine_download_active = false;
        self.engine_download_last_error = None;
        self.engine_download_retry_at = None;
        self.engine_download_phase = Some("Engine download complete".to_string());
        Ok(Some(path))
    }

    async fn download_engine_binary(
        &mut self,
        binaries_dir: &PathBuf,
        binary_path: &PathBuf,
    ) -> anyhow::Result<PathBuf> {
        self.engine_download_active = true;
        self.engine_download_total_bytes = None;
        self.engine_downloaded_bytes = 0;
        self.engine_download_phase = Some("Fetching release metadata".to_string());

        let client = Client::new();
        let release_url = format!("{}/repos/{}/releases", GITHUB_API, ENGINE_REPO);
        let releases: Vec<GitHubRelease> = client
            .get(release_url)
            .header("User-Agent", "Tandem-TUI")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let release = releases
            .iter()
            .find(|release| {
                !release.draft
                    && !release.prerelease
                    && release
                        .assets
                        .iter()
                        .any(|asset| Self::engine_asset_matches(&asset.name))
            })
            .or_else(|| {
                releases.iter().find(|release| {
                    !release.draft
                        && release
                            .assets
                            .iter()
                            .any(|asset| Self::engine_asset_matches(&asset.name))
                })
            })
            .ok_or_else(|| anyhow!("No compatible tandem-engine release found"))?;
        if release.prerelease {
            tracing::info!(
                "No stable compatible tandem-engine release found; using prerelease {}",
                release.tag_name
            );
        }

        let asset_name = Self::engine_asset_name();
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .or_else(|| {
                release
                    .assets
                    .iter()
                    .find(|asset| Self::engine_asset_matches(&asset.name))
            })
            .ok_or_else(|| anyhow!("No compatible tandem-engine asset found"))?;

        let download_url = asset.browser_download_url.clone();
        let archive_path = binary_path.with_extension("download");
        self.engine_download_total_bytes = Some(asset.size);
        self.engine_downloaded_bytes = 0;
        self.engine_download_phase = Some(format!("Downloading {}", asset.name));
        let mut response = client
            .get(&download_url)
            .header("User-Agent", "Tandem-TUI")
            .send()
            .await?
            .error_for_status()?;
        if let Some(total) = response.content_length() {
            self.engine_download_total_bytes = Some(total);
        }
        let mut file = tokio::fs::File::create(&archive_path).await?;
        while let Some(chunk) = response.chunk().await? {
            file.write_all(&chunk).await?;
            self.engine_downloaded_bytes = self
                .engine_downloaded_bytes
                .saturating_add(chunk.len() as u64);
            self.connection_status = match self.engine_download_total_bytes {
                Some(total) if total > 0 => {
                    let pct = (self.engine_downloaded_bytes as f64 / total as f64) * 100.0;
                    format!("Downloading engine... {:.0}%", pct.clamp(0.0, 100.0))
                }
                _ => format!(
                    "Downloading engine... {} KB",
                    self.engine_downloaded_bytes / 1024
                ),
            };
        }
        file.flush().await?;
        self.engine_download_phase = Some("Extracting engine archive".to_string());

        let asset_name = asset.name.clone();
        let archive_path_clone = archive_path.clone();
        let binaries_dir_clone = binaries_dir.clone();
        let binary_path_clone = binary_path.clone();

        let extracted_path = tokio::task::spawn_blocking(move || -> anyhow::Result<PathBuf> {
            if asset_name.ends_with(".zip") {
                let file = fs::File::open(&archive_path_clone)?;
                let mut archive = zip::ZipArchive::new(file)?;
                for i in 0..archive.len() {
                    let mut file = archive.by_index(i)?;
                    let outpath = binaries_dir_clone.join(file.mangled_name());
                    if file.is_dir() {
                        fs::create_dir_all(&outpath)?;
                    } else {
                        if let Some(p) = outpath.parent() {
                            fs::create_dir_all(p)?;
                        }
                        let mut outfile = fs::File::create(&outpath)?;
                        std::io::copy(&mut file, &mut outfile)?;
                    }
                }
            } else if asset_name.ends_with(".tar.gz") {
                let file = fs::File::open(&archive_path_clone)?;
                let gz = flate2::read::GzDecoder::new(file);
                let mut archive = tar::Archive::new(gz);
                archive.unpack(&binaries_dir_clone)?;
            }

            let extracted =
                Self::find_extracted_binary(&binaries_dir_clone, Self::engine_binary_name())?;
            if extracted != binary_path_clone {
                if binary_path_clone.exists() {
                    fs::remove_file(&binary_path_clone)?;
                }
                fs::rename(&extracted, &binary_path_clone)?;
            }

            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&binary_path_clone)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&binary_path_clone, perms)?;
            }

            fs::remove_file(&archive_path_clone).ok();
            Ok(binary_path_clone)
        })
        .await??;
        self.engine_download_phase = Some("Finalizing engine install".to_string());
        Ok(extracted_path)
    }

    pub fn handle_key_event(&self, key: KeyEvent) -> Option<Action> {
        // Global control keys
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') => return Some(Action::CtrlCPressed),
                KeyCode::Char('x') => return Some(Action::Quit),
                KeyCode::Char('n') => return Some(Action::NewAgent),
                KeyCode::Char('w') => return Some(Action::CloseActiveAgent),
                KeyCode::Char('u') => return Some(Action::PageUp),
                KeyCode::Char('d') => return Some(Action::PageDown),
                KeyCode::Char('y') => return Some(Action::CopyLastAssistant),
                _ => {}
            }
        }

        match self.state {
            AppState::StartupAnimation { .. } => {
                if !self.startup_engine_bootstrap_done {
                    return None;
                }
                match key.code {
                    KeyCode::Enter | KeyCode::Esc | KeyCode::Char(' ') => {
                        Some(Action::SkipAnimation)
                    }
                    _ => None,
                }
            }
            AppState::PinPrompt { .. } => match key.code {
                KeyCode::Esc => Some(Action::Quit),
                KeyCode::Enter => Some(Action::SubmitPin),
                KeyCode::Backspace => Some(Action::EnterPin('\x08')),
                KeyCode::Char(c) if c.is_ascii_digit() => Some(Action::EnterPin(c)),
                _ => None,
            },
            AppState::Connecting => {
                // Ignore typing while engine is loading.
                None
            }
            AppState::MainMenu => match key.code {
                KeyCode::Char('q') => Some(Action::Quit),
                KeyCode::Char('n') => Some(Action::NewSession),
                KeyCode::Char('d') | KeyCode::Delete => Some(Action::DeleteSelectedSession),
                KeyCode::Char('j') | KeyCode::Down => Some(Action::NextSession),
                KeyCode::Char('k') | KeyCode::Up => Some(Action::PreviousSession),
                KeyCode::Enter => Some(Action::SelectSession),
                _ => None,
            },

            AppState::Chat { .. } => {
                if let AppState::Chat {
                    modal,
                    pending_requests,
                    ..
                } = &self.state
                {
                    let active_modal = modal.clone();
                    if let Some(active_modal) = active_modal {
                        if matches!(active_modal, ModalState::RequestCenter)
                            && pending_requests.is_empty()
                        {
                            // Treat stale/empty request center as closed so normal typing works.
                        } else {
                            return match key.code {
                                KeyCode::Esc => Some(Action::CloseModal),
                                KeyCode::Up if matches!(active_modal, ModalState::FileSearch) => {
                                    Some(Action::FileSearchSelectPrev)
                                }
                                KeyCode::Down if matches!(active_modal, ModalState::FileSearch) => {
                                    Some(Action::FileSearchSelectNext)
                                }
                                KeyCode::Enter
                                    if matches!(active_modal, ModalState::FileSearch) =>
                                {
                                    Some(Action::FileSearchConfirm)
                                }
                                KeyCode::Backspace
                                    if matches!(active_modal, ModalState::FileSearch) =>
                                {
                                    Some(Action::FileSearchBackspace)
                                }
                                KeyCode::Char('\u{8}') | KeyCode::Char('\u{7f}')
                                    if matches!(active_modal, ModalState::FileSearch) =>
                                {
                                    Some(Action::FileSearchBackspace)
                                }
                                KeyCode::Char(c)
                                    if matches!(active_modal, ModalState::FileSearch) =>
                                {
                                    Some(Action::FileSearchInput(c))
                                }
                                KeyCode::Up if matches!(active_modal, ModalState::Pager) => {
                                    Some(Action::OverlayScrollUp)
                                }
                                KeyCode::Down if matches!(active_modal, ModalState::Pager) => {
                                    Some(Action::OverlayScrollDown)
                                }
                                KeyCode::PageUp if matches!(active_modal, ModalState::Pager) => {
                                    Some(Action::OverlayPageUp)
                                }
                                KeyCode::PageDown if matches!(active_modal, ModalState::Pager) => {
                                    Some(Action::OverlayPageDown)
                                }
                                KeyCode::Enter
                                    if matches!(active_modal, ModalState::RequestCenter) =>
                                {
                                    Some(Action::RequestConfirm)
                                }
                                KeyCode::Enter
                                    if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                                {
                                    Some(Action::PlanWizardSubmit)
                                }
                                KeyCode::Up
                                    if matches!(active_modal, ModalState::RequestCenter)
                                        && key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    Some(Action::RequestSelectPrev)
                                }
                                KeyCode::Down
                                    if matches!(active_modal, ModalState::RequestCenter)
                                        && key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    Some(Action::RequestSelectNext)
                                }
                                KeyCode::Up
                                    if matches!(active_modal, ModalState::RequestCenter)
                                        && self.request_center_active_is_question() =>
                                {
                                    Some(Action::RequestOptionPrev)
                                }
                                KeyCode::Down
                                    if matches!(active_modal, ModalState::RequestCenter)
                                        && self.request_center_active_is_question() =>
                                {
                                    Some(Action::RequestOptionNext)
                                }
                                KeyCode::Up
                                    if matches!(active_modal, ModalState::RequestCenter) =>
                                {
                                    Some(Action::RequestSelectPrev)
                                }
                                KeyCode::Up
                                    if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                                {
                                    Some(Action::PlanWizardPrevField)
                                }
                                KeyCode::Down
                                    if matches!(active_modal, ModalState::RequestCenter) =>
                                {
                                    Some(Action::RequestSelectNext)
                                }
                                KeyCode::Down
                                    if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                                {
                                    Some(Action::PlanWizardNextField)
                                }
                                KeyCode::Tab
                                    if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                                {
                                    Some(Action::PlanWizardNextField)
                                }
                                KeyCode::BackTab
                                    if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                                {
                                    Some(Action::PlanWizardPrevField)
                                }
                                KeyCode::Left
                                    if matches!(active_modal, ModalState::RequestCenter) =>
                                {
                                    Some(Action::RequestOptionPrev)
                                }
                                KeyCode::Right
                                    if matches!(active_modal, ModalState::RequestCenter) =>
                                {
                                    Some(Action::RequestOptionNext)
                                }
                                KeyCode::Backspace
                                    if matches!(active_modal, ModalState::RequestCenter) =>
                                {
                                    Some(Action::RequestBackspace)
                                }
                                KeyCode::Char('\u{8}') | KeyCode::Char('\u{7f}')
                                    if matches!(active_modal, ModalState::RequestCenter) =>
                                {
                                    Some(Action::RequestBackspace)
                                }
                                KeyCode::Backspace
                                    if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                                {
                                    Some(Action::PlanWizardBackspace)
                                }
                                KeyCode::Char(' ')
                                    if matches!(active_modal, ModalState::RequestCenter) =>
                                {
                                    Some(Action::RequestToggleCurrent)
                                }
                                KeyCode::Char('r') | KeyCode::Char('R')
                                    if matches!(active_modal, ModalState::RequestCenter) =>
                                {
                                    Some(Action::RequestReject)
                                }
                                KeyCode::Char('e') | KeyCode::Char('E')
                                    if matches!(active_modal, ModalState::RequestCenter)
                                        && key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    Some(Action::ToggleRequestPanelExpand)
                                }
                                KeyCode::Char(c)
                                    if matches!(active_modal, ModalState::RequestCenter)
                                        && c.is_ascii_digit()
                                        && self.request_center_digit_is_shortcut(c) =>
                                {
                                    Some(Action::RequestDigit(c as u8 - b'0'))
                                }
                                KeyCode::Char(c)
                                    if matches!(active_modal, ModalState::RequestCenter) =>
                                {
                                    Some(Action::RequestInput(c))
                                }
                                KeyCode::Char(c)
                                    if matches!(active_modal, ModalState::PlanFeedbackWizard) =>
                                {
                                    Some(Action::PlanWizardInput(c))
                                }
                                KeyCode::Char('y') | KeyCode::Char('Y')
                                    if matches!(
                                        active_modal,
                                        ModalState::ConfirmCloseAgent { .. }
                                    ) =>
                                {
                                    Some(Action::ConfirmCloseAgent(true))
                                }
                                KeyCode::Char('n') | KeyCode::Char('N')
                                    if matches!(
                                        active_modal,
                                        ModalState::ConfirmCloseAgent { .. }
                                    ) =>
                                {
                                    Some(Action::ConfirmCloseAgent(false))
                                }
                                KeyCode::Char('y') | KeyCode::Char('Y')
                                    if matches!(
                                        active_modal,
                                        ModalState::StartPlanAgents { .. }
                                    ) =>
                                {
                                    if let ModalState::StartPlanAgents { count } = active_modal {
                                        Some(Action::ConfirmStartPlanAgents {
                                            confirmed: true,
                                            count,
                                        })
                                    } else {
                                        None
                                    }
                                }
                                KeyCode::Char('n') | KeyCode::Char('N')
                                    if matches!(
                                        active_modal,
                                        ModalState::StartPlanAgents { .. }
                                    ) =>
                                {
                                    if let ModalState::StartPlanAgents { count } = active_modal {
                                        Some(Action::ConfirmStartPlanAgents {
                                            confirmed: false,
                                            count,
                                        })
                                    } else {
                                        None
                                    }
                                }
                                _ => None,
                            };
                        }
                    }
                }
                if self.show_autocomplete {
                    match key.code {
                        KeyCode::Esc => Some(Action::AutocompleteDismiss),
                        _ if Self::is_paste_shortcut(&key) => Some(Action::PasteFromClipboard),
                        KeyCode::Enter | KeyCode::Tab => Some(Action::AutocompleteAccept),
                        KeyCode::Down | KeyCode::Char('j')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            Some(Action::AutocompleteNext)
                        }
                        KeyCode::Up | KeyCode::Char('k')
                            if key.modifiers.contains(KeyModifiers::CONTROL) =>
                        {
                            Some(Action::AutocompletePrev)
                        }
                        KeyCode::Down => Some(Action::AutocompleteNext),
                        KeyCode::Up => Some(Action::AutocompletePrev),
                        KeyCode::Backspace => Some(Action::BackspaceCommand),
                        KeyCode::Char('\u{8}') | KeyCode::Char('\u{7f}') => {
                            Some(Action::BackspaceCommand)
                        }
                        KeyCode::Delete => Some(Action::DeleteForwardCommand),
                        KeyCode::Left => Some(Action::MoveCursorLeft),
                        KeyCode::Right => Some(Action::MoveCursorRight),
                        KeyCode::Home => Some(Action::MoveCursorHome),
                        KeyCode::End => Some(Action::MoveCursorEnd),
                        KeyCode::Char(c) => Some(Action::CommandInput(c)),
                        _ => None,
                    }
                } else {
                    match key.code {
                        KeyCode::Esc => None,
                        _ if Self::is_paste_shortcut(&key) => Some(Action::PasteFromClipboard),
                        KeyCode::F(1) => Some(Action::ShowHelpModal),
                        KeyCode::F(2) => Some(Action::OpenDocs),
                        KeyCode::Char('g') | KeyCode::Char('G')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::ToggleUiMode)
                        }
                        KeyCode::Char('m') | KeyCode::Char('M')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::CycleMode)
                        }
                        KeyCode::Char('r') | KeyCode::Char('R')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::OpenRequestCenter)
                        }
                        KeyCode::Char('i') | KeyCode::Char('I')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::QueueSteeringFromComposer)
                        }
                        KeyCode::Char('p') | KeyCode::Char('P')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::OpenFileSearch)
                        }
                        KeyCode::Char('d') | KeyCode::Char('D')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::OpenDiffOverlay)
                        }
                        KeyCode::Char('e') | KeyCode::Char('E')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::OpenExternalEditor)
                        }
                        KeyCode::Char('s') | KeyCode::Char('S')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::StartDemoStream)
                        }
                        KeyCode::Char('b') | KeyCode::Char('B')
                            if key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::SpawnBackgroundDemo)
                        }
                        KeyCode::Char('[') => Some(Action::GridPagePrev),
                        KeyCode::Char(']') => Some(Action::GridPageNext),
                        KeyCode::BackTab => Some(Action::SwitchAgentPrev),
                        KeyCode::Enter
                            if key.modifiers.contains(KeyModifiers::SHIFT)
                                || key.modifiers.contains(KeyModifiers::ALT) =>
                        {
                            Some(Action::InsertNewline)
                        }
                        KeyCode::Enter if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            Some(Action::SubmitCommand)
                        }
                        KeyCode::Enter => Some(Action::SubmitCommand),
                        KeyCode::Backspace => Some(Action::BackspaceCommand),
                        KeyCode::Char('\u{8}') | KeyCode::Char('\u{7f}') => {
                            Some(Action::BackspaceCommand)
                        }
                        KeyCode::Delete => Some(Action::DeleteForwardCommand),
                        KeyCode::Tab => Some(Action::SwitchAgentNext),
                        KeyCode::Up if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            Some(Action::MoveCursorUp)
                        }
                        KeyCode::Down if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            Some(Action::MoveCursorDown)
                        }
                        KeyCode::Up => Some(Action::ScrollUp),
                        KeyCode::Down => Some(Action::ScrollDown),
                        KeyCode::Left => Some(Action::MoveCursorLeft),
                        KeyCode::Right => Some(Action::MoveCursorRight),
                        KeyCode::Home => Some(Action::MoveCursorHome),
                        KeyCode::End => Some(Action::MoveCursorEnd),
                        KeyCode::PageUp => Some(Action::PageUp),
                        KeyCode::PageDown => Some(Action::PageDown),
                        KeyCode::Char(c)
                            if key.modifiers.contains(KeyModifiers::ALT) && c.is_ascii_digit() =>
                        {
                            let idx = (c as u8 - b'0') as usize;
                            if idx > 0 {
                                Some(Action::SelectAgentByNumber(idx))
                            } else {
                                None
                            }
                        }
                        KeyCode::Char(c) => Some(Action::CommandInput(c)),
                        _ => None,
                    }
                }
            }

            AppState::SetupWizard { .. } => {
                if Self::is_paste_shortcut(&key) {
                    return Some(Action::PasteFromClipboard);
                }
                match key.code {
                    KeyCode::Esc => Some(Action::Quit),
                    KeyCode::Enter => Some(Action::SetupNextStep),
                    KeyCode::Down => Some(Action::SetupNextItem),
                    KeyCode::Up => Some(Action::SetupPrevItem),
                    KeyCode::Char(c) => Some(Action::SetupInput(c)),
                    KeyCode::Backspace => Some(Action::SetupBackspace),
                    _ => None,
                }
            }
        }
    }
    pub fn handle_mouse_event(&self, mouse: MouseEvent) -> Option<Action> {
        match mouse.kind {
            MouseEventKind::ScrollDown => match self.state {
                AppState::MainMenu => Some(Action::NextSession),
                AppState::Chat { .. } => Some(Action::ScrollDown),
                AppState::SetupWizard { .. } => Some(Action::SetupNextItem),
                _ => None,
            },
            MouseEventKind::ScrollUp => match self.state {
                AppState::MainMenu => Some(Action::PreviousSession),
                AppState::Chat { .. } => Some(Action::ScrollUp),
                AppState::SetupWizard { .. } => Some(Action::SetupPrevItem),
                _ => None,
            },
            _ => None,
        }
    }

    pub async fn update(&mut self, action: Action) -> anyhow::Result<()> {
        match action {
            Action::Quit => self.should_quit = true,
            Action::CtrlCPressed => {
                let now = Instant::now();
                if self
                    .quit_armed_at
                    .map(|t| now.duration_since(t).as_millis() <= 1500)
                    .unwrap_or(false)
                {
                    self.should_quit = true;
                    self.quit_armed_at = None;
                } else {
                    self.quit_armed_at = Some(now);
                    let mut cancelled = false;
                    if let AppState::Chat {
                        active_agent_index,
                        agents,
                        ..
                    } = &self.state
                    {
                        if *active_agent_index < agents.len()
                            && agents[*active_agent_index].active_run_id.is_some()
                        {
                            self.cancel_agent_if_running(*active_agent_index).await;
                            cancelled = true;
                        }
                    }
                    if let AppState::Chat { messages, .. } = &mut self.state {
                        let notice = if cancelled {
                            "Cancelled active run. Press Ctrl+C again within 1.5s to quit."
                        } else {
                            "Press Ctrl+C again within 1.5s to quit."
                        };
                        messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: vec![ContentBlock::Text(notice.to_string())],
                        });
                    }
                }
            }
            Action::SkipAnimation => {
                if let AppState::StartupAnimation { .. } = self.state {
                    self.state = AppState::PinPrompt {
                        input: String::new(),
                        error: None,
                        // If a vault key exists, unlock flow should always be used.
                        // An empty/missing keystore can be recreated after successful unlock.
                        mode: if self.vault_key.is_some() {
                            PinPromptMode::UnlockExisting
                        } else {
                            PinPromptMode::CreateNew
                        },
                    };
                }
            }
            Action::ToggleTaskPin(task_id) => {
                if let AppState::Chat { tasks, .. } = &mut self.state {
                    if let Some(task) = tasks.iter_mut().find(|t| t.id == task_id) {
                        task.pinned = !task.pinned;
                    }
                }
            }

            Action::Tick => self.tick().await,

            Action::EnterPin(c) => {
                if let AppState::PinPrompt { input, .. } = &mut self.state {
                    if c == '\x08' {
                        input.pop();
                    } else if c.is_ascii_digit() && input.len() < MAX_PIN_LENGTH {
                        input.push(c);
                    }
                }
            }
            Action::SubmitPin => {
                let (input, mode) = match &self.state {
                    AppState::PinPrompt { input, mode, .. } => (input.clone(), mode.clone()),
                    _ => (String::new(), PinPromptMode::UnlockExisting),
                };

                match mode {
                    PinPromptMode::UnlockExisting => {
                        if let Err(e) = crate::crypto::vault::validate_pin_format(&input) {
                            self.state = AppState::PinPrompt {
                                input: String::new(),
                                error: Some(e.to_string()),
                                mode: PinPromptMode::UnlockExisting,
                            };
                            return Ok(());
                        }
                        match &self.vault_key {
                            Some(vk) => match vk.decrypt(&input) {
                                Ok(master_key) => {
                                    if let Some(config_dir) = &self.config_dir {
                                        let keystore_path = config_dir.join("tandem.keystore");
                                        match SecureKeyStore::load(&keystore_path, master_key) {
                                            Ok(store) => {
                                                // Ensure keystore file exists on disk for first-time users.
                                                if let Err(e) = store.save(&keystore_path) {
                                                    self.state = AppState::PinPrompt {
                                                        input: String::new(),
                                                        error: Some(format!(
                                                            "Failed to save keystore: {}",
                                                            e
                                                        )),
                                                        mode: PinPromptMode::UnlockExisting,
                                                    };
                                                    return Ok(());
                                                }
                                                self.keystore = Some(store);
                                                self.state = AppState::Connecting;
                                                return Ok(());
                                            }
                                            Err(_) => {
                                                self.state = AppState::PinPrompt {
                                                    input: String::new(),
                                                    error: Some(
                                                        "Failed to load keystore".to_string(),
                                                    ),
                                                    mode: PinPromptMode::UnlockExisting,
                                                };
                                            }
                                        }
                                    } else {
                                        self.state = AppState::PinPrompt {
                                            input: String::new(),
                                            error: Some("Config dir not found".to_string()),
                                            mode: PinPromptMode::UnlockExisting,
                                        };
                                    }
                                }
                                Err(_) => {
                                    self.state = AppState::PinPrompt {
                                        input: String::new(),
                                        error: Some("Invalid PIN".to_string()),
                                        mode: PinPromptMode::UnlockExisting,
                                    };
                                }
                            },
                            None => {
                                self.state = AppState::PinPrompt {
                                    input: String::new(),
                                    error: Some(
                                        "No vault key found. Create a new PIN.".to_string(),
                                    ),
                                    mode: PinPromptMode::CreateNew,
                                };
                            }
                        }
                    }
                    PinPromptMode::CreateNew => {
                        match crate::crypto::vault::validate_pin_format(&input) {
                            Ok(_) => {
                                self.state = AppState::PinPrompt {
                                    input: String::new(),
                                    error: None,
                                    mode: PinPromptMode::ConfirmNew { first_pin: input },
                                };
                            }
                            Err(e) => {
                                self.state = AppState::PinPrompt {
                                    input: String::new(),
                                    error: Some(e.to_string()),
                                    mode: PinPromptMode::CreateNew,
                                };
                            }
                        }
                    }
                    PinPromptMode::ConfirmNew { first_pin } => {
                        if let Err(e) = crate::crypto::vault::validate_pin_format(&input) {
                            self.state = AppState::PinPrompt {
                                input: String::new(),
                                error: Some(e.to_string()),
                                mode: PinPromptMode::CreateNew,
                            };
                            return Ok(());
                        }
                        if input != first_pin {
                            self.state = AppState::PinPrompt {
                                input: String::new(),
                                error: Some("PINs do not match. Enter a new PIN.".to_string()),
                                mode: PinPromptMode::CreateNew,
                            };
                            return Ok(());
                        }

                        if let Some(config_dir) = &self.config_dir {
                            let vault_path = config_dir.join("vault.key");
                            let keystore_path = config_dir.join("tandem.keystore");
                            match EncryptedVaultKey::create(&input) {
                                Ok((vault_key, master_key)) => {
                                    if let Err(e) = vault_key.save(&vault_path) {
                                        self.state = AppState::PinPrompt {
                                            input: String::new(),
                                            error: Some(format!("Failed to save vault: {}", e)),
                                            mode: PinPromptMode::CreateNew,
                                        };
                                        return Ok(());
                                    }

                                    match SecureKeyStore::load(&keystore_path, master_key) {
                                        Ok(store) => {
                                            if let Err(e) = store.save(&keystore_path) {
                                                self.state = AppState::PinPrompt {
                                                    input: String::new(),
                                                    error: Some(format!(
                                                        "Failed to save keystore: {}",
                                                        e
                                                    )),
                                                    mode: PinPromptMode::CreateNew,
                                                };
                                                return Ok(());
                                            }
                                            self.vault_key = Some(vault_key);
                                            self.keystore = Some(store);
                                            self.state = AppState::Connecting;
                                            return Ok(());
                                        }
                                        Err(e) => {
                                            self.state = AppState::PinPrompt {
                                                input: String::new(),
                                                error: Some(format!(
                                                    "Failed to initialize keystore: {}",
                                                    e
                                                )),
                                                mode: PinPromptMode::CreateNew,
                                            };
                                        }
                                    }
                                }
                                Err(e) => {
                                    self.state = AppState::PinPrompt {
                                        input: String::new(),
                                        error: Some(format!("Failed to create vault: {}", e)),
                                        mode: PinPromptMode::CreateNew,
                                    };
                                }
                            }
                        } else {
                            self.state = AppState::PinPrompt {
                                input: String::new(),
                                error: Some("Config dir not found".to_string()),
                                mode: PinPromptMode::CreateNew,
                            };
                        }
                    }
                }
            }

            Action::SessionsLoaded(sessions) => {
                self.sessions = sessions;
                if self.selected_session_index >= self.sessions.len() && !self.sessions.is_empty() {
                    self.selected_session_index = self.sessions.len() - 1;
                }
            }
            Action::NextSession => {
                if !self.sessions.is_empty() {
                    self.selected_session_index =
                        (self.selected_session_index + 1) % self.sessions.len();
                }
            }
            Action::PreviousSession => {
                if !self.sessions.is_empty() {
                    if self.selected_session_index > 0 {
                        self.selected_session_index -= 1;
                    } else {
                        self.selected_session_index = self.sessions.len() - 1;
                    }
                }
            }
            Action::NewSession => {
                // If configuration is missing, force wizard
                if (self.current_provider.is_none() || self.current_model.is_none())
                    && self.provider_catalog.is_some()
                {
                    let mut step = SetupStep::SelectProvider;
                    let mut selected_provider_index = 0;

                    if let Some(ref current_p) = self.current_provider {
                        if let Some(ref catalog) = self.provider_catalog {
                            if let Some(idx) = catalog.all.iter().position(|p| &p.id == current_p) {
                                selected_provider_index = idx;
                                if self.current_model.is_none() {
                                    step = SetupStep::SelectModel;
                                }
                            }
                        }
                    }

                    self.state = AppState::SetupWizard {
                        step,
                        provider_catalog: self.provider_catalog.clone(),
                        selected_provider_index,
                        selected_model_index: 0,
                        api_key_input: String::new(),
                        model_input: String::new(),
                    };
                    return Ok(());
                }

                if let Some(client) = &self.client {
                    let client = client.clone();
                    // We can't await easily here if update locks self?
                    // Actually update is async, so we can await.
                    // But we hold &mut self.
                    // client clone allows us to call it.
                    // But we can't assign to self.sessions *after* await while holding client?
                    // No, `client` is a local variable. `self` is currently borrowed.
                    // We can't call methods on self.

                    if let Ok(_) = client.create_session(Some("New session".to_string())).await {
                        // Refresh sessions
                        if let Ok(sessions) = client.list_sessions().await {
                            self.sessions = sessions;
                            // Select the new one (usually first or last depending on sort)
                            // server sorts by updated desc, so new one is first.
                            self.selected_session_index = 0;
                            if let Some(ref session) = self.sessions.first() {
                                let first_agent =
                                    Self::make_agent_pane("A1".to_string(), session.id.clone());
                                self.state = AppState::Chat {
                                    session_id: session.id.clone(),
                                    command_input: ComposerInputState::new(),
                                    messages: Vec::new(),
                                    scroll_from_bottom: 0,
                                    tasks: Vec::new(),
                                    active_task_id: None,
                                    agents: vec![first_agent],
                                    active_agent_index: 0,
                                    ui_mode: UiMode::Focus,
                                    grid_page: 0,
                                    modal: None,
                                    pending_requests: Vec::new(),
                                    request_cursor: 0,
                                    permission_choice: 0,
                                    plan_wizard: PlanFeedbackWizardState::default(),
                                    last_plan_task_fingerprint: Vec::new(),
                                    plan_awaiting_approval: false,
                                    plan_multi_agent_prompt: None,
                                    plan_waiting_for_clarification_question: false,
                                    request_panel_expanded: false,
                                };
                            }
                        }
                    }
                }
            }

            Action::SelectSession => {
                if !self.sessions.is_empty() {
                    let session = &self.sessions[self.selected_session_index];
                    let loaded_messages = self.load_chat_history(&session.id).await;
                    let (recalled_tasks, recalled_active_task_id) =
                        plan_helpers::rebuild_tasks_from_messages(&loaded_messages);
                    let mut first_agent =
                        Self::make_agent_pane("A1".to_string(), session.id.clone());
                    first_agent.messages = loaded_messages.clone();
                    first_agent.tasks = recalled_tasks.clone();
                    first_agent.active_task_id = recalled_active_task_id.clone();
                    self.state = AppState::Chat {
                        session_id: session.id.clone(),
                        command_input: ComposerInputState::new(),
                        messages: loaded_messages,
                        scroll_from_bottom: 0,
                        tasks: recalled_tasks,
                        active_task_id: recalled_active_task_id,
                        agents: vec![first_agent],
                        active_agent_index: 0,
                        ui_mode: UiMode::Focus,
                        grid_page: 0,
                        modal: None,
                        pending_requests: Vec::new(),
                        request_cursor: 0,
                        permission_choice: 0,
                        plan_wizard: PlanFeedbackWizardState::default(),
                        last_plan_task_fingerprint: Vec::new(),
                        plan_awaiting_approval: false,
                        plan_multi_agent_prompt: None,
                        plan_waiting_for_clarification_question: false,
                        request_panel_expanded: false,
                    };
                }
            }
            Action::DeleteSelectedSession => {
                if self.sessions.is_empty() {
                    self.connection_status = "No session selected to delete.".to_string();
                    return Ok(());
                }
                let selected_index = self.selected_session_index.min(self.sessions.len() - 1);
                let selected = self.sessions[selected_index].clone();
                if let Some(client) = &self.client {
                    match client.delete_session(&selected.id).await {
                        Ok(_) => {
                            self.sessions.remove(selected_index);
                            if self.sessions.is_empty() {
                                self.selected_session_index = 0;
                            } else if self.selected_session_index >= self.sessions.len() {
                                self.selected_session_index = self.sessions.len() - 1;
                            }
                            self.connection_status =
                                format!("Deleted session: {} ({})", selected.title, selected.id);
                        }
                        Err(err) => {
                            self.connection_status =
                                format!("Failed to delete session {}: {}", selected.id, err);
                        }
                    }
                } else {
                    self.connection_status = "Not connected to engine".to_string();
                }
            }

            Action::CommandInput(c) => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.insert_char(c);
                    let input = command_input.text().to_string();
                    self.update_autocomplete_for_input(&input);
                }
                self.sync_active_agent_from_chat();
            }

            Action::BackspaceCommand => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    if let Some((start, end)) = Self::paste_token_range_for_backspace(command_input)
                    {
                        command_input.remove_range(start, end);
                    } else {
                        command_input.backspace();
                    }
                    let input = command_input.text().to_string();
                    if input == "/" {
                        self.autocomplete_items = COMMAND_HELP
                            .iter()
                            .map(|(name, desc)| (name.to_string(), desc.to_string()))
                            .collect();
                        self.autocomplete_index = 0;
                        self.autocomplete_mode = AutocompleteMode::Command;
                        self.show_autocomplete = true;
                    } else {
                        self.update_autocomplete_for_input(&input);
                    }
                }
                self.sync_active_agent_from_chat();
            }
            Action::DeleteForwardCommand => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    if let Some((start, end)) = Self::paste_token_range_for_delete(command_input) {
                        command_input.remove_range(start, end);
                    } else {
                        command_input.delete_forward();
                    }
                    let input = command_input.text().to_string();
                    self.update_autocomplete_for_input(&input);
                }
                self.sync_active_agent_from_chat();
            }
            Action::InsertNewline => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.insert_char('\n');
                    let input = command_input.text().to_string();
                    self.update_autocomplete_for_input(&input);
                }
                self.sync_active_agent_from_chat();
            }
            Action::MoveCursorLeft => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.move_left();
                }
                self.sync_active_agent_from_chat();
            }
            Action::MoveCursorRight => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.move_right();
                }
                self.sync_active_agent_from_chat();
            }
            Action::MoveCursorHome => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.move_home();
                }
                self.sync_active_agent_from_chat();
            }
            Action::MoveCursorEnd => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.move_end();
                }
                self.sync_active_agent_from_chat();
            }
            Action::MoveCursorUp => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.move_line_up();
                }
                self.sync_active_agent_from_chat();
            }
            Action::MoveCursorDown => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.move_line_down();
                }
                self.sync_active_agent_from_chat();
            }
            Action::PasteFromClipboard => {
                match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
                    Ok(text) => {
                        let normalized = Self::normalize_paste_payload(&text);
                        if !normalized.is_empty() {
                            match &mut self.state {
                                AppState::Chat {
                                    command_input,
                                    agents,
                                    active_agent_index,
                                    ..
                                } => {
                                    let inserted = Self::insert_chat_paste(
                                        agents.get_mut(*active_agent_index),
                                        &normalized,
                                    );
                                    command_input.insert_str(&inserted);
                                    let input = command_input.text().to_string();
                                    if let Some(agent) = agents.get_mut(*active_agent_index) {
                                        agent.draft = command_input.clone();
                                        Self::prune_agent_paste_registry(agent);
                                    }
                                    self.update_autocomplete_for_input(&input);
                                }
                                AppState::SetupWizard {
                                    step,
                                    api_key_input,
                                    model_input,
                                    ..
                                } => match step {
                                    SetupStep::EnterApiKey => {
                                        api_key_input
                                            .push_str(normalized.trim_end_matches(['\n', '\r']));
                                    }
                                    SetupStep::SelectModel => {
                                        model_input
                                            .push_str(normalized.trim_end_matches(['\n', '\r']));
                                    }
                                    _ => {}
                                },
                                _ => {}
                            }
                            self.sync_active_agent_from_chat();
                        }
                    }
                    Err(err) => {
                        if let AppState::Chat { messages, .. } = &mut self.state {
                            messages.push(ChatMessage {
                                role: MessageRole::System,
                                content: vec![ContentBlock::Text(format!(
                                    "Clipboard paste failed: {}",
                                    err
                                ))],
                            });
                        }
                    }
                }
            }
            Action::PasteInput(text) => {
                let normalized = Self::normalize_paste_payload(&text);
                match &mut self.state {
                    AppState::Chat {
                        command_input,
                        agents,
                        active_agent_index,
                        ..
                    } => {
                        let inserted = Self::insert_chat_paste(
                            agents.get_mut(*active_agent_index),
                            &normalized,
                        );
                        command_input.insert_str(&inserted);
                        let input = command_input.text().to_string();
                        if let Some(agent) = agents.get_mut(*active_agent_index) {
                            agent.draft = command_input.clone();
                            Self::prune_agent_paste_registry(agent);
                        }
                        self.update_autocomplete_for_input(&input);
                    }
                    AppState::SetupWizard {
                        step,
                        api_key_input,
                        model_input,
                        ..
                    } => match step {
                        SetupStep::EnterApiKey => {
                            api_key_input.push_str(normalized.trim_end_matches(['\n', '\r']));
                        }
                        SetupStep::SelectModel => {
                            model_input.push_str(normalized.trim_end_matches(['\n', '\r']));
                        }
                        _ => {}
                    },
                    _ => {}
                }
                self.sync_active_agent_from_chat();
            }

            Action::Autocomplete => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    if !command_input.text().starts_with('/') {
                        command_input.clear();
                        command_input.insert_char('/');
                    }
                    let input = command_input.text().to_string();
                    self.update_autocomplete_for_input(&input);
                }
            }

            Action::AutocompleteNext => {
                if !self.autocomplete_items.is_empty() {
                    self.autocomplete_index =
                        (self.autocomplete_index + 1) % self.autocomplete_items.len();
                }
            }

            Action::AutocompletePrev => {
                if !self.autocomplete_items.is_empty() {
                    if self.autocomplete_index > 0 {
                        self.autocomplete_index -= 1;
                    } else {
                        self.autocomplete_index = self.autocomplete_items.len() - 1;
                    }
                }
            }

            Action::AutocompleteAccept => {
                if self.show_autocomplete && !self.autocomplete_items.is_empty() {
                    let (cmd, _) = self.autocomplete_items[self.autocomplete_index].clone();
                    if let AppState::Chat { command_input, .. } = &mut self.state {
                        command_input.clear();
                        match self.autocomplete_mode {
                            AutocompleteMode::Command => {
                                command_input.set_text(format!("/{} ", cmd));
                            }
                            AutocompleteMode::Provider => {
                                command_input.set_text(format!("/provider {}", cmd));
                            }
                            AutocompleteMode::Model => {
                                command_input.set_text(format!("/model {}", cmd));
                            }
                        }
                        command_input.move_end();
                    }
                    self.show_autocomplete = false;
                    self.autocomplete_items.clear();
                }
                self.sync_active_agent_from_chat();
            }

            Action::AutocompleteDismiss => {
                self.show_autocomplete = false;
                self.autocomplete_items.clear();
                self.autocomplete_mode = AutocompleteMode::Command;
            }

            Action::BackToMenu => {
                self.show_autocomplete = false;
                self.autocomplete_items.clear();
                self.autocomplete_mode = AutocompleteMode::Command;
                self.state = AppState::MainMenu;
            }

            Action::SwitchAgentNext => {
                self.sync_active_agent_from_chat();
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    if !agents.is_empty() {
                        *active_agent_index = (*active_agent_index + 1) % agents.len();
                    }
                }
                self.sync_chat_from_active_agent();
            }
            Action::SwitchAgentPrev => {
                self.sync_active_agent_from_chat();
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    if !agents.is_empty() {
                        if *active_agent_index == 0 {
                            *active_agent_index = agents.len().saturating_sub(1);
                        } else {
                            *active_agent_index -= 1;
                        }
                    }
                }
                self.sync_chat_from_active_agent();
            }
            Action::SelectAgentByNumber(n) => {
                self.sync_active_agent_from_chat();
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    if n > 0 && n <= agents.len() {
                        *active_agent_index = n - 1;
                    }
                }
                self.sync_chat_from_active_agent();
            }
            Action::ToggleUiMode => {
                if let AppState::Chat { ui_mode, .. } = &mut self.state {
                    *ui_mode = if *ui_mode == UiMode::Focus {
                        UiMode::Grid
                    } else {
                        UiMode::Focus
                    };
                }
            }
            Action::CycleMode => {
                self.current_mode = self.current_mode.next();
            }
            Action::GridPageNext => {
                if let AppState::Chat {
                    grid_page, agents, ..
                } = &mut self.state
                {
                    let max_page = agents.len().saturating_sub(1) / 4;
                    *grid_page = (*grid_page + 1).min(max_page);
                }
            }
            Action::GridPagePrev => {
                if let AppState::Chat { grid_page, .. } = &mut self.state {
                    *grid_page = grid_page.saturating_sub(1);
                }
            }
            Action::ShowHelpModal => {
                if let AppState::Chat { modal, .. } = &mut self.state {
                    *modal = Some(ModalState::Help);
                }
            }
            Action::OpenDocs => {
                // Open docs in default browser
                #[cfg(target_os = "windows")]
                let _ = std::process::Command::new("cmd")
                    .args(["/C", "start", "https://docs.tandem.ac/"])
                    .spawn();
                #[cfg(target_os = "macos")]
                let _ = std::process::Command::new("open")
                    .arg("https://docs.tandem.ac/")
                    .spawn();
                #[cfg(target_os = "linux")]
                let _ = std::process::Command::new("xdg-open")
                    .arg("https://docs.tandem.ac/")
                    .spawn();
            }
            Action::CopyLastAssistant => {
                let copied = if let AppState::Chat { messages, .. } = &self.state {
                    self.copy_latest_assistant_to_clipboard(messages)
                } else {
                    Err("Clipboard copy works in chat screens only.".to_string())
                };
                if let AppState::Chat { messages, .. } = &mut self.state {
                    let note = match copied {
                        Ok(len) => format!("Copied {} characters to clipboard.", len),
                        Err(err) => format!("Clipboard copy failed: {}", err),
                    };
                    messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: vec![ContentBlock::Text(note)],
                    });
                }
            }
            Action::CloseModal => {
                if let AppState::Chat { modal, .. } = &mut self.state {
                    if matches!(*modal, Some(ModalState::Pager)) {
                        self.pager_overlay = None;
                    }
                    *modal = None;
                }
                self.open_queued_plan_agent_prompt();
            }
            Action::OpenRequestCenter => {
                self.open_request_center_if_needed();
            }
            Action::OpenFileSearch => {
                self.open_file_search_modal(None);
            }
            Action::OpenDiffOverlay => {
                let status = self.open_diff_overlay().await;
                if let AppState::Chat { messages, .. } = &mut self.state {
                    messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: vec![ContentBlock::Text(status)],
                    });
                }
                self.sync_active_agent_from_chat();
            }
            Action::OpenExternalEditor => {
                let status = self.open_external_editor_for_active_input().await;
                if let AppState::Chat { messages, .. } = &mut self.state {
                    messages.push(ChatMessage {
                        role: MessageRole::System,
                        content: vec![ContentBlock::Text(status)],
                    });
                }
                self.sync_active_agent_from_chat();
            }
            Action::ToggleRequestPanelExpand => {
                if let AppState::Chat {
                    request_panel_expanded,
                    ..
                } = &mut self.state
                {
                    *request_panel_expanded = !*request_panel_expanded;
                }
            }
            Action::RequestSelectNext => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if !pending_requests.is_empty() {
                        *request_cursor = (*request_cursor + 1) % pending_requests.len();
                        *permission_choice = 0;
                    }
                }
            }
            Action::RequestSelectPrev => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if !pending_requests.is_empty() {
                        *request_cursor = if *request_cursor == 0 {
                            pending_requests.len().saturating_sub(1)
                        } else {
                            request_cursor.saturating_sub(1)
                        };
                        *permission_choice = 0;
                    }
                }
            }
            Action::RequestOptionNext => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        match &mut request.kind {
                            PendingRequestKind::Permission(_) => {
                                *permission_choice = (*permission_choice + 1) % 3;
                            }
                            PendingRequestKind::Question(question) => {
                                if let Some(q) = question.questions.get_mut(question.question_index)
                                {
                                    if !q.options.is_empty() {
                                        q.option_cursor = (q.option_cursor + 1) % q.options.len();
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Action::RequestOptionPrev => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        match &mut request.kind {
                            PendingRequestKind::Permission(_) => {
                                *permission_choice = if *permission_choice == 0 {
                                    2
                                } else {
                                    permission_choice.saturating_sub(1)
                                };
                            }
                            PendingRequestKind::Question(question) => {
                                if let Some(q) = question.questions.get_mut(question.question_index)
                                {
                                    if !q.options.is_empty() {
                                        q.option_cursor = if q.option_cursor == 0 {
                                            q.options.len().saturating_sub(1)
                                        } else {
                                            q.option_cursor.saturating_sub(1)
                                        };
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Action::FileSearchInput(c) => {
                self.file_search.query.push(c);
                self.refresh_file_search_matches();
            }
            Action::FileSearchBackspace => {
                self.file_search.query.pop();
                self.refresh_file_search_matches();
            }
            Action::FileSearchSelectNext => {
                if !self.file_search.matches.is_empty() {
                    self.file_search.cursor =
                        (self.file_search.cursor + 1) % self.file_search.matches.len();
                }
            }
            Action::FileSearchSelectPrev => {
                if !self.file_search.matches.is_empty() {
                    self.file_search.cursor = if self.file_search.cursor == 0 {
                        self.file_search.matches.len().saturating_sub(1)
                    } else {
                        self.file_search.cursor.saturating_sub(1)
                    };
                }
            }
            Action::FileSearchConfirm => {
                if let Some(selected) = self.file_search.matches.get(self.file_search.cursor) {
                    if let AppState::Chat { command_input, .. } = &mut self.state {
                        if !command_input.text().is_empty() {
                            command_input.insert_char(' ');
                        }
                        command_input.insert_str(&format!("@{}", selected));
                    }
                    if let AppState::Chat { modal, .. } = &mut self.state {
                        *modal = None;
                    }
                    self.sync_active_agent_from_chat();
                }
            }
            Action::RequestToggleCurrent => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        match &mut request.kind {
                            PendingRequestKind::Permission(_) => {
                                *permission_choice = (*permission_choice + 1) % 3;
                            }
                            PendingRequestKind::Question(question) => {
                                if let Some(q) = question.questions.get_mut(question.question_index)
                                {
                                    if q.option_cursor < q.options.len() {
                                        if q.multiple {
                                            if let Some(existing) = q
                                                .selected_options
                                                .iter()
                                                .position(|v| *v == q.option_cursor)
                                            {
                                                q.selected_options.remove(existing);
                                            } else {
                                                q.selected_options.push(q.option_cursor);
                                            }
                                        } else {
                                            q.selected_options = vec![q.option_cursor];
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Action::RequestDigit(digit) => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        match &mut request.kind {
                            PendingRequestKind::Permission(_) => {
                                if (1..=3).contains(&digit) {
                                    *permission_choice = digit as usize - 1;
                                }
                            }
                            PendingRequestKind::Question(question) => {
                                let idx = digit.saturating_sub(1) as usize;
                                if let Some(q) = question.questions.get_mut(question.question_index)
                                {
                                    if idx < q.options.len() {
                                        q.option_cursor = idx;
                                        if q.multiple {
                                            if let Some(existing) =
                                                q.selected_options.iter().position(|v| *v == idx)
                                            {
                                                q.selected_options.remove(existing);
                                            } else {
                                                q.selected_options.push(idx);
                                            }
                                        } else {
                                            q.selected_options = vec![idx];
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Action::RequestInput(c) => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        if let PendingRequestKind::Question(question) = &mut request.kind {
                            if let Some(q) = question.questions.get_mut(question.question_index) {
                                if q.custom || !q.options.is_empty() {
                                    q.custom_input.push(c);
                                }
                            }
                        }
                    }
                }
            }
            Action::RequestBackspace => {
                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        if let PendingRequestKind::Question(question) = &mut request.kind {
                            if let Some(q) = question.questions.get_mut(question.question_index) {
                                q.custom_input.pop();
                            }
                        }
                    }
                }
            }
            Action::PlanWizardNextField => {
                if let AppState::Chat { plan_wizard, .. } = &mut self.state {
                    plan_wizard.cursor_step = (plan_wizard.cursor_step + 1) % 5;
                }
            }
            Action::PlanWizardPrevField => {
                if let AppState::Chat { plan_wizard, .. } = &mut self.state {
                    plan_wizard.cursor_step = if plan_wizard.cursor_step == 0 {
                        4
                    } else {
                        plan_wizard.cursor_step.saturating_sub(1)
                    };
                }
            }
            Action::PlanWizardInput(c) => {
                if let AppState::Chat { plan_wizard, .. } = &mut self.state {
                    let target = match plan_wizard.cursor_step {
                        0 => &mut plan_wizard.plan_name,
                        1 => &mut plan_wizard.scope,
                        2 => &mut plan_wizard.constraints,
                        3 => &mut plan_wizard.priorities,
                        _ => &mut plan_wizard.notes,
                    };
                    target.push(c);
                }
            }
            Action::PlanWizardBackspace => {
                if let AppState::Chat { plan_wizard, .. } = &mut self.state {
                    let target = match plan_wizard.cursor_step {
                        0 => &mut plan_wizard.plan_name,
                        1 => &mut plan_wizard.scope,
                        2 => &mut plan_wizard.constraints,
                        3 => &mut plan_wizard.priorities,
                        _ => &mut plan_wizard.notes,
                    };
                    target.pop();
                }
            }
            Action::PlanWizardSubmit => {
                let (follow_up, needs_clarification_question) =
                    if let AppState::Chat { plan_wizard, .. } = &self.state {
                        (
                            plan_helpers::build_plan_feedback_markdown(plan_wizard),
                            Self::plan_feedback_needs_clarification(plan_wizard),
                        )
                    } else {
                        (String::new(), false)
                    };
                if !follow_up.trim().is_empty() {
                    if let AppState::Chat {
                        command_input,
                        modal,
                        plan_waiting_for_clarification_question,
                        ..
                    } = &mut self.state
                    {
                        *modal = None;
                        command_input.set_text(follow_up);
                        *plan_waiting_for_clarification_question =
                            matches!(self.current_mode, TandemMode::Plan)
                                && needs_clarification_question;
                    }
                    self.sync_active_agent_from_chat();
                    if let Some(tx) = &self.action_tx {
                        let _ = tx.send(Action::SubmitCommand);
                    }
                }
                self.open_queued_plan_agent_prompt();
            }
            Action::RequestReject => {
                let (request_id, reject_kind, question_permission_id) = if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    ..
                } = &self.state
                {
                    if let Some(request) = pending_requests.get(*request_cursor) {
                        match &request.kind {
                            PendingRequestKind::Permission(permission) => {
                                (Some(permission.id.clone()), Some("permission"), None)
                            }
                            PendingRequestKind::Question(question) => (
                                Some(question.id.clone()),
                                Some("question"),
                                question.permission_request_id.clone(),
                            ),
                        }
                    } else {
                        (None, None, None)
                    }
                } else {
                    (None, None, None)
                };
                if let (Some(request_id), Some(kind)) = (request_id, reject_kind) {
                    if let Some(client) = &self.client {
                        match kind {
                            "permission" => {
                                let _ = client.reply_permission(&request_id, "deny").await;
                            }
                            "question" => {
                                if let Some(permission_id) = question_permission_id {
                                    let _ = client.reply_permission(&permission_id, "deny").await;
                                }
                                let _ = client.reject_question(&request_id).await;
                            }
                            _ => {}
                        }
                    }
                    if let AppState::Chat {
                        pending_requests,
                        request_cursor,
                        modal,
                        ..
                    } = &mut self.state
                    {
                        pending_requests.retain(|request| match &request.kind {
                            PendingRequestKind::Permission(permission) => {
                                permission.id != request_id
                            }
                            PendingRequestKind::Question(question) => question.id != request_id,
                        });
                        if pending_requests.is_empty() {
                            *request_cursor = 0;
                            *modal = None;
                        } else if *request_cursor >= pending_requests.len() {
                            *request_cursor = pending_requests.len().saturating_sub(1);
                        }
                    }
                }
            }
            Action::RequestConfirm => {
                let mut remove_request_id: Option<String> = None;
                let mut permission_reply: Option<String> = None;
                let mut question_reply: Option<(String, Vec<Vec<String>>)> = None;
                let mut question_reply_preview: Option<String> = None;
                let mut question_permission_once: Option<String> = None;
                let mut approved_task_payload: Option<(String, Option<Value>)> = None;
                let mut approved_request_id: Option<String> = None;
                let mut question_request_target: Option<(String, String)> = None;

                if let AppState::Chat {
                    pending_requests,
                    request_cursor,
                    permission_choice,
                    ..
                } = &mut self.state
                {
                    if let Some(request) = pending_requests.get_mut(*request_cursor) {
                        let req_session_id = request.session_id.clone();
                        let req_agent_id = request.agent_id.clone();
                        match &mut request.kind {
                            PendingRequestKind::Permission(permission) => {
                                let reply = match *permission_choice {
                                    0 => "once",
                                    1 => "always",
                                    _ => "deny",
                                };
                                remove_request_id = Some(permission.id.clone());
                                permission_reply = Some(reply.to_string());
                                approved_request_id = Some(permission.id.clone());
                                if reply != "deny"
                                    && plan_helpers::is_task_tool_name(&permission.tool)
                                {
                                    approved_task_payload =
                                        Some((permission.tool.clone(), permission.args.clone()));
                                }
                            }
                            PendingRequestKind::Question(question) => {
                                let can_advance = if let Some(q) =
                                    question.questions.get_mut(question.question_index)
                                {
                                    // If the user highlighted an option but did not explicitly toggle
                                    // it, Enter should accept the highlighted choice.
                                    if q.selected_options.is_empty()
                                        && !q.options.is_empty()
                                        && q.option_cursor < q.options.len()
                                    {
                                        if q.multiple {
                                            q.selected_options.push(q.option_cursor);
                                        } else {
                                            q.selected_options = vec![q.option_cursor];
                                        }
                                    }
                                    !q.selected_options.is_empty()
                                        || !q.custom_input.trim().is_empty()
                                } else {
                                    false
                                };
                                if can_advance {
                                    if question.question_index + 1 < question.questions.len() {
                                        question.question_index += 1;
                                    } else {
                                        let mut answers: Vec<Vec<String>> = Vec::new();
                                        let mut answer_preview_lines: Vec<String> = Vec::new();
                                        for q in &question.questions {
                                            let mut question_answers = Vec::new();
                                            for idx in &q.selected_options {
                                                if let Some(option) = q.options.get(*idx) {
                                                    question_answers.push(option.label.clone());
                                                }
                                            }
                                            let custom = q.custom_input.trim();
                                            if !custom.is_empty() {
                                                question_answers.push(custom.to_string());
                                            }
                                            if question_answers.is_empty() {
                                                question_answers.push(String::new());
                                            }
                                            let preview_text = question_answers
                                                .iter()
                                                .filter(|s| !s.trim().is_empty())
                                                .cloned()
                                                .collect::<Vec<_>>()
                                                .join(" | ");
                                            answer_preview_lines.push(if preview_text.is_empty() {
                                                "- (empty)".to_string()
                                            } else {
                                                format!("- {}", preview_text)
                                            });
                                            answers.push(question_answers);
                                        }
                                        if !answer_preview_lines.is_empty() {
                                            question_reply_preview = Some(format!(
                                                "Submitted question answers:\n{}",
                                                answer_preview_lines.join("\n")
                                            ));
                                        }
                                        remove_request_id = Some(question.id.clone());
                                        if let Some(permission_id) =
                                            question.permission_request_id.clone()
                                        {
                                            question_permission_once = Some(permission_id);
                                        }
                                        question_reply = Some((question.id.clone(), answers));
                                        question_request_target =
                                            Some((req_session_id, req_agent_id));
                                    }
                                }
                            }
                        }
                    }
                }

                if let Some(client) = &self.client {
                    if let (Some(request_id), Some(reply)) =
                        (remove_request_id.clone(), permission_reply.clone())
                    {
                        let _ = client.reply_permission(&request_id, &reply).await;
                    }
                    if let Some(permission_id) = question_permission_once.clone() {
                        let _ = client.reply_permission(&permission_id, "once").await;
                    }
                    if let Some((question_id, answers)) = question_reply.clone() {
                        let _ = client.reply_question(&question_id, answers).await;
                    }
                }

                if let Some(request_id) = remove_request_id {
                    if permission_reply.is_some() || question_reply.is_some() {
                        if let AppState::Chat {
                            pending_requests,
                            request_cursor,
                            modal,
                            ..
                        } = &mut self.state
                        {
                            pending_requests.retain(|request| match &request.kind {
                                PendingRequestKind::Permission(permission) => {
                                    permission.id != request_id
                                }
                                PendingRequestKind::Question(question) => question.id != request_id,
                            });
                            if pending_requests.is_empty() {
                                *request_cursor = 0;
                                *modal = None;
                            } else if *request_cursor >= pending_requests.len() {
                                *request_cursor = pending_requests.len().saturating_sub(1);
                            }
                        }
                    }
                }
                if let Some(preview) = question_reply_preview {
                    if let AppState::Chat { messages, .. } = &mut self.state {
                        messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: vec![ContentBlock::Text(preview)],
                        });
                    }
                    self.sync_active_agent_from_chat();
                }

                if let Some((tool, args)) = approved_task_payload {
                    let fingerprint = plan_helpers::plan_fingerprint_from_args(args.as_ref());
                    let preview = plan_helpers::plan_preview_from_args(args.as_ref());
                    let should_open_wizard = if let AppState::Chat {
                        last_plan_task_fingerprint,
                        ..
                    } = &self.state
                    {
                        plan_helpers::is_todo_write_tool_name(&tool)
                            && !fingerprint.is_empty()
                            && *last_plan_task_fingerprint != fingerprint
                    } else {
                        false
                    };

                    if let AppState::Chat {
                        tasks,
                        active_task_id,
                        plan_wizard,
                        modal,
                        last_plan_task_fingerprint,
                        ..
                    } = &mut self.state
                    {
                        plan_helpers::apply_task_payload(
                            tasks,
                            active_task_id,
                            &tool,
                            args.as_ref(),
                        );
                        if plan_helpers::is_todo_write_tool_name(&tool) && !fingerprint.is_empty() {
                            *last_plan_task_fingerprint = fingerprint;
                        }
                        if should_open_wizard {
                            *modal = Some(ModalState::PlanFeedbackWizard);
                            *plan_wizard = PlanFeedbackWizardState {
                                plan_name: String::new(),
                                scope: String::new(),
                                constraints: String::new(),
                                priorities: String::new(),
                                notes: String::new(),
                                cursor_step: 0,
                                source_request_id: approved_request_id.clone(),
                                task_preview: preview,
                            };
                        }
                    }
                    if plan_helpers::is_todo_write_tool_name(&tool)
                        && matches!(self.current_mode, TandemMode::Plan)
                    {
                        self.queue_plan_agent_prompt(4);
                    }
                    self.sync_active_agent_from_chat();
                }

                if question_reply.is_some() && matches!(self.current_mode, TandemMode::Plan) {
                    if let Some((session_id, agent_id)) = question_request_target {
                        let follow_up = "Continue plan mode with the answered questions. Update `todowrite` tasks and statuses now, then ask for approval before execution.".to_string();
                        let mut queued = false;
                        if let AppState::Chat {
                            agents, messages, ..
                        } = &mut self.state
                        {
                            if let Some(agent) = agents
                                .iter_mut()
                                .find(|a| a.session_id == session_id && a.agent_id == agent_id)
                            {
                                if Self::is_agent_busy(&agent.status) {
                                    let merged_into_existing = !agent.follow_up_queue.is_empty();
                                    if merged_into_existing {
                                        if let Some(last) = agent.follow_up_queue.back_mut() {
                                            if !last.is_empty() {
                                                last.push('\n');
                                            }
                                            last.push_str(&follow_up);
                                        }
                                    } else {
                                        agent.follow_up_queue.push_back(follow_up.clone());
                                    }
                                    queued = true;
                                    if !merged_into_existing {
                                        messages.push(ChatMessage {
                                            role: MessageRole::System,
                                            content: vec![ContentBlock::Text(format!(
                                                "Queued follow-up message (#{}).",
                                                agent.follow_up_queue.len()
                                            ))],
                                        });
                                    }
                                }
                            }
                        }
                        if queued {
                            self.sync_active_agent_from_chat();
                        } else {
                            self.dispatch_prompt_for_agent(session_id, agent_id, follow_up);
                        }
                    }
                }
            }
            Action::NewAgent => {
                self.sync_active_agent_from_chat();
                let next_agent_id = if let AppState::Chat { agents, .. } = &self.state {
                    format!("A{}", agents.len() + 1)
                } else {
                    "A1".to_string()
                };
                let mut new_session_id: Option<String> = None;
                if let Some(client) = &self.client {
                    if let Ok(session) = client
                        .create_session(Some(format!("{} session", next_agent_id)))
                        .await
                    {
                        new_session_id = Some(session.id);
                    }
                }
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    let fallback_session = agents
                        .get(*active_agent_index)
                        .map(|a| a.session_id.clone())
                        .unwrap_or_default();
                    let pane = Self::make_agent_pane(
                        next_agent_id,
                        new_session_id.unwrap_or(fallback_session),
                    );
                    agents.push(pane);
                    *active_agent_index = agents.len().saturating_sub(1);
                }
                self.sync_chat_from_active_agent();
            }
            Action::CloseActiveAgent => {
                self.sync_active_agent_from_chat();
                let mut confirm = None;
                if let AppState::Chat {
                    agents,
                    active_agent_index,
                    modal,
                    ..
                } = &mut self.state
                {
                    if let Some(agent) = agents.get(*active_agent_index) {
                        if !agent.draft.text().trim().is_empty() {
                            confirm = Some(agent.agent_id.clone());
                        }
                    }
                    if let Some(agent_id) = confirm.clone() {
                        *modal = Some(ModalState::ConfirmCloseAgent {
                            target_agent_id: agent_id,
                        });
                    }
                }
                if confirm.is_none() {
                    let active_idx = if let AppState::Chat {
                        active_agent_index, ..
                    } = &self.state
                    {
                        *active_agent_index
                    } else {
                        0
                    };
                    self.cancel_agent_if_running(active_idx).await;
                    if let AppState::Chat {
                        agents,
                        modal,
                        active_agent_index,
                        grid_page,
                        ..
                    } = &mut self.state
                    {
                        if agents.len() <= 1 {
                            let replacement = Self::make_agent_pane(
                                "A1".to_string(),
                                agents
                                    .first()
                                    .map(|a| a.session_id.clone())
                                    .unwrap_or_default(),
                            );
                            agents.clear();
                            agents.push(replacement);
                        } else {
                            agents.remove(active_idx);
                            if *active_agent_index >= agents.len() {
                                *active_agent_index = agents.len().saturating_sub(1);
                            }
                            let max_page = agents.len().saturating_sub(1) / 4;
                            if *grid_page > max_page {
                                *grid_page = max_page;
                            }
                        }
                        *modal = None;
                    }
                    self.sync_chat_from_active_agent();
                }
            }
            Action::ConfirmCloseAgent(confirmed) => {
                if !confirmed {
                    if let AppState::Chat { modal, .. } = &mut self.state {
                        *modal = None;
                    }
                } else {
                    let active_idx = if let AppState::Chat {
                        active_agent_index, ..
                    } = &self.state
                    {
                        *active_agent_index
                    } else {
                        0
                    };
                    self.cancel_agent_if_running(active_idx).await;
                    if let AppState::Chat {
                        agents,
                        modal,
                        active_agent_index,
                        grid_page,
                        ..
                    } = &mut self.state
                    {
                        if agents.len() <= 1 {
                            let replacement = Self::make_agent_pane(
                                "A1".to_string(),
                                agents
                                    .first()
                                    .map(|a| a.session_id.clone())
                                    .unwrap_or_default(),
                            );
                            agents.clear();
                            agents.push(replacement);
                        } else {
                            agents.remove(active_idx);
                            if *active_agent_index >= agents.len() {
                                *active_agent_index = agents.len().saturating_sub(1);
                            }
                            let max_page = agents.len().saturating_sub(1) / 4;
                            if *grid_page > max_page {
                                *grid_page = max_page;
                            }
                        }
                        *modal = None;
                    }
                    self.sync_chat_from_active_agent();
                }
            }
            Action::ConfirmStartPlanAgents { confirmed, count } => {
                if let AppState::Chat {
                    modal,
                    plan_multi_agent_prompt,
                    ..
                } = &mut self.state
                {
                    *modal = None;
                    *plan_multi_agent_prompt = None;
                }
                if confirmed {
                    let created = self.ensure_agent_count(count).await;
                    if created > 0 {
                        if let AppState::Chat { messages, .. } = &mut self.state {
                            messages.push(ChatMessage {
                                role: MessageRole::System,
                                content: vec![ContentBlock::Text(format!(
                                    "Opened {} agent{} for plan execution.",
                                    created,
                                    if created == 1 { "" } else { "s" }
                                ))],
                            });
                        }
                    }
                }
                self.sync_chat_from_active_agent();
            }
            Action::CancelActiveAgent => {
                let mut cancel_idx: Option<usize> = None;
                if let AppState::Chat {
                    modal,
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    if modal.is_some() {
                        *modal = None;
                    } else if let Some(agent) = agents.get_mut(*active_agent_index) {
                        if matches!(agent.status, AgentStatus::Running | AgentStatus::Streaming) {
                            agent.status = AgentStatus::Cancelling;
                            cancel_idx = Some(*active_agent_index);
                        } else {
                            self.state = AppState::MainMenu;
                        }
                    }
                }
                if let Some(idx) = cancel_idx {
                    self.cancel_agent_if_running(idx).await;
                    if let AppState::Chat { agents, .. } = &mut self.state {
                        if let Some(agent) = agents.get_mut(idx) {
                            agent.status = AgentStatus::Idle;
                            agent.active_run_id = None;
                        }
                    }
                    self.sync_chat_from_active_agent();
                }
            }
            Action::StartDemoStream => {
                if let Some(tx) = &self.action_tx {
                    if let Some(agent) = self.active_agent_clone() {
                        let agent_id = agent.agent_id;
                        let session_id = agent.session_id;
                        let tx = tx.clone();
                        tokio::spawn(async move {
                            let _ = tx.send(Action::PromptRunStarted {
                                session_id: session_id.clone(),
                                agent_id: agent_id.clone(),
                                run_id: Some(format!(
                                    "demo-{}",
                                    std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .map(|d| d.as_millis())
                                        .unwrap_or(0)
                                )),
                            });
                            let tokens = ["demo ", "stream ", "for ", "active ", "agent"];
                            for t in tokens {
                                let _ = tx.send(Action::PromptDelta {
                                    session_id: session_id.clone(),
                                    agent_id: agent_id.clone(),
                                    delta: t.to_string(),
                                });
                                tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                            }
                        });
                    }
                }
            }
            Action::SpawnBackgroundDemo => {
                self.sync_active_agent_from_chat();
                let previous_active = if let AppState::Chat {
                    active_agent_index, ..
                } = &self.state
                {
                    *active_agent_index
                } else {
                    0
                };
                let next_agent_id = if let AppState::Chat { agents, .. } = &self.state {
                    format!("A{}", agents.len() + 1)
                } else {
                    "A1".to_string()
                };
                let mut new_session_id: Option<String> = None;
                if let Some(client) = &self.client {
                    if let Ok(session) = client
                        .create_session(Some(format!("{} session", next_agent_id)))
                        .await
                    {
                        new_session_id = Some(session.id);
                    }
                }
                let (new_agent_id, new_agent_session_id) = if let AppState::Chat {
                    agents,
                    active_agent_index,
                    ..
                } = &mut self.state
                {
                    let fallback_session = agents
                        .get(*active_agent_index)
                        .map(|a| a.session_id.clone())
                        .unwrap_or_default();
                    let pane = Self::make_agent_pane(
                        next_agent_id.clone(),
                        new_session_id.unwrap_or(fallback_session),
                    );
                    agents.push(pane);
                    *active_agent_index = agents.len().saturating_sub(1);
                    let session_id = agents
                        .get(*active_agent_index)
                        .map(|a| a.session_id.clone())
                        .unwrap_or_default();
                    (next_agent_id, session_id)
                } else {
                    ("A1".to_string(), String::new())
                };
                self.sync_chat_from_active_agent();
                if let Some(tx) = &self.action_tx {
                    let tx = tx.clone();
                    tokio::spawn(async move {
                        let _ = tx.send(Action::PromptRunStarted {
                            session_id: new_agent_session_id.clone(),
                            agent_id: new_agent_id.clone(),
                            run_id: Some(format!(
                                "demo-{}",
                                std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .map(|d| d.as_millis())
                                    .unwrap_or(0)
                            )),
                        });
                        let tokens = ["background ", "demo ", "stream"];
                        for t in tokens {
                            let _ = tx.send(Action::PromptDelta {
                                session_id: new_agent_session_id.clone(),
                                agent_id: new_agent_id.clone(),
                                delta: t.to_string(),
                            });
                            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
                        }
                    });
                }
                if let AppState::Chat {
                    active_agent_index,
                    agents,
                    ..
                } = &mut self.state
                {
                    *active_agent_index = previous_active.min(agents.len().saturating_sub(1));
                }
                self.sync_chat_from_active_agent();
            }

            Action::SetupNextStep => {
                let mut persist_provider: Option<(String, Option<String>, Option<String>)> = None;
                if let AppState::SetupWizard {
                    step,
                    provider_catalog,
                    selected_provider_index,
                    selected_model_index,
                    api_key_input,
                    model_input,
                } = &mut self.state
                {
                    match step.clone() {
                        SetupStep::Welcome => {
                            *step = SetupStep::SelectProvider;
                        }
                        SetupStep::SelectProvider => {
                            if let Some(ref catalog) = provider_catalog {
                                if *selected_provider_index < catalog.all.len() {
                                    *step = SetupStep::EnterApiKey;
                                }
                            } else {
                                *step = SetupStep::EnterApiKey;
                            }
                            model_input.clear();
                        }
                        SetupStep::EnterApiKey => {
                            if !api_key_input.is_empty() {
                                *step = SetupStep::SelectModel;
                            }
                        }
                        SetupStep::SelectModel => {
                            if let Some(ref catalog) = provider_catalog {
                                if *selected_provider_index < catalog.all.len() {
                                    let provider = &catalog.all[*selected_provider_index];
                                    let model_ids = Self::filtered_model_ids(
                                        catalog,
                                        *selected_provider_index,
                                        model_input,
                                    );
                                    let model_id = if model_ids.is_empty() {
                                        if model_input.trim().is_empty() {
                                            None
                                        } else {
                                            Some(model_input.trim().to_string())
                                        }
                                    } else {
                                        model_ids.get(*selected_model_index).cloned()
                                    };
                                    let api_key = if api_key_input.is_empty() {
                                        None
                                    } else {
                                        Some(api_key_input.clone())
                                    };
                                    persist_provider =
                                        Some((provider.id.clone(), model_id, api_key));
                                }
                            }
                            *step = SetupStep::Complete;
                        }
                        SetupStep::Complete => {
                            // Transition to MainMenu or Chat
                            self.state = AppState::MainMenu;
                        }
                    }
                }
                if let Some((provider_id, model_id, api_key)) = persist_provider {
                    self.current_provider = Some(provider_id.clone());
                    self.current_model = model_id.clone();
                    if let Some(ref key) = api_key {
                        self.save_provider_key_local(&provider_id, key);
                    }
                    self.persist_provider_defaults(
                        &provider_id,
                        model_id.as_deref(),
                        api_key.as_deref(),
                    )
                    .await;
                }
            }

            Action::SetupPrevItem => {
                if let AppState::SetupWizard {
                    step,
                    provider_catalog,
                    selected_provider_index,
                    selected_model_index,
                    model_input,
                    ..
                } = &mut self.state
                {
                    match step {
                        SetupStep::SelectProvider => {
                            if *selected_provider_index > 0 {
                                *selected_provider_index -= 1;
                            }
                            *selected_model_index = 0;
                            model_input.clear();
                        }
                        SetupStep::SelectModel => {
                            *selected_model_index = 0;
                        }
                        _ => {}
                    }

                    if let Some(catalog) = provider_catalog {
                        if *selected_provider_index >= catalog.all.len() {
                            *selected_provider_index = catalog.all.len().saturating_sub(1);
                        }
                    }
                }
            }

            Action::SetupNextItem => {
                if let AppState::SetupWizard {
                    step,
                    provider_catalog,
                    selected_provider_index,
                    selected_model_index,
                    model_input,
                    ..
                } = &mut self.state
                {
                    match step {
                        SetupStep::SelectProvider => {
                            if let Some(ref catalog) = provider_catalog {
                                if *selected_provider_index < catalog.all.len() - 1 {
                                    *selected_provider_index += 1;
                                }
                            }
                            model_input.clear();
                        }
                        SetupStep::SelectModel => {
                            if let Some(ref catalog) = provider_catalog {
                                if *selected_provider_index < catalog.all.len() {
                                    let model_ids = Self::filtered_model_ids(
                                        catalog,
                                        *selected_provider_index,
                                        model_input,
                                    );
                                    if !model_ids.is_empty()
                                        && *selected_model_index < model_ids.len() - 1
                                    {
                                        *selected_model_index += 1;
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            Action::SetupInput(c) => {
                if let AppState::SetupWizard {
                    step,
                    api_key_input,
                    model_input,
                    selected_model_index,
                    provider_catalog,
                    selected_provider_index,
                    ..
                } = &mut self.state
                {
                    match step {
                        SetupStep::EnterApiKey => {
                            api_key_input.push(c);
                        }
                        SetupStep::SelectModel => {
                            model_input.push(c);
                            if let Some(catalog) = provider_catalog {
                                let model_count = Self::filtered_model_ids(
                                    catalog,
                                    *selected_provider_index,
                                    model_input,
                                )
                                .len();
                                if model_count == 0 {
                                    *selected_model_index = 0;
                                } else if *selected_model_index >= model_count {
                                    *selected_model_index = model_count.saturating_sub(1);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            Action::SetupBackspace => {
                if let AppState::SetupWizard {
                    step,
                    api_key_input,
                    model_input,
                    selected_model_index,
                    provider_catalog,
                    selected_provider_index,
                    ..
                } = &mut self.state
                {
                    match step {
                        SetupStep::EnterApiKey => {
                            api_key_input.pop();
                        }
                        SetupStep::SelectModel => {
                            model_input.pop();
                            if let Some(catalog) = provider_catalog {
                                let model_count = Self::filtered_model_ids(
                                    catalog,
                                    *selected_provider_index,
                                    model_input,
                                )
                                .len();
                                if model_count == 0 {
                                    *selected_model_index = 0;
                                } else if *selected_model_index >= model_count {
                                    *selected_model_index = model_count.saturating_sub(1);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            Action::OverlayScrollUp => {
                if let Some(overlay) = &mut self.pager_overlay {
                    overlay.scroll = overlay.scroll.saturating_sub(1);
                }
            }
            Action::OverlayScrollDown => {
                if let Some(overlay) = &mut self.pager_overlay {
                    overlay.scroll = overlay.scroll.saturating_add(1);
                }
            }
            Action::OverlayPageUp => {
                if let Some(overlay) = &mut self.pager_overlay {
                    overlay.scroll = overlay.scroll.saturating_sub(SCROLL_PAGE_STEP as usize);
                }
            }
            Action::OverlayPageDown => {
                if let Some(overlay) = &mut self.pager_overlay {
                    overlay.scroll = overlay.scroll.saturating_add(SCROLL_PAGE_STEP as usize);
                }
            }
            Action::ScrollUp => {
                if let AppState::Chat {
                    scroll_from_bottom, ..
                } = &mut self.state
                {
                    *scroll_from_bottom = scroll_from_bottom.saturating_add(SCROLL_LINE_STEP);
                }
                self.sync_active_agent_from_chat();
            }
            Action::ScrollDown => {
                if let AppState::Chat {
                    scroll_from_bottom, ..
                } = &mut self.state
                {
                    *scroll_from_bottom = scroll_from_bottom.saturating_sub(SCROLL_LINE_STEP);
                }
                self.sync_active_agent_from_chat();
            }
            Action::PageUp => {
                if let AppState::Chat {
                    scroll_from_bottom, ..
                } = &mut self.state
                {
                    *scroll_from_bottom = scroll_from_bottom.saturating_add(SCROLL_PAGE_STEP);
                }
                self.sync_active_agent_from_chat();
            }
            Action::PageDown => {
                if let AppState::Chat {
                    scroll_from_bottom, ..
                } = &mut self.state
                {
                    *scroll_from_bottom = scroll_from_bottom.saturating_sub(SCROLL_PAGE_STEP);
                }
                self.sync_active_agent_from_chat();
            }

            Action::ClearCommand => {
                if let AppState::Chat { command_input, .. } = &mut self.state {
                    command_input.clear();
                }
                self.sync_active_agent_from_chat();
            }

            Action::QueueSteeringFromComposer => {
                let mut queue_note: Option<String> = None;
                let mut queue_error: Option<String> = None;
                let mut should_cancel_active = false;
                let mut should_dispatch_now = false;
                if let AppState::Chat {
                    command_input,
                    agents,
                    active_agent_index,
                    messages,
                    ..
                } = &mut self.state
                {
                    let raw = command_input.text().to_string();
                    if raw.trim().is_empty() {
                        return Ok(());
                    }
                    let msg = raw.trim().to_string();
                    if let Some(agent) = agents.get_mut(*active_agent_index) {
                        match Self::expand_paste_markers_checked(&msg, agent) {
                            Ok(expanded) => {
                                command_input.clear();
                                if Self::is_agent_busy(&agent.status) {
                                    agent.steering_message = Some(expanded);
                                    agent.follow_up_queue.clear();
                                    should_cancel_active = agent.active_run_id.is_some();
                                    queue_note = Some(
                                        "Steering message queued. Current run will be interrupted."
                                            .to_string(),
                                    );
                                } else {
                                    command_input.set_text(expanded);
                                    should_dispatch_now = true;
                                }
                            }
                            Err(err) => queue_error = Some(err),
                        }
                    }
                    if let Some(err) = queue_error.clone() {
                        messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: vec![ContentBlock::Text(err)],
                        });
                    }
                    if let Some(note) = queue_note.clone() {
                        messages.push(ChatMessage {
                            role: MessageRole::System,
                            content: vec![ContentBlock::Text(note)],
                        });
                    }
                }
                self.sync_active_agent_from_chat();
                if should_cancel_active {
                    let active_idx = if let AppState::Chat {
                        active_agent_index, ..
                    } = &self.state
                    {
                        *active_agent_index
                    } else {
                        0
                    };
                    self.cancel_agent_if_running(active_idx).await;
                }
                if should_dispatch_now {
                    if let Some(tx) = &self.action_tx {
                        let _ = tx.send(Action::SubmitCommand);
                    }
                }
            }

            Action::SubmitCommand => {
                let (session_id, active_agent_id, msg_to_send, queued_followup) =
                    if let AppState::Chat {
                        session_id,
                        command_input,
                        agents,
                        active_agent_index,
                        plan_awaiting_approval,
                        messages,
                        ..
                    } = &mut self.state
                    {
                        let raw = command_input.text().to_string();
                        if raw.trim().is_empty() {
                            return Ok(());
                        }
                        let msg = raw.trim().to_string();
                        let mut agent_id = "A1".to_string();
                        let mut queued = false;
                        let mut msg_to_send: Option<String> = None;
                        let mut blocked_error: Option<String> = None;
                        if let Some(agent) = agents.get_mut(*active_agent_index) {
                            agent_id = agent.agent_id.clone();
                            match Self::expand_paste_markers_checked(&msg, agent) {
                                Ok(expanded) => {
                                    command_input.clear();
                                    if Self::is_agent_busy(&agent.status) {
                                        let merged_into_existing =
                                            !agent.follow_up_queue.is_empty();
                                        if merged_into_existing {
                                            if let Some(last) = agent.follow_up_queue.back_mut() {
                                                if !last.is_empty() {
                                                    last.push('\n');
                                                }
                                                last.push_str(&expanded);
                                            }
                                        } else {
                                            agent.follow_up_queue.push_back(expanded);
                                        }
                                        queued = true;
                                        if !merged_into_existing {
                                            messages.push(ChatMessage {
                                                role: MessageRole::System,
                                                content: vec![ContentBlock::Text(format!(
                                                    "Queued follow-up message (#{}).",
                                                    agent.follow_up_queue.len()
                                                ))],
                                            });
                                        }
                                    } else {
                                        msg_to_send = Some(expanded);
                                    }
                                }
                                Err(err) => {
                                    blocked_error = Some(err);
                                    command_input.set_text(msg.clone());
                                }
                            }
                        }
                        if let Some(err) = blocked_error {
                            messages.push(ChatMessage {
                                role: MessageRole::System,
                                content: vec![ContentBlock::Text(err)],
                            });
                        }
                        *plan_awaiting_approval = false;
                        (session_id.clone(), agent_id, msg_to_send, queued)
                    } else {
                        (String::new(), "A1".to_string(), None, false)
                    };
                if queued_followup {
                    self.sync_active_agent_from_chat();
                    return Ok(());
                }

                if let Some(msg) = msg_to_send {
                    let is_single_line = !msg.contains('\n');
                    if is_single_line && msg.starts_with("/tool ") {
                        // Pass through engine-native tool invocation syntax.
                        // The engine loop handles permission and execution for /tool.
                        self.dispatch_prompt_for_agent(
                            session_id.clone(),
                            active_agent_id.clone(),
                            msg.clone(),
                        );
                    } else if is_single_line && msg.starts_with('/') {
                        let response = self.execute_command(&msg).await;
                        if let AppState::Chat { messages, .. } = &mut self.state {
                            messages.push(ChatMessage {
                                role: MessageRole::System,
                                content: vec![ContentBlock::Text(response)],
                            });
                        }
                        self.sync_active_agent_from_chat();
                    } else if let Some(provider_id) = self.pending_model_provider.clone() {
                        let model_id = msg.trim().to_string();
                        if model_id.is_empty() {
                            if let AppState::Chat { messages, .. } = &mut self.state {
                                messages.push(ChatMessage {
                                    role: MessageRole::System,
                                    content: vec![ContentBlock::Text(
                                        "Model cannot be empty. Paste a model name.".to_string(),
                                    )],
                                });
                            }
                        } else {
                            self.pending_model_provider = None;
                            self.current_provider = Some(provider_id.clone());
                            self.current_model = Some(model_id.clone());
                            self.persist_provider_defaults(&provider_id, Some(&model_id), None)
                                .await;
                            if let AppState::Chat { messages, .. } = &mut self.state {
                                messages.push(ChatMessage {
                                    role: MessageRole::System,
                                    content: vec![ContentBlock::Text(format!(
                                        "Provider set to {} with model {}.",
                                        provider_id, model_id
                                    ))],
                                });
                            }
                            self.sync_active_agent_from_chat();
                        }
                    } else {
                        if let Some(provider_id) = self.current_provider.clone() {
                            if !self.provider_is_connected(&provider_id)
                                && self.open_key_wizard_for_provider(&provider_id)
                            {
                                if let AppState::Chat { messages, .. } = &mut self.state {
                                    messages.push(ChatMessage {
                                        role: MessageRole::System,
                                        content: vec![ContentBlock::Text(format!(
                                            "Provider '{}' has no configured key. Enter API key in setup wizard to continue.",
                                            provider_id
                                        ))],
                                    });
                                }
                                self.sync_active_agent_from_chat();
                                return Ok(());
                            }
                        }
                        self.dispatch_prompt_for_agent(
                            session_id.clone(),
                            active_agent_id.clone(),
                            msg.clone(),
                        );
                    }
                }
            }

            Action::PromptRunStarted {
                session_id: event_session_id,
                agent_id,
                run_id,
            } => self.handle_prompt_run_started(event_session_id, agent_id, run_id),
            Action::PromptSuccess {
                session_id: event_session_id,
                agent_id,
                messages: new_messages,
            } => self.handle_prompt_success(event_session_id, agent_id, new_messages),
            Action::PromptTodoUpdated {
                session_id: event_session_id,
                todos,
            } => {
                self.handle_prompt_todo_updated(event_session_id, todos)
                    .await
            }
            Action::PromptAgentTeamEvent {
                session_id: event_session_id,
                agent_id,
                event,
            } => {
                self.handle_prompt_agent_team_event(event_session_id, agent_id, event)
                    .await
            }
            Action::PromptDelta {
                session_id: event_session_id,
                agent_id,
                delta,
            } => self.handle_prompt_delta(event_session_id, agent_id, delta),
            Action::PromptInfo {
                session_id: event_session_id,
                agent_id,
                message,
            } => self.handle_prompt_info(event_session_id, agent_id, message),
            Action::PromptToolDelta {
                session_id: event_session_id,
                agent_id,
                tool_call_id,
                tool_name,
                args_delta: _,
                args_preview,
            } => self.handle_prompt_tool_delta(
                event_session_id,
                agent_id,
                tool_call_id,
                tool_name,
                args_preview,
            ),
            Action::PromptMalformedQuestion {
                session_id: event_session_id,
                agent_id,
                request_id,
            } => {
                self.handle_prompt_malformed_question(event_session_id, agent_id, request_id)
                    .await
            }
            Action::PromptRequest {
                session_id: event_session_id,
                agent_id,
                request,
            } => {
                self.handle_prompt_request(event_session_id, agent_id, request)
                    .await
            }
            Action::PromptRequestResolved { request_id, .. } => {
                self.handle_prompt_request_resolved(request_id)
            }
            Action::PromptFailure {
                session_id: event_session_id,
                agent_id,
                error,
            } => self.handle_prompt_failure(event_session_id, agent_id, error),

            _ => {}
        }
        Ok(())
    }

    pub async fn tick(&mut self) {
        self.tick_count += 1;

        // Check engine health every ~1 second (assuming 60tps)
        if self.tick_count % 60 == 0 {
            if let Some(client) = &self.client {
                match client.check_health().await {
                    Ok(true) => self.engine_health = EngineConnectionStatus::Connected,
                    _ => self.engine_health = EngineConnectionStatus::Error,
                }
            } else {
                self.engine_health = EngineConnectionStatus::Disconnected;
            }
        }

        match &mut self.state {
            AppState::StartupAnimation { frame } => {
                *frame += 1;
                // Update matrix with real terminal size
                if let Ok((w, h)) = crossterm::terminal::size() {
                    self.matrix.update(w, h);
                } else {
                    self.matrix.update(120, 50);
                }

                if !self.startup_engine_bootstrap_done {
                    if let Some(retry_at) = self.engine_download_retry_at {
                        if retry_at > Instant::now() {
                            let wait_secs = retry_at
                                .saturating_duration_since(Instant::now())
                                .as_secs()
                                .max(1);
                            self.connection_status =
                                format!("Engine download failed. Retrying in {}s...", wait_secs);
                            return;
                        }
                    }
                    self.connection_status = "Preparing engine bootstrap...".to_string();
                    match self.ensure_engine_binary().await {
                        Ok(_) => {
                            self.startup_engine_bootstrap_done = true;
                            self.connection_status =
                                "Engine ready. Press Enter to continue.".to_string();
                        }
                        Err(err) => {
                            tracing::warn!("TUI engine bootstrap failed: {}", err);
                            self.engine_download_active = false;
                            self.engine_download_last_error = Some(err.to_string());
                            self.engine_download_retry_at =
                                Some(Instant::now() + std::time::Duration::from_secs(5));
                            self.connection_status = format!("Engine download failed: {}", err);
                        }
                    }
                }
            }
            AppState::PinPrompt { .. } => {
                if let Ok((w, h)) = crossterm::terminal::size() {
                    self.matrix.update(w, h);
                } else {
                    self.matrix.update(120, 50);
                }
            }

            AppState::Connecting => {
                // Continue matrix rain animation
                if let Ok((w, h)) = crossterm::terminal::size() {
                    self.matrix.update(w, h);
                } else {
                    self.matrix.update(120, 50);
                }

                // Try to connect or spawn
                if self.client.is_none() {
                    if let Some(child) = self.engine_process.as_mut() {
                        match child.try_wait() {
                            Ok(Some(status)) => {
                                self.engine_process = None;
                                self.engine_spawned_at = None;
                                self.engine_base_url_override = None;
                                self.engine_connection_source = EngineConnectionSource::Unknown;
                                self.connection_status =
                                    format!("Managed engine exited ({}). Restarting...", status);
                            }
                            Ok(None) => {}
                            Err(err) => {
                                self.engine_process = None;
                                self.engine_spawned_at = None;
                                self.engine_base_url_override = None;
                                self.engine_connection_source = EngineConnectionSource::Unknown;
                                self.connection_status =
                                    format!("Engine process check failed ({}). Restarting...", err);
                            }
                        }
                    }
                    self.connection_status = "Searching for engine...".to_string();
                    // Check if running
                    let client = EngineClient::new_with_token(
                        self.engine_target_base_url(),
                        self.engine_api_token.clone(),
                    );
                    if let Ok(status) = client.get_engine_status().await {
                        if status.healthy {
                            let required = Self::desired_engine_version();
                            let connected = Self::parse_semver_triplet(&status.version);
                            let stale = match (required, connected) {
                                (Some(required), Some(connected)) => connected < required,
                                _ => false,
                            };
                            if stale {
                                let policy = EngineStalePolicy::from_env();
                                let required_text = required
                                    .map(Self::format_semver_triplet)
                                    .unwrap_or_else(|| "unknown".to_string());
                                let connected_text = connected
                                    .map(Self::format_semver_triplet)
                                    .unwrap_or_else(|| status.version.clone());
                                match policy {
                                    EngineStalePolicy::AutoReplace => {
                                        self.connection_status = format!(
                                            "Found stale engine {} (required {}). Starting fresh managed engine...",
                                            connected_text, required_text
                                        );
                                        self.client = None;
                                    }
                                    EngineStalePolicy::Fail => {
                                        self.connection_status = format!(
                                            "Detected stale engine {} (required {}). Set TANDEM_ENGINE_STALE_POLICY=auto_replace or run /engine restart.",
                                            connected_text, required_text
                                        );
                                        return;
                                    }
                                    EngineStalePolicy::Warn => {
                                        self.connection_status = format!(
                                            "Warning: stale engine {} (required {}), continuing due to TANDEM_ENGINE_STALE_POLICY=warn.",
                                            connected_text, required_text
                                        );
                                        self.engine_connection_source =
                                            EngineConnectionSource::SharedAttached;
                                        self.engine_spawned_at = None;
                                        self.client = Some(client.clone());
                                        let _ = self.finalize_connecting(&client).await;
                                        return;
                                    }
                                }
                            } else {
                                self.connection_status =
                                    "Connected. Verifying readiness...".to_string();
                                self.engine_connection_source =
                                    EngineConnectionSource::SharedAttached;
                                self.engine_spawned_at = None;
                                self.client = Some(client.clone());
                                let _ = self.finalize_connecting(&client).await;
                                return;
                            }
                        }
                    }

                    // If not running and no process spawned, spawn it
                    if self.engine_process.is_none() {
                        self.connection_status = "Starting engine...".to_string();
                        if let Some(retry_at) = self.engine_download_retry_at {
                            if retry_at > Instant::now() {
                                let wait_secs = retry_at
                                    .saturating_duration_since(Instant::now())
                                    .as_secs()
                                    .max(1);
                                self.connection_status = format!(
                                    "Engine download failed. Retrying in {}s...",
                                    wait_secs
                                );
                                return;
                            }
                        }
                        let engine_binary = match self.ensure_engine_binary().await {
                            Ok(path) => path,
                            Err(err) => {
                                tracing::warn!("TUI could not prepare engine binary: {}", err);
                                self.engine_download_active = false;
                                self.engine_download_last_error = Some(err.to_string());
                                self.engine_download_retry_at =
                                    Some(Instant::now() + std::time::Duration::from_secs(5));
                                self.connection_status = format!("Engine download failed: {}", err);
                                return;
                            }
                        };

                        let mut spawned = false;
                        let spawn_port = Self::pick_spawn_port();
                        let configured_port = spawn_port.to_string();
                        if let Some(binary_path) = engine_binary {
                            let mut cmd = Command::new(binary_path);
                            cmd.kill_on_drop(!Self::shared_engine_mode_enabled());
                            cmd.arg("serve").arg("--port").arg(&configured_port);
                            if let Some(token) = &self.engine_api_token {
                                cmd.env("TANDEM_API_TOKEN", token);
                            }
                            cmd.stdout(Stdio::null()).stderr(Stdio::null());
                            if let Ok(child) = cmd.spawn() {
                                self.engine_process = Some(child);
                                self.engine_base_url_override =
                                    Some(Self::engine_base_url_for_port(spawn_port));
                                self.engine_connection_source =
                                    EngineConnectionSource::ManagedLocal;
                                self.engine_spawned_at = Some(Instant::now());
                                spawned = true;
                            }
                        }

                        if !spawned {
                            let mut cmd = Command::new("tandem-engine");
                            cmd.kill_on_drop(!Self::shared_engine_mode_enabled());
                            cmd.arg("serve").arg("--port").arg(&configured_port);
                            if let Some(token) = &self.engine_api_token {
                                cmd.env("TANDEM_API_TOKEN", token);
                            }
                            cmd.stdout(Stdio::null()).stderr(Stdio::null());
                            if let Ok(child) = cmd.spawn() {
                                self.engine_process = Some(child);
                                self.engine_base_url_override =
                                    Some(Self::engine_base_url_for_port(spawn_port));
                                self.engine_connection_source =
                                    EngineConnectionSource::ManagedLocal;
                                self.engine_spawned_at = Some(Instant::now());
                                spawned = true;
                            }
                        }

                        if !spawned && cfg!(debug_assertions) {
                            let mut cargo_cmd = Command::new("cargo");
                            cargo_cmd.kill_on_drop(!Self::shared_engine_mode_enabled());
                            cargo_cmd
                                .arg("run")
                                .arg("-p")
                                .arg("tandem-ai")
                                .arg("--")
                                .arg("serve")
                                .arg("--port")
                                .arg(&configured_port);
                            if let Some(token) = &self.engine_api_token {
                                cargo_cmd.env("TANDEM_API_TOKEN", token);
                            }
                            cargo_cmd.stdout(Stdio::null()).stderr(Stdio::null());
                            if let Ok(child) = cargo_cmd.spawn() {
                                self.engine_process = Some(child);
                                self.engine_base_url_override =
                                    Some(Self::engine_base_url_for_port(spawn_port));
                                self.engine_connection_source =
                                    EngineConnectionSource::ManagedLocal;
                                self.engine_spawned_at = Some(Instant::now());
                                spawned = true;
                            }
                        }

                        if !spawned {
                            tracing::warn!(
                                "TUI failed to spawn tandem-engine from downloaded binary, PATH, and cargo fallback"
                            );
                            self.connection_status = "Failed to start engine.".to_string();
                        }
                    } else {
                        let timed_out = self
                            .engine_spawned_at
                            .map(|t| t.elapsed() >= std::time::Duration::from_secs(20))
                            .unwrap_or(false);
                        if timed_out {
                            self.connection_status =
                                "Engine startup timeout. Restarting managed engine...".to_string();
                            self.stop_engine_process().await;
                            self.engine_base_url_override = None;
                            self.engine_connection_source = EngineConnectionSource::Unknown;
                            self.engine_spawned_at = None;
                            return;
                        }
                        self.connection_status =
                            format!("Waiting for engine... ({})", self.engine_target_base_url());
                    }
                } else {
                    if let Some(client) = self.client.clone() {
                        if let Ok(true) = client.check_health().await {
                            let _ = self.finalize_connecting(&client).await;
                        } else {
                            self.connection_status = "Waiting for engine health...".to_string();
                        }
                    }
                }
            }
            AppState::MainMenu | AppState::Chat { .. } => {
                self.renew_engine_lease_if_due().await;
                if self.tick_count % 63 == 0 {
                    if let Some(client) = &self.client {
                        if let AppState::MainMenu = self.state {
                            if let Ok(sessions) = client.list_sessions().await {
                                self.sessions = sessions;
                            }
                        }
                        if self.provider_catalog.is_none() {
                            if let Ok(catalog) = client.list_providers().await {
                                self.provider_catalog =
                                    Some(Self::sanitize_provider_catalog(catalog));
                            }
                        }
                        if (self.current_provider.is_none() || self.current_model.is_none())
                            && self.provider_catalog.is_some()
                        {
                            let config = client.config_providers().await.ok();
                            self.apply_provider_defaults(config.as_ref());
                        }
                    }
                }
            }

            _ => {}
        }
    }

    pub async fn execute_command(&mut self, cmd: &str) -> String {
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            return "Unknown command. Type /help for available commands.".to_string();
        }

        let cmd_name = &parts[0][1..];
        let args = &parts[1..];
        let normalized_cmd = cmd_name.to_lowercase();
        if normalized_cmd != "recent" {
            self.record_recent_command(cmd);
        }

        if let Some(result) = commands::try_execute_basic_command(self, &normalized_cmd, args).await
        {
            return result;
        }

        match normalized_cmd.as_str() {
            _ => format!(
                "Unknown command: {}. Type /help for available commands.",
                cmd_name
            ),
        }
    }

    fn copy_latest_assistant_to_clipboard(
        &self,
        messages: &[ChatMessage],
    ) -> Result<usize, String> {
        let Some(text) = plan_helpers::latest_assistant_text(messages) else {
            return Err("No assistant content available to copy.".to_string());
        };
        let mut clipboard =
            arboard::Clipboard::new().map_err(|err| format!("cannot access clipboard: {}", err))?;
        clipboard
            .set_text(text.clone())
            .map_err(|err| format!("cannot set clipboard text: {}", err))?;
        Ok(text.chars().count())
    }

    fn plan_feedback_needs_clarification(wizard: &PlanFeedbackWizardState) -> bool {
        wizard.plan_name.trim().is_empty()
            && wizard.scope.trim().is_empty()
            && wizard.constraints.trim().is_empty()
            && wizard.priorities.trim().is_empty()
            && wizard.notes.trim().is_empty()
    }

    fn prepare_prompt_text(&self, text: &str) -> String {
        let trimmed = text.trim_start();
        if trimmed.starts_with("/tool ") {
            return text.to_string();
        }
        if Self::is_agent_team_assignment_prompt(trimmed) {
            return text.to_string();
        }
        if !matches!(self.current_mode, TandemMode::Plan) {
            return text.to_string();
        }
        let task_context = self.plan_task_context_block();
        let task_context_block = task_context
            .as_deref()
            .map(|ctx| format!("\nCurrent task list context:\n{}\n", ctx))
            .unwrap_or_default();
        format!(
            "You are operating in Plan mode.\n\
             Please use the todowrite tool to create a structured task list. Then, ask for user approval before starting execution/completing the tasks.\n\
             Tool rule: Use `todowrite` (or `todo_write` / `update_todo_list`) for plan tasks.\n\
             Do NOT use the generic `task` tool for plan creation.\n\
             First-action rule: On a new planning request, your FIRST action must be creating/updating a structured todo list.\n\
             Breakdown rule: Do not create a single generic task. Create a concrete multi-step plan with at least 6 actionable tasks (prefer 8-12 when appropriate).\n\
             Do not return only a plain numbered/text plan before creating/updating todos.\n\
             Clarification rule: If information is missing, still create an initial draft todo breakdown first, then ask clarification questions.\n\
             Approval rule: After task creation/update, ask for user approval before execution/completing tasks.\n\
             Execution rule: During execution, after verifying each task is done, use `todowrite` with status=\"completed\" for that task.\n\
             If information is missing, ask clarifying questions via the question tool.\n\
             Ask ONE clarification question at a time, then wait for the user's answer.\n\
             Prefer structured question tool prompts over plain-text question lists.\n\
             If there is already one active task list, treat it as the default plan context; do not ask \"which plan\" unless there are multiple distinct plans.\n\
             When the user says execute/continue/go, update statuses and next steps for the current task list.\n\
             After tool calls, provide a concise summary.\n{}\n\
             User request:\n{}",
            task_context_block,
            text
        )
    }

    fn plan_task_context_block(&self) -> Option<String> {
        let (tasks, active_task_id) = match &self.state {
            AppState::Chat {
                tasks,
                active_task_id,
                ..
            } => (tasks, active_task_id),
            _ => return None,
        };
        plan_helpers::plan_task_context_block(tasks, active_task_id.as_deref())
    }

    fn resolve_workspace_path(raw: &str) -> Result<PathBuf, String> {
        let expanded = Self::expand_home_prefix(raw);
        let candidate = PathBuf::from(expanded);
        let absolute = if candidate.is_absolute() {
            candidate
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(candidate)
        };
        if !absolute.exists() {
            return Err(format!(
                "Workspace path does not exist: {}",
                absolute.display()
            ));
        }
        if !absolute.is_dir() {
            return Err(format!(
                "Workspace path is not a directory: {}",
                absolute.display()
            ));
        }
        absolute.canonicalize().map_err(|err| {
            format!(
                "Failed to resolve workspace path {}: {}",
                absolute.display(),
                err
            )
        })
    }

    fn expand_home_prefix(input: &str) -> String {
        if input == "~" {
            return Self::user_home_dir()
                .unwrap_or_else(|| PathBuf::from("~"))
                .display()
                .to_string();
        }
        if let Some(rest) = input.strip_prefix("~/") {
            if let Some(home) = Self::user_home_dir() {
                return home.join(rest).display().to_string();
            }
        }
        input.to_string()
    }

    fn user_home_dir() -> Option<PathBuf> {
        #[cfg(windows)]
        {
            std::env::var_os("USERPROFILE").map(PathBuf::from)
        }
        #[cfg(not(windows))]
        {
            std::env::var_os("HOME").map(PathBuf::from)
        }
    }

    fn request_center_digit_is_shortcut(&self, c: char) -> bool {
        if !c.is_ascii_digit() {
            return false;
        }
        let AppState::Chat {
            pending_requests,
            request_cursor,
            ..
        } = &self.state
        else {
            return false;
        };
        let Some(request) = pending_requests.get(*request_cursor) else {
            return false;
        };
        match &request.kind {
            PendingRequestKind::Permission(_) => true,
            PendingRequestKind::Question(question) => question
                .questions
                .get(question.question_index)
                .map(|q| !q.custom && !q.options.is_empty())
                .unwrap_or(false),
        }
    }

    fn request_center_active_is_question(&self) -> bool {
        let AppState::Chat {
            pending_requests,
            request_cursor,
            ..
        } = &self.state
        else {
            return false;
        };
        matches!(
            pending_requests.get(*request_cursor).map(|r| &r.kind),
            Some(PendingRequestKind::Question(_))
        )
    }

    async fn load_chat_history(&self, session_id: &str) -> Vec<ChatMessage> {
        let Some(client) = &self.client else {
            return Vec::new();
        };
        let Ok(wire_messages) = client.get_session_messages(session_id).await else {
            return Vec::new();
        };
        wire_messages
            .iter()
            .filter_map(Self::wire_message_to_chat_message)
            .collect()
    }

    fn wire_message_to_chat_message(msg: &WireSessionMessage) -> Option<ChatMessage> {
        let role = match msg.info.role.to_ascii_lowercase().as_str() {
            "user" => MessageRole::User,
            "assistant" => MessageRole::Assistant,
            "system" => MessageRole::System,
            _ => MessageRole::System,
        };
        let mut content = Vec::new();
        for part in &msg.parts {
            let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("text");
            match part_type {
                "text" | "reasoning" => {
                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            content.push(ContentBlock::Text(text.to_string()));
                        }
                    }
                }
                "tool_use" | "tool_call" | "tool" => {
                    let id = part
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = part
                        .get("name")
                        .or_else(|| part.get("tool"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("tool")
                        .to_string();
                    let args = part
                        .get("input")
                        .or_else(|| part.get("args"))
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "{}".to_string());
                    content.push(ContentBlock::ToolCall(ToolCallInfo { id, name, args }));
                    if let Some(result) = part.get("result") {
                        let text = if let Some(s) = result.as_str() {
                            s.to_string()
                        } else {
                            result.to_string()
                        };
                        if !text.is_empty() && text != "null" {
                            content.push(ContentBlock::ToolResult(text));
                        }
                    } else if let Some(error) = part.get("error").and_then(|v| v.as_str()) {
                        if !error.is_empty() {
                            content.push(ContentBlock::ToolResult(error.to_string()));
                        }
                    }
                }
                "tool_result" => {
                    let text = part
                        .get("output")
                        .or_else(|| part.get("result"))
                        .or_else(|| part.get("text"))
                        .map(|v| {
                            if let Some(s) = v.as_str() {
                                s.to_string()
                            } else {
                                v.to_string()
                            }
                        })
                        .unwrap_or_else(|| "tool result".to_string());
                    content.push(ContentBlock::ToolResult(text));
                }
                _ => {
                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                        if !text.is_empty() {
                            content.push(ContentBlock::Text(text.to_string()));
                        }
                    }
                }
            }
        }
        if content.is_empty() {
            None
        } else {
            Some(ChatMessage { role, content })
        }
    }

    async fn persist_provider_defaults(
        &self,
        provider_id: &str,
        model_id: Option<&str>,
        api_key: Option<&str>,
    ) {
        let Some(client) = &self.client else {
            return;
        };
        let mut patch = serde_json::Map::new();
        patch.insert("default_provider".to_string(), json!(provider_id));
        if model_id.is_some() {
            let mut provider_patch = serde_json::Map::new();
            if let Some(model_id) = model_id {
                provider_patch.insert("default_model".to_string(), json!(model_id));
            }
            let mut providers = serde_json::Map::new();
            providers.insert(provider_id.to_string(), Value::Object(provider_patch));
            patch.insert("providers".to_string(), Value::Object(providers));
        }
        if let Some(api_key) = api_key {
            let _ = client.set_auth(provider_id, api_key).await;
        }
        let _ = client.patch_config(Value::Object(patch)).await;
    }

    fn apply_provider_defaults(
        &mut self,
        config: Option<&crate::net::client::ConfigProvidersResponse>,
    ) {
        let Some(catalog) = self.provider_catalog.as_ref() else {
            return;
        };

        let connected = if catalog.connected.is_empty() {
            catalog
                .all
                .iter()
                .map(|p| p.id.clone())
                .collect::<Vec<String>>()
        } else {
            catalog.connected.clone()
        };

        let default_provider = catalog
            .default
            .clone()
            .filter(|id| connected.contains(id))
            .or_else(|| {
                config
                    .and_then(|cfg| cfg.default.clone())
                    .filter(|id| connected.contains(id))
            })
            .or_else(|| connected.first().cloned())
            .or_else(|| catalog.all.first().map(|p| p.id.clone()));

        let provider_invalid = self
            .current_provider
            .as_ref()
            .map(|id| !catalog.all.iter().any(|p| p.id == *id))
            .unwrap_or(true);
        let provider_unusable = self
            .current_provider
            .as_ref()
            .map(|id| !connected.contains(id))
            .unwrap_or(true);

        if provider_invalid || provider_unusable {
            self.current_provider = default_provider;
        } else if self.current_provider.is_none() {
            self.current_provider = default_provider;
        }

        let model_needs_reset = self.current_model.is_none()
            || self
                .current_provider
                .as_ref()
                .and_then(|provider_id| {
                    catalog
                        .all
                        .iter()
                        .find(|p| p.id == *provider_id)
                        .map(|provider| {
                            !self
                                .current_model
                                .as_ref()
                                .map(|m| provider.models.contains_key(m))
                                .unwrap_or(false)
                        })
                })
                .unwrap_or(true);

        if model_needs_reset {
            if let Some(provider_id) = self.current_provider.clone() {
                if let Some(provider) = catalog.all.iter().find(|p| p.id == provider_id) {
                    let default_model = config
                        .and_then(|cfg| cfg.providers.get(&provider_id))
                        .and_then(|p| p.default_model.clone())
                        .filter(|id| provider.models.contains_key(id));
                    let mut model_ids: Vec<String> = provider.models.keys().cloned().collect();
                    model_ids.sort();
                    self.current_model = default_model.or_else(|| model_ids.first().cloned());
                }
            }
        }
    }

    async fn stop_engine_process(&mut self) {
        let Some(mut child) = self.engine_process.take() else {
            self.engine_spawned_at = None;
            return;
        };

        let pid = child.id();
        let _ = child.start_kill();
        let _ = timeout(std::time::Duration::from_secs(2), child.wait()).await;

        #[cfg(windows)]
        if let Some(pid) = pid {
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .output();
        }

        #[cfg(unix)]
        if let Some(pid) = pid {
            let _ = std::process::Command::new("kill")
                .args(["-9", &pid.to_string()])
                .output();
        }
        self.engine_spawned_at = None;
    }

    pub async fn shutdown(&mut self) {
        self.release_engine_lease().await;
        if Self::shared_engine_mode_enabled()
            && self.engine_connection_source == EngineConnectionSource::SharedAttached
        {
            // Shared mode + attached engine: detach and leave ownership to the other client.
            let _ = self.engine_process.take();
            self.engine_spawned_at = None;
            return;
        }
        self.stop_engine_process().await;
    }

    async fn acquire_engine_lease(&mut self) {
        let Some(client) = &self.client else {
            return;
        };
        if self.engine_lease_id.is_some() {
            return;
        }
        match client.acquire_lease("tui-cli", "tui", Some(60_000)).await {
            Ok(lease) => {
                self.engine_lease_id = Some(lease.lease_id);
                self.engine_lease_last_renewed = Some(Instant::now());
            }
            Err(err) => {
                self.connection_status = format!("Lease acquire failed: {}", err);
            }
        }
    }

    async fn renew_engine_lease_if_due(&mut self) {
        let Some(lease_id) = self.engine_lease_id.clone() else {
            return;
        };
        let should_renew = self
            .engine_lease_last_renewed
            .map(|t| t.elapsed().as_secs() >= 20)
            .unwrap_or(true);
        if !should_renew {
            return;
        }
        let Some(client) = &self.client else {
            return;
        };
        match client.renew_lease(&lease_id).await {
            Ok(true) => {
                self.engine_lease_last_renewed = Some(Instant::now());
            }
            Ok(false) => {
                self.engine_lease_id = None;
                self.engine_lease_last_renewed = None;
                self.acquire_engine_lease().await;
            }
            Err(_) => {}
        }
    }

    async fn release_engine_lease(&mut self) {
        let Some(lease_id) = self.engine_lease_id.take() else {
            return;
        };
        self.engine_lease_last_renewed = None;
        if let Some(client) = &self.client {
            let _ = client.release_lease(&lease_id).await;
        }
    }
}
