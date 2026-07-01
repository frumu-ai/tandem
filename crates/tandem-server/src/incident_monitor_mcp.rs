use std::collections::HashMap;

use anyhow::Context;
use serde_json::{json, Map, Value};
use tandem_runtime::mcp_ready::{EnsureReadyPolicy, McpReadyError};
use tandem_runtime::{McpRemoteTool, McpServer};
use tandem_types::{EngineEvent, ToolResult};

use crate::{
    now_ms, sha256_hex, truncate_text, AppState, ExternalActionRecord, IncidentMonitorConfig,
    IncidentMonitorDestinationConfig, IncidentMonitorDestinationKind, IncidentMonitorDraftRecord,
    IncidentMonitorIncidentRecord, IncidentMonitorPostRecord,
};

pub use crate::incident_monitor_github::{PublishMode, PublishOutcome};

const MCP_TOOL_OPERATION: &str = "call_mcp_tool";
const DEFAULT_RESULT_EXCERPT_LIMIT: usize = 1_000;

#[derive(Debug, Clone)]
pub struct McpToolDestinationContext {
    pub destination_id: String,
    pub route_id: Option<String>,
    pub route_match_reason: Option<String>,
    pub mcp_server: Option<String>,
    pub mcp_tool: Option<String>,
    pub config: Option<Value>,
}

impl McpToolDestinationContext {
    fn route_match_reason(&self) -> Option<String> {
        self.route_match_reason
            .clone()
            .or_else(|| Some("destination_router".to_string()))
    }

    fn configured_server<'a>(&'a self, config: &'a IncidentMonitorConfig) -> Option<&'a str> {
        self.mcp_server
            .as_deref()
            .and_then(normalize_config_str)
            .or_else(|| config.mcp_server.as_deref().and_then(normalize_config_str))
            .or_else(|| config_string(self.config.as_ref(), &["mcp_server", "server"]))
    }

    fn configured_tool(&self) -> Option<&str> {
        self.mcp_tool
            .as_deref()
            .and_then(normalize_config_str)
            .or_else(|| config_string(self.config.as_ref(), &["mcp_tool", "tool", "tool_name"]))
    }
}

#[derive(Debug, Clone)]
struct ResolvedMcpToolDestination {
    server_name: String,
    tool: McpRemoteTool,
}

struct MappingContext<'a> {
    draft: &'a IncidentMonitorDraftRecord,
    incident: Option<&'a IncidentMonitorIncidentRecord>,
    destination: &'a McpToolDestinationContext,
    resolved: &'a ResolvedMcpToolDestination,
    target_ref: &'a str,
    evidence_digest: &'a str,
    idempotency_key: &'a str,
}

pub(crate) fn mcp_tool_destination_readiness(
    config: &IncidentMonitorConfig,
    destination: &IncidentMonitorDestinationConfig,
    servers: &HashMap<String, McpServer>,
) -> (bool, Vec<String>, Option<String>) {
    let mut missing = Vec::new();
    let context = McpToolDestinationContext {
        destination_id: destination.destination_id.clone(),
        route_id: None,
        route_match_reason: None,
        mcp_server: destination.mcp_server.clone(),
        mcp_tool: destination.mcp_tool.clone(),
        config: destination.config.clone(),
    };

    if !mcp_publish_allowed(destination.config.as_ref()) {
        missing.push("MCP publish is not explicitly allowed".to_string());
    }
    if let Err(reason) = payload_mapping(destination.config.as_ref()) {
        missing.push(reason);
    }

    let server_name = context.configured_server(config);
    let server = server_name.and_then(|name| servers.get(name));
    if server_name.is_none() {
        missing.push("MCP server is missing".to_string());
    } else if server.is_none() {
        missing.push("MCP server is not configured".to_string());
    } else if !server.as_ref().is_some_and(|row| row.enabled) {
        missing.push("MCP server is disabled".to_string());
    } else if !server.as_ref().is_some_and(|row| row.connected) {
        missing.push("MCP server is disconnected".to_string());
    }

    let tool_name = context.configured_tool();
    if tool_name.is_none() {
        missing.push("MCP tool is missing".to_string());
    } else if let (Some(server), Some(tool_name)) = (server, tool_name) {
        if server.enabled && server.connected && !server_has_mcp_tool(server, tool_name) {
            missing.push("MCP tool is not available".to_string());
        }
    }

    let detail = (!missing.is_empty()).then(|| {
        "MCP tool destination requires an enabled server, discovered allowlisted tool, explicit allow_publish flag, and non-empty payload mapping".to_string()
    });
    (missing.is_empty(), missing, detail)
}

