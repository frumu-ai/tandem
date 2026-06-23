use anyhow::Context;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tandem_types::{EngineEvent, ToolResult};

use crate::comment_summary::build_comment_recurrence_summary;
use crate::error_provenance::{locate_error_provenance, render_provenance_section};
use crate::types::{
    BugMonitorConfig, BugMonitorDestinationKind, BugMonitorDraftRecord, BugMonitorIncidentRecord,
    BugMonitorPostRecord, BugMonitorStatus, BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID,
};
use crate::{now_ms, sha256_hex, truncate_text};
use std::fs;

use tandem_core::ToolEffectLedgerRecord;

const BUG_MONITOR_LABEL: &str = "bug-monitor";
const ISSUE_BODY_MARKER_SAFE_SPACE: usize = 2;
const ISSUE_BODY_BYTE_BUDGET: usize = 12_000;
const ISSUE_BODY_LOG_CHAR_BUDGET: usize = 4_000;
const ISSUE_BODY_LOG_LINES: usize = 30;
const ISSUE_BODY_LOG_FALLBACK_LINES: usize = 12;
const ISSUE_BODY_EVIDENCE_REF_LIMIT: usize = 15;
const ISSUE_BODY_QUALITY_GATE_MISSING_LIMIT: usize = 20;
const ISSUE_BODY_TRIAGE_TIMEOUT_DETAIL_LINES: usize = 20;
const ISSUE_BODY_TOOL_EVIDENCE_LIMIT: usize = 12;
const ISSUE_BODY_TOOL_ERROR_CHAR_BUDGET: usize = 200;
const ISSUE_BODY_TOOL_RESULT_CHAR_BUDGET: usize = 220;
const ISSUE_BODY_TOOL_RECORD_BUDGET: usize = 640;

// Evidence policy:
// Keep GitHub issue bodies bounded and human-readable by preserving the
// strongest snippets inline and pushing deep context into artifacts/events.

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

#[async_trait::async_trait]
pub trait BugMonitorGithubHost: Sync {
    async fn bug_monitor_status_snapshot(&self) -> BugMonitorStatus;
    async fn get_bug_monitor_draft(&self, draft_id: &str) -> Option<BugMonitorDraftRecord>;
    async fn put_bug_monitor_draft(
        &self,
        draft: BugMonitorDraftRecord,
    ) -> anyhow::Result<BugMonitorDraftRecord>;
    async fn get_bug_monitor_incident(&self, incident_id: &str)
        -> Option<BugMonitorIncidentRecord>;
    async fn put_bug_monitor_post(
        &self,
        post: BugMonitorPostRecord,
    ) -> anyhow::Result<BugMonitorPostRecord>;
    async fn list_bug_monitor_posts(&self, limit: usize) -> Vec<BugMonitorPostRecord>;
    async fn list_bug_monitor_posts_by_draft(&self, draft_id: &str) -> Vec<BugMonitorPostRecord>;
    async fn list_bug_monitor_posts_by_fingerprint(
        &self,
        repo: &str,
        fingerprint: &str,
    ) -> Vec<BugMonitorPostRecord>;
    async fn list_bug_monitor_posts_by_idempotency_key(
        &self,
        idempotency_key: &str,
    ) -> Vec<BugMonitorPostRecord>;
    async fn try_claim_bug_monitor_post_idempotency(
        &self,
        post: BugMonitorPostRecord,
    ) -> anyhow::Result<(bool, BugMonitorPostRecord)>;
    async fn mirror_bug_monitor_post_as_external_action(
        &self,
        draft: &BugMonitorDraftRecord,
        post: &BugMonitorPostRecord,
    );
    async fn update_last_post_result(&self, result: String);
    fn publish_event(&self, event: EngineEvent);
    async fn ensure_bug_monitor_issue_draft(
        &self,
        draft_id: &str,
        force: bool,
    ) -> anyhow::Result<Value>;
    async fn load_bug_monitor_issue_draft_artifact(&self, triage_run_id: &str) -> Option<Value>;
    async fn resolve_github_tool_set(
        &self,
        config: &BugMonitorConfig,
    ) -> anyhow::Result<GithubToolSet>;
    async fn call_mcp_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        payload: Value,
    ) -> anyhow::Result<ToolResult>;
    fn context_run_events_path(&self, run_id: &str) -> PathBuf;
}

