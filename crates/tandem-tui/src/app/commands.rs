use serde_json::{json, Value};
use tandem_core::engine_api_token_file_path;
use tokio::time::sleep;

use super::plan_helpers;
use crate::app::{
    Action, AgentStatus, App, AppState, ContentBlock, EngineConnectionSource, EngineStalePolicy,
    MessageRole, ModalState, SetupStep, TandemMode, TaskStatus, UiMode,
};
use crate::command_catalog::HELP_TEXT;

macro_rules! basic_command_match_arms {
    () => {
        include!("commands_parts/match_arms_part01.rs");
        include!("commands_parts/match_arms_part02.rs");
        include!("commands_parts/match_arms_part03.rs");
    };
}

pub(super) async fn try_execute_basic_command(
    app: &mut App,
    cmd_name: &str,
    args: &[&str],
) -> Option<String> {
    match cmd_name {
        basic_command_match_arms!(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{
        ChatMessage, ComposerInputState, ModalState, PendingPermissionRequest, PendingRequest,
        PendingRequestKind, PlanFeedbackWizardState, Task, UiMode,
    };
    use crate::crypto::keystore::SecureKeyStore;
    use crate::net::client::EngineClient;
    use crate::net::client::{ProviderCatalog, Session, SessionTime};
    use std::collections::HashMap;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread::JoinHandle;
    use std::time::Duration;
    use tandem_wire::{WireProviderEntry, WireProviderModel};

    #[tokio::test]
    async fn rollback_commands_render_engine_responses() {
        let server = MockServer::start(HashMap::from([
            (
                "/context/runs/run-1/checkpoints/mutations/rollback-preview".to_string(),
                json_response(
                    r#"{"steps":[{"seq":3,"event_id":"evt-1","tool":"edit_file","executable":true,"operation_count":2},{"seq":4,"event_id":"evt-2","tool":"read_file","executable":false,"operation_count":1}],"step_count":2,"executable_step_count":1,"advisory_step_count":1,"executable":false}"#,
                ),
            ),
            (
                "/context/runs/run-1/checkpoints/mutations/rollback-history".to_string(),
                json_response(
                    r#"{"entries":[{"seq":7,"ts_ms":200,"event_id":"evt-rollback-2","outcome":"blocked","selected_event_ids":["evt-1"],"applied_step_count":0,"applied_operation_count":0,"reason":"approval required"},{"seq":6,"ts_ms":100,"event_id":"evt-rollback-1","outcome":"applied","selected_event_ids":["evt-1"],"applied_step_count":1,"applied_operation_count":2,"applied_by_action":{"rewrite_file":2}}],"summary":{"entry_count":2,"by_outcome":{"applied":1,"blocked":1}}}"#,
                ),
            ),
            (
                "/context/runs/run-1/checkpoints/mutations/rollback-execute".to_string(),
                json_response(
                    r#"{"applied":true,"selected_event_ids":["evt-1"],"applied_step_count":1,"applied_operation_count":2,"missing_event_ids":[],"reason":null}"#,
                ),
            ),
        ]))
        .expect("mock server");

        let mut app = App::new();
        app.client = Some(EngineClient::new(server.base_url()));

        let preview = app
            .execute_command("/context_run_rollback_preview run-1")
            .await;
        assert!(preview.contains("Rollback preview (run-1)"));
        assert!(preview.contains("evt-1"));

        let history = app
            .execute_command("/context_run_rollback_history run-1")
            .await;
        assert!(history.contains("Rollback receipts (run-1)"));
        assert!(history.contains("outcome=applied"));
        assert!(history.contains("outcome=blocked"));

        let execute = app
            .execute_command("/context_run_rollback_execute run-1 --ack evt-1")
            .await;
        assert!(execute.contains("Rollback execute (run-1)"));
        assert!(execute.contains("selected: evt-1"));
    }

    #[tokio::test]
    async fn recent_command_helper_lists_replays_and_clears() {
        let mut app = App::new();

        let mode = app.execute_command("/mode coder").await;
        assert!(mode.contains("Mode set to: Coder"));

        let workspace = app.execute_command("/workspace show").await;
        assert!(workspace.contains("Current workspace directory:"));

        let recent = app.execute_command("/recent").await;
        assert!(recent.contains("1. /workspace show"));
        assert!(recent.contains("2. /mode coder"));

        let replay = app.execute_command("/recent run 2").await;
        assert!(replay.contains("Replayed recent command #2: /mode coder"));
        assert!(replay.contains("Mode set to: Coder"));

        let cleared = app.execute_command("/recent clear").await;
        assert_eq!(cleared, "Cleared 2 recent command(s).");
        assert_eq!(
            app.execute_command("/recent").await,
            "No recent slash commands yet."
        );
    }

    #[tokio::test]
    async fn session_commands_list_and_switch_sessions() {
        let mut app = App::new();
        app.sessions = vec![
            Session {
                id: "s-1".to_string(),
                title: "First".to_string(),
                directory: None,
                workspace_root: None,
                time: Some(SessionTime {
                    created: Some(1),
                    updated: Some(2),
                }),
            },
            Session {
                id: "s-2".to_string(),
                title: "Second".to_string(),
                directory: None,
                workspace_root: None,
                time: Some(SessionTime {
                    created: Some(3),
                    updated: Some(4),
                }),
            },
        ];
        app.selected_session_index = 1;
        app.state = chat_state("s-1");

        let sessions = app.execute_command("/sessions").await;
        assert!(sessions.contains("→ Second (ID: s-2)"));
        assert!(sessions.contains("  First (ID: s-1)"));

        let switched = app.execute_command("/use s-2").await;
        assert_eq!(switched, "Switched to session: s-2");
        assert_eq!(app.selected_session_index, 1);
        match &app.state {
            AppState::Chat {
                session_id,
                active_agent_index,
                agents,
                ..
            } => {
                assert_eq!(session_id, "s-2");
                assert_eq!(agents[*active_agent_index].session_id, "s-2");
            }
            _ => panic!("expected chat state"),
        }
    }

    #[tokio::test]
    async fn key_commands_list_keys_and_open_wizard() {
        let mut app = App::new();
        let path =
            std::env::temp_dir().join(format!("tandem-tui-keystore-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut keystore = SecureKeyStore::load(&path, vec![7; 32]).expect("keystore");
        keystore
            .set("openai_api_key", "secret".to_string())
            .expect("set key");
        app.keystore = Some(keystore);
        app.current_provider = Some("openai".to_string());
        app.provider_catalog = Some(ProviderCatalog {
            all: vec![WireProviderEntry {
                id: "openai".to_string(),
                name: Some("OpenAI".to_string()),
                models: HashMap::<String, WireProviderModel>::new(),
                catalog_source: None,
                catalog_status: None,
                catalog_message: None,
            }],
            connected: vec!["openai".to_string()],
            default: Some("openai".to_string()),
        });

        let keys = app.execute_command("/keys").await;
        assert!(keys.contains("Configured providers:"));
        assert!(keys.contains("openai - configured"));

        let wizard = app.execute_command("/key set").await;
        assert_eq!(wizard, "Opening key setup wizard for openai...");
        match &app.state {
            AppState::SetupWizard { step, .. } => assert_eq!(*step, SetupStep::EnterApiKey),
            _ => panic!("expected setup wizard"),
        }

        let _ = std::fs::remove_file(PathBuf::from(path));
    }

    #[tokio::test]
    async fn queue_commands_manage_followups_and_errors() {
        let mut app = App::new();
        app.state = chat_state("s-1");
        if let AppState::Chat {
            agents, messages, ..
        } = &mut app.state
        {
            agents[0]
                .follow_up_queue
                .push_back("first follow-up".to_string());
            agents[0].steering_message = Some("steer".to_string());
            messages.push(ChatMessage {
                role: MessageRole::System,
                content: vec![ContentBlock::Text("Something failed badly".to_string())],
            });
        }

        let queue = app.execute_command("/queue").await;
        assert!(queue.contains("steering: yes"));
        assert!(queue.contains("follow-ups: 1"));
        assert!(queue.contains("first follow-up"));

        let error = app.execute_command("/last_error").await;
        assert_eq!(error, "Something failed badly");

        let cleared = app.execute_command("/queue clear").await;
        assert_eq!(cleared, "Cleared queued steering and follow-up messages.");

        let messages = app.execute_command("/messages 25").await;
        assert_eq!(messages, "Message history not implemented yet. (limit: 25)");
    }

    #[tokio::test]
    async fn steer_followup_and_cancel_commands_update_active_agent_state() {
        let mut app = App::new();
        app.state = chat_state("s-1");
        if let AppState::Chat { agents, .. } = &mut app.state {
            agents[0].status = AgentStatus::Running;
            agents[0].active_run_id = Some("run-1".to_string());
        }

        let steer = app.execute_command("/steer check logs").await;
        assert_eq!(steer, "Steering message queued.");
        match &app.state {
            AppState::Chat { command_input, .. } => assert_eq!(command_input.text(), "check logs"),
            _ => panic!("expected chat state"),
        }

        let followup = app.execute_command("/followup inspect rollback").await;
        assert_eq!(followup, "Queued follow-up message (#1).");
        match &app.state {
            AppState::Chat { agents, .. } => {
                assert_eq!(
                    agents[0].follow_up_queue.front().map(String::as_str),
                    Some("inspect rollback")
                );
            }
            _ => panic!("expected chat state"),
        }

        let cancel = app.execute_command("/cancel").await;
        assert_eq!(cancel, "Cancel requested for active agent.");
        match &app.state {
            AppState::Chat { agents, .. } => {
                assert_eq!(agents[0].status, AgentStatus::Idle);
                assert_eq!(agents[0].active_run_id, None);
            }
            _ => panic!("expected chat state"),
        }
    }

    #[tokio::test]
    async fn task_and_prompt_commands_update_chat_state() {
        let mut app = App::new();
        app.state = chat_state("s-1");

        let added = app.execute_command("/task add investigate rollback").await;
        assert_eq!(added, "Task added: investigate rollback (ID: task-1)");

        let pinned = app.execute_command("/task pin task-1").await;
        assert_eq!(pinned, "Task task-1 pinned: true");

        let worked = app.execute_command("/task work task-1").await;
        assert_eq!(worked, "Task task-1 marked as work");

        let listed = app.execute_command("/task list").await;
        assert!(listed.contains("[task-1] investigate rollback (Working) - Pinned: true"));

        let prompt = app.execute_command("/prompt review status").await;
        assert_eq!(prompt, "Prompt sent.");
        match &app.state {
            AppState::Chat {
                messages, agents, ..
            } => {
                assert!(messages
                    .iter()
                    .any(|m| m.content.iter().any(|block| match block {
                        ContentBlock::Text(text) => text == "review status",
                        _ => false,
                    })));
                assert!(agents[0].messages.iter().any(|m| m.content.iter().any(
                    |block| match block {
                        ContentBlock::Text(text) => text == "review status",
                        _ => false,
                    }
                )));
            }
            _ => panic!("expected chat state"),
        }
    }

    #[tokio::test]
    async fn title_command_renames_current_session() {
        let server = MockServer::start(HashMap::from([(
            "/session/s-1".to_string(),
            json_response(
                r#"{"id":"s-1","title":"Renamed Session","directory":null,"workspaceRoot":null,"time":{"created":1,"updated":2}}"#,
            ),
        )]))
        .expect("mock server");
        let mut app = App::new();
        app.client = Some(EngineClient::new(server.base_url()));
        app.sessions = vec![Session {
            id: "s-1".to_string(),
            title: "Original Session".to_string(),
            directory: None,
            workspace_root: None,
            time: Some(SessionTime {
                created: Some(1),
                updated: Some(2),
            }),
        }];
        app.state = chat_state("s-1");

        let renamed = app.execute_command("/title Renamed Session").await;
        assert_eq!(renamed, "Session renamed to: Renamed Session");
        assert_eq!(app.sessions[0].title, "Renamed Session");
    }

    #[tokio::test]
    async fn mission_commands_render_list_detail_and_create_views() {
        let mission =
            mission_state_json("m-1", "running", "Stabilize rollback", "Ship safer undo", 3);
        let created = mission_state_json("m-2", "draft", "Fresh mission", "Start clean", 1);
        let server = MockServer::start(HashMap::from([
            (
                "/mission".to_string(),
                json_response(
                    &serde_json::json!({
                        "missions": [serde_json::from_str::<serde_json::Value>(&mission).expect("mission")]
                    })
                    .to_string(),
                ),
            ),
            (
                "/mission/m-1".to_string(),
                json_response(
                    &serde_json::json!({
                        "mission": serde_json::from_str::<serde_json::Value>(&mission).expect("mission detail")
                    })
                    .to_string(),
                ),
            ),
        ]))
        .expect("mock server");
        let create_server = MockServer::start(HashMap::from([(
            "/mission".to_string(),
            json_response(
                &serde_json::json!({
                    "mission": serde_json::from_str::<serde_json::Value>(&created).expect("created mission")
                })
                .to_string(),
            ),
        )]))
        .expect("create mock server");

        let mut app = App::new();
        app.client = Some(EngineClient::new(server.base_url()));

        let list = app.execute_command("/missions").await;
        assert!(list.contains("Missions:"));
        assert!(list.contains("m-1 [running] Stabilize rollback (work_items=1)"));

        let detail = app.execute_command("/mission_get m-1").await;
        assert!(detail.contains("Mission m-1 [running]"));
        assert!(detail.contains("Title: Stabilize rollback"));
        assert!(detail.contains("Goal: Ship safer undo"));
        assert!(detail.contains("- Verify rollback [review]"));

        app.client = Some(EngineClient::new(create_server.base_url()));
        let created = app
            .execute_command("/mission_create Fresh mission :: Start clean :: Draft task")
            .await;
        assert_eq!(created, "Created mission m-2: Fresh mission");
    }

    #[tokio::test]
    async fn mission_event_commands_apply_expected_engine_events() {
        let mission =
            mission_state_json("m-1", "running", "Stabilize rollback", "Ship safer undo", 5);
        let server = MockServer::start(HashMap::from([(
            "/mission/m-1/event".to_string(),
            json_response(
                &serde_json::json!({
                    "mission": serde_json::from_str::<serde_json::Value>(&mission).expect("mission event result"),
                    "commands": [{ "type": "notify" }]
                })
                .to_string(),
            ),
        )]))
        .expect("mock server");

        let mut app = App::new();
        app.client = Some(EngineClient::new(server.base_url()));

        let invalid = app.execute_command("/mission_event m-1 nope").await;
        assert!(invalid.starts_with("Invalid event JSON:"));

        let applied = app
            .execute_command(r#"/mission_event m-1 {"type":"custom","state":"ok"}"#)
            .await;
        assert_eq!(
            applied,
            "Applied event to mission m-1 (revision=5, commands=1)"
        );

        let started = app.execute_command("/mission_start m-1").await;
        assert_eq!(started, "Mission started m-1 (revision=5)");

        let review_ok = app
            .execute_command("/mission_review_ok m-1 w-1 gate-7")
            .await;
        assert_eq!(review_ok, "Review approved for m-1:w-1 (revision=5)");

        let test_ok = app.execute_command("/mission_test_ok m-1 w-1").await;
        assert_eq!(test_ok, "Test approved for m-1:w-1 (revision=5)");

        let review_no = app
            .execute_command("/mission_review_no m-1 w-1 needs more logs")
            .await;
        assert_eq!(review_no, "Review denied for m-1:w-1 (revision=5)");
    }

    #[tokio::test]
    async fn agent_team_commands_render_summary_and_list_views() {
        let server = MockServer::start(HashMap::from([
            (
                "/agent-team/missions".to_string(),
                json_response(
                    r#"{"missions":[{"missionID":"mission-1","instanceCount":3,"runningCount":1,"completedCount":1,"failedCount":0,"cancelledCount":1}]}"#,
                ),
            ),
            (
                "/agent-team/instances".to_string(),
                json_response(
                    r#"{"instances":[{"instanceID":"agent-1","role":"reviewer","missionID":"mission-1","sessionID":"s-1","status":"running","parentInstanceID":"lead-1"}]}"#,
                ),
            ),
            (
                "/agent-team/approvals".to_string(),
                json_response(
                    r#"{"spawnApprovals":[{"approvalID":"spawn-1","createdAtMs":1,"request":{"missionID":"mission-1","reason":"Need helper"}}],"toolApprovals":[{"approvalID":"tool-1","sessionID":"s-1","toolCallID":"call-1","tool":"shell","status":"pending"}]}"#,
                ),
            ),
        ]))
        .expect("mock server");

        let mut app = App::new();
        app.client = Some(EngineClient::new(server.base_url()));

        let summary = app.execute_command("/agent-team").await;
        assert!(summary.contains("Agent-Team Summary:"));
        assert!(summary.contains("Missions: 1"));
        assert!(summary.contains("Instances: 1"));
        assert!(summary.contains("Spawn approvals: 1"));
        assert!(summary.contains("Tool approvals: 1"));

        let missions = app.execute_command("/agent-team missions").await;
        assert!(missions.contains("Agent-Team Missions:"));
        assert!(missions.contains("mission-1 total=3 running=1 done=1 failed=0 cancelled=1"));

        let instances = app.execute_command("/agent-team instances mission-1").await;
        assert!(instances.contains("Agent-Team Instances:"));
        assert!(instances
            .contains("agent-1 role=reviewer mission=mission-1 status=running parent=lead-1"));

        let approvals = app.execute_command("/agent-team approvals").await;
        assert!(approvals.contains("Agent-Team Approvals:"));
        assert!(approvals.contains("spawn approval spawn-1"));
        assert!(approvals.contains("tool approval tool-1 (shell)"));
    }

    #[tokio::test]
    async fn agent_team_commands_handle_bindings_and_permission_replies() {
        let server = MockServer::start(HashMap::from([
            (
                "/agent-team/approvals/spawn/spawn-1/approve".to_string(),
                json_response(r#"{"ok":true}"#),
            ),
            (
                "/agent-team/approvals/spawn/spawn-1/deny".to_string(),
                json_response(r#"{"ok":true}"#),
            ),
            (
                "/permission/tool-1/reply".to_string(),
                json_response(r#"{"ok":true}"#),
            ),
        ]))
        .expect("mock server");

        let mut app = App::new();
        app.client = Some(EngineClient::new(server.base_url()));

        let bindings = app.execute_command("/agent-team bindings").await;
        assert!(
            bindings == "No local agent-team state found."
                || bindings == "No local agent-team bindings found."
        );

        let approve_spawn = app
            .execute_command("/agent-team approve spawn spawn-1 looks good")
            .await;
        assert_eq!(approve_spawn, "Approved spawn approval spawn-1.");

        let approve_tool = app.execute_command("/agent-team approve tool tool-1").await;
        assert_eq!(approve_tool, "Approved tool request tool-1.");

        let deny_spawn = app.execute_command("/agent-team deny spawn spawn-1").await;
        assert_eq!(deny_spawn, "Denied spawn approval spawn-1.");

        let deny_tool = app.execute_command("/agent-team deny tool tool-1").await;
        assert_eq!(deny_tool, "Denied tool request tool-1.");
    }

    #[tokio::test]
    async fn agent_commands_manage_agent_panes() {
        let server = MockServer::start(HashMap::from([(
            "/api/session".to_string(),
            json_response(
                r#"{"id":"s-2","title":"A2 session","directory":null,"workspaceRoot":null,"time":{"created":1,"updated":2}}"#,
            ),
        )]))
        .expect("mock server");

        let mut app = App::new();
        app.client = Some(EngineClient::new(server.base_url()));
        app.state = chat_state("s-1");

        let created = app.execute_command("/agent new").await;
        assert_eq!(created, "Created new agent.");

        let listed = app.execute_command("/agent list").await;
        assert!(listed.contains("Agents:"));
        assert!(listed.contains("> A2 [s-2] Idle"));
        assert!(listed.contains("  A1 [s-1] Idle"));

        let switched = app.execute_command("/agent use A1").await;
        assert_eq!(switched, "Switched to A1.");

        let closed = app.execute_command("/agent close").await;
        assert_eq!(closed, "Closed active agent.");
        match &app.state {
            AppState::Chat {
                agents,
                active_agent_index,
                ..
            } => {
                assert_eq!(agents.len(), 1);
                assert_eq!(*active_agent_index, 0);
                assert_eq!(agents[0].agent_id, "A2");
            }
            _ => panic!("expected chat state"),
        }
    }

    #[tokio::test]
    async fn agent_fanout_creates_grid_and_switches_mode() {
        let mut app = App::new();
        app.state = chat_state("s-1");
        app.current_mode = TandemMode::Plan;

        let result = app.execute_command("/agent fanout 3").await;
        assert_eq!(
            result,
            "Started fanout: 3 total agents (created 2). Grid view enabled. Mode auto-switched from plan -> orchestrate."
        );
        assert_eq!(app.current_mode, TandemMode::Orchestrate);
        match &app.state {
            AppState::Chat {
                agents,
                ui_mode,
                grid_page,
                ..
            } => {
                assert_eq!(agents.len(), 3);
                assert_eq!(*ui_mode, UiMode::Grid);
                assert_eq!(*grid_page, 0);
                assert_eq!(agents[1].agent_id, "A2");
                assert_eq!(agents[2].agent_id, "A3");
            }
            _ => panic!("expected chat state"),
        }
    }

    #[tokio::test]
    async fn preset_commands_render_index_and_agent_views() {
        let server = MockServer::start(HashMap::from([
            (
                "/presets/index".to_string(),
                json_response(
                    r#"{"index":{"skill_modules":[{"id":"skill.a","version":"1","kind":"skill_module","layer":"base","path":"skills/a","tags":[],"publisher":null,"required_capabilities":[]}],"agent_presets":[{"id":"agent.main","version":"1","kind":"agent_preset","layer":"base","path":"agents/main","tags":[],"publisher":null,"required_capabilities":[]}],"automation_presets":[],"pack_presets":[],"generated_at_ms":42}}"#,
                ),
            ),
            (
                "/presets/compose/preview".to_string(),
                json_response(r#"{"composition":{"prompt":"merged preset prompt"}}"#),
            ),
            (
                "/presets/capability_summary".to_string(),
                json_response(r#"{"summary":{"required":["shell"],"optional":["git"]}}"#),
            ),
            (
                "/presets/fork".to_string(),
                json_response(r#"{"id":"agent-copy","kind":"agent_preset","layer":"override"}"#),
            ),
        ]))
        .expect("mock server");

        let mut app = App::new();
        app.client = Some(EngineClient::new(server.base_url()));

        let index = app.execute_command("/preset index").await;
        assert!(index.contains("Preset index:"));
        assert!(index.contains("skill_modules: 1"));
        assert!(index.contains("agent_presets: 1"));
        assert!(index.contains("generated_at_ms: 42"));

        let compose = app
            .execute_command(r#"/preset agent compose Base prompt :: [{"id":"frag-1","phase":"plan","content":"think"}]"#)
            .await;
        assert!(compose.contains("Agent compose preview:"));
        assert!(compose.contains("merged preset prompt"));

        let summary = app
            .execute_command("/preset agent summary required=shell optional=git")
            .await;
        assert!(summary.contains("Agent capability summary:"));
        assert!(summary.contains("\"required\""));

        let fork = app
            .execute_command("/preset agent fork presets/base.yaml agent-copy")
            .await;
        assert!(fork.contains("Forked agent preset override:"));
        assert!(fork.contains("agent-copy"));
    }

    #[tokio::test]
    async fn preset_automation_commands_validate_and_save() {
        let server = MockServer::start(HashMap::from([
            (
                "/presets/capability_summary".to_string(),
                json_response(r#"{"summary":{"score":"ok","required":["shell"]}}"#),
            ),
            (
                "/presets/overrides/automation_preset/nightly".to_string(),
                json_response(r#"{"ok":true,"path":"automation_preset/nightly"}"#),
            ),
        ]))
        .expect("mock server");

        let mut app = App::new();
        app.client = Some(EngineClient::new(server.base_url()));

        let invalid = app
            .execute_command("/preset agent compose Base prompt :: {\"bad\":true}")
            .await;
        assert_eq!(
            invalid,
            "fragments_json must be a JSON array of {id,phase,content}"
        );

        let summary = app
            .execute_command(
                r#"/preset automation summary [{"required":["shell"],"optional":["git"]}] :: required=python :: optional=gh"#,
            )
            .await;
        assert!(summary.contains("Automation capability summary (1 tasks):"));
        assert!(summary.contains("\"score\""));

        let saved = app
            .execute_command(
                r#"/preset automation save nightly :: [{"required":["shell"],"optional":["git"]}] :: required=python :: optional=gh"#,
            )
            .await;
        assert!(saved.contains("Saved automation preset override `nightly` with 1 task(s)."));
        assert!(saved.contains("automation_preset/nightly"));
    }

    #[tokio::test]
    async fn routine_commands_validate_usage_and_engine_requirements() {
        let mut app = App::new();

        assert_eq!(
            app.execute_command("/routines").await,
            "Engine client not connected."
        );
        assert_eq!(
            app.execute_command("/routine_create").await,
            "Usage: /routine_create <id> <interval_seconds> <entrypoint>"
        );
        assert_eq!(
            app.execute_command("/routine_edit nightly").await,
            "Usage: /routine_edit <id> <interval_seconds>"
        );
        assert_eq!(
            app.execute_command("/routine_run_now").await,
            "Usage: /routine_run_now <id> [run_count]"
        );
        assert_eq!(
            app.execute_command("/routine_history").await,
            "Usage: /routine_history <id> [limit]"
        );
        assert_eq!(
            app.execute_command("/routine_create nightly 60 plan nightly")
                .await,
            "Engine client not connected."
        );
        assert_eq!(
            app.execute_command("/routine_delete nightly").await,
            "Engine client not connected."
        );

        app.client = Some(EngineClient::new("http://127.0.0.1:1".to_string()));

        assert_eq!(
            app.execute_command("/routine_create nightly nope plan nightly")
                .await,
            "interval_seconds must be a positive integer."
        );
        assert_eq!(
            app.execute_command("/routine_edit nightly nope").await,
            "interval_seconds must be a positive integer."
        );
        assert_eq!(
            app.execute_command("/routine_run_now nightly nope").await,
            "run_count must be a positive integer."
        );
        assert_eq!(
            app.execute_command("/routine_history nightly nope").await,
            "limit must be a positive integer."
        );
    }

    #[tokio::test]
    async fn config_requests_and_copy_commands_use_expected_state() {
        let mut app = App::new();
        app.current_provider = Some("openai".to_string());
        app.current_model = Some("gpt-4.1".to_string());

        let config = app.execute_command("/config").await;
        assert!(config.contains("Configuration:"));
        assert!(config.contains("Current Provider: openai"));
        assert!(config.contains("Current Model: gpt-4.1"));

        let copy = app.execute_command("/copy").await;
        assert_eq!(copy, "Clipboard copy works in chat screens only.");

        app.state = chat_state("s-1");
        if let AppState::Chat {
            pending_requests,
            request_cursor,
            ..
        } = &mut app.state
        {
            pending_requests.push(PendingRequest {
                session_id: "s-1".to_string(),
                agent_id: "A1".to_string(),
                kind: PendingRequestKind::Permission(PendingPermissionRequest {
                    id: "perm-1".to_string(),
                    tool: "shell".to_string(),
                    args: None,
                    args_source: None,
                    args_integrity: None,
                    query: Some("ls".to_string()),
                    status: Some("pending".to_string()),
                }),
            });
            *request_cursor = 99;
        }

        let requests = app.execute_command("/requests").await;
        assert_eq!(requests, "Opened request center (1 pending).");
        match &app.state {
            AppState::Chat {
                modal,
                request_cursor,
                ..
            } => {
                assert_eq!(modal, &Some(ModalState::RequestCenter));
                assert_eq!(*request_cursor, 0);
            }
            _ => panic!("expected chat state"),
        }
    }

    #[tokio::test]
    async fn permission_commands_reply_and_filter_pending_requests() {
        let server = MockServer::start(HashMap::from([
            (
                "/permission".to_string(),
                json_response(
                    r#"{"requests":[{"id":"perm-1","sessionID":"s-1","status":"pending"},{"id":"perm-2","sessionID":"s-2","status":"pending"},{"id":"perm-3","sessionID":"s-1","status":"approved"}],"rules":[]}"#,
                ),
            ),
            (
                "/permission/perm-1/reply".to_string(),
                json_response(r#"{"ok":true}"#),
            ),
            (
                "/permission/perm-9/reply".to_string(),
                json_response(r#"{"ok":true}"#),
            ),
        ]))
        .expect("mock server");

        let mut app = App::new();
        app.client = Some(EngineClient::new(server.base_url()));
        app.state = chat_state("s-1");

        let approve_all = app.execute_command("/approve all").await;
        assert_eq!(approve_all, "Approved 1 pending permission request(s).");

        let approve_one = app.execute_command("/approve perm-9 always").await;
        assert_eq!(approve_one, "Approved permission request perm-9.");

        let deny = app.execute_command("/deny perm-9").await;
        assert_eq!(deny, "Denied permission request perm-9.");

        let answer = app.execute_command("/answer perm-9 once").await;
        assert_eq!(answer, "Replied to permission request perm-9.");
    }

    #[tokio::test]
    async fn context_run_commands_render_list_detail_and_driver_views() {
        let run_one = context_run_state_json("run-1", "running", "Investigate rollback", 200);
        let run_two = context_run_state_json("run-2", "paused", "Review logs", 100);
        let replay_run = context_run_state_json("run-1", "running", "Investigate rollback", 200);
        let persisted_run = context_run_state_json("run-1", "paused", "Investigate rollback", 210);
        let next_run = context_run_state_json("run-1", "running", "Investigate rollback", 220);
        let server = MockServer::start(HashMap::from([
            (
                "/context/runs".to_string(),
                json_response(
                    &serde_json::json!({
                        "runs": [
                            serde_json::from_str::<serde_json::Value>(&run_two).expect("run two"),
                            serde_json::from_str::<serde_json::Value>(&run_one).expect("run one")
                        ]
                    })
                    .to_string(),
                ),
            ),
            (
                "/context/runs/run-1".to_string(),
                json_response(
                    &serde_json::json!({
                        "run": serde_json::from_str::<serde_json::Value>(&run_one).expect("detail run"),
                        "rollback_preview_summary": { "step_count": 2 },
                        "rollback_history_summary": { "entry_count": 1 },
                        "last_rollback_outcome": { "outcome": "applied", "reason": "manual" },
                        "rollback_policy": { "eligible": true, "required_policy_ack": "allow_rollback_execution" }
                    })
                    .to_string(),
                ),
            ),
            (
                "/context/runs/run-1/events".to_string(),
                json_response(
                    &serde_json::json!({
                        "events": [
                            {
                                "event_id": "evt-2",
                                "run_id": "run-1",
                                "seq": 2,
                                "ts_ms": 220,
                                "type": "meta_next_step_selected",
                                "status": "running",
                                "step_id": "step-2",
                                "payload": {
                                    "why_next_step": "Need edit verification",
                                    "selected_step_id": "step-2"
                                }
                            },
                            {
                                "event_id": "evt-1",
                                "run_id": "run-1",
                                "seq": 1,
                                "ts_ms": 200,
                                "type": "tool_completed",
                                "status": "running",
                                "step_id": "step-1",
                                "payload": {}
                            }
                        ]
                    })
                    .to_string(),
                ),
            ),
            (
                "/context/runs/run-1/blackboard".to_string(),
                json_response(
                    &serde_json::json!({
                        "blackboard": {
                            "facts": [{ "id": "fact-1", "ts_ms": 1, "text": "Rollback ready" }],
                            "decisions": [{ "id": "decision-1", "ts_ms": 2, "text": "Pause before execute" }],
                            "open_questions": [{ "id": "question-1", "ts_ms": 3, "text": "Need approval?" }],
                            "artifacts": [{ "id": "artifact-1", "ts_ms": 4, "path": "/tmp/plan.md", "artifact_type": "note" }],
                            "summaries": { "rolling": "summary", "latest_context_pack": "pack" },
                            "revision": 9
                        }
                    })
                    .to_string(),
                ),
            ),
            (
                "/context/runs/run-1/replay".to_string(),
                json_response(
                    &serde_json::json!({
                        "ok": true,
                        "run_id": "run-1",
                        "from_checkpoint": true,
                        "checkpoint_seq": 3,
                        "events_applied": 4,
                        "replay": serde_json::from_str::<serde_json::Value>(&replay_run).expect("replay"),
                        "persisted": serde_json::from_str::<serde_json::Value>(&persisted_run).expect("persisted"),
                        "drift": {
                            "mismatch": true,
                            "status_mismatch": true,
                            "why_next_step_mismatch": false,
                            "step_count_mismatch": true
                        }
                    })
                    .to_string(),
                ),
            ),
            (
                "/context/runs/run-1/driver/next".to_string(),
                json_response(
                    &serde_json::json!({
                        "ok": true,
                        "dry_run": true,
                        "run_id": "run-1",
                        "selected_step_id": "step-2",
                        "target_status": "running",
                        "why_next_step": "Need edit verification",
                        "run": serde_json::from_str::<serde_json::Value>(&next_run).expect("next")
                    })
                    .to_string(),
                ),
            ),
        ]))
        .expect("mock server");

        let mut app = App::new();
        app.client = Some(EngineClient::new(server.base_url()));

        let list = app.execute_command("/context_runs 1").await;
        assert!(list.contains("Context runs:"));
        assert!(list.contains("run-1 [running]"));
        assert!(!list.contains("run-2 [paused]"));

        let detail = app.execute_command("/context_run_get run-1").await;
        assert!(detail.contains("Context run run-1"));
        assert!(detail.contains("preview_steps: 2"));
        assert!(detail.contains("required_ack: allow_rollback_execution"));

        let events = app.execute_command("/context_run_events run-1 10").await;
        assert!(events.contains("Context run events (run-1):"));
        assert!(events.contains("meta_next_step_selected"));

        let blackboard = app.execute_command("/context_run_blackboard run-1").await;
        assert!(blackboard.contains("Context blackboard run-1"));
        assert!(blackboard.contains("facts: 1"));
        assert!(blackboard.contains("latest_context_pack: <present>"));

        let next = app.execute_command("/context_run_next run-1 dry").await;
        assert!(next.contains("ContextDriver next (preview)"));
        assert!(next.contains("selected_step: step-2"));

        let replay = app.execute_command("/context_run_replay run-1 3").await;
        assert!(replay.contains("Context replay run-1"));
        assert!(replay.contains("drift: true"));

        let lineage = app.execute_command("/context_run_lineage run-1 10").await;
        assert!(lineage.contains("Context decision lineage (run-1):"));
        assert!(lineage.contains("why=Need edit verification"));
    }

    #[tokio::test]
    async fn context_run_create_and_lifecycle_commands_render_engine_responses() {
        let created_run = context_run_state_json("run-1", "planning", "Investigate rollback", 50);
        let server = MockServer::start(HashMap::from([
            (
                "/context/runs".to_string(),
                json_response(
                    &serde_json::json!({
                        "run": serde_json::from_str::<serde_json::Value>(&created_run).expect("created run")
                    })
                    .to_string(),
                ),
            ),
            (
                "/context/runs/run-1/events".to_string(),
                json_response(
                    &serde_json::json!({
                        "event": {
                            "event_id": "evt-lifecycle",
                            "run_id": "run-1",
                            "seq": 7,
                            "ts_ms": 500,
                            "type": "run_updated",
                            "status": "running",
                            "step_id": null,
                            "payload": { "source": "tui" }
                        }
                    })
                    .to_string(),
                ),
            ),
        ]))
        .expect("mock server");

        let mut app = App::new();
        app.client = Some(EngineClient::new(server.base_url()));

        let created = app
            .execute_command("/context_run_create Investigate rollback")
            .await;
        assert_eq!(created, "Created context run run-1 [interactive].");

        let paused = app.execute_command("/context_run_pause run-1").await;
        assert_eq!(
            paused,
            "Context run run-1 paused (seq=7 event=evt-lifecycle)."
        );

        let resumed = app.execute_command("/context_run_resume run-1").await;
        assert_eq!(
            resumed,
            "Context run run-1 running (seq=7 event=evt-lifecycle)."
        );

        let cancelled = app.execute_command("/context_run_cancel run-1").await;
        assert_eq!(
            cancelled,
            "Context run run-1 cancelled (seq=7 event=evt-lifecycle)."
        );
    }

    #[tokio::test]
    async fn context_run_bind_and_sync_tasks_update_chat_state() {
        let synced_run = context_run_state_json("run-1", "running", "Investigate rollback", 300);
        let server = MockServer::start(HashMap::from([(
            "/context/runs/run-1/todos/sync".to_string(),
            json_response(
                &serde_json::json!({
                    "run": serde_json::from_str::<serde_json::Value>(&synced_run).expect("synced run")
                })
                .to_string(),
            ),
        )]))
        .expect("mock server");

        let mut app = App::new();
        app.client = Some(EngineClient::new(server.base_url()));
        app.state = chat_state("s-1");
        if let AppState::Chat { tasks, agents, .. } = &mut app.state {
            tasks.push(Task {
                id: "task-1".to_string(),
                description: "Investigate rollback".to_string(),
                status: TaskStatus::Working,
                pinned: true,
            });
            agents[0].active_run_id = Some("source-run".to_string());
        }

        let bound = app.execute_command("/context_run_bind run-1").await;
        assert_eq!(bound, "Bound A1 todowrite updates to context run run-1.");
        match &app.state {
            AppState::Chat { agents, .. } => {
                assert_eq!(agents[0].bound_context_run_id.as_deref(), Some("run-1"));
            }
            _ => panic!("expected chat state"),
        }

        let synced = app.execute_command("/context_run_sync_tasks run-1").await;
        assert!(synced.contains("Synced tasks into context run run-1."));
        assert!(synced.contains("status: running"));
        assert!(synced.contains("why_next_step: Need edit verification"));

        let cleared = app.execute_command("/context_run_bind off").await;
        assert_eq!(cleared, "Cleared context-run binding for A1.");
    }

    fn chat_state(session_id: &str) -> AppState {
        let agent = App::make_agent_pane("A1".to_string(), session_id.to_string());
        AppState::Chat {
            session_id: session_id.to_string(),
            command_input: ComposerInputState::new(),
            messages: Vec::new(),
            scroll_from_bottom: 0,
            tasks: Vec::<Task>::new(),
            active_task_id: None,
            agents: vec![agent],
            active_agent_index: 0,
            ui_mode: UiMode::Focus,
            grid_page: 0,
            modal: Option::<ModalState>::None,
            pending_requests: Vec::<PendingRequest>::new(),
            request_cursor: 0,
            permission_choice: 0,
            plan_wizard: PlanFeedbackWizardState::default(),
            last_plan_task_fingerprint: Vec::new(),
            plan_awaiting_approval: false,
            plan_multi_agent_prompt: None,
            plan_waiting_for_clarification_question: false,
            request_panel_expanded: false,
        }
    }

    fn context_run_state_json(
        run_id: &str,
        status: &str,
        objective: &str,
        updated_at_ms: u64,
    ) -> String {
        serde_json::json!({
            "run_id": run_id,
            "run_type": "interactive",
            "status": status,
            "objective": objective,
            "workspace": {
                "workspace_id": "ws-1",
                "canonical_path": "/tmp/workspace",
                "lease_epoch": 1
            },
            "steps": [
                { "step_id": "step-1", "title": "Inspect", "status": "done" },
                { "step_id": "step-2", "title": "Verify", "status": "runnable" }
            ],
            "why_next_step": "Need edit verification",
            "revision": 4,
            "created_at_ms": 10,
            "updated_at_ms": updated_at_ms
        })
        .to_string()
    }

    fn mission_state_json(
        mission_id: &str,
        status: &str,
        title: &str,
        goal: &str,
        revision: u64,
    ) -> String {
        serde_json::json!({
            "mission_id": mission_id,
            "status": status,
            "spec": {
                "mission_id": mission_id,
                "title": title,
                "goal": goal,
                "success_criteria": [],
                "entrypoint": null,
                "budgets": {},
                "capabilities": {},
                "metadata": null
            },
            "work_items": [
                {
                    "work_item_id": "w-1",
                    "title": "Verify rollback",
                    "detail": null,
                    "status": "review",
                    "depends_on": [],
                    "assigned_agent": null,
                    "run_id": null,
                    "artifact_refs": [],
                    "metadata": null
                }
            ],
            "revision": revision,
            "updated_at_ms": 100
        })
        .to_string()
    }

    struct MockServer {
        addr: std::net::SocketAddr,
        running: Arc<AtomicBool>,
        worker: Option<JoinHandle<()>>,
    }

    impl MockServer {
        fn start(routes: HashMap<String, String>) -> anyhow::Result<Self> {
            let listener = TcpListener::bind("127.0.0.1:0")?;
            listener.set_nonblocking(true)?;
            let addr = listener.local_addr()?;
            let running = Arc::new(AtomicBool::new(true));
            let worker_running = Arc::clone(&running);
            let worker = std::thread::spawn(move || {
                while worker_running.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let _ = handle_request(stream, &routes);
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        Err(_) => break,
                    }
                }
            });
            Ok(Self {
                addr,
                running,
                worker: Some(worker),
            })
        }

        fn base_url(&self) -> String {
            format!("http://{}", self.addr)
        }
    }

    impl Drop for MockServer {
        fn drop(&mut self) {
            self.running.store(false, Ordering::SeqCst);
            let _ = TcpStream::connect(self.addr);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }

    fn json_response(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
    }

    fn handle_request(
        mut stream: TcpStream,
        routes: &HashMap<String, String>,
    ) -> anyhow::Result<()> {
        stream.set_read_timeout(Some(Duration::from_millis(250)))?;
        let mut buf = [0u8; 8192];
        let n = stream.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        let request = String::from_utf8_lossy(&buf[..n]);
        let first_line = request.lines().next().unwrap_or_default();
        let raw_path = first_line.split_whitespace().nth(1).unwrap_or("/");
        let path = raw_path.split('?').next().unwrap_or(raw_path);
        let response = routes.get(path).cloned().unwrap_or_else(|| {
            json_response(r#"{"error":"not found"}"#).replacen("200 OK", "404 Not Found", 1)
        });
        stream.write_all(response.as_bytes())?;
        stream.flush()?;
        Ok(())
    }
}
