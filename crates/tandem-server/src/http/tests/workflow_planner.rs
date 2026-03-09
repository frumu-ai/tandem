use super::*;

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

fn planner_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct PlannerEnvGuard {
    _guard: MutexGuard<'static, ()>,
    saved: Vec<(&'static str, Option<String>)>,
}

impl PlannerEnvGuard {
    fn new(vars: &[&'static str]) -> Self {
        let guard = planner_env_lock().lock().expect("planner env lock");
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

impl Drop for PlannerEnvGuard {
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

async fn configure_openai_provider(state: &AppState) {
    let mut providers = HashMap::new();
    providers.insert(
        "openai".to_string(),
        tandem_providers::ProviderConfig {
            api_key: None,
            url: Some("http://127.0.0.1:9/v1".to_string()),
            default_model: Some("gpt-5.1".to_string()),
        },
    );
    state
        .providers
        .reload(tandem_providers::AppConfig {
            providers,
            default_provider: Some("openai".to_string()),
        })
        .await;
}

fn planner_preferences() -> Value {
    json!({
        "role_models": {
            "planner": {
                "provider_id": "openai",
                "model_id": "gpt-5.1"
            }
        }
    })
}

fn manual_schedule_json() -> Value {
    json!({
        "type": "manual",
        "timezone": "UTC",
        "misfire_policy": {
            "type": "run_once"
        }
    })
}

fn cron_schedule_json(expr: &str) -> Value {
    json!({
        "type": "cron",
        "cron_expression": expr,
        "timezone": "UTC",
        "misfire_policy": {
            "type": "run_once"
        }
    })
}

fn step_json(
    step_id: &str,
    kind: &str,
    objective: &str,
    depends_on: &[&str],
    agent_role: &str,
    input_refs: Value,
    output_kind: &str,
) -> Value {
    json!({
        "step_id": step_id,
        "kind": kind,
        "objective": objective,
        "depends_on": depends_on,
        "agent_role": agent_role,
        "input_refs": input_refs,
        "output_contract": {
            "kind": output_kind
        }
    })
}

fn llm_plan_json(
    title: &str,
    description: &str,
    schedule: Value,
    workspace_root: &str,
    steps: Vec<Value>,
    operator_preferences: Option<Value>,
) -> Value {
    json!({
        "plan_id": "ignored",
        "planner_version": "ignored",
        "plan_source": "ignored",
        "original_prompt": "ignored",
        "normalized_prompt": "ignored",
        "confidence": "high",
        "title": title,
        "description": description,
        "schedule": schedule,
        "execution_target": "automation_v2",
        "workspace_root": workspace_root,
        "steps": steps,
        "requires_integrations": [],
        "allowed_mcp_servers": [],
        "operator_preferences": operator_preferences,
        "save_options": {
            "can_export_pack": true,
            "can_save_skill": true
        }
    })
}

fn preview_request(payload: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/workflow-plans/preview")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .expect("preview request")
}

fn chat_start_request(payload: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/start")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .expect("chat start request")
}

fn chat_message_request(plan_id: &str, message: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/message")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "message": message,
            })
            .to_string(),
        ))
        .expect("chat message request")
}

#[tokio::test]
async fn workflow_plan_preview_returns_minimal_fallback_without_planner_model() {
    let state = test_state().await;
    let app = app_router(state);

    let resp = app
        .oneshot(preview_request(json!({
            "prompt": "Research the market and generate a report",
            "workspace_root": "/tmp/custom-workspace"
        })))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload
            .get("plan")
            .and_then(|row| row.get("confidence"))
            .and_then(Value::as_str),
        Some("low")
    );
    assert_eq!(
        payload
            .get("plan")
            .and_then(|row| row.get("steps"))
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| row.get("step_id").and_then(Value::as_str))
                    .collect::<Vec<_>>()
            }),
        Some(vec!["execute_goal"])
    );
}

