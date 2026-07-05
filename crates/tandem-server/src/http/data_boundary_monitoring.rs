//! TAN-398: operator monitoring read model for data-boundary decisions.
//!
//! Aggregates the `data_boundary.*` records already persisted in the
//! protected audit ledger (see `data_boundary_bridge`) into counts an
//! operator can watch over time: leakage attempts by tenant, provider,
//! model, provider boundary class, action, sensitive class, source kind,
//! and policy fingerprint. Every field aggregated here comes from the
//! audit-safe `DataBoundaryEvent` shape — classes, counts, hashes, reason
//! codes — so the read model can never expose raw content. Payload hashes
//! support dedupe (`unique_payload_hashes` vs `repeat_payload_events`)
//! without storing payloads.

use std::collections::BTreeMap;

use axum::extract::{Extension, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use tandem_types::{RequestPrincipal, TenantContext};

use crate::audit::{load_protected_audit_events_for_tenant, ProtectedAuditEnvelope};
use crate::AppState;

const DEFAULT_RECENT_LIMIT: usize = 20;
const MAX_RECENT_LIMIT: usize = 100;

#[derive(Debug, Default, Deserialize)]
pub(crate) struct DataBoundaryMonitoringQuery {
    since_ms: Option<u64>,
    until_ms: Option<u64>,
    action: Option<String>,
    provider_id: Option<String>,
    source_kind: Option<String>,
    recent_limit: Option<usize>,
}

/// Same admin surface as the other `/audit/*` reads: the ledger is
/// tenant-scoped by `load_protected_audit_events_for_tenant`, and only
/// admin principals may query it.
fn monitoring_admin_allowed(principal: &RequestPrincipal) -> bool {
    matches!(principal.source.as_str(), "api_token" | "control_panel")
}

pub(crate) async fn get_data_boundary_monitoring(
    State(state): State<AppState>,
    Extension(principal): Extension<RequestPrincipal>,
    Extension(tenant_context): Extension<TenantContext>,
    Query(query): Query<DataBoundaryMonitoringQuery>,
) -> Response {
    if !monitoring_admin_allowed(&principal) {
        return (
            StatusCode::FORBIDDEN,
            axum::Json(json!({
                "error": "Admin capability required",
                "code": "AUDIT_ADMIN_REQUIRED"
            })),
        )
            .into_response();
    }

    let events = load_protected_audit_events_for_tenant(&state, &tenant_context)
        .await
        .into_iter()
        .filter(|event| event.event_type.starts_with("data_boundary."))
        .filter(|event| {
            query
                .since_ms
                .is_none_or(|since| event.created_at_ms >= since)
        })
        .filter(|event| {
            query
                .until_ms
                .is_none_or(|until| event.created_at_ms <= until)
        })
        .filter(|event| {
            query
                .action
                .as_deref()
                .is_none_or(|action| payload_str(&event.payload, &["action"]) == Some(action))
        })
        .filter(|event| {
            query.provider_id.as_deref().is_none_or(|provider_id| {
                payload_str(&event.payload, &["provider", "provider_id"]) == Some(provider_id)
            })
        })
        .filter(|event| {
            query
                .source_kind
                .as_deref()
                .is_none_or(|kind| payload_str(&event.payload, &["sourceKind"]) == Some(kind))
        })
        .collect::<Vec<_>>();

    let recent_limit = query
        .recent_limit
        .unwrap_or(DEFAULT_RECENT_LIMIT)
        .clamp(1, MAX_RECENT_LIMIT);

    axum::Json(build_read_model(&events, recent_limit)).into_response()
}

fn build_read_model(events: &[ProtectedAuditEnvelope], recent_limit: usize) -> Value {
    let mut by_event_type = BTreeMap::new();
    let mut by_action = BTreeMap::new();
    let mut by_provider = BTreeMap::new();
    let mut by_model = BTreeMap::new();
    let mut by_boundary_class = BTreeMap::new();
    let mut by_classification_source = BTreeMap::new();
    let mut by_sensitive_class: BTreeMap<String, u64> = BTreeMap::new();
    let mut by_source_kind = BTreeMap::new();
    let mut by_policy_fingerprint = BTreeMap::new();
    let mut by_tenant = BTreeMap::new();
    let mut payload_hash_counts: BTreeMap<String, u64> = BTreeMap::new();

    for event in events {
        bump(&mut by_event_type, Some(event.event_type.as_str()));
        bump(&mut by_action, payload_str(&event.payload, &["action"]));
        bump(
            &mut by_provider,
            payload_str(&event.payload, &["provider", "provider_id"]),
        );
        bump(
            &mut by_model,
            payload_str(&event.payload, &["provider", "model_id"]),
        );
        bump(
            &mut by_boundary_class,
            payload_str(&event.payload, &["provider", "boundary_class"]),
        );
        bump(
            &mut by_classification_source,
            payload_str(&event.payload, &["classificationSource"]),
        );
        bump(
            &mut by_source_kind,
            payload_str(&event.payload, &["sourceKind"]),
        );
        bump(
            &mut by_policy_fingerprint,
            payload_str(&event.payload, &["policy_fingerprint"]),
        );
        bump(&mut by_tenant, Some(tenant_key(event).as_str()));
        if let Some(hash) = payload_str(&event.payload, &["payload_hash"]) {
            *payload_hash_counts.entry(hash.to_string()).or_default() += 1;
        }
        if let Some(classes) = event
            .payload
            .get("finding_summary")
            .and_then(|summary| summary.get("by_class"))
            .and_then(Value::as_object)
        {
            for (class, count) in classes {
                *by_sensitive_class.entry(class.clone()).or_default() +=
                    count.as_u64().unwrap_or(0);
            }
        }
    }

    let unique_payload_hashes = payload_hash_counts.len() as u64;
    let repeat_payload_events: u64 = payload_hash_counts
        .values()
        .map(|count| count.saturating_sub(1))
        .sum();

    // The ledger loader orders same-millisecond ties by random event_id, so
    // "newest" must be decided by the monotonic ledger seq, not input order.
    let mut ordered = events.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|event| (event.created_at_ms, event.seq));
    let recent = ordered
        .iter()
        .rev()
        .take(recent_limit)
        .map(|event| recent_summary(event))
        .collect::<Vec<_>>();

    json!({
        "source": "data_boundary_monitoring",
        "payload_policy": "audit_safe_fields_only",
        "totals": {
            "events": events.len(),
            "unique_payload_hashes": unique_payload_hashes,
            "repeat_payload_events": repeat_payload_events,
        },
        "counts": {
            "by_event_type": counts_json(&by_event_type),
            "by_action": counts_json(&by_action),
            "by_provider": counts_json(&by_provider),
            "by_model": counts_json(&by_model),
            "by_provider_boundary_class": counts_json(&by_boundary_class),
            "by_classification_source": counts_json(&by_classification_source),
            "by_sensitive_class": counts_json(&by_sensitive_class),
            "by_source_kind": counts_json(&by_source_kind),
            "by_policy_fingerprint": counts_json(&by_policy_fingerprint),
            "by_tenant": counts_json(&by_tenant),
        },
        "recent": recent,
    })
}

