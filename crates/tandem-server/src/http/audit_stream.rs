use std::convert::Infallible;

use axum::body::{Body, Bytes};
use axum::extract::{Extension, Query, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{json, Value};
use tandem_types::{EngineEvent, RequestPrincipal, TenantContext};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::AppState;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct ProtectedAuditQuery {
    event_type: Option<String>,
    run_id: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct AuditExportQuery {
    since_ms: Option<u64>,
    until_ms: Option<u64>,
}

pub(crate) async fn protected_audit_events(
    State(state): State<AppState>,
    Extension(principal): Extension<RequestPrincipal>,
    Extension(tenant_context): Extension<TenantContext>,
    Query(query): Query<ProtectedAuditQuery>,
) -> Response {
    if !audit_admin_allowed(&principal) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(json!({
                "error": "Admin capability required",
                "code": "AUDIT_ADMIN_REQUIRED"
            })),
        )
            .into_response();
    }

    let mut rows = crate::audit::load_protected_audit_events_for_tenant(&state, &tenant_context)
        .await
        .into_iter()
        .filter(|event| {
            query
                .event_type
                .as_deref()
                .map(|event_type| event.event_type == event_type)
                .unwrap_or(true)
        })
        .filter(|event| {
            query
                .run_id
                .as_ref()
                .map(|run_ids| {
                    let run_ids = run_ids
                        .split(',')
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(str::to_string)
                        .collect::<std::collections::BTreeSet<_>>();
                    !run_ids.is_empty()
                        && protected_audit_payload_contains_any_run_id(&event.payload, &run_ids)
                })
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    if let Some(limit) = query.limit.filter(|limit| *limit > 0) {
        if rows.len() > limit {
            rows = rows.split_off(rows.len() - limit);
        }
    }

    axum::Json(json!({
        "events": rows,
        "count": rows.len(),
    }))
    .into_response()
}

pub(crate) async fn audit_stream(
    State(state): State<AppState>,
    Extension(principal): Extension<RequestPrincipal>,
    Extension(tenant_context): Extension<TenantContext>,
) -> Response {
    if !audit_admin_allowed(&principal) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(json!({
                "error": "Admin capability required",
                "code": "AUDIT_ADMIN_REQUIRED"
            })),
        )
            .into_response();
    }

    let rx = state.event_bus.subscribe();
    let stream_tenant = tenant_context.clone();
    let stream = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(event) if audit_event_matches_tenant(&event, &stream_tenant) => {
            audit_event_to_stream_record(&event).map(|record| {
                let line =
                    serde_json::to_string(&record).unwrap_or_else(|_| "{}".to_string()) + "\n";
                Ok::<Bytes, Infallible>(Bytes::from(line))
            })
        }
        Ok(_) => None,
        Err(_) => None,
    });

    let mut response = Body::from_stream(stream).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson"),
    );
    response
}

fn protected_audit_payload_contains_any_run_id(
    payload: &Value,
    run_ids: &std::collections::BTreeSet<String>,
) -> bool {
    match payload {
        Value::String(value) => run_ids.contains(value),
        Value::Array(values) => values
            .iter()
            .any(|value| protected_audit_payload_contains_any_run_id(value, run_ids)),
        Value::Object(map) => map
            .values()
            .any(|value| protected_audit_payload_contains_any_run_id(value, run_ids)),
        _ => false,
    }
}

fn audit_admin_allowed(principal: &RequestPrincipal) -> bool {
    matches!(principal.source.as_str(), "api_token" | "control_panel")
}

