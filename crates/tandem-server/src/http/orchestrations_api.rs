//! Tenant-scoped authoring APIs for orchestration drafts and immutable
//! published versions (TAN-694).
//!
//! Drafts live in the store's mutable `version = 0` slot and may be invalid
//! while being edited; validation and referenced-workflow checks gate
//! publishing, which snapshots the draft into the next immutable version.

use super::*;

use tandem_automation::{
    validate_orchestration_spec, GoalPolicy, OrchestrationEdgeSpec, OrchestrationNodeKind,
    OrchestrationNodeSpec, OrchestrationSpec, OrchestrationStatus, OrchestrationValidationIssue,
    OrchestrationValidationReport,
};
use tandem_types::RequestPrincipal;

use crate::stateful_runtime::{
    automation_definition_snapshot_hash, OrchestrationStateStore, DRAFT_CONCURRENCY_CONFLICT,
    ORCHESTRATION_DRAFT_VERSION,
};

const MAX_ORCHESTRATION_LIST_LIMIT: usize = 500;

#[derive(Debug, Deserialize)]
pub(super) struct OrchestrationDraftPayload {
    #[serde(default)]
    pub orchestration_id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub root_node_id: String,
    #[serde(default)]
    pub nodes: Vec<OrchestrationNodeSpec>,
    #[serde(default)]
    pub edges: Vec<OrchestrationEdgeSpec>,
    #[serde(default)]
    pub goal_policy: Option<GoalPolicy>,
    #[serde(default)]
    pub metadata: Option<Value>,
    /// Optimistic concurrency token; required when updating an existing draft.
    #[serde(default)]
    pub expected_updated_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct OrchestrationListQuery {
    pub status: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct OrchestrationDryRunPayload {
    pub from_node_id: String,
    pub transition_key: String,
    #[serde(default)]
    pub artifact_type: Option<String>,
    /// Preview against a published version; defaults to the draft.
    #[serde(default)]
    pub version: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct OrchestrationRefreshPayload {
    pub expected_updated_at_ms: u64,
}

fn definition_store(state: &AppState) -> Result<OrchestrationStateStore, Response> {
    OrchestrationStateStore::from_automation_runs_path(&state.automation_v2_runs_path).map_err(
        |error| {
            tracing::error!(?error, "failed to open orchestration store");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "orchestration_store_unavailable"})),
            )
                .into_response()
        },
    )
}

/// Map store-layer failures to stable HTTP error contracts.
pub(super) fn orchestration_error_response(error: &anyhow::Error) -> Response {
    let message = error.to_string();
    let (status, code) = if message.contains(DRAFT_CONCURRENCY_CONFLICT) {
        (StatusCode::CONFLICT, "draft_concurrency_conflict")
    } else if message.contains("immutable") || message.contains("raced") {
        (StatusCode::CONFLICT, "published_version_conflict")
    } else if message.contains("not found") {
        (StatusCode::NOT_FOUND, "orchestration_not_found")
    } else if message.contains("tenant scope") {
        // Fail closed: cross-tenant access is indistinguishable from absence.
        (StatusCode::NOT_FOUND, "orchestration_not_found")
    } else {
        (StatusCode::BAD_REQUEST, "invalid_orchestration_request")
    };
    (status, Json(json!({"error": code, "detail": message}))).into_response()
}

fn spec_response(spec: &OrchestrationSpec) -> Value {
    json!({
        "orchestration": spec,
        "orchestration_id": spec.orchestration_id,
        "version": spec.version,
        "status": spec.status,
        "updated_at_ms": spec.updated_at_ms,
    })
}

pub(super) async fn create_orchestration_draft(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Json(payload): Json<OrchestrationDraftPayload>,
) -> Response {
    if payload.expected_updated_at_ms.is_some() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "invalid_orchestration_request",
                "detail": "expected_updated_at_ms is only valid on draft updates",
            })),
        )
            .into_response();
    }
    let store = match definition_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let now = crate::util::time::now_ms();
    let orchestration_id = payload
        .orchestration_id
        .clone()
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| format!("orch-{}", uuid::Uuid::new_v4()));
    let spec = draft_spec(&tenant, orchestration_id, &payload, now, now);
    match store.put_orchestration_draft(&spec, None) {
        Ok(()) => (StatusCode::CREATED, Json(spec_response(&spec))).into_response(),
        Err(error) => orchestration_error_response(&error),
    }
}

