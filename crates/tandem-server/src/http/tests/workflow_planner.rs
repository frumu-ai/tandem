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
    step_json_with_metadata(
        step_id,
        kind,
        objective,
        depends_on,
        agent_role,
        input_refs,
        output_kind,
        None,
    )
}

fn step_json_with_metadata(
    step_id: &str,
    kind: &str,
    objective: &str,
    depends_on: &[&str],
    agent_role: &str,
    input_refs: Value,
    output_kind: &str,
    metadata: Option<Value>,
) -> Value {
    let validator = match output_kind {
        "brief" => "research_brief",
        "review" | "review_summary" | "approval_gate" => "review_decision",
        "structured_json" => "structured_json",
        _ => "generic_artifact",
    };
    let mut value = json!({
        "step_id": step_id,
        "kind": kind,
        "objective": objective,
        "depends_on": depends_on,
        "agent_role": agent_role,
        "input_refs": input_refs,
        "output_contract": {
            "kind": output_kind,
            "validator": validator
        }
    });
    if let Some(metadata) = metadata {
        value
            .as_object_mut()
            .expect("step object")
            .insert("metadata".to_string(), metadata);
    }
    value
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

async fn seed_prior_overlap_automation(state: &AppState, plan_payload: Value) {
    let mut prior_plan_package = tandem_plan_compiler::api::compile_workflow_plan_preview_package(
        &serde_json::from_value::<tandem_plan_compiler::api::WorkflowPlanJson>(
            plan_payload.clone(),
        )
        .expect("workflow plan json"),
        Some("workflow_planner"),
    );
    prior_plan_package.plan_id = "prior-overlap-plan".to_string();
    if let Some(exact_identity) = prior_plan_package
        .overlap_policy
        .as_mut()
        .and_then(|policy| policy.exact_identity.as_mut())
    {
        exact_identity.canonical_hash = Some("prior-overlap-custom-hash".to_string());
    }
    let prior_plan =
        serde_json::from_value::<crate::WorkflowPlan>(plan_payload).expect("workflow plan");
    let mut prior_automation = crate::http::workflow_planner_runtime::compile_plan_to_automation_v2(
        &prior_plan,
        Some(&prior_plan_package),
        "test",
    );
    prior_automation.automation_id = "prior-overlap-automation".to_string();
    if let Some(metadata) = prior_automation
        .metadata
        .as_mut()
        .and_then(Value::as_object_mut)
    {
        metadata.insert(
            "plan_package".to_string(),
            serde_json::to_value(&prior_plan_package).expect("plan package value"),
        );
    } else {
        prior_automation.metadata = Some(json!({
            "plan_package": prior_plan_package
        }));
    }
    state
        .put_automation_v2(prior_automation)
        .await
        .expect("store prior overlap automation");
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
    let teaching_library = payload
        .get("teaching_library")
        .and_then(Value::as_object)
        .expect("teaching library");
    assert!(teaching_library
        .get("explanations")
        .and_then(Value::as_array)
        .is_some());
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
    let app = app_router(state.clone());
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
    assert!(payload.get("plan_package_bundle").is_some());
    let steps = payload
        .get("plan")
        .and_then(|row| row.get("steps"))
        .and_then(Value::as_array)
        .expect("steps");
    assert_eq!(
        steps[0]
            .get("output_contract")
            .and_then(|row| row.get("validator"))
            .and_then(Value::as_str),
        Some("structured_json")
    );
    assert_eq!(
        steps[2]
            .get("output_contract")
            .and_then(|row| row.get("validator"))
            .and_then(Value::as_str),
        Some("generic_artifact")
    );
}

#[tokio::test]
async fn workflow_plan_preview_accepts_partial_llm_plan_payload() {
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
            "assistant_text": "Built a workflow plan from partial payload.",
            "plan": {
                "title": "Partial Planner Response",
                "steps": [
                    step_json("collect_inputs", "collect", "Gather project inputs.", &[], "researcher", json!([]), "structured_json"),
                    step_json("generate_report", "report", "Generate report.", &["collect_inputs"], "writer", json!([
                        {"from_step_id":"collect_inputs","alias":"inputs"}
                    ]), "report_markdown")
                ]
            }
        })
        .to_string(),
    );

    let resp = app
        .oneshot(preview_request(json!({
            "prompt": "Collect inputs and generate a report",
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
        Some("Partial Planner Response")
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
        Some(2)
    );
    assert!(
        payload
            .get("planner_diagnostics")
            .is_none_or(Value::is_null),
        "planner diagnostics should not indicate fallback for a repaired partial plan"
    );
}

#[tokio::test]
async fn workflow_plan_preview_accepts_research_and_review_validators_from_llm() {
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
            "assistant_text": "Built a research and review workflow plan.",
            "plan": llm_plan_json(
                "Research And Review Workflow",
                "Research a topic and route it through review.",
                manual_schedule_json(),
                "/tmp/ignored-by-normalizer",
                vec![
                    step_json("research_sources", "research", "Draft the research brief.", &[], "researcher", json!([]), "brief"),
                    step_json("generate_report", "review", "Review the research brief.", &["research_sources"], "reviewer", json!([
                        {"from_step_id":"research_sources","alias":"brief"}
                    ]), "review")
                ],
                None
            )
        })
        .to_string(),
    );

    let resp = app
        .oneshot(preview_request(json!({
            "prompt": "Research the topic and require review before approval",
            "workspace_root": "/tmp/custom-workspace",
            "operator_preferences": planner_preferences()
        })))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let steps = payload
        .get("plan")
        .and_then(|row| row.get("steps"))
        .and_then(Value::as_array)
        .expect("steps");
    assert_eq!(
        steps[0]
            .get("output_contract")
            .and_then(|row| row.get("validator"))
            .and_then(Value::as_str),
        Some("research_brief")
    );
    assert_eq!(
        steps[1]
            .get("output_contract")
            .and_then(|row| row.get("validator"))
            .and_then(Value::as_str),
        Some("review_decision")
    );
}