#[tokio::test]
async fn workflow_plan_preview_rejects_relative_workspace_root() {
    let state = test_state().await;
    let app = app_router(state);

    let resp = app
        .oneshot(preview_request(json!({
            "prompt": "Research the market",
            "workspace_root": "relative/path"
        })))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn workflow_plan_preview_uses_fallback_when_planner_provider_unconfigured() {
    let state = test_state().await;
    let app = app_router(state);

    let resp = app
        .oneshot(preview_request(json!({
            "prompt": "Research the market and generate a report",
            "workspace_root": "/tmp/custom-workspace",
            "operator_preferences": planner_preferences()
        })))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload
            .get("plan")
            .and_then(|row| row.get("steps"))
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| row.get("step_id").and_then(Value::as_str))
                    .collect::<Vec<_>>()
            }),
        Some(vec!["execute_goal"])
    );
    assert!(payload
        .get("clarifier")
        .and_then(|row| row.get("question"))
        .and_then(Value::as_str)
        .is_some_and(|text| text.contains("provider `openai`") && text.contains("not configured")));
    assert_eq!(
        payload
            .get("planner_diagnostics")
            .and_then(|row| row.get("fallback_reason"))
            .and_then(Value::as_str),
        Some("provider_unconfigured")
    );
}

#[tokio::test]
async fn workflow_plan_preview_accepts_valid_llm_created_plan() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state);
    let _guard = PlannerEnvGuard::new(&[
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_RESPONSE",
    ]);
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        json!({
            "action": "build",
            "assistant_text": "Built a richer workflow plan.",
            "plan": llm_plan_json(
                "Daily market report",
                "Research sources, analyze them, and generate a report.",
                cron_schedule_json("0 6 * * *"),
                "/tmp/ignored-by-normalizer",
                vec![
                    step_json("research_sources", "research", "Research the market.", &[], "researcher", json!([]), "structured_json"),
                    step_json("analyze_findings", "analyze", "Analyze the source material.", &["research_sources"], "analyst", json!([
                        {"from_step_id":"research_sources","alias":"source_findings"}
                    ]), "structured_json"),
                    step_json("generate_report", "report", "Generate the final report.", &["analyze_findings"], "writer", json!([
                        {"from_step_id":"analyze_findings","alias":"analysis"}
                    ]), "report_markdown")
                ],
                None
            )
        })
        .to_string(),
    );

    let resp = app
        .oneshot(preview_request(json!({
            "prompt": "Every morning research the market and generate a report",
            "workspace_root": "/tmp/custom-workspace",
            "operator_preferences": planner_preferences()
        })))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload
            .get("plan")
            .and_then(|row| row.get("title"))
            .and_then(Value::as_str),
        Some("Daily market report")
    );
    assert_eq!(
        payload
            .get("plan")
            .and_then(|row| row.get("workspace_root"))
            .and_then(Value::as_str),
        Some("/tmp/custom-workspace")
    );
    assert_eq!(
        payload
            .get("plan")
            .and_then(|row| row.get("steps"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(3)
    );
}

#[tokio::test]
async fn workflow_plan_preview_rejects_invalid_llm_step_id_and_uses_fallback() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state);
    let _guard = PlannerEnvGuard::new(&[
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_RESPONSE",
    ]);
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        json!({
            "action": "build",
            "plan": llm_plan_json(
                "Invalid plan",
                "Invalid plan",
                manual_schedule_json(),
                "/tmp/ignored",
                vec![step_json("custom_step", "custom", "Invalid.", &[], "worker", json!([]), "structured_json")],
                None
            )
        })
        .to_string(),
    );

    let resp = app
        .oneshot(preview_request(json!({
            "prompt": "Do something broad",
            "workspace_root": "/tmp/custom-workspace",
            "operator_preferences": planner_preferences()
        })))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload
            .get("plan")
            .and_then(|row| row.get("steps"))
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| row.get("step_id").and_then(Value::as_str))
                    .collect::<Vec<_>>()
            }),
        Some(vec!["execute_goal"])
    );
    assert_eq!(
        payload
            .get("planner_diagnostics")
            .and_then(|row| row.get("fallback_reason"))
            .and_then(Value::as_str),
        Some("validation_rejected")
    );
}

