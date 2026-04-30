use anyhow::Context;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::time::Duration;
use tandem_runtime::mcp_ready::{EnsureReadyPolicy, McpReadyError};
use tandem_runtime::McpRemoteTool;
use tandem_types::EngineEvent;

use crate::bug_monitor::error_provenance::{
    locate_error_provenance, render_provenance_section,
};
use crate::{
    now_ms, sha256_hex, truncate_text, AppState, BugMonitorConfig, BugMonitorDraftRecord,
    BugMonitorIncidentRecord, BugMonitorPostRecord, ExternalActionRecord,
};
use std::path::Path;

const BUG_MONITOR_LABEL: &str = "bug-monitor";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishMode {
    Auto,
    Recovery,
    ManualPublish,
    RecheckOnly,
}

#[derive(Debug, Clone)]
pub struct PublishOutcome {
    pub action: String,
    pub draft: BugMonitorDraftRecord,
    pub post: Option<BugMonitorPostRecord>,
}

pub async fn record_post_failure(
    state: &AppState,
    draft: &BugMonitorDraftRecord,
    incident_id: Option<&str>,
    operation: &str,
    evidence_digest: Option<&str>,
    error: &str,
) -> anyhow::Result<BugMonitorPostRecord> {
    let now = now_ms();
    let post = BugMonitorPostRecord {
        post_id: format!("failure-post-{}", uuid::Uuid::new_v4().simple()),
        draft_id: draft.draft_id.clone(),
        incident_id: incident_id.map(|value| value.to_string()),
        fingerprint: draft.fingerprint.clone(),
        repo: draft.repo.clone(),
        operation: operation.to_string(),
        status: "failed".to_string(),
        issue_number: draft.issue_number,
        issue_url: draft.github_issue_url.clone(),
        comment_id: None,
        comment_url: draft.github_comment_url.clone(),
        evidence_digest: evidence_digest.map(|value| value.to_string()),
        confidence: draft.confidence.clone(),
        risk_level: draft.risk_level.clone(),
        expected_destination: draft.expected_destination.clone(),
        evidence_refs: draft.evidence_refs.clone(),
        quality_gate: draft.quality_gate.clone(),
        idempotency_key: build_idempotency_key(
            &draft.repo,
            &draft.fingerprint,
            operation,
            evidence_digest.unwrap_or(""),
        ),
        response_excerpt: None,
        error: Some(truncate_text(error, 500)),
        created_at_ms: now,
        updated_at_ms: now,
    };
    let post = state.put_bug_monitor_post(post).await?;
    mirror_bug_monitor_post_as_external_action(state, draft, &post).await;
    Ok(post)
}

async fn mirror_bug_monitor_post_as_external_action(
    state: &AppState,
    draft: &BugMonitorDraftRecord,
    post: &BugMonitorPostRecord,
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
        source_kind: Some("bug_monitor".to_string()),
        source_id: Some(draft.draft_id.clone()),
        routine_run_id: None,
        context_run_id: draft.triage_run_id.clone(),
        capability_id,
        provider: Some(BUG_MONITOR_LABEL.to_string()),
        target: Some(draft.repo.clone()),
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
            "issue_number": post.issue_number,
            "issue_url": post.issue_url,
            "comment_id": post.comment_id,
            "comment_url": post.comment_url,
            "response_excerpt": post.response_excerpt,
        })),
        error: post.error.clone(),
        metadata: Some(json!({
            "repo": post.repo,
            "fingerprint": post.fingerprint,
            "evidence_digest": post.evidence_digest,
            "confidence": post.confidence,
            "risk_level": post.risk_level,
            "expected_destination": post.expected_destination,
            "evidence_refs": post.evidence_refs,
            "quality_gate": post.quality_gate,
            "bug_monitor_operation": post.operation,
        })),
        created_at_ms: post.created_at_ms,
        updated_at_ms: post.updated_at_ms,
    };
    if let Err(error) = state.record_external_action(action).await {
        tracing::warn!(
            "failed to persist external action mirror for bug monitor post {}: {}",
            post.post_id,
            error
        );
    }
}

#[derive(Debug, Clone, Default)]
struct GithubToolSet {
    server_name: String,
    list_issues: String,
    get_issue: String,
    create_issue: String,
    comment_on_issue: String,
}

