//! Unified approval-request shape used by all surfaces.
//!
//! Tandem has multiple gate primitives in different subsystems
//! (`automation_v2` `AutomationPendingGate`, coder `ContextRunStatus::AwaitingApproval`,
//! mission-builder `HumanApprovalGate`). This module defines one common shape so
//! the control panel inbox, channel adapters (Slack/Discord/Telegram), and any
//! future surface can render and decide approvals against one type.
//!
//! Per-subsystem aggregators in `tandem-server` translate their native pending
//! state into this shape; the existing `automations_v2_run_gate_decide`
//! handler remains the authoritative decision endpoint for automation_v2 runs.
//!
//! Scope intentionally minimal for v1: identity + preview + decision. Routing
//! metadata (which channels/users were notified) lives separately on the
//! notification fan-out task.

use serde::{Deserialize, Serialize};

/// Which Tandem subsystem owns the underlying pending state.
///
/// Used by aggregator endpoints and decision routing to dispatch back to the
/// correct subsystem handler.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalSourceKind {
    /// `automation_v2` mission run with `awaiting_gate` set.
    AutomationV2,
    /// Coder context run in `AwaitingApproval` state.
    Coder,
    /// Workflow run paused on a `HumanApprovalGate` (future — not yet wired).
    Workflow,
}

/// The decision a human (or programmatic surface acting on their behalf) can make.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approve,
    Rework,
    Cancel,
}

impl ApprovalDecision {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApprovalDecision::Approve => "approve",
            ApprovalDecision::Rework => "rework",
            ApprovalDecision::Cancel => "cancel",
        }
    }
}

/// A unified pending-approval request surfaced from any subsystem.
///
/// Aggregators populate `surface_payload` with subsystem-specific routing info
/// (e.g. the `automation_v2` `run_id`, the `coder` `context_run_id`) so the
/// decision handler can dispatch back to the right subsystem without losing
/// information through the normalization step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// Stable identifier for this approval request. For automation_v2 runs this
    /// is `automation_v2:{run_id}:{node_id}`; for coder runs it is
    /// `coder:{context_run_id}`. Aggregators are responsible for stable IDs.
    pub request_id: String,

    /// Which subsystem produced this pending state.
    pub source: ApprovalSourceKind,

    /// Org/workspace/user scope for the request. Surfaces filter by tenant.
    pub tenant: ApprovalTenantRef,

    /// Origin run identifier (subsystem-specific).
    pub run_id: String,

    /// Workflow / mission / project node the gate is attached to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,

    /// Human-readable workflow name for display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_name: Option<String>,

    /// What the action will do, in one short phrase ("send email", "create CRM contact").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_kind: Option<String>,

    /// Markdown rendering of what is about to happen — visible to the approver.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_preview_markdown: Option<String>,

    /// Subsystem-specific routing payload. Surfaces should treat this as opaque
    /// JSON; the aggregator and the decision dispatcher cooperate on its shape.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub surface_payload: Option<serde_json::Value>,

    /// When the approval request was first created.
    pub requested_at_ms: u64,

    /// When it expires. Subsystem schedulers may auto-cancel, escalate, or
    /// remind according to their native gate policy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,

    /// Allowed decisions for this gate (mirrors `HumanApprovalGate.decisions`).
    #[serde(default)]
    pub decisions: Vec<ApprovalDecision>,

    /// Stage IDs a `Rework` decision can target (mirrors
    /// `HumanApprovalGate.rework_targets`).
    #[serde(default)]
    pub rework_targets: Vec<String>,

    /// Optional instructions to display to the approver.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,

    /// Set once the request has been decided. Pending requests have all four
    /// fields below as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<ApprovalActorRef>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_at_ms: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<ApprovalDecision>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rework_feedback: Option<String>,
}

/// Tenant scope reference. Lighter than the full `TenantContext` — surfaces
/// only need the IDs to filter and display.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalTenantRef {
    pub org_id: String,
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