#[tokio::test]
async fn workflow_plan_preview_rejects_invalid_llm_dependency_and_uses_fallback() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state);
    let _guard = PlannerEnvGuard::new(&[
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_RESPONSE",
    ]);
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        json!({
            "action": "build",
            "plan": llm_plan_json(
                "Broken dependency plan",
                "Broken dependency plan",
                manual_schedule_json(),
                "/tmp/ignored",
                vec![
                    step_json("generate_report", "report", "Generate the final report.", &["missing_step"], "writer", json!([
                        {"from_step_id":"missing_step","alias":"analysis"}
                    ]), "report_markdown")
                ],
                None
            )
        })
        .to_string(),
    );

    let resp = app
        .oneshot(preview_request(json!({
            "prompt": "Do something broad",
            "workspace_root": "/tmp/custom-workspace",
            "operator_preferences": planner_preferences()
        })))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload
            .get("plan")
            .and_then(|row| row.get("steps"))
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| row.get("step_id").and_then(Value::as_str))
                    .collect::<Vec<_>>()
            }),
        Some(vec!["execute_goal"])
    );
}

#[tokio::test]
async fn workflow_plan_apply_persists_automation_v2_with_planner_metadata() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state.clone());
    let _guard = PlannerEnvGuard::new(&[
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_RESPONSE",
    ]);
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        json!({
            "action": "build",
            "plan": llm_plan_json(
                "Comparison Workflow",
                "Collect inputs, compare them, and produce a report.",
                manual_schedule_json(),
                "/tmp/ignored",
                vec![
                    step_json("collect_inputs", "collect", "Gather inputs.", &[], "researcher", json!([]), "structured_json"),
                    step_json("compare_results", "compare", "Compare them.", &["collect_inputs"], "analyst", json!([
                        {"from_step_id":"collect_inputs","alias":"comparison_inputs"}
                    ]), "structured_json"),
                    step_json("generate_report", "report", "Generate the report.", &["compare_results"], "writer", json!([
                        {"from_step_id":"compare_results","alias":"comparison_findings"}
                    ]), "report_markdown")
                ],
                Some(json!({
                    "execution_mode": "swarm",
                    "max_parallel_agents": 6,
                    "model_provider": "openai",
                    "model_id": "gpt-5.1",
                    "role_models": {
                        "planner": {
                            "provider_id": "openai",
                            "model_id": "gpt-5.1"
                        }
                    }
                }))
            )
        })
        .to_string(),
    );

    let preview_resp = app
        .clone()
        .oneshot(preview_request(json!({
            "prompt": "Compare two competitor summaries and generate a report",
            "plan_source": "automations_page",
            "allowed_mcp_servers": ["slack", "github", "github"],
            "workspace_root": "/tmp/custom-workspace",
            "operator_preferences": {
                "execution_mode": "swarm",
                "max_parallel_agents": 6,
                "model_provider": "openai",
                "model_id": "gpt-5.1",
                "role_models": {
                    "planner": {
                        "provider_id": "openai",
                        "model_id": "gpt-5.1"
                    }
                }
            }
        })))
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");
    let plan_id = preview_payload
        .get("plan")
        .and_then(|plan| plan.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id");

    let apply_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/apply")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "creator_id": "control-panel"
            })
            .to_string(),
        ))
        .expect("apply request");
    let apply_resp = app
        .clone()
        .oneshot(apply_req)
        .await
        .expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    let automation_id = apply_payload
        .get("automation")
        .and_then(|row| row.get("automation_id"))
        .and_then(Value::as_str)
        .expect("automation id");
    let stored = state
        .get_automation_v2(automation_id)
        .await
        .expect("stored automation");
    assert_eq!(stored.creator_id, "control-panel");
    assert_eq!(
        stored.workspace_root.as_deref(),
        Some("/tmp/custom-workspace")
    );
    assert_eq!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("plan_source"))
            .and_then(Value::as_str),
        Some("automations_page")
    );
    let operator_agent = stored
        .agents
        .iter()
        .find(|agent| agent.agent_id == "agent_writer")
        .expect("writer agent");
    assert!(operator_agent
        .tool_policy
        .allowlist
        .contains(&"mcp.github.*".to_string()));
    assert!(operator_agent
        .tool_policy
        .allowlist
        .contains(&"mcp.slack.*".to_string()));
    assert!(stored
        .flow
        .nodes
        .iter()
        .any(|node| !node.input_refs.is_empty()));
}

