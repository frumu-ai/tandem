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