#[tokio::test]
async fn workflow_plan_preview_accepts_allowed_step_id_suffix_variant() {
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
            "assistant_text": "Built a plan with web research suffix step ids.",
            "plan": llm_plan_json(
                "Web research variant",
                "Use a suffixed research step id.",
                manual_schedule_json(),
                "/tmp/ignored-by-normalizer",
                vec![
                    step_json("research_sources_web", "research", "Gather current web sources.", &[], "researcher", json!([]), "structured_json"),
                    step_json("analyze_findings", "analysis", "Analyze the web sources.", &["research_sources_web"], "analyst", json!([
                        {"from_step_id":"research_sources_web","alias":"web_sources"}
                    ]), "structured_json")
                ],
                None
            )
        })
        .to_string(),
    );

    let resp = app
        .oneshot(preview_request(json!({
            "prompt": "Research online sources and analyze findings",
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
            .map(|rows| rows.len()),
        Some(2)
    );
    assert!(
        payload
            .get("planner_diagnostics")
            .is_none_or(Value::is_null),
        "suffix-variant step ids should not force fallback"
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
    let apply_status = apply_resp.status();
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    assert_eq!(apply_status, StatusCode::OK);
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
    assert!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("plan_package_bundle"))
            .is_some(),
        "plan package bundle should be stored on the automation snapshot"
    );
    assert!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("plan_package"))
            .is_some(),
        "plan package should be stored on the automation snapshot"
    );
    assert_eq!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("plan_package"))
            .and_then(|row| row.get("plan_revision"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("plan_package_validation"))
            .is_some(),
        "plan package validation should be stored on the automation snapshot"
    );
    assert!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("approved_plan_materialization"))
            .is_some(),
        "approved plan materialization should be stored on the automation snapshot"
    );
    assert!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("planner_diagnostics"))
            .is_some(),
        "planner diagnostics should be present on the automation snapshot"
    );
    assert!(apply_payload.get("plan_package_bundle").is_some());
    assert!(apply_payload.get("approved_plan_materialization").is_some());
    let stored_draft = state.get_workflow_plan_draft(plan_id).await.expect("draft");
    assert!(stored_draft.last_success_materialization.is_some());
    assert_eq!(
        stored_draft
            .last_success_materialization
            .as_ref()
            .and_then(|value| value.get("plan_id"))
            .and_then(Value::as_str),
        Some(plan_id)
    );
    assert_eq!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("approved_plan_materialization"))
            .and_then(|row| row.get("plan_id"))
            .and_then(Value::as_str),
        Some(plan_id)
    );
    let dry_run_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/v2/{automation_id}/run_now"))
        .header("content-type", "application/json")
        .body(Body::from(json!({"dry_run": true}).to_string()))
        .expect("dry run request");
    let dry_run_resp = app
        .clone()
        .oneshot(dry_run_req)
        .await
        .expect("dry run response");
    assert_eq!(dry_run_resp.status(), StatusCode::OK);
    let dry_run_body = to_bytes(dry_run_resp.into_body(), usize::MAX)
        .await
        .expect("dry run body");
    let dry_run_payload: Value = serde_json::from_slice(&dry_run_body).expect("dry run json");
    let dry_run_run_id = dry_run_payload
        .get("run")
        .and_then(|row| row.get("run_id"))
        .and_then(Value::as_str)
        .expect("dry run id");
    assert_eq!(
        dry_run_payload
            .get("run")
            .and_then(|row| row.get("trigger_type"))
            .and_then(Value::as_str),
        Some("manual_dry_run")
    );
    let stored_after_run_now = state
        .get_automation_v2(automation_id)
        .await
        .expect("stored automation after manual run");
    let expected_trigger_id = format!("manual-trigger-{dry_run_run_id}");
    let manual_trigger_record = stored_after_run_now
        .metadata
        .as_ref()
        .and_then(|row| row.get("plan_package"))
        .and_then(|row| row.get("manual_trigger_record"))
        .expect("manual trigger record");
    assert_eq!(
        manual_trigger_record
            .get("trigger_id")
            .and_then(Value::as_str),
        Some(expected_trigger_id.as_str())
    );
    assert_eq!(
        manual_trigger_record
            .get("triggered_by")
            .and_then(Value::as_str),
        Some("control-panel")
    );
    assert_eq!(
        manual_trigger_record
            .get("trigger_source")
            .and_then(Value::as_str),
        Some("dry_run")
    );
    assert_eq!(
        manual_trigger_record
            .get("dry_run")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        dry_run_payload
            .get("run")
            .and_then(|row| row.get("automation_snapshot"))
            .and_then(|row| row.get("metadata"))
            .and_then(|row| row.get("plan_package"))
            .and_then(|row| row.get("manual_trigger_record"))
            .and_then(|row| row.get("run_id"))
            .and_then(Value::as_str),
        Some(dry_run_run_id)
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
async fn workflow_plan_preview_returns_overlap_analysis_from_prior_automation() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state.clone());
    let _guard = PlannerEnvGuard::new(&[
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_RESPONSE",
    ]);
    let mut plan_payload = llm_plan_json(
        "Comparison Workflow",
        "Collect inputs, compare them, and produce a report.",
        manual_schedule_json(),
        "/tmp/ignored",
        vec![
            step_json(
                "collect_inputs",
                "collect",
                "Gather inputs.",
                &[],
                "researcher",
                json!([]),
                "structured_json",
            ),
            step_json(
                "compare_results",
                "compare",
                "Compare them.",
                &["collect_inputs"],
                "analyst",
                json!([{ "from_step_id": "collect_inputs", "alias": "comparison_inputs" }]),
                "structured_json",
            ),
            step_json(
                "generate_report",
                "report",
                "Generate the report.",
                &["compare_results"],
                "writer",
                json!([{ "from_step_id": "compare_results", "alias": "comparison_findings" }]),
                "report_markdown",
            ),
        ],
        Some(planner_preferences()),
    );
    plan_payload["original_prompt"] =
        json!("Compare two competitor summaries and generate a report");
    plan_payload["normalized_prompt"] =
        json!("compare two competitor summaries and generate a report");
    seed_prior_overlap_automation(&state, plan_payload.clone()).await;
    let stored_prior = state
        .get_automation_v2("prior-overlap-automation")
        .await
        .expect("stored prior automation");
    assert!(stored_prior
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("plan_package"))
        .is_some());
    let prior_packages = state
        .list_automations_v2()
        .await
        .into_iter()
        .filter_map(|automation| {
            automation
                .metadata
                .and_then(|metadata| metadata.get("plan_package").cloned())
        })
        .map(|value| {
            serde_json::from_value::<tandem_plan_compiler::api::PlanPackage>(value)
                .expect("plan package")
        })
        .collect::<Vec<_>>();
    let candidate_package = tandem_plan_compiler::api::compile_workflow_plan_preview_package(
        &serde_json::from_value::<tandem_plan_compiler::api::WorkflowPlanJson>(
            plan_payload.clone(),
        )
        .expect("candidate workflow plan json"),
        Some("workflow_planner"),
    );
    let direct_overlap =
        tandem_plan_compiler::api::analyze_plan_overlap(&candidate_package, &prior_packages);
    assert_eq!(
        direct_overlap.matched_plan_id.as_deref(),
        Some("prior-overlap-plan")
    );
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        json!({
            "action": "build",
            "plan": plan_payload
        })
        .to_string(),
    );

    let resp = app
        .oneshot(preview_request(json!({
            "prompt": "Compare two competitor summaries and generate a report",
            "workspace_root": "/tmp/custom-workspace",
            "operator_preferences": planner_preferences()
        })))
        .await
        .expect("preview response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let payload: Value = serde_json::from_slice(&body).expect("preview json");
    assert_eq!(
        payload
            .get("overlap_analysis")
            .and_then(|row| row.get("matched_plan_id"))
            .and_then(Value::as_str),
        Some("prior-overlap-plan")
    );
    assert_eq!(
        payload
            .get("overlap_analysis")
            .and_then(|row| row.get("requires_user_confirmation"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        payload
            .get("overlap_analysis")
            .and_then(|row| row.get("match_layer"))
            .and_then(Value::as_str),
        Some("semantic")
    );
}

#[tokio::test]
async fn workflow_plan_apply_requires_overlap_confirmation_and_persists_decision_log() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state.clone());
    let _guard = PlannerEnvGuard::new(&[
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_RESPONSE",
    ]);
    let mut plan_payload = llm_plan_json(
        "Comparison Workflow",
        "Collect inputs, compare them, and produce a report.",
        manual_schedule_json(),
        "/tmp/ignored",
        vec![
            step_json(
                "collect_inputs",
                "collect",
                "Gather inputs.",
                &[],
                "researcher",
                json!([]),
                "structured_json",
            ),
            step_json(
                "compare_results",
                "compare",
                "Compare them.",
                &["collect_inputs"],
                "analyst",
                json!([{ "from_step_id": "collect_inputs", "alias": "comparison_inputs" }]),
                "structured_json",
            ),
            step_json(
                "generate_report",
                "report",
                "Generate the report.",
                &["compare_results"],
                "writer",
                json!([{ "from_step_id": "compare_results", "alias": "comparison_findings" }]),
                "report_markdown",
            ),
        ],
        Some(planner_preferences()),
    );
    plan_payload["original_prompt"] =
        json!("Compare two competitor summaries and generate a report");
    plan_payload["normalized_prompt"] =
        json!("compare two competitor summaries and generate a report");
    seed_prior_overlap_automation(&state, plan_payload.clone()).await;
    let stored_prior = state
        .get_automation_v2("prior-overlap-automation")
        .await
        .expect("stored prior automation");
    assert!(stored_prior
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("plan_package"))
        .is_some());
    let prior_packages = state
        .list_automations_v2()
        .await
        .into_iter()
        .filter_map(|automation| {
            automation
                .metadata
                .and_then(|metadata| metadata.get("plan_package").cloned())
        })
        .map(|value| {
            serde_json::from_value::<tandem_plan_compiler::api::PlanPackage>(value)
                .expect("plan package")
        })
        .collect::<Vec<_>>();
    let candidate_package = tandem_plan_compiler::api::compile_workflow_plan_preview_package(
        &serde_json::from_value::<tandem_plan_compiler::api::WorkflowPlanJson>(
            plan_payload.clone(),
        )
        .expect("candidate workflow plan json"),
        Some("workflow_planner"),
    );
    let direct_overlap =
        tandem_plan_compiler::api::analyze_plan_overlap(&candidate_package, &prior_packages);
    assert_eq!(
        direct_overlap.matched_plan_id.as_deref(),
        Some("prior-overlap-plan")
    );
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        json!({
            "action": "build",
            "plan": plan_payload
        })
        .to_string(),
    );

    let preview_resp = app
        .clone()
        .oneshot(preview_request(json!({
            "prompt": "Compare two competitor summaries and generate a report",
            "workspace_root": "/tmp/custom-workspace",
            "operator_preferences": planner_preferences()
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

    let apply_without_confirmation = app
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
    assert_eq!(apply_without_confirmation.status(), StatusCode::CONFLICT);
    let apply_without_confirmation_body =
        to_bytes(apply_without_confirmation.into_body(), usize::MAX)
            .await
            .expect("apply conflict body");
    let apply_without_confirmation_payload: Value =
        serde_json::from_slice(&apply_without_confirmation_body).expect("apply conflict json");
    assert_eq!(
        apply_without_confirmation_payload
            .get("code")
            .and_then(Value::as_str),
        Some("WORKFLOW_PLAN_OVERLAP_CONFIRMATION_REQUIRED")
    );

    let apply_with_confirmation = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow-plans/apply")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "plan_id": plan_id,
                        "creator_id": "control-panel",
                        "overlap_decision": "fork"
                    })
                    .to_string(),
                ))
                .expect("apply request"),
        )
        .await
        .expect("apply response");
    assert_eq!(apply_with_confirmation.status(), StatusCode::OK);
    let apply_with_confirmation_body = to_bytes(apply_with_confirmation.into_body(), usize::MAX)
        .await
        .expect("apply success body");
    let apply_with_confirmation_payload: Value =
        serde_json::from_slice(&apply_with_confirmation_body).expect("apply success json");
    let automation_id = apply_with_confirmation_payload
        .get("automation")
        .and_then(|row| row.get("automation_id"))
        .and_then(Value::as_str)
        .expect("automation id");
    assert_eq!(
        apply_with_confirmation_payload
            .get("overlap_analysis")
            .and_then(|row| row.get("decision"))
            .and_then(Value::as_str),
        Some("fork")
    );
    let stored = state
        .get_automation_v2(automation_id)
        .await
        .expect("stored automation");
    assert_eq!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("plan_package"))
            .and_then(|row| row.get("overlap_policy"))
            .and_then(|row| row.get("overlap_log"))
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("decision"))
            .and_then(Value::as_str),
        Some("fork")
    );
}

