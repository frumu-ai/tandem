use std::time::Duration;

use anyhow::Context;
use serde_json::{json, Value};
use tandem_runtime::mcp_ready::{EnsureReadyPolicy, McpReadyError};
use tandem_runtime::McpRemoteTool;
use tandem_types::{EngineEvent, ToolResult};

use crate::{
    now_ms, sha256_hex, truncate_text, AppState, ExternalActionRecord, IncidentMonitorConfig,
    IncidentMonitorDestinationKind, IncidentMonitorDraftRecord, IncidentMonitorIncidentRecord,
    IncidentMonitorPostRecord,
};

pub use crate::incident_monitor_github::{PublishMode, PublishOutcome};

const INCIDENT_MONITOR_LABEL: &str = "incident-monitor";
const LINEAR_BODY_BYTE_BUDGET: usize = 18_000;
const LINEAR_BODY_MARKER_SAFE_SPACE: usize = 512;
const LINEAR_EVIDENCE_REF_LIMIT: usize = 15;

#[derive(Debug, Clone)]
pub struct LinearDestinationContext {
    pub destination_id: String,
    pub route_id: Option<String>,
    pub route_match_reason: Option<String>,
    pub mcp_server: Option<String>,
    pub linear_team: Option<String>,
    pub linear_project: Option<String>,
}

impl LinearDestinationContext {
    fn route_match_reason(&self) -> Option<String> {
        self.route_match_reason
            .clone()
            .or_else(|| Some("destination_router".to_string()))
    }

    fn team(&self) -> anyhow::Result<&str> {
        self.linear_team
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Linear destination team is missing"))
    }

    fn project(&self) -> anyhow::Result<&str> {
        self.linear_project
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Linear destination project is missing"))
    }

    fn target_ref(&self) -> anyhow::Result<String> {
        Ok(format!("{}/{}", self.team()?, self.project()?))
    }
}

#[derive(Debug, Clone)]
struct LinearToolSet {
    server_name: String,
    list_issues: String,
    create_issue: String,
}

#[derive(Debug, Clone, Default)]
struct LinearIssue {
    id: Option<String>,
    identifier: Option<String>,
    title: String,
    description: String,
    url: Option<String>,
    state: Option<String>,
    state_type: Option<String>,
}