fn audit_event_matches_tenant(event: &EngineEvent, tenant: &TenantContext) -> bool {
    // Producers tag tenancy in one of two shapes: a nested `tenantContext` object (the
    // canonical serialized `TenantContext`, e.g. approval/fintech/tool-effect events) or
    // flat top-level `org_id`/`workspace_id` fields. Read the nested object first, then
    // fall back to the flat spellings so every recognized shape is scoped.
    let tenant_ctx = event.properties.get("tenantContext");
    let event_org = tenant_ctx
        .and_then(|ctx| ctx.get("org_id"))
        .or_else(|| event.properties.get("org_id"))
        .or_else(|| event.properties.get("orgID"))
        .or_else(|| event.properties.get("organization_id"))
        .and_then(Value::as_str);
    let event_workspace = tenant_ctx
        .and_then(|ctx| ctx.get("workspace_id"))
        .or_else(|| event.properties.get("workspace_id"))
        .or_else(|| event.properties.get("workspaceID"))
        .and_then(Value::as_str);

    // Local/single-tenant deployments are a no-op: there is exactly one tenant, so the
    // implicit-local admin sees every event regardless of whether it carries an org tag.
    if tenant.is_local_implicit() {
        return true;
    }

    // Explicit (multi-tenant) context: fail closed. An event is only visible to a reader
    // that can be positively matched to it. An untagged event (no org_id) cannot be proven
    // to belong to this tenant, so it is hidden rather than leaked to every tenant (TAN-9).
    let Some(org_id) = event_org else {
        return false;
    };
    if org_id != tenant.org_id {
        return false;
    }
    // Workspace is a second isolation axis. When the event carries a workspace, it must
    // match; when it omits one, org-level scoping above is sufficient to attribute it.
    if let Some(workspace_id) = event_workspace {
        if workspace_id != tenant.workspace_id {
            return false;
        }
    }
    true
}

pub(crate) fn audit_event_to_stream_record(event: &EngineEvent) -> Option<Value> {
    match event.event_type.as_str() {
        "tool.effect.recorded" => tool_effect_record(event),
        "approval.decision.recorded" => approval_decision_record(event),
        "channel.capability.changed" => capability_change_record(event),
        "fintech.protected_action.denied" | "fintech.protected_action.approved" => {
            fintech_protected_action_record(event)
        }
        _ => None,
    }
}

fn base_record(event: &EngineEvent, command: &str, result: Value) -> Value {
    json!({
        "event_type": event.event_type,
        "actor_id": event.properties.get("actor_id").and_then(Value::as_str),
        "executed_as": event.properties.get("executed_as").and_then(Value::as_str).unwrap_or("tandem-server"),
        "command": command,
        "workspace": event.properties.get("workspace").and_then(Value::as_str),
        "tool_call_id": event.properties.get("tool_call_id").and_then(Value::as_str),
        "result": result,
        "timestamp": crate::now_ms(),
        "channel": event.properties.get("channel").and_then(Value::as_str),
    })
}

fn tool_effect_record(event: &EngineEvent) -> Option<Value> {
    let record = event.properties.get("record")?;
    let command = record.get("tool").and_then(Value::as_str).unwrap_or("tool");
    let workspace = record
        .pointer("/args_summary/workspace_root")
        .and_then(Value::as_str);
    let mut row = base_record(
        event,
        command,
        json!({
            "phase": record.get("phase"),
            "status": record.get("status"),
            "error": record.get("error"),
        }),
    );
    let obj = row.as_object_mut()?;
    if let Some(workspace) = workspace {
        obj.insert(
            "workspace".to_string(),
            Value::String(workspace.to_string()),
        );
    }
    if let Some(tool_call_id) = record.get("tool_call_id").and_then(Value::as_str) {
        obj.insert(
            "tool_call_id".to_string(),
            Value::String(tool_call_id.to_string()),
        );
    }
    Some(row)
}

fn approval_decision_record(event: &EngineEvent) -> Option<Value> {
    Some(base_record(
        event,
        "approval_decision",
        json!({
            "run_id": event.properties.get("run_id"),
            "node_id": event.properties.get("node_id"),
            "decision": event.properties.get("decision"),
            "reason": event.properties.get("reason"),
        }),
    ))
}

fn capability_change_record(event: &EngineEvent) -> Option<Value> {
    Some(base_record(
        event,
        "capability_change",
        json!({
            "channel": event.properties.get("channel"),
            "user_id": event.properties.get("user_id"),
            "max_tier": event.properties.get("max_tier"),
        }),
    ))
}

fn fintech_protected_action_record(event: &EngineEvent) -> Option<Value> {
    let command = match event.event_type.as_str() {
        "fintech.protected_action.approved" => "fintech_protected_action_approved",
        _ => "fintech_protected_action_denied",
    };
    Some(base_record(
        event,
        command,
        json!({
            "run_id": event.properties.get("runID"),
            "automation_id": event.properties.get("automationID"),
            "tool": event.properties.get("tool"),
            "classification": event.properties.get("classification"),
            "category": event.properties.get("category"),
            "reason": event.properties.get("reason"),
            "approval": event.properties.get("approval"),
        }),
    ))
}

