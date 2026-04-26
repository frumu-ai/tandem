#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};

    fn dispatcher_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct DispatcherEnvGuard {
        _guard: std::sync::MutexGuard<'static, ()>,
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl DispatcherEnvGuard {
        fn new(vars: &[&'static str]) -> Self {
            let guard = dispatcher_env_lock().lock().expect("dispatcher env lock");
            let saved = vars
                .iter()
                .copied()
                .map(|key| (key, std::env::var(key).ok()))
                .collect::<Vec<_>>();
            Self {
                _guard: guard,
                saved,
            }
        }

        fn set(&self, key: &'static str, value: impl AsRef<str>) {
            std::env::set_var(key, value.as_ref());
        }
    }

    impl Drop for DispatcherEnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..) {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }

    // ── Slash command parser ──────────────────────────────────────────────

    #[test]
    fn parse_new_no_name() {
        assert!(matches!(
            parse_slash_command("/new"),
            Some(SlashCommand::New { name: None })
        ));
    }

    #[test]
    fn parse_new_with_name() {
        let cmd = parse_slash_command("/new my session");
        assert!(matches!(
            cmd,
            Some(SlashCommand::New { name: Some(ref n) }) if n == "my session"
        ));
    }

    #[test]
    fn parse_sessions() {
        assert!(matches!(
            parse_slash_command("/sessions"),
            Some(SlashCommand::ListSessions)
        ));
        assert!(matches!(
            parse_slash_command("/session"),
            Some(SlashCommand::ListSessions)
        ));
    }

    #[test]
    fn parse_resume() {
        let cmd = parse_slash_command("/resume abc123");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Resume { ref query }) if query == "abc123"
        ));
    }

    #[test]
    fn parse_rename() {
        let cmd = parse_slash_command("/rename new name");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Rename { ref name }) if name == "new name"
        ));
    }

    #[test]
    fn parse_status() {
        assert!(matches!(
            parse_slash_command("/status"),
            Some(SlashCommand::Status)
        ));
    }

    #[test]
    fn parse_run() {
        assert!(matches!(
            parse_slash_command("/run"),
            Some(SlashCommand::Run)
        ));
    }

    #[test]
    fn parse_cancel_aliases() {
        assert!(matches!(
            parse_slash_command("/cancel"),
            Some(SlashCommand::Cancel)
        ));
        assert!(matches!(
            parse_slash_command("/abort"),
            Some(SlashCommand::Cancel)
        ));
    }

    #[test]
    fn parse_todos_aliases() {
        assert!(matches!(
            parse_slash_command("/todos"),
            Some(SlashCommand::Todos)
        ));
        assert!(matches!(
            parse_slash_command("/todo"),
            Some(SlashCommand::Todos)
        ));
    }

    #[test]
    fn parse_requests() {
        assert!(matches!(
            parse_slash_command("/requests"),
            Some(SlashCommand::Requests)
        ));
    }

    #[test]
    fn parse_answer() {
        let cmd = parse_slash_command("/answer q123 continue with option A");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Answer { ref question_id, ref answer })
            if question_id == "q123" && answer == "continue with option A"
        ));
    }

    #[test]
    fn parse_providers() {
        assert!(matches!(
            parse_slash_command("/providers"),
            Some(SlashCommand::Providers)
        ));
    }

    #[test]
    fn parse_models() {
        assert!(matches!(
            parse_slash_command("/models"),
            Some(SlashCommand::Models { provider: None })
        ));
        let cmd = parse_slash_command("/models openrouter");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Models { provider: Some(ref p) }) if p == "openrouter"
        ));
    }

    #[test]
    fn parse_model_set() {
        let cmd = parse_slash_command("/model gpt-5-mini");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Model { ref model_id }) if model_id == "gpt-5-mini"
        ));
    }

    #[test]
    fn parse_help() {
        assert!(matches!(
            parse_slash_command("/help"),
            Some(SlashCommand::Help { topic: None })
        ));
        assert!(matches!(
            parse_slash_command("/?"),
            Some(SlashCommand::Help { topic: None })
        ));
        assert!(matches!(
            parse_slash_command("/help schedule"),
            Some(SlashCommand::Help { topic: Some(ref topic) }) if topic == "schedule"
        ));
    }

    #[test]
    fn parse_unknown_returns_none() {
        assert!(parse_slash_command("/unknown").is_none());
        assert!(parse_slash_command("not a command").is_none());
        assert!(parse_slash_command("").is_none());
    }

    #[test]
    fn parse_trims_whitespace() {
        assert!(matches!(
            parse_slash_command("  /help  "),
            Some(SlashCommand::Help { topic: None })
        ));
    }

    #[test]
    fn parse_schedule_help_and_default() {
        assert!(matches!(
            parse_slash_command("/schedule"),
            Some(SlashCommand::Schedule {
                action: ScheduleCommand::Help
            })
        ));
        assert!(matches!(
            parse_slash_command("/schedule help"),
            Some(SlashCommand::Schedule {
                action: ScheduleCommand::Help
            })
        ));
    }

    #[test]
    fn parse_schedule_plan() {
        let cmd = parse_slash_command("/schedule plan daily repo summary at 9am");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Schedule {
                action: ScheduleCommand::Plan { ref prompt }
            }) if prompt == "daily repo summary at 9am"
        ));
    }

    #[test]
    fn parse_schedule_show() {
        let cmd = parse_slash_command("/schedule show wfplan-123");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Schedule {
                action: ScheduleCommand::Show { ref plan_id }
            }) if plan_id == "wfplan-123"
        ));
    }

    #[test]
    fn parse_schedule_edit() {
        let cmd = parse_slash_command("/schedule edit wfplan-123 change this to every monday");
        assert!(matches!(
            cmd,
            Some(SlashCommand::Schedule {
                action: ScheduleCommand::Edit {
                    ref plan_id,
                    ref message
                }
            }) if plan_id == "wfplan-123" && message == "change this to every monday"
        ));
    }

    #[test]
    fn parse_schedule_reset_and_apply() {
        assert!(matches!(
            parse_slash_command("/schedule reset wfplan-123"),
            Some(SlashCommand::Schedule {
                action: ScheduleCommand::Reset { ref plan_id }
            }) if plan_id == "wfplan-123"
        ));
        assert!(matches!(
            parse_slash_command("/schedule apply wfplan-123"),
            Some(SlashCommand::Schedule {
                action: ScheduleCommand::Apply { ref plan_id }
            }) if plan_id == "wfplan-123"
        ));
    }

    #[test]
    fn parse_automations_commands() {
        assert!(matches!(
            parse_slash_command("/automations"),
            Some(SlashCommand::Automations {
                action: AutomationsCommand::List
            })
        ));
        assert!(matches!(
            parse_slash_command("/automations delete auto-1 --yes"),
            Some(SlashCommand::Automations {
                action: AutomationsCommand::Delete {
                    ref automation_id,
                    confirmed: true
                }
            }) if automation_id == "auto-1"
        ));
    }

    #[test]
    fn parse_runs_memory_workspace_commands() {
        assert!(matches!(
            parse_slash_command("/runs artifacts run-1"),
            Some(SlashCommand::Runs {
                action: RunsCommand::Artifacts { ref run_id }
            }) if run_id == "run-1"
        ));
        assert!(matches!(
            parse_slash_command("/memory search deployment notes"),
            Some(SlashCommand::Memory {
                action: MemoryCommand::Search { ref query }
            }) if query == "deployment notes"
        ));
        assert!(matches!(
            parse_slash_command("/workspace files dispatcher"),
            Some(SlashCommand::Workspace {
                action: WorkspaceCommand::Files { ref query }
            }) if query == "dispatcher"
        ));
    }

    #[test]
    fn parse_mcp_packs_and_config_commands() {
        assert!(matches!(
            parse_slash_command("/mcp refresh github-only"),
            Some(SlashCommand::Mcp {
                action: McpCommand::Refresh { ref name }
            }) if name == "github-only"
        ));
        assert!(matches!(
            parse_slash_command("/packs uninstall starter-pack --yes"),
            Some(SlashCommand::Packs {
                action: PacksCommand::Uninstall {
                    ref selector,
                    confirmed: true
                }
            }) if selector == "starter-pack"
        ));
        assert!(matches!(
            parse_slash_command("/config set-model gpt-5-mini"),
            Some(SlashCommand::Config {
                action: ConfigCommand::SetModel { ref model_id }
            }) if model_id == "gpt-5-mini"
        ));
    }

    #[test]
    fn help_text_lists_schedule_topic() {
        let help = help_text(None, ChannelSecurityProfile::Operator);
        assert!(help.contains("/schedule help"));
        assert!(help.contains("/help schedule"));
        assert!(help.contains("/automations"));
        assert!(help.contains("/memory"));
    }

    #[test]
    fn schedule_help_text_lists_subcommands() {
        let help = help_text(Some("schedule"), ChannelSecurityProfile::Operator);
        assert!(help.contains("/schedule plan <prompt>"));
        assert!(help.contains("/schedule apply <plan_id>"));
    }

    #[test]
    fn topic_help_for_new_namespaces() {
        assert!(
            help_text(Some("automations"), ChannelSecurityProfile::Operator)
                .contains("/automations run <id>")
        );
        assert!(help_text(Some("memory"), ChannelSecurityProfile::Operator)
            .contains("/memory save <text>"));
        assert!(
            help_text(Some("workspace"), ChannelSecurityProfile::Operator)
                .contains("/workspace branch")
        );
        assert!(help_text(Some("mcp"), ChannelSecurityProfile::Operator)
            .contains("/mcp tools [server]"));
        assert!(help_text(Some("packs"), ChannelSecurityProfile::Operator)
            .contains("/packs install <path-or-url>"));
        assert!(help_text(Some("config"), ChannelSecurityProfile::Operator)
            .contains("/config set-model <model_id>"));
    }

    #[test]
    fn detects_pack_builder_intent() {
        let text = "create me a pack that checks latest headline news and posts to slack";
        assert!(is_pack_builder_intent(text));
        let route = route_agent_for_channel_message(text);
        assert_eq!(route.agent.as_deref(), Some("pack_builder"));
        assert!(route
            .tool_allowlist
            .as_ref()
            .is_some_and(|v| v.iter().any(|t| t == "pack_builder")));
    }

    #[test]
    fn non_pack_intent_uses_default_route() {
        let text = "what model am I using?";
        assert!(!is_pack_builder_intent(text));
        let route = route_agent_for_channel_message(text);
        assert!(route.agent.is_none());
        assert!(route.tool_allowlist.is_none());
    }

    #[test]
    fn parses_pack_builder_confirm_cancel_and_connector_override() {
        assert!(matches!(
            parse_pack_builder_reply_command("confirm"),
            Some(PackBuilderReplyCommand::Confirm)
        ));
        assert!(matches!(
            parse_pack_builder_reply_command("ok"),
            Some(PackBuilderReplyCommand::Confirm)
        ));
        assert!(matches!(
            parse_pack_builder_reply_command("cancel"),
            Some(PackBuilderReplyCommand::Cancel)
        ));
        let parsed = parse_pack_builder_reply_command("use connectors: notion, slack");
        match parsed {
            Some(PackBuilderReplyCommand::UseConnectors(rows)) => {
                assert_eq!(rows, vec!["notion".to_string(), "slack".to_string()]);
            }
            _ => panic!("expected connector override parse"),
        }
    }

    // ── SessionRecord ─────────────────────────────────────────────────────

    #[test]
    fn session_record_roundtrip() {
        let record = SessionRecord {
            session_id: "s1".to_string(),
            created_at_ms: 1000,
            last_seen_at_ms: 2000,
            channel: "telegram".to_string(),
            sender: "user1".to_string(),
            scope_id: Some("chat:42".to_string()),
            scope_kind: Some("room".to_string()),
            tool_preferences: None,
            workflow_planner_session_id: None,
        };
        let serialized = serde_json::to_string(&record).unwrap();
        let deserialized: SessionRecord = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.session_id, "s1");
        assert_eq!(deserialized.created_at_ms, 1000);
        assert_eq!(deserialized.last_seen_at_ms, 2000);
        assert_eq!(deserialized.channel, "telegram");
        assert_eq!(deserialized.sender, "user1");
    }

    fn test_channel_message(scope_id: &str) -> ChannelMessage {
        ChannelMessage {
            id: "m1".to_string(),
            sender: "user1".to_string(),
            reply_target: "room1".to_string(),
            content: "hello".to_string(),
            channel: "discord".to_string(),
            timestamp: chrono::Utc::now(),
            attachment: None,
            attachment_url: None,
            attachment_path: None,
            attachment_mime: None,
            attachment_filename: None,
            trigger: crate::traits::MessageTriggerContext::default(),
            scope: crate::traits::ConversationScope {
                kind: crate::traits::ConversationScopeKind::Room,
                id: scope_id.to_string(),
            },
        }
    }

    #[test]
    fn session_map_key_includes_scope() {
        let room_a = test_channel_message("channel:room-a");
        let room_b = test_channel_message("channel:room-b");
        assert_ne!(session_map_key(&room_a), session_map_key(&room_b));
    }

    #[test]
    fn channel_session_create_body_allows_memory_and_browser_tools() {
        let body = build_channel_session_create_body(
            "Channel Session",
            ChannelSecurityProfile::Operator,
            None,
        );
        let permissions = body
            .get("permission")
            .and_then(|value| value.as_array())
            .expect("permission array");

        for permission_name in ["memory_search", "memory_store", "memory_list"] {
            assert!(permissions.iter().any(|value| {
                value.get("permission").and_then(|row| row.as_str()) == Some(permission_name)
                    && value.get("action").and_then(|row| row.as_str()) == Some("allow")
            }));
        }

        assert!(permissions.iter().any(|value| {
            value.get("permission").and_then(|row| row.as_str()) == Some("mcp*")
                && value.get("action").and_then(|row| row.as_str()) == Some("allow")
        }));

        for permission_name in [
            "browser_status",
            "browser_open",
            "browser_navigate",
            "browser_snapshot",
            "browser_click",
            "browser_type",
            "browser_press",
            "browser_wait",
            "browser_extract",
            "browser_screenshot",
            "browser_close",
        ] {
            assert!(permissions.iter().any(|value| {
                value.get("permission").and_then(|row| row.as_str()) == Some(permission_name)
                    && value.get("action").and_then(|row| row.as_str()) == Some("allow")
            }));
        }
    }

    #[test]
    fn public_demo_session_create_body_disables_workspace_and_shell_access() {
        let body = build_channel_session_create_body(
            "Public Demo Session",
            ChannelSecurityProfile::PublicDemo,
            Some("channel-public::discord::room-a"),
        );
        let permissions = body
            .get("permission")
            .and_then(|value| value.as_array())
            .expect("permission array");

        assert!(body.get("directory").is_none());
        assert_eq!(
            body.get("project_id").and_then(|value| value.as_str()),
            Some("channel-public::discord::room-a")
        );
        assert!(permissions.iter().any(|value| {
            value.get("permission").and_then(|row| row.as_str()) == Some("websearch")
                && value.get("action").and_then(|row| row.as_str()) == Some("allow")
        }));
        assert!(permissions.iter().any(|value| {
            value.get("permission").and_then(|row| row.as_str()) == Some("memory_search")
                && value.get("action").and_then(|row| row.as_str()) == Some("allow")
        }));
        assert!(!permissions.iter().any(|value| {
            matches!(
                value.get("permission").and_then(|row| row.as_str()),
                Some("read" | "bash" | "browser_open" | "mcp*")
            )
        }));
    }

    #[test]
    fn public_demo_help_lists_disabled_commands_for_security() {
        let help = help_text(None, ChannelSecurityProfile::PublicDemo);
        assert!(help.contains("Disabled In This Public Channel For Security"));
        assert!(help.contains("/workspace"));
        assert!(help.contains("/memory"));
        assert!(help.contains("real Tandem capabilities"));
    }

    #[test]
    fn public_demo_memory_help_is_disabled() {
        let help = help_text(Some("memory"), ChannelSecurityProfile::PublicDemo);
        assert!(help.contains("Public Channel Memory Commands"));
        assert!(help.contains("quarantined"));
    }

    #[test]
    fn public_demo_allows_memory_commands() {
        let reason = blocked_command_reason(
            &SlashCommand::Memory {
                action: MemoryCommand::Help,
            },
            ChannelSecurityProfile::PublicDemo,
        );
        assert!(reason.is_none());
    }

    #[test]
    fn public_demo_tool_allowlist_cannot_be_widened_by_route_override() {
        let prefs = ChannelToolPreferences::default();
        let route_allowlist = vec![
            "pack_builder".to_string(),
            "websearch".to_string(),
            WORKFLOW_PLANNER_PSEUDO_TOOL.to_string(),
        ];
        let result = build_channel_tool_allowlist(
            Some(&route_allowlist),
            &prefs,
            ChannelSecurityProfile::PublicDemo,
        )
        .expect("public demo allowlist");

        assert_eq!(result, vec!["websearch".to_string()]);
    }

    #[test]
    fn workflow_planner_gate_is_removed_from_operator_allowlists() {
        let prefs = ChannelToolPreferences {
            enabled_tools: vec!["read".to_string(), WORKFLOW_PLANNER_PSEUDO_TOOL.to_string()],
            disabled_tools: vec![WORKFLOW_PLANNER_PSEUDO_TOOL.to_string()],
            ..Default::default()
        };
        let route_allowlist = vec![
            "read".to_string(),
            WORKFLOW_PLANNER_PSEUDO_TOOL.to_string(),
            "websearch".to_string(),
        ];

        let result = build_channel_tool_allowlist(
            Some(&route_allowlist),
            &prefs,
            ChannelSecurityProfile::Operator,
        )
        .expect("operator allowlist");

        assert_eq!(result, vec!["read".to_string(), "websearch".to_string()]);
        assert!(!result
            .iter()
            .any(|tool| tool == WORKFLOW_PLANNER_PSEUDO_TOOL));
    }

    #[test]
    fn workflow_planner_gate_is_not_implied_by_docs_mcp_servers() {
        let prefs = ChannelToolPreferences {
            enabled_mcp_servers: vec!["tandem-mcp".to_string()],
            ..Default::default()
        };

        assert!(!channel_workflow_planner_enabled(&prefs));
    }

    #[test]
    fn strict_kb_routing_prefers_answer_mode_for_factual_questions() {
        let prefs = ChannelToolPreferences {
            enabled_mcp_servers: vec!["kb".to_string()],
            ..Default::default()
        };

        assert!(!channel_message_has_explicit_workflow_intent(
            "What time does sponsor booth setup start, and when must it be finished?"
        ));
        assert!(strict_kb_prefers_answer_mode(
            "What time does sponsor booth setup start, and when must it be finished?",
            true,
            &prefs,
        ));
        assert!(setup_intent_requires_explicit_workflow_authoring(
            &SetupIntentKind::WorkflowPlannerCreate
        ));
        assert!(setup_intent_requires_explicit_workflow_authoring(
            &SetupIntentKind::AutomationCreate
        ));
    }

    #[test]
    fn strict_kb_routing_prefers_answer_mode_for_policy_questions() {
        let prefs = ChannelToolPreferences {
            enabled_mcp_tools: vec!["mcp.kb.search".to_string()],
            ..Default::default()
        };

        assert!(strict_kb_prefers_answer_mode(
            "What is the escalation path for a VIP support issue?",
            true,
            &prefs,
        ));
    }

    #[test]
    fn explicit_mcp_context_makes_factual_questions_strict_even_when_channel_toggle_is_off() {
        let prefs = ChannelToolPreferences {
            enabled_mcp_servers: vec!["kb".to_string()],
            ..Default::default()
        };
        let message = "Can you ban a Discord user who is spamming?";

        assert!(channel_message_is_factual_question(message));
        assert!(effective_channel_strict_kb_grounding(
            message, false, &prefs
        ));
        assert!(strict_kb_prefers_answer_mode(message, false, &prefs));
    }

    #[test]
    fn strict_kb_routing_prefers_answer_mode_without_explicit_mcp_preferences() {
        let prefs = ChannelToolPreferences::default();

        assert!(strict_kb_prefers_answer_mode(
            "What time does sponsor booth setup start, and when must it be finished?",
            true,
            &prefs,
        ));
    }

    #[test]
    fn strict_kb_routing_keeps_explicit_workflow_requests_on_planner_path() {
        let prefs = ChannelToolPreferences {
            enabled_tools: vec![WORKFLOW_PLANNER_PSEUDO_TOOL.to_string()],
            enabled_mcp_servers: vec!["kb".to_string()],
            ..Default::default()
        };

        assert!(channel_workflow_planner_enabled(&prefs));
        assert!(channel_message_has_explicit_workflow_intent(
            "Create a workflow that sends a sponsor setup reminder every event morning."
        ));
        assert!(!strict_kb_prefers_answer_mode(
            "Create a workflow that sends a sponsor setup reminder every event morning.",
            true,
            &prefs,
        ));
    }

    #[test]
    fn explicit_workflow_requests_surface_disabled_planner_message_when_gate_is_off() {
        let prefs = ChannelToolPreferences {
            enabled_mcp_servers: vec!["kb".to_string()],
            ..Default::default()
        };

        assert!(!channel_workflow_planner_enabled(&prefs));
        assert!(channel_message_has_explicit_workflow_intent(
            "Create a workflow that sends a sponsor setup reminder every event morning."
        ));
        assert_eq!(
            workflow_planner_disabled_channel_message(false),
            "🗓️ Workflow drafting is disabled for this channel. Enable the workflow planner gate in Settings to continue."
        );
    }

    #[test]
    fn factual_kb_questions_do_not_surface_disabled_planner_message() {
        let prefs = ChannelToolPreferences {
            enabled_mcp_servers: vec!["kb".to_string()],
            ..Default::default()
        };
        let message = "What time does sponsor booth setup start, and when must it be finished?";

        assert!(!channel_workflow_planner_enabled(&prefs));
        assert!(channel_message_is_factual_question(message));
        assert!(!channel_message_has_explicit_workflow_intent(message));
        assert!(strict_kb_prefers_answer_mode(message, true, &prefs));
        assert!(!workflow_planner_disabled_channel_message(false)
            .contains("Sponsor booth setup starts"));
        assert!(setup_intent_requires_explicit_workflow_authoring(
            &SetupIntentKind::WorkflowPlannerCreate
        ));
    }

    #[test]
    fn workflow_planner_channel_summary_surfaces_questions_in_chat() {
        let payload = serde_json::json!({
            "session": {
                "operation": {
                    "status": "completed",
                    "response": {
                        "clarifier": {
                            "question": "Should Notion saves draft pages or publish pages?"
                        }
                    }
                },
                "planning": {
                    "validation_state": "incomplete",
                    "blocked_tools": [],
                    "missing_requirements": ["notion publish mode"]
                }
            }
        });

        let reply =
            workflow_planner_channel_summary_reply(Some(&payload), "wfplan-session-1", "preview");

        assert!(reply.contains("Workflow draft needs one answer."));
        assert!(reply.contains("Should Notion saves draft pages or publish pages?"));
        assert!(reply.contains("Missing details: notion publish mode"));
        assert!(reply.contains("Reply here with answers or changes"));
        assert!(!reply.contains("`wfplan-session-1`"));
    }

    #[test]
    fn workflow_planner_channel_summary_surfaces_blocked_capabilities_in_chat() {
        let payload = serde_json::json!({
            "session": {
                "operation": {
                    "status": "completed",
                    "response": {
                        "plan": {
                            "title": "Reddit research workflow",
                            "steps": [{ "step_id": "collect" }],
                            "schedule": { "kind": "interval", "hours": 4 }
                        }
                    }
                },
                "planning": {
                    "validation_state": "needs_approval",
                    "blocked_tools": ["mcp.notion.pages_create"],
                    "missing_requirements": []
                }
            }
        });

        let reply =
            workflow_planner_channel_summary_reply(Some(&payload), "wfplan-session-2", "preview");

        assert!(reply.contains("Workflow draft created: Reddit research workflow"));
        assert!(reply.contains("Validation: needs_approval"));
        assert!(reply.contains("Blocked capabilities: mcp.notion.pages_create"));
        assert!(reply.contains("Activation still requires review/apply"));
    }

    #[test]
    fn workflow_planner_channel_summary_explains_planner_model_pause() {
        let payload = serde_json::json!({
            "session": {
                "operation": {
                    "status": "completed",
                    "response": {
                        "clarifier": {
                            "question": "This workflow needs planner model settings before Tandem can revise it. Configure a planner model and try again."
                        }
                    }
                },
                "planning": {
                    "validation_state": "valid",
                    "blocked_tools": [],
                    "missing_requirements": []
                }
            }
        });

        let reply =
            workflow_planner_channel_summary_reply(Some(&payload), "wfplan-session-3", "preview");

        assert!(reply.contains("Workflow draft paused: planner model settings are missing."));
        assert!(reply.contains("retry workflow draft"));
        assert!(!reply.contains("Workflow draft needs one answer."));
        assert!(!reply.contains("Validation: valid"));
    }

    #[test]
    fn workflow_planner_linked_session_ignores_plain_information_requests() {
        assert!(!workflow_planner_channel_message_should_update(
            "what is Northstar Events?"
        ));
        assert!(!workflow_planner_channel_message_should_update(
            "what do i do?"
        ));
        assert!(workflow_planner_channel_message_should_update(
            "save the workflow references to Notion"
        ));
        assert!(workflow_planner_channel_message_should_update(
            "yes, draft only before publishing"
        ));
    }

    #[test]
    fn channel_mcp_server_names_are_normalized_into_tool_allowlist_patterns() {
        let prefs = ChannelToolPreferences {
            enabled_mcp_servers: vec!["composio-1".to_string(), "tandem-mcp".to_string()],
            ..Default::default()
        };

        let result = build_channel_tool_allowlist(None, &prefs, ChannelSecurityProfile::Operator)
            .expect("channel allowlist");

        assert!(result.contains(&"mcp.composio_1.*".to_string()));
        assert!(result.contains(&"mcp.tandem_mcp.*".to_string()));
        assert!(result.contains(&"mcp_list".to_string()));
        assert!(result.iter().any(|tool| tool == "read"));
    }

    #[test]
    fn channel_exact_mcp_tools_are_added_to_tool_allowlist() {
        let prefs = ChannelToolPreferences {
            enabled_tools: vec!["read".to_string()],
            enabled_mcp_tools: vec!["mcp.composio_1.gmail_send_email".to_string()],
            ..Default::default()
        };

        let result = build_channel_tool_allowlist(None, &prefs, ChannelSecurityProfile::Operator)
            .expect("channel allowlist");

        assert!(result.iter().any(|tool| tool == "read"));
        assert!(result
            .iter()
            .any(|tool| tool == "mcp.composio_1.gmail_send_email"));
        assert!(result.iter().any(|tool| tool == "mcp_list"));
        assert!(!result.iter().any(|tool| tool == "mcp.composio_1.*"));
    }

    #[test]
    fn channel_tool_allowlist_includes_browser_tools_for_operator_channels() {
        let prefs = ChannelToolPreferences {
            disabled_tools: vec!["read".to_string()],
            ..Default::default()
        };

        let result = build_channel_tool_allowlist(None, &prefs, ChannelSecurityProfile::Operator)
            .expect("channel allowlist");

        for tool in [
            "browser_status",
            "browser_open",
            "browser_navigate",
            "browser_snapshot",
            "browser_click",
            "browser_type",
            "browser_press",
            "browser_wait",
            "browser_extract",
            "browser_screenshot",
            "browser_close",
        ] {
            assert!(result.iter().any(|entry| entry == tool));
        }
    }

    #[test]
    fn channel_tool_allowlist_defaults_to_wildcard_for_operator_channels() {
        let prefs = ChannelToolPreferences::default();
        let result = build_channel_tool_allowlist(None, &prefs, ChannelSecurityProfile::Operator)
            .expect("channel allowlist");
        assert_eq!(result, vec!["*".to_string()]);
    }

    #[tokio::test]
    async fn channel_tool_preferences_fall_back_to_channel_defaults_for_scoped_sessions() {
        let _guard = DispatcherEnvGuard::new(&["TANDEM_STATE_DIR"]);
        let state_dir =
            std::env::temp_dir().join(format!("tandem-channel-prefs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&state_dir).expect("state dir");
        _guard.set("TANDEM_STATE_DIR", state_dir.display().to_string());

        let mut map = std::collections::HashMap::new();
        map.insert(
            "telegram".to_string(),
            ChannelToolPreferences {
                enabled_mcp_servers: vec!["composio-1".to_string()],
                ..Default::default()
            },
        );
        save_tool_preferences(&map).await;

        let prefs = load_channel_tool_preferences("telegram", "chat:123").await;
        assert_eq!(prefs.enabled_mcp_servers, vec!["composio-1".to_string()]);
    }

    #[tokio::test]
    async fn scoped_tool_preferences_inherit_channel_defaults_when_empty() {
        let _guard = DispatcherEnvGuard::new(&["TANDEM_STATE_DIR"]);
        let state_dir =
            std::env::temp_dir().join(format!("tandem-channel-prefs-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&state_dir).expect("state dir");
        _guard.set("TANDEM_STATE_DIR", state_dir.display().to_string());

        let mut map = std::collections::HashMap::new();
        map.insert(
            "telegram".to_string(),
            ChannelToolPreferences {
                enabled_tools: vec!["read".to_string()],
                disabled_tools: vec!["grep".to_string()],
                enabled_mcp_servers: vec!["composio-1".to_string()],
                enabled_mcp_tools: vec!["mcp.composio_1.search_pages".to_string()],
            },
        );
        map.insert(
            "telegram:chat:123".to_string(),
            ChannelToolPreferences::default(),
        );
        save_tool_preferences(&map).await;

        let prefs = load_channel_tool_preferences("telegram", "chat:123").await;
        assert_eq!(prefs.enabled_tools, vec!["read".to_string()]);
        assert_eq!(prefs.disabled_tools, vec!["grep".to_string()]);
        assert_eq!(prefs.enabled_mcp_servers, vec!["composio-1".to_string()]);
        assert_eq!(
            prefs.enabled_mcp_tools,
            vec!["mcp.composio_1.search_pages".to_string()]
        );
    }

    #[tokio::test]
    async fn active_session_id_migrates_legacy_key_to_scoped_key() {
        let msg = test_channel_message("channel:room-a");
        let legacy_key = legacy_session_map_key(&msg);
        let scoped_key = session_map_key(&msg);
        let mut map = std::collections::HashMap::new();
        map.insert(
            legacy_key,
            SessionRecord {
                session_id: "s-legacy".to_string(),
                created_at_ms: 1,
                last_seen_at_ms: 2,
                channel: msg.channel.clone(),
                sender: msg.sender.clone(),
                scope_id: None,
                scope_kind: None,
                tool_preferences: None,
                workflow_planner_session_id: None,
            },
        );
        let session_map = std::sync::Arc::new(tokio::sync::Mutex::new(map));

        let active = active_session_id(&msg, &session_map).await;

        assert_eq!(active.as_deref(), Some("s-legacy"));
        let guard = session_map.lock().await;
        assert!(guard.get(&scoped_key).is_some());
        assert!(guard.get(&legacy_session_map_key(&msg)).is_none());
    }

    #[test]
    fn extracts_markdown_image_and_cleans_text() {
        let input = "Here is the render:\n![chart](https://cdn.example.com/chart.png)\nLooks good.";
        let (text, urls) = extract_image_urls_and_clean_text(input);
        assert_eq!(urls, vec!["https://cdn.example.com/chart.png"]);
        assert!(text.contains("Here is the render:"));
        assert!(text.contains("Looks good."));
        assert!(!text.contains("![chart]"));
    }

    #[test]
    fn extracts_direct_image_url_token() {
        let input = "Generated image: https://example.com/out/final.webp";
        let (text, urls) = extract_image_urls_and_clean_text(input);
        assert_eq!(urls, vec!["https://example.com/out/final.webp"]);
        assert!(text.contains("Generated image:"));
    }

    #[test]
    fn synthesize_attachment_prompt_includes_reference_when_present() {
        let out = synthesize_attachment_prompt(
            "telegram",
            "photo",
            "please analyze",
            Some("run/s1/channel_uploads/u1"),
            Some("/tmp/photo.jpg"),
            Some("https://example.com/photo.jpg"),
            Some("photo.jpg"),
            Some("image/jpeg"),
        );
        assert!(out.contains("Channel upload received"));
        assert!(out.contains("Stored upload reference"));
        assert!(out.contains("Stored local attachment path"));
        assert!(out.contains("please analyze"));
    }

    #[test]
    fn sanitize_resource_segment_replaces_invalid_chars() {
        assert_eq!(
            sanitize_resource_segment("abc/def:ghi"),
            "abc_def_ghi".to_string()
        );
    }

    #[test]
    fn zip_attachment_detection_handles_filename_path_and_url() {
        let mut msg = ChannelMessage {
            id: "m1".to_string(),
            sender: "u1".to_string(),
            reply_target: "c1".to_string(),
            content: "hello".to_string(),
            channel: "discord".to_string(),
            timestamp: chrono::Utc::now(),
            attachment: None,
            attachment_url: None,
            attachment_path: None,
            attachment_mime: None,
            attachment_filename: Some("pack.zip".to_string()),
            trigger: crate::traits::MessageTriggerContext::default(),
            scope: crate::traits::ConversationScope {
                kind: crate::traits::ConversationScopeKind::Room,
                id: "channel:c1".to_string(),
            },
        };
        assert!(is_zip_attachment(&msg));
        msg.attachment_filename = None;
        msg.attachment_path = Some("/tmp/upload.PACK.ZIP".to_string());
        assert!(is_zip_attachment(&msg));
        msg.attachment_path = None;
        msg.attachment_url = Some("https://example.com/x/y/pack.zip?sig=1".to_string());
        assert!(is_zip_attachment(&msg));
    }

    #[test]
    fn trusted_source_matching_supports_channel_room_sender_scopes() {
        let msg = ChannelMessage {
            id: "m1".to_string(),
            sender: "userA".to_string(),
            reply_target: "room1".to_string(),
            content: "hello".to_string(),
            channel: "discord".to_string(),
            timestamp: chrono::Utc::now(),
            attachment: None,
            attachment_url: None,
            attachment_path: None,
            attachment_mime: None,
            attachment_filename: None,
            trigger: crate::traits::MessageTriggerContext::default(),
            scope: crate::traits::ConversationScope {
                kind: crate::traits::ConversationScopeKind::Room,
                id: "channel:room1".to_string(),
            },
        };
        assert!(source_is_trusted_for_auto_install(
            &msg,
            &["discord".to_string()]
        ));
        assert!(source_is_trusted_for_auto_install(
            &msg,
            &["discord:room1".to_string()]
        ));
        assert!(source_is_trusted_for_auto_install(
            &msg,
            &["discord:room1:usera".to_string()]
        ));
        assert!(!source_is_trusted_for_auto_install(
            &msg,
            &["slack".to_string()]
        ));
    }

    #[test]
    fn retries_empty_channel_event_stream_on_decode_error() {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        assert!(should_retry_channel_event_stream(
            "error decoding response body",
            "",
            deadline
        ));
    }

    #[test]
    fn does_not_retry_channel_event_stream_after_content_arrives() {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        assert!(!should_retry_channel_event_stream(
            "error decoding response body",
            "partial reply",
            deadline
        ));
    }

    fn pending_test_message(sender: &str, scope_id: &str, content: &str) -> ChannelMessage {
        ChannelMessage {
            id: format!("{sender}-{scope_id}"),
            sender: sender.to_string(),
            reply_target: scope_id.to_string(),
            content: content.to_string(),
            channel: "discord".to_string(),
            timestamp: chrono::Utc::now(),
            attachment: None,
            attachment_url: None,
            attachment_path: None,
            attachment_mime: None,
            attachment_filename: None,
            trigger: crate::traits::MessageTriggerContext::default(),
            scope: crate::traits::ConversationScope {
                kind: crate::traits::ConversationScopeKind::Room,
                id: scope_id.to_string(),
            },
        }
    }

    #[tokio::test]
    async fn pending_channel_interaction_matches_same_sender_and_scope() {
        let map: PendingChannelInteractionMap = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let msg = pending_test_message("alice", "room-1", "answer");
        let response = ChannelAutomationDraftApiResponse {
            draft: ChannelAutomationDraftApiRecord {
                draft_id: "draft-1".to_string(),
                status: "collecting".to_string(),
                expires_at_ms: now_unix_ms() + 60_000,
            },
            message: Some("question".to_string()),
        };
        remember_pending_channel_automation_draft(&msg, &response, &map).await;

        assert_eq!(
            pending_channel_automation_draft_id(&msg, &map).await,
            Some("draft-1".to_string())
        );
        assert_eq!(
            pending_channel_automation_draft_id(
                &pending_test_message("bob", "room-1", "answer"),
                &map
            )
            .await,
            None
        );
        assert_eq!(
            pending_channel_automation_draft_id(
                &pending_test_message("alice", "room-2", "answer"),
                &map
            )
            .await,
            None
        );
    }

    #[tokio::test]
    async fn pending_channel_interaction_expires_and_can_clear() {
        let map: PendingChannelInteractionMap = Arc::new(tokio::sync::Mutex::new(HashMap::new()));
        let msg = pending_test_message("alice", "room-1", "answer");
        {
            let mut guard = map.lock().await;
            guard.insert(
                pending_channel_interaction_key(&msg),
                PendingChannelInteraction {
                    draft_id: "draft-expired".to_string(),
                    expires_at_ms: now_unix_ms().saturating_sub(1),
                },
            );
        }
        assert_eq!(pending_channel_automation_draft_id(&msg, &map).await, None);

        let response = ChannelAutomationDraftApiResponse {
            draft: ChannelAutomationDraftApiRecord {
                draft_id: "draft-clear".to_string(),
                status: "preview_ready".to_string(),
                expires_at_ms: now_unix_ms() + 60_000,
            },
            message: Some("preview".to_string()),
        };
        remember_pending_channel_automation_draft(&msg, &response, &map).await;
        clear_pending_channel_automation_draft(&msg, &map).await;
        assert_eq!(pending_channel_automation_draft_id(&msg, &map).await, None);
    }

    #[test]
    fn channel_automation_confirm_cancel_text_is_plain_text_only() {
        assert!(is_channel_automation_confirm_text("confirm"));
        assert!(is_channel_automation_cancel_text("never mind"));
        assert!(!is_channel_automation_confirm_text("/confirm"));
        assert!(!is_channel_automation_cancel_text("/cancel"));
    }

    #[test]
    fn parse_pending_command() {
        assert!(matches!(
            parse_slash_command("/pending"),
            Some(SlashCommand::Pending)
        ));
    }

    #[test]
    fn parse_rework_command_with_run_id_and_feedback() {
        let cmd = parse_slash_command("/rework auto-v2-run-abc123 tighten the ICP filter");
        match cmd {
            Some(SlashCommand::Rework { run_id, feedback }) => {
                assert_eq!(run_id, "auto-v2-run-abc123");
                assert_eq!(feedback, "tighten the ICP filter");
            }
            other => panic!("expected Rework, got {other:?}"),
        }
    }

    #[test]
    fn parse_rework_without_feedback_returns_none() {
        // /rework <run_id> alone is not actionable — feedback is required.
        // Returning None falls through to the standard "unknown command" path
        // which guides the user to /help.
        assert!(parse_slash_command("/rework auto-v2-run-abc123").is_none());
        assert!(parse_slash_command("/rework").is_none());
    }

    #[test]
    fn rework_help_lists_in_capabilities_registry() {
        let registry = crate::channel_registry::BUILTIN_CHANNEL_COMMANDS;
        assert!(
            registry.iter().any(|c| c.name == "pending"),
            "/pending must be in BUILTIN_CHANNEL_COMMANDS so registry-driven help shows it"
        );
        assert!(
            registry.iter().any(|c| c.name == "rework"),
            "/rework must be in BUILTIN_CHANNEL_COMMANDS so registry-driven help shows it"
        );
    }

    #[test]
    fn pending_and_rework_are_disabled_in_public_demo() {
        let registry = crate::channel_registry::BUILTIN_CHANNEL_COMMANDS;
        let pending = registry
            .iter()
            .find(|c| c.name == "pending")
            .expect("pending registered");
        let rework = registry
            .iter()
            .find(|c| c.name == "rework")
            .expect("rework registered");
        assert!(!pending.enabled_for_public_demo);
        assert!(!rework.enabled_for_public_demo);
    }
}
