// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

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

#[derive(Debug)]
pub(crate) struct HostedPolicyUnavailable;

impl std::fmt::Display for HostedPolicyUnavailable {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("incident_monitor_hosted_policy_unavailable")
    }
}

impl std::error::Error for HostedPolicyUnavailable {}

pub(crate) fn require_current_policy(state: &crate::AppState) -> anyhow::Result<()> {
    if !state.enterprise.hosted_policy.is_ready() {
        return Err(HostedPolicyUnavailable.into());
    }
    Ok(())
}

impl crate::AppState {
    // New work must recheck policy after the registry wait. Existing setters
    // remain available to durably record outcomes of already-admitted effects.
    pub(crate) async fn put_incident_monitor_draft_with_current_policy(
        &self,
        draft: crate::IncidentMonitorDraftRecord,
    ) -> anyhow::Result<crate::IncidentMonitorDraftRecord> {
        {
            let mut guard = self.incident_monitor_drafts.write().await;
            require_current_policy(self)?;
            guard.insert(draft.draft_id.clone(), draft.clone());
        }
        self.persist_incident_monitor_drafts().await?;
        Ok(draft)
    }

    pub(crate) async fn put_incident_monitor_incident_with_current_policy(
        &self,
        incident: crate::IncidentMonitorIncidentRecord,
    ) -> anyhow::Result<crate::IncidentMonitorIncidentRecord> {
        {
            let mut guard = self.incident_monitor_incidents.write().await;
            require_current_policy(self)?;
            guard.insert(incident.incident_id.clone(), incident.clone());
        }
        self.persist_incident_monitor_incidents().await?;
        Ok(incident)
    }
}

pub(crate) fn draft_tenant_context(
    draft: &crate::IncidentMonitorDraftRecord,
) -> tandem_types::TenantContext {
    let tenant_id = draft
        .tenant_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let workspace_id = draft
        .workspace_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match (tenant_id, workspace_id) {
        (Some(org_id), Some(workspace_id)) => {
            tandem_types::TenantContext::explicit(org_id, workspace_id, draft.actor.clone())
        }
        _ => tandem_types::TenantContext::local_implicit(),
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
    require_current_policy(state)?;
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
        None,
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

#[cfg(test)]
mod tests {
    use crate::IncidentMonitorDraftRecord;

    use super::draft_tenant_context;

    #[test]
    fn complete_draft_scope_produces_explicit_tenant_context() {
        let draft = IncidentMonitorDraftRecord {
            tenant_id: Some(" tenant-a ".to_string()),
            workspace_id: Some(" workspace-a ".to_string()),
            actor: Some("incident-monitor".to_string()),
            ..IncidentMonitorDraftRecord::default()
        };

        let tenant = draft_tenant_context(&draft);

        assert!(!tenant.is_local_implicit());
        assert_eq!(tenant.org_id, "tenant-a");
        assert_eq!(tenant.workspace_id, "workspace-a");
        assert_eq!(tenant.actor_id.as_deref(), Some("incident-monitor"));
    }

    #[test]
    fn partial_or_blank_draft_scope_falls_back_to_local_implicit() {
        let drafts = [
            IncidentMonitorDraftRecord {
                tenant_id: Some("tenant-a".to_string()),
                ..IncidentMonitorDraftRecord::default()
            },
            IncidentMonitorDraftRecord {
                workspace_id: Some("workspace-a".to_string()),
                ..IncidentMonitorDraftRecord::default()
            },
            IncidentMonitorDraftRecord {
                tenant_id: Some("tenant-a".to_string()),
                workspace_id: Some("   ".to_string()),
                ..IncidentMonitorDraftRecord::default()
            },
        ];

        assert!(drafts
            .iter()
            .map(draft_tenant_context)
            .all(|tenant| tenant.is_local_implicit()));
    }
}
