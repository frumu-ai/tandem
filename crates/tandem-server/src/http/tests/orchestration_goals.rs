// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Contract tests for the public orchestration authoring APIs (TAN-694) and
//! long-running goal runtime APIs (TAN-695).

use super::*;

use crate::app::state::tests::AutomationSpecBuilder;
use crate::stateful_runtime::automation_definition_snapshot_hash;
use tandem_enterprise_contract::{
    AccessEffect, AccessPermission, OrganizationUnit, OrganizationUnitAccessGrant,
    OrganizationUnitKind, OrganizationUnitMembership, OrganizationUnitMembershipSource,
    PrincipalRef, ResourceKind, ResourceRef, ResourceScope,
};

fn orchestration_request(
    method: &str,
    uri: impl Into<String>,
    org_id: &str,
    workspace_id: &str,
    body: Option<Value>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri.into())
        .header("x-tandem-org-id", org_id)
        .header("x-tandem-workspace-id", workspace_id)
        .header("x-tandem-actor-id", "operator");
    let body = match body {
        Some(value) => {
            builder = builder.header("content-type", "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    builder.body(body).expect("orchestration request")
}

fn local_request(method: &str, uri: impl Into<String>, body: Option<Value>) -> Request<Body> {
    orchestration_request(method, uri, "local", "local", body)
}

fn unauthenticated_local_request(
    method: &str,
    uri: impl Into<String>,
    body: Option<Value>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri.into())
        .header("x-tandem-org-id", "local")
        .header("x-tandem-workspace-id", "local");
    let body = match body {
        Some(value) => {
            builder = builder.header("content-type", "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    builder.body(body).expect("unauthenticated request")
}

fn verified_context(actor_id: &str) -> tandem_types::VerifiedTenantContext {
    let tenant_context = TenantContext::local_implicit();
    let request_principal =
        tandem_types::RequestPrincipal::authenticated_user(actor_id, "tandem-web");
    tandem_types::VerifiedTenantContext {
        tenant_context,
        human_actor: tandem_types::HumanActor::tandem_user(actor_id),
        authority_chain: tandem_types::AuthorityChain::from_request(request_principal),
        roles: Vec::new(),
        org_units: Vec::new(),
        capabilities: Vec::new(),
        policy_version: None,
        strict_projection: None,
        issuer: "tandem-web".to_string(),
        audience: "tandem-runtime".to_string(),
        issued_at_ms: 1_000,
        expires_at_ms: 9_999_999_999_999,
        assertion_id: format!("assertion-{actor_id}"),
        assertion_key_id: None,
    }
}

fn foreign_handoff(goal_id: &str) -> tandem_automation::WorkflowHandoff {
    tandem_automation::WorkflowHandoff {
        schema_version: 1,
        handoff_id: "foreign-projection-handoff".to_string(),
        idempotency_key: "foreign-projection-key".to_string(),
        goal_id: goal_id.to_string(),
        orchestration_id: "orch-goals".to_string(),
        orchestration_version: 1,
        tenant_context: TenantContext::explicit("other", "tenant", None),
        edge_id: "plan-to-execute".to_string(),
        transition_key: "continue".to_string(),
        source_automation_id: "planner".to_string(),
        source_run_id: "foreign-run".to_string(),
        source_node_id: "plan".to_string(),
        target_automation_id: "executor".to_string(),
        target_node_id: "execute".to_string(),
        artifact: tandem_automation::OrchestrationArtifactRef {
            artifact_type: "plan".to_string(),
            content_path: None,
            content_digest: None,
            value: Some(json!({"tenant": "other"})),
        },
        status: tandem_automation::WorkflowHandoffStatus::PendingApproval,
        created_at_ms: 1,
        updated_at_ms: 1,
        consumed_by_run_id: None,
        metadata: None,
    }
}

async fn json_body(response: axum::response::Response) -> Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

async fn dispatch(app: &Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.expect("dispatch");
    let status = response.status();
    (status, json_body(response).await)
}

/// Mark a goal-linked Automation V2 run completed in both the in-memory map
/// and the durable store, as the scheduler would after real execution.
async fn complete_run(state: &AppState, run_id: &str) {
    let mut run = state
        .get_automation_v2_run(run_id)
        .await
        .expect("goal run exists");
    run.status = tandem_automation::AutomationRunStatus::Completed;
    run.finished_at_ms = Some(crate::now_ms());
    run.updated_at_ms = crate::now_ms();
    state
        .automation_v2_runs
        .write()
        .await
        .insert(run.run_id.clone(), run.clone());
    crate::stateful_runtime::OrchestrationStateStore::from_automation_runs_path(
        &state.automation_v2_runs_path,
    )
    .expect("store")
    .upsert_automation_runs([&run])
    .expect("persist completed run");
}

/// Seed planner/executor Automation V2 definitions and return their current
/// definition hashes for pinning.
async fn seed_workflows(state: &AppState) -> (String, String) {
    let planner = state
        .put_automation_v2(AutomationSpecBuilder::new("planner").build())
        .await
        .expect("seed planner");
    let executor = state
        .put_automation_v2(AutomationSpecBuilder::new("executor").build())
        .await
        .expect("seed executor");
    (
        automation_definition_snapshot_hash(&planner),
        automation_definition_snapshot_hash(&executor),
    )
}

async fn seed_enterprise_workflow(
    state: &AppState,
    automation_id: &str,
    authorize_operator: bool,
) -> (String, OrganizationUnitAccessGrant) {
    let tenant = TenantContext::local_implicit();
    let unit = OrganizationUnit::active(
        "workflow-authors",
        tenant.clone(),
        "Workflow Authors",
        OrganizationUnitKind::Team,
        PrincipalRef::human_user("admin"),
        1,
    );
    let resource = ResourceRef::new(
        tenant.org_id.clone(),
        tenant.workspace_id.clone(),
        ResourceKind::Automation,
        automation_id,
    );
    let grant = OrganizationUnitAccessGrant::active(
        "workflow-grant",
        tenant.clone(),
        unit.principal_ref(),
        resource.clone(),
        1,
    )
    .with_permissions(vec![AccessPermission::View, AccessPermission::Execute]);

    state
        .enterprise
        .org_units
        .write()
        .await
        .insert("workflow-authors".to_string(), unit.clone());
    state
        .enterprise
        .org_unit_access_grants
        .write()
        .await
        .insert("allow-workflow-grant".to_string(), grant.clone());
    if authorize_operator {
        let membership = OrganizationUnitMembership::active(
            "operator-workflow-author",
            tenant,
            unit.principal_ref(),
            PrincipalRef::human_user("operator"),
            OrganizationUnitMembershipSource::Direct,
            1,
        );
        state
            .enterprise
            .org_unit_memberships
            .write()
            .await
            .insert(membership.membership_id.clone(), membership);
    }

    let automation = AutomationSpecBuilder::new(automation_id)
        .metadata(json!({
            "enterprise_scope": {
                "owning_org_unit_id": "workflow-authors",
                "resource_scope": ResourceScope::root(resource),
                "delegation_grant_ids": ["workflow-grant"],
            }
        }))
        .build();
    let automation = state
        .put_automation_v2(automation)
        .await
        .expect("seed enterprise workflow");
    (automation_definition_snapshot_hash(&automation), grant)
}

fn enterprise_draft_payload(automation_id: &str, definition_hash: &str) -> Value {
    json!({
        "orchestration_id": "enterprise-orchestration",
        "name": "Enterprise workflow",
        "root_node_id": "workflow",
        "nodes": [
            {
                "node_id": "workflow",
                "name": "Workflow",
                "kind": "workflow",
                "automation_id": automation_id,
                "pinned_definition_hash": definition_hash,
                "allowed_transition_keys": ["complete"]
            },
            {
                "node_id": "done",
                "name": "Done",
                "kind": "terminal",
                "outcome": "complete"
            }
        ],
        "edges": [{
            "edge_id": "workflow-done",
            "from_node_id": "workflow",
            "to_node_id": "done",
            "transition_key": "complete"
        }]
    })
}

fn draft_payload(planner_hash: &str, executor_hash: &str) -> Value {
    json!({
        "orchestration_id": "orch-goals",
        "name": "Plan and execute",
        "root_node_id": "plan",
        "nodes": [
            {
                "node_id": "plan",
                "name": "Plan",
                "kind": "workflow",
                "automation_id": "planner",
                "pinned_definition_hash": planner_hash,
                "allowed_transition_keys": ["continue"],
                "emits_artifact_types": ["plan"]
            },
            {
                "node_id": "execute",
                "name": "Execute",
                "kind": "workflow",
                "automation_id": "executor",
                "pinned_definition_hash": executor_hash,
                "accepts_artifact_types": ["plan"],
                "allowed_transition_keys": ["complete"]
            },
            {
                "node_id": "done",
                "name": "Done",
                "kind": "terminal",
                "outcome": "complete"
            }
        ],
        "edges": [
            {
                "edge_id": "plan-execute",
                "from_node_id": "plan",
                "to_node_id": "execute",
                "transition_key": "continue",
                "artifact_contract": {"artifact_type": "plan", "required": true}
            },
            {
                "edge_id": "execute-done",
                "from_node_id": "execute",
                "to_node_id": "done",
                "transition_key": "complete"
            }
        ],
        "goal_policy": {"max_hops": 5}
    })
}

async fn publish_orchestration(app: &Router, state: &AppState) -> u64 {
    let (planner_hash, executor_hash) = seed_workflows(state).await;
    let (status, _) = dispatch(
        app,
        local_request(
            "POST",
            "/orchestrations",
            Some(draft_payload(&planner_hash, &executor_hash)),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, body) = dispatch(
        app,
        local_request("POST", "/orchestrations/orch-goals/publish", None),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "publish failed: {body}");
    body["version"].as_u64().expect("published version")
}

include!("orchestration_goals_parts/part01.rs");
include!("orchestration_goals_parts/part02.rs");