pub(super) async fn update_orchestration_draft(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(orchestration_id): Path<String>,
    Json(payload): Json<OrchestrationDraftPayload>,
) -> Response {
    let store = match definition_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let existing = match load_tenant_draft(&store, &tenant, &orchestration_id) {
        Ok(existing) => existing,
        Err(response) => return response,
    };
    let Some(expected) = payload.expected_updated_at_ms else {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "draft_concurrency_conflict",
                "detail": "draft updates require expected_updated_at_ms",
                "updated_at_ms": existing.updated_at_ms,
            })),
        )
            .into_response();
    };
    let now = crate::util::time::now_ms();
    let spec = draft_spec(
        &tenant,
        orchestration_id,
        &payload,
        existing.created_at_ms,
        now,
    );
    match store.put_orchestration_draft(&spec, Some(expected)) {
        Ok(()) => Json(spec_response(&spec)).into_response(),
        Err(error) => orchestration_error_response(&error),
    }
}

/// Resource scope for persisted orchestration rows: the caller's actor
/// identity never becomes part of a shared resource's tenant, and a bare
/// local scope normalizes to the canonical local-implicit context.
pub(super) fn resource_scope_tenant(tenant: &TenantContext) -> TenantContext {
    let local = TenantContext::local_implicit();
    if tenant.org_id == local.org_id
        && tenant.workspace_id == local.workspace_id
        && tenant.deployment_id.is_none()
    {
        return local;
    }
    let mut scope = tenant.clone();
    scope.actor_id = None;
    scope
}

fn draft_spec(
    tenant: &TenantContext,
    orchestration_id: String,
    payload: &OrchestrationDraftPayload,
    created_at_ms: u64,
    updated_at_ms: u64,
) -> OrchestrationSpec {
    OrchestrationSpec {
        schema_version: 1,
        orchestration_id,
        name: payload.name.clone(),
        description: payload.description.clone(),
        status: OrchestrationStatus::Draft,
        version: ORCHESTRATION_DRAFT_VERSION,
        root_node_id: payload.root_node_id.clone(),
        nodes: payload.nodes.clone(),
        edges: payload.edges.clone(),
        goal_policy: payload.goal_policy.clone().unwrap_or_default(),
        tenant_context: resource_scope_tenant(tenant),
        created_at_ms,
        updated_at_ms,
        published_at_ms: None,
        metadata: payload.metadata.clone(),
    }
}

fn load_tenant_draft(
    store: &OrchestrationStateStore,
    tenant: &TenantContext,
    orchestration_id: &str,
) -> Result<OrchestrationSpec, Response> {
    match store.get_orchestration_draft(orchestration_id) {
        Ok(Some(draft)) if super::tenant_matches(tenant, &draft.tenant_context) => Ok(draft),
        Ok(_) => Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "orchestration_not_found"})),
        )
            .into_response()),
        Err(error) => Err(orchestration_error_response(&error)),
    }
}

pub(super) async fn list_orchestrations(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Query(query): Query<OrchestrationListQuery>,
) -> Response {
    let store = match definition_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let specs = match store.list_orchestration_specs(&tenant) {
        Ok(specs) => specs,
        Err(error) => return orchestration_error_response(&error),
    };
    let limit = query
        .limit
        .unwrap_or(MAX_ORCHESTRATION_LIST_LIMIT)
        .clamp(1, MAX_ORCHESTRATION_LIST_LIMIT);
    // One summary per orchestration: the draft slot plus published versions.
    let mut summaries = std::collections::BTreeMap::<String, Value>::new();
    for spec in specs {
        let entry = summaries
            .entry(spec.orchestration_id.clone())
            .or_insert_with(|| {
                json!({
                    "orchestration_id": spec.orchestration_id,
                    "name": spec.name,
                    "draft": Value::Null,
                    "latest_published_version": Value::Null,
                    "published_versions": [],
                })
            });
        let object = entry.as_object_mut().expect("summary object");
        object.insert("name".to_string(), json!(spec.name));
        if spec.version == ORCHESTRATION_DRAFT_VERSION {
            object.insert(
                "draft".to_string(),
                json!({
                    "status": spec.status,
                    "updated_at_ms": spec.updated_at_ms,
                }),
            );
        } else {
            object.insert("latest_published_version".to_string(), json!(spec.version));
            object
                .get_mut("published_versions")
                .and_then(Value::as_array_mut)
                .expect("published versions array")
                .push(json!({
                    "version": spec.version,
                    "published_at_ms": spec.published_at_ms,
                }));
        }
    }
    let mut rows = summaries.into_values().collect::<Vec<_>>();
    if let Some(status) = query
        .status
        .as_deref()
        .map(str::trim)
        .filter(|status| !status.is_empty())
    {
        rows.retain(|row| match status {
            "published" => !row["latest_published_version"].is_null(),
            "draft" => row["draft"]["status"] == json!("draft"),
            "archived" => row["draft"]["status"] == json!("archived"),
            _ => true,
        });
    }
    rows.truncate(limit);
    Json(json!({"orchestrations": rows, "count": rows.len()})).into_response()
}