pub async fn publish_draft(
    state: &AppState,
    draft_id: &str,
    incident_id: Option<&str>,
    mode: PublishMode,
    destination: McpToolDestinationContext,
) -> anyhow::Result<PublishOutcome> {
    let status = state.incident_monitor_status_snapshot().await;
    let config = status.config.clone();
    validate_mcp_publish_config(&config, mode, &destination)?;

    let mut draft = state
        .get_incident_monitor_draft(draft_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Incident Monitor draft not found"))?;
    if draft.status.eq_ignore_ascii_case("denied") {
        anyhow::bail!("Incident Monitor draft has been denied");
    }
    if mode == PublishMode::Auto
        && config.require_approval_for_new_issues
        && draft.status.eq_ignore_ascii_case("approval_required")
    {
        return Ok(PublishOutcome {
            action: "approval_required".to_string(),
            draft,
            post: None,
        });
    }

    let resolved = resolve_mcp_tool_for_state(state, &config, &destination).await?;
    let target_ref = target_ref(&resolved);
    let incident = match incident_id {
        Some(id) => state.get_incident_monitor_incident(id).await,
        None => None,
    };
    let evidence_digest = compute_evidence_digest(&draft);
    draft.evidence_digest = Some(evidence_digest.clone());

    if mode == PublishMode::RecheckOnly {
        if let Some(existing) = successful_post_for_draft(
            state,
            &draft.draft_id,
            &destination.destination_id,
            &target_ref,
            Some(&evidence_digest),
        )
        .await
        {
            apply_existing_mcp_tool_post_to_draft(&mut draft, &existing);
            let draft = state.put_incident_monitor_draft(draft).await?;
            return Ok(PublishOutcome {
                action: "mcp_tool_record_found".to_string(),
                draft,
                post: None,
            });
        }
        let draft = state.put_incident_monitor_draft(draft).await?;
        return Ok(PublishOutcome {
            action: "no_match".to_string(),
            draft,
            post: None,
        });
    }

    if let Some(existing) = successful_post_for_draft(
        state,
        &draft.draft_id,
        &destination.destination_id,
        &target_ref,
        Some(&evidence_digest),
    )
    .await
    {
        apply_existing_mcp_tool_post_to_draft(&mut draft, &existing);
        mirror_mcp_tool_post_as_external_action(state, &draft, &existing).await;
        let draft = state.put_incident_monitor_draft(draft).await?;
        return Ok(PublishOutcome {
            action: "skip_duplicate".to_string(),
            draft,
            post: Some(existing),
        });
    }

    if !matches!(mode, PublishMode::ManualPublish) {
        if let Some(previous) = latest_failed_mcp_tool_post_for_draft(
            state,
            &draft,
            &destination.destination_id,
            &target_ref,
            &evidence_digest,
        )
        .await
        {
            let detail = format!(
                "suppressed MCP tool publish for fingerprint {} after previous tool call {} failed",
                draft.fingerprint, previous.post_id
            );
            draft.status = "mcp_tool_failed".to_string();
            draft.github_status = Some("mcp_tool_failed".to_string());
            draft.last_post_error = Some(truncate_text(&detail, 500));
            let draft = state.put_incident_monitor_draft(draft).await?;
            return Ok(PublishOutcome {
                action: "mcp_tool_retry_suppressed".to_string(),
                draft,
                post: Some(previous),
            });
        }
    }

    let idempotency_key = build_idempotency_key(
        &destination.destination_id,
        &target_ref,
        &draft.fingerprint,
        MCP_TOOL_OPERATION,
        &evidence_digest,
    );
    if let Some(existing) = successful_post_by_idempotency(state, &idempotency_key).await {
        apply_existing_mcp_tool_post_to_draft(&mut draft, &existing);
        mirror_mcp_tool_post_as_external_action(state, &draft, &existing).await;
        let draft = state.put_incident_monitor_draft(draft).await?;
        return Ok(PublishOutcome {
            action: "skip_duplicate".to_string(),
            draft,
            post: Some(existing),
        });
    }

    let record_id = deterministic_record_id(&destination, &target_ref, &draft, &evidence_digest);
    let claim = pending_mcp_tool_post(
        &draft,
        incident.as_ref(),
        &destination,
        &resolved,
        &target_ref,
        &record_id,
        &idempotency_key,
        &evidence_digest,
    );
    let (claimed, existing_claim) = state
        .try_claim_incident_monitor_post_idempotency(claim)
        .await?;
    if !claimed {
        if existing_claim.status == "posted" {
            apply_existing_mcp_tool_post_to_draft(&mut draft, &existing_claim);
            mirror_mcp_tool_post_as_external_action(state, &draft, &existing_claim).await;
            let draft = state.put_incident_monitor_draft(draft).await?;
            return Ok(PublishOutcome {
                action: "skip_duplicate".to_string(),
                draft,
                post: Some(existing_claim),
            });
        }
        draft.github_status = Some("mcp_tool_calling".to_string());
        draft.last_post_error = Some(
            "another Incident Monitor publisher already claimed this MCP tool idempotency key"
                .to_string(),
        );
        return Ok(PublishOutcome {
            action: "publish_in_progress".to_string(),
            draft,
            post: Some(existing_claim),
        });
    }

    let mapping = payload_mapping(destination.config.as_ref()).map_err(anyhow::Error::msg)?;
    let args = match render_payload_mapping(
        mapping,
        &MappingContext {
            draft: &draft,
            incident: incident.as_ref(),
            destination: &destination,
            resolved: &resolved,
            target_ref: &target_ref,
            evidence_digest: &evidence_digest,
            idempotency_key: &idempotency_key,
        },
    )
    .context("render MCP tool destination payload mapping")
    {
        Ok(args) => args,
        Err(error) => {
            let error_text = truncate_text(&safe_result_excerpt(&format!("{error:#}")), 500);
            let failed = failed_mcp_tool_post(
                existing_claim,
                &destination,
                &resolved,
                &target_ref,
                &record_id,
                &Value::Object(Map::new()),
                &error_text,
            );
            let _ = state.put_incident_monitor_post(failed).await;
            draft.status = "mcp_tool_failed".to_string();
            draft.github_status = Some("mcp_tool_failed".to_string());
            draft.last_post_error = Some(error_text.clone());
            let _ = state.put_incident_monitor_draft(draft).await;
            return Err(anyhow::anyhow!(error_text));
        }
    };

    let call_result = state
        .mcp
        .call_tool(
            &resolved.server_name,
            &resolved.tool.tool_name,
            args.clone(),
        )
        .await;
    match call_result {
        Ok(result) => {
            if mcp_auth_required(&result) {
                let error_text =
                    "MCP authorization required before Incident Monitor MCP tool can execute"
                        .to_string();
                let blocked = blocked_mcp_tool_post(
                    existing_claim,
                    &destination,
                    &resolved,
                    &target_ref,
                    &record_id,
                    &args,
                    &result,
                    &error_text,
                );
                let _ = state.put_incident_monitor_post(blocked).await;
                draft.status = "mcp_tool_auth_required".to_string();
                draft.github_status = Some("mcp_tool_auth_required".to_string());
                draft.last_post_error = Some(error_text.clone());
                let _ = state.put_incident_monitor_draft(draft).await;
                return Err(anyhow::anyhow!(error_text));
            }
            let post = posted_mcp_tool_post(
                existing_claim,
                &destination,
                &resolved,
                &target_ref,
                &record_id,
                &args,
                &result,
            );
            let post = state.put_incident_monitor_post(post).await?;
            mirror_mcp_tool_post_as_external_action(state, &draft, &post).await;
            apply_existing_mcp_tool_post_to_draft(&mut draft, &post);
            let draft = state.put_incident_monitor_draft(draft).await?;
            state
                .update_incident_monitor_runtime_status(|runtime| {
                    runtime.last_post_result = Some(format!(
                        "called MCP tool {}",
                        post.external_title
                            .as_deref()
                            .unwrap_or(resolved.tool.namespaced_name.as_str())
                    ));
                })
                .await;
            state.event_bus.publish(EngineEvent::new(
                "incident_monitor.mcp_tool.called",
                json!({
                    "draft_id": draft.draft_id,
                    "repo": draft.repo,
                    "target_ref": target_ref,
                    "destination_id": destination.destination_id,
                    "server": resolved.server_name,
                    "tool": resolved.tool.tool_name,
                    "namespaced_tool": resolved.tool.namespaced_name,
                    "external_id": post.external_id,
                }),
            ));
            Ok(PublishOutcome {
                action: MCP_TOOL_OPERATION.to_string(),
                draft,
                post: Some(post),
            })
        }
        Err(error) => {
            let error_text = truncate_text(&safe_result_excerpt(&error), 500);
            let failed = failed_mcp_tool_post(
                existing_claim,
                &destination,
                &resolved,
                &target_ref,
                &record_id,
                &args,
                &error_text,
            );
            let _ = state.put_incident_monitor_post(failed).await;
            draft.status = "mcp_tool_failed".to_string();
            draft.github_status = Some("mcp_tool_failed".to_string());
            draft.last_post_error = Some(error_text.clone());
            let _ = state.put_incident_monitor_draft(draft).await;
            Err(anyhow::anyhow!(error_text)).with_context(|| {
                format!(
                    "call Incident Monitor MCP tool {} on server {}",
                    resolved.tool.tool_name, resolved.server_name
                )
            })
        }
    }
}

fn validate_mcp_publish_config(
    config: &IncidentMonitorConfig,
    mode: PublishMode,
    destination: &McpToolDestinationContext,
) -> anyhow::Result<()> {
    if !config.enabled {
        anyhow::bail!("Incident Monitor is disabled");
    }
    if config.paused && matches!(mode, PublishMode::Auto | PublishMode::Recovery) {
        anyhow::bail!("Incident Monitor is paused");
    }
    if !mcp_publish_allowed(destination.config.as_ref()) {
        anyhow::bail!("MCP publish is not explicitly allowed for this destination");
    }
    payload_mapping(destination.config.as_ref()).map_err(anyhow::Error::msg)?;
    destination
        .configured_server(config)
        .ok_or_else(|| anyhow::anyhow!("MCP destination server is missing"))?;
    destination
        .configured_tool()
        .ok_or_else(|| anyhow::anyhow!("MCP destination tool is missing"))?;
    Ok(())
}

async fn resolve_mcp_tool_for_state(
    state: &AppState,
    config: &IncidentMonitorConfig,
    destination: &McpToolDestinationContext,
) -> anyhow::Result<ResolvedMcpToolDestination> {
    let server_name = destination
        .configured_server(config)
        .ok_or_else(|| anyhow::anyhow!("MCP destination server is missing"))?
        .to_string();
    state
        .mcp
        .ensure_ready(&server_name, EnsureReadyPolicy::with_retries(3, 750))
        .await
        .map_err(|error| match error {
            McpReadyError::NotFound => {
                anyhow::anyhow!("MCP destination server `{server_name}` was not found")
            }
            McpReadyError::Disabled => {
                anyhow::anyhow!("MCP destination server `{server_name}` is disabled")
            }
            McpReadyError::PermanentlyFailed { last_error } => {
                let detail = last_error.unwrap_or_else(|| "connect failed".to_string());
                anyhow::anyhow!("MCP destination server `{server_name}` was not ready: {detail}")
            }
        })?;
    let configured_tool = destination
        .configured_tool()
        .ok_or_else(|| anyhow::anyhow!("MCP destination tool is missing"))?
        .to_string();
    let tools = state.mcp.server_tools(&server_name).await;
    let tool = find_mcp_tool(&tools, &configured_tool).ok_or_else(|| {
        anyhow::anyhow!(
            "MCP tool `{}` is not available on destination server `{}`",
            configured_tool,
            server_name
        )
    })?;
    Ok(ResolvedMcpToolDestination { server_name, tool })
}

fn pending_mcp_tool_post(
    draft: &IncidentMonitorDraftRecord,
    incident: Option<&IncidentMonitorIncidentRecord>,
    destination: &McpToolDestinationContext,
    resolved: &ResolvedMcpToolDestination,
    target_ref: &str,
    record_id: &str,
    idempotency_key: &str,
    evidence_digest: &str,
) -> IncidentMonitorPostRecord {
    let now = now_ms();
    IncidentMonitorPostRecord {
        post_id: format!("failure-post-{}", uuid::Uuid::new_v4().simple()),
        draft_id: draft.draft_id.clone(),
        incident_id: incident.map(|row| row.incident_id.clone()),
        fingerprint: draft.fingerprint.clone(),
        repo: draft.repo.clone(),
        operation: MCP_TOOL_OPERATION.to_string(),
        status: "pending".to_string(),
        issue_number: None,
        issue_url: None,
        comment_id: None,
        comment_url: None,
        destination_id: Some(destination.destination_id.clone()),
        destination_kind: Some(IncidentMonitorDestinationKind::McpTool),
        route_id: destination.route_id.clone(),
        route_match_reason: destination.route_match_reason(),
        external_id: Some(record_id.to_string()),
        external_url: None,
        external_title: Some(resolved.tool.namespaced_name.clone()),
        target_ref: Some(target_ref.to_string()),
        receipt: Some(json!({
            "provider": "mcp_tool",
            "destination_id": destination.destination_id,
            "operation": MCP_TOOL_OPERATION,
            "status": "pending",
            "server": resolved.server_name,
            "tool": resolved.tool.tool_name,
            "namespaced_tool": resolved.tool.namespaced_name,
            "target_ref": target_ref,
            "arguments_redacted": true,
        })),
        evidence_digest: Some(evidence_digest.to_string()),
        confidence: draft.confidence.clone(),
        risk_level: draft.risk_level.clone(),
        expected_destination: draft.expected_destination.clone(),
        evidence_refs: safe_evidence_refs(&draft.evidence_refs),
        quality_gate: None,
        idempotency_key: idempotency_key.to_string(),
        response_excerpt: None,
        error: None,
        created_at_ms: now,
        updated_at_ms: now,
    }
}

fn posted_mcp_tool_post(
    claim: IncidentMonitorPostRecord,
    destination: &McpToolDestinationContext,
    resolved: &ResolvedMcpToolDestination,
    target_ref: &str,
    record_id: &str,
    args: &Value,
    result: &ToolResult,
) -> IncidentMonitorPostRecord {
    let excerpt = tool_result_excerpt(result);
    IncidentMonitorPostRecord {
        status: "posted".to_string(),
        external_id: Some(record_id.to_string()),
        external_title: Some(resolved.tool.namespaced_name.clone()),
        receipt: Some(json!({
            "provider": "mcp_tool",
            "destination_id": destination.destination_id,
            "operation": MCP_TOOL_OPERATION,
            "status": "posted",
            "server": resolved.server_name,
            "tool": resolved.tool.tool_name,
            "namespaced_tool": resolved.tool.namespaced_name,
            "tool_schema_hash": resolved.tool.schema_hash,
            "target_ref": target_ref,
            "argument_keys": argument_keys(args),
            "arguments_redacted": true,
            "result_excerpt": excerpt,
            "result_metadata_keys": metadata_keys(&result.metadata),
            "mcp_auth_required": false,
        })),
        response_excerpt: excerpt,
        error: None,
        updated_at_ms: now_ms(),
        ..claim
    }
}

fn blocked_mcp_tool_post(
    claim: IncidentMonitorPostRecord,
    destination: &McpToolDestinationContext,
    resolved: &ResolvedMcpToolDestination,
    target_ref: &str,
    record_id: &str,
    args: &Value,
    result: &ToolResult,
    error: &str,
) -> IncidentMonitorPostRecord {
    IncidentMonitorPostRecord {
        status: "blocked".to_string(),
        external_id: Some(record_id.to_string()),
        external_title: Some(resolved.tool.namespaced_name.clone()),
        receipt: Some(json!({
            "provider": "mcp_tool",
            "destination_id": destination.destination_id,
            "operation": MCP_TOOL_OPERATION,
            "status": "blocked",
            "server": resolved.server_name,
            "tool": resolved.tool.tool_name,
            "namespaced_tool": resolved.tool.namespaced_name,
            "tool_schema_hash": resolved.tool.schema_hash,
            "target_ref": target_ref,
            "argument_keys": argument_keys(args),
            "arguments_redacted": true,
            "mcp_auth_required": true,
            "mcp_auth_status": result
                .metadata
                .get("mcpAuth")
                .and_then(|row| row.get("status"))
                .and_then(Value::as_str),
            "error": error,
        })),
        response_excerpt: None,
        error: Some(error.to_string()),
        updated_at_ms: now_ms(),
        ..claim
    }
}

fn failed_mcp_tool_post(
    claim: IncidentMonitorPostRecord,
    destination: &McpToolDestinationContext,
    resolved: &ResolvedMcpToolDestination,
    target_ref: &str,
    record_id: &str,
    args: &Value,
    error: &str,
) -> IncidentMonitorPostRecord {
    IncidentMonitorPostRecord {
        status: "failed".to_string(),
        external_id: Some(record_id.to_string()),
        external_title: Some(resolved.tool.namespaced_name.clone()),
        receipt: Some(json!({
            "provider": "mcp_tool",
            "destination_id": destination.destination_id,
            "operation": MCP_TOOL_OPERATION,
            "status": "failed",
            "server": resolved.server_name,
            "tool": resolved.tool.tool_name,
            "namespaced_tool": resolved.tool.namespaced_name,
            "tool_schema_hash": resolved.tool.schema_hash,
            "target_ref": target_ref,
            "argument_keys": argument_keys(args),
            "arguments_redacted": true,
            "error": error,
        })),
        response_excerpt: None,
        error: Some(error.to_string()),
        updated_at_ms: now_ms(),
        ..claim
    }
}

fn mcp_auth_required(result: &ToolResult) -> bool {
    result
        .metadata
        .get("mcpAuth")
        .and_then(|row| row.get("required"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

async fn successful_post_by_idempotency(
    state: &AppState,
    idempotency_key: &str,
) -> Option<IncidentMonitorPostRecord> {
    let mut rows = state
        .incident_monitor_posts
        .read()
        .await
        .values()
        .filter(|post| post.idempotency_key == idempotency_key && post.status == "posted")
        .cloned()
        .collect::<Vec<_>>();
    rows.sort_by_key(|post| std::cmp::Reverse(post.updated_at_ms));
    rows.into_iter().next()
}

async fn successful_post_for_draft(
    state: &AppState,
    draft_id: &str,
    destination_id: &str,
    target_ref: &str,
    evidence_digest: Option<&str>,
) -> Option<IncidentMonitorPostRecord> {
    let mut rows = state
        .incident_monitor_posts
        .read()
        .await
        .values()
        .filter(|post| post.draft_id == draft_id && post.status == "posted")
        .cloned()
        .collect::<Vec<_>>();
    rows.sort_by_key(|post| std::cmp::Reverse(post.updated_at_ms));
    rows.into_iter().find(|row| {
        row.destination_id.as_deref() == Some(destination_id)
            && row.target_ref.as_deref() == Some(target_ref)
            && match evidence_digest {
                Some(expected) => row.evidence_digest.as_deref() == Some(expected),
                None => true,
            }
    })
}

async fn latest_failed_mcp_tool_post_for_draft(
    state: &AppState,
    draft: &IncidentMonitorDraftRecord,
    destination_id: &str,
    target_ref: &str,
    evidence_digest: &str,
) -> Option<IncidentMonitorPostRecord> {
    let mut rows = state
        .incident_monitor_posts
        .read()
        .await
        .values()
        .filter(|post| {
            post.draft_id == draft.draft_id
                && post.fingerprint == draft.fingerprint
                && post.operation == MCP_TOOL_OPERATION
                && post.status == "failed"
                && post.destination_id.as_deref() == Some(destination_id)
                && post.target_ref.as_deref() == Some(target_ref)
                && post.evidence_digest.as_deref() == Some(evidence_digest)
        })
        .cloned()
        .collect::<Vec<_>>();
    rows.sort_by_key(|post| std::cmp::Reverse(post.updated_at_ms));
    rows.into_iter().next()
}

fn apply_existing_mcp_tool_post_to_draft(
    draft: &mut IncidentMonitorDraftRecord,
    post: &IncidentMonitorPostRecord,
) {
    draft.status = "mcp_tool_called".to_string();
    draft.github_status = Some("mcp_tool_called".to_string());
    draft.github_issue_url = post.external_url.clone();
    draft.github_posted_at_ms = Some(post.updated_at_ms);
    draft.last_post_error = None;
}

async fn mirror_mcp_tool_post_as_external_action(
    state: &AppState,
    draft: &IncidentMonitorDraftRecord,
    post: &IncidentMonitorPostRecord,
) {
    let action = ExternalActionRecord {
        action_id: post.post_id.clone(),
        operation: post.operation.clone(),
        status: post.status.clone(),
        source_kind: Some("incident_monitor".to_string()),
        source_id: Some(draft.draft_id.clone()),
        routine_run_id: None,
        context_run_id: draft.triage_run_id.clone(),
        capability_id: post
            .receipt
            .as_ref()
            .and_then(|row| row.get("namespaced_tool"))
            .and_then(Value::as_str)
            .map(ToString::to_string),
        provider: Some("incident-monitor".to_string()),
        target: post.target_ref.clone(),
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
            "external_id": post.external_id,
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
            "incident_monitor_operation": post.operation,
        })),
        created_at_ms: post.created_at_ms,
        updated_at_ms: post.updated_at_ms,
    };
    if let Err(error) = AppState::record_external_action(state, action).await {
        tracing::warn!(
            "failed to persist external action mirror for incident monitor MCP tool post {}: {}",
            post.post_id,
            error
        );
    }
}

fn render_payload_mapping(
    mapping: &Map<String, Value>,
    context: &MappingContext<'_>,
) -> anyhow::Result<Value> {
    let mut out = Map::new();
    for (key, value) in mapping {
        out.insert(key.clone(), render_mapping_value(value, context)?);
    }
    Ok(Value::Object(out))
}

fn render_mapping_value(value: &Value, context: &MappingContext<'_>) -> anyhow::Result<Value> {
    match value {
        Value::String(raw) if raw.starts_with('$') => placeholder_value(raw, context),
        Value::Array(rows) => Ok(Value::Array(
            rows.iter()
                .map(|row| render_mapping_value(row, context))
                .collect::<anyhow::Result<Vec<_>>>()?,
        )),
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, value) in map {
                out.insert(key.clone(), render_mapping_value(value, context)?);
            }
            Ok(Value::Object(out))
        }
        other => Ok(other.clone()),
    }
}