/// GET /audit/ledger/manifest
///
/// Returns the `AuditLedgerManifest` for the protected audit ledger: schema version,
/// record count, last seq, ledger root hash, and generation timestamp. Callers can use
/// this to verify that the ledger has not been truncated or tampered with since it was
/// last exported.
pub(crate) async fn audit_ledger_manifest(
    State(state): State<AppState>,
    Extension(principal): Extension<RequestPrincipal>,
    Extension(_tenant_context): Extension<TenantContext>,
) -> Response {
    if !audit_admin_allowed(&principal) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(json!({
                "error": "Admin capability required",
                "code": "AUDIT_ADMIN_REQUIRED"
            })),
        )
            .into_response();
    }
    match crate::audit::generate_audit_ledger_manifest(&state.protected_audit_path).await {
        Ok(manifest) => axum::Json(manifest).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            axum::Json(json!({
                "error": err.to_string(),
                "code": "AUDIT_MANIFEST_ERROR"
            })),
        )
            .into_response(),
    }
}

/// GET /audit/ledger/export
///
/// Produces a deterministic NDJSON bundle of protected audit events for the requesting
/// tenant, followed by a `bundle_manifest` trailer record. The bundle is independently
/// verifiable: each record carries `seq`, `prev_hash`, and `record_hash` fields that
/// can be re-hashed to confirm chain integrity. Query params:
///
/// - `since_ms` (optional): include only records with `created_at_ms >= since_ms`
/// - `until_ms` (optional): include only records with `created_at_ms <= until_ms`
pub(crate) async fn audit_ledger_export(
    State(state): State<AppState>,
    Extension(principal): Extension<RequestPrincipal>,
    Extension(tenant_context): Extension<TenantContext>,
    Query(query): Query<AuditExportQuery>,
) -> Response {
    if !audit_admin_allowed(&principal) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(json!({
                "error": "Admin capability required",
                "code": "AUDIT_ADMIN_REQUIRED"
            })),
        )
            .into_response();
    }

    let mut records =
        crate::audit::load_protected_audit_events_for_tenant(&state, &tenant_context).await;

    // Sort by seq for stable chain ordering in the export.
    records.sort_by_key(|e| e.seq);

    // Apply optional time-range filter.
    let filtered: Vec<_> = records
        .into_iter()
        .filter(|e| {
            query
                .since_ms
                .map(|ms| e.created_at_ms >= ms)
                .unwrap_or(true)
        })
        .filter(|e| {
            query
                .until_ms
                .map(|ms| e.created_at_ms <= ms)
                .unwrap_or(true)
        })
        .collect();

    let record_count = filtered.len() as u64;
    let last_seq = filtered.iter().map(|e| e.seq).max().unwrap_or(0);
    let root_hash = filtered
        .iter()
        .rev()
        .find(|e| !e.record_hash.is_empty())
        .map(|e| e.record_hash.clone());

    // Build NDJSON body.
    let mut body = String::new();
    for record in &filtered {
        match serde_json::to_string(record) {
            Ok(line) => {
                body.push_str(&line);
                body.push('\n');
            }
            Err(err) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    axum::Json(json!({
                        "error": err.to_string(),
                        "code": "AUDIT_EXPORT_SERIALIZE_ERROR"
                    })),
                )
                    .into_response();
            }
        }
    }

    // Append bundle manifest trailer as the final NDJSON record.
    let trailer = json!({
        "type": "bundle_manifest",
        "schema_version": 2u32,
        "record_count": record_count,
        "last_seq": last_seq,
        "root_hash": root_hash,
        "tenant_org_id": &tenant_context.org_id,
        "tenant_workspace_id": &tenant_context.workspace_id,
        "since_ms": query.since_ms,
        "until_ms": query.until_ms,
        "exported_at_ms": crate::now_ms(),
    });
    if let Ok(line) = serde_json::to_string(&trailer) {
        body.push_str(&line);
        body.push('\n');
    }

    let mut response = Body::from(body).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static("attachment; filename=\"audit-ledger-export.ndjson\""),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_tool_effect_event_to_ndjson_record_shape() {
        let event = EngineEvent::new(
            "tool.effect.recorded",
            json!({
                "record": {
                    "tool": "read",
                    "tool_call_id": "call-1",
                    "phase": "outcome",
                    "status": "succeeded",
                    "args_summary": { "workspace_root": "/workspace/acme" }
                }
            }),
        );
        let row = audit_event_to_stream_record(&event).unwrap();
        assert_eq!(row["command"], "read");
        assert_eq!(row["workspace"], "/workspace/acme");
        assert_eq!(row["tool_call_id"], "call-1");
    }

    #[test]
    fn maps_capability_change_event_to_audit_record() {
        let event = EngineEvent::new(
            "channel.capability.changed",
            json!({
                "channel": "telegram",
                "user_id": "42",
                "max_tier": "approve"
            }),
        );
        let row = audit_event_to_stream_record(&event).unwrap();
        assert_eq!(row["command"], "capability_change");
        assert_eq!(row["channel"], "telegram");
        assert_eq!(row["result"]["max_tier"], "approve");
    }

    #[test]
    fn maps_fintech_protected_action_denial_to_audit_record() {
        let event = EngineEvent::new(
            "fintech.protected_action.denied",
            json!({
                "runID": "run-1",
                "automationID": "automation-1",
                "tool": "mcp.bank.release_funds",
                "classification": "requires_approval",
                "category": "money_movement",
                "reason": "approval required"
            }),
        );
        let row = audit_event_to_stream_record(&event).unwrap();
        assert_eq!(row["command"], "fintech_protected_action_denied");
        assert_eq!(row["result"]["run_id"], "run-1");
        assert_eq!(row["result"]["category"], "money_movement");
    }

    #[test]
    fn maps_fintech_protected_action_approval_to_audit_record() {
        let event = EngineEvent::new(
            "fintech.protected_action.approved",
            json!({
                "runID": "run-1",
                "automationID": "automation-1",
                "tool": "mcp.bank.release_funds",
                "category": "money_movement",
                "approval": {
                    "gate_node_id": "approve_protected_action",
                    "action_hash": "hash-1"
                }
            }),
        );
        let row = audit_event_to_stream_record(&event).unwrap();
        assert_eq!(row["command"], "fintech_protected_action_approved");
        assert_eq!(row["result"]["approval"]["action_hash"], "hash-1");
    }

    // --- TAN-9 / CT-04: cross-tenant audit visibility negative tests ---------
    //
    // These exercise the pure tenant-scoping predicate `audit_event_matches_tenant`,
    // which is the gate every event passes through before it is streamed to a reader
    // (see the `audit_stream` handler). Driving the full `/audit/stream` HTTP path is
    // scaffolded separately below (`tenant_b_cannot_read_tenant_a_audit_events`) but
    // left #[ignore] until the streaming harness gotchas are resolved.

    fn tenant(org: &str, workspace: &str) -> TenantContext {
        TenantContext::explicit(org, workspace, None)
    }

    fn org_tagged_event(org: &str, workspace: &str) -> EngineEvent {
        EngineEvent::new(
            "fintech.protected_action.denied",
            json!({
                "org_id": org,
                "workspace_id": workspace,
                "runID": "run-sensitive-1",
                "automationID": "automation-sensitive-1",
                "tool": "mcp.bank.release_funds",
                "classification": "requires_approval",
                "category": "money_movement",
                "reason": "approval required",
            }),
        )
    }

    #[test]
    fn tenant_a_can_see_its_own_org_scoped_audit_event() {
        let event = org_tagged_event("tenant-a-org", "tenant-a-workspace");
        let reader = tenant("tenant-a-org", "tenant-a-workspace");
        assert!(
            audit_event_matches_tenant(&event, &reader),
            "tenant A must see audit events emitted under its own org/workspace"
        );
    }

    #[test]
    fn tenant_b_cannot_see_tenant_a_org_scoped_audit_event() {
        // The core CT-04 negative assertion: an event tagged with tenant A's org
        // must be filtered out for a tenant B reader.
        let event = org_tagged_event("tenant-a-org", "tenant-a-workspace");
        let reader = tenant("tenant-b-org", "tenant-b-workspace");
        assert!(
            !audit_event_matches_tenant(&event, &reader),
            "tenant B must NOT see tenant A's audit events"
        );
    }

    #[test]
    fn matching_org_but_foreign_workspace_is_denied() {
        // Workspace is a second isolation axis: same org, different workspace -> deny.
        let event = org_tagged_event("shared-org", "workspace-a");
        let reader = tenant("shared-org", "workspace-b");
        assert!(
            !audit_event_matches_tenant(&event, &reader),
            "a foreign workspace within the same org must still be denied"
        );
    }

    #[test]
    fn alternate_org_id_property_spellings_are_scoped() {
        // The handler accepts `org_id` / `orgID` / `organization_id`. A negative test
        // must hold for each spelling so a producer can't sidestep scoping by casing.
        for org_key in ["org_id", "orgID", "organization_id"] {
            let event = EngineEvent::new(
                "fintech.protected_action.denied",
                json!({ org_key: "tenant-a-org", "tool": "mcp.bank.release_funds" }),
            );
            let reader = tenant("tenant-b-org", "tenant-b-workspace");
            assert!(
                !audit_event_matches_tenant(&event, &reader),
                "event keyed by `{org_key}` must be scoped out for a foreign tenant"
            );
        }
    }

    fn tenant_context_tagged_event(org: &str, workspace: &str) -> EngineEvent {
        // Mirrors the canonical producer shape (approval/fintech/tool-effect events):
        // tenancy carried as a nested serialized `TenantContext` under `tenantContext`.
        EngineEvent::new(
            "approval.decision.recorded",
            json!({
                "run_id": "run-1",
                "automation_id": "automation-1",
                "node_id": "approve_protected_action",
                "decision": "approved",
                "executed_as": "approval_gate",
                "tenantContext": TenantContext::explicit(org, workspace, None),
            }),
        )
    }

    #[test]
    fn nested_tenant_context_tag_is_scoped_to_its_tenant() {
        // Regression guard for the review finding: events tagged via a nested
        // `tenantContext` object (not top-level org_id) must still be tenant-scoped.
        let event = tenant_context_tagged_event("tenant-a-org", "tenant-a-workspace");
        let owner = tenant("tenant-a-org", "tenant-a-workspace");
        let other = tenant("tenant-b-org", "tenant-b-workspace");
        assert!(
            audit_event_matches_tenant(&event, &owner),
            "owning tenant must see its own tenantContext-tagged audit event"
        );
        assert!(
            !audit_event_matches_tenant(&event, &other),
            "a tenantContext-tagged event must not leak to another tenant"
        );
    }

    #[test]
    fn untagged_event_is_hidden_from_explicit_tenant_fail_closed() {
        // TAN-9 hardening: an event with no org_id/workspace_id cannot be attributed to
        // any tenant, so under an explicit (multi-tenant) context it must be hidden rather
        // than leaked to every reader. Regression guard for the former fail-open gap.
        let event = EngineEvent::new(
            "fintech.protected_action.denied",
            json!({ "tool": "mcp.bank.release_funds", "category": "money_movement" }),
        );
        let reader = tenant("tenant-b-org", "tenant-b-workspace");
        assert!(
            !audit_event_matches_tenant(&event, &reader),
            "an untagged audit event must fail closed for an explicit tenant reader"
        );
    }

    #[test]
    fn local_implicit_reader_sees_untagged_events_no_op() {
        // The "local/single-tenant no-op" invariant: with exactly one (implicit-local)
        // tenant, the admin must still see untagged events. Fail-closed only applies to
        // explicit multi-tenant contexts.
        let event = EngineEvent::new(
            "fintech.protected_action.denied",
            json!({ "tool": "mcp.bank.release_funds", "category": "money_movement" }),
        );
        let reader = TenantContext::local_implicit();
        assert!(
            audit_event_matches_tenant(&event, &reader),
            "local/single-tenant deployments must remain a no-op (see untagged events)"
        );
    }

    // The full HTTP-path negative tests for `/audit/stream` (subscribe-then-publish
    // ordering + bounded stream reads) live in
    // `crate::http::tests::audit` — see `audit_stream_hides_other_tenants_events` and
    // `audit_stream_hides_untagged_events_from_explicit_tenant`.
}
