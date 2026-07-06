use super::*;

fn sample_blueprint() -> Value {
    json!({
        "mission_id": "mission-preview",
        "title": "Competitive analysis mission",
        "goal": "Produce a cross-functional brief",
        "success_criteria": ["Useful brief", "Actionable recommendations"],
        "shared_context": "Targeting a Q2 planning review.",
        "workspace_root": "/tmp/mission-workspace",
        "orchestrator_template_id": "orchestrator-default",
        "team": {
            "allowed_mcp_servers": ["github"],
            "max_parallel_agents": 3,
            "orchestrator_only_tool_calls": false
        },
        "workstreams": [
            {
                "workstream_id": "research",
                "title": "Research",
                "objective": "Collect competitor signals",
                "role": "researcher",
                "prompt": "Research the market and summarize competitors.",
                "depends_on": [],
                "input_refs": [],
                "output_contract": { "kind": "report_markdown" }
            },
            {
                "workstream_id": "synthesis",
                "title": "Synthesis",
                "objective": "Turn findings into recommendations",
                "role": "analyst",
                "prompt": "Synthesize the findings into a brief.",
                "depends_on": ["research"],
                "input_refs": [{ "from_step_id": "research", "alias": "research_report" }],
                "output_contract": { "kind": "report_markdown" }
            }
        ],
        "review_stages": [
            {
                "stage_id": "approval",
                "stage_kind": "approval",
                "title": "Human approval",
                "target_ids": ["synthesis"],
                "prompt": "",
                "checklist": [],
                "gate": {
                    "required": true,
                    "decisions": ["approve", "rework", "cancel"],
                    "rework_targets": ["synthesis"]
                }
            }
        ]
    })
}

#[tokio::test]
#[serial_test::serial]
async fn mission_builder_generate_draft_returns_generated_blueprint_and_schedule() {
    let state = test_state().await;
    let app = app_router(state);
    let response = json!({
        "blueprint": {
            "mission_id": "",
            "title": "Weekly release mission",
            "goal": "Prepare a weekly release readiness package",
            "success_criteria": ["Includes risks", "Includes owner and date"],
            "shared_context": "Audience is engineering leadership.",
            "workspace_root": "/tmp/ignored-by-server",
            "workstreams": [
                {
                    "workstream_id": "collect",
                    "title": "Collect release inputs",
                    "objective": "Collect status, blockers, and dependencies",
                    "role": "analyst",
                    "prompt": "Gather current release inputs and summarize them.",
                    "depends_on": [],
                    "input_refs": [],
                    "output_contract": { "kind": "report_markdown" }
                },
                {
                    "workstream_id": "review",
                    "title": "Review release risk",
                    "objective": "Review the release packet and flag risks",
                    "role": "reviewer",
                    "prompt": "Review the release packet and highlight actionable risks.",
                    "depends_on": ["collect"],
                    "input_refs": [{ "from_step_id": "collect", "alias": "release_packet" }],
                    "output_contract": { "kind": "report_markdown" }
                },
                {
                    "workstream_id": "publish",
                    "title": "Publish readiness packet",
                    "objective": "Publish the final release readiness update",
                    "role": "operator",
                    "prompt": "Publish the approved release readiness packet.",
                    "depends_on": ["review"],
                    "input_refs": [{ "from_step_id": "review", "alias": "risk_review" }],
                    "output_contract": { "kind": "report_markdown" }
                }
            ],
            "review_stages": []
        },
        "suggested_schedule": {
            "type": "cron",
            "cron_expression": "0 9 * * 1",
            "timezone": "UTC"
        },
        "generation_warnings": ["verify the release audience before publishing"]
    });

    let previous = std::env::var("TANDEM_MISSION_BUILDER_TEST_GENERATE_RESPONSE").ok();
    std::env::set_var(
        "TANDEM_MISSION_BUILDER_TEST_GENERATE_RESPONSE",
        response.to_string(),
    );
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mission-builder/generate-draft")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "intent": "Every Monday prepare a release readiness packet for leadership.",
                        "workspace_root": "/tmp/mission-workspace"
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    if let Some(previous) = previous {
        std::env::set_var("TANDEM_MISSION_BUILDER_TEST_GENERATE_RESPONSE", previous);
    } else {
        std::env::remove_var("TANDEM_MISSION_BUILDER_TEST_GENERATE_RESPONSE");
    }

    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload
            .get("blueprint")
            .and_then(|row| row.get("workspace_root"))
            .and_then(Value::as_str),
        Some("/tmp/mission-workspace")
    );
    assert_eq!(
        payload
            .get("blueprint")
            .and_then(|row| row.get("mission_id"))
            .and_then(Value::as_str)
            .map(|value| value.starts_with("mission_")),
        Some(true)
    );
    assert_eq!(
        payload
            .get("suggested_schedule")
            .and_then(|row| row.get("type"))
            .and_then(Value::as_str),
        Some("cron")
    );
    assert_eq!(
        payload
            .get("validation")
            .and_then(Value::as_array)
            .map(|rows| rows
                .iter()
                .all(|row| { row.get("severity").and_then(Value::as_str) == Some("warning") })),
        Some(true)
    );
    assert_eq!(
        payload
            .get("generation_warnings")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
}