fn placeholder_value(raw: &str, context: &MappingContext<'_>) -> anyhow::Result<Value> {
    let incident = context.incident;
    let destination = context.destination;
    let resolved = context.resolved;
    Ok(match raw {
        "$draft.id" | "$draft.draft_id" => json!(context.draft.draft_id),
        "$draft.fingerprint" => json!(context.draft.fingerprint),
        "$draft.repo" => json!(context.draft.repo),
        "$draft.status" => json!(context.draft.status),
        "$draft.title" => json!(context.draft.title),
        "$draft.detail" => json!(context.draft.detail),
        "$draft.risk_level" => json!(context.draft.risk_level),
        "$draft.risk_category" => json!(context.draft.risk_category),
        "$draft.actor" => json!(context.draft.actor),
        "$draft.model" => json!(context.draft.model),
        "$draft.tool_name" => json!(context.draft.tool_name),
        "$draft.action" => json!(context.draft.action),
        "$draft.policy" => json!(context.draft.policy),
        "$draft.approval_state" => json!(context.draft.approval_state),
        "$draft.blast_radius" => json!(context.draft.blast_radius),
        "$draft.external_correlation_ids" => json!(context.draft.external_correlation_ids),
        "$draft.confidence" => json!(context.draft.confidence),
        "$draft.expected_destination" => json!(context.draft.expected_destination),
        "$draft.project_id" => json!(context.draft.project_id),
        "$draft.log_source_id" => json!(context.draft.log_source_id),
        "$draft.source_kind" => json!(context.draft.source_kind),
        "$draft.tenant_id" => json!(context.draft.tenant_id),
        "$draft.workspace_id" => json!(context.draft.workspace_id),
        "$draft.event_schema_version" => json!(context.draft.event_schema_version),
        "$draft.route_tags" => json!(context.draft.route_tags),
        "$draft.evidence_refs" => json!(context.draft.evidence_refs),
        "$draft.triage_run_id" => json!(context.draft.triage_run_id),
        "$draft.evidence_digest" | "$evidence.digest" => json!(context.evidence_digest),
        "$destination.id" => json!(destination.destination_id),
        "$destination.kind" => json!("mcp_tool"),
        "$destination.route_id" => json!(destination.route_id),
        "$destination.route_match_reason" => json!(destination.route_match_reason()),
        "$destination.target_ref" => json!(context.target_ref),
        "$mcp.server" => json!(resolved.server_name),
        "$mcp.tool" => json!(resolved.tool.tool_name),
        "$mcp.namespaced_tool" => json!(resolved.tool.namespaced_name),
        "$mcp.schema_hash" => json!(resolved.tool.schema_hash),
        "$idempotency_key" => json!(context.idempotency_key),
        "$incident.id" | "$incident.incident_id" => {
            json!(incident.map(|row| row.incident_id.clone()))
        }
        "$incident.title" => json!(incident.map(|row| row.title.clone())),
        "$incident.event_type" => json!(incident.map(|row| row.event_type.clone())),
        "$incident.status" => json!(incident.map(|row| row.status.clone())),
        "$incident.detail" => json!(incident.and_then(|row| row.detail.clone())),
        "$incident.source" => json!(incident.and_then(|row| row.source.clone())),
        "$incident.component" => json!(incident.and_then(|row| row.component.clone())),
        "$incident.level" => json!(incident.and_then(|row| row.level.clone())),
        "$incident.risk_level" => json!(incident.and_then(|row| row.risk_level.clone())),
        "$incident.risk_category" => json!(incident.and_then(|row| row.risk_category.clone())),
        "$incident.actor" => json!(incident.and_then(|row| row.actor.clone())),
        "$incident.model" => json!(incident.and_then(|row| row.model.clone())),
        "$incident.tool_name" => json!(incident.and_then(|row| row.tool_name.clone())),
        "$incident.action" => json!(incident.and_then(|row| row.action.clone())),
        "$incident.policy" => json!(incident.and_then(|row| row.policy.clone())),
        "$incident.approval_state" => {
            json!(incident.and_then(|row| row.approval_state.clone()))
        }
        "$incident.blast_radius" => json!(incident.and_then(|row| row.blast_radius.clone())),
        "$incident.external_correlation_ids" => {
            json!(incident.map(|row| row.external_correlation_ids.clone()))
        }
        "$incident.occurrence_count" => json!(incident.map(|row| row.occurrence_count)),
        "$incident.evidence_refs" => json!(incident.map(|row| row.evidence_refs.clone())),
        _ => anyhow::bail!("Unsupported MCP payload mapping placeholder `{raw}`"),
    })
}

