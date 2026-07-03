use super::*;

use crate::stateful_runtime::{
    list_stateful_run_snapshots as list_snapshot_records, list_stateful_waits,
    load_stateful_run_events, query_stateful_run_events, read_stateful_run_snapshot_for_run,
    stateful_run_from_automation_v2, stateful_run_from_workflow, StatefulRunEventQuery,
    StatefulRunEventRecord, StatefulRuntimeStoragePaths, StatefulWaitQuery,
    StatefulWorkflowRunKind, StatefulWorkflowRunRecord,
};
use tandem_enterprise_contract::{canonical_enterprise_scope_id, enterprise_scope_ids_match};
use tandem_types::{
    AccessEffect, OrganizationUnit, OrganizationUnitAccessGrant, ResourceRef, ResourceScope,
    SourceBinding,
};

const DEFAULT_STATEFUL_RUNTIME_LIMIT: usize = 250;
const MAX_STATEFUL_RUNTIME_LIMIT: usize = 1_000;
const MAX_SCOPE_SOURCE_BINDINGS: usize = 12;

#[derive(Debug, Deserialize, Default)]
pub(super) struct StatefulRunEventsQuery {
    pub after_seq: Option<u64>,
    pub since_seq: Option<u64>,
    pub before_seq: Option<u64>,
    pub limit: Option<usize>,
    pub tail: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct StatefulRunSnapshotsQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct StatefulRunsQuery {
    pub limit: Option<usize>,
    pub status: Option<String>,
    pub phase: Option<String>,
    pub trigger: Option<String>,
    pub kind: Option<String>,
    pub source: Option<String>,
    pub org_id: Option<String>,
    pub workspace_id: Option<String>,
    pub deployment_id: Option<String>,
    pub workflow_id: Option<String>,
    pub automation_id: Option<String>,
    pub org_unit_id: Option<String>,
    pub owner_id: Option<String>,
    pub owner_kind: Option<String>,
    pub resource_kind: Option<String>,
    pub resource_id: Option<String>,
    pub policy_version_id: Option<String>,
    pub data_class: Option<String>,
    pub risk_tier: Option<String>,
    pub delegation_grant_id: Option<String>,
    pub source_binding_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct StatefulRunDetailQuery {
    pub event_limit: Option<usize>,
    pub snapshot_limit: Option<usize>,
}

pub(super) async fn list_stateful_runs(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Query(query): Query<StatefulRunsQuery>,
) -> Json<Value> {
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    let limit = query
        .limit
        .unwrap_or(DEFAULT_STATEFUL_RUNTIME_LIMIT)
        .clamp(1, MAX_STATEFUL_RUNTIME_LIMIT);
    let enterprise_catalog = EnterpriseScopeCatalog::from_state(&state).await;
    let mut rows = collect_stateful_runs(&state, &tenant_context).await;
    rows.retain(|run| {
        run_matches_query(run, &query)
            && run_matches_enterprise_query(run, &query, &enterprise_catalog)
    });
    rows.sort_by(|left, right| {
        right
            .updated_at_ms
            .cmp(&left.updated_at_ms)
            .then_with(|| left.run_id.cmp(&right.run_id))
    });
    rows.truncate(limit);
    let event_summaries =
        stateful_run_event_summaries_by_run(&paths.run_events_path, &tenant_context);
    let runs = rows
        .into_iter()
        .map(|run| {
            stateful_run_response(
                &paths,
                &tenant_context,
                &enterprise_catalog,
                run,
                false,
                Some(&event_summaries),
            )
        })
        .collect::<Vec<_>>();
    let count = runs.len();

    Json(json!({
        "runs": runs,
        "count": count,
        "limit": limit,
        "filters": {
            "status": query.status,
            "phase": query.phase,
            "trigger": query.trigger,
            "kind": query.kind.or(query.source),
            "org_id": query.org_id,
            "workspace_id": query.workspace_id,
            "deployment_id": query.deployment_id,
            "workflow_id": query.workflow_id,
            "automation_id": query.automation_id,
            "org_unit_id": query.org_unit_id,
            "owner_id": query.owner_id,
            "owner_kind": query.owner_kind,
            "resource_kind": query.resource_kind,
            "resource_id": query.resource_id,
            "policy_version_id": query.policy_version_id,
            "data_class": query.data_class,
            "risk_tier": query.risk_tier,
            "delegation_grant_id": query.delegation_grant_id,
            "source_binding_id": query.source_binding_id,
        },
        "source": "stateful_runtime",
    }))
}

pub(super) async fn get_stateful_run(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(run_id): Path<String>,
    Query(query): Query<StatefulRunDetailQuery>,
) -> Response {
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    let Some(run) = collect_stateful_runs(&state, &tenant_context)
        .await
        .into_iter()
        .find(|run| run.run_id == run_id)
    else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "stateful_run_not_found",
                "run_id": run_id,
            })),
        )
            .into_response();
    };

    let event_limit = query
        .event_limit
        .unwrap_or(50)
        .clamp(1, MAX_STATEFUL_RUNTIME_LIMIT);
    let snapshot_limit = query
        .snapshot_limit
        .unwrap_or(10)
        .clamp(1, MAX_STATEFUL_RUNTIME_LIMIT);
    let events = query_stateful_run_events(
        &paths.run_events_path,
        &tenant_context,
        StatefulRunEventQuery {
            run_id: &run_id,
            after_seq: None,
            before_seq: None,
            limit: Some(event_limit),
            tail: true,
        },
    );
    let snapshots = list_snapshot_records(
        &paths.snapshots_root,
        &tenant_context,
        &run_id,
        Some(snapshot_limit),
    );
    let enterprise_catalog = EnterpriseScopeCatalog::from_state(&state).await;
    let mut body = stateful_run_response(
        &paths,
        &tenant_context,
        &enterprise_catalog,
        run,
        true,
        None,
    );
    if let Some(object) = body.as_object_mut() {
        object.insert("events".to_string(), json!(events));
        object.insert("snapshots".to_string(), json!(snapshots));
        object.insert("event_source".to_string(), json!("stateful_runtime"));
        object.insert(
            "event_authority".to_string(),
            json!("authoritative_runtime_log"),
        );
    }

    Json(body).into_response()
}