#[tokio::test]
async fn workflow_plan_apply_normalizes_mcp_server_prefixes_into_tool_allowlist() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state.clone());
    let _guard = PlannerEnvGuard::new(&[
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_RESPONSE",
    ]);
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        json!({
            "action": "build",
            "plan": llm_plan_json(
                "Delivery Workflow",
                "Write a report and email it.",
                manual_schedule_json(),
                "/tmp/ignored",
                vec![
                    step_json("generate_report", "report", "Generate the report.", &[], "writer", json!([]), "report_markdown"),
                    step_json("notify_user", "notify", "Send the report by email.", &["generate_report"], "operator", json!([
                        {"from_step_id":"generate_report","alias":"final_report"}
                    ]), "text_summary")
                ],
                Some(json!({
                    "model_provider": "openai",
                    "model_id": "gpt-5.1"
                }))
            )
        })
        .to_string(),
    );

    let preview_resp = app
        .clone()
        .oneshot(preview_request(json!({
            "prompt": "Generate a report and send it by email",
            "plan_source": "automations_page",
            "allowed_mcp_servers": ["composio-1"],
            "workspace_root": "/tmp/custom-workspace",
            "operator_preferences": {
                "model_provider": "openai",
                "model_id": "gpt-5.1"
            }
        })))
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");
    let plan_id = preview_payload
        .get("plan")
        .and_then(|plan| plan.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id");

    let apply_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/apply")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "creator_id": "control-panel"
            })
            .to_string(),
        ))
        .expect("apply request");
    let apply_resp = app
        .clone()
        .oneshot(apply_req)
        .await
        .expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    let automation_id = apply_payload
        .get("automation")
        .and_then(|row| row.get("automation_id"))
        .and_then(Value::as_str)
        .expect("automation id");
    let stored = state
        .get_automation_v2(automation_id)
        .await
        .expect("stored automation");
    let operator_agent = stored
        .agents
        .iter()
        .find(|agent| agent.agent_id == "agent_operator")
        .expect("operator agent");
    assert!(operator_agent
        .tool_policy
        .allowlist
        .contains(&"mcp.composio_1.*".to_string()));
}

#[tokio::test]
async fn workflow_plan_apply_succeeds_when_legacy_automations_file_is_stale() {
    let _guard = PlannerEnvGuard::new(&[
        "TANDEM_STATE_DIR",
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_RESPONSE",
    ]);
    let state_root =
        std::env::temp_dir().join(format!("tandem-workflow-plan-legacy-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&state_root).expect("state root");
    std::fs::write(
        state_root.join("automations_v2.json"),
        r#"{
  "legacy-automation": {
    "automation_id": "legacy-automation",
    "name": "Legacy Automation",
    "description": "stale legacy file",
    "enabled": true,
    "trigger": {
      "type": "manual"
    },
    "schedule": {
      "type": "manual",
      "timezone": "UTC",
      "misfire_policy": {
        "type": "run_once"
      }
    },
    "agents": [],
    "flow": {
      "nodes": [],
      "edges": []
    },
    "created_at_ms": 1,
    "updated_at_ms": 1
  }
}"#,
    )
    .expect("write stale legacy automations file");
    _guard.set("TANDEM_STATE_DIR", state_root.display().to_string());

    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state.clone());
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        json!({
            "action": "build",
            "plan": llm_plan_json(
                "Delivery Workflow",
                "Write a report and notify the operator.",
                manual_schedule_json(),
                "/tmp/ignored",
                vec![
                    step_json("generate_report", "report", "Generate the report.", &[], "writer", json!([]), "report_markdown"),
                    step_json("notify_user", "notify", "Notify the operator.", &["generate_report"], "operator", json!([
                        {"from_step_id":"generate_report","alias":"final_report"}
                    ]), "text_summary")
                ],
                Some(json!({
                    "model_provider": "openai",
                    "model_id": "gpt-5.1"
                }))
            )
        })
        .to_string(),
    );

    let preview_resp = app
        .clone()
        .oneshot(preview_request(json!({
            "prompt": "Generate a report and notify the operator",
            "plan_source": "automations_page",
            "workspace_root": "/tmp/custom-workspace",
            "operator_preferences": {
                "model_provider": "openai",
                "model_id": "gpt-5.1"
            }
        })))
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");
    let plan_id = preview_payload
        .get("plan")
        .and_then(|plan| plan.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id");

    let apply_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow-plans/apply")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "plan_id": plan_id,
                        "creator_id": "control-panel"
                    })
                    .to_string(),
                ))
                .expect("apply request"),
        )
        .await
        .expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    let automation_id = apply_payload
        .get("automation")
        .and_then(|row| row.get("automation_id"))
        .and_then(Value::as_str)
        .expect("automation id");

    let canonical_path = state_root.join("data").join("automations_v2.json");
    let canonical_raw =
        std::fs::read_to_string(&canonical_path).expect("read canonical automations file");
    let canonical_json: Value = serde_json::from_str(&canonical_raw).expect("canonical json");
    assert!(canonical_json.get(automation_id).is_some());

    let legacy_raw = std::fs::read_to_string(state_root.join("automations_v2.json"))
        .expect("read legacy automations file");
    let legacy_json: Value = serde_json::from_str(&legacy_raw).expect("legacy json");
    assert!(legacy_json.get(automation_id).is_none());
}