#[derive(Debug, Clone, Default)]
struct GithubIssue {
    number: u64,
    title: String,
    body: String,
    state: String,
    html_url: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct GithubComment {
    id: Option<String>,
    html_url: Option<String>,
}

pub async fn publish_draft(
    state: &AppState,
    draft_id: &str,
    incident_id: Option<&str>,
    mode: PublishMode,
) -> anyhow::Result<PublishOutcome> {
    let status = state.bug_monitor_status_snapshot().await;
    let config = status.config.clone();
    if !config.enabled {
        anyhow::bail!("Bug Monitor is disabled");
    }
    if config.paused && matches!(mode, PublishMode::Auto | PublishMode::Recovery) {
        anyhow::bail!("Bug Monitor is paused");
    }
    if !status.readiness.runtime_ready && mode == PublishMode::Auto {
        anyhow::bail!(
            "{}",
            status
                .last_error
                .clone()
                .unwrap_or_else(|| "Bug Monitor is not ready for GitHub posting".to_string())
        );
    }
    let mut draft = state
        .get_bug_monitor_draft(draft_id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Bug Monitor draft not found"))?;
    if draft.status.eq_ignore_ascii_case("denied") {
        anyhow::bail!("Bug Monitor draft has been denied");
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

    let tools = resolve_github_tool_set(state, &config)
        .await
        .context("resolve GitHub MCP tools for Bug Monitor")?;
    let incident = match incident_id {
        Some(id) => state.get_bug_monitor_incident(id).await,
        None => None,
    };
    let evidence_digest = compute_evidence_digest(&draft, incident.as_ref());
    draft.evidence_digest = Some(evidence_digest.clone());
    if mode != PublishMode::RecheckOnly {
        if let Some(existing) =
            successful_post_for_draft(state, &draft.draft_id, Some(&evidence_digest)).await
        {
            draft.github_status = Some("duplicate_skipped".to_string());
            draft.issue_number = existing.issue_number;
            draft.github_issue_url = existing.issue_url.clone();
            draft.github_comment_url = existing.comment_url.clone();
            draft.github_posted_at_ms = Some(existing.updated_at_ms);
            draft.last_post_error = None;
            mirror_bug_monitor_post_as_external_action(state, &draft, &existing).await;
            let draft = state.put_bug_monitor_draft(draft).await?;
            return Ok(PublishOutcome {
                action: "skip_duplicate".to_string(),
                draft,
                post: Some(existing),
            });
        }
    }
    let issue_draft = if mode == PublishMode::RecheckOnly {
        None
    } else if draft.triage_run_id.is_none() {
        if mode == PublishMode::ManualPublish {
            anyhow::bail!("Bug Monitor draft needs a triage run before GitHub publish");
        }
        None
    } else if mode == PublishMode::ManualPublish {
        Some(
            crate::http::bug_monitor::ensure_bug_monitor_issue_draft(
                state.clone(),
                &draft.draft_id,
                false,
            )
            .await
            .context("generate Bug Monitor issue draft")?,
        )
    } else {
        crate::http::bug_monitor::load_bug_monitor_issue_draft_artifact(
            state,
            draft.triage_run_id.as_deref().unwrap_or_default(),
        )
        .await
    };
    let triage_marked_timed_out = draft
        .github_status
        .as_deref()
        .is_some_and(|status| status.eq_ignore_ascii_case("triage_timed_out"));
    if issue_draft.is_none()
        && draft.triage_run_id.is_some()
        && !triage_marked_timed_out
        && mode == PublishMode::Auto
    {
        draft.github_status = Some("triage_pending".to_string());
        let draft = state.put_bug_monitor_draft(draft).await?;
        return Ok(PublishOutcome {
            action: "triage_pending".to_string(),
            draft,
            post: None,
        });
    }

    let owner_repo = split_owner_repo(&draft.repo)?;
    let matched_issue = find_matching_issue(state, &tools, &owner_repo, &draft)
        .await
        .context("match existing GitHub issue for Bug Monitor draft")?;

    match matched_issue {
        Some(issue) if issue.state.eq_ignore_ascii_case("open") => {
            draft.matched_issue_number = Some(issue.number);
            draft.matched_issue_state = Some(issue.state.clone());
            if mode == PublishMode::RecheckOnly {
                let draft = state.put_bug_monitor_draft(draft).await?;
                return Ok(PublishOutcome {
                    action: "matched_open".to_string(),
                    draft,
                    post: None,
                });
            }
            if !config.auto_comment_on_matched_open_issues {
                draft.github_status = Some("draft_ready".to_string());
                let draft = state.put_bug_monitor_draft(draft).await?;
                return Ok(PublishOutcome {
                    action: "matched_open_no_comment".to_string(),
                    draft,
                    post: None,
                });
            }
            let idempotency_key = build_idempotency_key(
                &draft.repo,
                &draft.fingerprint,
                "comment_issue",
                &evidence_digest,
            );
            if let Some(existing) = successful_post_by_idempotency(state, &idempotency_key).await {
                draft.github_status = Some("duplicate_skipped".to_string());
                draft.issue_number = existing.issue_number;
                draft.github_issue_url = existing.issue_url.clone();
                draft.github_comment_url = existing.comment_url.clone();
                draft.github_posted_at_ms = Some(existing.updated_at_ms);
                draft.last_post_error = None;
                mirror_bug_monitor_post_as_external_action(state, &draft, &existing).await;
                let draft = state.put_bug_monitor_draft(draft).await?;
                return Ok(PublishOutcome {
                    action: "skip_duplicate".to_string(),
                    draft,
                    post: Some(existing),
                });
            }
            let body = build_comment_body(
                &draft,
                incident.as_ref(),
                issue.number,
                &evidence_digest,
                issue_draft.as_ref(),
            );
            let body = append_error_provenance_section(body, &draft, incident.as_ref()).await;
            let result = call_add_issue_comment(state, &tools, &owner_repo, issue.number, &body)
                .await
                .context("post Bug Monitor comment to GitHub")?;
            let post = BugMonitorPostRecord {
                post_id: format!("failure-post-{}", uuid::Uuid::new_v4().simple()),
                draft_id: draft.draft_id.clone(),
                incident_id: incident.as_ref().map(|row| row.incident_id.clone()),
                fingerprint: draft.fingerprint.clone(),
                repo: draft.repo.clone(),
                operation: "comment_issue".to_string(),
                status: "posted".to_string(),
                issue_number: Some(issue.number),
                issue_url: issue.html_url.clone(),
                comment_id: result.id.clone(),
                comment_url: result.html_url.clone(),
                evidence_digest: Some(evidence_digest.clone()),
                confidence: draft.confidence.clone(),
                risk_level: draft.risk_level.clone(),
                expected_destination: draft.expected_destination.clone(),
                evidence_refs: draft.evidence_refs.clone(),
                quality_gate: draft.quality_gate.clone(),
                idempotency_key,
                response_excerpt: Some(truncate_text(&body, 400)),
                error: None,
                created_at_ms: now_ms(),
                updated_at_ms: now_ms(),
            };
            let post = state.put_bug_monitor_post(post).await?;
            mirror_bug_monitor_post_as_external_action(state, &draft, &post).await;
            draft.status = "github_comment_posted".to_string();
            draft.github_status = Some("github_comment_posted".to_string());
            draft.github_issue_url = issue.html_url.clone();
            draft.github_comment_url = result.html_url.clone();
            draft.github_posted_at_ms = Some(post.updated_at_ms);
            draft.issue_number = Some(issue.number);
            draft.last_post_error = None;
            let draft = state.put_bug_monitor_draft(draft).await?;
            state
                .update_bug_monitor_runtime_status(|runtime| {
                    runtime.last_post_result = Some(format!("commented issue #{}", issue.number));
                })
                .await;
            state.event_bus.publish(EngineEvent::new(
                "bug_monitor.github.comment_posted",
                json!({
                    "draft_id": draft.draft_id,
                    "issue_number": issue.number,
                    "repo": draft.repo,
                }),
            ));
            Ok(PublishOutcome {
                action: "comment_issue".to_string(),
                draft,
                post: Some(post),
            })
        }
        Some(issue) => {
            draft.matched_issue_number = Some(issue.number);
            draft.matched_issue_state = Some(issue.state.clone());
            if mode == PublishMode::RecheckOnly {
                let draft = state.put_bug_monitor_draft(draft).await?;
                return Ok(PublishOutcome {
                    action: "matched_closed".to_string(),
                    draft,
                    post: None,
                });
            }
            create_issue_from_draft(
                state,
                &tools,
                &config,
                draft,
                incident.as_ref(),
                Some(&issue),
                &evidence_digest,
                issue_draft.as_ref(),
            )
            .await
        }
        None => {
            if mode == PublishMode::RecheckOnly {
                let draft = state.put_bug_monitor_draft(draft).await?;
                return Ok(PublishOutcome {
                    action: "no_match".to_string(),
                    draft,
                    post: None,
                });
            }
            create_issue_from_draft(
                state,
                &tools,
                &config,
                draft,
                incident.as_ref(),
                None,
                &evidence_digest,
                issue_draft.as_ref(),
            )
            .await
        }
    }
}

async fn create_issue_from_draft(
    state: &AppState,
    tools: &GithubToolSet,
    config: &BugMonitorConfig,
    mut draft: BugMonitorDraftRecord,
    incident: Option<&crate::BugMonitorIncidentRecord>,
    matched_closed_issue: Option<&GithubIssue>,
    evidence_digest: &str,
    issue_draft: Option<&Value>,
) -> anyhow::Result<PublishOutcome> {
    if config.require_approval_for_new_issues && !draft.status.eq_ignore_ascii_case("draft_ready") {
        draft.status = "approval_required".to_string();
        draft.github_status = Some("approval_required".to_string());
        let draft = state.put_bug_monitor_draft(draft).await?;
        return Ok(PublishOutcome {
            action: "approval_required".to_string(),
            draft,
            post: None,
        });
    }
    if !config.auto_create_new_issues && draft.status.eq_ignore_ascii_case("draft_ready") {
        let draft = state.put_bug_monitor_draft(draft).await?;
        return Ok(PublishOutcome {
            action: "draft_ready".to_string(),
            draft,
            post: None,
        });
    }
    let idempotency_key = build_idempotency_key(
        &draft.repo,
        &draft.fingerprint,
        "create_issue",
        evidence_digest,
    );
    if let Some(existing) = successful_post_by_idempotency(state, &idempotency_key).await {
        draft.status = "github_issue_created".to_string();
        draft.github_status = Some("github_issue_created".to_string());
        draft.issue_number = existing.issue_number;
        draft.github_issue_url = existing.issue_url.clone();
        draft.github_posted_at_ms = Some(existing.updated_at_ms);
        draft.last_post_error = None;
        mirror_bug_monitor_post_as_external_action(state, &draft, &existing).await;
        let draft = state.put_bug_monitor_draft(draft).await?;
        return Ok(PublishOutcome {
            action: "skip_duplicate".to_string(),
            draft,
            post: Some(existing),
        });
    }

    let owner_repo = split_owner_repo(&draft.repo)?;
    let title = issue_draft
        .and_then(|row| row.get("suggested_title"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| draft.title.as_deref().unwrap_or("Bug Monitor issue"));
    let body = issue_draft
        .and_then(|row| row.get("rendered_body"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            build_issue_body(&draft, incident, matched_closed_issue, evidence_digest)
        });
    let body = append_error_provenance_section(body, &draft, incident).await;
    let created = call_create_issue(state, tools, &owner_repo, title, &body)
        .await
        .context("create Bug Monitor issue on GitHub")?;
    let post = BugMonitorPostRecord {
        post_id: format!("failure-post-{}", uuid::Uuid::new_v4().simple()),
        draft_id: draft.draft_id.clone(),
        incident_id: incident.map(|row| row.incident_id.clone()),
        fingerprint: draft.fingerprint.clone(),
        repo: draft.repo.clone(),
        operation: "create_issue".to_string(),
        status: "posted".to_string(),
        issue_number: Some(created.number),
        issue_url: created.html_url.clone(),
        comment_id: None,
        comment_url: None,
        evidence_digest: Some(evidence_digest.to_string()),
        confidence: draft.confidence.clone(),
        risk_level: draft.risk_level.clone(),
        expected_destination: draft.expected_destination.clone(),
        evidence_refs: draft.evidence_refs.clone(),
        quality_gate: draft.quality_gate.clone(),
        idempotency_key,
        response_excerpt: Some(truncate_text(&body, 400)),
        error: None,
        created_at_ms: now_ms(),
        updated_at_ms: now_ms(),
    };
    let post = state.put_bug_monitor_post(post).await?;
    mirror_bug_monitor_post_as_external_action(state, &draft, &post).await;
    draft.status = "github_issue_created".to_string();
    draft.github_status = Some("github_issue_created".to_string());
    draft.github_issue_url = created.html_url.clone();
    draft.github_posted_at_ms = Some(post.updated_at_ms);
    draft.issue_number = Some(created.number);
    draft.last_post_error = None;
    let draft = state.put_bug_monitor_draft(draft).await?;
    state
        .update_bug_monitor_runtime_status(|runtime| {
            runtime.last_post_result = Some(format!("created issue #{}", created.number));
        })
        .await;
    state.event_bus.publish(EngineEvent::new(
        "bug_monitor.github.issue_created",
        json!({
            "draft_id": draft.draft_id,
            "issue_number": created.number,
            "repo": draft.repo,
        }),
    ));
    Ok(PublishOutcome {
        action: "create_issue".to_string(),
        draft,
        post: Some(post),
    })
}

async fn resolve_github_tool_set(
    state: &AppState,
    config: &BugMonitorConfig,
) -> anyhow::Result<GithubToolSet> {
    let server_name = config
        .mcp_server
        .as_ref()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("Bug Monitor MCP server is not configured"))?
        .to_string();
    state
        .mcp
        .ensure_ready(&server_name, EnsureReadyPolicy::with_retries(3, 750))
        .await
        .map_err(|error| match error {
            McpReadyError::NotFound => {
                anyhow::anyhow!("Bug Monitor MCP server `{server_name}` was not found")
            }
            McpReadyError::Disabled => {
                anyhow::anyhow!("Bug Monitor MCP server `{server_name}` is disabled")
            }
            McpReadyError::PermanentlyFailed { last_error } => {
                let detail = last_error.unwrap_or_else(|| "connect failed".to_string());
                anyhow::anyhow!(
                    "Bug Monitor MCP server `{server_name}` was not ready for GitHub publish: {detail}"
                )
            }
        })?;
    let server_tools = state.mcp.server_tools(&server_name).await;
    if server_tools.is_empty() {
        anyhow::bail!("no MCP tools were discovered for selected Bug Monitor server");
    }
    let discovered = state
        .capability_resolver
        .discover_from_runtime(server_tools.clone(), Vec::new())
        .await;
    let mut resolved = state
        .capability_resolver
        .resolve(
            crate::capability_resolver::CapabilityResolveInput {
                workflow_id: Some("bug-monitor-github".to_string()),
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
                    workflow_id: Some("bug-monitor-github".to_string()),
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

async fn find_matching_issue(
    state: &AppState,
    tools: &GithubToolSet,
    owner_repo: &(&str, &str),
    draft: &BugMonitorDraftRecord,
) -> anyhow::Result<Option<GithubIssue>> {
    let mut issues = call_list_issues(state, tools, owner_repo).await?;
    if let Some(existing_number) = draft.issue_number {
        if let Some(existing) = issues
            .iter()
            .find(|row| row.number == existing_number)
            .cloned()
        {
            return Ok(Some(existing));
        }
        if let Ok(issue) = call_get_issue(state, tools, owner_repo, existing_number).await {
            return Ok(Some(issue));
        }
    }
    let marker = fingerprint_marker(&draft.fingerprint);
    issues.sort_by(|a, b| b.number.cmp(&a.number));
    let exact_marker = issues
        .iter()
        .find(|issue| issue.body.contains(&marker))
        .cloned();
    if exact_marker.is_some() {
        return Ok(exact_marker);
    }
    let normalized_title = draft
        .title
        .as_deref()
        .map(|value| value.trim().to_ascii_lowercase())
        .unwrap_or_default();
    Ok(issues.into_iter().find(|issue| {
        issue.title.trim().eq_ignore_ascii_case(&normalized_title)
            || issue.body.contains(&draft.fingerprint)
    }))
}

async fn successful_post_by_idempotency(
    state: &AppState,
    idempotency_key: &str,
) -> Option<BugMonitorPostRecord> {
    state
        .bug_monitor_posts
        .read()
        .await
        .values()
        .find(|row| row.idempotency_key == idempotency_key && row.status == "posted")
        .cloned()
}

async fn successful_post_for_draft(
    state: &AppState,
    draft_id: &str,
    evidence_digest: Option<&str>,
) -> Option<BugMonitorPostRecord> {
    let mut rows = state.list_bug_monitor_posts(200).await;
    rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    rows.into_iter().find(|row| {
        row.draft_id == draft_id
            && row.status == "posted"
            && match evidence_digest {
                Some(expected) => row.evidence_digest.as_deref() == Some(expected),
                None => true,
            }
    })
}

fn compute_evidence_digest(
    draft: &BugMonitorDraftRecord,
    incident: Option<&crate::BugMonitorIncidentRecord>,
) -> String {
    sha256_hex(&[
        draft.repo.as_str(),
        draft.fingerprint.as_str(),
        draft.title.as_deref().unwrap_or(""),
        draft.detail.as_deref().unwrap_or(""),
        draft.triage_run_id.as_deref().unwrap_or(""),
        incident
            .and_then(|row| row.session_id.as_deref())
            .unwrap_or(""),
        incident.and_then(|row| row.run_id.as_deref()).unwrap_or(""),
        incident
            .map(|row| row.occurrence_count.to_string())
            .unwrap_or_default()
            .as_str(),
    ])
}

fn build_idempotency_key(repo: &str, fingerprint: &str, operation: &str, digest: &str) -> String {
    sha256_hex(&[repo, fingerprint, operation, digest])
}

fn build_issue_body(
    draft: &BugMonitorDraftRecord,
    incident: Option<&crate::BugMonitorIncidentRecord>,
    matched_closed_issue: Option<&GithubIssue>,
    evidence_digest: &str,
) -> String {
    let mut lines = Vec::new();
    if let Some(detail) = draft.detail.as_deref() {
        lines.push(truncate_text(detail, 4_000));
    }
    if let Some(run_id) = draft.triage_run_id.as_deref() {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(format!("triage_run_id: {run_id}"));
    }
    if let Some(issue) = matched_closed_issue {
        lines.push(format!(
            "previous_closed_issue: #{} ({})",
            issue.number, issue.state
        ));
    }
    if let Some(incident) = incident {
        lines.push(format!("incident_id: {}", incident.incident_id));
        if let Some(event_type) = Some(incident.event_type.as_str()) {
            lines.push(format!("event_type: {event_type}"));
        }
        if !incident.workspace_root.trim().is_empty() {
            lines.push(format!("local_directory: {}", incident.workspace_root));
        }
    }
    if let Some(logs) = fallback_issue_logs(draft, incident) {
        lines.push(String::new());
        lines.push("### Logs".to_string());
        lines.push("```".to_string());
        lines.push(logs);
        lines.push("```".to_string());
    }
    let evidence_refs = fallback_issue_evidence_refs(draft, incident);
    if !evidence_refs.is_empty() {
        lines.push(String::new());
        lines.push("### Evidence".to_string());
        for evidence_ref in evidence_refs {
            lines.push(format!("- {evidence_ref}"));
        }
    }
    if let Some(incident) = incident {
        let mut metadata = Vec::new();
        if let Some(run_id) = incident.run_id.as_deref() {
            metadata.push(format!("run_id: {run_id}"));
        }
        if let Some(session_id) = incident.session_id.as_deref() {
            metadata.push(format!("session_id: {session_id}"));
        }
        if let Some(correlation_id) = incident.correlation_id.as_deref() {
            metadata.push(format!("correlation_id: {correlation_id}"));
        }
        if let Some(component) = incident.component.as_deref() {
            metadata.push(format!("component: {component}"));
        }
        if let Some(level) = incident.level.as_deref() {
            metadata.push(format!("level: {level}"));
        }
        if incident.occurrence_count > 1 {
            let occurrence_count = incident.occurrence_count;
            metadata.push(format!("occurrence_count: {occurrence_count}"));
        }
        if let Some(last_seen_at_ms) = incident.last_seen_at_ms {
            metadata.push(format!(
                "last_seen_at_ms: {}",
                format_bug_monitor_ms(last_seen_at_ms)
            ));
        }
        if !metadata.is_empty() {
            lines.push(String::new());
            lines.push("### Diagnostic metadata".to_string());
            lines.extend(metadata);
        }
    }
    let mut triage_signal = Vec::new();
    if let Some(confidence) = draft.confidence.as_deref() {
        triage_signal.push(format!("confidence: {confidence}"));
    }
    if let Some(risk_level) = draft.risk_level.as_deref() {
        triage_signal.push(format!("risk_level: {risk_level}"));
    }
    if let Some(expected_destination) = draft.expected_destination.as_deref() {
        triage_signal.push(format!("expected_destination: {expected_destination}"));
    }
    if let Some(gate) = draft.quality_gate.as_ref() {
        if !gate.passed {
            triage_signal.push("quality_gate_status: blocked".to_string());
            if !gate.missing.is_empty() {
                triage_signal.push("quality_gate_missing:".to_string());
                for missing in gate.missing.iter().take(20) {
                    triage_signal.push(format!("- {missing}"));
                }
            }
            if let Some(reason) = gate.blocked_reason.as_deref() {
                triage_signal.push(format!(
                    "quality_gate_reason: {}",
                    truncate_text(reason, 500)
                ));
            }
        }
    }
    if !triage_signal.is_empty() {
        lines.push(String::new());
        lines.push("### Triage signal".to_string());
        lines.extend(triage_signal);
    }
    if let Some(status) = fallback_issue_triage_status(draft.github_status.as_deref()) {
        lines.push(String::new());
        lines.push("### Triage status".to_string());
        lines.push(format!("triage_status: {status}"));
        if status == "triage_timed_out" {
            if let Some(diagnostics) = draft
                .last_post_error
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .filter(|s| s.contains('\n'))
            {
                lines.push(String::new());
                lines.push("### Triage timeout details".to_string());
                for line in diagnostics.lines().take(20) {
                    lines.push(line.to_string());
                }
            }
        }
    }
    lines.push(String::new());
    let markers = [
        fingerprint_marker(&draft.fingerprint),
        evidence_marker(evidence_digest),
    ];
    let marker_text = markers.join("\n");
    let body_budget = 12_000usize
        .saturating_sub(marker_text.len())
        .saturating_sub(2);
    let body = truncate_text(&lines.join("\n"), body_budget);
    format!("{body}\n{marker_text}")
}

fn fallback_issue_logs(
    draft: &BugMonitorDraftRecord,
    incident: Option<&crate::BugMonitorIncidentRecord>,
) -> Option<String> {
    let rows = incident
        .map(|row| {
            row.excerpt
                .iter()
                .filter_map(|line| normalize_issue_body_line(line))
                .take(30)
                .collect::<Vec<_>>()
        })
        .filter(|rows| !rows.is_empty())
        .unwrap_or_else(|| {
            draft
                .detail
                .as_deref()
                .unwrap_or_default()
                .lines()
                .filter_map(normalize_issue_body_line)
                .take(12)
                .collect::<Vec<_>>()
        });
    if rows.is_empty() {
        None
    } else {
        Some(truncate_text(&rows.join("\n"), 4_000))
    }
}

fn fallback_issue_evidence_refs(
    draft: &BugMonitorDraftRecord,
    incident: Option<&crate::BugMonitorIncidentRecord>,
) -> Vec<String> {
    let mut refs = BTreeSet::new();
    for evidence_ref in draft.evidence_refs.iter() {
        if let Some(row) = normalize_issue_body_line(evidence_ref) {
            refs.insert(row);
        }
    }
    if let Some(incident) = incident {
        for evidence_ref in incident.evidence_refs.iter() {
            if let Some(row) = normalize_issue_body_line(evidence_ref) {
                refs.insert(row);
            }
        }
    }
    refs.into_iter().take(15).collect()
}

fn normalize_issue_body_line(value: impl AsRef<str>) -> Option<String> {
    let value = value.as_ref().trim();
    (!value.is_empty()).then(|| truncate_text(value, 1_500))
}

fn format_bug_monitor_ms(ms: u64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms as i64)
        .map(|value| value.to_rfc3339())
        .unwrap_or_else(|| ms.to_string())
}

fn fallback_issue_triage_status(status: Option<&str>) -> Option<&str> {
    match status {
        Some("triage_timed_out" | "triage_pending" | "github_post_failed") => status,
        _ => None,
    }
}

fn build_comment_body(
    draft: &BugMonitorDraftRecord,
    incident: Option<&crate::BugMonitorIncidentRecord>,
    issue_number: u64,
    evidence_digest: &str,
    issue_draft: Option<&Value>,
) -> String {
    let mut lines = vec![format!(
        "New Bug Monitor evidence detected for #{issue_number}."
    )];
    if let Some(summary) = issue_draft
        .and_then(|row| row.get("what_happened"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
    {
        lines.push(String::new());
        lines.push(truncate_text(summary, 1_500));
    } else if let Some(detail) = draft.detail.as_deref() {
        lines.push(String::new());
        lines.push(truncate_text(detail, 1_500));
    }
    if let Some(logs) = issue_draft
        .and_then(|row| row.get("logs"))
        .and_then(Value::as_array)
        .filter(|rows| !rows.is_empty())
    {
        lines.push(String::new());
        lines.push("logs:".to_string());
        for line in logs.iter().filter_map(Value::as_str).take(6) {
            lines.push(format!("  {line}"));
        }
    }
    if let Some(incident) = incident {
        lines.push(String::new());
        lines.push(format!("incident_id: {}", incident.incident_id));
        if let Some(run_id) = incident.run_id.as_deref() {
            lines.push(format!("run_id: {run_id}"));
        }
        if let Some(session_id) = incident.session_id.as_deref() {
            lines.push(format!("session_id: {session_id}"));
        }
    }
    if let Some(run_id) = draft.triage_run_id.as_deref() {
        lines.push(format!("triage_run_id: {run_id}"));
    }
    lines.push(String::new());
    lines.push(evidence_marker(evidence_digest));
    lines.join("\n")
}

fn fingerprint_marker(fingerprint: &str) -> String {
    format!("<!-- tandem:fingerprint:v1:{fingerprint} -->")
}

fn evidence_marker(digest: &str) -> String {
    format!("<!-- tandem:evidence:v1:{digest} -->")
}

fn split_owner_repo(repo: &str) -> anyhow::Result<(&str, &str)> {
    let mut parts = repo.split('/');
    let owner = parts
        .next()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("invalid owner/repo value"))?;
    let repo_name = parts
        .next()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("invalid owner/repo value"))?;
    if parts.next().is_some() {
        anyhow::bail!("invalid owner/repo value");
    }
    Ok((owner, repo_name))
}

async fn call_list_issues(
    state: &AppState,
    tools: &GithubToolSet,
    (owner, repo): &(&str, &str),
) -> anyhow::Result<Vec<GithubIssue>> {
    let result = state
        .mcp
        .call_tool(
            &tools.server_name,
            &tools.list_issues,
            json!({
                "owner": owner,
                "repo": repo,
                "state": "all",
                "perPage": 100
            }),
        )
        .await
        .map_err(anyhow::Error::msg)?;
    Ok(extract_issues_from_tool_result(&result))
}

async fn call_get_issue(
    state: &AppState,
    tools: &GithubToolSet,
    (owner, repo): &(&str, &str),
    issue_number: u64,
) -> anyhow::Result<GithubIssue> {
    let result = state
        .mcp
        .call_tool(
            &tools.server_name,
            &tools.get_issue,
            json!({
                "owner": owner,
                "repo": repo,
                "issue_number": issue_number
            }),
        )
        .await
        .map_err(anyhow::Error::msg)?;
    extract_issues_from_tool_result(&result)
        .into_iter()
        .find(|issue| issue.number == issue_number)
        .ok_or_else(|| anyhow::anyhow!("GitHub issue #{issue_number} was not returned"))
}

async fn call_create_issue(
    state: &AppState,
    tools: &GithubToolSet,
    (owner, repo): &(&str, &str),
    title: &str,
    body: &str,
) -> anyhow::Result<GithubIssue> {
    let preferred = json!({
        "method": "create",
        "owner": owner,
        "repo": repo,
        "title": title,
        "body": body,
        "labels": [BUG_MONITOR_LABEL],
    });
    let fallback = json!({
        "owner": owner,
        "repo": repo,
        "title": title,
        "body": body,
        "labels": [BUG_MONITOR_LABEL],
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
    if let Some(issue) = extract_issues_from_tool_result(&result).into_iter().next() {
        return Ok(issue);
    }
    let fingerprint_marker = body
        .lines()
        .find(|line| line.contains("<!-- tandem:fingerprint:v1:"));
    find_created_issue_after_create(state, tools, &(owner, repo), title, fingerprint_marker).await
}

async fn find_created_issue_after_create(
    state: &AppState,
    tools: &GithubToolSet,
    owner_repo: &(&str, &str),
    title: &str,
    fingerprint_marker: Option<&str>,
) -> anyhow::Result<GithubIssue> {
    let mut last_error = None;
    for delay_ms in [0_u64, 250, 750, 1500] {
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
        match call_list_issues(state, tools, owner_repo).await {
            Ok(issues) => {
                if let Some(issue) = issues.into_iter().find(|issue| {
                    issue.title.trim() == title.trim()
                        || fingerprint_marker.is_some_and(|marker| issue.body.contains(marker))
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
        return Err(error).context("GitHub issue creation returned no issue payload");
    }
    Err(anyhow::anyhow!(
        "GitHub issue creation returned no issue payload"
    ))
}

async fn call_add_issue_comment(
    state: &AppState,
    tools: &GithubToolSet,
    (owner, repo): &(&str, &str),
    issue_number: u64,
    body: &str,
) -> anyhow::Result<GithubComment> {
    let result = state
        .mcp
        .call_tool(
            &tools.server_name,
            &tools.comment_on_issue,
            json!({
                "owner": owner,
                "repo": repo,
                "issue_number": issue_number,
                "body": body
            }),
        )
        .await
        .map_err(anyhow::Error::msg)?;
    extract_comments_from_tool_result(&result)
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("GitHub comment creation returned no comment payload"))
}

fn extract_issues_from_tool_result(result: &tandem_types::ToolResult) -> Vec<GithubIssue> {
    let mut out = Vec::new();
    for candidate in tool_result_values(result) {
        collect_issues(&candidate, &mut out);
    }
    dedupe_issues(out)
}

fn extract_comments_from_tool_result(result: &tandem_types::ToolResult) -> Vec<GithubComment> {
    let mut out = Vec::new();
    for candidate in tool_result_values(result) {
        collect_comments(&candidate, &mut out);
    }
    dedupe_comments(out)
}

fn tool_result_values(result: &tandem_types::ToolResult) -> Vec<Value> {
    let mut values = Vec::new();
    if let Some(value) = result.metadata.get("result") {
        values.push(value.clone());
    }
    if let Ok(parsed) = serde_json::from_str::<Value>(&result.output) {
        values.push(parsed);
    }
    values
}

fn collect_issues(value: &Value, out: &mut Vec<GithubIssue>) {
    match value {
        Value::Object(map) => {
            let issue_number = map
                .get("number")
                .or_else(|| map.get("issue_number"))
                .and_then(Value::as_u64);
            let title = map
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let body = map
                .get("body")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let state = map
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let html_url = map
                .get("html_url")
                .or_else(|| map.get("url"))
                .and_then(Value::as_str)
                .map(|value| value.to_string());
            if let Some(number) = issue_number {
                if !title.is_empty() || !body.is_empty() || !state.is_empty() {
                    out.push(GithubIssue {
                        number,
                        title,
                        body,
                        state,
                        html_url,
                    });
                }
            }
            for nested in map.values() {
                collect_issues(nested, out);
            }
        }
        Value::Array(rows) => {
            for row in rows {
                collect_issues(row, out);
            }
        }
        _ => {}
    }
}

fn collect_comments(value: &Value, out: &mut Vec<GithubComment>) {
    match value {
        Value::Object(map) => {
            if map.contains_key("id") && (map.contains_key("html_url") || map.contains_key("url")) {
                out.push(GithubComment {
                    id: map.get("id").map(|value| {
                        value
                            .as_str()
                            .map(|row| row.to_string())
                            .unwrap_or_else(|| value.to_string())
                    }),
                    html_url: map
                        .get("html_url")
                        .or_else(|| map.get("url"))
                        .and_then(Value::as_str)
                        .map(|value| value.to_string()),
                });
            }
            for nested in map.values() {
                collect_comments(nested, out);
            }
        }
        Value::Array(rows) => {
            for row in rows {
                collect_comments(row, out);
            }
        }
        _ => {}
    }
}

fn dedupe_issues(rows: Vec<GithubIssue>) -> Vec<GithubIssue> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        if seen.insert(row.number) {
            out.push(row);
        }
    }
    out
}

fn dedupe_comments(rows: Vec<GithubComment>) -> Vec<GithubComment> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        let key = row.id.clone().or(row.html_url.clone()).unwrap_or_default();
        if !key.is_empty() && seen.insert(key) {
            out.push(row);
        }
    }
    out
}

/// Run the deterministic error-string → workspace-source grep and
/// append a markdown "Error provenance" section to the issue body.
/// Best-effort: any failure to locate provenance just leaves the body
/// unchanged. The added section is bounded; see `error_provenance`.
async fn append_error_provenance_section(
    body: String,
    draft: &BugMonitorDraftRecord,
    incident: Option<&BugMonitorIncidentRecord>,
) -> String {
    let Some(error_message) = pick_error_message_for_provenance(draft, incident) else {
        return body;
    };
    let workspace_root = pick_workspace_root_for_provenance(incident);
    let Some(workspace_root) = workspace_root else {
        return body;
    };
    let hits = locate_error_provenance(&workspace_root, &error_message).await;
    let Some(section) = render_provenance_section(&hits) else {
        return body;
    };
    let mut combined = body;
    if !combined.ends_with('\n') {
        combined.push('\n');
    }
    combined.push('\n');
    combined.push_str(&section);
    combined
}

fn pick_error_message_for_provenance(
    draft: &BugMonitorDraftRecord,
    incident: Option<&BugMonitorIncidentRecord>,
) -> Option<String> {
    let candidates = [
        incident.and_then(|row| row.last_error.clone()),
        incident.and_then(|row| row.detail.clone()),
        draft.detail.clone(),
        incident.map(|row| row.title.clone()),
        draft.title.clone(),
    ];
    candidates
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

fn pick_workspace_root_for_provenance(
    incident: Option<&BugMonitorIncidentRecord>,
) -> Option<std::path::PathBuf> {
    let raw = incident.map(|row| row.workspace_root.trim()).unwrap_or("");
    if raw.is_empty() {
        return None;
    }
    let path = Path::new(raw);
    if !path.is_absolute() {
        return None;
    }
    if !path.exists() {
        return None;
    }
    Some(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_types::ToolResult;

    #[test]
    fn build_issue_body_includes_hidden_markers() {
        let draft = BugMonitorDraftRecord {
            draft_id: "draft-1".to_string(),
            fingerprint: "abc123".to_string(),
            repo: "acme/platform".to_string(),
            status: "draft_ready".to_string(),
            created_at_ms: 1,
            triage_run_id: Some("triage-1".to_string()),
            issue_number: None,
            title: Some("session.error detected".to_string()),
            detail: Some("summary".to_string()),
            ..BugMonitorDraftRecord::default()
        };
        let body = build_issue_body(&draft, None, None, "digest-1");
        assert!(body.contains("<!-- tandem:fingerprint:v1:abc123 -->"));
        assert!(body.contains("<!-- tandem:evidence:v1:digest-1 -->"));
        assert!(body.contains("triage_run_id: triage-1"));
    }

    #[test]
    fn build_issue_body_renders_incident_excerpt_as_fenced_logs() {
        let draft = BugMonitorDraftRecord {
            draft_id: "draft-logs".to_string(),
            fingerprint: "log-fingerprint".to_string(),
            repo: "acme/platform".to_string(),
            status: "draft_ready".to_string(),
            created_at_ms: 1,
            detail: Some("fallback detail".to_string()),
            ..BugMonitorDraftRecord::default()
        };
        let incident = crate::BugMonitorIncidentRecord {
            incident_id: "incident-logs".to_string(),
            fingerprint: draft.fingerprint.clone(),
            event_type: "workflow.run.failed".to_string(),
            status: "triage_queued".to_string(),
            repo: draft.repo.clone(),
            workspace_root: "/tmp/acme".to_string(),
            title: "Workflow failed".to_string(),
            excerpt: vec![
                "first failure line".to_string(),
                "second failure line".to_string(),
            ],
            ..crate::BugMonitorIncidentRecord::default()
        };
        let body = build_issue_body(&draft, Some(&incident), None, "digest-logs");
        assert!(body.contains("### Logs\n```\nfirst failure line\nsecond failure line\n```"));
        assert!(body.contains("incident_id: incident-logs"));
        assert!(body.contains("event_type: workflow.run.failed"));
    }

    #[test]
    fn build_issue_body_renders_deduped_evidence_refs() {
        let draft = BugMonitorDraftRecord {
            draft_id: "draft-evidence".to_string(),
            fingerprint: "evidence-fingerprint".to_string(),
            repo: "acme/platform".to_string(),
            status: "draft_ready".to_string(),
            created_at_ms: 1,
            evidence_refs: vec![
                "artifacts/shared.json".to_string(),
                "artifacts/draft-only.log".to_string(),
            ],
            ..BugMonitorDraftRecord::default()
        };
        let incident = crate::BugMonitorIncidentRecord {
            incident_id: "incident-evidence".to_string(),
            fingerprint: draft.fingerprint.clone(),
            event_type: "workflow.run.failed".to_string(),
            status: "triage_queued".to_string(),
            repo: draft.repo.clone(),
            workspace_root: "/tmp/acme".to_string(),
            title: "Workflow failed".to_string(),
            evidence_refs: vec![
                "artifacts/shared.json".to_string(),
                "artifacts/incident-only.log".to_string(),
            ],
            ..crate::BugMonitorIncidentRecord::default()
        };
        let body = build_issue_body(&draft, Some(&incident), None, "digest-evidence");
        assert!(body.contains("### Evidence"));
        assert_eq!(body.matches("- artifacts/shared.json").count(), 1);
        assert!(body.contains("- artifacts/draft-only.log"));
        assert!(body.contains("- artifacts/incident-only.log"));
    }

    #[test]
    fn build_issue_body_renders_only_present_diagnostic_metadata() {
        let draft = BugMonitorDraftRecord {
            draft_id: "draft-metadata".to_string(),
            fingerprint: "metadata-fingerprint".to_string(),
            repo: "acme/platform".to_string(),
            status: "draft_ready".to_string(),
            created_at_ms: 1,
            ..BugMonitorDraftRecord::default()
        };
        let incident = crate::BugMonitorIncidentRecord {
            incident_id: "incident-metadata".to_string(),
            fingerprint: draft.fingerprint.clone(),
            event_type: "workflow.run.failed".to_string(),
            status: "triage_queued".to_string(),
            repo: draft.repo.clone(),
            workspace_root: "/tmp/acme".to_string(),
            title: "Workflow failed".to_string(),
            run_id: Some("run-1".to_string()),
            component: Some("automation_v2".to_string()),
            occurrence_count: 3,
            last_seen_at_ms: Some(1_777_485_515_668),
            ..crate::BugMonitorIncidentRecord::default()
        };
        let body = build_issue_body(&draft, Some(&incident), None, "digest-metadata");
        assert!(body.contains("### Diagnostic metadata"));
        assert!(body.contains("run_id: run-1"));
        assert!(body.contains("component: automation_v2"));
        assert!(body.contains("occurrence_count: 3"));
        assert!(body.contains("last_seen_at_ms: 2026-04-29T"));
        assert!(!body.contains("session_id:"));
        assert!(!body.contains("correlation_id:"));
        assert!(!body.contains("level:"));
    }

    #[test]
    fn build_issue_body_renders_fallback_triage_status_for_known_states() {
        let mut draft = BugMonitorDraftRecord {
            draft_id: "draft-status".to_string(),
            fingerprint: "status-fingerprint".to_string(),
            repo: "acme/platform".to_string(),
            status: "draft_ready".to_string(),
            created_at_ms: 1,
            github_status: Some("triage_timed_out".to_string()),
            confidence: Some("medium".to_string()),
            risk_level: Some("medium".to_string()),
            expected_destination: Some("bug_monitor_issue_draft".to_string()),
            quality_gate: Some(crate::BugMonitorQualityGateReport {
                stage: "draft_to_proposal".to_string(),
                status: "blocked".to_string(),
                passed: false,
                passed_count: 2,
                total_count: 4,
                gates: Vec::new(),
                missing: vec!["research_performed".to_string()],
                blocked_reason: Some("triage timed out".to_string()),
            }),
            ..BugMonitorDraftRecord::default()
        };
        let body = build_issue_body(&draft, None, None, "digest-status");
        assert!(body.contains("### Triage signal"));
        assert!(body.contains("confidence: medium"));
        assert!(body.contains("quality_gate_status: blocked"));
        assert!(body.contains("- research_performed"));
        assert!(body.contains("quality_gate_reason: triage timed out"));
        assert!(body.contains("triage_status: triage_timed_out"));

        draft.github_status = Some("issue_draft_ready".to_string());
        let body = build_issue_body(&draft, None, None, "digest-status");
        assert!(!body.contains("triage_status:"));
        draft.github_status = None;
        let body = build_issue_body(&draft, None, None, "digest-status");
        assert!(!body.contains("triage_status:"));
    }

    #[test]
    fn build_issue_body_truncates_long_excerpt() {
        let draft = BugMonitorDraftRecord {
            draft_id: "draft-long".to_string(),
            fingerprint: "long-fingerprint".to_string(),
            repo: "acme/platform".to_string(),
            status: "draft_ready".to_string(),
            created_at_ms: 1,
            ..BugMonitorDraftRecord::default()
        };
        let incident = crate::BugMonitorIncidentRecord {
            incident_id: "incident-long".to_string(),
            fingerprint: draft.fingerprint.clone(),
            event_type: "workflow.run.failed".to_string(),
            status: "triage_queued".to_string(),
            repo: draft.repo.clone(),
            workspace_root: "/tmp/acme".to_string(),
            title: "Workflow failed".to_string(),
            excerpt: vec!["x".repeat(8_000)],
            ..crate::BugMonitorIncidentRecord::default()
        };
        let body = build_issue_body(&draft, Some(&incident), None, "digest-long");
        assert!(body.len() < 12_500);
        assert!(body.contains("<!-- tandem:evidence:v1:digest-long -->"));
    }

    #[test]
    fn extract_issues_from_official_github_mcp_result() {
        let result = ToolResult {
            output: String::new(),
            metadata: json!({
                "result": {
                    "issues": [
                        {
                            "number": 42,
                            "title": "Bug Monitor issue",
                            "body": "details\n<!-- tandem:fingerprint:v1:deadbeef -->",
                            "state": "open",
                            "html_url": "https://github.com/acme/platform/issues/42"
                        }
                    ]
                }
            }),
        };
        let issues = extract_issues_from_tool_result(&result);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].number, 42);
        assert_eq!(issues[0].state, "open");
        assert!(issues[0].body.contains("deadbeef"));
    }
}