pub(super) async fn get_stateful_run_events(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(run_id): Path<String>,
    Query(query): Query<StatefulRunEventsQuery>,
) -> Json<Value> {
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    let limit = query
        .tail
        .or(query.limit)
        .unwrap_or(DEFAULT_STATEFUL_RUNTIME_LIMIT)
        .clamp(1, MAX_STATEFUL_RUNTIME_LIMIT);
    let tail = query.tail.is_some();
    let rows = query_stateful_run_events(
        &paths.run_events_path,
        &tenant_context,
        StatefulRunEventQuery {
            run_id: &run_id,
            after_seq: query.after_seq.or(query.since_seq),
            before_seq: query.before_seq,
            limit: Some(limit),
            tail,
        },
    );
    let last_seq = rows.last().map(|row| row.seq);
    let count = rows.len();

    Json(json!({
        "run_id": run_id,
        "events": rows,
        "count": count,
        "last_seq": last_seq,
        "limit": limit,
        "sequence_scope": "stateful_runtime",
        "event_source": "stateful_runtime",
        "event_authority": "authoritative_runtime_log",
    }))
}

pub(super) async fn list_stateful_run_snapshots(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(run_id): Path<String>,
    Query(query): Query<StatefulRunSnapshotsQuery>,
) -> Json<Value> {
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    let limit = query
        .limit
        .unwrap_or(DEFAULT_STATEFUL_RUNTIME_LIMIT)
        .clamp(1, MAX_STATEFUL_RUNTIME_LIMIT);
    let snapshots =
        list_snapshot_records(&paths.snapshots_root, &tenant_context, &run_id, Some(limit));
    let latest_seq = snapshots.last().map(|snapshot| snapshot.seq);
    let count = snapshots.len();

    Json(json!({
        "run_id": run_id,
        "snapshots": snapshots,
        "count": count,
        "latest_seq": latest_seq,
        "limit": limit,
    }))
}

pub(super) async fn get_stateful_run_snapshot(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path((run_id, snapshot_id)): Path<(String, String)>,
) -> Response {
    let paths = StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
    match read_stateful_run_snapshot_for_run(
        &paths.snapshots_root,
        &tenant_context,
        &run_id,
        &snapshot_id,
    ) {
        Ok(Some(snapshot)) => Json(json!({ "snapshot": snapshot })).into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "stateful_snapshot_not_found",
                "run_id": run_id,
                "snapshot_id": snapshot_id,
            })),
        )
            .into_response(),
        Err(error) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "stateful_snapshot_read_failed",
                "message": error.to_string(),
            })),
        )
            .into_response(),
    }
}

async fn collect_stateful_runs(
    state: &AppState,
    tenant_context: &TenantContext,
) -> Vec<StatefulWorkflowRunRecord> {
    let mut by_run_id = HashMap::<String, StatefulWorkflowRunRecord>::new();
    let automation_runs = state.automation_v2_runs.read().await;
    for run in automation_runs
        .values()
        .map(stateful_run_from_automation_v2)
    {
        insert_visible_stateful_run(&mut by_run_id, tenant_context, run);
    }
    drop(automation_runs);

    let workflow_runs = state.workflow_runs.read().await;
    for run in workflow_runs.values().map(stateful_run_from_workflow) {
        insert_visible_stateful_run(&mut by_run_id, tenant_context, run);
    }

    by_run_id.into_values().collect()
}

fn insert_visible_stateful_run(
    by_run_id: &mut HashMap<String, StatefulWorkflowRunRecord>,
    tenant_context: &TenantContext,
    run: StatefulWorkflowRunRecord,
) {
    if !run.scope.visible_to_tenant(tenant_context) {
        return;
    }
    match by_run_id.get(&run.run_id) {
        Some(existing) if existing.updated_at_ms > run.updated_at_ms => {}
        _ => {
            by_run_id.insert(run.run_id.clone(), run);
        }
    }
}

#[derive(Default)]
struct EnterpriseScopeCatalog {
    org_units: Vec<OrganizationUnit>,
    org_unit_access_grants: Vec<OrganizationUnitAccessGrant>,
    source_bindings: Vec<SourceBinding>,
}

impl EnterpriseScopeCatalog {
    async fn from_state(state: &AppState) -> Self {
        let org_units = state
            .enterprise
            .org_units
            .read()
            .await
            .values()
            .cloned()
            .collect();
        let org_unit_access_grants = state
            .enterprise
            .org_unit_access_grants
            .read()
            .await
            .values()
            .cloned()
            .collect();
        let source_bindings = state
            .enterprise
            .source_bindings
            .read()
            .await
            .values()
            .cloned()
            .collect();
        Self {
            org_units,
            org_unit_access_grants,
            source_bindings,
        }
    }
}

struct StatefulRunEventListSummary {
    first_event_seq: u64,
    latest_event: StatefulRunEventRecord,
}

fn stateful_run_event_summaries_by_run(
    path: &std::path::Path,
    tenant_context: &TenantContext,
) -> HashMap<String, StatefulRunEventListSummary> {
    let mut summaries = HashMap::<String, StatefulRunEventListSummary>::new();
    for event in load_stateful_run_events(path)
        .into_iter()
        .filter(|event| event.visible_to_tenant(tenant_context))
    {
        match summaries.get_mut(&event.run_id) {
            Some(summary) => {
                summary.first_event_seq = summary.first_event_seq.min(event.seq);
                if event.seq >= summary.latest_event.seq {
                    summary.latest_event = event;
                }
            }
            None => {
                summaries.insert(
                    event.run_id.clone(),
                    StatefulRunEventListSummary {
                        first_event_seq: event.seq,
                        latest_event: event,
                    },
                );
            }
        }
    }
    summaries
}