/// Newest-first, audit-safe summary of one boundary decision. Restates
/// only fields from the safe evidence shape — never message content.
fn recent_summary(event: &ProtectedAuditEnvelope) -> Value {
    json!({
        "event_id": &event.event_id,
        "event_type": &event.event_type,
        "created_at_ms": event.created_at_ms,
        "seq": event.seq,
        "action": payload_str(&event.payload, &["action"]),
        "provider_id": payload_str(&event.payload, &["provider", "provider_id"]),
        "model_id": payload_str(&event.payload, &["provider", "model_id"]),
        "boundary_class": payload_str(&event.payload, &["provider", "boundary_class"]),
        "source_kind": payload_str(&event.payload, &["sourceKind"]),
        "tenant": tenant_key(event),
        "payload_hash": payload_str(&event.payload, &["payload_hash"]),
        "policy_fingerprint": payload_str(&event.payload, &["policy_fingerprint"]),
        "total_findings": event
            .payload
            .get("finding_summary")
            .and_then(|summary| summary.get("total_findings"))
            .and_then(Value::as_u64),
        "reason_codes": event.payload.get("reason_codes").cloned().unwrap_or(Value::Null),
    })
}

fn payload_str<'a>(payload: &'a Value, path: &[&str]) -> Option<&'a str> {
    let mut value = payload;
    for key in path {
        value = value.get(key)?;
    }
    value.as_str()
}

fn tenant_key(event: &ProtectedAuditEnvelope) -> String {
    format!(
        "{}/{}/{}",
        event.tenant_context.org_id,
        event.tenant_context.workspace_id,
        event.tenant_context.deployment_id.as_deref().unwrap_or("-"),
    )
}

fn bump(map: &mut BTreeMap<String, u64>, key: Option<&str>) {
    if let Some(key) = key.filter(|key| !key.is_empty()) {
        *map.entry(key.to_string()).or_default() += 1;
    }
}

fn counts_json(map: &BTreeMap<String, u64>) -> Value {
    Value::Object(
        map.iter()
            .map(|(key, count)| (key.clone(), json!(count)))
            .collect::<Map<_, _>>(),
    )
}
