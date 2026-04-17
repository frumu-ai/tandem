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

include!("app_impl_parts/part01.rs");
include!("app_impl_parts/part02.rs");
