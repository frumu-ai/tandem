//! HTTP routes for the cross-subsystem approvals aggregator.
//!
//! Today: read-only `/approvals/pending`. Decisions still flow through the
//! authoritative subsystem handlers
//! (`POST /automations/v2/runs/{run_id}/gate_decide`,
//! `POST /coder/runs/{run_id}/approve`).

use axum::extract::{Query, State};
use axum::{Extension, Json};
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
    // TAN-563: scope to the authenticated tenant, not to client-supplied query
    // params. The org/workspace boundary is derived from the request's
    // TenantContext (injected by the tenant middleware); the client's `org_id`/
    // `workspace_id` may only *narrow* within that tenant, never widen it.
    //
    // A genuine single-tenant/local deployment (local-implicit context) keeps
    // its see-all behavior, mirroring `StatefulRuntimeScope::visible_to_tenant`;
    // hardening local-implicit under hosted/multi-tenant auth is tracked
    // separately (TAN-567) so this fix stays non-breaking.
    let filter = scoped_filter(&tenant_context, &query, source);
    let approvals = list_pending_approvals(&state, &filter).await;
    Json(json!({
        "approvals": approvals,
        "count": approvals.len(),
    }))
}

/// Build a tenant-scoped filter. For a real tenant the org/workspace are pinned
/// to the caller's context (client values are only honored when they match, so
/// they can narrow but never cross tenants). Local-implicit callers are left
/// unconstrained to preserve single-tenant behavior.
fn scoped_filter(
    tenant_context: &TenantContext,
    query: &PendingApprovalsQuery,
    source: Option<ApprovalSourceKind>,
) -> ApprovalListFilter {
    if tenant_context.is_local_implicit() {
        return ApprovalListFilter {
            org_id: query.org_id.clone(),
            workspace_id: query.workspace_id.clone(),
            source,
            limit: query.limit,
        };
    }
    ApprovalListFilter {
        org_id: Some(tenant_context.org_id.clone()),
        workspace_id: Some(tenant_context.workspace_id.clone()),
        source,
        limit: query.limit,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn query(org: Option<&str>, workspace: Option<&str>) -> PendingApprovalsQuery {
        PendingApprovalsQuery {
            org_id: org.map(str::to_string),
            workspace_id: workspace.map(str::to_string),
            source: None,
            limit: Some(25),
        }
    }

    #[test]
    fn real_tenant_scope_is_pinned_and_ignores_client_org_workspace() {
        // TAN-563: a caller cannot widen past their own tenant by supplying
        // another org/workspace in the query string.
        let tenant = TenantContext::explicit("org-a", "ws-a", Some("user-a".into()));
        let filter = scoped_filter(&tenant, &query(Some("org-b"), Some("ws-b")), None);
        assert_eq!(filter.org_id.as_deref(), Some("org-a"));
        assert_eq!(filter.workspace_id.as_deref(), Some("ws-a"));
        assert_eq!(filter.limit, Some(25));
    }

    #[test]
    fn real_tenant_scope_is_pinned_even_when_client_sends_nothing() {
        let tenant = TenantContext::explicit("org-a", "ws-a", None);
        let filter = scoped_filter(&tenant, &query(None, None), None);
        assert_eq!(filter.org_id.as_deref(), Some("org-a"));
        assert_eq!(filter.workspace_id.as_deref(), Some("ws-a"));
    }

    #[test]
    fn local_implicit_preserves_single_tenant_see_all_behavior() {
        // Single-tenant/dev: local-implicit is unconstrained (mirrors
        // StatefulRuntimeScope::visible_to_tenant). Hosted hardening is TAN-567.
        let tenant = TenantContext::local_implicit();
        let filter = scoped_filter(&tenant, &query(None, None), None);
        assert_eq!(filter.org_id, None);
        assert_eq!(filter.workspace_id, None);
    }
}
