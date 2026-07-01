use serde_json::{json, Value};
use std::path::PathBuf;

use tandem_incident_monitor::github::{GithubToolSet, IncidentMonitorGithubHost};
use tandem_runtime::mcp_ready::{EnsureReadyPolicy, McpReadyError};
use tandem_runtime::McpRemoteTool;
use tandem_types::{EngineEvent, ToolResult};

use crate::{
    AppState, ExternalActionRecord, IncidentMonitorConfig, IncidentMonitorDraftRecord,
    IncidentMonitorIncidentRecord, IncidentMonitorPostRecord, IncidentMonitorStatus,
};

pub use tandem_incident_monitor::github::{
    publish_draft, record_post_failure, GithubDestinationContext, PublishMode, PublishOutcome,
};

const INCIDENT_MONITOR_LABEL: &str = "incident-monitor";

#[async_trait::async_trait]
impl IncidentMonitorGithubHost for AppState {
    async fn incident_monitor_status_snapshot(&self) -> IncidentMonitorStatus {
        AppState::incident_monitor_status_snapshot(self).await
    }

    async fn get_incident_monitor_draft(
        &self,
        draft_id: &str,
    ) -> Option<IncidentMonitorDraftRecord> {
        AppState::get_incident_monitor_draft(self, draft_id).await
    }

    async fn put_incident_monitor_draft(
        &self,
        draft: IncidentMonitorDraftRecord,
    ) -> anyhow::Result<IncidentMonitorDraftRecord> {
        AppState::put_incident_monitor_draft(self, draft).await
    }

    async fn get_incident_monitor_incident(
        &self,
        incident_id: &str,
    ) -> Option<IncidentMonitorIncidentRecord> {
        AppState::get_incident_monitor_incident(self, incident_id).await
    }

    async fn put_incident_monitor_post(
        &self,
        post: IncidentMonitorPostRecord,
    ) -> anyhow::Result<IncidentMonitorPostRecord> {
        AppState::put_incident_monitor_post(self, post).await
    }

    async fn list_incident_monitor_posts(&self, limit: usize) -> Vec<IncidentMonitorPostRecord> {
        AppState::list_incident_monitor_posts(self, limit).await
    }

    async fn list_incident_monitor_posts_by_draft(
        &self,
        draft_id: &str,
    ) -> Vec<IncidentMonitorPostRecord> {
        let mut rows = self
            .incident_monitor_posts
            .read()
            .await
            .values()
            .filter(|post| post.draft_id == draft_id)
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by_key(|post| std::cmp::Reverse(post.updated_at_ms));
        rows
    }

    async fn list_incident_monitor_posts_by_fingerprint(
        &self,
        repo: &str,
        fingerprint: &str,
    ) -> Vec<IncidentMonitorPostRecord> {
        let mut rows = self
            .incident_monitor_posts
            .read()
            .await
            .values()
            .filter(|post| post.repo == repo && post.fingerprint == fingerprint)
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by_key(|post| std::cmp::Reverse(post.updated_at_ms));
        rows
    }

    async fn list_incident_monitor_posts_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Vec<IncidentMonitorPostRecord> {
        let mut rows = self
            .incident_monitor_posts
            .read()
            .await
            .values()
            .filter(|post| post.idempotency_key == idempotency_key)
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by_key(|post| std::cmp::Reverse(post.updated_at_ms));
        rows
    }

    async fn try_claim_incident_monitor_post_idempotency(
        &self,
        post: IncidentMonitorPostRecord,
    ) -> anyhow::Result<(bool, IncidentMonitorPostRecord)> {
        AppState::try_claim_incident_monitor_post_idempotency(self, post).await
    }

