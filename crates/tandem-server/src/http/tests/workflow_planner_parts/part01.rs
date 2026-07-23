// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

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

fn compact_research_prompt() -> &'static str {
    r#"research this topic:

"What are the current approaches to making AI agents reliable for business workflows?"

Use the connected Tandem MCP docs as reference if needed, and use the connected Reddit MCP plus web research to gather current market signals, discussions, examples, and source links.

Then create a concise market brief and save the completed report into the Notion database:

Operational Workflow Results
collection://892d3e9b-2bf8-4b3e-a541-dc725f77295d

The Notion page should include:
- Summary
- Key Findings
- Market Notes
- Reddit Signals
- Sources
- Tandem Run details"#
}

fn oversized_compact_research_steps() -> Vec<Value> {
    let objectives = [
        "Define scope, success criteria, and report requirements.",
        "Use mcp.tandem_mcp.search_docs for reliable workflow design docs.",
        "Use mcp.tandem_mcp.get_doc for selected Tandem docs.",
        "Use web_research and web_fetch for current market approaches.",
        "Collect vendor and enterprise examples with web source links.",
        "Collect observability, guardrails, evals, retries, and fallback practices.",
        "Use mcp.composio.reddit_get_subreddits_search for Reddit signals.",
        "Use mcp.composio.reddit_search_across_subreddits for candidate posts.",
        "Use mcp.composio.reddit_retrieve_reddit_post for discussion excerpts.",
        "Extract practitioner Reddit concerns and reliability tactics.",
        "Normalize sources into one ledger.",
        "Synthesize a taxonomy of reliable AI agent workflow approaches.",
        "Draft Summary section.",
        "Draft Key Findings section.",
        "Draft Market Notes section.",
        "Draft Reddit Signals section.",
        "Draft Sources section.",
        "Draft Tandem Run details section.",
        "Assemble concise market brief.",
        "Validate the brief is current, concise, and section-complete.",
        "Transform brief into Notion page payload.",
        "Create Notion page in collection://892d3e9b-2bf8-4b3e-a541-dc725f77295d.",
        "Verify Notion page has Summary.",
        "Verify Notion page has Key Findings.",
        "Verify Notion page has Market Notes.",
        "Verify Notion page has Reddit Signals.",
        "Verify Notion page has Sources.",
        "Verify Notion page has Tandem Run details.",
        "Capture final Notion page URL and run details.",
    ];
    objectives
        .iter()
        .enumerate()
        .map(|(index, objective)| {
            let step_id = format!("generated_step_{:02}", index + 1);
            let previous = (index > 0).then(|| format!("generated_step_{index:02}"));
            let depends_on = previous.iter().map(String::as_str).collect::<Vec<_>>();
            step_json(
                &step_id,
                if objective.contains("Notion") || objective.contains("collection://") {
                    "deliver"
                } else if objective.contains("Draft")
                    || objective.contains("Assemble")
                    || objective.contains("Synthesize")
                {
                    "synthesize"
                } else {
                    "research"
                },
                objective,
                &depends_on,
                "agent_planner",
                json!([]),
                "structured_json",
            )
        })
        .collect()
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
#[serial_test::serial]
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
    assert_eq!(
        payload
            .get("planner_diagnostics")
            .and_then(|row| row.get("generated_step_count"))
            .and_then(Value::as_u64),
        Some(3)
    );
    assert!(
        payload
            .get("planner_diagnostics")
            .and_then(|row| row.get("decomposition_profile"))
            .is_some(),
        "planner diagnostics should include a decomposition profile for a valid plan"
    );
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
#[serial_test::serial]
async fn workflow_plan_preview_compacts_oversized_generated_llm_plan() {
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
            "assistant_text": "Built an oversized workflow plan.",
            "plan": llm_plan_json(
                "AI workflow reliability market brief",
                "Oversized section/source split draft.",
                manual_schedule_json(),
                "/tmp/ignored-by-normalizer",
                oversized_compact_research_steps(),
                None
            )
        })
        .to_string(),
    );

    let resp = app
        .oneshot(preview_request(json!({
            "prompt": compact_research_prompt(),
            "workspace_root": "/tmp/custom-workspace",
            "allowed_mcp_servers": ["tandem_mcp", "reddit", "notion"],
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
    assert!(steps.len() <= tandem_plan_compiler::api::GENERATED_WORKFLOW_MAX_STEPS);
    let step_ids = steps
        .iter()
        .filter_map(|step| step.get("step_id").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert!(step_ids.contains(&"gather_tandem_docs"));
    assert!(step_ids.contains(&"gather_reddit_signals"));
    assert!(step_ids.contains(&"gather_market_sources"));
    assert!(step_ids.contains(&"draft_market_brief"));
    assert!(step_ids.contains(&"create_and_verify_notion_page"));
    let objectives = steps
        .iter()
        .filter_map(|step| step.get("objective").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(objectives.contains("mcp.tandem_mcp.search_docs"));
    assert!(objectives.contains("web_research"));
    assert!(objectives.contains("mcp.composio.reddit"));
    assert!(objectives.contains("collection://892d3e9b-2bf8-4b3e-a541-dc725f77295d"));
    assert!(objectives.contains("Summary"));
    assert!(objectives.contains("Key Findings"));
    assert_eq!(
        payload
            .get("planner_diagnostics")
            .and_then(|row| row.get("task_budget"))
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("compacted")
    );
    assert_eq!(
        payload
            .get("planner_diagnostics")
            .and_then(|row| row.get("task_budget"))
            .and_then(|row| row.get("original_step_count"))
            .and_then(Value::as_u64),
        Some(29)
    );
}

#[tokio::test]
async fn workflow_plan_apply_rejects_oversized_generated_plan_without_compaction_metadata() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let plan = llm_plan_json(
        "Oversized generated plan",
        "This plan should be rejected at apply.",
        manual_schedule_json(),
        "/tmp/workspace",
        oversized_compact_research_steps(),
        None,
    );

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow-plans/apply")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "plan": plan }).to_string()))
                .expect("apply request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("WORKFLOW_PLAN_TASK_BUDGET_EXCEEDED")
    );
    assert_eq!(
        payload
            .get("task_budget")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("rejected")
    );
    assert_eq!(
        payload
            .get("task_budget")
            .and_then(|row| row.get("max_generated_steps"))
            .and_then(Value::as_u64),
        Some(8)
    );
}

#[tokio::test]
#[serial_test::serial]
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
    assert_eq!(
        payload
            .get("planner_diagnostics")
            .and_then(|row| row.get("generated_step_count"))
            .and_then(Value::as_u64),
        Some(2)
    );
    assert!(
        payload
            .get("planner_diagnostics")
            .and_then(|row| row.get("decomposition_profile"))
            .is_some(),
        "planner diagnostics should include a decomposition profile for a repaired partial plan"
    );
}

#[tokio::test]
#[serial_test::serial]
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
#[serial_test::serial]
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
    assert_eq!(
        payload
            .get("planner_diagnostics")
            .and_then(|row| row.get("generated_step_count"))
            .and_then(Value::as_u64),
        Some(2)
    );
    assert!(
        payload
            .get("planner_diagnostics")
            .and_then(|row| row.get("decomposition_profile"))
            .is_some(),
        "suffix-variant step ids should not force fallback"
    );
}

#[tokio::test]
#[serial_test::serial]
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
                vec![step_json("123 totally custom step", "custom", "Invalid.", &[], "worker", json!([]), "structured_json")],
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
#[serial_test::serial]
async fn workflow_plan_preview_uses_phased_fallback_for_complex_prompt_on_invalid_json() {
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
            "assistant_text": "missing required planner action payload"
        })
        .to_string(),
    );

    let resp = app
        .oneshot(preview_request(json!({
            "prompt": "Analyze the local RESUME.md file and use it as the source of truth for skills, role targets, seniority, technologies, and geography preferences. If resume_overview.md does not exist, create it. Use websearch to find relevant job boards and recruitment sites in Europe. Save all results to daily_results_2026-04-15.md and append new findings on repeated runs.",
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
            .get("planner_diagnostics")
            .and_then(|row| row.get("fallback_reason"))
            .and_then(Value::as_str),
        Some("invalid_json")
    );
    let step_ids = payload
        .get("plan")
        .and_then(|row| row.get("steps"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.get("step_id").and_then(Value::as_str))
                .collect::<Vec<_>>()
        })
        .expect("steps");
    assert!(
        step_ids.len() > 1,
        "fallback plan should not collapse to one step"
    );
    assert_eq!(step_ids.first().copied(), Some("assess"));
    assert_eq!(step_ids.get(1).copied(), Some("collect_inputs"));
    assert_eq!(step_ids.last().copied(), Some("finalize_outputs"));
}

#[tokio::test]
#[serial_test::serial]
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
#[serial_test::serial]
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
#[serial_test::serial]
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
#[serial_test::serial]
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
#[serial_test::serial]
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
#[serial_test::serial(pack_signature_env)]
async fn workflow_plan_pack_import_preview_and_commit_installs_pack_session() {
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
                "Pack Import Workflow",
                "Create a workflow pack import bundle.",
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
    let bundle = serde_json::to_string_pretty(
        preview_payload
            .get("plan_package_bundle")
            .expect("plan package bundle"),
    )
    .expect("bundle json");

    let temp = tempfile::tempdir().expect("tempdir");
    let pack_path = temp.path().join("workflow-pack.zip");
    let _trusted_key = write_signed_pack_zip_with_entries(
        &pack_path,
        r#"
name: workflow-pack-test
version: 0.1.0
type: workflow
pack_id: workflow-pack-test
marketplace:
  publisher:
    publisher_id: test
    display_name: Test
    verification_tier: unverified
  listing:
    display_name: Workflow Pack Test
    description: Workflow pack import regression
    license_spdx: Proprietary
    categories: [workflow]
    tags: [workflow, tandem]
    cover_image: assets/cover.png
capabilities:
  required: []
entrypoints:
  workflows: [pack-import-workflow]
contents:
  workflows:
    - id: pack-import-workflow
      path: workflows/pack-import-workflow/plan-package.json
      format: workflow_plan_bundle
"#,
        &[
            ("workflows/pack-import-workflow/plan-package.json", &bundle),
            ("assets/cover.png", "fake image bytes"),
        ],
    );

    let import_preview_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow-plans/import/pack/preview")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "path": pack_path.to_string_lossy() }).to_string(),
                ))
                .expect("pack preview request"),
        )
        .await
        .expect("pack preview response");
    assert_eq!(import_preview_resp.status(), StatusCode::OK);
    let import_preview_body = to_bytes(import_preview_resp.into_body(), usize::MAX)
        .await
        .expect("pack preview body");
    let import_preview_payload: Value =
        serde_json::from_slice(&import_preview_body).expect("pack preview json");
    assert_eq!(
        import_preview_payload
            .get("persisted")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        import_preview_payload
            .get("pack")
            .and_then(|value| value.get("pack_type"))
            .and_then(Value::as_str),
        Some("workflow")
    );
    assert!(import_preview_payload
        .get("cover_image_data_url")
        .and_then(Value::as_str)
        .map(|value| value.starts_with("data:image/png;base64,"))
        .unwrap_or(false));
    assert_eq!(
        import_preview_payload
            .get("workflows")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );

    let import_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow-plans/import/pack")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "path": pack_path.to_string_lossy() }).to_string(),
                ))
                .expect("pack import request"),
        )
        .await
        .expect("pack import response");
    assert_eq!(import_resp.status(), StatusCode::OK);
    let import_body = to_bytes(import_resp.into_body(), usize::MAX)
        .await
        .expect("pack import body");
    let import_payload: Value = serde_json::from_slice(&import_body).expect("pack import json");
    assert_eq!(
        import_payload.get("persisted").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        import_payload
            .get("installed")
            .and_then(|value| value.get("pack_id"))
            .and_then(Value::as_str),
        Some("workflow-pack-test")
    );
    let session_id = import_payload
        .get("sessions")
        .and_then(Value::as_array)
        .and_then(|sessions| sessions.first())
        .and_then(|session| session.get("session_id"))
        .and_then(Value::as_str)
        .expect("session id")
        .to_string();
    let stored_session = state
        .get_workflow_planner_session(&session_id)
        .await
        .expect("stored pack session");
    assert_eq!(stored_session.source_kind, "workflow_pack");
    assert_eq!(
        stored_session.source_pack_id.as_deref(),
        Some("workflow-pack-test")
    );
}