pub(super) async fn get_orchestration(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(orchestration_id): Path<String>,
) -> Response {
    let store = match definition_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let draft = store
        .get_orchestration_draft(&orchestration_id)
        .ok()
        .flatten();
    let latest_published = store
        .latest_published_orchestration_version(&orchestration_id)
        .ok()
        .flatten()
        .and_then(|version| {
            store
                .get_orchestration(&orchestration_id, version)
                .ok()
                .flatten()
        });
    let visible = |spec: &OrchestrationSpec| super::tenant_matches(&tenant, &spec.tenant_context);
    let draft = draft.filter(visible);
    let latest_published = latest_published.filter(visible);
    if draft.is_none() && latest_published.is_none() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "orchestration_not_found"})),
        )
            .into_response();
    }
    Json(json!({
        "orchestration_id": orchestration_id,
        "draft": draft,
        "latest_published": latest_published,
    }))
    .into_response()
}

pub(super) async fn archive_orchestration_draft(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(orchestration_id): Path<String>,
) -> Response {
    let store = match definition_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    match store.archive_orchestration_draft(&tenant, &orchestration_id, crate::util::time::now_ms())
    {
        Ok(spec) => Json(spec_response(&spec)).into_response(),
        Err(error) => orchestration_error_response(&error),
    }
}

pub(super) async fn list_orchestration_versions(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(orchestration_id): Path<String>,
) -> Response {
    let store = match definition_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    match store.list_orchestration_versions(&tenant, &orchestration_id) {
        Ok(versions) => Json(json!({
            "orchestration_id": orchestration_id,
            "versions": versions
                .iter()
                .map(|spec| json!({
                    "version": spec.version,
                    "name": spec.name,
                    "published_at_ms": spec.published_at_ms,
                    "metadata": spec.metadata,
                }))
                .collect::<Vec<_>>(),
            "count": versions.len(),
        }))
        .into_response(),
        Err(error) => orchestration_error_response(&error),
    }
}

pub(super) async fn get_orchestration_version(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path((orchestration_id, version)): Path<(String, u64)>,
) -> Response {
    let store = match definition_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    match store.get_orchestration(&orchestration_id, version) {
        Ok(Some(spec)) if super::tenant_matches(&tenant, &spec.tenant_context) => {
            Json(spec_response(&spec)).into_response()
        }
        Ok(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "orchestration_not_found"})),
        )
            .into_response(),
        Err(error) => orchestration_error_response(&error),
    }
}

/// Graph validation plus referenced-workflow checks: existence, tenant scope
/// (fail closed), and pinned-hash freshness. The report is node/edge
/// addressed so the visual canvas can badge the affected graph items.
pub(super) async fn validate_orchestration(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(orchestration_id): Path<String>,
) -> Response {
    let store = match definition_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let draft = match load_tenant_draft(&store, &tenant, &orchestration_id) {
        Ok(draft) => draft,
        Err(response) => return response,
    };
    let report = full_validation_report(&state, &tenant, &draft).await;
    Json(json!({
        "orchestration_id": orchestration_id,
        "version": draft.version,
        "report": report.report,
        "stale_references": report.stale_references,
    }))
    .into_response()
}

pub(super) async fn orchestration_stale_references(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(orchestration_id): Path<String>,
) -> Response {
    let store = match definition_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let draft = match load_tenant_draft(&store, &tenant, &orchestration_id) {
        Ok(draft) => draft,
        Err(response) => return response,
    };
    let references = workflow_reference_states(&state, &tenant, &draft).await;
    Json(json!({
        "orchestration_id": orchestration_id,
        "references": references,
        "stale_count": references
            .iter()
            .filter(|reference| reference["state"] == json!("stale"))
            .count(),
    }))
    .into_response()
}