#[tokio::test]
async fn load_automations_v2_prefers_canonical_file_over_stale_legacy_entries() {
    let _guard = PlannerEnvGuard::new(&["TANDEM_STATE_DIR"]);
    let state_root =
        std::env::temp_dir().join(format!("tandem-automation-v2-load-{}", Uuid::new_v4()));
    let canonical_dir = state_root.join("data");
    std::fs::create_dir_all(&canonical_dir).expect("canonical dir");
    std::fs::write(
        canonical_dir.join("automations_v2.json"),
        r#"{
  "automation-v2-current": {
    "automation_id": "automation-v2-current",
    "name": "Current Automation",
    "description": "canonical definition",
    "status": "active",
    "schedule": {
      "type": "manual",
      "timezone": "UTC",
      "misfire_policy": { "type": "run_once" }
    },
    "agents": [],
    "flow": { "nodes": [] },
    "execution": { "max_parallel_agents": 1 },
    "output_targets": [],
    "created_at_ms": 20,
    "updated_at_ms": 20,
    "creator_id": "test"
  }
}"#,
    )
    .expect("write canonical automations file");
    std::fs::write(
        state_root.join("automations_v2.json"),
        r#"{
  "automation-v2-stale": {
    "automation_id": "automation-v2-stale",
    "name": "Stale Legacy Automation",
    "description": "legacy definition",
    "status": "paused",
    "schedule": {
      "type": "manual",
      "timezone": "UTC",
      "misfire_policy": { "type": "run_once" }
    },
    "agents": [],
    "flow": { "nodes": [] },
    "execution": { "max_parallel_agents": 1 },
    "output_targets": [],
    "created_at_ms": 1,
    "updated_at_ms": 1,
    "creator_id": "legacy"
  }
}"#,
    )
    .expect("write legacy automations file");
    _guard.set("TANDEM_STATE_DIR", state_root.display().to_string());

    let state = test_state().await;
    let automations = state.list_automations_v2().await;
    let ids = automations
        .iter()
        .map(|row| row.automation_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["automation-v2-current"]);
}

#[tokio::test]
async fn workflow_plan_chat_message_returns_planner_model_hint_without_model() {
    let state = test_state().await;
    let app = app_router(state);

    let start_resp = app
        .clone()
        .oneshot(chat_start_request(json!({
            "prompt": "Research the market and generate a report",
            "workspace_root": "/tmp/initial-workspace"
        })))
        .await
        .expect("start response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id");

    let message_resp = app
        .clone()
        .oneshot(chat_message_request(
            plan_id,
            "Make this weekly and add analysis.",
        ))
        .await
        .expect("message response");
    assert_eq!(message_resp.status(), StatusCode::OK);
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    assert!(message_payload
        .get("clarifier")
        .and_then(|row| row.get("question"))
        .and_then(Value::as_str)
        .is_some_and(|text| text.contains("planner model settings")));
}