pub async fn record_post_failure(
    state: &dyn BugMonitorGithubHost,
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
        destination_id: Some(BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()),
        destination_kind: Some(BugMonitorDestinationKind::GithubIssue),
        route_id: None,
        route_match_reason: Some("legacy_github".to_string()),
        external_id: draft.issue_number.map(|number| number.to_string()),
        external_url: draft
            .github_comment_url
            .clone()
            .or_else(|| draft.github_issue_url.clone()),
        external_title: draft
            .issue_number
            .map(|number| format!("GitHub issue #{number}")),
        target_ref: Some(draft.repo.clone()),
        receipt: Some(json!({
            "provider": "github",
            "operation": operation,
            "status": "failed",
        })),
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
    state: &dyn BugMonitorGithubHost,
    draft: &BugMonitorDraftRecord,
    post: &BugMonitorPostRecord,
) {
    state
        .mirror_bug_monitor_post_as_external_action(draft, post)
        .await;
}

#[derive(Debug, Clone, Default)]
pub struct GithubToolSet {
    pub server_name: String,
    pub list_issues: String,
    pub get_issue: String,
    pub create_issue: String,
    pub comment_on_issue: String,
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
    state: &dyn BugMonitorGithubHost,
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
    if !status.readiness.publish_ready && mode == PublishMode::Auto {
        anyhow::bail!("{}", bug_monitor_publish_not_ready_reason(&status));
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

    let tools = state
        .resolve_github_tool_set(&config)
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
            state
                .ensure_bug_monitor_issue_draft(&draft.draft_id, false)
                .await
                .context("generate Bug Monitor issue draft")?,
        )
    } else {
        state
            .load_bug_monitor_issue_draft_artifact(
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
        draft.github_status = Some("triage_enrichment_pending_fallback_publish".to_string());
        draft = state.put_bug_monitor_draft(draft).await?;
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
            let body =
                append_error_provenance_section(state, body, &draft, incident.as_ref()).await;
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
                destination_id: Some(BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()),
                destination_kind: Some(BugMonitorDestinationKind::GithubIssue),
                route_id: None,
                route_match_reason: Some("legacy_github".to_string()),
                external_id: result.id.clone(),
                external_url: result.html_url.clone().or_else(|| issue.html_url.clone()),
                external_title: Some(format!("GitHub issue #{} comment", issue.number)),
                target_ref: Some(draft.repo.clone()),
                receipt: Some(json!({
                    "provider": "github",
                    "operation": "comment_issue",
                    "issue_number": issue.number,
                    "comment_id": result.id,
                })),
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
                .update_last_post_result(format!("commented issue #{}", issue.number))
                .await;
            state.publish_event(EngineEvent::new(
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

fn bug_monitor_publish_not_ready_reason(status: &BugMonitorStatus) -> String {
    if let Some(error) = status.last_error.as_ref() {
        let model_only_not_ready = !status.readiness.selected_model_ready
            && status.readiness.repo_valid
            && status.readiness.mcp_connected
            && status.readiness.github_read_ready
            && status.readiness.github_write_ready;
        if !model_only_not_ready {
            return error.clone();
        }
    }
    "Bug Monitor is not ready for GitHub posting".to_string()
}

async fn create_issue_from_draft(
    state: &dyn BugMonitorGithubHost,
    tools: &GithubToolSet,
    config: &BugMonitorConfig,
    mut draft: BugMonitorDraftRecord,
    incident: Option<&BugMonitorIncidentRecord>,
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
    if let Some(previous) = latest_failed_create_post_for_draft(state, &draft).await {
        let detail = format!(
            "suppressed automatic GitHub issue creation for fingerprint {} after previous {} post attempt {} failed; refusing to retry create_issue because the previous attempt may have created an issue without returning a parseable payload",
            draft.fingerprint, previous.operation, previous.post_id
        );
        draft.status = "github_post_failed".to_string();
        draft.github_status = Some("github_post_failed".to_string());
        draft.last_post_error = Some(truncate_text(&detail, 500));
        let draft = state.put_bug_monitor_draft(draft).await?;
        return Ok(PublishOutcome {
            action: "create_issue_retry_suppressed".to_string(),
            draft,
            post: Some(previous),
        });
    }
    let claim = BugMonitorPostRecord {
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
        destination_id: Some(BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()),
        destination_kind: Some(BugMonitorDestinationKind::GithubIssue),
        route_id: None,
        route_match_reason: Some("legacy_github".to_string()),
        external_id: None,
        external_url: None,
        external_title: None,
        target_ref: Some(draft.repo.clone()),
        receipt: Some(json!({
            "provider": "github",
            "operation": "create_issue",
            "status": "pending",
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
    let (claimed, existing_claim) = state.try_claim_bug_monitor_post_idempotency(claim).await?;
    if !claimed {
        if existing_claim.status == "posted" {
            draft.status = "github_issue_created".to_string();
            draft.github_status = Some("github_issue_created".to_string());
            draft.issue_number = existing_claim.issue_number;
            draft.github_issue_url = existing_claim.issue_url.clone();
            draft.github_posted_at_ms = Some(existing_claim.updated_at_ms);
            draft.last_post_error = None;
            mirror_bug_monitor_post_as_external_action(state, &draft, &existing_claim).await;
            let draft = state.put_bug_monitor_draft(draft).await?;
            return Ok(PublishOutcome {
                action: "skip_duplicate".to_string(),
                draft,
                post: Some(existing_claim),
            });
        }
        draft.github_status = Some("github_posting".to_string());
        draft.last_post_error = Some(
            "another Bug Monitor publisher already claimed this GitHub create_issue idempotency key"
                .to_string(),
        );
        // Do not persist this stale draft snapshot: the winning publisher
        // may already be writing the issue number back.
        return Ok(PublishOutcome {
            action: "publish_in_progress".to_string(),
            draft,
            post: Some(existing_claim),
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
    let body = append_error_provenance_section(state, body, &draft, incident).await;
    let created = match call_create_issue(state, tools, &owner_repo, title, &body).await {
        Ok(created) => created,
        Err(error) => {
            let mut failed_claim = existing_claim.clone();
            failed_claim.status = "failed".to_string();
            failed_claim.error = Some(truncate_text(&error.to_string(), 500));
            failed_claim.updated_at_ms = now_ms();
            if let Err(record_err) = state.put_bug_monitor_post(failed_claim).await {
                tracing::warn!(
                    draft_id = %draft.draft_id,
                    error = %record_err,
                    "failed to record ambiguous Bug Monitor create_issue failure",
                );
            }
            return Err(error).context("create Bug Monitor issue on GitHub");
        }
    };
    let post = BugMonitorPostRecord {
        status: "posted".to_string(),
        issue_number: Some(created.number),
        issue_url: created.html_url.clone(),
        external_id: Some(created.number.to_string()),
        external_url: created.html_url.clone(),
        external_title: Some(format!("GitHub issue #{}", created.number)),
        receipt: Some(json!({
            "provider": "github",
            "operation": "create_issue",
            "issue_number": created.number,
            "issue_url": created.html_url.clone(),
        })),
        response_excerpt: Some(truncate_text(&body, 400)),
        error: None,
        updated_at_ms: now_ms(),
        ..existing_claim
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
        .update_last_post_result(format!("created issue #{}", created.number))
        .await;
    state.publish_event(EngineEvent::new(
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

async fn find_matching_issue(
    state: &dyn BugMonitorGithubHost,
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
    issues.sort_by_key(|issue| std::cmp::Reverse(issue.number));
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
    state: &dyn BugMonitorGithubHost,
    idempotency_key: &str,
) -> Option<BugMonitorPostRecord> {
    let mut rows = state
        .list_bug_monitor_posts_by_idempotency_key(idempotency_key)
        .await
        .into_iter()
        .filter(|row| row.status == "posted")
        .collect::<Vec<_>>();
    rows.sort_by_key(|post| std::cmp::Reverse(post.updated_at_ms));
    rows.into_iter().next()
}

async fn successful_post_for_draft(
    state: &dyn BugMonitorGithubHost,
    draft_id: &str,
    evidence_digest: Option<&str>,
) -> Option<BugMonitorPostRecord> {
    let mut rows = state.list_bug_monitor_posts_by_draft(draft_id).await;
    rows.sort_by_key(|post| std::cmp::Reverse(post.updated_at_ms));
    rows.into_iter().find(|row| {
        row.status == "posted"
            && match evidence_digest {
                Some(expected) => row.evidence_digest.as_deref() == Some(expected),
                None => true,
            }
    })
}

fn failed_post_suppresses_create(
    draft: &BugMonitorDraftRecord,
    post: &BugMonitorPostRecord,
) -> bool {
    post.repo == draft.repo
        && post.fingerprint == draft.fingerprint
        && post.status == "failed"
        && (post.operation == "create_issue"
            || (post.operation == "auto_post" && !post_failure_is_preflight_only(post)))
}

fn post_failure_is_preflight_only(post: &BugMonitorPostRecord) -> bool {
    let error = post
        .error
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase();
    error.contains("not ready")
        || error.contains("disabled")
        || error.contains("paused")
        || error.contains("provider/model")
        || error.contains("selected mcp server")
        || error.contains("target repo")
}

async fn latest_failed_create_post_for_draft(
    state: &dyn BugMonitorGithubHost,
    draft: &BugMonitorDraftRecord,
) -> Option<BugMonitorPostRecord> {
    let mut rows = state
        .list_bug_monitor_posts_by_fingerprint(&draft.repo, &draft.fingerprint)
        .await
        .into_iter()
        .filter(|post| failed_post_suppresses_create(draft, post))
        .collect::<Vec<_>>();
    rows.sort_by_key(|post| std::cmp::Reverse(post.updated_at_ms));
    rows.into_iter().next()
}

/// Hashes the IDENTITY of the failure being reported — not the
/// execution metadata of how we triaged it on a particular pass.
/// Excluded: `triage_run_id` (recreated for stale/blocked triages,
/// drove the #69-#194 spam), `incident.run_id` / `session_id`
/// (redundant with fingerprint), `occurrence_count` (removed in #48).
fn compute_evidence_digest(
    draft: &BugMonitorDraftRecord,
    incident: Option<&BugMonitorIncidentRecord>,
) -> String {
    let _ = incident;
    sha256_hex(&[
        draft.repo.as_str(),
        draft.fingerprint.as_str(),
        draft.title.as_deref().unwrap_or(""),
        draft.detail.as_deref().unwrap_or(""),
    ])
}

fn build_idempotency_key(repo: &str, fingerprint: &str, operation: &str, digest: &str) -> String {
    sha256_hex(&[
        BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID,
        "github_issue",
        repo,
        fingerprint,
        operation,
        digest,
    ])
}

fn build_issue_body(
    draft: &BugMonitorDraftRecord,
    incident: Option<&BugMonitorIncidentRecord>,
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
                for missing in gate
                    .missing
                    .iter()
                    .take(ISSUE_BODY_QUALITY_GATE_MISSING_LIMIT)
                {
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
                for line in diagnostics
                    .lines()
                    .take(ISSUE_BODY_TRIAGE_TIMEOUT_DETAIL_LINES)
                {
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
    let body_budget = ISSUE_BODY_BYTE_BUDGET
        .saturating_sub(marker_text.len())
        .saturating_sub(ISSUE_BODY_MARKER_SAFE_SPACE);
    let body = truncate_text(&lines.join("\n"), body_budget);
    format!("{body}\n{marker_text}")
}

fn fallback_issue_logs(
    draft: &BugMonitorDraftRecord,
    incident: Option<&BugMonitorIncidentRecord>,
) -> Option<String> {
    let rows = incident
        .map(|row| {
            row.excerpt
                .iter()
                .filter_map(normalize_issue_body_line)
                .take(ISSUE_BODY_LOG_LINES)
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
                .take(ISSUE_BODY_LOG_FALLBACK_LINES)
                .collect::<Vec<_>>()
        });
    if rows.is_empty() {
        None
    } else {
        Some(truncate_text(&rows.join("\n"), ISSUE_BODY_LOG_CHAR_BUDGET))
    }
}

fn fallback_issue_evidence_refs(
    draft: &BugMonitorDraftRecord,
    incident: Option<&BugMonitorIncidentRecord>,
) -> Vec<String> {
    // Evidence references are capped so issue bodies stay skimmable.
    // Full evidence graphs stay in artifacts/run logs and can be fetched by ID.
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
    refs.into_iter()
        .take(ISSUE_BODY_EVIDENCE_REF_LIMIT)
        .collect()
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
        Some(
            "triage_timed_out"
            | "triage_failed_fallback_publish"
            | "triage_pending"
            | "triage_enrichment_pending_fallback_publish"
            | "github_post_failed",
        ) => status,
        _ => None,
    }
}

fn build_comment_body(
    draft: &BugMonitorDraftRecord,
    incident: Option<&BugMonitorIncidentRecord>,
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
    } else {
        // No LLM-produced narrative for this occurrence (triage timed
        // out, hasn't run yet, or didn't produce a `what_happened`).
        // Don't dump the verbose event payload from `draft.detail` —
        // it just repeats the original issue body and adds noise.
        // Emit a focused recurrence summary instead.
        lines.push(String::new());
        lines.push(build_comment_recurrence_summary(draft, incident));
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
    state: &dyn BugMonitorGithubHost,
    tools: &GithubToolSet,
    (owner, repo): &(&str, &str),
) -> anyhow::Result<Vec<GithubIssue>> {
    let result = state
        .call_mcp_tool(
            &tools.server_name,
            &tools.list_issues,
            github_list_issues_payload(owner, repo),
        )
        .await?;
    Ok(extract_issues_from_tool_result(&result))
}

fn github_list_issues_payload(owner: &str, repo: &str) -> Value {
    json!({
        "owner": owner,
        "repo": repo,
        "perPage": 100
    })
}

async fn call_get_issue(
    state: &dyn BugMonitorGithubHost,
    tools: &GithubToolSet,
    (owner, repo): &(&str, &str),
    issue_number: u64,
) -> anyhow::Result<GithubIssue> {
    let result = state
        .call_mcp_tool(
            &tools.server_name,
            &tools.get_issue,
            github_get_issue_payload(owner, repo, issue_number),
        )
        .await?;
    extract_issues_from_tool_result(&result)
        .into_iter()
        .find(|issue| issue.number == issue_number)
        .ok_or_else(|| anyhow::anyhow!("GitHub issue #{issue_number} was not returned"))
}

fn github_get_issue_payload(owner: &str, repo: &str, issue_number: u64) -> Value {
    json!({
        "method": "get",
        "owner": owner,
        "repo": repo,
        "issue_number": issue_number
    })
}

async fn call_create_issue(
    state: &dyn BugMonitorGithubHost,
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
        .call_mcp_tool(&tools.server_name, &tools.create_issue, preferred)
        .await;
    let result = match first {
        Ok(result) => result,
        Err(_) => {
            state
                .call_mcp_tool(&tools.server_name, &tools.create_issue, fallback)
                .await?
        }
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
    state: &dyn BugMonitorGithubHost,
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
    state: &dyn BugMonitorGithubHost,
    tools: &GithubToolSet,
    (owner, repo): &(&str, &str),
    issue_number: u64,
    body: &str,
) -> anyhow::Result<GithubComment> {
    let result = state
        .call_mcp_tool(
            &tools.server_name,
            &tools.comment_on_issue,
            json!({
                "owner": owner,
                "repo": repo,
                "issue_number": issue_number,
                "body": body
            }),
        )
        .await?;
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
///
/// Each silent-skip path emits a `tracing::info!` so operators can tell
/// from logs *why* an issue body shipped without an Error provenance
/// section — currently a recurring symptom and the only signal that
/// distinguishes "no error message picked", "workspace path not
/// accessible to this process", and "grep returned zero hits".
async fn append_error_provenance_section(
    state: &dyn BugMonitorGithubHost,
    body: String,
    draft: &BugMonitorDraftRecord,
    incident: Option<&BugMonitorIncidentRecord>,
) -> String {
    let incident_id = incident.map(|row| row.incident_id.as_str()).unwrap_or("");
    let draft_id = draft.draft_id.as_str();
    let mut combined = body;
    let section = fallback_tool_evidence_section(state, incident, draft.triage_run_id.as_deref());
    if !section.trim().is_empty() {
        if !combined.ends_with('\n') {
            combined.push('\n');
        }
        combined.push('\n');
        combined.push_str(&section);
    }
    let Some(error_message) = pick_error_message_for_provenance(draft, incident) else {
        tracing::info!(
            incident_id = %incident_id,
            draft_id = %draft_id,
            reason = "no_error_message",
            "skipping error provenance: no usable error message on draft/incident",
        );
        return combined;
    };
    let raw_workspace_root = incident
        .map(|row| row.workspace_root.as_str())
        .unwrap_or("");
    let workspace_root = pick_workspace_root_for_provenance(incident);
    let Some(workspace_root) = workspace_root else {
        tracing::info!(
            incident_id = %incident_id,
            draft_id = %draft_id,
            reason = "workspace_root_inaccessible",
            workspace_root = %raw_workspace_root,
            "skipping error provenance: workspace_root missing, not absolute, or not present on this process's filesystem",
        );
        return combined;
    };
    let hits = locate_error_provenance(&workspace_root, &error_message).await;
    let Some(section) = render_provenance_section(&hits) else {
        let preview = error_message.chars().take(160).collect::<String>();
        tracing::info!(
            incident_id = %incident_id,
            draft_id = %draft_id,
            reason = "no_grep_hits",
            workspace_root = %workspace_root.display(),
            error_message_preview = %preview,
            hit_count = hits.len(),
            "skipping error provenance: git grep returned no usable hits in the workspace for the error message",
        );
        return combined;
    };
    tracing::info!(
        incident_id = %incident_id,
        draft_id = %draft_id,
        workspace_root = %workspace_root.display(),
        hit_count = hits.len(),
        "appended error provenance section to issue body",
    );
    if !combined.ends_with('\n') {
        combined.push('\n');
    }
    combined.push('\n');
    combined.push_str(&section);
    combined
}

fn fallback_tool_evidence_section(
    state: &dyn BugMonitorGithubHost,
    incident: Option<&BugMonitorIncidentRecord>,
    draft_run_id: Option<&str>,
) -> String {
    // Show only the most useful recent tool calls.
    // This preserves debuggability without flooding the issue with full event history.
    let run_id = incident
        .and_then(|row| row.run_id.as_deref())
        .or(draft_run_id)
        .filter(|value| !value.trim().is_empty());
    let Some(run_id) = run_id else {
        return String::new();
    };

    let events = fs::read_to_string(state.context_run_events_path(run_id))
        .ok()
        .map(|content| {
            content
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .filter_map(|line| serde_json::from_str::<Value>(line).ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let rows = events
        .into_iter()
        .filter_map(|row| {
            let event_type = row.get("type").and_then(Value::as_str)?;
            if event_type != "tool_effect_recorded" {
                return None;
            }
            row.get("payload")?
                .get("record")
                .and_then(|row| serde_json::from_value::<ToolEffectLedgerRecord>(row.clone()).ok())
        })
        .filter_map(format_tool_effect_record)
        .take(ISSUE_BODY_TOOL_EVIDENCE_LIMIT)
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return String::new();
    }
    let mut lines = vec!["### Tool evidence".to_string()];
    lines.extend(rows);
    lines.join("\n")
}

fn format_tool_effect_record(record: ToolEffectLedgerRecord) -> Option<String> {
    let status = serde_json::to_string(&record.status)
        .map(|value| value.trim_matches('"').to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    let phase = serde_json::to_string(&record.phase)
        .map(|value| value.trim_matches('"').to_string())
        .unwrap_or_else(|_| "unknown".to_string());

    let mut details = Vec::new();
    if let Some(path) = record.args_summary.get("path").and_then(Value::as_str) {
        details.push(format!("path={path}"));
    }
    if let Some(url) = record.args_summary.get("url").and_then(Value::as_str) {
        details.push(format!("url={url}"));
    }
    if let Some(command_hash) = record
        .args_summary
        .get("command_hash")
        .and_then(Value::as_str)
    {
        details.push(format!("command_hash={command_hash}"));
    }
    if let Some(query_hash) = record
        .args_summary
        .get("query_hash")
        .and_then(Value::as_str)
    {
        details.push(format!("query_hash={query_hash}"));
    }

    let mut result = Vec::new();
    if let Some(error) = record.error.as_ref() {
        let error = truncate_text(error, ISSUE_BODY_TOOL_ERROR_CHAR_BUDGET);
        if !error.is_empty() {
            result.push(format!("error={error}"));
        }
    }
    if let Some(value) = record
        .result_summary
        .as_ref()
        .and_then(|value| serde_json::to_string(value).ok())
    {
        let value = truncate_text(&value, ISSUE_BODY_TOOL_RESULT_CHAR_BUDGET);
        if !value.is_empty() {
            result.push(format!("result={value}"));
        }
    }

    details.extend(result);
    let details = if details.is_empty() {
        String::new()
    } else {
        format!(" ({})", details.join(", "))
    };
    Some(truncate_text(
        &format!("- {} {} / {}{}", record.tool, phase, status, details),
        ISSUE_BODY_TOOL_RECORD_BUDGET,
    ))
}

fn pick_error_message_for_provenance(
    draft: &BugMonitorDraftRecord,
    incident: Option<&BugMonitorIncidentRecord>,
) -> Option<String> {
    // Prefer fields written at incident/draft creation time. Avoid
    // `last_error` and `last_post_error` because the triage deadline
    // task rewrites those with the multi-line timeout diagnostics
    // ("triage run X did not reach a terminal status within …\n
    // timeout_ms: …"). Grepping the codebase for that diagnostic
    // text always returns no hits, so the Error provenance section
    // would silently disappear on every triage timeout — exactly
    // the issues we most need provenance for.
    let candidates = [
        incident.and_then(|row| {
            row.excerpt
                .iter()
                .find(|line| !line.trim().is_empty())
                .cloned()
        }),
        draft.detail.clone(),
        incident.and_then(|row| row.detail.clone()),
        incident.and_then(|row| extract_error_after_colon(&row.title)),
        incident.map(|row| row.title.clone()),
        draft.title.clone(),
    ];
    candidates
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

/// Pull the trailing-colon portion out of a bug-monitor incident
/// title so that "Workflow X failed at Y: real error here" yields
/// "real error here". Uses leftmost split so titles whose error
/// itself contains colons survive intact. The full title still
/// serves as a fallback when the suffix is too short to be useful.
fn extract_error_after_colon(title: &str) -> Option<String> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return None;
    }
    let suffix = trimmed.split_once(':').map(|(_, suffix)| suffix.trim());
    suffix
        .filter(|s| !s.is_empty() && s.split_whitespace().count() >= 3)
        .map(str::to_string)
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
mod tests;