#[tokio::test]
async fn workflow_plan_apply_rejects_plan_package_blockers() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state);

    let mut apply_payload: Value = llm_plan_json(
        "Blocked Workflow",
        "Needs github before it can be applied.",
        manual_schedule_json(),
        "/tmp/custom-workspace",
        vec![step_json(
            "generate_report",
            "report",
            "Generate the report.",
            &[],
            "writer",
            json!([]),
            "report_markdown",
        )],
        Some(planner_preferences()),
    );
    apply_payload["requires_integrations"] = json!(["github"]);

    let apply_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/apply")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "creator_id": "control-panel",
                "plan": apply_payload
            })
            .to_string(),
        ))
        .expect("apply request");

    let apply_resp = app
        .clone()
        .oneshot(apply_req)
        .await
        .expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::BAD_REQUEST);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    assert_eq!(
        apply_payload.get("code").and_then(Value::as_str),
        Some("WORKFLOW_PLAN_INVALID")
    );
    assert_eq!(
        apply_payload
            .get("plan_package_validation")
            .and_then(|value| value.get("blocker_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        apply_payload
            .get("plan_package_validation")
            .and_then(|value| value.get("issues"))
            .and_then(Value::as_array)
            .and_then(|issues| issues.first())
            .and_then(|issue| issue.get("code"))
            .and_then(Value::as_str),
        Some("required_connector_unresolved")
    );
}

#[tokio::test]
async fn workflow_plan_apply_preserves_research_web_expectation_metadata() {
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
                "Research Workflow",
                "Research the latest topic and write a brief.",
                manual_schedule_json(),
                "/tmp/ignored",
                vec![
                    step_json_with_metadata(
                        "research_sources",
                        "research",
                        "Research the latest topic and draft a citation-backed brief.",
                        &[],
                        "researcher",
                        json!([]),
                        "brief",
                        Some(json!({
                            "builder": {
                                "web_research_expected": true
                            }
                        }))
                    )
                ],
                None
            )
        })
        .to_string(),
    );

    let preview_resp = app
        .clone()
        .oneshot(preview_request(json!({
            "prompt": "Research the latest topic and write a citation-backed brief",
            "workspace_root": "/tmp/custom-workspace",
            "operator_preferences": planner_preferences()
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
    let stored = state
        .get_automation_v2(automation_id)
        .await
        .expect("stored automation");
    let research_node = stored
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == "research_sources")
        .expect("research node");
    assert_eq!(
        research_node
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("builder"))
            .and_then(|builder| builder.get("web_research_expected"))
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[tokio::test]
async fn workflow_plan_import_accepts_exported_bundle() {
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
                "Importable Workflow",
                "Create a shareable bundle.",
                manual_schedule_json(),
                "/tmp/importable-workspace",
                vec![step_json(
                    "draft_bundle",
                    "draft",
                    "Draft the bundle.",
                    &[],
                    "writer",
                    json!([]),
                    "report_markdown",
                )],
                Some(planner_preferences())
            )
        })
        .to_string(),
    );

    let preview_resp = app
        .clone()
        .oneshot(preview_request(json!({
            "prompt": "Create a shareable workflow bundle",
            "workspace_root": "/tmp/importable-workspace",
            "operator_preferences": planner_preferences()
        })))
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");
    let bundle = preview_payload
        .get("plan_package_bundle")
        .cloned()
        .expect("plan package bundle");

    let import_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow-plans/import")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "bundle": bundle }).to_string()))
                .expect("import request"),
        )
        .await
        .expect("import response");
    assert_eq!(import_resp.status(), StatusCode::OK);
    let import_body = to_bytes(import_resp.into_body(), usize::MAX)
        .await
        .expect("import body");
    let import_payload: Value = serde_json::from_slice(&import_body).expect("import json");
    assert_eq!(
        import_payload
            .get("import_validation")
            .and_then(|value| value.get("compatible"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert!(import_payload.get("bundle").is_some());
    assert_eq!(
        import_payload
            .get("plan_package_preview")
            .and_then(|value| value.get("lifecycle_state"))
            .and_then(Value::as_str),
        Some("preview")
    );
    assert!(import_payload
        .get("plan_package_preview")
        .and_then(|value| value.get("plan_id"))
        .and_then(Value::as_str)
        .map(|plan_id| plan_id.starts_with("imported-"))
        .unwrap_or(false));
    assert!(
        import_payload
            .get("plan_package_preview")
            .and_then(|value| value.get("metadata"))
            .and_then(|value| value.get("import"))
            .and_then(|value| value.get("mode"))
            .and_then(Value::as_str)
            == Some("sanitized_local_preview")
    );
    assert!(import_payload
        .get("import_transform_log")
        .and_then(Value::as_array)
        .map(|entries| !entries.is_empty())
        .unwrap_or(false));
    assert_eq!(
        import_payload.get("persisted").and_then(Value::as_bool),
        Some(true)
    );
    let session = import_payload
        .get("session")
        .and_then(Value::as_object)
        .cloned();
    assert!(session.is_some());
    let session = session.expect("session");
    assert_eq!(
        session.get("source_kind").and_then(Value::as_str),
        Some("imported_bundle")
    );
    assert_eq!(
        session.get("source_bundle_digest").and_then(Value::as_str),
        import_payload
            .get("import_source_bundle_digest")
            .and_then(Value::as_str)
    );
    let session_id = session
        .get("session_id")
        .and_then(Value::as_str)
        .expect("session id")
        .to_string();
    let stored_session = state
        .get_workflow_planner_session(&session_id)
        .await
        .expect("stored session");
    assert_eq!(stored_session.source_kind, "imported_bundle");
    assert!(stored_session.draft.is_some());
}

#[tokio::test]
async fn workflow_plan_import_preview_returns_scope_snapshot_and_summary() {
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
                "Preview Import Workflow",
                "Create a preview import bundle.",
                manual_schedule_json(),
                "/tmp/importable-workspace",
                vec![step_json(
                    "draft_bundle",
                    "draft",
                    "Draft the bundle.",
                    &[],
                    "writer",
                    json!([]),
                    "report_markdown",
                )],
                Some(planner_preferences())
            )
        })
        .to_string(),
    );

    let preview_resp = app
        .clone()
        .oneshot(preview_request(json!({
            "prompt": "Create a shareable workflow bundle",
            "workspace_root": "/tmp/importable-workspace",
            "operator_preferences": planner_preferences()
        })))
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");
    let bundle = preview_payload
        .get("plan_package_bundle")
        .cloned()
        .expect("plan package bundle");

    let import_preview_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow-plans/import/preview")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "bundle": bundle }).to_string()))
                .expect("import preview request"),
        )
        .await
        .expect("import preview response");
    assert_eq!(import_preview_resp.status(), StatusCode::OK);
    let import_preview_body = to_bytes(import_preview_resp.into_body(), usize::MAX)
        .await
        .expect("import preview body");
    let import_preview_payload: Value =
        serde_json::from_slice(&import_preview_body).expect("import preview json");
    assert_eq!(
        import_preview_payload
            .get("persisted")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert!(import_preview_payload
        .get("derived_scope_snapshot")
        .and_then(|value| value.get("plan_id"))
        .and_then(Value::as_str)
        .map(|plan_id| plan_id.starts_with("imported-"))
        .unwrap_or(false));
    assert!(import_preview_payload.get("session").is_none());
    assert!(import_preview_payload
        .get("plan_package_preview")
        .and_then(|value| value.get("validation_state"))
        .is_none());
    assert_eq!(
        import_preview_payload
            .get("summary")
            .and_then(|value| value.get("routine_count"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert!(import_preview_payload
        .get("summary")
        .and_then(|value| value.get("credential_envelope_count"))
        .and_then(Value::as_u64)
        .is_some());
}

#[tokio::test]
async fn workflow_plan_import_rejects_missing_scope_snapshot() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state.clone());

    let import_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow-plans/import")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "bundle": {
                            "bundle_version": "1",
                            "plan": {
                                "plan_id": "plan_missing_snapshot",
                                "plan_revision": 1,
                                "lifecycle_state": "preview",
                                "owner": {
                                    "owner_id": "control-panel",
                                    "scope": "workspace",
                                    "audience": "internal"
                                },
                                "mission": {
                                    "goal": "Import without a snapshot",
                                    "summary": null,
                                    "domain": "workflow"
                                },
                                "success_criteria": {
                                    "required_artifacts": [],
                                    "minimum_viable_completion": null,
                                    "minimum_output": null,
                                    "freshness_window_hours": null
                                },
                                "routine_graph": [],
                                "connector_intents": [],
                                "connector_bindings": [],
                                "credential_envelopes": [],
                                "context_objects": [],
                                "metadata": null
                            },
                            "scope_snapshot": null
                        }
                    })
                    .to_string(),
                ))
                .expect("import request"),
        )
        .await
        .expect("import response");
    assert_eq!(import_resp.status(), StatusCode::BAD_REQUEST);
    let import_body = to_bytes(import_resp.into_body(), usize::MAX)
        .await
        .expect("import body");
    let import_payload: Value = serde_json::from_slice(&import_body).expect("import json");
    assert_eq!(
        import_payload
            .get("import_validation")
            .and_then(|value| value.get("compatible"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert!(import_payload
        .get("import_validation")
        .and_then(|value| value.get("issues"))
        .and_then(Value::as_array)
        .map(|issues| {
            issues.iter().any(|issue| {
                issue.get("code").and_then(Value::as_str) == Some("missing_scope_snapshot")
            })
        })
        .unwrap_or(false));
}

#[tokio::test]
async fn workflow_plan_import_rejects_runnable_lifecycle_state() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state.clone());

    let import_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow-plans/import")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "bundle": {
                            "bundle_version": "1",
                            "plan": {
                                "plan_id": "plan_runnable_state",
                                "plan_revision": 1,
                                "lifecycle_state": "applied",
                                "owner": {
                                    "owner_id": "control-panel",
                                    "scope": "workspace",
                                    "audience": "internal"
                                },
                                "mission": {
                                    "goal": "Import with runnable lifecycle",
                                    "summary": null,
                                    "domain": "workflow"
                                },
                                "success_criteria": {
                                    "required_artifacts": [],
                                    "minimum_viable_completion": null,
                                    "minimum_output": null,
                                    "freshness_window_hours": null
                                },
                                "routine_graph": [],
                                "connector_intents": [],
                                "connector_bindings": [],
                                "credential_envelopes": [],
                                "context_objects": [],
                                "metadata": null
                            },
                            "scope_snapshot": {
                                "plan_id": "plan_runnable_state",
                                "plan_revision": 1,
                                "output_roots": null,
                                "inter_routine_policy": null,
                                "credential_envelopes": [],
                                "context_objects": [],
                                "routine_scopes": []
                            }
                        }
                    })
                    .to_string(),
                ))
                .expect("import request"),
        )
        .await
        .expect("import response");
    assert_eq!(import_resp.status(), StatusCode::BAD_REQUEST);
    let import_body = to_bytes(import_resp.into_body(), usize::MAX)
        .await
        .expect("import body");
    let import_payload: Value = serde_json::from_slice(&import_body).expect("import json");
    assert_eq!(
        import_payload
            .get("import_validation")
            .and_then(|value| value.get("compatible"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert!(import_payload
        .get("import_validation")
        .and_then(|value| value.get("issues"))
        .and_then(Value::as_array)
        .map(|issues| {
            issues.iter().any(|issue| {
                issue.get("code").and_then(Value::as_str)
                    == Some("import_requires_preview_lifecycle")
            })
        })
        .unwrap_or(false));
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

    assert!(
        !state_root.join("automations_v2.json").exists(),
        "stale legacy automations file should be removed once canonical persistence succeeds"
    );
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
    assert!(
        !state_root.join("automations_v2.json").exists(),
        "stale legacy automations file should be removed when canonical definitions load"
    );
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
    assert!(message_payload.get("plan_package_bundle").is_some());
    assert!(message_payload.get("plan_package_replay").is_some());
    assert_eq!(
        message_payload
            .get("plan_package")
            .and_then(|row| row.get("plan_revision"))
            .and_then(Value::as_u64),
        Some(2)
    );
    let steps = message_payload
        .get("plan")
        .and_then(|row| row.get("steps"))
        .and_then(Value::as_array)
        .expect("steps");
    assert_eq!(
        steps[0]
            .get("output_contract")
            .and_then(|row| row.get("validator"))
            .and_then(Value::as_str),
        Some("structured_json")
    );
    assert_eq!(
        steps[2]
            .get("output_contract")
            .and_then(|row| row.get("validator"))
            .and_then(Value::as_str),
        Some("generic_artifact")
    );
    assert_eq!(
        steps[3]
            .get("output_contract")
            .and_then(|row| row.get("validator"))
            .and_then(Value::as_str),
        Some("generic_artifact")
    );

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
    assert_eq!(
        apply_payload
            .get("plan_package")
            .and_then(|row| row.get("plan_revision"))
            .and_then(Value::as_u64),
        Some(2)
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
async fn workflow_planner_sessions_support_crud_and_duplication() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let preview_resp = app
        .clone()
        .oneshot(preview_request(json!({
            "prompt": "Write a small workflow plan for testing session CRUD",
            "workspace_root": "/tmp/custom-workspace"
        })))
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");

    let create_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow-plans/sessions")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "project_slug": "planner-crud",
                        "title": "Planner CRUD",
                        "workspace_root": "/tmp/custom-workspace",
                        "goal": "Write a small workflow plan for testing session CRUD",
                        "notes": "seeded from preview payload",
                        "plan_source": "coding_task_planning",
                        "plan": preview_payload.get("plan").cloned(),
                        "conversation": preview_payload.get("conversation").cloned(),
                        "planner_diagnostics": preview_payload.get("planner_diagnostics").cloned(),
                    })
                    .to_string(),
                ))
                .expect("create request"),
        )
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let create_body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("create body");
    let create_payload: Value = serde_json::from_slice(&create_body).expect("create json");
    assert_eq!(
        create_payload
            .get("session")
            .and_then(|row| row.get("source_kind"))
            .and_then(Value::as_str),
        Some("planner")
    );
    let session_id = create_payload
        .get("session")
        .and_then(|row| row.get("session_id"))
        .and_then(Value::as_str)
        .expect("session id")
        .to_string();

    let list_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/workflow-plans/sessions?project_slug=planner-crud")
                .body(Body::empty())
                .expect("list request"),
        )
        .await
        .expect("list response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("list body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("list json");
    assert!(list_payload
        .get("sessions")
        .and_then(Value::as_array)
        .is_some_and(|rows| rows.iter().any(|row| {
            row.get("session_id").and_then(Value::as_str) == Some(session_id.as_str())
        })));
    assert!(list_payload
        .get("sessions")
        .and_then(Value::as_array)
        .is_some_and(|rows| rows
            .iter()
            .any(|row| { row.get("source_kind").and_then(Value::as_str) == Some("planner") })));

    let patch_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/workflow-plans/sessions/{session_id}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "title": "Planner CRUD Renamed" }).to_string(),
                ))
                .expect("patch request"),
        )
        .await
        .expect("patch response");
    assert_eq!(patch_resp.status(), StatusCode::OK);
    let patch_body = to_bytes(patch_resp.into_body(), usize::MAX)
        .await
        .expect("patch body");
    let patch_payload: Value = serde_json::from_slice(&patch_body).expect("patch json");
    assert_eq!(
        patch_payload
            .get("session")
            .and_then(|row| row.get("title"))
            .and_then(Value::as_str),
        Some("Planner CRUD Renamed")
    );

    let duplicate_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/workflow-plans/sessions/{session_id}/duplicate"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "title": "Planner CRUD Copy" }).to_string(),
                ))
                .expect("duplicate request"),
        )
        .await
        .expect("duplicate response");
    assert_eq!(duplicate_resp.status(), StatusCode::OK);
    let duplicate_body = to_bytes(duplicate_resp.into_body(), usize::MAX)
        .await
        .expect("duplicate body");
    let duplicate_payload: Value = serde_json::from_slice(&duplicate_body).expect("duplicate json");
    let duplicate_session_id = duplicate_payload
        .get("session")
        .and_then(|row| row.get("session_id"))
        .and_then(Value::as_str)
        .expect("duplicate session id");
    assert_ne!(duplicate_session_id, session_id);
    assert_eq!(
        duplicate_payload
            .get("session")
            .and_then(|row| row.get("source_kind"))
            .and_then(Value::as_str),
        Some("forked_planner")
    );
}