fn payload_mapping(config: Option<&Value>) -> Result<&Map<String, Value>, String> {
    let Some(config) = config.and_then(Value::as_object) else {
        return Err("MCP destination config is missing".to_string());
    };
    for key in ["payload", "arguments"] {
        if let Some(value) = config.get(key) {
            let Some(map) = value.as_object() else {
                return Err("MCP payload mapping must be an object".to_string());
            };
            if map.is_empty() {
                return Err("MCP payload mapping must not be empty".to_string());
            }
            return Ok(map);
        }
    }
    Err("MCP payload mapping is missing".to_string())
}

fn mcp_publish_allowed(config: Option<&Value>) -> bool {
    config_bool(config, "allow_publish").unwrap_or(false)
        || config_bool(config, "allow_mcp_publish").unwrap_or(false)
}

fn config_bool(config: Option<&Value>, key: &str) -> Option<bool> {
    config?.get(key).and_then(Value::as_bool)
}

fn config_string<'a>(config: Option<&'a Value>, keys: &[&str]) -> Option<&'a str> {
    let config = config?.as_object()?;
    keys.iter()
        .find_map(|key| config.get(*key).and_then(Value::as_str))
        .and_then(normalize_config_str)
}

fn normalize_config_str(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn find_mcp_tool(tools: &[McpRemoteTool], configured_tool: &str) -> Option<McpRemoteTool> {
    let configured_tool = configured_tool.trim();
    tools
        .iter()
        .find(|tool| {
            tool.tool_name.eq_ignore_ascii_case(configured_tool)
                || tool.namespaced_name.eq_ignore_ascii_case(configured_tool)
        })
        .cloned()
}

fn server_has_mcp_tool(server: &McpServer, configured_tool: &str) -> bool {
    let server_slug = sanitize_namespace_segment(&server.name);
    server.tool_cache.iter().any(|tool| {
        if !tool_allowed_for_server(&server_slug, server.allowed_tools.as_ref(), &tool.tool_name) {
            return false;
        }
        let tool_slug = sanitize_namespace_segment(&tool.tool_name);
        let namespaced_name = format!("mcp.{server_slug}.{tool_slug}");
        tool.tool_name.eq_ignore_ascii_case(configured_tool)
            || namespaced_name.eq_ignore_ascii_case(configured_tool)
    })
}

fn tool_allowed_for_server(
    server_slug: &str,
    allowed_tools: Option<&Vec<String>>,
    tool_name: &str,
) -> bool {
    let Some(allowed_tools) = allowed_tools else {
        return true;
    };
    if allowed_tools.is_empty() {
        return false;
    }
    let tool_slug = sanitize_namespace_segment(tool_name);
    let namespaced_name = format!("mcp.{server_slug}.{tool_slug}");
    allowed_tools.iter().any(|entry| {
        let pattern = entry.trim();
        !pattern.is_empty()
            && (pattern == "*"
                || pattern == tool_name.trim()
                || pattern == namespaced_name
                || pattern == format!("mcp.{server_slug}.*"))
    })
}

fn sanitize_namespace_segment(raw: &str) -> String {
    let mut out = String::new();
    let mut previous_underscore = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            previous_underscore = false;
        } else if !previous_underscore {
            out.push('_');
            previous_underscore = true;
        }
    }
    let cleaned = out.trim_matches('_');
    if cleaned.is_empty() {
        "tool".to_string()
    } else {
        cleaned.to_string()
    }
}

