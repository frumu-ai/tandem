use super::*;

#[tokio::test]
async fn orchestration_tool_catalog_is_complete_and_handoff_targets_are_not_agent_selected() {
    let tools = crate::http::orchestration_tools::orchestration_tools(test_state().await);
    let schemas = tools.iter().map(|tool| tool.schema()).collect::<Vec<_>>();
    let names = schemas
        .iter()
        .map(|schema| schema.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            "orchestration_create_draft",
            "orchestration_validate",
            "orchestration_publish",
            "goal_start",
            "goal_get",
            "goal_cancel",
            "handoff_emit",
            "handoff_approve",
            "wait_inspect",
            "wait_resolve",
        ]
    );
    let handoff = schemas
        .iter()
        .find(|schema| schema.name == "handoff_emit")
        .unwrap();
    let properties = handoff.input_schema["properties"].as_object().unwrap();
    assert!(properties.contains_key("transition_key"));
    assert!(!properties.contains_key("target_automation_id"));
    assert!(!properties.contains_key("target_node_id"));
}

#[tokio::test]
async fn approval_and_wait_resolution_tools_fail_closed_without_explicit_authority() {
    let tools = crate::http::orchestration_tools::orchestration_tools(test_state().await);
    for (name, args) in [
        (
            "handoff_approve",
            json!({
                "goal_id": "goal-1",
                "handoff_id": "handoff-1",
                "decision": "approve",
                "idempotency_key": "approve-1"
            }),
        ),
        (
            "wait_resolve",
            json!({
                "goal_id": "goal-1",
                "wait_id": "wait-1",
                "resolution": {"approved": true},
                "idempotency_key": "resolve-1"
            }),
        ),
    ] {
        let tool = tools
            .iter()
            .find(|tool| tool.schema().name == name)
            .unwrap();
        let error = tool
            .execute_for_tenant(args, TenantContext::local_implicit())
            .await
            .expect_err("authority-free mutation must fail closed");
        assert!(error.to_string().contains("lacks orchestration."));
    }
}

#[tokio::test]
async fn orchestration_create_tool_replays_matching_idempotency_key() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = test_state().await;
    let runs_path = directory.path().join("automation_v2_runs.json");
    state.automation_v2_runs_path = runs_path.clone();
    let tool = crate::http::orchestration_tools::orchestration_tools(state)
        .into_iter()
        .find(|tool| tool.schema().name == "orchestration_create_draft")
        .unwrap();
    let tenant = TenantContext::local_implicit();
    let args = json!({
        "orchestration_id": "mcp-loop",
        "name": "MCP loop",
        "root_node_id": "done",
        "nodes": [{
            "node_id": "done",
            "name": "Done",
            "kind": "terminal",
            "outcome": "complete"
        }],
        "edges": [],
        "idempotency_key": "create-loop-1"
    });
    let mut conflicting = args.clone();
    conflicting["orchestration_id"] = json!("different-loop");

    let first = tool
        .execute_for_tenant(args.clone(), tenant.clone())
        .await
        .unwrap();
    let replay = tool
        .execute_for_tenant(args.clone(), tenant.clone())
        .await
        .unwrap();

    assert_eq!(first.metadata, replay.metadata);

    let store_paths =
        crate::stateful_runtime::OrchestrationStorePaths::from_automation_runs_path(&runs_path);
    let store =
        crate::stateful_runtime::OrchestrationStateStore::open(store_paths.clone()).unwrap();
    let tenant = TenantContext::local_implicit();
    let mut stored = store
        .get_orchestration_draft(&tenant, "mcp-loop")
        .unwrap()
        .unwrap();
    let previous_updated_at_ms = stored.updated_at_ms;
    stored.created_at_ms = 123;
    stored.updated_at_ms = 456;
    store
        .put_orchestration_draft(&stored, Some(previous_updated_at_ms))
        .unwrap();
    let connection = rusqlite::Connection::open(&store_paths.database_path).unwrap();
    connection
        .execute(
            "UPDATE orchestration_tool_requests
             SET response_json = NULL, completed_at_ms = NULL, created_at_ms = 0
             WHERE operation = 'orchestration_create_draft'
               AND idempotency_key = 'create-loop-1'",
            [],
        )
        .unwrap();
    let recovered = tool.execute_for_tenant(args.clone(), tenant).await.unwrap();
    assert_eq!(recovered.metadata["updated_at_ms"], 456);
    assert_eq!(recovered.metadata["orchestration"]["created_at_ms"], 123);

    let error = tool
        .execute_for_tenant(conflicting, TenantContext::local_implicit())
        .await
        .expect_err("one operation key must not bind multiple drafts");
    assert!(error.to_string().contains("already bound"));
}

#[tokio::test]
async fn orchestration_publish_tool_rejects_archived_drafts() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = test_state().await;
    let runs_path = directory.path().join("automation_v2_runs.json");
    state.automation_v2_runs_path = runs_path.clone();
    let mut tools = crate::http::orchestration_tools::orchestration_tools(state);
    let create_index = tools
        .iter()
        .position(|tool| tool.schema().name == "orchestration_create_draft")
        .unwrap();
    let create = tools.remove(create_index);
    let publish = tools
        .into_iter()
        .find(|tool| tool.schema().name == "orchestration_publish")
        .unwrap();
    let tenant = TenantContext::local_implicit();

    create
        .execute_for_tenant(
            json!({
                "orchestration_id": "archived-loop",
                "name": "Archived loop",
                "root_node_id": "done",
                "nodes": [{
                    "node_id": "done",
                    "name": "Done",
                    "kind": "terminal",
                    "outcome": "complete"
                }],
                "edges": [],
                "idempotency_key": "create-archived-1"
            }),
            tenant.clone(),
        )
        .await
        .unwrap();
    let store =
        crate::stateful_runtime::OrchestrationStateStore::from_automation_runs_path(&runs_path)
            .unwrap();
    let mut draft = store
        .get_orchestration_draft(&tenant, "archived-loop")
        .unwrap()
        .unwrap();
    let expected_updated_at_ms = draft.updated_at_ms;
    draft.status = tandem_automation::OrchestrationStatus::Archived;
    draft.updated_at_ms += 1;
    store
        .put_orchestration_draft(&draft, Some(expected_updated_at_ms))
        .unwrap();

    let error = publish
        .execute_for_tenant(
            json!({
                "orchestration_id": "archived-loop",
                "idempotency_key": "publish-archived-1"
            }),
            tenant,
        )
        .await
        .expect_err("archived drafts must remain unpublishable through MCP");
    assert!(error
        .to_string()
        .contains("archived drafts cannot be published"));
}