#[tokio::test]
#[serial_test::serial]
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
#[serial_test::serial]
async fn workflow_planner_session_create_normalizes_control_panel_provenance() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());
    let _guard = PlannerEnvGuard::new(&["TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE"]);
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        json!({
            "action": "build",
            "plan": llm_plan_json(
                "Human Owned Workflow",
                "Create a human-owned workflow draft.",
                manual_schedule_json(),
                "/tmp/control-panel-workspace",
                vec![step_json(
                    "draft_summary",
                    "draft",
                    "Draft the summary.",
                    &[],
                    "writer",
                    json!([]),
                    "report_markdown"
                )],
                Some(planner_preferences())
            )
        })
        .to_string(),
    );

    let preview_resp = app
        .clone()
        .oneshot(preview_request(json!({
            "prompt": "Create a human-owned workflow draft",
            "workspace_root": "/tmp/control-panel-workspace",
            "plan_source": "intent_planner_page",
            "operator_preferences": planner_preferences()
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
                        "project_slug": "planner-control-panel",
                        "title": "Control Panel Draft",
                        "workspace_root": "/tmp/control-panel-workspace",
                        "goal": "Create a human-owned workflow draft",
                        "plan_source": "intent_planner_page",
                        "plan": preview_payload.get("plan").cloned(),
                        "conversation": preview_payload.get("conversation").cloned(),
                        "planner_diagnostics": preview_payload.get("planner_diagnostics").cloned(),
                        "planning": {
                            "mode": "channel",
                            "source_platform": "control_panel",
                            "validation_status": "pending",
                            "approval_status": "not_required"
                        }
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
    let session = create_payload.get("session").expect("session");
    assert_eq!(
        session
            .get("planning")
            .and_then(|row| row.get("mode"))
            .and_then(Value::as_str),
        Some("workflow_planning")
    );
    assert_eq!(
        session
            .get("planning")
            .and_then(|row| row.get("created_by_agent"))
            .and_then(Value::as_str),
        Some("human")
    );
    assert_eq!(
        session
            .get("planning")
            .and_then(|row| row.get("source_platform"))
            .and_then(Value::as_str),
        Some("control_panel")
    );
    assert_eq!(
        session
            .get("planning")
            .and_then(|row| row.get("draft_id"))
            .and_then(Value::as_str),
        session.get("current_plan_id").and_then(Value::as_str)
    );
    let _ = next_event_of_type(&mut rx, "workflow_planner.session.started").await;
}

#[tokio::test]
async fn workflow_planner_session_create_rejects_oversized_generated_plan() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let plan = llm_plan_json(
        "Oversized generated session plan",
        "This session draft should be rejected.",
        manual_schedule_json(),
        "/tmp/workspace",
        oversized_compact_research_steps(),
        None,
    );

    let create_resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow-plans/sessions")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "project_slug": "planner-control-panel",
                        "title": "Oversized Draft",
                        "workspace_root": "/tmp/workspace",
                        "goal": compact_research_prompt(),
                        "plan_source": "intent_planner_page",
                        "plan": plan,
                    })
                    .to_string(),
                ))
                .expect("create request"),
        )
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("create body");
    let payload: Value = serde_json::from_slice(&body).expect("create json");
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("WORKFLOW_PLAN_TASK_BUDGET_EXCEEDED")
    );
}
