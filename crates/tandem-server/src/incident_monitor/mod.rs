pub use tandem_incident_monitor::{
    comment_summary, error_provenance, governance_metrics, log_parser, reassessment, scenarios,
    types,
};
pub mod log_artifacts;
pub mod log_watcher;
pub mod router;
pub mod safety_context;
pub mod service;
pub mod source_readiness;

pub(crate) fn draft_tenant_context(
    draft: &crate::IncidentMonitorDraftRecord,
) -> tandem_types::TenantContext {
    match draft
        .tenant_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        Some(org_id) => tandem_types::TenantContext::explicit(
            org_id,
            draft
                .workspace_id
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .unwrap_or(org_id),
            draft.actor.clone(),
        ),
        None => tandem_types::TenantContext::local_implicit(),
    }
}

pub(crate) async fn dispatch_mcp_tool(
    state: &crate::AppState,
    draft: &crate::IncidentMonitorDraftRecord,
    server_name: &str,
    tool_name: &str,
    args: serde_json::Value,
    operation: &str,
) -> anyhow::Result<tandem_types::ToolResult> {
    let mut source = tandem_tools::ToolDispatchSource::new("incident_monitor_destination")
        .request(format!("{}:{operation}", draft.draft_id));
    if let Some(run_id) = draft.triage_run_id.as_deref() {
        source = source.run(run_id);
    }
    crate::http::mcp::dispatch_mcp_tool_for_tenant(
        state,
        server_name,
        tool_name,
        args,
        draft_tenant_context(draft),
        source,
    )
    .await
}

pub(crate) fn source_identity_matches_draft(
    draft: &crate::IncidentMonitorDraftRecord,
    submission: &crate::IncidentMonitorSubmission,
) -> bool {
    let draft_project = draft.project_id.as_deref();
    let draft_source = draft.log_source_id.as_deref();
    let submission_project = submission.project_id.as_deref();
    let submission_source = submission.log_source_id.as_deref();
    let source_bound = draft_project.is_some()
        || draft_source.is_some()
        || submission_project.is_some()
        || submission_source.is_some();
    !source_bound || (draft_project == submission_project && draft_source == submission_source)
}