fn stateful_run_response(
    paths: &StatefulRuntimeStoragePaths,
    tenant_context: &TenantContext,
    enterprise_catalog: &EnterpriseScopeCatalog,
    mut run: StatefulWorkflowRunRecord,
    include_details: bool,
    event_summaries: Option<&HashMap<String, StatefulRunEventListSummary>>,
) -> Value {
    let latest_snapshot =
        list_snapshot_records(&paths.snapshots_root, tenant_context, &run.run_id, Some(1))
            .into_iter()
            .next();
    if run.latest_snapshot_id.is_none() {
        run.latest_snapshot_id = latest_snapshot
            .as_ref()
            .map(|snapshot| snapshot.snapshot_id.clone());
    }
    let current_wait = current_wait_for_run(paths, tenant_context, &run.run_id);
    let (latest_event, first_event_seq, latest_event_seq) = match event_summaries {
        Some(summaries) => summaries
            .get(&run.run_id)
            .map(|summary| {
                (
                    Some(stateful_event_summary(&summary.latest_event)),
                    Some(summary.first_event_seq),
                    Some(summary.latest_event.seq),
                )
            })
            .unwrap_or((None, None, None)),
        None => {
            let events = query_stateful_run_events(
                &paths.run_events_path,
                tenant_context,
                StatefulRunEventQuery {
                    run_id: &run.run_id,
                    after_seq: None,
                    before_seq: None,
                    limit: None,
                    tail: false,
                },
            );
            (
                events.last().map(stateful_event_summary),
                events.first().map(|event| event.seq),
                events.last().map(|event| event.seq),
            )
        }
    };
    let latest_snapshot_summary = latest_snapshot.as_ref().map(stateful_snapshot_summary);
    let enterprise_scope = stateful_enterprise_scope_summary(enterprise_catalog, &run);
    let replay_boundaries = json!({
        "earliest_event_seq": first_event_seq,
        "latest_event_seq": latest_event_seq,
        "latest_snapshot_id": latest_snapshot.as_ref().map(|snapshot| snapshot.snapshot_id.as_str()),
        "latest_snapshot_seq": latest_snapshot.as_ref().map(|snapshot| snapshot.seq),
        "can_replay_from_event_log": latest_event_seq.is_some(),
        "can_replay_from_snapshot": latest_snapshot.is_some(),
    });

    json!({
        "run": run,
        "current_wait": current_wait,
        "latest_event": latest_event,
        "latest_snapshot": latest_snapshot_summary,
        "enterprise_scope": enterprise_scope,
        "replay_boundaries": replay_boundaries,
        "event_source": "stateful_runtime",
        "event_authority": "authoritative_runtime_log",
        "detail_level": if include_details { "detail" } else { "list" },
    })
}

fn stateful_enterprise_scope_summary(
    catalog: &EnterpriseScopeCatalog,
    run: &StatefulWorkflowRunRecord,
) -> Value {
    let scope = &run.scope;
    let root_resource = scope.resource_scope.as_ref().map(|scope| &scope.root);
    let org_unit = scope
        .owning_org_unit_id
        .as_deref()
        .and_then(|unit_id| organization_unit_for_scope(catalog, scope, unit_id))
        .map(organization_unit_summary);
    let org_unit_label = org_unit
        .as_ref()
        .and_then(|unit| unit.get("display_name"))
        .and_then(Value::as_str)
        .or(scope.owning_org_unit_id.as_deref())
        .map(ToOwned::to_owned);
    let org_unit_grants = active_org_unit_grants_for_scope(catalog, scope)
        .into_iter()
        .map(org_unit_grant_summary)
        .collect::<Vec<_>>();
    let missing_delegation_grant_ids = missing_delegation_grant_ids(catalog, scope);
    let delegation_grant_authority_status = if scope.delegation_grant_ids.is_empty() {
        "not_required"
    } else if missing_delegation_grant_ids.is_empty() {
        "active"
    } else {
        "invalid"
    };
    let visible_sources = source_bindings_for_scope(catalog, scope)
        .into_iter()
        .take(MAX_SCOPE_SOURCE_BINDINGS)
        .map(source_binding_summary)
        .collect::<Vec<_>>();
    let source_count = visible_sources.len();
    let grant_count = org_unit_grants.len();

    json!({
        "tenant_context": &scope.tenant_context,
        "owning_org_unit_id": &scope.owning_org_unit_id,
        "owning_org_unit": org_unit,
        "owner_principal": &scope.owner_principal,
        "resource_scope": &scope.resource_scope,
        "resource_kind": root_resource.map(|resource| &resource.resource_kind),
        "resource_id": root_resource.map(|resource| resource.resource_id.as_str()),
        "data_classes": &scope.data_classes,
        "risk_tier": &scope.risk_tier,
        "policy_version_id": &scope.policy_version_id,
        "delegation_grant_ids": &scope.delegation_grant_ids,
        "delegation_grant_authority": {
            "status": delegation_grant_authority_status,
            "missing_grant_ids": missing_delegation_grant_ids,
        },
        "org_unit_grants": org_unit_grants,
        "visible_knowledge_sources": visible_sources,
        "summary": {
            "org_unit": org_unit_label,
            "resource": root_resource.map(resource_ref_label),
            "knowledge_source_count": source_count,
            "org_unit_grant_count": grant_count,
            "data_class_count": scope.data_classes.len(),
            "delegation_grant_count": scope.delegation_grant_ids.len(),
        },
    })
}

fn organization_unit_for_scope<'a>(
    catalog: &'a EnterpriseScopeCatalog,
    scope: &crate::stateful_runtime::StatefulRuntimeScope,
    unit_id: &str,
) -> Option<&'a OrganizationUnit> {
    catalog.org_units.iter().find(|unit| {
        tenant_matches(&unit.tenant_context, &scope.tenant_context)
            && organization_unit_id_matches(unit, unit_id)
    })
}

fn organization_unit_id_matches(unit: &OrganizationUnit, unit_id: &str) -> bool {
    enterprise_scope_ids_match(&unit.unit_id, unit_id)
        || enterprise_scope_ids_match(&format!("{}/{}", unit.taxonomy_id, unit.unit_id), unit_id)
}

fn organization_unit_summary(unit: &OrganizationUnit) -> Value {
    json!({
        "unit_id": &unit.unit_id,
        "taxonomy_id": &unit.taxonomy_id,
        "display_name": &unit.display_name,
        "kind": &unit.kind,
        "parent_unit_id": &unit.parent_unit_id,
        "state": &unit.state,
        "labels": &unit.labels,
    })
}

