use anyhow::Context;
use serde_json::{json, Value};
use tandem_runtime::mcp_ready::{EnsureReadyPolicy, McpReadyError};
use tandem_runtime::McpRemoteTool;
use tandem_types::EngineEvent;

use crate::{
    now_ms, sha256_hex, truncate_text, AppState, BugMonitorConfig, BugMonitorDraftRecord,
    BugMonitorPostRecord, ExternalActionRecord,
};

const BUG_MONITOR_LABEL: &str = "bug-monitor";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishMode {
    Auto,
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
    let status = state.bug_monitor_status().await;
    let config = status.config.clone();
    if !config.enabled {
        anyhow::bail!("Bug Monitor is disabled");
    }
    if config.paused && mode == PublishMode::Auto {
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
    if issue_draft.is_none()
        && draft.triage_run_id.is_some()
        && draft.triage_timed_out_at_ms.is_none()
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
        direct_tool_name_fallback(&["list_repository_issues", "mcp.github.list_issues"])
            .ok_or_else(|| anyhow::anyhow!("missing resolved tool for github.list_issues"))
    })?;
    let get_issue = tool_name("github.get_issue").or_else(|_| {
        direct_tool_name_fallback(&["get_issue", "mcp.github.get_issue"])
            .ok_or_else(|| anyhow::anyhow!("missing resolved tool for github.get_issue"))
    })?;
    let create_issue = tool_name("github.create_issue").or_else(|_| {
        direct_tool_name_fallback(&["mcp.github.create_issue", "create_issue"])
            .ok_or_else(|| anyhow::anyhow!("missing resolved tool for github.create_issue"))
    })?;
    let comment_on_issue = tool_name("github.comment_on_issue").or_else(|_| {
        direct_tool_name_fallback(&[
            "mcp.github.create_issue_comment",
            "create_issue_comment",
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
        lines.push(detail.to_string());
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
    lines.push(String::new());
    lines.push(fingerprint_marker(&draft.fingerprint));
    lines.push(evidence_marker(evidence_digest));
    lines.join("\n")
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
    extract_issues_from_tool_result(&result)
        .into_iter()
        .next()
        .ok_or_else(|| anyhow::anyhow!("GitHub issue creation returned no issue payload"))
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