#[tokio::test]
async fn workflow_plan_chat_message_uses_llm_revision_when_planner_model_is_configured() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state);
    let _guard = PlannerEnvGuard::new(&[
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_REVISION_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_RESPONSE",
    ]);
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        json!({
            "action": "build",
            "plan": llm_plan_json(
                "Initial Workflow",
                "Initial workflow",
                manual_schedule_json(),
                "/tmp/ignored",
                vec![step_json("execute_goal", "execute", "Execute the goal.", &[], "worker", json!([]), "structured_json")],
                Some(planner_preferences())
            )
        })
        .to_string(),
    );
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_REVISION_RESPONSE",
        json!({
            "action": "revise",
            "assistant_text": "Updated the workflow to research, analyze, report, and notify.",
            "change_summary": ["updated workflow plan"],
            "plan": llm_plan_json(
                "Market Workflow",
                "Research, analyze, report, and notify.",
                cron_schedule_json("0 9 * * 1"),
                "/tmp/initial-workspace",
                vec![
                    step_json("research_sources", "research", "Research the market.", &[], "researcher", json!([]), "structured_json"),
                    step_json("analyze_findings", "analyze", "Analyze the research.", &["research_sources"], "analyst", json!([
                        {"from_step_id":"research_sources","alias":"source_findings"}
                    ]), "structured_json"),
                    step_json("generate_report", "report", "Generate the report.", &["analyze_findings"], "writer", json!([
                        {"from_step_id":"analyze_findings","alias":"analysis"}
                    ]), "report_markdown"),
                    step_json("notify_user", "notify", "Email the report.", &["generate_report"], "writer", json!([
                        {"from_step_id":"generate_report","alias":"report"}
                    ]), "text_summary")
                ],
                Some(planner_preferences())
            )
        })
        .to_string(),
    );

    let start_resp = app
        .clone()
        .oneshot(chat_start_request(json!({
            "prompt": "Research the market and generate a report",
            "workspace_root": "/tmp/initial-workspace",
            "operator_preferences": planner_preferences()
        })))
        .await
        .expect("start response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id");

    let message_resp = app
        .clone()
        .oneshot(chat_message_request(
            plan_id,
            "Add a delivery step and make this weekly.",
        ))
        .await
        .expect("message response");
    assert_eq!(message_resp.status(), StatusCode::OK);
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    assert_eq!(
        message_payload
            .get("plan")
            .and_then(|row| row.get("steps"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(4)
    );
    assert_eq!(
        message_payload
            .get("plan")
            .and_then(|row| row.get("schedule"))
            .and_then(|row| row.get("cron_expression"))
            .and_then(Value::as_str),
        Some("0 9 * * 1")
    );
}

#[tokio::test]
async fn workflow_plan_chat_message_returns_clarify_from_llm() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state);
    let _guard = PlannerEnvGuard::new(&[
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_REVISION_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_RESPONSE",
    ]);
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        json!({
            "action": "build",
            "plan": llm_plan_json(
                "Initial Workflow",
                "Initial workflow",
                manual_schedule_json(),
                "/tmp/ignored",
                vec![step_json("execute_goal", "execute", "Execute the goal.", &[], "worker", json!([]), "structured_json")],
                Some(planner_preferences())
            )
        })
        .to_string(),
    );
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_REVISION_RESPONSE",
        json!({
            "action": "clarify",
            "assistant_text": "Do you want email delivery or a saved report only?",
            "clarifier": {
                "field": "general",
                "question": "Do you want email delivery or a saved report only?"
            }
        })
        .to_string(),
    );

    let start_resp = app
        .clone()
        .oneshot(chat_start_request(json!({
            "prompt": "Research the market and generate a report",
            "workspace_root": "/tmp/initial-workspace",
            "operator_preferences": planner_preferences()
        })))
        .await
        .expect("start response");
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id");

    let message_resp = app
        .clone()
        .oneshot(chat_message_request(
            plan_id,
            "Make sure it gets delivered.",
        ))
        .await
        .expect("message response");
    assert_eq!(message_resp.status(), StatusCode::OK);
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    assert_eq!(
        message_payload
            .get("clarifier")
            .and_then(|row| row.get("question"))
            .and_then(Value::as_str),
        Some("Do you want email delivery or a saved report only?")
    );
    assert_eq!(
        message_payload
            .get("change_summary")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
}