fn active_org_unit_grants_for_scope<'a>(
    catalog: &'a EnterpriseScopeCatalog,
    scope: &crate::stateful_runtime::StatefulRuntimeScope,
) -> Vec<&'a OrganizationUnitAccessGrant> {
    if scope.owning_org_unit_id.is_none() && scope.delegation_grant_ids.is_empty() {
        return Vec::new();
    }
    let now = crate::util::time::now_ms();
    let mut grants = catalog
        .org_unit_access_grants
        .iter()
        .filter(|grant| {
            tenant_matches(&grant.tenant_context, &scope.tenant_context)
                && grant.effect == AccessEffect::Allow
                && scope
                    .owning_org_unit_id
                    .as_deref()
                    .map(|org_unit_id| principal_matches_org_unit_id(&grant.unit, org_unit_id))
                    .unwrap_or(true)
                && grant.is_active_at(now)
                && delegation_grant_ids_authorize_scope(scope, grant)
                && scope
                    .resource_scope
                    .as_ref()
                    .map(|resource_scope| {
                        resource_ref_matches_scope(&grant.resource, resource_scope)
                    })
                    .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    grants.sort_by(|left, right| left.grant_id.cmp(&right.grant_id));
    grants
}

fn delegation_grant_ids_authorize_scope(
    scope: &crate::stateful_runtime::StatefulRuntimeScope,
    grant: &OrganizationUnitAccessGrant,
) -> bool {
    scope.delegation_grant_ids.is_empty()
        || scope
            .delegation_grant_ids
            .iter()
            .any(|grant_id| delegation_grant_id_matches(grant_id, &grant.grant_id))
}

fn missing_delegation_grant_ids(
    catalog: &EnterpriseScopeCatalog,
    scope: &crate::stateful_runtime::StatefulRuntimeScope,
) -> Vec<String> {
    scope
        .delegation_grant_ids
        .iter()
        .filter(|grant_id| {
            !active_org_unit_grants_for_scope(catalog, scope)
                .iter()
                .any(|grant| delegation_grant_id_matches(grant_id, &grant.grant_id))
        })
        .cloned()
        .collect()
}

fn delegation_grant_id_matches(left: &str, right: &str) -> bool {
    normalize_filter_value(left) == normalize_filter_value(right)
}

fn principal_matches_org_unit_id(
    principal: &tandem_types::PrincipalRef,
    org_unit_id: &str,
) -> bool {
    let Some(principal_id) = canonical_enterprise_scope_id(&principal.id) else {
        return false;
    };
    let Some(org_unit_id) = canonical_enterprise_scope_id(org_unit_id) else {
        return false;
    };
    principal_id == org_unit_id || principal_id.ends_with(&format!("/{org_unit_id}"))
}

fn org_unit_grant_summary(grant: &OrganizationUnitAccessGrant) -> Value {
    json!({
        "grant_id": &grant.grant_id,
        "unit": &grant.unit,
        "resource": &grant.resource,
        "effect": &grant.effect,
        "permissions": &grant.permissions,
        "data_classes": &grant.data_classes,
        "tool_patterns": &grant.tool_patterns,
        "state": &grant.state,
        "expires_at_ms": grant.expires_at_ms,
    })
}

fn source_bindings_for_scope<'a>(
    catalog: &'a EnterpriseScopeCatalog,
    scope: &crate::stateful_runtime::StatefulRuntimeScope,
) -> Vec<&'a SourceBinding> {
    let mut bindings = catalog
        .source_bindings
        .iter()
        .filter(|binding| {
            binding.tenant_matches(&scope.tenant_context)
                && binding.state.allows_ingestion()
                && binding.ingestion_policy.allow_prompt_context
                && scope
                    .resource_scope
                    .as_ref()
                    .map(|resource_scope| {
                        resource_ref_matches_scope(&binding.resource_ref, resource_scope)
                    })
                    .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    bindings.sort_by(|left, right| left.binding_id.cmp(&right.binding_id));
    bindings
}

fn resource_ref_matches_scope(resource: &ResourceRef, scope: &ResourceScope) -> bool {
    scope.contains(resource) || resource.applies_to(&scope.root)
}

fn source_binding_summary(binding: &SourceBinding) -> Value {
    json!({
        "binding_id": &binding.binding_id,
        "connector_id": &binding.connector_id,
        "source_type": &binding.source_type,
        "native_source_id": &binding.native_source_id,
        "source_root_label": &binding.source_root_label,
        "resource_ref": &binding.resource_ref,
        "data_class": &binding.data_class,
        "state": &binding.state,
        "ingestion_policy": &binding.ingestion_policy,
    })
}

fn resource_ref_label(resource: &ResourceRef) -> String {
    format!(
        "{}:{}",
        serialized_key(&resource.resource_kind),
        resource.resource_id
    )
}

fn current_wait_for_run(
    paths: &StatefulRuntimeStoragePaths,
    tenant_context: &TenantContext,
    run_id: &str,
) -> Option<Value> {
    let waits = list_stateful_waits(
        &paths.waits_path,
        tenant_context,
        StatefulWaitQuery {
            run_id: Some(run_id),
            wait_kind: None,
            status: None,
            limit: None,
        },
    );
    waits
        .iter()
        .find(|wait| !wait.status.is_terminal())
        .or_else(|| waits.last())
        .map(|wait| {
            json!({
                "wait_id": &wait.wait_id,
                "wait_kind": &wait.wait_kind,
                "status": &wait.status,
                "phase_id": &wait.phase_id,
                "reason": &wait.reason,
                "wake_at_ms": wait.wake_at_ms,
                "timeout_policy": &wait.timeout_policy,
                "event_seq": wait.event_seq,
            })
        })
}

fn stateful_event_summary(event: &crate::stateful_runtime::StatefulRunEventRecord) -> Value {
    json!({
        "event_id": &event.event_id,
        "seq": event.seq,
        "event_type": &event.event_type,
        "occurred_at_ms": event.occurred_at_ms,
        "phase_id": &event.phase_id,
        "wait_kind": &event.wait_kind,
        "authoritative": true,
    })
}

fn stateful_snapshot_summary(
    snapshot: &crate::stateful_runtime::StatefulRunSnapshotRecord,
) -> Value {
    json!({
        "snapshot_id": &snapshot.snapshot_id,
        "seq": snapshot.seq,
        "created_at_ms": snapshot.created_at_ms,
        "status": &snapshot.status,
        "phase": &snapshot.phase,
        "phase_id": &snapshot.phase_id,
        "payload_digest": &snapshot.payload_digest,
        "workflow_definition_version": &snapshot.workflow_definition_version,
        "workflow_definition_snapshot_hash": &snapshot.workflow_definition_snapshot_hash,
    })
}

fn run_matches_query(run: &StatefulWorkflowRunRecord, query: &StatefulRunsQuery) -> bool {
    string_filter_matches(query.status.as_deref(), &serialized_key(&run.status))
        && string_filter_matches(query.phase.as_deref(), &serialized_key(&run.phase))
        && string_filter_matches(query.org_id.as_deref(), run.scope.organization_id())
        && string_filter_matches(query.workspace_id.as_deref(), run.scope.workspace_id())
        && option_filter_matches(query.deployment_id.as_deref(), run.scope.deployment_id())
        && option_filter_matches(query.workflow_id.as_deref(), run.workflow_id.as_deref())
        && option_filter_matches(query.automation_id.as_deref(), run.automation_id.as_deref())
        && trigger_filter_matches(run, query.trigger.as_deref())
        && kind_filter_matches(run, query.kind.as_deref().or(query.source.as_deref()))
}

fn run_matches_enterprise_query(
    run: &StatefulWorkflowRunRecord,
    query: &StatefulRunsQuery,
    catalog: &EnterpriseScopeCatalog,
) -> bool {
    org_unit_filter_matches(&run.scope, query.org_unit_id.as_deref())
        && owner_filter_matches(
            &run.scope,
            query.owner_id.as_deref(),
            query.owner_kind.as_deref(),
        )
        && resource_scope_filter_matches(
            run.scope.resource_scope.as_ref(),
            query.resource_kind.as_deref(),
            query.resource_id.as_deref(),
        )
        && option_filter_matches(
            query.policy_version_id.as_deref(),
            run.scope.policy_version_id.as_deref(),
        )
        && data_class_filter_matches(run, query.data_class.as_deref(), catalog)
        && option_serialized_filter_matches(
            query.risk_tier.as_deref(),
            run.scope.risk_tier.as_ref(),
        )
        && delegation_grant_filter_matches(
            &run.scope,
            query.delegation_grant_id.as_deref(),
            catalog,
        )
        && source_binding_filter_matches(&run.scope, query.source_binding_id.as_deref(), catalog)
}

fn org_unit_filter_matches(
    scope: &crate::stateful_runtime::StatefulRuntimeScope,
    expected: Option<&str>,
) -> bool {
    let Some(expected) = normalized_filter(expected) else {
        return true;
    };
    scope
        .owning_org_unit_id
        .as_deref()
        .map(|unit_id| {
            normalize_filter_value(unit_id) == expected
                || normalize_filter_value(unit_id.rsplit('/').next().unwrap_or(unit_id)) == expected
        })
        .unwrap_or(false)
}

fn owner_filter_matches(
    scope: &crate::stateful_runtime::StatefulRuntimeScope,
    owner_id: Option<&str>,
    owner_kind: Option<&str>,
) -> bool {
    let id_matches = option_filter_matches(
        owner_id,
        scope
            .owner_principal
            .as_ref()
            .map(|principal| principal.id.as_str()),
    );
    let kind_matches = option_serialized_filter_matches(
        owner_kind,
        scope.owner_principal.as_ref().map(|p| &p.kind),
    );
    id_matches && kind_matches
}

fn resource_scope_filter_matches(
    scope: Option<&ResourceScope>,
    resource_kind: Option<&str>,
    resource_id: Option<&str>,
) -> bool {
    let kind_matches = option_serialized_filter_matches(
        resource_kind,
        scope.map(|scope| &scope.root.resource_kind),
    );
    let id_matches = option_filter_matches(
        resource_id,
        scope.map(|scope| scope.root.resource_id.as_str()),
    );
    kind_matches && id_matches
}

fn data_class_filter_matches(
    run: &StatefulWorkflowRunRecord,
    expected: Option<&str>,
    catalog: &EnterpriseScopeCatalog,
) -> bool {
    let Some(expected) = normalized_filter(expected) else {
        return true;
    };
    run.scope
        .data_classes
        .iter()
        .any(|data_class| serialized_key(data_class) == expected)
        || source_bindings_for_scope(catalog, &run.scope)
            .iter()
            .any(|binding| serialized_key(&binding.data_class) == expected)
}

fn delegation_grant_filter_matches(
    scope: &crate::stateful_runtime::StatefulRuntimeScope,
    expected: Option<&str>,
    catalog: &EnterpriseScopeCatalog,
) -> bool {
    let Some(expected) = normalized_filter(expected) else {
        return true;
    };
    if !scope
        .delegation_grant_ids
        .iter()
        .any(|grant_id| normalize_filter_value(grant_id) == expected)
    {
        return false;
    }
    active_org_unit_grants_for_scope(catalog, scope)
        .iter()
        .any(|grant| normalize_filter_value(&grant.grant_id) == expected)
}

fn source_binding_filter_matches(
    scope: &crate::stateful_runtime::StatefulRuntimeScope,
    expected: Option<&str>,
    catalog: &EnterpriseScopeCatalog,
) -> bool {
    let Some(expected) = normalized_filter(expected) else {
        return true;
    };
    source_bindings_for_scope(catalog, scope)
        .iter()
        .any(|binding| normalize_filter_value(&binding.binding_id) == expected)
}

fn trigger_filter_matches(run: &StatefulWorkflowRunRecord, expected: Option<&str>) -> bool {
    let Some(expected) = normalized_filter(expected) else {
        return true;
    };
    [
        run.trigger_type.as_deref(),
        run.trigger_event.as_deref(),
        run.source_event_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| normalize_filter_value(value).contains(&expected))
}

fn kind_filter_matches(run: &StatefulWorkflowRunRecord, expected: Option<&str>) -> bool {
    let Some(expected) = normalized_filter(expected) else {
        return true;
    };
    let kind = serialized_key(&run.kind);
    let source_alias = match run.kind {
        StatefulWorkflowRunKind::AutomationV2 => "automation",
        StatefulWorkflowRunKind::Workflow => "workflow",
        StatefulWorkflowRunKind::ContextRun => "context",
        StatefulWorkflowRunKind::Unknown => "unknown",
    };
    kind == expected || source_alias == expected
}

fn option_filter_matches(expected: Option<&str>, actual: Option<&str>) -> bool {
    let Some(expected) = normalized_filter(expected) else {
        return true;
    };
    actual
        .map(|value| normalize_filter_value(value) == expected)
        .unwrap_or(false)
}

fn option_serialized_filter_matches<T: Serialize>(
    expected: Option<&str>,
    actual: Option<&T>,
) -> bool {
    let Some(expected) = normalized_filter(expected) else {
        return true;
    };
    actual
        .map(|value| serialized_key(value) == expected)
        .unwrap_or(false)
}

fn string_filter_matches(expected: Option<&str>, actual: &str) -> bool {
    let Some(expected) = normalized_filter(expected) else {
        return true;
    };
    normalize_filter_value(actual) == expected
}

fn normalized_filter(value: Option<&str>) -> Option<String> {
    let value = normalize_filter_value(value.unwrap_or_default());
    if value.is_empty() || value == "all" {
        None
    } else {
        Some(value)
    }
}

fn normalize_filter_value(value: &str) -> String {
    value.trim().replace('-', "_").to_ascii_lowercase()
}

fn serialized_key<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tandem_types::{
        AccessPermission, DataClass, OrganizationUnit, OrganizationUnitAccessGrant,
        OrganizationUnitKind, PrincipalKind, PrincipalRef, ResourceKind, ResourceRef,
        ResourceScope, SourceBinding, TenantContext, ToolRiskTier,
    };
    use uuid::Uuid;

    use super::*;
    use crate::automation_v2::types::{
        AutomationExecutionPolicy, AutomationFlowSpec, AutomationRunCheckpoint,
        AutomationRunStatus, AutomationV2RunRecord, AutomationV2Schedule, AutomationV2ScheduleType,
        AutomationV2Spec, AutomationV2Status,
    };
    use crate::stateful_runtime::{
        append_stateful_run_event, phase_state_from_status, upsert_stateful_wait,
        write_stateful_run_snapshot, StatefulRunEventRecord, StatefulRunSnapshotRecord,
        StatefulRuntimeScope, StatefulWaitKind, StatefulWaitRecord, StatefulWaitStatus,
        StatefulWorkflowRunStatus,
    };

    fn tenant(org: &str, workspace: &str) -> TenantContext {
        TenantContext::explicit_user_workspace(org, workspace, None, "user-a")
    }

    fn automation_snapshot_with_enterprise_scope(
        tenant_context: &TenantContext,
        resource_scope: ResourceScope,
    ) -> AutomationV2Spec {
        let mut snapshot = AutomationV2Spec {
            automation_id: "automation-a".to_string(),
            name: "Scoped webhook".to_string(),
            description: None,
            status: AutomationV2Status::Active,
            schedule: AutomationV2Schedule {
                schedule_type: AutomationV2ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                timezone: "UTC".to_string(),
                misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
            },
            knowledge: Default::default(),
            agents: Vec::new(),
            flow: AutomationFlowSpec { nodes: Vec::new() },
            execution: AutomationExecutionPolicy::default(),
            output_targets: Vec::new(),
            created_at_ms: 1,
            updated_at_ms: 2,
            creator_id: "user-a".to_string(),
            workspace_root: None,
            metadata: Some(json!({
                "automation_webhook": {
                    "owner_principal": PrincipalRef::new(PrincipalKind::Automation, "automation-a"),
                    "owning_org_unit_id": "finance",
                    "resource_scope": resource_scope,
                    "data_class": DataClass::FinancialRecord,
                    "risk_tier": ToolRiskTier::FinancialRecordAccess,
                    "policy_version_id": "policy-2026-06",
                    "delegation_grant_ids": ["delegation-a"]
                }
            })),
            next_fire_at_ms: None,
            last_fired_at_ms: None,
            scope_policy: None,
            watch_conditions: Vec::new(),
            handoff_config: None,
        };
        snapshot.set_tenant_context(tenant_context);
        snapshot
    }

    fn event(seq: u64, run_id: &str, tenant_context: TenantContext) -> StatefulRunEventRecord {
        StatefulRunEventRecord {
            schema_version: 1,
            event_id: format!("evt-{seq}"),
            run_id: run_id.to_string(),
            seq,
            event_type: "workflow.phase.changed".to_string(),
            occurred_at_ms: 1_000 + seq,
            scope: StatefulRuntimeScope::from_tenant_context(tenant_context),
            actor: Some(PrincipalRef::new(PrincipalKind::Automation, "automation-a")),
            phase_id: Some("phase-a".to_string()),
            phase_transition: None,
            wait_kind: None,
            causation_id: None,
            correlation_id: None,
            payload: json!({ "seq": seq }),
        }
    }

    fn snapshot(
        seq: u64,
        run_id: &str,
        tenant_context: TenantContext,
    ) -> StatefulRunSnapshotRecord {
        let status = StatefulWorkflowRunStatus::Running;
        let phase_state = phase_state_from_status(run_id, &status, 2_000 + seq, Some("phase-a"));
        StatefulRunSnapshotRecord {
            schema_version: 1,
            snapshot_id: format!("snapshot-{seq}"),
            run_id: run_id.to_string(),
            seq,
            created_at_ms: 2_000 + seq,
            scope: StatefulRuntimeScope::from_tenant_context(tenant_context),
            status,
            phase: phase_state.phase,
            phase_history: phase_state.phase_history,
            allowed_next_phases: phase_state.allowed_next_phases,
            phase_id: Some("phase-a".to_string()),
            source_record_kind: None,
            checkpoint: Some(json!({ "seq": seq })),
            payload_digest: Some(format!("sha256:{seq}")),
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            metadata: None,
        }
    }

    async fn stateful_test_state() -> AppState {
        let mut state = crate::test_support::test_state().await;
        let root = std::env::temp_dir().join(format!("stateful-runtime-api-{}", Uuid::new_v4()));
        state.runtime_events_path = root.join("events.jsonl");
        state
    }

    fn automation_run(
        run_id: &str,
        tenant_context: TenantContext,
        status: AutomationRunStatus,
        updated_at_ms: u64,
    ) -> AutomationV2RunRecord {
        AutomationV2RunRecord {
            run_id: run_id.to_string(),
            automation_id: format!("automation-{run_id}"),
            tenant_context,
            trigger_type: "webhook".to_string(),
            status,
            created_at_ms: 1_000,
            updated_at_ms,
            started_at_ms: Some(1_100),
            finished_at_ms: None,
            active_session_ids: Vec::new(),
            latest_session_id: None,
            active_instance_ids: vec![format!("context-{run_id}")],
            checkpoint: AutomationRunCheckpoint {
                completed_nodes: Vec::new(),
                pending_nodes: Vec::new(),
                node_outputs: Default::default(),
                node_attempts: Default::default(),
                node_attempt_verdicts: Default::default(),
                blocked_nodes: Vec::new(),
                awaiting_gate: None,
                gate_history: Vec::new(),
                lifecycle_history: Vec::new(),
                last_failure: None,
            },
            runtime_context: None,
            automation_snapshot: None,
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            execution_claim: None,
            execution_claim_epoch: 0,
            pause_reason: None,
            resume_reason: None,
            detail: None,
            stop_kind: None,
            stop_reason: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            estimated_cost_usd: 0.0,
            scheduler: None,
            trigger_reason: None,
            consumed_handoff_id: None,
            learning_summary: None,
            effective_execution_profile: Default::default(),
            requested_execution_profile: None,
        }
    }

    fn wait(run_id: &str, tenant_context: TenantContext) -> StatefulWaitRecord {
        StatefulWaitRecord {
            schema_version: 1,
            wait_id: "wait-a".to_string(),
            run_id: run_id.to_string(),
            wait_kind: StatefulWaitKind::Webhook,
            status: StatefulWaitStatus::Waiting,
            scope: StatefulRuntimeScope::from_tenant_context(tenant_context),
            phase_id: Some("phase-a".to_string()),
            reason: Some("wait for provider callback".to_string()),
            created_at_ms: 1_200,
            updated_at_ms: 1_200,
            wake_at_ms: None,
            timeout_policy: None,
            event_seq: None,
            wake_idempotency_key: None,
            claimed_by: None,
            claimed_at_ms: None,
            claim_expires_at_ms: None,
            completed_at_ms: None,
            metadata: None,
        }
    }

    #[tokio::test]
    async fn get_events_filters_by_tenant_and_sequence() {
        let state = stateful_test_state().await;
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        let paths =
            StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
        for record in [
            event(1, "run-a", tenant_a.clone()),
            event(2, "run-a", tenant_b),
            event(3, "run-a", tenant_a.clone()),
        ] {
            append_stateful_run_event(&paths.run_events_path, &record)
                .await
                .expect("append event");
        }

        let Json(body) = get_stateful_run_events(
            State(state.clone()),
            Extension(tenant_a),
            Path("run-a".to_string()),
            Query(StatefulRunEventsQuery {
                after_seq: Some(1),
                since_seq: None,
                before_seq: None,
                limit: Some(10),
                tail: None,
            }),
        )
        .await;

        assert_eq!(body.get("count").and_then(Value::as_u64), Some(1));
        assert_eq!(body.get("last_seq").and_then(Value::as_u64), Some(3));
        assert_eq!(
            body.get("sequence_scope").and_then(Value::as_str),
            Some("stateful_runtime")
        );
        let _ = tokio::fs::remove_dir_all(
            paths
                .run_events_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new(".")),
        )
        .await;
    }

    #[tokio::test]
    async fn get_events_uses_tail_value_as_window_size() {
        let state = stateful_test_state().await;
        let tenant_a = tenant("org-a", "workspace-a");
        let paths =
            StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
        for seq in 1..=4 {
            append_stateful_run_event(
                &paths.run_events_path,
                &event(seq, "run-a", tenant_a.clone()),
            )
            .await
            .expect("append event");
        }

        let Json(body) = get_stateful_run_events(
            State(state.clone()),
            Extension(tenant_a),
            Path("run-a".to_string()),
            Query(StatefulRunEventsQuery {
                after_seq: None,
                since_seq: None,
                before_seq: None,
                limit: None,
                tail: Some(2),
            }),
        )
        .await;

        assert_eq!(body.get("count").and_then(Value::as_u64), Some(2));
        assert_eq!(body.get("limit").and_then(Value::as_u64), Some(2));
        let sequences = body
            .get("events")
            .and_then(Value::as_array)
            .map(|events| {
                events
                    .iter()
                    .filter_map(|event| event.get("seq").and_then(Value::as_u64))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        assert_eq!(sequences, vec![3, 4]);
        let _ = tokio::fs::remove_dir_all(
            paths
                .run_events_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new(".")),
        )
        .await;
    }

    #[tokio::test]
    async fn run_event_summary_cache_tracks_visible_first_and_latest_seq() {
        let state = stateful_test_state().await;
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        let paths =
            StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
        for record in [
            event(3, "run-a", tenant_a.clone()),
            event(2, "run-a", tenant_b),
            event(1, "run-a", tenant_a.clone()),
        ] {
            append_stateful_run_event(&paths.run_events_path, &record)
                .await
                .expect("append event");
        }

        let summaries = stateful_run_event_summaries_by_run(&paths.run_events_path, &tenant_a);
        let summary = summaries.get("run-a").expect("run summary");

        assert_eq!(summary.first_event_seq, 1);
        assert_eq!(summary.latest_event.seq, 3);
        assert_eq!(summary.latest_event.event_id, "evt-3");
        let _ = tokio::fs::remove_dir_all(
            paths
                .run_events_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new(".")),
        )
        .await;
    }

    #[test]
    fn run_response_treats_missing_cached_event_summary_as_empty() {
        let tenant_a = tenant("org-a", "workspace-a");
        let root = std::env::temp_dir().join(format!(
            "stateful-runtime-api-empty-summary-{}",
            Uuid::new_v4()
        ));
        let paths = StatefulRuntimeStoragePaths::new(
            root.join("events.jsonl"),
            root.join("snapshots"),
            root.join("waits.json"),
        );
        let run = crate::stateful_runtime::stateful_run_from_automation_v2(&automation_run(
            "run-without-events",
            tenant_a.clone(),
            AutomationRunStatus::Running,
            4_000,
        ));
        let summaries = HashMap::new();

        let body = stateful_run_response(
            &paths,
            &tenant_a,
            &EnterpriseScopeCatalog::default(),
            run,
            false,
            Some(&summaries),
        );

        assert!(body.get("latest_event").is_some_and(Value::is_null));
        assert_eq!(
            body["replay_boundaries"]["can_replay_from_event_log"],
            false
        );
    }

    #[tokio::test]
    async fn snapshot_endpoints_filter_by_tenant() {
        let state = stateful_test_state().await;
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        let paths =
            StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
        for record in [
            snapshot(1, "run-a", tenant_a.clone()),
            snapshot(2, "run-a", tenant_b),
            snapshot(3, "run-a", tenant_a.clone()),
        ] {
            write_stateful_run_snapshot(&paths.snapshots_root, &record)
                .await
                .expect("write snapshot");
        }

        let Json(body) = list_stateful_run_snapshots(
            State(state.clone()),
            Extension(tenant_a.clone()),
            Path("run-a".to_string()),
            Query(StatefulRunSnapshotsQuery { limit: Some(10) }),
        )
        .await;

        assert_eq!(body.get("count").and_then(Value::as_u64), Some(2));
        assert_eq!(body.get("latest_seq").and_then(Value::as_u64), Some(3));

        let response = get_stateful_run_snapshot(
            State(state.clone()),
            Extension(tenant_a.clone()),
            Path(("run-a".to_string(), "snapshot-3".to_string())),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let hidden = get_stateful_run_snapshot(
            State(state.clone()),
            Extension(tenant_a),
            Path(("run-a".to_string(), "snapshot-2".to_string())),
        )
        .await;
        assert_eq!(hidden.status(), StatusCode::NOT_FOUND);

        let _ = tokio::fs::remove_dir_all(&paths.snapshots_root).await;
    }

    #[tokio::test]
    async fn run_list_and_detail_use_canonical_stateful_sources() {
        let state = stateful_test_state().await;
        let tenant_a = tenant("org-a", "workspace-a");
        let tenant_b = tenant("org-b", "workspace-b");
        let paths =
            StatefulRuntimeStoragePaths::from_runtime_events_path(&state.runtime_events_path);
        state.automation_v2_runs.write().await.insert(
            "run-a".to_string(),
            automation_run(
                "run-a",
                tenant_a.clone(),
                AutomationRunStatus::Running,
                4_000,
            ),
        );
        state.automation_v2_runs.write().await.insert(
            "run-b".to_string(),
            automation_run("run-b", tenant_b, AutomationRunStatus::Failed, 5_000),
        );
        append_stateful_run_event(&paths.run_events_path, &event(1, "run-a", tenant_a.clone()))
            .await
            .expect("append event");
        write_stateful_run_snapshot(
            &paths.snapshots_root,
            &snapshot(1, "run-a", tenant_a.clone()),
        )
        .await
        .expect("write snapshot");
        upsert_stateful_wait(&paths.waits_path, wait("run-a", tenant_a.clone()))
            .await
            .expect("write wait");

        let Json(body) = list_stateful_runs(
            State(state.clone()),
            Extension(tenant_a.clone()),
            Query(StatefulRunsQuery {
                status: Some("running".to_string()),
                workspace_id: Some("workspace-a".to_string()),
                limit: Some(25),
                ..Default::default()
            }),
        )
        .await;

        assert_eq!(body.get("count").and_then(Value::as_u64), Some(1));
        let rows = body.get("runs").and_then(Value::as_array).expect("runs");
        assert_eq!(rows[0]["run"]["run_id"], "run-a");
        assert_eq!(rows[0]["current_wait"]["wait_kind"], "webhook");
        assert_eq!(rows[0]["latest_event"]["seq"], 1);
        assert_eq!(rows[0]["latest_snapshot"]["snapshot_id"], "snapshot-1");
        assert_eq!(
            rows[0]["replay_boundaries"]["can_replay_from_snapshot"],
            true
        );

        let response = get_stateful_run(
            State(state.clone()),
            Extension(tenant_a),
            Path("run-a".to_string()),
            Query(StatefulRunDetailQuery {
                event_limit: Some(5),
                snapshot_limit: Some(5),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let hidden = get_stateful_run(
            State(state.clone()),
            Extension(tenant("org-a", "other-workspace")),
            Path("run-a".to_string()),
            Query(StatefulRunDetailQuery::default()),
        )
        .await;
        assert_eq!(hidden.status(), StatusCode::NOT_FOUND);

        let _ = tokio::fs::remove_dir_all(
            paths
                .run_events_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new(".")),
        )
        .await;
    }

    #[tokio::test]
    async fn run_list_exposes_and_filters_enterprise_scope_summary() {
        let state = stateful_test_state().await;
        let tenant_a = tenant("org-a", "workspace-a");
        let resource = ResourceRef::new("org-a", "workspace-a", ResourceKind::Repository, "repo-a");
        let resource_scope = ResourceScope::root(resource.clone());
        let org_unit = OrganizationUnit::active(
            "Finance",
            tenant_a.clone(),
            "Finance Ops",
            OrganizationUnitKind::Department,
            PrincipalRef::human_user("user-a"),
            1,
        )
        .with_taxonomy_id(
            // The runtime stamps canonical lowercase IDs, while imported catalogs may
            // preserve source casing.
            "Organization_Unit",
        );
        let grant = OrganizationUnitAccessGrant::active(
            "delegation-a",
            tenant_a.clone(),
            org_unit.principal_ref(),
            resource.clone(),
            1,
        )
        .with_permissions(vec![AccessPermission::Read])
        .with_data_classes(vec![DataClass::FinancialRecord]);
        state
            .enterprise
            .org_units
            .write()
            .await
            .insert("Finance".to_string(), org_unit);
        state
            .enterprise
            .org_unit_access_grants
            .write()
            .await
            .insert("delegation-a".to_string(), grant);
        {
            let mut source_bindings = state.enterprise.source_bindings.write().await;
            for index in 0..13 {
                let binding_id = format!("binding-repo-{index:02}");
                let mut binding = SourceBinding::enabled(
                    binding_id.clone(),
                    tenant_a.clone(),
                    "github",
                    "github",
                    format!("repo-a-{index:02}"),
                    resource.clone(),
                    DataClass::FinancialRecord,
                    PrincipalRef::human_user("user-a"),
                    1,
                );
                binding.source_root_label = Some(format!("Finance repo {index:02}"));
                source_bindings.insert(binding_id, binding);
            }
        }
        let mut run = automation_run(
            "run-a",
            tenant_a.clone(),
            AutomationRunStatus::Running,
            4_000,
        );
        run.automation_snapshot = Some(automation_snapshot_with_enterprise_scope(
            &tenant_a,
            resource_scope,
        ));
        state
            .automation_v2_runs
            .write()
            .await
            .insert("run-a".to_string(), run);

        let Json(body) = list_stateful_runs(
            State(state.clone()),
            Extension(tenant_a),
            Query(StatefulRunsQuery {
                org_unit_id: Some("finance".to_string()),
                owner_id: Some("automation-a".to_string()),
                owner_kind: Some("automation".to_string()),
                resource_kind: Some("repository".to_string()),
                resource_id: Some("repo-a".to_string()),
                policy_version_id: Some("policy-2026-06".to_string()),
                data_class: Some("financial_record".to_string()),
                risk_tier: Some("financial_record_access".to_string()),
                delegation_grant_id: Some("delegation-a".to_string()),
                source_binding_id: Some("binding-repo-12".to_string()),
                limit: Some(25),
                ..Default::default()
            }),
        )
        .await;

        assert_eq!(body.get("count").and_then(Value::as_u64), Some(1));
        assert_eq!(
            body.pointer("/filters/source_binding_id")
                .and_then(Value::as_str),
            Some("binding-repo-12")
        );
        let row = body
            .get("runs")
            .and_then(Value::as_array)
            .and_then(|runs| runs.first())
            .expect("run row");
        assert_eq!(
            row.pointer("/run/run_id").and_then(Value::as_str),
            Some("run-a")
        );
        assert_eq!(
            row.pointer("/enterprise_scope/owning_org_unit/display_name")
                .and_then(Value::as_str),
            Some("Finance Ops")
        );
        assert_eq!(
            row.pointer("/enterprise_scope/owning_org_unit_id")
                .and_then(Value::as_str),
            Some("finance")
        );
        assert_eq!(
            row.pointer("/enterprise_scope/summary/resource")
                .and_then(Value::as_str),
            Some("repository:repo-a")
        );
        assert_eq!(
            row.pointer("/enterprise_scope/visible_knowledge_sources/0/binding_id")
                .and_then(Value::as_str),
            Some("binding-repo-00")
        );
        assert_eq!(
            row.pointer("/enterprise_scope/summary/knowledge_source_count")
                .and_then(Value::as_u64),
            Some(12)
        );
        assert_eq!(
            row.pointer("/enterprise_scope/org_unit_grants/0/grant_id")
                .and_then(Value::as_str),
            Some("delegation-a")
        );
    }
}
