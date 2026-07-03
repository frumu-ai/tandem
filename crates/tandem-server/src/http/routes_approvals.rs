//! HTTP routes for the cross-subsystem approvals aggregator.
//!
//! Today: read-only `/approvals/pending`. Decisions still flow through the
//! authoritative subsystem handlers
//! (`POST /automations/v2/runs/{run_id}/gate_decide`,
//! `POST /coder/runs/{run_id}/approve`).

use axum::extract::{Query, State};
use axum::Extension;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use tandem_types::{ApprovalListFilter, ApprovalSourceKind, TenantContext};

use super::approvals::list_pending_approvals;
use crate::AppState;

#[derive(Debug, Default, Deserialize)]
pub(super) struct PendingApprovalsQuery {
    #[serde(default)]
    pub org_id: Option<String>,
    #[serde(default)]
    pub workspace_id: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub limit: Option<u32>,
}

pub(super) async fn approvals_pending_list(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Query(query): Query<PendingApprovalsQuery>,
) -> Json<Value> {
    let source = query.source.as_deref().and_then(parse_source);
    let query_scope_matches = query
        .org_id
        .as_deref()
        .map(|org_id| org_id == tenant_context.org_id)
        .unwrap_or(true)
        && query
            .workspace_id
            .as_deref()
            .map(|workspace_id| workspace_id == tenant_context.workspace_id)
            .unwrap_or(true);
    let filter = ApprovalListFilter {
        org_id: Some(tenant_context.org_id),
        workspace_id: Some(tenant_context.workspace_id),
        source,
        limit: query.limit,
    };
    let approvals = if query_scope_matches {
        list_pending_approvals(&state, &filter).await
    } else {
        Vec::new()
    };
    Json(json!({
        "approvals": approvals,
        "count": approvals.len(),
    }))
}

fn parse_source(raw: &str) -> Option<ApprovalSourceKind> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "automation_v2" | "automationv2" => Some(ApprovalSourceKind::AutomationV2),
        "coder" => Some(ApprovalSourceKind::Coder),
        "workflow" => Some(ApprovalSourceKind::Workflow),
        _ => None,
    }
}

pub(super) fn apply(router: axum::Router<AppState>) -> axum::Router<AppState> {
    router.route(
        "/approvals/pending",
        axum::routing::get(approvals_pending_list),
    )
}