/// Actor reference for `decided_by`. Lighter than the full `RequestPrincipal` —
/// captures the surface (slack, control_panel, etc.) and the surface-specific
/// user ID for display, plus the resolved engine actor for audit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalActorRef {
    /// Where the decision came from: `control_panel`, `slack`, `discord`,
    /// `telegram`, `api`, etc.
    pub surface: String,
    /// Surface-specific user identifier (Slack user ID, Discord user ID, etc.).
    pub surface_user_id: String,
    /// Display name when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Resolved engine principal ID (the canonical Tandem actor) when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
}

/// Decision input accepted by the unified `/approvals/{id}/decide` endpoint
/// (added in a later milestone). For v1, decisions go through subsystem
/// handlers (`/automations/v2/runs/{run_id}/gate_decide`,
/// `/coder/runs/{id}/approve`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalDecisionInput {
    pub decision: ApprovalDecision,
    /// Optional reason / rework feedback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Caller's identity for audit. The server resolves this against the
    /// authenticated principal and refuses if they disagree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<ApprovalActorRef>,
}

/// Filter parameters for `GET /approvals/pending`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApprovalListFilter {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ApprovalSourceKind>,
    /// Cap the number of results returned. Defaults to 100 server-side.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_decision_serializes_lowercase() {
        assert_eq!(
            serde_json::to_string(&ApprovalDecision::Approve).unwrap(),
            "\"approve\""
        );
        assert_eq!(
            serde_json::to_string(&ApprovalDecision::Rework).unwrap(),
            "\"rework\""
        );
        assert_eq!(
            serde_json::to_string(&ApprovalDecision::Cancel).unwrap(),
            "\"cancel\""
        );
    }

    #[test]
    fn approval_source_serializes_snake_case() {
        assert_eq!(
            serde_json::to_string(&ApprovalSourceKind::AutomationV2).unwrap(),
            "\"automation_v2\""
        );
        assert_eq!(
            serde_json::to_string(&ApprovalSourceKind::Coder).unwrap(),
            "\"coder\""
        );
        assert_eq!(
            serde_json::to_string(&ApprovalSourceKind::Workflow).unwrap(),
            "\"workflow\""
        );
    }

    #[test]
    fn approval_request_round_trips_minimal() {
        let request = ApprovalRequest {
            request_id: "automation_v2:run-1:node-2".to_string(),
            source: ApprovalSourceKind::AutomationV2,
            tenant: ApprovalTenantRef {
                org_id: "local-default-org".to_string(),
                workspace_id: "local-default-workspace".to_string(),
                user_id: None,
            },
            run_id: "run-1".to_string(),
            node_id: Some("node-2".to_string()),
            workflow_name: Some("sales-research-outreach".to_string()),
            action_kind: Some("send_email".to_string()),
            action_preview_markdown: Some("Will email **alice@example.com**".to_string()),
            surface_payload: Some(serde_json::json!({ "automation_v2_run_id": "run-1" })),
            requested_at_ms: 1_700_000_000_000,
            expires_at_ms: None,
            decisions: vec![
                ApprovalDecision::Approve,
                ApprovalDecision::Rework,
                ApprovalDecision::Cancel,
            ],
            rework_targets: vec!["draft-stage".to_string()],
            instructions: Some("Verify recipient is in the approved ICP.".to_string()),
            decided_by: None,
            decided_at_ms: None,
            decision: None,
            rework_feedback: None,
        };
        let json = serde_json::to_string(&request).unwrap();
        let parsed: ApprovalRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.request_id, request.request_id);
        assert_eq!(parsed.source, ApprovalSourceKind::AutomationV2);
        assert_eq!(parsed.decisions.len(), 3);
        assert!(parsed.decided_at_ms.is_none());
    }

    #[test]
    fn approval_decision_input_accepts_optional_reason() {
        let input: ApprovalDecisionInput = serde_json::from_str(
            r#"{"decision":"rework","reason":"please tighten the ICP filter"}"#,
        )
        .unwrap();
        assert_eq!(input.decision, ApprovalDecision::Rework);
        assert_eq!(
            input.reason.as_deref(),
            Some("please tighten the ICP filter")
        );
    }

    #[test]
    fn approval_list_filter_defaults_to_empty() {
        let filter = ApprovalListFilter::default();
        assert!(filter.org_id.is_none());
        assert!(filter.source.is_none());
        assert!(filter.limit.is_none());
    }
}