#[tokio::test]
async fn workflow_plan_chat_message_returns_keep_from_llm() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state);
    let _guard = PlannerEnvGuard::new(&[
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_REVISION_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_RESPONSE",
    ]);
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        json!({
            "action": "build",
            "plan": llm_plan_json(
                "Initial Workflow",
                "Initial workflow",
                manual_schedule_json(),
                "/tmp/ignored",
                vec![step_json("execute_goal", "execute", "Execute the goal.", &[], "worker", json!([]), "structured_json")],
                Some(planner_preferences())
            )
        })
        .to_string(),
    );
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_REVISION_RESPONSE",
        json!({
            "action": "keep",
            "assistant_text": "The current workflow already matches the request."
        })
        .to_string(),
    );

    let start_resp = app
        .clone()
        .oneshot(chat_start_request(json!({
            "prompt": "Research the market and generate a report",
            "workspace_root": "/tmp/initial-workspace",
            "operator_preferences": planner_preferences()
        })))
        .await
        .expect("start response");
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id");

    let message_resp = app
        .clone()
        .oneshot(chat_message_request(plan_id, "Keep it as-is."))
        .await
        .expect("message response");
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    assert_eq!(
        message_payload
            .get("assistant_message")
            .and_then(|row| row.get("text"))
            .and_then(Value::as_str),
        Some("The current workflow already matches the request.")
    );
    assert_eq!(
        message_payload
            .get("change_summary")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
}

#[tokio::test]
async fn workflow_plan_chat_message_falls_back_when_llm_revision_is_invalid() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state);
    let _guard = PlannerEnvGuard::new(&[
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_REVISION_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_RESPONSE",
    ]);
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        json!({
            "action": "build",
            "plan": llm_plan_json(
                "Initial Workflow",
                "Initial workflow",
                manual_schedule_json(),
                "/tmp/ignored",
                vec![step_json("execute_goal", "execute", "Execute the goal.", &[], "worker", json!([]), "structured_json")],
                Some(planner_preferences())
            )
        })
        .to_string(),
    );
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_REVISION_RESPONSE",
        r#"{"action":"revise","plan":{"steps":[{"step_id":"custom_step"}]}}"#,
    );

    let start_resp = app
        .clone()
        .oneshot(chat_start_request(json!({
            "prompt": "Research the market and generate a report",
            "workspace_root": "/tmp/initial-workspace",
            "operator_preferences": planner_preferences()
        })))
        .await
        .expect("start response");
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id");

    let message_resp = app
        .clone()
        .oneshot(chat_message_request(plan_id, "Rewrite the workflow."))
        .await
        .expect("message response");
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    assert!(message_payload
        .get("clarifier")
        .and_then(|row| row.get("question"))
        .and_then(Value::as_str)
        .is_some_and(|text| text.contains("could not produce a valid workflow revision")));
}

#[tokio::test]
async fn workflow_plan_chat_reset_restores_initial_plan() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let start_resp = app
        .clone()
        .oneshot(chat_start_request(json!({
            "prompt": "Research the market and generate a report",
            "workspace_root": "/tmp/initial-workspace"
        })))
        .await
        .expect("start response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id");

    let reset_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/reset")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id
            })
            .to_string(),
        ))
        .expect("reset request");
    let reset_resp = app
        .clone()
        .oneshot(reset_req)
        .await
        .expect("reset response");
    assert_eq!(reset_resp.status(), StatusCode::OK);
    let draft = state.get_workflow_plan_draft(plan_id).await.expect("draft");
    assert_eq!(
        serde_json::to_value(&draft.initial_plan.steps).expect("initial steps"),
        serde_json::to_value(&draft.current_plan.steps).expect("current steps")
    );
}

#[tokio::test]
async fn workflow_plan_planner_model_spec_prefers_planner_role_model() {
    let spec = crate::http::workflow_planner::planner_model_spec(Some(&json!({
        "model_provider": "openai",
        "model_id": "gpt-5.1",
        "role_models": {
            "planner": {
                "provider_id": "anthropic",
                "model_id": "claude-sonnet-4"
            }
        }
    })))
    .expect("planner spec");
    assert_eq!(spec.provider_id, "anthropic");
    assert_eq!(spec.model_id, "claude-sonnet-4");
}
