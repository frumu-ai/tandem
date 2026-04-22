use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::{json, Value};
use tandem_types::{RequestPrincipal, TenantContext};

use crate::automation_v2::governance::{
    AgentCreationReviewSummary, AgentSpendSummary, AgentSpendWindowRecord,
    AutomationGovernanceRecord, AutomationGrantRecord, AutomationLifecycleFinding,
    AutomationProvenanceRecord, GovernanceActorKind, GovernanceActorRef, GovernanceApprovalRequest,
    GovernanceError, GovernanceLineageEntry,
};

fn first_header(headers: &HeaderMap, names: &[&str]) -> Option<String> {
    for name in names {
        if let Some(value) = headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(value.to_string());
        }
    }
    None
}

pub(crate) fn resolve_governance_actor(
    headers: &HeaderMap,
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
) -> GovernanceActorRef {
    if let Some(agent_id) = first_header(headers, &["x-tandem-agent-id"]) {
        return GovernanceActorRef::agent(Some(agent_id), request_principal.source.clone());
    }
    let actor_id = tenant_context
        .actor_id
        .clone()
        .or_else(|| request_principal.actor_id.clone());
    if actor_id.is_some() {
        return GovernanceActorRef::human(actor_id, request_principal.source.clone());
    }
    GovernanceActorRef::system(request_principal.source.clone())
}