    async fn mirror_incident_monitor_post_as_external_action(
        &self,
        draft: &IncidentMonitorDraftRecord,
        post: &IncidentMonitorPostRecord,
    ) {
        let capability_id = match post.operation.as_str() {
            "comment_issue" => Some("github.comment_on_issue".to_string()),
            "create_issue" => Some("github.create_issue".to_string()),
            _ => None,
        };
        let action = ExternalActionRecord {
            action_id: post.post_id.clone(),
            operation: post.operation.clone(),
            status: post.status.clone(),
            source_kind: Some("incident_monitor".to_string()),
            source_id: Some(draft.draft_id.clone()),
            routine_run_id: None,
            context_run_id: draft.triage_run_id.clone(),
            capability_id,
            provider: Some(INCIDENT_MONITOR_LABEL.to_string()),
            target: Some(
                post.target_ref
                    .clone()
                    .unwrap_or_else(|| draft.repo.clone()),
            ),
            approval_state: Some(if draft.status.eq_ignore_ascii_case("approval_required") {
                "approval_required".to_string()
            } else {
                "executed".to_string()
            }),
            idempotency_key: Some(post.idempotency_key.clone()),
            receipt: Some(json!({
                "post_id": post.post_id,
                "draft_id": post.draft_id,
                "incident_id": post.incident_id,
                "destination_id": post.destination_id,
                "destination_kind": post.destination_kind,
                "route_id": post.route_id,
                "route_match_reason": post.route_match_reason,
                "issue_number": post.issue_number,
                "issue_url": post.issue_url,
                "comment_id": post.comment_id,
                "comment_url": post.comment_url,
                "external_id": post.external_id,
                "external_url": post.external_url,
                "external_title": post.external_title,
                "target_ref": post.target_ref,
                "response_excerpt": post.response_excerpt,
            })),
            error: post.error.clone(),
            metadata: Some(json!({
                "repo": post.repo,
                "destination_id": post.destination_id,
                "destination_kind": post.destination_kind,
                "route_id": post.route_id,
                "route_match_reason": post.route_match_reason,
                "target_ref": post.target_ref,
                "fingerprint": post.fingerprint,
                "evidence_digest": post.evidence_digest,
                "confidence": post.confidence,
                "risk_level": post.risk_level,
                "risk_category": draft.risk_category,
                "actor": draft.actor,
                "model": draft.model,
                "tool_name": draft.tool_name,
                "action": draft.action,
                "policy": draft.policy,
                "approval_state": draft.approval_state,
                "blast_radius": draft.blast_radius,
                "external_correlation_ids": draft.external_correlation_ids,
                "expected_destination": post.expected_destination,
                "evidence_refs": post.evidence_refs,
                "quality_gate": post.quality_gate,
                "incident_monitor_operation": post.operation,
            })),
            created_at_ms: post.created_at_ms,
            updated_at_ms: post.updated_at_ms,
        };
        if let Err(error) = AppState::record_external_action(self, action).await {
            tracing::warn!(
                "failed to persist external action mirror for incident monitor post {}: {}",
                post.post_id,
                error
            );
        }
    }

    async fn update_last_post_result(&self, result: String) {
        self.update_incident_monitor_runtime_status(|runtime| {
            runtime.last_post_result = Some(result);
        })
        .await;
    }

    fn publish_event(&self, event: EngineEvent) {
        self.event_bus.publish(event);
    }

    async fn ensure_incident_monitor_issue_draft(
        &self,
        draft_id: &str,
        force: bool,
    ) -> anyhow::Result<Value> {
        crate::http::incident_monitor::ensure_incident_monitor_issue_draft(
            self.clone(),
            draft_id,
            force,
        )
        .await
    }

    async fn load_incident_monitor_issue_draft_artifact(
        &self,
        triage_run_id: &str,
    ) -> Option<Value> {
        crate::http::incident_monitor::load_incident_monitor_issue_draft_artifact(
            self,
            triage_run_id,
        )
        .await
    }

    async fn resolve_github_tool_set(
        &self,
        config: &IncidentMonitorConfig,
    ) -> anyhow::Result<GithubToolSet> {
        resolve_github_tool_set_for_state(self, config).await
    }

    async fn call_mcp_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        payload: Value,
    ) -> anyhow::Result<ToolResult> {
        self.mcp
            .call_tool(server_name, tool_name, payload)
            .await
            .map_err(anyhow::Error::msg)
    }

    fn context_run_events_path(&self, run_id: &str) -> PathBuf {
        crate::http::context_runs::context_run_events_path(self, run_id)
    }
}