pub async fn publish_draft(
    state: &AppState,
    draft_id: &str,
    incident_id: Option<&str>,
    mode: PublishMode,
    destination: LinearDestinationContext,
) -> anyhow::Result<PublishOutcome> {
    let status = state.incident_monitor_status_snapshot().await;
    let config = status.config.clone();
    if !config.enabled {
        anyhow::bail!("Incident Monitor is disabled");
    }
    if config.paused && matches!(mode, PublishMode::Auto | PublishMode::Recovery) {
        anyhow::bail!("Incident Monitor is paused");
    }

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

    let target_ref = destination.target_ref()?;
    let tools = resolve_linear_tool_set_for_state(state, &config, &destination)
        .await
        .context("resolve Linear MCP tools for Incident Monitor")?;
    let incident = match incident_id {
        Some(id) => state.get_incident_monitor_incident(id).await,
        None => None,
    };
    let evidence_digest = compute_evidence_digest(&draft, incident.as_ref());

    // Resolve the current matching issue up front: it drives both the
    // duplicate short-circuit and the create/comment decision, and its state is
    // what distinguishes a live duplicate from a recurrence after close.
    let matched_issue = find_matching_linear_issue(
        state,
        &tools,
        destination.team()?,
        destination.project()?,
        &draft,
        &evidence_digest,
    )
    .await
    .context("match existing Linear issue for Incident Monitor draft")?;

    // When the match is terminal (completed/canceled), namespace the idempotency
    // to that closed issue so the recurrence gets a fresh create key instead of
    // colliding with the closed issue's key and being suppressed.
    let publish_evidence_digest = match matched_issue.as_ref() {
        Some(issue) if linear_issue_is_terminal(issue) => {
            let anchor = issue
                .identifier
                .clone()
                .or_else(|| issue.id.clone())
                .unwrap_or_default();
            sha256_hex(&[evidence_digest.as_str(), "recurrence", anchor.as_str()])
        }
        _ => evidence_digest.clone(),
    };
    draft.evidence_digest = Some(publish_evidence_digest.clone());

    if mode != PublishMode::RecheckOnly {
        if let Some(existing) = successful_post_for_draft(
            state,
            &draft.draft_id,
            &destination.destination_id,
            &target_ref,
            Some(&evidence_digest),
        )
        .await
        {
            // Only a genuine duplicate — the still-open issue that this prior post
            // created — should short-circuit. If that issue was completed/canceled,
            // or a newer open issue now matches, fall through so the recurrence
            // creates or comments on the correct live issue.
            let is_live_duplicate = matched_issue.as_ref().is_some_and(|issue| {
                !linear_issue_is_terminal(issue) && linear_post_references_issue(&existing, issue)
            });
            if is_live_duplicate {
                apply_existing_linear_post_to_draft(&mut draft, &existing);
                mirror_linear_post_as_external_action(state, &draft, &existing).await;
                let draft = state.put_incident_monitor_draft(draft).await?;
                return Ok(PublishOutcome {
                    action: "skip_duplicate".to_string(),
                    draft,
                    post: Some(existing),
                });
            }
        }
    }

    let issue_draft = if mode == PublishMode::RecheckOnly {
        None
    } else if draft.triage_run_id.is_none() {
        if mode == PublishMode::ManualPublish {
            anyhow::bail!("Incident Monitor draft needs a triage run before Linear publish");
        }
        None
    } else if mode == PublishMode::ManualPublish {
        Some(
            crate::http::incident_monitor::ensure_incident_monitor_issue_draft(
                state.clone(),
                &draft.draft_id,
                false,
            )
            .await
            .context("generate Incident Monitor issue draft")?,
        )
    } else {
        match draft.triage_run_id.as_deref() {
            Some(run_id) => {
                crate::http::incident_monitor::load_incident_monitor_issue_draft_artifact(
                    state, run_id,
                )
                .await
            }
            None => None,
        }
    };

    if mode == PublishMode::RecheckOnly {
        if let Some(issue) = matched_issue {
            draft.github_status = Some("matched_linear_issue".to_string());
            draft.github_issue_url = issue.url.clone();
            draft.matched_issue_state = issue.state.clone();
            let draft = state.put_incident_monitor_draft(draft).await?;
            return Ok(PublishOutcome {
                action: "matched_linear_issue".to_string(),
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

    if let Some(issue) = matched_issue {
        if linear_issue_is_terminal(&issue) {
            // Recurrence after the matched issue was completed/canceled: file a
            // fresh issue instead of silently recording a match, mirroring the
            // GitHub closed-issue path. Once the new (open) issue exists, future
            // recurrences match it and record a match rather than re-creating.
            draft.matched_issue_state = issue.state.clone();
        } else {
            return record_matched_linear_issue(
                state,
                draft,
                incident.as_ref(),
                &destination,
                &target_ref,
                &evidence_digest,
                issue,
            )
            .await;
        }
    }

    create_linear_issue_from_draft(
        state,
        &tools,
        &config,
        draft,
        incident.as_ref(),
        &publish_evidence_digest,
        issue_draft.as_ref(),
        &destination,
        &target_ref,
    )
    .await
}

/// Whether a post record refers to the given Linear issue (by URL, then id).
fn linear_post_references_issue(post: &IncidentMonitorPostRecord, issue: &LinearIssue) -> bool {
    let same = |a: &Option<String>, b: &Option<String>| matches!((a, b), (Some(a), Some(b)) if a == b);
    same(&post.external_url, &issue.url)
        || same(&post.external_id, &issue.identifier)
        || same(&post.external_id, &issue.id)
}

async fn create_linear_issue_from_draft(
    state: &AppState,
    tools: &LinearToolSet,
    config: &IncidentMonitorConfig,
    mut draft: IncidentMonitorDraftRecord,
    incident: Option<&IncidentMonitorIncidentRecord>,
    evidence_digest: &str,
    issue_draft: Option<&Value>,
    destination: &LinearDestinationContext,
    target_ref: &str,
) -> anyhow::Result<PublishOutcome> {
    if config.require_approval_for_new_issues && !draft.status.eq_ignore_ascii_case("draft_ready") {
        draft.status = "approval_required".to_string();
        draft.github_status = Some("approval_required".to_string());
        let draft = state.put_incident_monitor_draft(draft).await?;
        return Ok(PublishOutcome {
            action: "approval_required".to_string(),
            draft,
            post: None,
        });
    }
    if !config.auto_create_new_issues && draft.status.eq_ignore_ascii_case("draft_ready") {
        let draft = state.put_incident_monitor_draft(draft).await?;
        return Ok(PublishOutcome {
            action: "draft_ready".to_string(),
            draft,
            post: None,
        });
    }

    let idempotency_key = build_idempotency_key(
        &destination.destination_id,
        target_ref,
        &draft.fingerprint,
        "create_issue",
        evidence_digest,
    );
    if let Some(existing) = successful_post_by_idempotency(state, &idempotency_key).await {
        apply_existing_linear_post_to_draft(&mut draft, &existing);
        mirror_linear_post_as_external_action(state, &draft, &existing).await;
        let draft = state.put_incident_monitor_draft(draft).await?;
        return Ok(PublishOutcome {
            action: "skip_duplicate".to_string(),
            draft,
            post: Some(existing),
        });
    }
    if let Some(previous) = latest_failed_create_post_for_draft(
        state,
        &draft,
        &destination.destination_id,
        target_ref,
        evidence_digest,
    )
    .await
    {
        let detail = format!(
            "suppressed Linear issue creation for fingerprint {} after previous create_issue post attempt {} failed; refusing to retry create_issue because the previous attempt may have created an issue without returning a parseable payload",
            draft.fingerprint, previous.post_id
        );
        draft.status = "linear_post_failed".to_string();
        draft.github_status = Some("linear_post_failed".to_string());
        draft.last_post_error = Some(truncate_text(&detail, 500));
        let draft = state.put_incident_monitor_draft(draft).await?;
        return Ok(PublishOutcome {
            action: "create_issue_retry_suppressed".to_string(),
            draft,
            post: Some(previous),
        });
    }

    let claim = IncidentMonitorPostRecord {
        post_id: format!("failure-post-{}", uuid::Uuid::new_v4().simple()),
        draft_id: draft.draft_id.clone(),
        incident_id: incident.map(|row| row.incident_id.clone()),
        fingerprint: draft.fingerprint.clone(),
        repo: draft.repo.clone(),
        operation: "create_issue".to_string(),
        status: "pending".to_string(),
        issue_number: None,
        issue_url: None,
        comment_id: None,
        comment_url: None,
        destination_id: Some(destination.destination_id.clone()),
        destination_kind: Some(IncidentMonitorDestinationKind::LinearIssue),
        route_id: destination.route_id.clone(),
        route_match_reason: destination.route_match_reason(),
        external_id: None,
        external_url: None,
        external_title: None,
        target_ref: Some(target_ref.to_string()),
        receipt: Some(json!({
            "provider": "linear",
            "destination_id": destination.destination_id,
            "operation": "create_issue",
            "status": "pending",
            "team": destination.team().ok(),
            "project": destination.project().ok(),
        })),
        evidence_digest: Some(evidence_digest.to_string()),
        confidence: draft.confidence.clone(),
        risk_level: draft.risk_level.clone(),
        expected_destination: draft.expected_destination.clone(),
        evidence_refs: draft.evidence_refs.clone(),
        quality_gate: draft.quality_gate.clone(),
        idempotency_key: idempotency_key.clone(),
        response_excerpt: None,
        error: None,
        created_at_ms: now_ms(),
        updated_at_ms: now_ms(),
    };
    let (claimed, existing_claim) = state
        .try_claim_incident_monitor_post_idempotency(claim)
        .await?;
    if !claimed {
        if existing_claim.status == "posted" {
            apply_existing_linear_post_to_draft(&mut draft, &existing_claim);
            mirror_linear_post_as_external_action(state, &draft, &existing_claim).await;
            let draft = state.put_incident_monitor_draft(draft).await?;
            return Ok(PublishOutcome {
                action: "skip_duplicate".to_string(),
                draft,
                post: Some(existing_claim),
            });
        }
        draft.github_status = Some("linear_posting".to_string());
        draft.last_post_error = Some(
            "another Incident Monitor publisher already claimed this Linear create_issue idempotency key"
                .to_string(),
        );
        return Ok(PublishOutcome {
            action: "publish_in_progress".to_string(),
            draft,
            post: Some(existing_claim),
        });
    }

    let title = issue_draft
        .and_then(|row| row.get("suggested_title"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| draft.title.as_deref().unwrap_or("Incident Monitor issue"));
    let body = build_linear_issue_description(&draft, incident, issue_draft, evidence_digest);

    let created = match call_create_linear_issue(
        state,
        tools,
        destination.team()?,
        destination.project()?,
        title,
        &body,
        draft.risk_level.as_deref(),
    )
    .await
    {
        Ok(created) => created,
        Err(error) => {
            let error_text = truncate_text(&error.to_string(), 500);
            let mut failed_claim = existing_claim.clone();
            failed_claim.status = "failed".to_string();
            failed_claim.error = Some(error_text.clone());
            failed_claim.updated_at_ms = now_ms();
            if let Err(record_err) = state.put_incident_monitor_post(failed_claim).await {
                tracing::warn!(
                    draft_id = %draft.draft_id,
                    error = %record_err,
                    "failed to record ambiguous Incident Monitor Linear create_issue failure",
                );
            }
            draft.status = "linear_post_failed".to_string();
            draft.github_status = Some("linear_post_failed".to_string());
            draft.last_post_error = Some(error_text);
            let _ = state.put_incident_monitor_draft(draft).await;
            return Err(error).context("create Incident Monitor issue in Linear");
        }
    };

    let post = IncidentMonitorPostRecord {
        status: "posted".to_string(),
        issue_url: created.url.clone(),
        external_id: linear_external_id(&created),
        external_url: created.url.clone(),
        external_title: Some(linear_external_title(&created)),
        receipt: Some(json!({
            "provider": "linear",
            "destination_id": destination.destination_id,
            "operation": "create_issue",
            "issue_id": created.id,
            "identifier": created.identifier,
            "issue_url": created.url,
            "team": destination.team().ok(),
            "project": destination.project().ok(),
        })),
        response_excerpt: Some(truncate_text(&body, 400)),
        error: None,
        updated_at_ms: now_ms(),
        ..existing_claim
    };
    let post = state.put_incident_monitor_post(post).await?;
    mirror_linear_post_as_external_action(state, &draft, &post).await;
    draft.status = "linear_issue_created".to_string();
    draft.github_status = Some("linear_issue_created".to_string());
    draft.github_issue_url = post.issue_url.clone().or(post.external_url.clone());
    draft.github_posted_at_ms = Some(post.updated_at_ms);
    draft.last_post_error = None;
    let draft = state.put_incident_monitor_draft(draft).await?;
    state
        .update_incident_monitor_runtime_status(|runtime| {
            runtime.last_post_result = Some(format!(
                "created Linear issue {}",
                post.external_id.as_deref().unwrap_or("unknown")
            ));
        })
        .await;
    state.event_bus.publish(EngineEvent::new(
        "incident_monitor.linear.issue_created",
        json!({
            "draft_id": draft.draft_id,
            "repo": draft.repo,
            "target_ref": target_ref,
            "destination_id": destination.destination_id,
            "external_id": post.external_id,
            "external_url": post.external_url,
        }),
    ));
    Ok(PublishOutcome {
        action: "create_issue".to_string(),
        draft,
        post: Some(post),
    })
}

async fn record_matched_linear_issue(
    state: &AppState,
    mut draft: IncidentMonitorDraftRecord,
    incident: Option<&IncidentMonitorIncidentRecord>,
    destination: &LinearDestinationContext,
    target_ref: &str,
    evidence_digest: &str,
    issue: LinearIssue,
) -> anyhow::Result<PublishOutcome> {
    let idempotency_key = build_idempotency_key(
        &destination.destination_id,
        target_ref,
        &draft.fingerprint,
        "match_issue",
        evidence_digest,
    );
    if let Some(existing) = successful_post_by_idempotency(state, &idempotency_key).await {
        apply_existing_linear_post_to_draft(&mut draft, &existing);
        mirror_linear_post_as_external_action(state, &draft, &existing).await;
        let draft = state.put_incident_monitor_draft(draft).await?;
        return Ok(PublishOutcome {
            action: "skip_duplicate".to_string(),
            draft,
            post: Some(existing),
        });
    }

    let now = now_ms();
    let post = IncidentMonitorPostRecord {
        post_id: format!("failure-post-{}", uuid::Uuid::new_v4().simple()),
        draft_id: draft.draft_id.clone(),
        incident_id: incident.map(|row| row.incident_id.clone()),
        fingerprint: draft.fingerprint.clone(),
        repo: draft.repo.clone(),
        operation: "match_issue".to_string(),
        status: "posted".to_string(),
        issue_number: None,
        issue_url: issue.url.clone(),
        comment_id: None,
        comment_url: None,
        destination_id: Some(destination.destination_id.clone()),
        destination_kind: Some(IncidentMonitorDestinationKind::LinearIssue),
        route_id: destination.route_id.clone(),
        route_match_reason: destination.route_match_reason(),
        external_id: linear_external_id(&issue),
        external_url: issue.url.clone(),
        external_title: Some(linear_external_title(&issue)),
        target_ref: Some(target_ref.to_string()),
        receipt: Some(json!({
            "provider": "linear",
            "destination_id": destination.destination_id,
            "operation": "match_issue",
            "issue_id": issue.id.clone(),
            "identifier": issue.identifier.clone(),
            "issue_url": issue.url.clone(),
            "team": destination.team().ok(),
            "project": destination.project().ok(),
        })),
        evidence_digest: Some(evidence_digest.to_string()),
        confidence: draft.confidence.clone(),
        risk_level: draft.risk_level.clone(),
        expected_destination: draft.expected_destination.clone(),
        evidence_refs: draft.evidence_refs.clone(),
        quality_gate: draft.quality_gate.clone(),
        idempotency_key,
        response_excerpt: Some(truncate_text(&issue.description, 400)),
        error: None,
        created_at_ms: now,
        updated_at_ms: now,
    };
    let post = state.put_incident_monitor_post(post).await?;
    mirror_linear_post_as_external_action(state, &draft, &post).await;
    draft.status = "linear_issue_matched".to_string();
    draft.github_status = Some("linear_issue_matched".to_string());
    draft.github_issue_url = post.issue_url.clone().or(post.external_url.clone());
    draft.github_posted_at_ms = Some(post.updated_at_ms);
    draft.matched_issue_state = issue.state.clone();
    draft.last_post_error = None;
    let draft = state.put_incident_monitor_draft(draft).await?;
    state
        .update_incident_monitor_runtime_status(|runtime| {
            runtime.last_post_result = Some(format!(
                "matched Linear issue {}",
                post.external_id.as_deref().unwrap_or("unknown")
            ));
        })
        .await;
    state.event_bus.publish(EngineEvent::new(
        "incident_monitor.linear.issue_matched",
        json!({
            "draft_id": draft.draft_id,
            "repo": draft.repo,
            "target_ref": target_ref,
            "destination_id": destination.destination_id,
            "external_id": post.external_id,
            "external_url": post.external_url,
        }),
    ));
    Ok(PublishOutcome {
        action: "matched_linear_issue".to_string(),
        draft,
        post: Some(post),
    })
}

async fn resolve_linear_tool_set_for_state(
    state: &AppState,
    config: &IncidentMonitorConfig,
    destination: &LinearDestinationContext,
) -> anyhow::Result<LinearToolSet> {
    let server_name = destination
        .mcp_server
        .as_ref()
        .or(config.mcp_server.as_ref())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("Linear destination MCP server is not configured"))?
        .to_string();
    state
        .mcp
        .ensure_ready(&server_name, EnsureReadyPolicy::with_retries(3, 750))
        .await
        .map_err(|error| match error {
            McpReadyError::NotFound => {
                anyhow::anyhow!("Linear destination MCP server `{server_name}` was not found")
            }
            McpReadyError::Disabled => {
                anyhow::anyhow!("Linear destination MCP server `{server_name}` is disabled")
            }
            McpReadyError::PermanentlyFailed { last_error } => {
                let detail = last_error.unwrap_or_else(|| "connect failed".to_string());
                anyhow::anyhow!(
                    "Linear destination MCP server `{server_name}` was not ready: {detail}"
                )
            }
        })?;
    let server_tools = state.mcp.server_tools(&server_name).await;
    if server_tools.is_empty() {
        anyhow::bail!("no MCP tools were discovered for selected Linear destination server");
    }

    let discovered = state
        .capability_resolver
        .discover_from_runtime(server_tools.clone(), Vec::new())
        .await;
    let mut resolved = state
        .capability_resolver
        .resolve(
            crate::capability_resolver::CapabilityResolveInput {
                workflow_id: Some("incident-monitor-linear".to_string()),
                required_capabilities: vec![
                    "linear.list_issues".to_string(),
                    "linear.create_issue".to_string(),
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
                    workflow_id: Some("incident-monitor-linear".to_string()),
                    required_capabilities: vec![
                        "linear.list_issues".to_string(),
                        "linear.create_issue".to_string(),
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
    let list_issues = tool_name("linear.list_issues").or_else(|_| {
        direct_tool_name_fallback(&[
            "list_issues",
            "list_my_issues",
            "mcp.linear.list_issues",
            "mcp.linear.list_my_issues",
            "mcp.app_linear_linear.list_issues",
            "mcp.app_linear_linear.list_my_issues",
            "linear_list_issues",
        ])
        .ok_or_else(|| anyhow::anyhow!("missing resolved tool for linear.list_issues"))
    })?;
    let create_issue = tool_name("linear.create_issue").or_else(|_| {
        direct_tool_name_fallback(&[
            "create_issue",
            "save_issue",
            "update_issue",
            "mcp.linear.create_issue",
            "mcp.linear.save_issue",
            "mcp.linear.update_issue",
            "mcp.app_linear_linear.create_issue",
            "mcp.app_linear_linear.save_issue",
            "mcp.app_linear_linear.update_issue",
            "linear_create_issue",
            "linear_save_issue",
        ])
        .ok_or_else(|| anyhow::anyhow!("missing resolved tool for linear.create_issue"))
    })?;

    Ok(LinearToolSet {
        server_name,
        list_issues,
        create_issue,
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

async fn find_matching_linear_issue(
    state: &AppState,
    tools: &LinearToolSet,
    team: &str,
    project: &str,
    draft: &IncidentMonitorDraftRecord,
    evidence_digest: &str,
) -> anyhow::Result<Option<LinearIssue>> {
    let issues = call_list_linear_issues(state, tools, team, project, &draft.fingerprint).await?;
    let marker = fingerprint_marker(&draft.fingerprint);
    let evidence = evidence_marker(evidence_digest);
    let normalized_title = draft
        .title
        .as_deref()
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    Ok(select_matching_linear_issue(
        issues,
        &marker,
        &evidence,
        &normalized_title,
        &draft.fingerprint,
    ))
}

/// Whether a Linear issue is in a terminal (completed/canceled) state. Prefers
/// the canonical workflow-state `type` and falls back to the state name.
fn linear_issue_is_terminal(issue: &LinearIssue) -> bool {
    if let Some(state_type) = issue.state_type.as_deref() {
        return matches!(
            state_type.trim().to_ascii_lowercase().as_str(),
            "completed" | "canceled" | "cancelled"
        );
    }
    match issue.state.as_deref() {
        Some(name) => matches!(
            name.trim().to_ascii_lowercase().as_str(),
            "done"
                | "completed"
                | "complete"
                | "closed"
                | "canceled"
                | "cancelled"
                | "resolved"
                | "won't do"
                | "wont do"
                | "won't fix"
                | "wontfix"
        ),
        None => false,
    }
}

/// Select the best matching Linear issue for a draft. Marker/evidence matches
/// take priority over title/fingerprint matches, and within each an open
/// (non-terminal) issue is preferred over a terminal one so recurrences converge
/// onto the currently-open issue instead of repeatedly matching a closed one.
fn select_matching_linear_issue(
    mut issues: Vec<LinearIssue>,
    marker: &str,
    evidence: &str,
    normalized_title: &str,
    fingerprint: &str,
) -> Option<LinearIssue> {
    issues.sort_by_key(|issue| std::cmp::Reverse(issue.identifier.clone()));
    let is_marker_match = |issue: &LinearIssue| {
        issue.description.contains(marker) || issue.description.contains(evidence)
    };
    let is_title_match = |issue: &LinearIssue| {
        issue.title.trim().eq_ignore_ascii_case(normalized_title)
            || issue.description.contains(fingerprint)
    };
    let prefer_open = |predicate: &dyn Fn(&LinearIssue) -> bool| -> Option<LinearIssue> {
        issues
            .iter()
            .find(|issue| predicate(issue) && !linear_issue_is_terminal(issue))
            .cloned()
            .or_else(|| issues.iter().find(|issue| predicate(issue)).cloned())
    };
    prefer_open(&is_marker_match).or_else(|| prefer_open(&is_title_match))
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

async fn latest_failed_create_post_for_draft(
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
                && post.operation == "create_issue"
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

fn apply_existing_linear_post_to_draft(
    draft: &mut IncidentMonitorDraftRecord,
    post: &IncidentMonitorPostRecord,
) {
    let status = if post.operation == "match_issue" {
        "linear_issue_matched"
    } else {
        "linear_issue_created"
    };
    draft.status = status.to_string();
    draft.github_status = Some(status.to_string());
    draft.github_issue_url = post.issue_url.clone().or(post.external_url.clone());
    draft.github_posted_at_ms = Some(post.updated_at_ms);
    draft.last_post_error = None;
}

fn compute_evidence_digest(
    draft: &IncidentMonitorDraftRecord,
    incident: Option<&IncidentMonitorIncidentRecord>,
) -> String {
    let _ = incident;
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
        "linear_issue",
        target_ref,
        fingerprint,
        operation,
        digest,
    ])
}

fn build_linear_issue_description(
    draft: &IncidentMonitorDraftRecord,
    incident: Option<&IncidentMonitorIncidentRecord>,
    issue_draft: Option<&Value>,
    evidence_digest: &str,
) -> String {
    let mut lines = Vec::new();
    if let Some(rendered) = issue_draft
        .and_then(|row| row.get("rendered_body"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(truncate_text(rendered, 8_000));
    } else {
        if let Some(detail) = draft.detail.as_deref() {
            lines.push(truncate_text(detail, 4_000));
        }
        if let Some(summary) = issue_draft
            .and_then(|row| row.get("what_happened"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            if !lines.is_empty() {
                lines.push(String::new());
            }
            lines.push("### Triage summary".to_string());
            lines.push(truncate_text(summary, 2_000));
        }
        if let Some(fix) = issue_draft
            .and_then(|row| row.get("recommended_fix"))
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            lines.push(String::new());
            lines.push("### Recommended fix".to_string());
            lines.push(truncate_text(fix, 2_000));
        }
    }

    lines.push(String::new());
    lines.push("### Incident Monitor metadata".to_string());
    lines.push(format!("draft_id: {}", draft.draft_id));
    lines.push(format!("fingerprint: {}", draft.fingerprint));
    lines.push(format!("source_repo: {}", draft.repo));
    if let Some(run_id) = draft.triage_run_id.as_deref() {
        lines.push(format!("triage_run_id: {run_id}"));
    }
    if let Some(project_id) = draft.project_id.as_deref() {
        lines.push(format!("project_id: {project_id}"));
    }
    if let Some(log_source_id) = draft.log_source_id.as_deref() {
        lines.push(format!("log_source_id: {log_source_id}"));
    }
    if let Some(confidence) = draft.confidence.as_deref() {
        lines.push(format!("confidence: {confidence}"));
    }
    if let Some(risk_level) = draft.risk_level.as_deref() {
        lines.push(format!("risk_level: {risk_level}"));
    }
    if let Some(risk_category) = draft.risk_category.as_deref() {
        lines.push(format!("risk_category: {risk_category}"));
    }
    if let Some(actor) = draft.actor.as_deref() {
        lines.push(format!("actor: {actor}"));
    }
    if let Some(model) = draft.model.as_deref() {
        lines.push(format!("model: {model}"));
    }
    if let Some(tool_name) = draft.tool_name.as_deref() {
        lines.push(format!("tool_name: {tool_name}"));
    }
    if let Some(action) = draft.action.as_deref() {
        lines.push(format!("action: {action}"));
    }
    if let Some(policy) = draft.policy.as_deref() {
        lines.push(format!("policy: {policy}"));
    }
    if let Some(approval_state) = draft.approval_state.as_deref() {
        lines.push(format!("approval_state: {approval_state}"));
    }
    if let Some(blast_radius) = draft.blast_radius.as_deref() {
        lines.push(format!("blast_radius: {blast_radius}"));
    }
    if !draft.external_correlation_ids.is_empty() {
        lines.push(format!(
            "external_correlation_ids: {}",
            draft.external_correlation_ids.join(", ")
        ));
    }
    if let Some(expected_destination) = draft.expected_destination.as_deref() {
        lines.push(format!("expected_destination: {expected_destination}"));
    }
    if let Some(gate) = draft.quality_gate.as_ref() {
        if !gate.passed {
            lines.push("quality_gate_status: blocked".to_string());
            if let Some(reason) = gate.blocked_reason.as_deref() {
                lines.push(format!(
                    "quality_gate_reason: {}",
                    truncate_text(reason, 500)
                ));
            }
        }
    }

    if let Some(incident) = incident {
        lines.push(String::new());
        lines.push("### Incident context".to_string());
        lines.push(format!("incident_id: {}", incident.incident_id));
        lines.push(format!("event_type: {}", incident.event_type));
        if let Some(source) = incident.source.as_deref() {
            lines.push(format!("source: {source}"));
        }
        if let Some(component) = incident.component.as_deref() {
            lines.push(format!("component: {component}"));
        }
        if incident.occurrence_count > 1 {
            lines.push(format!("occurrence_count: {}", incident.occurrence_count));
        }
    }

    let evidence_refs = issue_evidence_refs(draft, incident);
    if !evidence_refs.is_empty() {
        lines.push(String::new());
        lines.push("### Evidence".to_string());
        for evidence_ref in evidence_refs {
            lines.push(format!("- {evidence_ref}"));
        }
    }

    let markers = [
        fingerprint_marker(&draft.fingerprint),
        evidence_marker(evidence_digest),
        "<!-- tandem:destination:v1:linear -->".to_string(),
    ];
    let marker_text = markers.join("\n");
    let body_budget = LINEAR_BODY_BYTE_BUDGET
        .saturating_sub(marker_text.len())
        .saturating_sub(LINEAR_BODY_MARKER_SAFE_SPACE);
    let body = truncate_text(&lines.join("\n"), body_budget);
    format!("{body}\n{marker_text}")
}

fn issue_evidence_refs(
    draft: &IncidentMonitorDraftRecord,
    incident: Option<&IncidentMonitorIncidentRecord>,
) -> Vec<String> {
    let mut refs = std::collections::BTreeSet::new();
    for evidence_ref in &draft.evidence_refs {
        if let Some(row) = normalize_body_line(evidence_ref) {
            refs.insert(row);
        }
    }
    if let Some(incident) = incident {
        for evidence_ref in &incident.evidence_refs {
            if let Some(row) = normalize_body_line(evidence_ref) {
                refs.insert(row);
            }
        }
    }
    refs.into_iter().take(LINEAR_EVIDENCE_REF_LIMIT).collect()
}

fn normalize_body_line(value: impl AsRef<str>) -> Option<String> {
    let value = value.as_ref().trim();
    (!value.is_empty()).then(|| truncate_text(value, 1_500))
}

fn fingerprint_marker(fingerprint: &str) -> String {
    format!("<!-- tandem:fingerprint:v1:{fingerprint} -->")
}

fn evidence_marker(digest: &str) -> String {
    format!("<!-- tandem:evidence:v1:{digest} -->")
}

async fn call_list_linear_issues(
    state: &AppState,
    tools: &LinearToolSet,
    team: &str,
    project: &str,
    query: &str,
) -> anyhow::Result<Vec<LinearIssue>> {
    let result = state
        .mcp
        .call_tool(
            &tools.server_name,
            &tools.list_issues,
            json!({
                "team": team,
                "project": project,
                "query": query,
                "limit": 50
            }),
        )
        .await
        .map_err(anyhow::Error::msg)?;
    Ok(extract_linear_issues_from_tool_result(&result))
}

async fn call_create_linear_issue(
    state: &AppState,
    tools: &LinearToolSet,
    team: &str,
    project: &str,
    title: &str,
    description: &str,
    risk_level: Option<&str>,
) -> anyhow::Result<LinearIssue> {
    let preferred = json!({
        "method": "create",
        "team": team,
        "project": project,
        "title": title,
        "description": description,
        "priority": linear_priority(risk_level),
        "labels": [INCIDENT_MONITOR_LABEL],
    });
    let fallback = json!({
        "teamId": team,
        "projectId": project,
        "title": title,
        "description": description,
        "priority": linear_priority(risk_level),
    });
    let first = state
        .mcp
        .call_tool(&tools.server_name, &tools.create_issue, preferred)
        .await;
    let result = match first {
        Ok(result) => result,
        Err(_) => state
            .mcp
            .call_tool(&tools.server_name, &tools.create_issue, fallback)
            .await
            .map_err(anyhow::Error::msg)?,
    };
    if let Some(issue) = extract_linear_issues_from_tool_result(&result)
        .into_iter()
        .next()
    {
        return Ok(issue);
    }
    let fingerprint_marker = description
        .lines()
        .find(|line| line.contains("<!-- tandem:fingerprint:v1:"));
    find_created_linear_issue_after_create(state, tools, team, project, title, fingerprint_marker)
        .await
}

async fn find_created_linear_issue_after_create(
    state: &AppState,
    tools: &LinearToolSet,
    team: &str,
    project: &str,
    title: &str,
    fingerprint_marker: Option<&str>,
) -> anyhow::Result<LinearIssue> {
    let mut last_error = None;
    for delay_ms in [0_u64, 250, 750] {
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        match call_list_linear_issues(
            state,
            tools,
            team,
            project,
            fingerprint_marker.unwrap_or(title),
        )
        .await
        {
            Ok(issues) => {
                if let Some(issue) = issues.into_iter().find(|issue| {
                    issue.title.trim() == title.trim()
                        || fingerprint_marker
                            .is_some_and(|marker| issue.description.contains(marker))
                }) {
                    return Ok(issue);
                }
            }
            Err(error) => {
                last_error = Some(error);
            }
        }
    }
    if let Some(error) = last_error {
        return Err(error).context("Linear issue creation returned no issue payload");
    }
    Err(anyhow::anyhow!(
        "Linear issue creation returned no issue payload"
    ))
}

fn linear_priority(risk_level: Option<&str>) -> u8 {
    match risk_level
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
        .as_str()
    {
        "critical" | "urgent" | "severe" => 1,
        "high" => 2,
        "medium" => 3,
        "low" => 4,
        _ => 3,
    }
}

fn extract_linear_issues_from_tool_result(result: &ToolResult) -> Vec<LinearIssue> {
    let mut out = Vec::new();
    for candidate in tool_result_values(result) {
        collect_linear_issues(&candidate, &mut out);
    }
    dedupe_linear_issues(out)
}

fn tool_result_values(result: &ToolResult) -> Vec<Value> {
    let mut values = Vec::new();
    if let Some(value) = result.metadata.get("result") {
        values.push(value.clone());
    }
    if let Ok(parsed) = serde_json::from_str::<Value>(&result.output) {
        values.push(parsed);
    }
    values
}

fn collect_linear_issues(value: &Value, out: &mut Vec<LinearIssue>) {
    match value {
        Value::Object(map) => {
            let id = value_string(map.get("id").or_else(|| map.get("issue_id")));
            let identifier = value_string(
                map.get("identifier")
                    .or_else(|| map.get("key"))
                    .or_else(|| map.get("number")),
            );
            let title = map
                .get("title")
                .or_else(|| map.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let description = map
                .get("description")
                .or_else(|| map.get("body"))
                .or_else(|| map.get("content"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let url = map
                .get("url")
                .or_else(|| map.get("html_url"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let state = map.get("state").and_then(|value| {
                value.as_str().map(ToString::to_string).or_else(|| {
                    value
                        .get("name")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })
            });
            let state_type = map
                .get("state")
                .and_then(|value| value.get("type"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let issue_like =
                id.is_some() || identifier.is_some() || url.as_deref().is_some_and(is_linear_url);
            if issue_like && (!title.is_empty() || !description.is_empty()) {
                out.push(LinearIssue {
                    id,
                    identifier,
                    title,
                    description,
                    url,
                    state,
                    state_type,
                });
            }
            for nested in map.values() {
                collect_linear_issues(nested, out);
            }
        }
        Value::Array(rows) => {
            for row in rows {
                collect_linear_issues(row, out);
            }
        }
        _ => {}
    }
}

fn value_string(value: Option<&Value>) -> Option<String> {
    value.and_then(|value| {
        if let Some(text) = value.as_str() {
            let text = text.trim();
            if text.is_empty() {
                None
            } else {
                Some(text.to_string())
            }
        } else if value.is_number() {
            Some(value.to_string())
        } else {
            None
        }
    })
}

fn is_linear_url(value: &str) -> bool {
    value.contains("linear.app/")
}

fn dedupe_linear_issues(rows: Vec<LinearIssue>) -> Vec<LinearIssue> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        let key = row
            .identifier
            .clone()
            .or(row.id.clone())
            .or(row.url.clone())
            .unwrap_or_else(|| row.title.clone());
        if seen.insert(key) {
            out.push(row);
        }
    }
    out
}

fn linear_external_id(issue: &LinearIssue) -> Option<String> {
    issue
        .identifier
        .clone()
        .or(issue.id.clone())
        .or(issue.url.clone())
}

fn linear_external_title(issue: &LinearIssue) -> String {
    match issue.identifier.as_deref() {
        Some(identifier) if !identifier.trim().is_empty() => format!("Linear issue {identifier}"),
        _ if !issue.title.trim().is_empty() => issue.title.clone(),
        _ => "Linear issue".to_string(),
    }
}

async fn mirror_linear_post_as_external_action(
    state: &AppState,
    draft: &IncidentMonitorDraftRecord,
    post: &IncidentMonitorPostRecord,
) {
    let capability_id = match post.operation.as_str() {
        "create_issue" => Some("linear.create_issue".to_string()),
        "match_issue" => Some("linear.list_issues".to_string()),
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
    if let Err(error) = AppState::record_external_action(state, action).await {
        tracing::warn!(
            "failed to persist external action mirror for incident monitor Linear post {}: {}",
            post.post_id,
            error
        );
    }
}

#[cfg(test)]
mod linear_recurrence_tests {
    use super::*;

    fn issue(
        identifier: &str,
        description: &str,
        state: Option<&str>,
        state_type: Option<&str>,
    ) -> LinearIssue {
        LinearIssue {
            id: Some(identifier.to_string()),
            identifier: Some(identifier.to_string()),
            title: "Failure".to_string(),
            description: description.to_string(),
            url: None,
            state: state.map(ToString::to_string),
            state_type: state_type.map(ToString::to_string),
        }
    }

    #[test]
    fn terminal_detection_prefers_state_type() {
        // TAN-551: the canonical workflow-state type wins over the display name.
        assert!(linear_issue_is_terminal(&issue(
            "T-1",
            "",
            Some("In Progress"),
            Some("completed")
        )));
        assert!(linear_issue_is_terminal(&issue("T-2", "", None, Some("canceled"))));
        assert!(!linear_issue_is_terminal(&issue(
            "T-3",
            "",
            Some("Done"),
            Some("started")
        )));
    }

    #[test]
    fn terminal_detection_falls_back_to_state_name() {
        assert!(linear_issue_is_terminal(&issue("T-1", "", Some("Done"), None)));
        assert!(linear_issue_is_terminal(&issue("T-2", "", Some("Canceled"), None)));
        assert!(!linear_issue_is_terminal(&issue(
            "T-3",
            "",
            Some("In Progress"),
            None
        )));
        assert!(!linear_issue_is_terminal(&issue("T-4", "", None, None)));
    }

    #[test]
    fn selection_prefers_open_marker_match_over_closed() {
        let marker = fingerprint_marker("fp-1");
        let closed = issue("T-9", &marker, Some("Done"), Some("completed"));
        let open = issue("T-8", &marker, Some("In Progress"), Some("started"));
        let picked =
            select_matching_linear_issue(vec![closed, open], &marker, "evidence-x", "failure", "fp-1");
        assert_eq!(picked.and_then(|issue| issue.identifier), Some("T-8".to_string()));
    }

    #[test]
    fn selection_returns_closed_match_when_no_open_exists() {
        let marker = fingerprint_marker("fp-1");
        let closed = issue("T-9", &marker, Some("Canceled"), Some("canceled"));
        let picked =
            select_matching_linear_issue(vec![closed], &marker, "evidence-x", "failure", "fp-1");
        assert!(picked.as_ref().map(linear_issue_is_terminal).unwrap_or(false));
        assert_eq!(picked.and_then(|issue| issue.identifier), Some("T-9".to_string()));
    }

    #[test]
    fn post_references_issue_matches_by_url_or_id() {
        // TAN-551: the duplicate short-circuit only fires when the prior post
        // points at the currently-matched (still-open) issue.
        let open = issue("T-8", "", Some("In Progress"), Some("started"));
        let mut post = IncidentMonitorPostRecord {
            external_url: Some("https://linear.app/acme/issue/T-8".to_string()),
            ..Default::default()
        };
        // The post's external_id is not set yet and its url doesn't match.
        assert!(!linear_post_references_issue(&post, &open));
        // Matches once the post's external_id lines up with the issue identifier.
        post.external_id = Some("T-8".to_string());
        assert!(linear_post_references_issue(&post, &open));
        // A post pointing at a different issue does not match.
        let other = issue("T-9", "", Some("In Progress"), Some("started"));
        assert!(!linear_post_references_issue(&post, &other));
    }
}