/// Rewrite every workflow node's pinned hash to the current definition hash.
/// This is the explicit "refresh" step that unblocks publishing after a
/// referenced workflow changed.
pub(super) async fn refresh_orchestration_references(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(orchestration_id): Path<String>,
    Json(payload): Json<OrchestrationRefreshPayload>,
) -> Response {
    let store = match definition_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let mut draft = match load_tenant_draft(&store, &tenant, &orchestration_id) {
        Ok(draft) => draft,
        Err(response) => return response,
    };
    let expected = payload.expected_updated_at_ms;
    let mut refreshed = Vec::new();
    for node in &mut draft.nodes {
        let node_id = node.node_id.clone();
        if let OrchestrationNodeKind::Workflow {
            automation_id,
            pinned_definition_hash,
            ..
        } = &mut node.node
        {
            let Some(automation) = state.get_automation_v2(automation_id).await else {
                continue;
            };
            if !super::tenant_matches(&tenant, &automation.tenant_context()) {
                continue;
            }
            let current = automation_definition_snapshot_hash(&automation);
            if pinned_definition_hash.as_deref() != Some(current.as_str()) {
                *pinned_definition_hash = Some(current);
                refreshed.push(node_id);
            }
        }
    }
    draft.updated_at_ms = crate::util::time::now_ms();
    match store.put_orchestration_draft(&draft, Some(expected)) {
        Ok(()) => Json(json!({
            "orchestration": draft,
            "refreshed_node_ids": refreshed,
        }))
        .into_response(),
        Err(error) => orchestration_error_response(&error),
    }
}

pub(super) async fn publish_orchestration(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(principal): Extension<RequestPrincipal>,
    Path(orchestration_id): Path<String>,
) -> Response {
    let store = match definition_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let draft = match load_tenant_draft(&store, &tenant, &orchestration_id) {
        Ok(draft) => draft,
        Err(response) => return response,
    };
    if draft.status == OrchestrationStatus::Archived {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "orchestration_archived",
                "detail": "archived drafts cannot be published",
            })),
        )
            .into_response();
    }
    let now = crate::util::time::now_ms();
    let next_version = store
        .latest_published_orchestration_version(&orchestration_id)
        .ok()
        .flatten()
        .unwrap_or(0)
        .saturating_add(1);

    let mut candidate = draft.clone();
    candidate.status = OrchestrationStatus::Published;
    candidate.version = next_version;
    candidate.published_at_ms = Some(now);
    candidate.updated_at_ms = now;

    let validation = full_validation_report(&state, &tenant, &candidate).await;
    if !validation.report.valid || !validation.stale_references.is_empty() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "error": "orchestration_invalid",
                "report": validation.report,
                "stale_references": validation.stale_references,
                "detail": "publishing requires a valid graph and refreshed workflow references",
            })),
        )
            .into_response();
    }

    // The published snapshot records who published it, the validation report,
    // and the exact referenced definition hashes at publish time.
    let mut metadata = candidate
        .metadata
        .take()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    metadata.insert(
        "publish".to_string(),
        json!({
            "actor": principal,
            "published_at_ms": now,
            "validation": validation.report,
            "workflow_definition_hashes": validation.workflow_hashes,
        }),
    );
    candidate.metadata = Some(Value::Object(metadata));

    match store.publish_orchestration_draft(&candidate) {
        Ok(()) => (StatusCode::CREATED, Json(spec_response(&candidate))).into_response(),
        Err(error) => orchestration_error_response(&error),
    }
}