async fn resolve_github_tool_set_for_state(
    state: &AppState,
    config: &IncidentMonitorConfig,
) -> anyhow::Result<GithubToolSet> {
    let server_name = config
        .mcp_server
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("Incident Monitor MCP server is not configured"))?
        .to_string();
    state
        .mcp
        .ensure_ready(&server_name, EnsureReadyPolicy::with_retries(3, 750))
        .await
        .map_err(|error| match error {
            McpReadyError::NotFound => {
                anyhow::anyhow!("Incident Monitor MCP server `{server_name}` was not found")
            }
            McpReadyError::Disabled => {
                anyhow::anyhow!("Incident Monitor MCP server `{server_name}` is disabled")
            }
            McpReadyError::PermanentlyFailed { last_error } => {
                let detail = last_error.unwrap_or_else(|| "connect failed".to_string());
                anyhow::anyhow!(
                    "Incident Monitor MCP server `{server_name}` was not ready for GitHub publish: {detail}"
                )
            }
        })?;
    let server_tools = state.mcp.server_tools(&server_name).await;
    if server_tools.is_empty() {
        anyhow::bail!("no MCP tools were discovered for selected Incident Monitor server");
    }
    let discovered = state
        .capability_resolver
        .discover_from_runtime(server_tools.clone(), Vec::new())
        .await;
    let mut resolved = state
        .capability_resolver
        .resolve(
            crate::capability_resolver::CapabilityResolveInput {
                workflow_id: Some("incident-monitor-github".to_string()),
                required_capabilities: vec![
                    "github.list_issues".to_string(),
                    "github.get_issue".to_string(),
                    "github.create_issue".to_string(),
                    "github.comment_on_issue".to_string(),
                ],
                optional_capabilities: Vec::new(),
                provider_preference: vec!["mcp".to_string()],
                available_tools: discovered,
            },
            Vec::new(),
        )
        .await?;
    if !resolved.missing_required.is_empty() {
        let _ = state.capability_resolver.refresh_builtin_bindings().await;
        let discovered = state
            .capability_resolver
            .discover_from_runtime(server_tools.clone(), Vec::new())
            .await;
        resolved = state
            .capability_resolver
            .resolve(
                crate::capability_resolver::CapabilityResolveInput {
                    workflow_id: Some("incident-monitor-github".to_string()),
                    required_capabilities: vec![
                        "github.list_issues".to_string(),
                        "github.get_issue".to_string(),
                        "github.create_issue".to_string(),
                        "github.comment_on_issue".to_string(),
                    ],
                    optional_capabilities: Vec::new(),
                    provider_preference: vec!["mcp".to_string()],
                    available_tools: discovered,
                },
                Vec::new(),
            )
            .await?;
    }
    let tool_name = |capability_id: &str| -> anyhow::Result<String> {
        let namespaced = resolved
            .resolved
            .iter()
            .find(|row| row.capability_id == capability_id)
            .map(|row| row.tool_name.clone())
            .ok_or_else(|| anyhow::anyhow!("missing resolved tool for {capability_id}"))?;
        map_namespaced_to_raw_tool(&server_tools, &namespaced)
    };
    let direct_tool_name_fallback = |candidates: &[&str]| -> Option<String> {
        server_tools
            .iter()
            .find(|row| {
                candidates.iter().any(|candidate| {
                    row.tool_name.eq_ignore_ascii_case(candidate)
                        || row.namespaced_name.eq_ignore_ascii_case(candidate)
                })
            })
            .map(|row| row.tool_name.clone())
    };
    let list_issues = tool_name("github.list_issues").or_else(|_| {
        direct_tool_name_fallback(&[
            "list_issues",
            "list_repository_issues",
            "mcp.github.list_issues",
            "mcp.githubcopilot.list_issues",
        ])
        .ok_or_else(|| anyhow::anyhow!("missing resolved tool for github.list_issues"))
    })?;
    let get_issue = tool_name("github.get_issue").or_else(|_| {
        direct_tool_name_fallback(&[
            "get_issue",
            "issue_read",
            "mcp.github.get_issue",
            "mcp.github.issue_read",
            "mcp.githubcopilot.issue_read",
        ])
        .ok_or_else(|| anyhow::anyhow!("missing resolved tool for github.get_issue"))
    })?;
    let create_issue = tool_name("github.create_issue").or_else(|_| {
        direct_tool_name_fallback(&[
            "create_issue",
            "issue_write",
            "mcp.github.create_issue",
            "mcp.github.issue_write",
            "mcp.githubcopilot.issue_write",
        ])
        .ok_or_else(|| anyhow::anyhow!("missing resolved tool for github.create_issue"))
    })?;
    let comment_on_issue = tool_name("github.comment_on_issue").or_else(|_| {
        direct_tool_name_fallback(&[
            "add_issue_comment",
            "create_issue_comment",
            "mcp.github.add_issue_comment",
            "mcp.github.create_issue_comment",
            "mcp.githubcopilot.add_issue_comment",
            "github.comment_on_issue",
        ])
        .ok_or_else(|| anyhow::anyhow!("missing resolved tool for github.comment_on_issue"))
    })?;
    Ok(GithubToolSet {
        server_name,
        list_issues,
        get_issue,
        create_issue,
        comment_on_issue,
    })
}

fn map_namespaced_to_raw_tool(
    tools: &[McpRemoteTool],
    namespaced_name: &str,
) -> anyhow::Result<String> {
    tools
        .iter()
        .find(|row| row.namespaced_name == namespaced_name)
        .map(|row| row.tool_name.clone())
        .ok_or_else(|| anyhow::anyhow!("failed to map MCP tool `{namespaced_name}` to raw tool"))
}