#[tokio::test]
async fn workflow_planner_session_reset_recovers_missing_current_plan_id() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let preview_resp = app
        .clone()
        .oneshot(preview_request(json!({
            "prompt": "Write a small workflow plan for reset recovery",
            "workspace_root": "/tmp/custom-workspace"
        })))
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");

    let create_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow-plans/sessions")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "project_slug": "planner-reset",
                        "title": "Planner Reset",
                        "workspace_root": "/tmp/custom-workspace",
                        "goal": "Write a small workflow plan for reset recovery",
                        "notes": "seeded from preview payload",
                        "plan_source": "coding_task_planning",
                        "plan": preview_payload.get("plan").cloned(),
                        "conversation": preview_payload.get("conversation").cloned(),
                        "planner_diagnostics": preview_payload.get("planner_diagnostics").cloned(),
                    })
                    .to_string(),
                ))
                .expect("create request"),
        )
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let create_body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("create body");
    let create_payload: Value = serde_json::from_slice(&create_body).expect("create json");
    let session_id = create_payload
        .get("session")
        .and_then(|row| row.get("session_id"))
        .and_then(Value::as_str)
        .expect("session id")
        .to_string();

    let patch_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/workflow-plans/sessions/{session_id}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "current_plan_id": "" }).to_string()))
                .expect("patch request"),
        )
        .await
        .expect("patch response");
    assert_eq!(patch_resp.status(), StatusCode::OK);

    let reset_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/workflow-plans/sessions/{session_id}/reset"))
                .body(Body::empty())
                .expect("reset request"),
        )
        .await
        .expect("reset response");
    assert_eq!(reset_resp.status(), StatusCode::OK);
    let reset_body = to_bytes(reset_resp.into_body(), usize::MAX)
        .await
        .expect("reset body");
    let reset_payload: Value = serde_json::from_slice(&reset_body).expect("reset json");
    assert!(reset_payload
        .get("session")
        .and_then(|row| row.get("current_plan_id"))
        .and_then(Value::as_str)
        .is_some());
    assert!(reset_payload.get("plan").is_some());
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