fn argument_keys(args: &Value) -> Vec<String> {
    let mut keys = args
        .as_object()
        .map(|map| map.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    keys.sort();
    keys
}

fn metadata_keys(value: &Value) -> Vec<String> {
    let mut keys = value
        .as_object()
        .map(|map| map.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    keys.sort();
    keys
}

fn tool_result_excerpt(result: &ToolResult) -> Option<String> {
    if result.output.trim().is_empty() {
        return None;
    }
    Some(truncate_text(
        &safe_result_excerpt(&result.output),
        DEFAULT_RESULT_EXCERPT_LIMIT,
    ))
}

fn safe_result_excerpt(value: &str) -> String {
    redact_sensitive_text(value)
}

fn safe_evidence_refs(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| truncate_text(&redact_sensitive_text(value), 500))
        .collect()
}

fn redact_sensitive_text(value: &str) -> String {
    value
        .lines()
        .map(redact_sensitive_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_sensitive_line(line: &str) -> String {
    let lower = line.to_ascii_lowercase();
    for marker in [
        "authorization:",
        "authorization=",
        "password:",
        "password=",
        "secret:",
        "secret=",
        "token:",
        "token=",
        "api_key:",
        "api_key=",
        "apikey:",
        "apikey=",
    ] {
        if let Some(index) = lower.find(marker) {
            let keep = &line[..index + marker.len()];
            return format!("{keep}[redacted]");
        }
    }
    line.split_whitespace()
        .map(redact_sensitive_token)
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_sensitive_token(token: &str) -> String {
    let lower = token.to_ascii_lowercase();
    if lower.starts_with("sk-")
        || lower.starts_with("github_pat_")
        || lower.starts_with("tim_intake_")
    {
        "[redacted]".to_string()
    } else {
        token.to_string()
    }
}

fn compute_evidence_digest(draft: &IncidentMonitorDraftRecord) -> String {
    sha256_hex(&[
        draft.repo.as_str(),
        draft.fingerprint.as_str(),
        draft.title.as_deref().unwrap_or(""),
        draft.detail.as_deref().unwrap_or(""),
    ])
}

fn build_idempotency_key(
    destination_id: &str,
    target_ref: &str,
    fingerprint: &str,
    operation: &str,
    digest: &str,
) -> String {
    sha256_hex(&[
        destination_id,
        "mcp_tool",
        target_ref,
        fingerprint,
        operation,
        digest,
    ])
}

fn deterministic_record_id(
    destination: &McpToolDestinationContext,
    target_ref: &str,
    draft: &IncidentMonitorDraftRecord,
    evidence_digest: &str,
) -> String {
    let digest = sha256_hex(&[
        &destination.destination_id,
        "mcp_tool",
        target_ref,
        &draft.repo,
        &draft.fingerprint,
        evidence_digest,
    ]);
    format!("bmmcp_{}", &digest[..24])
}

fn target_ref(resolved: &ResolvedMcpToolDestination) -> String {
    format!(
        "mcp:{}/{}",
        resolved.server_name, resolved.tool.namespaced_name
    )
}