pub(crate) fn resolve_governance_provenance(
    headers: &HeaderMap,
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
) -> AutomationProvenanceRecord {
    let request_source = first_header(headers, &["x-tandem-request-source"])
        .or_else(|| Some(request_principal.source.clone()));
    let Some(agent_id) = first_header(headers, &["x-tandem-agent-id"]) else {
        return AutomationProvenanceRecord::human(
            tenant_context
                .actor_id
                .clone()
                .or_else(|| request_principal.actor_id.clone()),
            request_principal.source.clone(),
        );
    };

    let ancestor_chain = first_header(headers, &["x-tandem-agent-ancestor-ids"])
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .enumerate()
                .map(|(index, value)| GovernanceLineageEntry {
                    depth: (index + 1) as u64,
                    actor: GovernanceActorRef::agent(
                        Some(value.to_string()),
                        "ancestor_chain".to_string(),
                    ),
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let depth = ancestor_chain
        .last()
        .map(|entry| entry.depth.saturating_add(1))
        .unwrap_or(1);
    let root_actor = tenant_context
        .actor_id
        .clone()
        .or_else(|| request_principal.actor_id.clone())
        .map(|actor_id| GovernanceActorRef::human(Some(actor_id), request_principal.source.clone()))
        .unwrap_or_else(|| GovernanceActorRef::system(request_principal.source.clone()));
    AutomationProvenanceRecord {
        creator: GovernanceActorRef::agent(Some(agent_id), request_principal.source.clone()),
        root_actor,
        ancestor_chain,
        depth,
        request_source,
    }
}

pub(crate) fn governance_error_response(error: GovernanceError) -> (StatusCode, Json<Value>) {
    (
        error.status,
        Json(json!({
            "error": error.message,
            "code": error.code,
        })),
    )
}

pub(crate) fn automation_governance_wire(record: &AutomationGovernanceRecord) -> Value {
    json!({
        "automation_id": record.automation_id,
        "provenance": record.provenance,
        "declared_capabilities": record.declared_capabilities,
        "grant_count": record.modify_grants.len(),
        "capability_grant_count": record.capability_grants.len(),
        "published_externally": record.published_externally,
        "creation_paused": record.creation_paused,
        "review_required": record.review_required,
        "review_kind": record.review_kind,
        "review_requested_at_ms": record.review_requested_at_ms,
        "review_request_id": record.review_request_id,
        "last_reviewed_at_ms": record.last_reviewed_at_ms,
        "runs_since_review": record.runs_since_review,
        "expires_at_ms": record.expires_at_ms,
        "expired_at_ms": record.expired_at_ms,
        "retired_at_ms": record.retired_at_ms,
        "retire_reason": record.retire_reason,
        "paused_for_lifecycle": record.paused_for_lifecycle,
        "health_last_checked_at_ms": record.health_last_checked_at_ms,
        "health_findings": record
            .health_findings
            .iter()
            .map(automation_lifecycle_finding_wire)
            .collect::<Vec<_>>(),
        "deleted_at_ms": record.deleted_at_ms,
        "delete_retention_until_ms": record.delete_retention_until_ms,
    })
}

pub(crate) fn agent_creation_review_wire(record: &AgentCreationReviewSummary) -> Value {
    json!({
        "agent_id": record.agent_id,
        "created_since_review": record.created_since_review,
        "review_required": record.review_required,
        "review_kind": record.review_kind,
        "review_requested_at_ms": record.review_requested_at_ms,
        "review_request_id": record.review_request_id,
        "last_reviewed_at_ms": record.last_reviewed_at_ms,
        "last_review_notes": record.last_review_notes,
        "updated_at_ms": record.updated_at_ms,
    })
}

pub(crate) fn automation_lifecycle_finding_wire(record: &AutomationLifecycleFinding) -> Value {
    json!({
        "finding_id": record.finding_id,
        "kind": record.kind,
        "severity": record.severity,
        "summary": record.summary,
        "detail": record.detail,
        "observed_at_ms": record.observed_at_ms,
        "automation_run_id": record.automation_run_id,
        "approval_id": record.approval_id,
        "evidence": record.evidence,
    })
}

pub(crate) fn automation_lifecycle_summary_wire(record: &AutomationGovernanceRecord) -> Value {
    json!({
        "automation_id": record.automation_id,
        "review_required": record.review_required,
        "review_kind": record.review_kind,
        "review_requested_at_ms": record.review_requested_at_ms,
        "last_reviewed_at_ms": record.last_reviewed_at_ms,
        "runs_since_review": record.runs_since_review,
        "expires_at_ms": record.expires_at_ms,
        "expired_at_ms": record.expired_at_ms,
        "retired_at_ms": record.retired_at_ms,
        "retire_reason": record.retire_reason,
        "paused_for_lifecycle": record.paused_for_lifecycle,
        "health_last_checked_at_ms": record.health_last_checked_at_ms,
        "health_findings": record
            .health_findings
            .iter()
            .map(automation_lifecycle_finding_wire)
            .collect::<Vec<_>>(),
    })
}

pub(crate) fn automation_grant_wire(record: &AutomationGrantRecord) -> Value {
    json!({
        "grant_id": record.grant_id,
        "automation_id": record.automation_id,
        "grant_kind": record.grant_kind,
        "granted_to": record.granted_to,
        "granted_by": record.granted_by,
        "capability_key": record.capability_key,
        "created_at_ms": record.created_at_ms,
        "revoked_at_ms": record.revoked_at_ms,
        "revoke_reason": record.revoke_reason,
    })
}

pub(crate) fn approval_request_wire(record: &GovernanceApprovalRequest) -> Value {
    json!({
        "approval_id": record.approval_id,
        "request_type": record.request_type,
        "requested_by": record.requested_by,
        "target_resource": record.target_resource,
        "rationale": record.rationale,
        "context": record.context,
        "status": record.status,
        "expires_at_ms": record.expires_at_ms,
        "reviewed_by": record.reviewed_by,
        "reviewed_at_ms": record.reviewed_at_ms,
        "review_notes": record.review_notes,
        "created_at_ms": record.created_at_ms,
        "updated_at_ms": record.updated_at_ms,
    })
}

pub(crate) fn agent_spend_window_wire(record: &AgentSpendWindowRecord) -> Value {
    json!({
        "kind": record.kind,
        "window_start_ms": record.window_start_ms,
        "window_end_ms": record.window_end_ms,
        "prompt_tokens": record.prompt_tokens,
        "completion_tokens": record.completion_tokens,
        "total_tokens": record.total_tokens,
        "cost_usd": record.cost_usd,
        "last_automation_id": record.last_automation_id,
        "last_run_id": record.last_run_id,
        "updated_at_ms": record.updated_at_ms,
        "soft_warning_at_ms": record.soft_warning_at_ms,
        "hard_stop_at_ms": record.hard_stop_at_ms,
    })
}

pub(crate) fn agent_spend_wire(record: &AgentSpendSummary) -> Value {
    json!({
        "agent_id": record.agent_id,
        "daily": agent_spend_window_wire(&record.daily),
        "weekly": agent_spend_window_wire(&record.weekly),
        "monthly": agent_spend_window_wire(&record.monthly),
        "lifetime": agent_spend_window_wire(&record.lifetime),
        "updated_at_ms": record.updated_at_ms,
        "paused_at_ms": record.paused_at_ms,
        "pause_reason": record.pause_reason,
    })
}

pub(crate) fn requested_by_is_agent(requested_by: &GovernanceActorRef) -> bool {
    requested_by.kind == GovernanceActorKind::Agent
}