#[tokio::test]
async fn mission_builder_preview_returns_compiled_automation() {
    let state = test_state().await;
    let app = app_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mission-builder/compile-preview")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "blueprint": sample_blueprint() }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload
            .get("automation")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("draft")
    );
    assert_eq!(
        payload
            .get("automation")
            .and_then(|row| row.get("knowledge"))
            .and_then(|row| row.get("reuse_mode"))
            .and_then(Value::as_str),
        Some("preflight")
    );
    assert_eq!(
        payload
            .get("automation")
            .and_then(|row| row.get("knowledge"))
            .and_then(|row| row.get("read_spaces"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        payload
            .get("automation")
            .and_then(|row| row.get("flow"))
            .and_then(|row| row.get("nodes"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(3)
    );
    let nodes = payload
        .get("automation")
        .and_then(|row| row.get("flow"))
        .and_then(|row| row.get("nodes"))
        .and_then(Value::as_array)
        .expect("nodes");
    let research = nodes
        .iter()
        .find(|row| row.get("node_id").and_then(Value::as_str) == Some("research"))
        .expect("research node");
    assert_eq!(
        research
            .get("knowledge")
            .and_then(|row| row.get("trust_floor"))
            .and_then(Value::as_str),
        Some("promoted")
    );
    assert_eq!(
        research
            .get("knowledge")
            .and_then(|row| row.get("read_spaces"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        research
            .get("output_contract")
            .and_then(|row| row.get("validator"))
            .and_then(Value::as_str),
        Some("generic_artifact")
    );
    let approval = nodes
        .iter()
        .find(|row| row.get("node_id").and_then(Value::as_str) == Some("approval"))
        .expect("approval node");
    assert_eq!(
        approval
            .get("output_contract")
            .and_then(|row| row.get("validator"))
            .and_then(Value::as_str),
        Some("review_decision")
    );
}

#[tokio::test]
async fn mission_builder_apply_persists_draft_automation() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mission-builder/apply")
                .header("x-tandem-org-id", "org-a")
                .header("x-tandem-workspace-id", "workspace-a")
                .header("x-tandem-actor-id", "user-a")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "blueprint": sample_blueprint(),
                        "creator_id": "desktop"
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let automation_id = payload
        .get("automation")
        .and_then(|row| row.get("automation_id"))
        .and_then(Value::as_str)
        .expect("automation id");
    let stored = state
        .get_automation_v2(automation_id)
        .await
        .expect("stored");
    let tenant = stored.tenant_context();
    assert_eq!(tenant.org_id, "org-a");
    assert_eq!(tenant.workspace_id, "workspace-a");
    assert_eq!(tenant.actor_id.as_deref(), Some("user-a"));
    assert_eq!(stored.status, crate::AutomationV2Status::Draft);
    assert_eq!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("builder_kind"))
            .and_then(Value::as_str),
        Some("mission_blueprint")
    );
    assert_eq!(stored.knowledge.reuse_mode.to_string(), "preflight");
    assert_eq!(stored.knowledge.read_spaces.len(), 1);
}

#[tokio::test]
async fn mission_builder_apply_preserves_research_web_expectation_metadata() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut blueprint = sample_blueprint();
    let workstreams = blueprint
        .get_mut("workstreams")
        .and_then(Value::as_array_mut)
        .expect("workstreams");
    workstreams[0] = json!({
        "workstream_id": "research",
        "title": "Research",
        "objective": "Research the latest competitor signals",
        "role": "researcher",
        "prompt": "Research the latest competitor news and produce a citation-backed brief.",
        "depends_on": [],
        "input_refs": [],
        "output_contract": { "kind": "brief" },
        "metadata": {
            "builder": {
                "web_research_expected": true,
                "note": "require current web coverage"
            }
        }
    });

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/mission-builder/apply")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "blueprint": blueprint,
                        "creator_id": "desktop"
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let automation_id = payload
        .get("automation")
        .and_then(|row| row.get("automation_id"))
        .and_then(Value::as_str)
        .expect("automation id");
    let stored = state
        .get_automation_v2(automation_id)
        .await
        .expect("stored");
    let research = stored
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == "research")
        .expect("research node");
    assert_eq!(
        research
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("builder"))
            .and_then(|builder| builder.get("web_research_expected"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        research
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("builder"))
            .and_then(|builder| builder.get("note"))
            .and_then(Value::as_str),
        Some("require current web coverage")
    );
    assert_eq!(
        research
            .output_contract
            .as_ref()
            .and_then(|contract| contract.validator),
        Some(crate::AutomationOutputValidatorKind::ResearchBrief)
    );
}
