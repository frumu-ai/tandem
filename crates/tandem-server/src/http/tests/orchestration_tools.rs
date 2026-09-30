// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

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
    stored.updated_at_ms = previous_updated_at_ms.saturating_add(1);
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
    assert_eq!(recovered.metadata["updated_at_ms"], stored.updated_at_ms);
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

#[tokio::test]
async fn goal_start_tool_denies_private_source_and_cross_actor_replay() {
    use crate::app::state::tests::AutomationSpecBuilder;
    use tandem_automation::{
        OrchestrationNodeKind, OrchestrationNodeSpec, OrchestrationSpec, OrchestrationStatus,
    };
    use tandem_types::{
        AccessPermission, DataBoundary, DataClass, GrantSource, PrincipalRef, ResourceKind,
        ResourceRef, ResourceScope, ScopedGrant, StrictTenantContext,
    };

    let directory = tempfile::tempdir().unwrap();
    let mut state = test_state().await;
    state.automation_v2_runs_path = directory.path().join("automation_v2_runs.json");
    let alice_tenant = TenantContext::explicit_user_workspace(
        "org-a",
        "dep-a",
        Some("dep-a".to_string()),
        "alice",
    );
    let mut workflow = AutomationSpecBuilder::new("mcp-private-root").build();
    workflow.set_tenant_context(&alice_tenant);
    let workflow = state.put_automation_v2(workflow).await.unwrap();
    let now = crate::now_ms();
    let draft = OrchestrationSpec {
        schema_version: 1,
        orchestration_id: "mcp-private-source".to_string(),
        name: "Private MCP source".to_string(),
        description: None,
        status: OrchestrationStatus::Draft,
        version: 0,
        root_node_id: "root".to_string(),
        nodes: vec![OrchestrationNodeSpec {
            node_id: "root".to_string(),
            name: "Root workflow".to_string(),
            position: Default::default(),
            node: OrchestrationNodeKind::Workflow {
                automation_id: workflow.automation_id.clone(),
                pinned_definition_hash: Some(
                    crate::stateful_runtime::automation_definition_snapshot_hash(&workflow),
                ),
                allowed_transition_keys: Vec::new(),
                accepts_artifact_types: Vec::new(),
                emits_artifact_types: Vec::new(),
            },
        }],
        edges: Vec::new(),
        goal_policy: Default::default(),
        tenant_context: alice_tenant.clone(),
        created_at_ms: now,
        updated_at_ms: now,
        published_at_ms: None,
        metadata: Some(json!({"created_by": "alice"})),
    };
    let store = crate::stateful_runtime::OrchestrationStateStore::from_automation_runs_path(
        &state.automation_v2_runs_path,
    )
    .unwrap();
    store.put_orchestration_draft(&draft, None).unwrap();
    let mut published = draft.clone();
    published.status = OrchestrationStatus::Published;
    published.version = 1;
    published.updated_at_ms = now.saturating_add(1);
    published.published_at_ms = Some(now);
    store
        .publish_orchestration_draft(&published, Some(now))
        .unwrap();

    let tool = crate::http::orchestration_tools::orchestration_tools(state)
        .into_iter()
        .find(|tool| tool.schema().name == "goal_start")
        .unwrap();
    let verified = |actor: &str| {
        let tenant = TenantContext::explicit_user_workspace(
            "org-a",
            "dep-a",
            Some("dep-a".to_string()),
            actor,
        );
        tandem_types::VerifiedTenantContext {
            tenant_context: tenant,
            human_actor: tandem_types::HumanActor::tandem_user(actor),
            authority_chain: tandem_types::AuthorityChain::from_request(
                tandem_types::RequestPrincipal::authenticated_user(actor, "tandem-web"),
            ),
            roles: Vec::new(),
            org_units: Vec::new(),
            capabilities: Vec::new(),
            policy_version: None,
            strict_projection: None,
            issuer: "tandem-web".to_string(),
            audience: "tandem-runtime".to_string(),
            issued_at_ms: 1_000,
            expires_at_ms: 9_999_999_999_999,
            assertion_id: format!("tool-{actor}"),
            assertion_key_id: None,
        }
    };
    let args = |key: &str, context: tandem_types::VerifiedTenantContext| {
        json!({
            "orchestration_id": "mcp-private-source",
            "objective": "MCP private objective",
            "idempotency_key": key,
            "__verified_tenant_context": context,
        })
    };
    let bob_tenant =
        TenantContext::explicit_user_workspace("org-a", "dep-a", Some("dep-a".to_string()), "bob");
    let bob_new = tool
        .execute_for_tenant(args("bob-new", verified("bob")), bob_tenant.clone())
        .await
        .expect_err("a different actor must not start a private source");
    assert!(bob_new.to_string().contains("goal not found"));
    for version in [None, Some(1)] {
        let mut missing_source = args("bob-missing", verified("bob"));
        missing_source["orchestration_id"] = json!("missing-goal-source");
        missing_source["orchestration_version"] = json!(version);
        let absent = tool
            .execute_for_tenant(missing_source, bob_tenant.clone())
            .await
            .expect_err("missing and private sources must be indistinguishable");
        assert_eq!(absent.to_string(), bob_new.to_string());
    }
    let alice_start = tool
        .execute_for_tenant(args("shared-key", verified("alice")), alice_tenant.clone())
        .await
        .unwrap();
    let goal_id = alice_start.metadata["goal"]["goal_id"]
        .as_str()
        .unwrap()
        .to_string();
    let bob_replay = tool
        .execute_for_tenant(args("shared-key", verified("bob")), bob_tenant.clone())
        .await
        .expect_err("a different actor must not retrieve the private replay");
    assert!(bob_replay.to_string().contains("goal not found"));
    let alice_replay = tool
        .execute_for_tenant(args("shared-key", verified("alice")), alice_tenant.clone())
        .await
        .unwrap();
    assert_eq!(alice_replay.metadata["replayed"], true);

    let bob_with_grant = |kind: ResourceKind, resource_id: &str| {
        let mut context = verified("bob");
        let resource = ResourceRef::new("org-a", "dep-a", kind, resource_id);
        let principal = PrincipalRef::human_user("bob");
        context.strict_projection = Some(
            StrictTenantContext::new(
                bob_tenant.clone(),
                principal.clone(),
                context.authority_chain.clone(),
                ResourceScope::root(resource.clone()),
                tandem_types::AssertionMetadata::new(
                    "tandem-web",
                    "tandem-runtime",
                    1_000,
                    9_999_999_999_999,
                    "bob-run-read",
                ),
            )
            .with_grants(vec![ScopedGrant::new(
                "bob-run-read",
                principal,
                resource,
                GrantSource::Direct,
            )
            .with_permissions(vec![AccessPermission::Read])
            .with_data_classes(vec![DataClass::Internal])])
            .with_data_boundary(DataBoundary::allow(vec![DataClass::Internal])),
        );
        context
    };
    let alias_replay = tool
        .execute_for_tenant(
            args(
                "shared-key",
                bob_with_grant(ResourceKind::Project, &goal_id),
            ),
            bob_tenant.clone(),
        )
        .await
        .expect_err("a same-ID Project grant is not a Run read grant");
    assert!(alias_replay.to_string().contains("goal not found"));
    let visible_replay = tool
        .execute_for_tenant(
            args("shared-key", bob_with_grant(ResourceKind::Run, &goal_id)),
            bob_tenant.clone(),
        )
        .await
        .unwrap();
    assert_eq!(visible_replay.metadata["replayed"], true);
    assert_eq!(visible_replay.metadata["goal"]["goal_id"], goal_id);
    let workspace_replay = tool
        .execute_for_tenant(
            args(
                "shared-key",
                bob_with_grant(ResourceKind::Workspace, "dep-a"),
            ),
            bob_tenant,
        )
        .await
        .unwrap();
    assert_eq!(workspace_replay.metadata["replayed"], true);
}