/// Pure transition preview: which edge fires, where it leads, and what the
/// artifact/approval contracts would demand — without touching any state.
pub(super) async fn dry_run_orchestration_transition(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Path(orchestration_id): Path<String>,
    Json(payload): Json<OrchestrationDryRunPayload>,
) -> Response {
    let store = match definition_store(&state) {
        Ok(store) => store,
        Err(response) => return response,
    };
    let spec = match payload.version {
        Some(version) => store
            .get_orchestration(&orchestration_id, version)
            .ok()
            .flatten(),
        None => store
            .get_orchestration_draft(&orchestration_id)
            .ok()
            .flatten(),
    };
    let Some(spec) = spec.filter(|spec| super::tenant_matches(&tenant, &spec.tenant_context))
    else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "orchestration_not_found"})),
        )
            .into_response();
    };
    let source = spec
        .nodes
        .iter()
        .find(|node| node.node_id == payload.from_node_id);
    let edge = spec.edges.iter().find(|edge| {
        edge.from_node_id == payload.from_node_id && edge.transition_key == payload.transition_key
    });
    let mut issues: Vec<Value> = Vec::new();
    if source.is_none() {
        issues.push(json!({"code": "unknown_source_node", "node_id": payload.from_node_id}));
    }
    if let Some(OrchestrationNodeSpec {
        node:
            OrchestrationNodeKind::Workflow {
                allowed_transition_keys,
                ..
            },
        ..
    }) = source
    {
        if !allowed_transition_keys.is_empty()
            && !allowed_transition_keys.contains(&payload.transition_key)
        {
            issues.push(json!({
                "code": "transition_key_not_allowed",
                "node_id": payload.from_node_id,
                "transition_key": payload.transition_key,
            }));
        }
    }
    let Some(edge) = edge else {
        issues.push(json!({
            "code": "no_matching_edge",
            "node_id": payload.from_node_id,
            "transition_key": payload.transition_key,
        }));
        return Json(json!({"allowed": false, "issues": issues})).into_response();
    };
    let target = spec
        .nodes
        .iter()
        .find(|node| node.node_id == edge.to_node_id);
    if let (Some(contract), Some(artifact_type)) = (
        edge.artifact_contract.as_ref(),
        payload.artifact_type.as_deref(),
    ) {
        if contract.artifact_type != artifact_type {
            issues.push(json!({
                "code": "artifact_type_mismatch",
                "edge_id": edge.edge_id,
                "expected": contract.artifact_type,
                "provided": artifact_type,
            }));
        }
    } else if edge
        .artifact_contract
        .as_ref()
        .is_some_and(|contract| contract.required)
        && payload.artifact_type.is_none()
    {
        issues.push(json!({
            "code": "artifact_required",
            "edge_id": edge.edge_id,
        }));
    }
    Json(json!({
        "allowed": issues.is_empty(),
        "issues": issues,
        "edge": {
            "edge_id": edge.edge_id,
            "transition_key": edge.transition_key,
            "artifact_contract": edge.artifact_contract,
            "approval_required": edge.approval.as_ref().is_some_and(|approval| approval.required),
        },
        "target": target.map(|node| json!({
            "node_id": node.node_id,
            "name": node.name,
            "kind": node.node,
        })),
    }))
    .into_response()
}

pub(super) struct FullValidation {
    pub report: OrchestrationValidationReport,
    pub stale_references: Vec<Value>,
    pub workflow_hashes: Value,
}

/// Graph validation + referenced-definition checks. Missing and cross-tenant
/// workflows are hard validation errors (fail closed); stale pinned hashes
/// are reported separately so drafts can warn while publish blocks.
pub(super) async fn full_validation_report(
    state: &AppState,
    tenant: &TenantContext,
    spec: &OrchestrationSpec,
) -> FullValidation {
    let mut report = validate_orchestration_spec(spec);
    let references = workflow_reference_states(state, tenant, spec).await;
    let mut stale = Vec::new();
    let mut hashes = serde_json::Map::new();
    for reference in &references {
        let node_id = reference["node_id"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        match reference["state"].as_str().unwrap_or_default() {
            "missing" => {
                report.valid = false;
                report.issues.push(OrchestrationValidationIssue {
                    code: "missing_workflow".to_string(),
                    message: format!(
                        "workflow {} referenced by node {node_id} does not exist in this tenant",
                        reference["automation_id"].as_str().unwrap_or_default()
                    ),
                    node_id: Some(node_id),
                    edge_id: None,
                });
            }
            "stale" => stale.push(reference.clone()),
            _ => {}
        }
        if let (Some(automation_id), Some(hash)) = (
            reference["automation_id"].as_str(),
            reference["current_hash"].as_str(),
        ) {
            hashes.insert(automation_id.to_string(), json!(hash));
        }
    }
    FullValidation {
        report,
        stale_references: stale,
        workflow_hashes: Value::Object(hashes),
    }
}

async fn workflow_reference_states(
    state: &AppState,
    tenant: &TenantContext,
    spec: &OrchestrationSpec,
) -> Vec<Value> {
    let mut references = Vec::new();
    for node in &spec.nodes {
        let OrchestrationNodeKind::Workflow {
            automation_id,
            pinned_definition_hash,
            ..
        } = &node.node
        else {
            continue;
        };
        let automation = state.get_automation_v2(automation_id).await;
        // Cross-tenant definitions are reported as missing: fail closed.
        let automation = automation
            .filter(|automation| super::tenant_matches(tenant, &automation.tenant_context()));
        let Some(automation) = automation else {
            references.push(json!({
                "node_id": node.node_id,
                "automation_id": automation_id,
                "state": "missing",
            }));
            continue;
        };
        let current = automation_definition_snapshot_hash(&automation);
        let reference_state = match pinned_definition_hash.as_deref() {
            None => "unpinned",
            Some(pinned) if pinned == current => "fresh",
            Some(_) => "stale",
        };
        references.push(json!({
            "node_id": node.node_id,
            "automation_id": automation_id,
            "state": reference_state,
            "pinned_hash": pinned_definition_hash,
            "current_hash": current,
        }));
    }
    references
}
