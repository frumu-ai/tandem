use super::context_runs::{
    claim_next_context_task, context_run_create, context_run_engine, context_run_task_transition,
    context_run_tasks_create, ensure_context_run_dir, load_context_blackboard,
    load_context_run_state, save_context_run_state,
};
use super::context_types::{
    ContextBlackboardArtifact, ContextBlackboardPatchOp, ContextBlackboardTaskStatus,
    ContextRunCreateInput, ContextRunEventAppendInput, ContextRunState, ContextRunStatus,
    ContextTaskCreateBatchInput, ContextTaskCreateInput, ContextTaskTransitionInput,
    ContextWorkspaceLease,
};
use super::*;
use axum::extract::Path;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet, VecDeque};
use std::path::PathBuf;
use tandem_memory::{
    types::MemoryTier, GovernedMemoryTier, MemoryClassification, MemoryContentKind, MemoryManager,
    MemoryPartition, MemoryPromoteRequest, MemoryPutRequest, PromotionReview,
};
use tandem_runtime::McpRemoteTool;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum CoderWorkflowMode {
    IssueTriage,
    IssueFix,
    PrReview,
    MergeRecommendation,
}

impl CoderWorkflowMode {
    fn as_context_run_type(&self) -> &'static str {
        match self {
            Self::IssueTriage => "coder_issue_triage",
            Self::IssueFix => "coder_issue_fix",
            Self::PrReview => "coder_pr_review",
            Self::MergeRecommendation => "coder_merge_recommendation",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum CoderGithubRefKind {
    Issue,
    PullRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CoderGithubRef {
    pub(super) kind: CoderGithubRefKind,
    pub(super) number: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CoderRepoBinding {
    pub(super) project_id: String,
    pub(super) workspace_id: String,
    pub(super) workspace_root: String,
    pub(super) repo_slug: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) default_branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CoderRunRecord {
    pub(super) coder_run_id: String,
    pub(super) workflow_mode: CoderWorkflowMode,
    pub(super) linked_context_run_id: String,
    pub(super) repo_binding: CoderRepoBinding,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) github_ref: Option<CoderGithubRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) source_client: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) model_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) parent_coder_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) origin_artifact_type: Option<String>,
    pub(super) created_at_ms: u64,
    pub(super) updated_at_ms: u64,
}

#[derive(Debug, Deserialize)]
pub(super) struct CoderRunCreateInput {
    #[serde(default)]
    pub(super) coder_run_id: Option<String>,
    pub(super) workflow_mode: CoderWorkflowMode,
    pub(super) repo_binding: CoderRepoBinding,
    #[serde(default)]
    pub(super) github_ref: Option<CoderGithubRef>,
    #[serde(default)]
    pub(super) objective: Option<String>,
    #[serde(default)]
    pub(super) source_client: Option<String>,
    #[serde(default)]
    pub(super) workspace: Option<ContextWorkspaceLease>,
    #[serde(default)]
    pub(super) model_provider: Option<String>,
    #[serde(default)]
    pub(super) model_id: Option<String>,
    #[serde(default)]
    pub(super) mcp_servers: Option<Vec<String>>,
    #[serde(default)]
    pub(super) parent_coder_run_id: Option<String>,
    #[serde(default)]
    pub(super) origin: Option<String>,
    #[serde(default)]
    pub(super) origin_artifact_type: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderRunListQuery {
    #[serde(default)]
    pub(super) workflow_mode: Option<CoderWorkflowMode>,
    #[serde(default)]
    pub(super) repo_slug: Option<String>,
    #[serde(default)]
    pub(super) limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum CoderMemoryCandidateKind {
    TriageMemory,
    FixPattern,
    ValidationMemory,
    ReviewMemory,
    MergeRecommendationMemory,
    RegressionSignal,
    FailurePattern,
    RunOutcome,
}

#[derive(Debug, Deserialize)]
pub(super) struct CoderMemoryCandidateCreateInput {
    pub(super) kind: CoderMemoryCandidateKind,
    #[serde(default)]
    pub(super) task_id: Option<String>,
    #[serde(default)]
    pub(super) summary: Option<String>,
    #[serde(default)]
    pub(super) payload: Value,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderMemoryCandidatePromoteInput {
    #[serde(default)]
    pub(super) to_tier: Option<GovernedMemoryTier>,
    #[serde(default)]
    pub(super) reviewer_id: Option<String>,
    #[serde(default)]
    pub(super) approval_id: Option<String>,
    #[serde(default)]
    pub(super) reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderTriageSummaryCreateInput {
    #[serde(default)]
    pub(super) summary: Option<String>,
    #[serde(default)]
    pub(super) confidence: Option<String>,
    #[serde(default)]
    pub(super) affected_files: Vec<String>,
    #[serde(default)]
    pub(super) duplicate_candidates: Vec<Value>,
    #[serde(default)]
    pub(super) memory_hits_used: Vec<String>,
    #[serde(default)]
    pub(super) reproduction: Option<Value>,
    #[serde(default)]
    pub(super) notes: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderTriageReproductionReportCreateInput {
    #[serde(default)]
    pub(super) summary: Option<String>,
    #[serde(default)]
    pub(super) outcome: Option<String>,
    #[serde(default)]
    pub(super) steps: Vec<String>,
    #[serde(default)]
    pub(super) observed_logs: Vec<String>,
    #[serde(default)]
    pub(super) affected_files: Vec<String>,
    #[serde(default)]
    pub(super) memory_hits_used: Vec<String>,
    #[serde(default)]
    pub(super) notes: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderTriageInspectionReportCreateInput {
    #[serde(default)]
    pub(super) summary: Option<String>,
    #[serde(default)]
    pub(super) likely_areas: Vec<String>,
    #[serde(default)]
    pub(super) affected_files: Vec<String>,
    #[serde(default)]
    pub(super) memory_hits_used: Vec<String>,
    #[serde(default)]
    pub(super) notes: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderPrReviewSummaryCreateInput {
    #[serde(default)]
    pub(super) verdict: Option<String>,
    #[serde(default)]
    pub(super) summary: Option<String>,
    #[serde(default)]
    pub(super) risk_level: Option<String>,
    #[serde(default)]
    pub(super) changed_files: Vec<String>,
    #[serde(default)]
    pub(super) blockers: Vec<String>,
    #[serde(default)]
    pub(super) requested_changes: Vec<String>,
    #[serde(default)]
    pub(super) regression_signals: Vec<Value>,
    #[serde(default)]
    pub(super) memory_hits_used: Vec<String>,
    #[serde(default)]
    pub(super) notes: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderPrReviewEvidenceCreateInput {
    #[serde(default)]
    pub(super) verdict: Option<String>,
    #[serde(default)]
    pub(super) summary: Option<String>,
    #[serde(default)]
    pub(super) risk_level: Option<String>,
    #[serde(default)]
    pub(super) changed_files: Vec<String>,
    #[serde(default)]
    pub(super) blockers: Vec<String>,
    #[serde(default)]
    pub(super) requested_changes: Vec<String>,
    #[serde(default)]
    pub(super) regression_signals: Vec<Value>,
    #[serde(default)]
    pub(super) memory_hits_used: Vec<String>,
    #[serde(default)]
    pub(super) notes: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderIssueFixSummaryCreateInput {
    #[serde(default)]
    pub(super) summary: Option<String>,
    #[serde(default)]
    pub(super) root_cause: Option<String>,
    #[serde(default)]
    pub(super) fix_strategy: Option<String>,
    #[serde(default)]
    pub(super) changed_files: Vec<String>,
    #[serde(default)]
    pub(super) validation_steps: Vec<String>,
    #[serde(default)]
    pub(super) validation_results: Vec<Value>,
    #[serde(default)]
    pub(super) memory_hits_used: Vec<String>,
    #[serde(default)]
    pub(super) notes: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderIssueFixValidationReportCreateInput {
    #[serde(default)]
    pub(super) summary: Option<String>,
    #[serde(default)]
    pub(super) root_cause: Option<String>,
    #[serde(default)]
    pub(super) fix_strategy: Option<String>,
    #[serde(default)]
    pub(super) changed_files: Vec<String>,
    #[serde(default)]
    pub(super) validation_steps: Vec<String>,
    #[serde(default)]
    pub(super) validation_results: Vec<Value>,
    #[serde(default)]
    pub(super) memory_hits_used: Vec<String>,
    #[serde(default)]
    pub(super) notes: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderIssueFixPrDraftCreateInput {
    #[serde(default)]
    pub(super) title: Option<String>,
    #[serde(default)]
    pub(super) body: Option<String>,
    #[serde(default)]
    pub(super) base_branch: Option<String>,
    #[serde(default)]
    pub(super) head_branch: Option<String>,
    #[serde(default)]
    pub(super) changed_files: Vec<String>,
    #[serde(default)]
    pub(super) memory_hits_used: Vec<String>,
    #[serde(default)]
    pub(super) notes: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderIssueFixPrSubmitInput {
    #[serde(default)]
    pub(super) approved_by: Option<String>,
    #[serde(default)]
    pub(super) reason: Option<String>,
    #[serde(default)]
    pub(super) mcp_server: Option<String>,
    #[serde(default)]
    pub(super) dry_run: Option<bool>,
    #[serde(default)]
    pub(super) spawn_follow_on_runs: Vec<CoderWorkflowMode>,
    #[serde(default)]
    pub(super) allow_auto_merge_recommendation: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderMergeRecommendationSummaryCreateInput {
    #[serde(default)]
    pub(super) recommendation: Option<String>,
    #[serde(default)]
    pub(super) summary: Option<String>,
    #[serde(default)]
    pub(super) risk_level: Option<String>,
    #[serde(default)]
    pub(super) blockers: Vec<String>,
    #[serde(default)]
    pub(super) required_checks: Vec<String>,
    #[serde(default)]
    pub(super) required_approvals: Vec<String>,
    #[serde(default)]
    pub(super) memory_hits_used: Vec<String>,
    #[serde(default)]
    pub(super) notes: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderMergeReadinessReportCreateInput {
    #[serde(default)]
    pub(super) recommendation: Option<String>,
    #[serde(default)]
    pub(super) summary: Option<String>,
    #[serde(default)]
    pub(super) risk_level: Option<String>,
    #[serde(default)]
    pub(super) blockers: Vec<String>,
    #[serde(default)]
    pub(super) required_checks: Vec<String>,
    #[serde(default)]
    pub(super) required_approvals: Vec<String>,
    #[serde(default)]
    pub(super) memory_hits_used: Vec<String>,
    #[serde(default)]
    pub(super) notes: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderMemoryHitsQuery {
    #[serde(default)]
    pub(super) q: Option<String>,
    #[serde(default)]
    pub(super) limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderRunControlInput {
    #[serde(default)]
    pub(super) reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderRunExecuteNextInput {
    #[serde(default)]
    pub(super) agent_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderRunExecuteAllInput {
    #[serde(default)]
    pub(super) agent_id: Option<String>,
    #[serde(default)]
    pub(super) max_steps: Option<usize>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CoderFollowOnRunCreateInput {
    pub(super) workflow_mode: CoderWorkflowMode,
    #[serde(default)]
    pub(super) coder_run_id: Option<String>,
    #[serde(default)]
    pub(super) source_client: Option<String>,
    #[serde(default)]
    pub(super) model_provider: Option<String>,
    #[serde(default)]
    pub(super) model_id: Option<String>,
    #[serde(default)]
    pub(super) mcp_servers: Option<Vec<String>>,
}

fn coder_runs_root(state: &AppState) -> PathBuf {
    state
        .shared_resources_path
        .parent()
        .map(|parent| parent.join("coder_runs"))
        .unwrap_or_else(|| PathBuf::from(".tandem").join("coder_runs"))
}

fn coder_run_path(state: &AppState, coder_run_id: &str) -> PathBuf {
    coder_runs_root(state).join(format!("{coder_run_id}.json"))
}

fn coder_memory_candidates_dir(state: &AppState, linked_context_run_id: &str) -> PathBuf {
    super::context_runs::context_run_dir(state, linked_context_run_id).join("coder_memory")
}

fn coder_memory_candidate_path(
    state: &AppState,
    linked_context_run_id: &str,
    candidate_id: &str,
) -> PathBuf {
    coder_memory_candidates_dir(state, linked_context_run_id).join(format!("{candidate_id}.json"))
}

async fn ensure_coder_runs_dir(state: &AppState) -> Result<(), StatusCode> {
    tokio::fs::create_dir_all(coder_runs_root(state))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn save_coder_run_record(
    state: &AppState,
    record: &CoderRunRecord,
) -> Result<(), StatusCode> {
    ensure_coder_runs_dir(state).await?;
    let path = coder_run_path(state, &record.coder_run_id);
    let payload =
        serde_json::to_string_pretty(record).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::fs::write(path, payload)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn load_coder_run_record(
    state: &AppState,
    coder_run_id: &str,
) -> Result<CoderRunRecord, StatusCode> {
    let path = coder_run_path(state, coder_run_id);
    let raw = tokio::fs::read_to_string(path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    serde_json::from_str::<CoderRunRecord>(&raw).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn load_coder_memory_candidate_payload(
    state: &AppState,
    record: &CoderRunRecord,
    candidate_id: &str,
) -> Result<Value, StatusCode> {
    let raw = tokio::fs::read_to_string(coder_memory_candidate_path(
        state,
        &record.linked_context_run_id,
        candidate_id,
    ))
    .await
    .map_err(|_| StatusCode::NOT_FOUND)?;
    serde_json::from_str::<Value>(&raw).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn open_semantic_memory_manager() -> Option<MemoryManager> {
    let paths = tandem_core::resolve_shared_paths().ok()?;
    MemoryManager::new(&paths.memory_db_path).await.ok()
}

async fn list_repo_memory_candidates(
    state: &AppState,
    repo_slug: &str,
    github_ref: Option<&CoderGithubRef>,
    limit: usize,
) -> Result<Vec<Value>, StatusCode> {
    let mut hits = Vec::<Value>::new();
    let root = coder_runs_root(state);
    if !root.exists() {
        return Ok(hits);
    }
    let mut dir = tokio::fs::read_dir(root)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    while let Ok(Some(entry)) = dir.next_entry().await {
        if !entry
            .file_type()
            .await
            .map(|row| row.is_file())
            .unwrap_or(false)
        {
            continue;
        }
        let raw = tokio::fs::read_to_string(entry.path())
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let Ok(record) = serde_json::from_str::<CoderRunRecord>(&raw) else {
            continue;
        };
        if record.repo_binding.repo_slug != repo_slug {
            continue;
        }
        let candidates_dir = coder_memory_candidates_dir(state, &record.linked_context_run_id);
        if !candidates_dir.exists() {
            continue;
        }
        let mut candidate_dir = tokio::fs::read_dir(candidates_dir)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        while let Ok(Some(candidate_entry)) = candidate_dir.next_entry().await {
            if !candidate_entry
                .file_type()
                .await
                .map(|row| row.is_file())
                .unwrap_or(false)
            {
                continue;
            }
            let candidate_raw = tokio::fs::read_to_string(candidate_entry.path())
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let Ok(candidate_payload) = serde_json::from_str::<Value>(&candidate_raw) else {
                continue;
            };
            let same_ref = github_ref.is_some_and(|reference| {
                candidate_payload
                    .get("github_ref")
                    .and_then(|row| row.get("number"))
                    .and_then(Value::as_u64)
                    == Some(reference.number)
                    && candidate_payload
                        .get("github_ref")
                        .and_then(|row| row.get("kind"))
                        .and_then(Value::as_str)
                        == Some(match reference.kind {
                            CoderGithubRefKind::Issue => "issue",
                            CoderGithubRefKind::PullRequest => "pull_request",
                        })
            });
            let same_issue = same_ref
                && github_ref
                    .map(|reference| matches!(reference.kind, CoderGithubRefKind::Issue))
                    .unwrap_or(false);
            let candidate_kind = candidate_payload
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            hits.push(json!({
                "source": "coder_memory_candidate",
                "candidate_id": candidate_payload.get("candidate_id").cloned().unwrap_or(Value::Null),
                "kind": candidate_kind,
                "repo_slug": repo_slug,
                "same_ref": same_ref,
                "same_issue": same_issue,
                "summary": candidate_payload.get("summary").cloned().unwrap_or(Value::Null),
                "payload": candidate_payload.get("payload").cloned().unwrap_or(Value::Null),
                "path": candidate_entry.path(),
                "source_coder_run_id": candidate_payload.get("coder_run_id").cloned().unwrap_or(Value::Null),
                "created_at_ms": candidate_payload.get("created_at_ms").cloned().unwrap_or(Value::Null),
            }));
        }
    }
    hits.sort_by(|a, b| {
        let a_same_ref = a.get("same_ref").and_then(Value::as_bool).unwrap_or(false);
        let b_same_ref = b.get("same_ref").and_then(Value::as_bool).unwrap_or(false);
        let a_same_issue = a
            .get("same_issue")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let b_same_issue = b
            .get("same_issue")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        b_same_ref
            .cmp(&a_same_ref)
            .then_with(|| b_same_issue.cmp(&a_same_issue))
            .then_with(|| {
                b.get("created_at_ms")
                    .and_then(Value::as_u64)
                    .cmp(&a.get("created_at_ms").and_then(Value::as_u64))
            })
    });
    hits.truncate(limit.clamp(1, 20));
    Ok(hits)
}

async fn list_repo_memory_candidate_payloads(
    state: &AppState,
    repo_slug: &str,
    kind: Option<CoderMemoryCandidateKind>,
    limit: usize,
) -> Result<Vec<Value>, StatusCode> {
    let mut hits = Vec::<Value>::new();
    let root = coder_runs_root(state);
    if !root.exists() {
        return Ok(hits);
    }
    let mut dir = tokio::fs::read_dir(root)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    while let Ok(Some(entry)) = dir.next_entry().await {
        if !entry
            .file_type()
            .await
            .map(|row| row.is_file())
            .unwrap_or(false)
        {
            continue;
        }
        let raw = tokio::fs::read_to_string(entry.path())
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let Ok(record) = serde_json::from_str::<CoderRunRecord>(&raw) else {
            continue;
        };
        if record.repo_binding.repo_slug != repo_slug {
            continue;
        }
        let candidates_dir = coder_memory_candidates_dir(state, &record.linked_context_run_id);
        if !candidates_dir.exists() {
            continue;
        }
        let mut candidate_dir = tokio::fs::read_dir(candidates_dir)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        while let Ok(Some(candidate_entry)) = candidate_dir.next_entry().await {
            if !candidate_entry
                .file_type()
                .await
                .map(|row| row.is_file())
                .unwrap_or(false)
            {
                continue;
            }
            let candidate_raw = tokio::fs::read_to_string(candidate_entry.path())
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let Ok(candidate_payload) = serde_json::from_str::<Value>(&candidate_raw) else {
                continue;
            };
            let parsed_kind = candidate_payload
                .get("kind")
                .cloned()
                .and_then(|value| serde_json::from_value::<CoderMemoryCandidateKind>(value).ok());
            if kind.is_some() && parsed_kind.as_ref() != kind.as_ref() {
                continue;
            }
            hits.push(json!({
                "candidate": candidate_payload,
                "artifact_path": candidate_entry.path(),
                "source_coder_run_id": record.coder_run_id,
                "linked_context_run_id": record.linked_context_run_id,
            }));
        }
    }
    hits.sort_by(|a, b| {
        b.get("candidate")
            .and_then(|row| row.get("created_at_ms"))
            .and_then(Value::as_u64)
            .cmp(
                &a.get("candidate")
                    .and_then(|row| row.get("created_at_ms"))
                    .and_then(Value::as_u64),
            )
    });
    hits.truncate(limit.clamp(1, 50));
    Ok(hits)
}

fn normalize_failure_pattern_text(values: &[Option<&str>]) -> String {
    values
        .iter()
        .filter_map(|value| value.map(str::trim))
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn compare_failure_pattern_duplicate_matches(a: &Value, b: &Value) -> std::cmp::Ordering {
    let is_exact = |value: &Value| {
        value
            .get("match_reason")
            .and_then(Value::as_str)
            .map(|reason| reason == "exact_fingerprint")
            .unwrap_or_else(|| {
                value
                    .get("match_reasons")
                    .and_then(Value::as_array)
                    .map(|reasons| {
                        reasons
                            .iter()
                            .filter_map(Value::as_str)
                            .any(|reason| reason == "exact_fingerprint")
                    })
                    .unwrap_or(false)
            })
    };
    let a_exact = is_exact(a);
    let b_exact = is_exact(b);
    let a_score = a.get("score").and_then(Value::as_f64).unwrap_or(0.0);
    let b_score = b.get("score").and_then(Value::as_f64).unwrap_or(0.0);
    let a_recurrence = a
        .get("recurrence_count")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    let b_recurrence = b
        .get("recurrence_count")
        .and_then(Value::as_u64)
        .unwrap_or(1);
    b_exact.cmp(&a_exact).then_with(|| {
        b_recurrence.cmp(&a_recurrence).then_with(|| {
            b_score
                .partial_cmp(&a_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    })
}

pub(crate) async fn query_failure_pattern_matches(
    state: &AppState,
    repo_slug: &str,
    fingerprint: &str,
    title: Option<&str>,
    detail: Option<&str>,
    excerpt: &[String],
    limit: usize,
) -> Result<Vec<Value>, StatusCode> {
    let excerpt_text = (!excerpt.is_empty()).then(|| excerpt.join(" "));
    let haystack = normalize_failure_pattern_text(&[
        Some(fingerprint),
        title,
        detail,
        excerpt_text.as_deref(),
    ]);
    let candidates = list_repo_memory_candidate_payloads(
        state,
        repo_slug,
        Some(CoderMemoryCandidateKind::FailurePattern),
        limit.saturating_mul(4).max(8),
    )
    .await?;
    let mut matches = Vec::<Value>::new();
    let mut seen_match_ids = HashSet::<String>::new();
    for row in candidates {
        let candidate = row.get("candidate").cloned().unwrap_or(Value::Null);
        let payload = candidate.get("payload").cloned().unwrap_or(Value::Null);
        let candidate_fingerprint = payload
            .get("fingerprint")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let summary = candidate
            .get("summary")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let canonical_markers = payload
            .get("canonical_markers")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let symptoms = payload
            .get("symptoms")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut score = 0.0_f64;
        let mut reasons = Vec::<String>::new();
        if !candidate_fingerprint.is_empty() && candidate_fingerprint == fingerprint {
            score += 100.0;
            reasons.push("exact_fingerprint".to_string());
        }
        let marker_matches = canonical_markers
            .iter()
            .filter_map(Value::as_str)
            .filter(|marker| {
                let marker = marker.trim().to_ascii_lowercase();
                !marker.is_empty() && haystack.contains(&marker)
            })
            .count();
        if marker_matches > 0 {
            score += (marker_matches as f64) * 10.0;
            reasons.push(format!("marker_overlap:{marker_matches}"));
        }
        let symptom_matches = symptoms
            .iter()
            .filter_map(Value::as_str)
            .filter(|symptom| {
                let symptom = symptom.trim().to_ascii_lowercase();
                !symptom.is_empty() && haystack.contains(&symptom)
            })
            .count();
        if symptom_matches > 0 {
            score += (symptom_matches as f64) * 4.0;
            reasons.push(format!("symptom_overlap:{symptom_matches}"));
        }
        if !summary.is_empty() && haystack.contains(&summary.to_ascii_lowercase()) {
            score += 2.0;
            reasons.push("summary_overlap".to_string());
        }
        if score <= 0.0 {
            continue;
        }
        let identity = candidate
            .get("candidate_id")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .unwrap_or_else(|| candidate_fingerprint.to_string());
        if !seen_match_ids.insert(identity) {
            continue;
        }
        matches.push(json!({
            "candidate_id": candidate.get("candidate_id").cloned().unwrap_or(Value::Null),
            "summary": candidate.get("summary").cloned().unwrap_or(Value::Null),
            "fingerprint": payload.get("fingerprint").cloned().unwrap_or(Value::Null),
            "match_reason": if reasons.iter().any(|reason| reason == "exact_fingerprint") {
                Value::from("exact_fingerprint")
            } else {
                reasons
                    .first()
                    .cloned()
                    .map(Value::from)
                    .unwrap_or(Value::Null)
            },
            "linked_issue_numbers": payload.get("linked_issue_numbers").cloned().unwrap_or_else(|| json!([])),
            "recurrence_count": payload.get("recurrence_count").cloned().unwrap_or_else(|| Value::from(1_u64)),
            "linked_pr_numbers": payload.get("linked_pr_numbers").cloned().unwrap_or_else(|| json!([])),
            "artifact_refs": payload.get("artifact_refs").cloned().unwrap_or_else(|| json!([])),
            "source_coder_run_id": row.get("source_coder_run_id").cloned().unwrap_or(Value::Null),
            "linked_context_run_id": row.get("linked_context_run_id").cloned().unwrap_or(Value::Null),
            "artifact_path": row.get("artifact_path").cloned().unwrap_or(Value::Null),
            "score": score,
            "match_reasons": reasons,
        }));
    }
    let governed_matches = find_failure_pattern_duplicates(
        state,
        repo_slug,
        None,
        &[
            "bug_monitor".to_string(),
            "default".to_string(),
            "coder_api".to_string(),
            "desktop_developer_mode".to_string(),
        ],
        &haystack,
        Some(fingerprint),
        limit,
    )
    .await?;
    for governed in governed_matches {
        let identity = governed
            .get("candidate_id")
            .and_then(Value::as_str)
            .map(ToString::to_string)
            .or_else(|| {
                governed
                    .get("memory_id")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .or_else(|| {
                governed
                    .get("fingerprint")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
            })
            .unwrap_or_else(|| format!("governed-{}", matches.len()));
        if !seen_match_ids.insert(identity) {
            continue;
        }
        matches.push(governed);
    }
    matches.sort_by(compare_failure_pattern_duplicate_matches);
    matches.truncate(limit.clamp(1, 10));
    Ok(matches)
}

fn build_failure_pattern_payload(
    record: &CoderRunRecord,
    summary_artifact_path: &str,
    summary_text: &str,
    affected_files: &[String],
    duplicate_candidates: &[Value],
    notes: Option<&str>,
) -> Value {
    let fallback_component = record
        .repo_binding
        .repo_slug
        .rsplit('/')
        .next()
        .unwrap_or(record.repo_binding.repo_slug.as_str())
        .to_string();
    let mut canonical_markers = summary_text
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_' && ch != '-')
        .map(str::trim)
        .filter(|token| token.len() >= 5)
        .map(ToString::to_string)
        .take(5)
        .collect::<Vec<_>>();
    if let Some(note_text) = notes.map(str::trim).filter(|value| !value.is_empty()) {
        canonical_markers.push(note_text.to_string());
    }
    canonical_markers.sort();
    canonical_markers.dedup();
    let mut linked_issue_numbers = record
        .github_ref
        .as_ref()
        .filter(|reference| matches!(reference.kind, CoderGithubRefKind::Issue))
        .map(|reference| vec![reference.number])
        .unwrap_or_default();
    for number in duplicate_candidates
        .iter()
        .filter_map(|candidate| {
            candidate
                .get("linked_issue_numbers")
                .and_then(Value::as_array)
        })
        .flatten()
        .filter_map(Value::as_u64)
    {
        linked_issue_numbers.push(number);
    }
    linked_issue_numbers.sort_unstable();
    linked_issue_numbers.dedup();
    let affected_components = if affected_files.is_empty() {
        vec![fallback_component]
    } else {
        affected_files.to_vec()
    };
    let fingerprint = failure_pattern_fingerprint(
        &record.repo_binding.repo_slug,
        summary_text,
        affected_files,
        &canonical_markers,
    );
    json!({
        "type": "failure.pattern",
        "repo_slug": record.repo_binding.repo_slug,
        "fingerprint": fingerprint,
        "symptoms": [summary_text],
        "canonical_markers": canonical_markers,
        "linked_issue_numbers": linked_issue_numbers,
        "recurrence_count": 1,
        "linked_pr_numbers": duplicate_candidates
            .iter()
            .filter_map(|candidate| candidate.get("kind").and_then(Value::as_str).filter(|kind| *kind == "pull_request").and_then(|_| candidate.get("number")).and_then(Value::as_u64))
            .collect::<Vec<_>>(),
        "affected_components": affected_components,
        "artifact_refs": [summary_artifact_path],
    })
}

async fn list_project_memory_hits(
    repo_binding: &CoderRepoBinding,
    query: &str,
    limit: usize,
) -> Vec<Value> {
    let Some(manager) = open_semantic_memory_manager().await else {
        return Vec::new();
    };
    let Ok(results) = manager
        .search(
            query,
            Some(MemoryTier::Project),
            Some(&repo_binding.project_id),
            None,
            Some(limit.clamp(1, 20) as i64),
        )
        .await
    else {
        return Vec::new();
    };
    results
        .into_iter()
        .map(|hit| {
            json!({
                "source": "project_memory",
                "memory_id": hit.chunk.id,
                "score": hit.similarity,
                "content": hit.chunk.content,
                "memory_tier": hit.chunk.tier,
                "content_source": hit.chunk.source,
                "source_path": hit.chunk.source_path,
                "created_at": hit.chunk.created_at,
            })
        })
        .collect::<Vec<_>>()
}

fn governed_memory_subjects(record: &CoderRunRecord) -> Vec<String> {
    let mut subjects = Vec::new();
    if let Some(source_client) = record
        .source_client
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        subjects.push(source_client.to_string());
    }
    subjects.push("default".to_string());
    subjects.sort();
    subjects.dedup();
    subjects
}

async fn list_governed_memory_hits(
    record: &CoderRunRecord,
    query: &str,
    limit: usize,
) -> Vec<Value> {
    let Some(db) = super::skills_memory::open_global_memory_db().await else {
        return Vec::new();
    };
    let mut hits = Vec::<Value>::new();
    let mut seen_ids = HashSet::<String>::new();
    for subject in governed_memory_subjects(record) {
        let Ok(results) = db
            .search_global_memory(
                &subject,
                query,
                limit.clamp(1, 20) as i64,
                Some(&record.repo_binding.project_id),
                None,
                None,
            )
            .await
        else {
            continue;
        };
        for hit in results {
            if !seen_ids.insert(hit.record.id.clone()) {
                continue;
            }
            hits.push(json!({
                "source": "governed_memory",
                "memory_id": hit.record.id,
                "score": hit.score,
                "content": hit.record.content,
                "metadata": hit.record.metadata,
                "memory_visibility": hit.record.visibility,
                "source_type": hit.record.source_type,
                "run_id": hit.record.run_id,
                "project_tag": hit.record.project_tag,
                "subject": subject,
                "created_at_ms": hit.record.created_at_ms,
            }));
        }
    }
    hits
}

async fn collect_issue_triage_memory_hits(
    state: &AppState,
    record: &CoderRunRecord,
    query: &str,
    limit: usize,
) -> Result<Vec<Value>, StatusCode> {
    let mut hits = list_repo_memory_candidates(
        state,
        &record.repo_binding.repo_slug,
        record.github_ref.as_ref(),
        limit,
    )
    .await?;
    let mut project_hits = list_project_memory_hits(&record.repo_binding, query, limit).await;
    let mut governed_hits = list_governed_memory_hits(record, query, limit).await;
    hits.append(&mut project_hits);
    hits.append(&mut governed_hits);
    hits.sort_by(|a, b| compare_coder_memory_hits(record, a, b));
    hits.truncate(limit.clamp(1, 20));
    Ok(hits)
}

fn compare_coder_memory_hits(record: &CoderRunRecord, a: &Value, b: &Value) -> std::cmp::Ordering {
    let a_same_ref = a.get("same_ref").and_then(Value::as_bool).unwrap_or(false);
    let b_same_ref = b.get("same_ref").and_then(Value::as_bool).unwrap_or(false);
    let a_same_issue = a
        .get("same_issue")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let b_same_issue = b
        .get("same_issue")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let a_score = a.get("score").and_then(Value::as_f64).unwrap_or(0.0);
    let b_score = b.get("score").and_then(Value::as_f64).unwrap_or(0.0);
    let ref_order = b_same_ref
        .cmp(&a_same_ref)
        .then_with(|| b_same_issue.cmp(&a_same_issue));
    let kind_weight = |hit: &Value| match memory_hit_kind(hit).as_deref() {
        Some("failure_pattern")
            if matches!(record.workflow_mode, CoderWorkflowMode::IssueTriage) =>
        {
            4_u8
        }
        Some("triage_memory") if matches!(record.workflow_mode, CoderWorkflowMode::IssueTriage) => {
            3_u8
        }
        Some("run_outcome")
            if matches!(record.workflow_mode, CoderWorkflowMode::IssueTriage)
                && memory_hit_workflow_mode(hit).as_deref() == Some("issue_triage") =>
        {
            2_u8
        }
        Some("fix_pattern") if matches!(record.workflow_mode, CoderWorkflowMode::IssueFix) => 4_u8,
        Some("validation_memory")
            if matches!(record.workflow_mode, CoderWorkflowMode::IssueFix) =>
        {
            3_u8
        }
        Some("run_outcome")
            if matches!(record.workflow_mode, CoderWorkflowMode::IssueFix)
                && memory_hit_workflow_mode(hit).as_deref() == Some("issue_fix") =>
        {
            2_u8
        }
        Some("triage_memory") if matches!(record.workflow_mode, CoderWorkflowMode::IssueFix) => {
            1_u8
        }
        Some("merge_recommendation_memory")
            if matches!(record.workflow_mode, CoderWorkflowMode::MergeRecommendation) =>
        {
            4_u8
        }
        Some("run_outcome")
            if matches!(record.workflow_mode, CoderWorkflowMode::MergeRecommendation)
                && memory_hit_workflow_mode(hit).as_deref() == Some("merge_recommendation") =>
        {
            3_u8
        }
        Some("regression_signal")
            if matches!(record.workflow_mode, CoderWorkflowMode::MergeRecommendation) =>
        {
            2_u8
        }
        Some("review_memory") if matches!(record.workflow_mode, CoderWorkflowMode::PrReview) => {
            4_u8
        }
        Some("regression_signal")
            if matches!(record.workflow_mode, CoderWorkflowMode::PrReview) =>
        {
            3_u8
        }
        Some("run_outcome")
            if matches!(record.workflow_mode, CoderWorkflowMode::PrReview)
                && memory_hit_workflow_mode(hit).as_deref() == Some("pr_review") =>
        {
            2_u8
        }
        _ => 1_u8,
    };
    let structured_signal_weight = |hit: &Value| {
        let payload = hit
            .get("payload")
            .or_else(|| hit.get("metadata"))
            .cloned()
            .unwrap_or(Value::Null);
        let list_weight = |key: &str| {
            payload
                .get(key)
                .and_then(Value::as_array)
                .map(|rows| !rows.is_empty() as u8)
                .unwrap_or(0_u8)
        };
        match record.workflow_mode {
            CoderWorkflowMode::MergeRecommendation => {
                list_weight("blockers")
                    + list_weight("required_checks")
                    + list_weight("required_approvals")
            }
            CoderWorkflowMode::PrReview => {
                list_weight("blockers")
                    + list_weight("requested_changes")
                    + list_weight("regression_signals")
            }
            _ => 0_u8,
        }
    };
    let governed_issue_fix_weight = |hit: &Value| {
        (matches!(record.workflow_mode, CoderWorkflowMode::IssueFix)
            && matches!(
                memory_hit_kind(hit).as_deref(),
                Some("fix_pattern") | Some("validation_memory")
            )
            && hit.get("source").and_then(Value::as_str) == Some("governed_memory")) as u8
    };
    let governed_issue_triage_weight = |hit: &Value| {
        (matches!(record.workflow_mode, CoderWorkflowMode::IssueTriage)
            && memory_hit_kind(hit).as_deref() == Some("failure_pattern")
            && hit.get("source").and_then(Value::as_str) == Some("governed_memory")) as u8
    };
    let governed_issue_triage_outcome_weight = |hit: &Value| {
        (matches!(record.workflow_mode, CoderWorkflowMode::IssueTriage)
            && memory_hit_kind(hit).as_deref() == Some("run_outcome")
            && memory_hit_workflow_mode(hit).as_deref() == Some("issue_triage")
            && hit.get("source").and_then(Value::as_str) == Some("governed_memory")) as u8
    };
    let governed_pr_review_weight = |hit: &Value| {
        (matches!(record.workflow_mode, CoderWorkflowMode::PrReview)
            && memory_hit_kind(hit).as_deref() == Some("regression_signal")
            && hit.get("source").and_then(Value::as_str) == Some("governed_memory")) as u8
    };
    let governed_merge_weight = |hit: &Value| {
        (matches!(record.workflow_mode, CoderWorkflowMode::MergeRecommendation)
            && memory_hit_kind(hit).as_deref() == Some("run_outcome")
            && memory_hit_workflow_mode(hit).as_deref() == Some("merge_recommendation")
            && hit.get("source").and_then(Value::as_str) == Some("governed_memory")) as u8
    };
    let kind_order = kind_weight(b).cmp(&kind_weight(a));
    let structured_order = structured_signal_weight(b).cmp(&structured_signal_weight(a));
    let governed_issue_fix_order = governed_issue_fix_weight(b).cmp(&governed_issue_fix_weight(a));
    let governed_issue_triage_order =
        governed_issue_triage_weight(b).cmp(&governed_issue_triage_weight(a));
    let governed_issue_triage_outcome_order =
        governed_issue_triage_outcome_weight(b).cmp(&governed_issue_triage_outcome_weight(a));
    let governed_pr_review_order = governed_pr_review_weight(b).cmp(&governed_pr_review_weight(a));
    let governed_merge_order = governed_merge_weight(b).cmp(&governed_merge_weight(a));
    let score_order = || {
        b_score
            .partial_cmp(&a_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.get("created_at_ms")
                    .and_then(Value::as_u64)
                    .cmp(&a.get("created_at_ms").and_then(Value::as_u64))
            })
    };
    ref_order
        .then_with(|| governed_issue_triage_order)
        .then_with(|| governed_issue_triage_outcome_order)
        .then_with(|| governed_issue_fix_order)
        .then_with(|| governed_pr_review_order)
        .then_with(|| governed_merge_order)
        .then_with(|| kind_order)
        .then_with(|| structured_order)
        .then_with(score_order)
}

fn memory_hit_workflow_mode(hit: &Value) -> Option<String> {
    value_string(
        hit.get("payload")
            .and_then(|row| row.get("workflow_mode"))
            .or_else(|| hit.get("metadata").and_then(|row| row.get("workflow_mode"))),
    )
}

fn memory_hit_kind(hit: &Value) -> Option<String> {
    value_string(hit.get("kind"))
        .or_else(|| value_string(hit.get("metadata").and_then(|row| row.get("kind"))))
}

fn derive_failure_pattern_duplicate_matches(
    hits: &[Value],
    fingerprint: Option<&str>,
    limit: usize,
) -> Vec<Value> {
    let normalized_fingerprint = fingerprint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);
    let mut duplicates = Vec::<Value>::new();
    let mut seen = HashSet::<String>::new();
    for hit in hits {
        let kind = memory_hit_kind(hit).unwrap_or_default();
        if kind != "failure_pattern" {
            continue;
        }
        let hit_fingerprint =
            value_string(hit.get("payload").and_then(|row| row.get("fingerprint"))).or_else(|| {
                value_string(
                    hit.get("metadata")
                        .and_then(|row| row.get("failure_pattern_fingerprint")),
                )
            });
        let exact_fingerprint =
            normalized_fingerprint.is_some() && normalized_fingerprint == hit_fingerprint;
        let score = hit.get("score").and_then(Value::as_f64).unwrap_or(0.0);
        if !exact_fingerprint && score <= 0.0 {
            continue;
        }
        let identity = value_string(hit.get("candidate_id"))
            .or_else(|| value_string(hit.get("memory_id")))
            .or_else(|| hit_fingerprint.clone())
            .unwrap_or_else(|| format!("failure-pattern-{}", duplicates.len()));
        if !seen.insert(identity) {
            continue;
        }
        duplicates.push(json!({
            "kind": "failure_pattern",
            "source": hit.get("source").cloned().unwrap_or(Value::Null),
            "match_reason": if exact_fingerprint { "exact_fingerprint" } else { "historical_failure_pattern" },
            "score": if exact_fingerprint { Value::from(1.0) } else { Value::from(score) },
            "fingerprint": hit_fingerprint,
            "summary": hit.get("summary").cloned().unwrap_or_else(|| hit.get("content").cloned().unwrap_or(Value::Null)),
            "linked_issue_numbers": hit
                .get("payload")
                .and_then(|row| row.get("linked_issue_numbers"))
                .cloned()
                .or_else(|| hit.get("metadata").and_then(|row| row.get("linked_issue_numbers")).cloned())
                .unwrap_or_else(|| Value::Array(Vec::new())),
            "recurrence_count": hit
                .get("payload")
                .and_then(|row| row.get("recurrence_count"))
                .cloned()
                .or_else(|| hit.get("metadata").and_then(|row| row.get("recurrence_count")).cloned())
                .unwrap_or_else(|| Value::from(1_u64)),
            "affected_components": hit
                .get("payload")
                .and_then(|row| row.get("affected_components"))
                .cloned()
                .unwrap_or_else(|| Value::Array(Vec::new())),
            "candidate_id": hit.get("candidate_id").cloned().unwrap_or(Value::Null),
            "memory_id": hit.get("memory_id").cloned().unwrap_or(Value::Null),
            "artifact_path": hit.get("path").cloned().unwrap_or(Value::Null),
            "run_id": hit.get("run_id").cloned().unwrap_or_else(|| hit.get("source_coder_run_id").cloned().unwrap_or(Value::Null)),
        }));
    }
    duplicates.sort_by(compare_failure_pattern_duplicate_matches);
    duplicates.truncate(limit.clamp(1, 8));
    duplicates
}
fn default_coder_memory_query(record: &CoderRunRecord) -> String {
    match record.github_ref.as_ref() {
        Some(reference) if matches!(reference.kind, CoderGithubRefKind::PullRequest) => {
            format!(
                "{} pull request #{}",
                record.repo_binding.repo_slug, reference.number
            )
        }
        Some(reference) => format!(
            "{} issue #{}",
            record.repo_binding.repo_slug, reference.number
        ),
        None => record.repo_binding.repo_slug.clone(),
    }
}

fn value_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(crate) fn failure_pattern_fingerprint(
    repo_slug: &str,
    summary: &str,
    affected_files: &[String],
    canonical_markers: &[String],
) -> String {
    let mut parts = VecDeque::<String>::new();
    parts.push_back(repo_slug.to_string());
    parts.push_back(summary.trim().to_string());
    for marker in canonical_markers {
        parts.push_back(marker.trim().to_string());
    }
    for path in affected_files {
        parts.push_back(path.trim().to_string());
    }
    let joined = parts.into_iter().collect::<Vec<_>>().join("|");
    crate::sha256_hex(&[joined.as_str()])
}

pub(crate) async fn find_failure_pattern_duplicates(
    state: &AppState,
    repo_slug: &str,
    project_id: Option<&str>,
    subjects: &[String],
    query: &str,
    fingerprint: Option<&str>,
    limit: usize,
) -> Result<Vec<Value>, StatusCode> {
    let mut hits =
        list_repo_memory_candidates(state, repo_slug, None, limit.saturating_mul(3)).await?;
    if let Some(db) = super::skills_memory::open_global_memory_db().await {
        let mut seen_memory_ids = HashSet::<String>::new();
        for subject in subjects {
            let Ok(results) = db
                .search_global_memory(
                    subject,
                    query,
                    limit.clamp(1, 20) as i64,
                    project_id,
                    None,
                    None,
                )
                .await
            else {
                continue;
            };
            for hit in results {
                if !seen_memory_ids.insert(hit.record.id.clone()) {
                    continue;
                }
                hits.push(json!({
                    "source": "governed_memory",
                    "memory_id": hit.record.id,
                    "score": hit.score,
                    "content": hit.record.content,
                    "metadata": hit.record.metadata,
                    "memory_visibility": hit.record.visibility,
                    "source_type": hit.record.source_type,
                    "run_id": hit.record.run_id,
                    "project_tag": hit.record.project_tag,
                    "subject": subject,
                    "created_at_ms": hit.record.created_at_ms,
                }));
            }
        }
        if let Some(target_fingerprint) =
            fingerprint.map(str::trim).filter(|value| !value.is_empty())
        {
            for subject in subjects {
                let Ok(records) = db.list_global_memory(subject, None, 200, 0).await else {
                    continue;
                };
                for record in records {
                    if !seen_memory_ids.insert(record.id.clone()) {
                        continue;
                    }
                    if record.project_tag.as_deref() != project_id.or(Some(repo_slug)) {
                        continue;
                    }
                    let Some(metadata) = record.metadata.as_ref() else {
                        continue;
                    };
                    let stored_fingerprint = metadata
                        .get("failure_pattern_fingerprint")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty());
                    if stored_fingerprint != Some(target_fingerprint) {
                        continue;
                    }
                    hits.push(json!({
                        "source": "governed_memory",
                        "memory_id": record.id,
                        "score": 1.0,
                        "content": record.content,
                        "metadata": record.metadata,
                        "memory_visibility": record.visibility,
                        "source_type": record.source_type,
                        "run_id": record.run_id,
                        "project_tag": record.project_tag,
                        "subject": subject,
                        "created_at_ms": record.created_at_ms,
                    }));
                }
            }
        }
    }
    Ok(derive_failure_pattern_duplicate_matches(
        &hits,
        fingerprint,
        limit,
    ))
}

async fn write_coder_artifact(
    state: &AppState,
    linked_context_run_id: &str,
    artifact_id: &str,
    artifact_type: &str,
    relative_path: &str,
    payload: &Value,
) -> Result<ContextBlackboardArtifact, StatusCode> {
    let path =
        super::context_runs::context_run_dir(state, linked_context_run_id).join(relative_path);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    let raw =
        serde_json::to_string_pretty(payload).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::fs::write(&path, raw)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let artifact = ContextBlackboardArtifact {
        id: artifact_id.to_string(),
        ts_ms: crate::now_ms(),
        path: path.to_string_lossy().to_string(),
        artifact_type: artifact_type.to_string(),
        step_id: None,
        source_event_id: None,
    };
    context_run_engine()
        .commit_blackboard_patch(
            state,
            linked_context_run_id,
            ContextBlackboardPatchOp::AddArtifact,
            serde_json::to_value(&artifact).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        )
        .await?;
    Ok(artifact)
}

async fn write_coder_memory_candidate_artifact(
    state: &AppState,
    record: &CoderRunRecord,
    kind: CoderMemoryCandidateKind,
    summary: Option<String>,
    task_id: Option<String>,
    payload: Value,
) -> Result<(String, ContextBlackboardArtifact), StatusCode> {
    let candidate_id = format!("memcand-{}", Uuid::new_v4().simple());
    let stored_payload = json!({
        "candidate_id": candidate_id,
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "kind": kind,
        "task_id": task_id,
        "summary": summary,
        "payload": payload,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "created_at_ms": crate::now_ms(),
    });
    let artifact = write_coder_artifact(
        state,
        &record.linked_context_run_id,
        &candidate_id,
        "coder_memory_candidate",
        &format!("coder_memory/{candidate_id}.json"),
        &stored_payload,
    )
    .await?;
    publish_coder_artifact_added(state, record, &artifact, Some("artifact_write"), {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("memory_candidate"));
        extra.insert("candidate_id".to_string(), json!(candidate_id));
        extra.insert("candidate_kind".to_string(), json!(kind));
        extra
    });
    publish_coder_run_event(
        state,
        "coder.memory.candidate_added",
        record,
        Some("artifact_write"),
        {
            let mut extra = coder_artifact_event_fields(&artifact, Some("memory_candidate"));
            extra.insert("candidate_id".to_string(), json!(candidate_id));
            extra.insert("candidate_kind".to_string(), json!(kind));
            extra
        },
    );
    Ok((candidate_id, artifact))
}

fn build_governed_memory_content(candidate_payload: &Value) -> Option<String> {
    let base = candidate_payload
        .get("summary")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            candidate_payload
                .get("payload")
                .and_then(|row| row.get("summary"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        });
    let payload = candidate_payload.get("payload");
    let mut segments = Vec::<String>::new();
    if let Some(summary) = base {
        segments.push(summary);
    }
    let push_optional = |segments: &mut Vec<String>, label: &str, value: Option<&Value>| {
        if let Some(text) = value
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            segments.push(format!("{label}: {text}"));
        }
    };
    let push_list = |segments: &mut Vec<String>, label: &str, value: Option<&Value>| {
        let values = value
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| row.as_str().map(str::trim))
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !values.is_empty() {
            segments.push(format!("{label}: {}", values.join(", ")));
        }
    };
    let push_object_summaries = |segments: &mut Vec<String>, label: &str, value: Option<&Value>| {
        let values = value
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| {
                        row.get("summary")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(ToString::to_string)
                            .or_else(|| {
                                row.get("kind")
                                    .and_then(Value::as_str)
                                    .map(str::trim)
                                    .filter(|value| !value.is_empty())
                                    .map(ToString::to_string)
                            })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !values.is_empty() {
            segments.push(format!("{label}: {}", values.join(", ")));
        }
    };
    push_optional(
        &mut segments,
        "workflow",
        payload.and_then(|row| row.get("workflow_mode")),
    );
    push_optional(
        &mut segments,
        "result",
        payload.and_then(|row| row.get("result")),
    );
    push_optional(
        &mut segments,
        "verdict",
        payload.and_then(|row| row.get("verdict")),
    );
    push_optional(
        &mut segments,
        "recommendation",
        payload.and_then(|row| row.get("recommendation")),
    );
    push_optional(
        &mut segments,
        "fix_strategy",
        payload.and_then(|row| row.get("fix_strategy")),
    );
    push_optional(
        &mut segments,
        "root_cause",
        payload.and_then(|row| row.get("root_cause")),
    );
    push_optional(
        &mut segments,
        "risk_level",
        payload.and_then(|row| row.get("risk_level")),
    );
    push_list(
        &mut segments,
        "changed_files",
        payload.and_then(|row| row.get("changed_files")),
    );
    push_list(
        &mut segments,
        "blockers",
        payload.and_then(|row| row.get("blockers")),
    );
    push_list(
        &mut segments,
        "requested_changes",
        payload.and_then(|row| row.get("requested_changes")),
    );
    push_list(
        &mut segments,
        "required_checks",
        payload.and_then(|row| row.get("required_checks")),
    );
    push_list(
        &mut segments,
        "required_approvals",
        payload.and_then(|row| row.get("required_approvals")),
    );
    push_list(
        &mut segments,
        "validation_steps",
        payload.and_then(|row| row.get("validation_steps")),
    );
    push_object_summaries(
        &mut segments,
        "validation_results",
        payload.and_then(|row| row.get("validation_results")),
    );
    push_object_summaries(
        &mut segments,
        "regression_signals",
        payload.and_then(|row| row.get("regression_signals")),
    );
    if segments.is_empty() {
        None
    } else {
        Some(segments.join("\n"))
    }
}

fn coder_memory_partition(record: &CoderRunRecord, tier: GovernedMemoryTier) -> MemoryPartition {
    MemoryPartition {
        org_id: record.repo_binding.workspace_id.clone(),
        workspace_id: record.repo_binding.workspace_id.clone(),
        project_id: record.repo_binding.project_id.clone(),
        tier,
    }
}

fn project_coder_phase(run: &ContextRunState) -> &'static str {
    if matches!(
        run.status,
        ContextRunStatus::Queued | ContextRunStatus::Planning
    ) {
        return "bootstrapping";
    }
    if matches!(run.status, ContextRunStatus::Completed) {
        return "completed";
    }
    if matches!(run.status, ContextRunStatus::Cancelled) {
        return "cancelled";
    }
    if matches!(
        run.status,
        ContextRunStatus::Failed | ContextRunStatus::Blocked
    ) {
        return "failed";
    }
    for task in &run.tasks {
        if matches!(
            task.status,
            ContextBlackboardTaskStatus::Runnable | ContextBlackboardTaskStatus::InProgress
        ) {
            return match task.workflow_node_id.as_deref() {
                Some("ingest_reference") => "bootstrapping",
                Some("retrieve_memory") => "memory_retrieval",
                Some("inspect_repo") => "repo_inspection",
                Some("inspect_pull_request") => "repo_inspection",
                Some("attempt_reproduction") => "reproduction",
                Some("review_pull_request") => "analysis",
                Some("write_triage_artifact") => "artifact_write",
                Some("write_review_artifact") => "artifact_write",
                Some("write_fix_artifact") => "artifact_write",
                Some("write_merge_artifact") => "artifact_write",
                _ => "analysis",
            };
        }
    }
    "analysis"
}

async fn finalize_coder_workflow_run(
    state: &AppState,
    record: &CoderRunRecord,
    workflow_node_ids: &[&str],
    completion_reason: &str,
) -> Result<ContextRunState, StatusCode> {
    let mut run = load_context_run_state(state, &record.linked_context_run_id).await?;
    let now = crate::now_ms();
    let workflow_nodes: HashSet<&str> = workflow_node_ids.iter().copied().collect();
    for task in &mut run.tasks {
        if task
            .workflow_node_id
            .as_deref()
            .is_some_and(|node_id| workflow_nodes.contains(node_id))
        {
            task.status = ContextBlackboardTaskStatus::Done;
            task.lease_owner = None;
            task.lease_token = None;
            task.lease_expires_at_ms = None;
            task.updated_ts = now;
            task.task_rev = task.task_rev.saturating_add(1);
        }
    }
    for workflow_node_id in workflow_node_ids {
        if run
            .tasks
            .iter()
            .any(|task| task.workflow_node_id.as_deref() == Some(*workflow_node_id))
        {
            continue;
        }
        let task_type = match *workflow_node_id {
            "retrieve_memory" => "research",
            "inspect_repo" | "inspect_pull_request" | "inspect_issue_context" => "inspection",
            "attempt_reproduction"
            | "review_pull_request"
            | "prepare_fix"
            | "assess_merge_readiness" => "analysis",
            _ => "implementation",
        };
        run.tasks.push(super::context_types::ContextBlackboardTask {
            id: format!("coder-autocomplete-{}", Uuid::new_v4().simple()),
            task_type: task_type.to_string(),
            payload: json!({
                "task_kind": task_type,
                "title": format!("Complete workflow step: {workflow_node_id}"),
                "source": "coder_summary_finalize",
            }),
            status: ContextBlackboardTaskStatus::Done,
            workflow_id: Some(run.run_type.clone()),
            workflow_node_id: Some((*workflow_node_id).to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            assigned_agent: None,
            priority: 0,
            attempt: 0,
            max_attempts: 1,
            last_error: None,
            next_retry_at_ms: None,
            lease_owner: None,
            lease_token: None,
            lease_expires_at_ms: None,
            task_rev: 1,
            created_ts: now,
            updated_ts: now,
        });
    }
    run.status = ContextRunStatus::Completed;
    run.updated_at_ms = now;
    run.why_next_step = Some(completion_reason.to_string());
    ensure_context_run_dir(state, &record.linked_context_run_id).await?;
    save_context_run_state(state, &run).await?;
    publish_coder_run_event(
        state,
        "coder.run.phase_changed",
        record,
        Some(project_coder_phase(&run)),
        {
            let mut extra = serde_json::Map::new();
            extra.insert("status".to_string(), json!(run.status));
            extra.insert("event_type".to_string(), json!("workflow_summary_recorded"));
            extra
        },
    );
    Ok(run)
}

async fn advance_coder_workflow_run(
    state: &AppState,
    record: &CoderRunRecord,
    completed_workflow_node_ids: &[&str],
    runnable_workflow_node_ids: &[&str],
    next_reason: &str,
) -> Result<ContextRunState, StatusCode> {
    let mut run = load_context_run_state(state, &record.linked_context_run_id).await?;
    let now = crate::now_ms();
    let completed_nodes: HashSet<&str> = completed_workflow_node_ids.iter().copied().collect();
    let runnable_nodes: HashSet<&str> = runnable_workflow_node_ids.iter().copied().collect();
    for task in &mut run.tasks {
        if task
            .workflow_node_id
            .as_deref()
            .is_some_and(|node_id| completed_nodes.contains(node_id))
        {
            task.status = ContextBlackboardTaskStatus::Done;
            task.lease_owner = None;
            task.lease_token = None;
            task.lease_expires_at_ms = None;
            task.updated_ts = now;
            task.task_rev = task.task_rev.saturating_add(1);
            continue;
        }
        if task
            .workflow_node_id
            .as_deref()
            .is_some_and(|node_id| runnable_nodes.contains(node_id))
            && matches!(task.status, ContextBlackboardTaskStatus::Pending)
        {
            task.status = ContextBlackboardTaskStatus::Runnable;
            task.updated_ts = now;
            task.task_rev = task.task_rev.saturating_add(1);
        }
    }
    for workflow_node_id in completed_workflow_node_ids {
        if run
            .tasks
            .iter()
            .any(|task| task.workflow_node_id.as_deref() == Some(*workflow_node_id))
        {
            continue;
        }
        let task_type = match *workflow_node_id {
            "retrieve_memory" => "research",
            "inspect_repo" | "inspect_pull_request" | "inspect_issue_context" => "inspection",
            "attempt_reproduction"
            | "review_pull_request"
            | "prepare_fix"
            | "assess_merge_readiness" => "analysis",
            _ => "implementation",
        };
        run.tasks.push(super::context_types::ContextBlackboardTask {
            id: format!("coder-progress-complete-{}", Uuid::new_v4().simple()),
            task_type: task_type.to_string(),
            payload: json!({
                "task_kind": task_type,
                "title": format!("Complete workflow step: {workflow_node_id}"),
                "source": "coder_progress_advance",
            }),
            status: ContextBlackboardTaskStatus::Done,
            workflow_id: Some(run.run_type.clone()),
            workflow_node_id: Some((*workflow_node_id).to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            assigned_agent: None,
            priority: 0,
            attempt: 0,
            max_attempts: 1,
            last_error: None,
            next_retry_at_ms: None,
            lease_owner: None,
            lease_token: None,
            lease_expires_at_ms: None,
            task_rev: 1,
            created_ts: now,
            updated_ts: now,
        });
    }
    for workflow_node_id in runnable_workflow_node_ids {
        if run
            .tasks
            .iter()
            .any(|task| task.workflow_node_id.as_deref() == Some(*workflow_node_id))
        {
            continue;
        }
        let task_type = match *workflow_node_id {
            "retrieve_memory" => "research",
            "inspect_repo" | "inspect_pull_request" | "inspect_issue_context" => "inspection",
            "attempt_reproduction"
            | "review_pull_request"
            | "prepare_fix"
            | "assess_merge_readiness" => "analysis",
            _ => "implementation",
        };
        run.tasks.push(super::context_types::ContextBlackboardTask {
            id: format!("coder-progress-runnable-{}", Uuid::new_v4().simple()),
            task_type: task_type.to_string(),
            payload: json!({
                "task_kind": task_type,
                "title": format!("Continue workflow step: {workflow_node_id}"),
                "source": "coder_progress_advance",
            }),
            status: ContextBlackboardTaskStatus::Runnable,
            workflow_id: Some(run.run_type.clone()),
            workflow_node_id: Some((*workflow_node_id).to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            assigned_agent: None,
            priority: 0,
            attempt: 0,
            max_attempts: 1,
            last_error: None,
            next_retry_at_ms: None,
            lease_owner: None,
            lease_token: None,
            lease_expires_at_ms: None,
            task_rev: 1,
            created_ts: now,
            updated_ts: now,
        });
    }
    run.status = ContextRunStatus::Running;
    run.started_at_ms.get_or_insert(now);
    run.updated_at_ms = now;
    run.why_next_step = Some(next_reason.to_string());
    ensure_context_run_dir(state, &record.linked_context_run_id).await?;
    save_context_run_state(state, &run).await?;
    publish_coder_run_event(
        state,
        "coder.run.phase_changed",
        record,
        Some(project_coder_phase(&run)),
        {
            let mut extra = serde_json::Map::new();
            extra.insert("status".to_string(), json!(run.status));
            extra.insert("event_type".to_string(), json!("workflow_progressed"));
            extra
        },
    );
    Ok(run)
}

async fn bootstrap_coder_workflow_run(
    state: &AppState,
    record: &CoderRunRecord,
    completed_workflow_node_ids: &[&str],
    runnable_workflow_node_ids: &[&str],
    next_reason: &str,
) -> Result<ContextRunState, StatusCode> {
    advance_coder_workflow_run(
        state,
        record,
        completed_workflow_node_ids,
        runnable_workflow_node_ids,
        next_reason,
    )
    .await
}

fn default_coder_worker_agent_id(input: Option<&str>) -> String {
    input
        .map(str::trim)
        .filter(|row| !row.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| "coder_engine_worker".to_string())
}

fn summarize_workflow_memory_hits(
    record: &CoderRunRecord,
    run: &ContextRunState,
    workflow_node_id: &str,
) -> Vec<String> {
    run.tasks
        .iter()
        .find(|task| task.workflow_node_id.as_deref() == Some(workflow_node_id))
        .and_then(|task| task.payload.get("memory_hits"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .take(3)
                .filter_map(|row| {
                    row.get("summary")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToString::to_string)
                        .or_else(|| {
                            row.get("content")
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(|value| value.chars().take(120).collect::<String>())
                        })
                })
                .collect::<Vec<_>>()
        })
        .filter(|rows| !rows.is_empty())
        .unwrap_or_else(|| {
            vec![format!(
                "No reusable workflow memory was available for {}.",
                record.repo_binding.repo_slug
            )]
        })
}

async fn complete_claimed_coder_task(
    state: &AppState,
    run_id: String,
    task: &super::context_types::ContextBlackboardTask,
    agent_id: &str,
) -> Result<(), StatusCode> {
    let lease_token = task
        .lease_token
        .clone()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let response = context_run_task_transition(
        State(state.clone()),
        Path((run_id, task.id.clone())),
        Json(ContextTaskTransitionInput {
            action: "complete".to_string(),
            command_id: Some(format!(
                "coder:{}:complete:{}",
                task.id,
                Uuid::new_v4().simple()
            )),
            expected_task_rev: Some(task.task_rev),
            lease_token: Some(lease_token),
            agent_id: Some(agent_id.to_string()),
            status: None,
            error: None,
            lease_ms: None,
        }),
    )
    .await?;
    let payload = response.0;
    if payload.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(StatusCode::CONFLICT);
    }
    Ok(())
}

async fn fail_claimed_coder_task(
    state: &AppState,
    run_id: String,
    task: &super::context_types::ContextBlackboardTask,
    agent_id: &str,
    error: &str,
) -> Result<(), StatusCode> {
    let lease_token = task
        .lease_token
        .clone()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let response = context_run_task_transition(
        State(state.clone()),
        Path((run_id, task.id.clone())),
        Json(ContextTaskTransitionInput {
            action: "fail".to_string(),
            command_id: Some(format!(
                "coder:{}:fail:{}",
                task.id,
                Uuid::new_v4().simple()
            )),
            expected_task_rev: Some(task.task_rev),
            lease_token: Some(lease_token),
            agent_id: Some(agent_id.to_string()),
            status: None,
            error: Some(crate::truncate_text(error, 500)),
            lease_ms: None,
        }),
    )
    .await?;
    let payload = response.0;
    if payload.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(StatusCode::CONFLICT);
    }
    Ok(())
}

async fn dispatch_issue_triage_task(
    state: AppState,
    record: &CoderRunRecord,
    task: &super::context_types::ContextBlackboardTask,
    agent_id: &str,
) -> Result<Value, StatusCode> {
    let run = load_context_run_state(&state, &record.linked_context_run_id).await?;
    let issue_number = record
        .github_ref
        .as_ref()
        .map(|row| row.number)
        .unwrap_or_default();
    match task.workflow_node_id.as_deref() {
        Some("inspect_repo") => {
            let memory_hits_used = summarize_workflow_memory_hits(record, &run, "retrieve_memory");
            let summary = format!(
                "Engine worker inspected likely repo areas for {} issue #{}.",
                record.repo_binding.repo_slug, issue_number
            );
            let response = coder_triage_inspection_report_create(
                State(state),
                Path(record.coder_run_id.clone()),
                Json(CoderTriageInspectionReportCreateInput {
                    summary: Some(summary),
                    likely_areas: vec![
                        "repo workspace context".to_string(),
                        "prior triage memory".to_string(),
                    ],
                    affected_files: Vec::new(),
                    memory_hits_used,
                    notes: Some("Auto-generated by coder engine worker dispatch.".to_string()),
                }),
            )
            .await?;
            Ok(response.0)
        }
        Some("attempt_reproduction") => {
            let memory_hits_used = summarize_workflow_memory_hits(record, &run, "retrieve_memory");
            let response = coder_triage_reproduction_report_create(
                State(state),
                Path(record.coder_run_id.clone()),
                Json(CoderTriageReproductionReportCreateInput {
                    summary: Some(format!(
                        "Engine worker attempted constrained reproduction for {} issue #{}.",
                        record.repo_binding.repo_slug, issue_number
                    )),
                    outcome: Some("needs_follow_up".to_string()),
                    steps: vec![
                        "Review current issue context".to_string(),
                        "Use prior memory hits to constrain reproduction".to_string(),
                    ],
                    observed_logs: Vec::new(),
                    affected_files: Vec::new(),
                    memory_hits_used,
                    notes: Some("Auto-generated by coder engine worker dispatch.".to_string()),
                }),
            )
            .await?;
            Ok(response.0)
        }
        Some("write_triage_artifact") => {
            let memory_hits_used = summarize_workflow_memory_hits(record, &run, "retrieve_memory");
            let response = coder_triage_summary_create(
                State(state),
                Path(record.coder_run_id.clone()),
                Json(CoderTriageSummaryCreateInput {
                    summary: Some(format!(
                        "Engine worker completed initial triage for {} issue #{}.",
                        record.repo_binding.repo_slug, issue_number
                    )),
                    confidence: Some("medium".to_string()),
                    affected_files: Vec::new(),
                    duplicate_candidates: Vec::new(),
                    memory_hits_used,
                    reproduction: Some(json!({
                        "outcome": "needs_follow_up",
                        "source": "coder_engine_worker"
                    })),
                    notes: Some("Auto-generated by coder engine worker dispatch.".to_string()),
                }),
            )
            .await?;
            Ok(response.0)
        }
        Some("ingest_reference") | Some("retrieve_memory") => {
            complete_claimed_coder_task(
                &state,
                record.linked_context_run_id.clone(),
                task,
                agent_id,
            )
            .await?;
            let run = load_context_run_state(&state, &record.linked_context_run_id).await?;
            Ok(json!({
                "ok": true,
                "task": task,
                "run": run,
                "coder_run": coder_run_payload(record, &run),
                "dispatched": false,
                "reason": "bootstrap task completed through generic task transition"
            }))
        }
        _ => Err(StatusCode::CONFLICT),
    }
}

async fn dispatch_issue_fix_task(
    state: AppState,
    record: &CoderRunRecord,
    task: &super::context_types::ContextBlackboardTask,
    agent_id: &str,
) -> Result<Value, StatusCode> {
    let run = load_context_run_state(&state, &record.linked_context_run_id).await?;
    let issue_number = record
        .github_ref
        .as_ref()
        .map(|row| row.number)
        .unwrap_or_default();
    match task.workflow_node_id.as_deref() {
        Some("inspect_issue_context") => {
            let final_run = advance_coder_workflow_run(
                &state,
                record,
                &["inspect_issue_context"],
                &["prepare_fix"],
                "Issue context inspected; prepare a constrained fix.",
            )
            .await?;
            Ok(json!({
                "ok": true,
                "run": final_run,
                "coder_run": coder_run_payload(record, &final_run),
                "dispatched": false,
                "reason": "inspection task advanced through coder workflow progression"
            }))
        }
        Some("prepare_fix") => {
            let memory_hits_used = summarize_workflow_memory_hits(record, &run, "retrieve_memory");
            let worker_result = run_issue_fix_prepare_worker(&state, record, &run).await;
            let (worker_artifact, worker_payload) = match worker_result {
                Ok(result) => result,
                Err(error) => {
                    let detail = format!(
                        "Issue-fix worker session failed during prepare_fix with status {}.",
                        error
                    );
                    fail_claimed_coder_task(
                        &state,
                        record.linked_context_run_id.clone(),
                        task,
                        agent_id,
                        &detail,
                    )
                    .await?;
                    let failed = coder_run_transition(
                        &state,
                        record,
                        "run_failed",
                        ContextRunStatus::Failed,
                        Some(detail.clone()),
                    )
                    .await?;
                    return Ok(json!({
                        "ok": false,
                        "error": detail,
                        "code": "CODER_WORKER_SESSION_FAILED",
                        "run": failed.get("run").cloned().unwrap_or(Value::Null),
                        "coder_run": failed.get("coder_run").cloned().unwrap_or(Value::Null),
                    }));
                }
            };
            let plan_artifact = write_issue_fix_plan_artifact(
                &state,
                record,
                &worker_payload,
                &memory_hits_used,
                Some("analysis"),
            )
            .await?;
            let changed_file_artifact = write_issue_fix_changed_file_evidence_artifact(
                &state,
                record,
                &worker_payload,
                Some("analysis"),
            )
            .await?;
            let final_run = advance_coder_workflow_run(
                &state,
                record,
                &["prepare_fix"],
                &["validate_fix"],
                "Fix plan prepared; validate the constrained patch.",
            )
            .await?;
            Ok(json!({
                "ok": true,
                "worker_artifact": worker_artifact,
                "plan_artifact": plan_artifact,
                "changed_file_artifact": changed_file_artifact,
                "worker_session": worker_payload,
                "run": final_run,
                "coder_run": coder_run_payload(record, &final_run),
                "dispatched": true,
                "reason": "prepare_fix completed through a real coder worker session"
            }))
        }
        Some("validate_fix") => {
            let memory_hits_used = summarize_workflow_memory_hits(record, &run, "retrieve_memory");
            let worker_session = load_latest_coder_artifact_payload(
                &state,
                record,
                "coder_issue_fix_worker_session",
            )
            .await;
            let fix_plan =
                load_latest_coder_artifact_payload(&state, record, "coder_issue_fix_plan").await;
            let validation_worker =
                run_issue_fix_validation_worker(&state, record, &run, fix_plan.as_ref()).await;
            let (validation_worker_artifact, validation_worker_payload) = match validation_worker {
                Ok(result) => result,
                Err(error) => {
                    let detail = format!(
                        "Issue-fix validation worker session failed during validate_fix with status {}.",
                        error
                    );
                    fail_claimed_coder_task(
                        &state,
                        record.linked_context_run_id.clone(),
                        task,
                        agent_id,
                        &detail,
                    )
                    .await?;
                    let failed = coder_run_transition(
                        &state,
                        record,
                        "run_failed",
                        ContextRunStatus::Failed,
                        Some(detail.clone()),
                    )
                    .await?;
                    return Ok(json!({
                        "ok": false,
                        "error": detail,
                        "code": "CODER_WORKER_SESSION_FAILED",
                        "run": failed.get("run").cloned().unwrap_or(Value::Null),
                        "coder_run": failed.get("coder_run").cloned().unwrap_or(Value::Null),
                    }));
                }
            };
            let worker_summary = validation_worker_payload
                .get("assistant_text")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(|text| crate::truncate_text(text, 240));
            let response = coder_issue_fix_validation_report_create(
                State(state),
                Path(record.coder_run_id.clone()),
                Json(CoderIssueFixValidationReportCreateInput {
                    summary: fix_plan
                        .as_ref()
                        .and_then(|payload| payload.get("summary"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some(format!(
                            "Engine worker validated a constrained fix proposal for {} issue #{}.",
                            record.repo_binding.repo_slug, issue_number
                        ))),
                    root_cause: fix_plan
                        .as_ref()
                        .and_then(|payload| payload.get("root_cause"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some(
                            "Issue-fix worker used prior context and reusable memory.".to_string(),
                        )),
                    fix_strategy: fix_plan
                        .as_ref()
                        .and_then(|payload| payload.get("fix_strategy"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some(
                            "Apply a constrained patch after issue-context inspection."
                                .to_string(),
                        )),
                    changed_files: fix_plan
                        .as_ref()
                        .and_then(|payload| payload.get("changed_files"))
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                    validation_steps: {
                        let mut steps = fix_plan
                            .as_ref()
                            .and_then(|payload| payload.get("validation_steps"))
                            .and_then(Value::as_array)
                            .map(|rows| {
                                rows.iter()
                                    .filter_map(Value::as_str)
                                    .map(ToString::to_string)
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        steps.push("Inspect coder worker session output".to_string());
                        steps.push("Record validation outcome for follow-up artifact writing".to_string());
                        steps
                    },
                    validation_results: vec![json!({
                        "kind": "engine_worker_validation",
                        "status": "needs_follow_up",
                        "summary": "Validation completed through the coder engine worker bridge.",
                        "validation_worker_artifact_path": validation_worker_artifact.path,
                        "worker_session_id": worker_session.as_ref().and_then(|payload| payload.get("session_id")).cloned(),
                        "worker_session_run_id": worker_session.as_ref().and_then(|payload| payload.get("session_run_id")).cloned(),
                        "validation_session_id": validation_worker_payload.get("session_id").cloned(),
                        "validation_session_run_id": validation_worker_payload.get("session_run_id").cloned(),
                        "worker_assistant_excerpt": worker_summary,
                    })],
                    memory_hits_used,
                    notes: Some(format!(
                        "Auto-generated by coder engine worker dispatch. Worker session: {}. Validation session: {}. Plan artifact available: {}",
                        worker_session
                            .as_ref()
                            .and_then(|payload| payload.get("session_id"))
                            .and_then(Value::as_str)
                            .unwrap_or("unknown"),
                        validation_worker_payload
                            .get("session_id")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown"),
                        fix_plan.is_some()
                    )),
                }),
            )
            .await?;
            Ok(response.0)
        }
        Some("write_fix_artifact") => {
            let memory_hits_used = summarize_workflow_memory_hits(record, &run, "retrieve_memory");
            let fix_plan =
                load_latest_coder_artifact_payload(&state, record, "coder_issue_fix_plan").await;
            let validation_session = load_latest_coder_artifact_payload(
                &state,
                record,
                "coder_issue_fix_validation_session",
            )
            .await;
            let response = coder_issue_fix_summary_create(
                State(state),
                Path(record.coder_run_id.clone()),
                Json(CoderIssueFixSummaryCreateInput {
                    summary: fix_plan
                        .as_ref()
                        .and_then(|payload| payload.get("summary"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some(format!(
                            "Engine worker completed an initial issue-fix pass for {} issue #{}.",
                            record.repo_binding.repo_slug, issue_number
                        ))),
                    root_cause: fix_plan
                        .as_ref()
                        .and_then(|payload| payload.get("root_cause"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some(
                            "Issue context and prior reusable memory were inspected before fix generation."
                                .to_string(),
                        )),
                    fix_strategy: fix_plan
                        .as_ref()
                        .and_then(|payload| payload.get("fix_strategy"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some(
                            "Use a constrained patch flow with recorded validation evidence."
                                .to_string(),
                        )),
                    changed_files: fix_plan
                        .as_ref()
                        .and_then(|payload| payload.get("changed_files"))
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                    validation_steps: fix_plan
                        .as_ref()
                        .and_then(|payload| payload.get("validation_steps"))
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .filter(|rows| !rows.is_empty())
                        .unwrap_or_else(|| vec![
                            "Review constrained fix plan".to_string(),
                            "Record validation outcome for follow-up artifact writing".to_string(),
                        ]),
                    validation_results: vec![json!({
                        "kind": "engine_worker_validation",
                        "status": "needs_follow_up",
                        "summary": validation_session
                            .as_ref()
                            .and_then(|payload| payload.get("assistant_text"))
                            .and_then(Value::as_str)
                            .map(|text| crate::truncate_text(text, 240))
                            .unwrap_or_else(|| "Validation completed through the coder engine worker bridge.".to_string()),
                        "validation_session_id": validation_session.as_ref().and_then(|payload| payload.get("session_id")).cloned(),
                        "validation_session_run_id": validation_session.as_ref().and_then(|payload| payload.get("session_run_id")).cloned(),
                    })],
                    memory_hits_used,
                    notes: Some(format!(
                        "Auto-generated by coder engine worker dispatch. Plan artifact available: {}. Validation session available: {}",
                        fix_plan.is_some(),
                        validation_session.is_some()
                    )),
                }),
            )
            .await?;
            Ok(response.0)
        }
        _ => Err(StatusCode::CONFLICT),
    }
}

async fn dispatch_pr_review_task(
    state: AppState,
    record: &CoderRunRecord,
    task: &super::context_types::ContextBlackboardTask,
) -> Result<Value, StatusCode> {
    let run = load_context_run_state(&state, &record.linked_context_run_id).await?;
    let pull_number = record
        .github_ref
        .as_ref()
        .map(|row| row.number)
        .unwrap_or_default();
    match task.workflow_node_id.as_deref() {
        Some("inspect_pull_request") => {
            let final_run = advance_coder_workflow_run(
                &state,
                record,
                &["inspect_pull_request"],
                &["review_pull_request"],
                "Pull request inspected; perform the review analysis.",
            )
            .await?;
            Ok(json!({
                "ok": true,
                "run": final_run,
                "coder_run": coder_run_payload(record, &final_run),
                "dispatched": false,
                "reason": "inspect_pull_request advanced through coder workflow progression"
            }))
        }
        Some("review_pull_request") => {
            let memory_hits_used = summarize_workflow_memory_hits(record, &run, "retrieve_memory");
            let (worker_artifact, worker_payload) =
                run_pr_review_worker(&state, record, &run).await?;
            let parsed_review = parse_pr_review_from_worker_payload(&worker_payload);
            let response = coder_pr_review_evidence_create(
                State(state),
                Path(record.coder_run_id.clone()),
                Json(CoderPrReviewEvidenceCreateInput {
                    verdict: parsed_review
                        .get("verdict")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some("needs_changes".to_string())),
                    summary: parsed_review
                        .get("summary")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some(format!(
                            "Engine worker reviewed {} pull request #{}.",
                            record.repo_binding.repo_slug, pull_number
                        ))),
                    risk_level: parsed_review
                        .get("risk_level")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some("medium".to_string())),
                    changed_files: parsed_review
                        .get("changed_files")
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                    blockers: parsed_review
                        .get("blockers")
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .filter(|rows| !rows.is_empty())
                        .unwrap_or_else(|| {
                            vec!["Follow-up human review is still recommended.".to_string()]
                        }),
                    requested_changes: parsed_review
                        .get("requested_changes")
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .filter(|rows| !rows.is_empty())
                        .unwrap_or_else(|| {
                            vec![
                                "Validate the constrained change set against broader repo context."
                                    .to_string(),
                            ]
                        }),
                    regression_signals: parsed_review
                        .get("regression_signals")
                        .and_then(Value::as_array)
                        .cloned()
                        .filter(|rows| !rows.is_empty())
                        .unwrap_or_else(|| {
                            vec![json!({
                                "kind": "engine_worker_regression_signal",
                                "summary": "Automated review flagged residual regression risk."
                            })]
                        }),
                    memory_hits_used,
                    notes: Some(format!(
                        "Auto-generated by coder engine worker dispatch. Worker session: {}. Worker artifact: {}.",
                        worker_payload
                            .get("session_id")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown"),
                        worker_artifact.path
                    )),
                }),
            )
            .await?;
            Ok(response.0)
        }
        Some("write_review_artifact") => {
            let memory_hits_used = summarize_workflow_memory_hits(record, &run, "retrieve_memory");
            let worker_payload = load_latest_coder_artifact_payload(
                &state,
                record,
                "coder_pr_review_worker_session",
            )
            .await;
            let parsed_review = worker_payload
                .as_ref()
                .map(parse_pr_review_from_worker_payload);
            let response = coder_pr_review_summary_create(
                State(state),
                Path(record.coder_run_id.clone()),
                Json(CoderPrReviewSummaryCreateInput {
                    verdict: parsed_review
                        .as_ref()
                        .and_then(|payload| payload.get("verdict"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some("needs_changes".to_string())),
                    summary: parsed_review
                        .as_ref()
                        .and_then(|payload| payload.get("summary"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some(format!(
                            "Engine worker completed an initial review pass for {} pull request #{}.",
                            record.repo_binding.repo_slug, pull_number
                        ))),
                    risk_level: parsed_review
                        .as_ref()
                        .and_then(|payload| payload.get("risk_level"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some("medium".to_string())),
                    changed_files: parsed_review
                        .as_ref()
                        .and_then(|payload| payload.get("changed_files"))
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                    blockers: parsed_review
                        .as_ref()
                        .and_then(|payload| payload.get("blockers"))
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .filter(|rows| !rows.is_empty())
                        .unwrap_or_else(|| {
                            vec!["Follow-up human review is still recommended.".to_string()]
                        }),
                    requested_changes: parsed_review
                        .as_ref()
                        .and_then(|payload| payload.get("requested_changes"))
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .filter(|rows| !rows.is_empty())
                        .unwrap_or_else(|| {
                            vec![
                                "Validate the constrained change set against broader repo context."
                                    .to_string(),
                            ]
                        }),
                    regression_signals: parsed_review
                        .as_ref()
                        .and_then(|payload| payload.get("regression_signals"))
                        .and_then(Value::as_array)
                        .cloned()
                        .filter(|rows| !rows.is_empty())
                        .unwrap_or_else(|| {
                            vec![json!({
                                "kind": "engine_worker_regression_signal",
                                "summary": "Automated review flagged residual regression risk."
                            })]
                        }),
                    memory_hits_used,
                    notes: Some(format!(
                        "Auto-generated by coder engine worker dispatch. Review worker artifact available: {}",
                        worker_payload.is_some()
                    )),
                }),
            )
            .await?;
            Ok(response.0)
        }
        _ => Err(StatusCode::CONFLICT),
    }
}

async fn dispatch_merge_recommendation_task(
    state: AppState,
    record: &CoderRunRecord,
    task: &super::context_types::ContextBlackboardTask,
) -> Result<Value, StatusCode> {
    let run = load_context_run_state(&state, &record.linked_context_run_id).await?;
    let pull_number = record
        .github_ref
        .as_ref()
        .map(|row| row.number)
        .unwrap_or_default();
    match task.workflow_node_id.as_deref() {
        Some("inspect_pull_request") => {
            let final_run = advance_coder_workflow_run(
                &state,
                record,
                &["inspect_pull_request"],
                &["assess_merge_readiness"],
                "Pull request inspected; assess merge readiness.",
            )
            .await?;
            Ok(json!({
                "ok": true,
                "run": final_run,
                "coder_run": coder_run_payload(record, &final_run),
                "dispatched": false,
                "reason": "inspect_pull_request advanced through coder workflow progression"
            }))
        }
        Some("assess_merge_readiness") => {
            let memory_hits_used = summarize_workflow_memory_hits(record, &run, "retrieve_memory");
            let (worker_artifact, worker_payload) =
                run_merge_recommendation_worker(&state, record, &run).await?;
            let parsed_merge = parse_merge_recommendation_from_worker_payload(&worker_payload);
            let response = coder_merge_readiness_report_create(
                State(state),
                Path(record.coder_run_id.clone()),
                Json(CoderMergeReadinessReportCreateInput {
                    recommendation: parsed_merge
                        .get("recommendation")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some("hold".to_string())),
                    summary: parsed_merge
                        .get("summary")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some(format!(
                            "Engine worker assessed merge readiness for {} pull request #{}.",
                            record.repo_binding.repo_slug, pull_number
                        ))),
                    risk_level: parsed_merge
                        .get("risk_level")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some("medium".to_string())),
                    blockers: parsed_merge
                        .get("blockers")
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .filter(|rows| !rows.is_empty())
                        .unwrap_or_else(|| {
                            vec!["Follow-up human approval is still required.".to_string()]
                        }),
                    required_checks: parsed_merge
                        .get("required_checks")
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .filter(|rows| !rows.is_empty())
                        .unwrap_or_else(|| vec!["ci / test".to_string()]),
                    required_approvals: parsed_merge
                        .get("required_approvals")
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .filter(|rows| !rows.is_empty())
                        .unwrap_or_else(|| vec!["codeowners".to_string()]),
                    memory_hits_used,
                    notes: Some(format!(
                        "Auto-generated by coder engine worker dispatch. Worker session: {}. Worker artifact: {}.",
                        worker_payload
                            .get("session_id")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown"),
                        worker_artifact.path
                    )),
                }),
            )
            .await?;
            Ok(response.0)
        }
        Some("write_merge_artifact") => {
            let memory_hits_used = summarize_workflow_memory_hits(record, &run, "retrieve_memory");
            let worker_payload = load_latest_coder_artifact_payload(
                &state,
                record,
                "coder_merge_recommendation_worker_session",
            )
            .await;
            let parsed_merge = worker_payload
                .as_ref()
                .map(parse_merge_recommendation_from_worker_payload);
            let response = coder_merge_recommendation_summary_create(
                State(state),
                Path(record.coder_run_id.clone()),
                Json(CoderMergeRecommendationSummaryCreateInput {
                    recommendation: parsed_merge
                        .as_ref()
                        .and_then(|payload| payload.get("recommendation"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some("hold".to_string())),
                    summary: parsed_merge
                        .as_ref()
                        .and_then(|payload| payload.get("summary"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some(format!(
                            "Engine worker completed an initial merge assessment for {} pull request #{}.",
                            record.repo_binding.repo_slug, pull_number
                        ))),
                    risk_level: parsed_merge
                        .as_ref()
                        .and_then(|payload| payload.get("risk_level"))
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                        .or_else(|| Some("medium".to_string())),
                    blockers: parsed_merge
                        .as_ref()
                        .and_then(|payload| payload.get("blockers"))
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .filter(|rows| !rows.is_empty())
                        .unwrap_or_else(|| {
                            vec!["Follow-up human approval is still required.".to_string()]
                        }),
                    required_checks: parsed_merge
                        .as_ref()
                        .and_then(|payload| payload.get("required_checks"))
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .filter(|rows| !rows.is_empty())
                        .unwrap_or_else(|| vec!["ci / test".to_string()]),
                    required_approvals: parsed_merge
                        .as_ref()
                        .and_then(|payload| payload.get("required_approvals"))
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .filter(|rows| !rows.is_empty())
                        .unwrap_or_else(|| vec!["codeowners".to_string()]),
                    memory_hits_used,
                    notes: Some(format!(
                        "Auto-generated by coder engine worker dispatch. Merge worker artifact available: {}",
                        worker_payload.is_some()
                    )),
                }),
            )
            .await?;
            Ok(response.0)
        }
        _ => Err(StatusCode::CONFLICT),
    }
}

async fn write_issue_fix_validation_outputs(
    state: &AppState,
    record: &CoderRunRecord,
    summary: Option<&str>,
    root_cause: Option<&str>,
    fix_strategy: Option<&str>,
    changed_files: &[String],
    validation_steps: &[String],
    validation_results: &[Value],
    memory_hits_used: &[String],
    notes: Option<&str>,
    summary_artifact_path: Option<&str>,
) -> Result<(Option<ContextBlackboardArtifact>, Vec<Value>), StatusCode> {
    if validation_steps.is_empty() && validation_results.is_empty() {
        return Ok((None, Vec::new()));
    }
    let validation_id = format!("issue-fix-validation-{}", Uuid::new_v4().simple());
    let validation_payload = json!({
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "summary": summary,
        "root_cause": root_cause,
        "fix_strategy": fix_strategy,
        "changed_files": changed_files,
        "validation_steps": validation_steps,
        "validation_results": validation_results,
        "memory_hits_used": memory_hits_used,
        "notes": notes,
        "summary_artifact_path": summary_artifact_path,
        "created_at_ms": crate::now_ms(),
    });
    let validation_artifact = write_coder_artifact(
        state,
        &record.linked_context_run_id,
        &validation_id,
        "coder_validation_report",
        "artifacts/issue_fix.validation.json",
        &validation_payload,
    )
    .await?;
    publish_coder_artifact_added(state, record, &validation_artifact, Some("validation"), {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("validation_report"));
        extra.insert("workflow_mode".to_string(), json!("issue_fix"));
        extra
    });

    let validation_summary = validation_results
        .iter()
        .filter_map(|row| {
            row.get("summary")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
        })
        .next()
        .or_else(|| {
            (!validation_steps.is_empty())
                .then(|| format!("Validation attempted: {}", validation_steps.join(", ")))
        })
        .unwrap_or_else(|| "Validation evidence captured for issue fix.".to_string());
    let mut generated_candidates = Vec::<Value>::new();
    let (validation_memory_id, validation_memory_artifact) = write_coder_memory_candidate_artifact(
        state,
        record,
        CoderMemoryCandidateKind::ValidationMemory,
        Some(validation_summary),
        Some("validate_fix".to_string()),
        json!({
            "workflow_mode": "issue_fix",
            "summary": summary,
            "root_cause": root_cause,
            "fix_strategy": fix_strategy,
            "changed_files": changed_files,
            "validation_steps": validation_steps,
            "validation_results": validation_results,
            "memory_hits_used": memory_hits_used,
            "notes": notes,
            "summary_artifact_path": summary_artifact_path,
            "validation_artifact_path": validation_artifact.path,
        }),
    )
    .await?;
    generated_candidates.push(json!({
        "candidate_id": validation_memory_id,
        "kind": "validation_memory",
        "artifact_path": validation_memory_artifact.path,
    }));
    Ok((Some(validation_artifact), generated_candidates))
}

fn coder_event_base(record: &CoderRunRecord) -> serde_json::Map<String, Value> {
    let mut payload = serde_json::Map::new();
    payload.insert("coder_run_id".to_string(), json!(record.coder_run_id));
    payload.insert(
        "linked_context_run_id".to_string(),
        json!(record.linked_context_run_id),
    );
    payload.insert("workflow_mode".to_string(), json!(record.workflow_mode));
    payload.insert("repo_binding".to_string(), json!(record.repo_binding));
    payload.insert("github_ref".to_string(), json!(record.github_ref));
    if let Some(source_client) = record
        .source_client
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        payload.insert("source_client".to_string(), json!(source_client));
    }
    payload
}

fn coder_artifact_event_fields(
    artifact: &ContextBlackboardArtifact,
    kind: Option<&str>,
) -> serde_json::Map<String, Value> {
    let mut payload = serde_json::Map::new();
    payload.insert("artifact_id".to_string(), json!(artifact.id));
    payload.insert("artifact_type".to_string(), json!(artifact.artifact_type));
    payload.insert("artifact_path".to_string(), json!(artifact.path));
    if let Some(kind) = kind.map(str::trim).filter(|value| !value.is_empty()) {
        payload.insert("kind".to_string(), json!(kind));
    }
    payload
}

fn publish_coder_run_event(
    state: &AppState,
    event_type: &str,
    record: &CoderRunRecord,
    phase: Option<&str>,
    extra: serde_json::Map<String, Value>,
) {
    let mut payload = coder_event_base(record);
    if let Some(phase) = phase {
        payload.insert("phase".to_string(), json!(phase));
    }
    payload.extend(extra);
    state
        .event_bus
        .publish(EngineEvent::new(event_type, Value::Object(payload)));
}

fn publish_coder_artifact_added(
    state: &AppState,
    record: &CoderRunRecord,
    artifact: &ContextBlackboardArtifact,
    phase: Option<&str>,
    extra: serde_json::Map<String, Value>,
) {
    let kind = extra
        .get("kind")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let mut payload = coder_artifact_event_fields(artifact, kind.as_deref());
    payload.extend(extra);
    publish_coder_run_event(state, "coder.artifact.added", record, phase, payload);
}

async fn coder_issue_triage_readiness(
    state: &AppState,
    input: &CoderRunCreateInput,
) -> Result<CapabilityReadinessOutput, StatusCode> {
    let mut readiness = super::capabilities::evaluate_capability_readiness(
        state,
        &CapabilityReadinessInput {
            workflow_id: Some("coder_issue_triage".to_string()),
            required_capabilities: vec![
                "github.list_issues".to_string(),
                "github.get_issue".to_string(),
            ],
            optional_capabilities: Vec::new(),
            provider_preference: input
                .mcp_servers
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|row| row.to_ascii_lowercase())
                .collect(),
            available_tools: Vec::new(),
            allow_unbound: false,
        },
    )
    .await?;
    let mcp_servers = state.mcp.list().await;
    let enabled_servers = mcp_servers
        .values()
        .filter(|server| server.enabled)
        .collect::<Vec<_>>();
    let connected_servers = enabled_servers
        .iter()
        .filter(|server| server.connected)
        .map(|server| server.name.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    let preferred_servers = input
        .mcp_servers
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|row| row.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let mut missing_preferred = Vec::new();
    let mut disconnected_preferred = Vec::new();
    for provider in preferred_servers {
        let any_enabled = enabled_servers
            .iter()
            .any(|server| server.name.eq_ignore_ascii_case(&provider));
        if !any_enabled {
            missing_preferred.push(provider.clone());
            continue;
        }
        if !connected_servers.contains(&provider) {
            disconnected_preferred.push(provider);
        }
    }
    if !missing_preferred.is_empty() {
        readiness.blocking_issues.push(CapabilityBlockingIssue {
            code: "missing_mcp_servers".to_string(),
            message: "Preferred MCP servers are not configured.".to_string(),
            capability_ids: Vec::new(),
            providers: missing_preferred.clone(),
            tools: Vec::new(),
        });
        readiness.missing_servers.extend(missing_preferred);
    }
    if !disconnected_preferred.is_empty() {
        readiness.blocking_issues.push(CapabilityBlockingIssue {
            code: "disconnected_mcp_servers".to_string(),
            message: "Preferred MCP servers are configured but disconnected.".to_string(),
            capability_ids: Vec::new(),
            providers: disconnected_preferred.clone(),
            tools: Vec::new(),
        });
        readiness
            .disconnected_servers
            .extend(disconnected_preferred);
    }
    readiness.missing_servers.sort();
    readiness.missing_servers.dedup();
    readiness.disconnected_servers.sort();
    readiness.disconnected_servers.dedup();
    readiness.runnable = readiness.blocking_issues.is_empty();
    Ok(readiness)
}

async fn coder_pr_review_readiness(
    state: &AppState,
    input: &CoderRunCreateInput,
) -> Result<CapabilityReadinessOutput, StatusCode> {
    let mut readiness = super::capabilities::evaluate_capability_readiness(
        state,
        &CapabilityReadinessInput {
            workflow_id: Some("coder_pr_review".to_string()),
            required_capabilities: vec![
                "github.list_pull_requests".to_string(),
                "github.get_pull_request".to_string(),
            ],
            optional_capabilities: vec!["github.comment_on_pull_request".to_string()],
            provider_preference: input
                .mcp_servers
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|row| row.to_ascii_lowercase())
                .collect(),
            available_tools: Vec::new(),
            allow_unbound: false,
        },
    )
    .await?;
    let mcp_servers = state.mcp.list().await;
    let enabled_servers = mcp_servers
        .values()
        .filter(|server| server.enabled)
        .collect::<Vec<_>>();
    let connected_servers = enabled_servers
        .iter()
        .filter(|server| server.connected)
        .map(|server| server.name.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let preferred_servers = input
        .mcp_servers
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|row| row.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let mut missing_preferred = Vec::new();
    let mut disconnected_preferred = Vec::new();
    for provider in preferred_servers {
        let any_enabled = enabled_servers
            .iter()
            .any(|server| server.name.eq_ignore_ascii_case(&provider));
        if !any_enabled {
            missing_preferred.push(provider.clone());
            continue;
        }
        if !connected_servers.contains(&provider) {
            disconnected_preferred.push(provider);
        }
    }
    if !missing_preferred.is_empty() {
        readiness.blocking_issues.push(CapabilityBlockingIssue {
            code: "missing_mcp_servers".to_string(),
            message: "Preferred MCP servers are not configured.".to_string(),
            capability_ids: Vec::new(),
            providers: missing_preferred.clone(),
            tools: Vec::new(),
        });
        readiness.missing_servers.extend(missing_preferred);
    }
    if !disconnected_preferred.is_empty() {
        readiness.blocking_issues.push(CapabilityBlockingIssue {
            code: "disconnected_mcp_servers".to_string(),
            message: "Preferred MCP servers are configured but disconnected.".to_string(),
            capability_ids: Vec::new(),
            providers: disconnected_preferred.clone(),
            tools: Vec::new(),
        });
        readiness
            .disconnected_servers
            .extend(disconnected_preferred);
    }
    readiness.missing_servers.sort();
    readiness.missing_servers.dedup();
    readiness.disconnected_servers.sort();
    readiness.disconnected_servers.dedup();
    readiness.runnable = readiness.blocking_issues.is_empty();
    Ok(readiness)
}

async fn coder_merge_recommendation_readiness(
    state: &AppState,
    input: &CoderRunCreateInput,
) -> Result<CapabilityReadinessOutput, StatusCode> {
    let mut readiness = super::capabilities::evaluate_capability_readiness(
        state,
        &CapabilityReadinessInput {
            workflow_id: Some("coder_merge_recommendation".to_string()),
            required_capabilities: vec![
                "github.list_pull_requests".to_string(),
                "github.get_pull_request".to_string(),
            ],
            optional_capabilities: vec!["github.comment_on_pull_request".to_string()],
            provider_preference: input
                .mcp_servers
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|row| row.to_ascii_lowercase())
                .collect(),
            available_tools: Vec::new(),
            allow_unbound: false,
        },
    )
    .await?;
    let mcp_servers = state.mcp.list().await;
    let enabled_servers = mcp_servers
        .values()
        .filter(|server| server.enabled)
        .collect::<Vec<_>>();
    let connected_servers = enabled_servers
        .iter()
        .filter(|server| server.connected)
        .map(|server| server.name.to_ascii_lowercase())
        .collect::<HashSet<_>>();
    let preferred_servers = input
        .mcp_servers
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|row| row.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let mut missing_preferred = Vec::new();
    let mut disconnected_preferred = Vec::new();
    for provider in preferred_servers {
        let any_enabled = enabled_servers
            .iter()
            .any(|server| server.name.eq_ignore_ascii_case(&provider));
        if !any_enabled {
            missing_preferred.push(provider.clone());
            continue;
        }
        if !connected_servers.contains(&provider) {
            disconnected_preferred.push(provider);
        }
    }
    if !missing_preferred.is_empty() {
        readiness.blocking_issues.push(CapabilityBlockingIssue {
            code: "missing_mcp_servers".to_string(),
            message: "Preferred MCP servers are not configured.".to_string(),
            capability_ids: Vec::new(),
            providers: missing_preferred.clone(),
            tools: Vec::new(),
        });
        readiness.missing_servers.extend(missing_preferred);
    }
    if !disconnected_preferred.is_empty() {
        readiness.blocking_issues.push(CapabilityBlockingIssue {
            code: "disconnected_mcp_servers".to_string(),
            message: "Preferred MCP servers are configured but disconnected.".to_string(),
            capability_ids: Vec::new(),
            providers: disconnected_preferred.clone(),
            tools: Vec::new(),
        });
        readiness
            .disconnected_servers
            .extend(disconnected_preferred);
    }
    readiness.missing_servers.sort();
    readiness.missing_servers.dedup();
    readiness.disconnected_servers.sort();
    readiness.disconnected_servers.dedup();
    readiness.runnable = readiness.blocking_issues.is_empty();
    Ok(readiness)
}

async fn coder_pr_submit_readiness(
    state: &AppState,
    preferred_server: Option<&str>,
) -> Result<CapabilityReadinessOutput, StatusCode> {
    let provider_preference = preferred_server
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| vec![value.to_ascii_lowercase()])
        .unwrap_or_default();
    let mut readiness = super::capabilities::evaluate_capability_readiness(
        state,
        &CapabilityReadinessInput {
            workflow_id: Some("coder_issue_fix_pr_submit".to_string()),
            required_capabilities: vec!["github.create_pull_request".to_string()],
            optional_capabilities: Vec::new(),
            provider_preference,
            available_tools: Vec::new(),
            allow_unbound: false,
        },
    )
    .await?;
    if let Some(server_name) = preferred_server
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
    {
        let servers = state.mcp.list().await;
        match servers
            .values()
            .find(|server| server.name.eq_ignore_ascii_case(&server_name))
        {
            None => {
                readiness.blocking_issues.push(CapabilityBlockingIssue {
                    code: "missing_mcp_servers".to_string(),
                    message: "Preferred MCP server is not configured.".to_string(),
                    capability_ids: Vec::new(),
                    providers: vec![server_name.clone()],
                    tools: Vec::new(),
                });
                readiness.missing_servers.push(server_name);
            }
            Some(server) if !server.connected => {
                readiness.blocking_issues.push(CapabilityBlockingIssue {
                    code: "disconnected_mcp_servers".to_string(),
                    message: "Preferred MCP server is configured but disconnected.".to_string(),
                    capability_ids: Vec::new(),
                    providers: vec![server.name.to_ascii_lowercase()],
                    tools: Vec::new(),
                });
                readiness
                    .disconnected_servers
                    .push(server.name.to_ascii_lowercase());
            }
            Some(_) => {}
        }
    }
    readiness.missing_servers.sort();
    readiness.missing_servers.dedup();
    readiness.disconnected_servers.sort();
    readiness.disconnected_servers.dedup();
    readiness.runnable = readiness.blocking_issues.is_empty();
    Ok(readiness)
}

fn compose_issue_triage_objective(input: &CoderRunCreateInput) -> String {
    if let Some(objective) = input
        .objective
        .as_deref()
        .map(str::trim)
        .filter(|row| !row.is_empty())
    {
        return objective.to_string();
    }
    match input.github_ref.as_ref() {
        Some(reference) if matches!(reference.kind, CoderGithubRefKind::Issue) => format!(
            "Triage GitHub issue #{} for {}",
            reference.number, input.repo_binding.repo_slug
        ),
        Some(reference) => format!(
            "Start {:?} workflow for #{} in {}",
            reference.kind, reference.number, input.repo_binding.repo_slug
        ),
        None => format!(
            "Start {:?} workflow for {}",
            input.workflow_mode, input.repo_binding.repo_slug
        ),
    }
}

fn compose_pr_review_objective(input: &CoderRunCreateInput) -> String {
    if let Some(objective) = input
        .objective
        .as_deref()
        .map(str::trim)
        .filter(|row| !row.is_empty())
    {
        return objective.to_string();
    }
    match input.github_ref.as_ref() {
        Some(reference) if matches!(reference.kind, CoderGithubRefKind::PullRequest) => format!(
            "Review GitHub pull request #{} for {}",
            reference.number, input.repo_binding.repo_slug
        ),
        Some(reference) => format!(
            "Start {:?} workflow for #{} in {}",
            reference.kind, reference.number, input.repo_binding.repo_slug
        ),
        None => format!(
            "Review pull request activity for {}",
            input.repo_binding.repo_slug
        ),
    }
}

fn compose_issue_fix_objective(input: &CoderRunCreateInput) -> String {
    if let Some(objective) = input
        .objective
        .as_deref()
        .map(str::trim)
        .filter(|row| !row.is_empty())
    {
        return objective.to_string();
    }
    match input.github_ref.as_ref() {
        Some(reference) if matches!(reference.kind, CoderGithubRefKind::Issue) => format!(
            "Prepare a fix for GitHub issue #{} in {}",
            reference.number, input.repo_binding.repo_slug
        ),
        Some(reference) => format!(
            "Start {:?} workflow for #{} in {}",
            reference.kind, reference.number, input.repo_binding.repo_slug
        ),
        None => format!("Prepare an issue fix for {}", input.repo_binding.repo_slug),
    }
}

fn compose_merge_recommendation_objective(input: &CoderRunCreateInput) -> String {
    if let Some(objective) = input
        .objective
        .as_deref()
        .map(str::trim)
        .filter(|row| !row.is_empty())
    {
        return objective.to_string();
    }
    match input.github_ref.as_ref() {
        Some(reference) if matches!(reference.kind, CoderGithubRefKind::PullRequest) => format!(
            "Prepare merge recommendation for GitHub pull request #{} in {}",
            reference.number, input.repo_binding.repo_slug
        ),
        Some(reference) => format!(
            "Start {:?} workflow for #{} in {}",
            reference.kind, reference.number, input.repo_binding.repo_slug
        ),
        None => format!(
            "Prepare merge recommendation for {}",
            input.repo_binding.repo_slug
        ),
    }
}

fn derive_workspace(input: &CoderRunCreateInput) -> ContextWorkspaceLease {
    input.workspace.clone().unwrap_or(ContextWorkspaceLease {
        workspace_id: input.repo_binding.workspace_id.clone(),
        canonical_path: input.repo_binding.workspace_root.clone(),
        lease_epoch: crate::now_ms(),
    })
}

async fn seed_issue_triage_tasks(
    state: AppState,
    coder_run: &CoderRunRecord,
) -> Result<(), StatusCode> {
    let run_id = coder_run.linked_context_run_id.clone();
    let issue_number = coder_run.github_ref.as_ref().map(|row| row.number);
    let workflow_id = "coder_issue_triage".to_string();
    let retrieval_query = format!(
        "{} issue #{}",
        coder_run.repo_binding.repo_slug,
        issue_number.unwrap_or_default()
    );
    let memory_hits =
        collect_issue_triage_memory_hits(&state, coder_run, &retrieval_query, 6).await?;
    let tasks = vec![
        ContextTaskCreateInput {
            command_id: Some(format!("coder:{run_id}:ingest_reference")),
            id: Some(format!("triage-ingest-{}", Uuid::new_v4().simple())),
            task_type: "inspection".to_string(),
            payload: json!({
                "task_kind": "inspection",
                "title": "Normalize issue or failure reference",
                "repo_slug": coder_run.repo_binding.repo_slug,
                "github_ref": coder_run.github_ref,
            }),
            status: Some(ContextBlackboardTaskStatus::Runnable),
            workflow_id: Some(workflow_id.clone()),
            workflow_node_id: Some("ingest_reference".to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            priority: Some(20),
            max_attempts: Some(1),
        },
        ContextTaskCreateInput {
            command_id: Some(format!("coder:{run_id}:retrieve_memory")),
            id: Some(format!("triage-memory-{}", Uuid::new_v4().simple())),
            task_type: "research".to_string(),
            payload: json!({
                "task_kind": "research",
                "title": "Retrieve similar failures and prior triage memory",
                "repo_slug": coder_run.repo_binding.repo_slug,
                "github_issue_number": issue_number,
                "memory_recipe": "issue_triage",
                "memory_hits": memory_hits,
            }),
            status: Some(ContextBlackboardTaskStatus::Pending),
            workflow_id: Some(workflow_id.clone()),
            workflow_node_id: Some("retrieve_memory".to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            priority: Some(18),
            max_attempts: Some(2),
        },
        ContextTaskCreateInput {
            command_id: Some(format!("coder:{run_id}:inspect_repo")),
            id: Some(format!("triage-inspect-{}", Uuid::new_v4().simple())),
            task_type: "inspection".to_string(),
            payload: json!({
                "task_kind": "inspection",
                "title": "Inspect likely affected repo areas",
                "repo_slug": coder_run.repo_binding.repo_slug,
                "project_id": coder_run.repo_binding.project_id,
            }),
            status: Some(ContextBlackboardTaskStatus::Pending),
            workflow_id: Some(workflow_id.clone()),
            workflow_node_id: Some("inspect_repo".to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            priority: Some(16),
            max_attempts: Some(2),
        },
        ContextTaskCreateInput {
            command_id: Some(format!("coder:{run_id}:attempt_reproduction")),
            id: Some(format!("triage-repro-{}", Uuid::new_v4().simple())),
            task_type: "validation".to_string(),
            payload: json!({
                "task_kind": "validation",
                "title": "Attempt constrained reproduction",
                "repo_slug": coder_run.repo_binding.repo_slug,
                "github_issue_number": issue_number
            }),
            status: Some(ContextBlackboardTaskStatus::Pending),
            workflow_id: Some(workflow_id.clone()),
            workflow_node_id: Some("attempt_reproduction".to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            priority: Some(14),
            max_attempts: Some(2),
        },
        ContextTaskCreateInput {
            command_id: Some(format!("coder:{run_id}:write_triage_artifact")),
            id: Some(format!("triage-artifact-{}", Uuid::new_v4().simple())),
            task_type: "implementation".to_string(),
            payload: json!({
                "task_kind": "implementation",
                "title": "Write triage artifact and memory candidates",
                "repo_slug": coder_run.repo_binding.repo_slug,
                "output_target": {
                    "path": format!("artifacts/{run_id}/triage.summary.json"),
                    "kind": "artifact",
                    "operation": "write"
                }
            }),
            status: Some(ContextBlackboardTaskStatus::Pending),
            workflow_id: Some(workflow_id),
            workflow_node_id: Some("write_triage_artifact".to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            priority: Some(10),
            max_attempts: Some(1),
        },
    ];
    context_run_tasks_create(
        State(state),
        Path(run_id),
        Json(ContextTaskCreateBatchInput { tasks }),
    )
    .await
    .map(|_| ())
}

async fn seed_pr_review_tasks(
    state: AppState,
    coder_run: &CoderRunRecord,
) -> Result<(), StatusCode> {
    let run_id = coder_run.linked_context_run_id.clone();
    let workflow_id = "coder_pr_review".to_string();
    let retrieval_query = default_coder_memory_query(coder_run);
    let memory_hits =
        collect_issue_triage_memory_hits(&state, coder_run, &retrieval_query, 6).await?;
    let tasks = vec![
        ContextTaskCreateInput {
            command_id: Some(format!("coder:{run_id}:inspect_pull_request")),
            id: Some(format!("review-inspect-{}", Uuid::new_v4().simple())),
            task_type: "inspection".to_string(),
            payload: json!({
                "task_kind": "inspection",
                "title": "Inspect pull request metadata and changed files",
                "repo_slug": coder_run.repo_binding.repo_slug,
                "github_ref": coder_run.github_ref,
            }),
            status: Some(ContextBlackboardTaskStatus::Runnable),
            workflow_id: Some(workflow_id.clone()),
            workflow_node_id: Some("inspect_pull_request".to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            priority: Some(18),
            max_attempts: Some(1),
        },
        ContextTaskCreateInput {
            command_id: Some(format!("coder:{run_id}:retrieve_memory")),
            id: Some(format!("review-memory-{}", Uuid::new_v4().simple())),
            task_type: "research".to_string(),
            payload: json!({
                "task_kind": "research",
                "title": "Retrieve regression and review memory",
                "memory_recipe": "pr_review",
                "repo_slug": coder_run.repo_binding.repo_slug,
                "github_ref": coder_run.github_ref,
                "memory_hits": memory_hits,
            }),
            status: Some(ContextBlackboardTaskStatus::Pending),
            workflow_id: Some(workflow_id.clone()),
            workflow_node_id: Some("retrieve_memory".to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            priority: Some(16),
            max_attempts: Some(2),
        },
        ContextTaskCreateInput {
            command_id: Some(format!("coder:{run_id}:review_pull_request")),
            id: Some(format!("review-analyze-{}", Uuid::new_v4().simple())),
            task_type: "analysis".to_string(),
            payload: json!({
                "task_kind": "analysis",
                "title": "Review risk, regressions, and missing coverage",
                "repo_slug": coder_run.repo_binding.repo_slug,
                "github_ref": coder_run.github_ref,
            }),
            status: Some(ContextBlackboardTaskStatus::Pending),
            workflow_id: Some(workflow_id.clone()),
            workflow_node_id: Some("review_pull_request".to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            priority: Some(14),
            max_attempts: Some(2),
        },
        ContextTaskCreateInput {
            command_id: Some(format!("coder:{run_id}:write_review_artifact")),
            id: Some(format!("review-artifact-{}", Uuid::new_v4().simple())),
            task_type: "implementation".to_string(),
            payload: json!({
                "task_kind": "implementation",
                "title": "Write structured PR review artifact",
                "artifact_type": "coder_pr_review_summary",
                "repo_slug": coder_run.repo_binding.repo_slug,
                "github_ref": coder_run.github_ref,
            }),
            status: Some(ContextBlackboardTaskStatus::Pending),
            workflow_id: Some(workflow_id),
            workflow_node_id: Some("write_review_artifact".to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            priority: Some(12),
            max_attempts: Some(2),
        },
    ];
    context_run_tasks_create(
        State(state),
        Path(run_id),
        Json(ContextTaskCreateBatchInput { tasks }),
    )
    .await
    .map(|_| ())
}

async fn seed_issue_fix_tasks(
    state: AppState,
    coder_run: &CoderRunRecord,
) -> Result<(), StatusCode> {
    let run_id = coder_run.linked_context_run_id.clone();
    let workflow_id = "coder_issue_fix".to_string();
    let retrieval_query = default_coder_memory_query(coder_run);
    let memory_hits =
        collect_issue_triage_memory_hits(&state, coder_run, &retrieval_query, 6).await?;
    let issue_number = coder_run.github_ref.as_ref().map(|row| row.number);
    let tasks = vec![
        ContextTaskCreateInput {
            command_id: Some(format!("coder:{run_id}:inspect_issue_context")),
            id: Some(format!("fix-inspect-{}", Uuid::new_v4().simple())),
            task_type: "inspection".to_string(),
            payload: json!({
                "task_kind": "inspection",
                "title": "Inspect issue context and likely affected files",
                "repo_slug": coder_run.repo_binding.repo_slug,
                "github_ref": coder_run.github_ref,
            }),
            status: Some(ContextBlackboardTaskStatus::Runnable),
            workflow_id: Some(workflow_id.clone()),
            workflow_node_id: Some("inspect_issue_context".to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            priority: Some(20),
            max_attempts: Some(1),
        },
        ContextTaskCreateInput {
            command_id: Some(format!("coder:{run_id}:retrieve_memory")),
            id: Some(format!("fix-memory-{}", Uuid::new_v4().simple())),
            task_type: "research".to_string(),
            payload: json!({
                "task_kind": "research",
                "title": "Retrieve prior triage, fix, and validation memory",
                "memory_recipe": "issue_fix",
                "repo_slug": coder_run.repo_binding.repo_slug,
                "github_issue_number": issue_number,
                "memory_hits": memory_hits,
            }),
            status: Some(ContextBlackboardTaskStatus::Pending),
            workflow_id: Some(workflow_id.clone()),
            workflow_node_id: Some("retrieve_memory".to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            priority: Some(18),
            max_attempts: Some(2),
        },
        ContextTaskCreateInput {
            command_id: Some(format!("coder:{run_id}:prepare_fix")),
            id: Some(format!("fix-prepare-{}", Uuid::new_v4().simple())),
            task_type: "research".to_string(),
            payload: json!({
                "task_kind": "research",
                "title": "Prepare constrained fix plan and code changes",
                "repo_slug": coder_run.repo_binding.repo_slug,
                "github_issue_number": issue_number,
            }),
            status: Some(ContextBlackboardTaskStatus::Pending),
            workflow_id: Some(workflow_id.clone()),
            workflow_node_id: Some("prepare_fix".to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            priority: Some(16),
            max_attempts: Some(2),
        },
        ContextTaskCreateInput {
            command_id: Some(format!("coder:{run_id}:validate_fix")),
            id: Some(format!("fix-validate-{}", Uuid::new_v4().simple())),
            task_type: "validation".to_string(),
            payload: json!({
                "task_kind": "validation",
                "title": "Run targeted validation for the proposed fix",
                "repo_slug": coder_run.repo_binding.repo_slug,
                "github_issue_number": issue_number,
            }),
            status: Some(ContextBlackboardTaskStatus::Pending),
            workflow_id: Some(workflow_id.clone()),
            workflow_node_id: Some("validate_fix".to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            priority: Some(14),
            max_attempts: Some(2),
        },
        ContextTaskCreateInput {
            command_id: Some(format!("coder:{run_id}:write_fix_artifact")),
            id: Some(format!("fix-artifact-{}", Uuid::new_v4().simple())),
            task_type: "implementation".to_string(),
            payload: json!({
                "task_kind": "implementation",
                "title": "Write structured fix summary artifact",
                "artifact_type": "coder_issue_fix_summary",
                "repo_slug": coder_run.repo_binding.repo_slug,
                "github_ref": coder_run.github_ref,
                "output_target": {
                    "path": format!("artifacts/{run_id}/issue_fix.summary.json"),
                    "kind": "artifact",
                    "operation": "write"
                }
            }),
            status: Some(ContextBlackboardTaskStatus::Pending),
            workflow_id: Some(workflow_id),
            workflow_node_id: Some("write_fix_artifact".to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            priority: Some(12),
            max_attempts: Some(2),
        },
    ];
    context_run_tasks_create(
        State(state),
        Path(run_id),
        Json(ContextTaskCreateBatchInput { tasks }),
    )
    .await
    .map(|_| ())
}

async fn seed_merge_recommendation_tasks(
    state: AppState,
    coder_run: &CoderRunRecord,
) -> Result<(), StatusCode> {
    let run_id = coder_run.linked_context_run_id.clone();
    let workflow_id = "coder_merge_recommendation".to_string();
    let retrieval_query = default_coder_memory_query(coder_run);
    let memory_hits =
        collect_issue_triage_memory_hits(&state, coder_run, &retrieval_query, 6).await?;
    let tasks = vec![
        ContextTaskCreateInput {
            command_id: Some(format!("coder:{run_id}:inspect_pull_request")),
            id: Some(format!("merge-inspect-{}", Uuid::new_v4().simple())),
            task_type: "inspection".to_string(),
            payload: json!({
                "task_kind": "inspection",
                "title": "Inspect pull request state and review status",
                "repo_slug": coder_run.repo_binding.repo_slug,
                "github_ref": coder_run.github_ref,
            }),
            status: Some(ContextBlackboardTaskStatus::Runnable),
            workflow_id: Some(workflow_id.clone()),
            workflow_node_id: Some("inspect_pull_request".to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            priority: Some(18),
            max_attempts: Some(1),
        },
        ContextTaskCreateInput {
            command_id: Some(format!("coder:{run_id}:retrieve_memory")),
            id: Some(format!("merge-memory-{}", Uuid::new_v4().simple())),
            task_type: "research".to_string(),
            payload: json!({
                "task_kind": "research",
                "title": "Retrieve merge and regression memory",
                "memory_recipe": "merge_recommendation",
                "repo_slug": coder_run.repo_binding.repo_slug,
                "github_ref": coder_run.github_ref,
                "memory_hits": memory_hits,
            }),
            status: Some(ContextBlackboardTaskStatus::Pending),
            workflow_id: Some(workflow_id.clone()),
            workflow_node_id: Some("retrieve_memory".to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            priority: Some(16),
            max_attempts: Some(2),
        },
        ContextTaskCreateInput {
            command_id: Some(format!("coder:{run_id}:assess_merge_readiness")),
            id: Some(format!("merge-assess-{}", Uuid::new_v4().simple())),
            task_type: "analysis".to_string(),
            payload: json!({
                "task_kind": "analysis",
                "title": "Assess merge readiness, blockers, and residual risk",
                "repo_slug": coder_run.repo_binding.repo_slug,
                "github_ref": coder_run.github_ref,
            }),
            status: Some(ContextBlackboardTaskStatus::Pending),
            workflow_id: Some(workflow_id.clone()),
            workflow_node_id: Some("assess_merge_readiness".to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            priority: Some(14),
            max_attempts: Some(2),
        },
        ContextTaskCreateInput {
            command_id: Some(format!("coder:{run_id}:write_merge_artifact")),
            id: Some(format!("merge-artifact-{}", Uuid::new_v4().simple())),
            task_type: "implementation".to_string(),
            payload: json!({
                "task_kind": "implementation",
                "title": "Write structured merge recommendation artifact",
                "artifact_type": "coder_merge_recommendation_summary",
                "repo_slug": coder_run.repo_binding.repo_slug,
                "github_ref": coder_run.github_ref,
            }),
            status: Some(ContextBlackboardTaskStatus::Pending),
            workflow_id: Some(workflow_id),
            workflow_node_id: Some("write_merge_artifact".to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            priority: Some(12),
            max_attempts: Some(2),
        },
    ];
    context_run_tasks_create(
        State(state),
        Path(run_id),
        Json(ContextTaskCreateBatchInput { tasks }),
    )
    .await
    .map(|_| ())
}

fn normalize_source_client(input: Option<&str>) -> Option<String> {
    input
        .map(str::trim)
        .filter(|row| !row.is_empty())
        .map(ToString::to_string)
}

async fn resolve_coder_worker_model_spec(
    state: &AppState,
    record: &CoderRunRecord,
) -> Option<tandem_types::ModelSpec> {
    if let (Some(provider_id), Some(model_id)) = (
        normalize_source_client(record.model_provider.as_deref()),
        normalize_source_client(record.model_id.as_deref()),
    ) {
        return Some(tandem_types::ModelSpec {
            provider_id,
            model_id,
        });
    }

    let effective_config = state.config.get_effective_value().await;
    if let Some(spec) = crate::default_model_spec_from_effective_config(&effective_config) {
        return Some(spec);
    }

    state
        .providers
        .list()
        .await
        .into_iter()
        .find_map(|provider| {
            provider
                .models
                .first()
                .map(|model| tandem_types::ModelSpec {
                    provider_id: provider.id.clone(),
                    model_id: model.id.clone(),
                })
        })
}

fn compact_session_messages(session: &Session) -> Vec<Value> {
    session
        .messages
        .iter()
        .map(|message| {
            let parts = message
                .parts
                .iter()
                .map(|part| match part {
                    MessagePart::Text { text } => json!({
                        "type": "text",
                        "text": crate::truncate_text(text, 500),
                    }),
                    MessagePart::Reasoning { text } => json!({
                        "type": "reasoning",
                        "text": crate::truncate_text(text, 500),
                    }),
                    MessagePart::ToolInvocation {
                        tool,
                        args,
                        result,
                        error,
                    } => json!({
                        "type": "tool_invocation",
                        "tool": tool,
                        "args": args,
                        "result": result,
                        "error": error,
                    }),
                })
                .collect::<Vec<_>>();
            json!({
                "id": message.id,
                "role": message.role,
                "parts": parts,
                "created_at": message.created_at,
            })
        })
        .collect()
}

fn latest_assistant_session_text(session: &Session) -> Option<String> {
    session.messages.iter().rev().find_map(|message| {
        if !matches!(message.role, MessageRole::Assistant) {
            return None;
        }
        message.parts.iter().rev().find_map(|part| match part {
            MessagePart::Text { text } | MessagePart::Reasoning { text } => Some(text.clone()),
            _ => None,
        })
    })
}

fn count_session_tool_invocations(session: &Session) -> usize {
    session
        .messages
        .iter()
        .flat_map(|message| message.parts.iter())
        .filter(|part| matches!(part, MessagePart::ToolInvocation { .. }))
        .count()
}

fn normalize_changed_file_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.replace('\\', "/"))
}

fn change_preview_from_value(value: Option<&Value>) -> Option<String> {
    let text = value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    Some(crate::truncate_text(text, 240))
}

fn change_preview_from_bytes(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }
    let excerpt = String::from_utf8_lossy(&bytes[..bytes.len().min(1_200)]);
    let trimmed = excerpt.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(crate::truncate_text(trimmed, 240))
}

fn extract_changed_files_from_value(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::String(text) => {
            if let Some(path) = normalize_changed_file_path(text) {
                out.insert(path);
            }
        }
        Value::Array(rows) => {
            for row in rows {
                extract_changed_files_from_value(row, out);
            }
        }
        Value::Object(map) => {
            for key in ["path", "file", "target_file", "target", "destination"] {
                if let Some(value) = map.get(key) {
                    extract_changed_files_from_value(value, out);
                }
            }
            if let Some(value) = map.get("files") {
                extract_changed_files_from_value(value, out);
            }
        }
        _ => {}
    }
}

fn extract_session_change_evidence(session: &Session) -> Vec<Value> {
    let mut out = Vec::<Value>::new();
    let mut seen = BTreeSet::<String>::new();
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool, args, result, ..
            } = part
            else {
                continue;
            };
            let normalized_tool = tool.trim().to_ascii_lowercase();
            if matches!(
                normalized_tool.as_str(),
                "write" | "edit" | "patch" | "apply_patch" | "str_replace"
            ) {
                let mut paths = BTreeSet::<String>::new();
                extract_changed_files_from_value(args, &mut paths);
                if let Some(result) = result {
                    extract_changed_files_from_value(result, &mut paths);
                }
                for path in paths {
                    if !seen.insert(format!("{normalized_tool}:{path}")) {
                        continue;
                    }
                    let preview = if normalized_tool == "write" {
                        change_preview_from_value(args.get("content"))
                    } else if matches!(normalized_tool.as_str(), "edit" | "str_replace") {
                        change_preview_from_value(args.get("new_string"))
                            .or_else(|| change_preview_from_value(args.get("replacement")))
                    } else {
                        change_preview_from_value(args.get("patch"))
                            .or_else(|| change_preview_from_value(args.get("diff")))
                    };
                    out.push(json!({
                        "path": path,
                        "tool": normalized_tool,
                        "preview": preview,
                        "has_result": result.is_some(),
                    }));
                }
            }
        }
    }
    out
}

#[cfg(test)]
fn extract_session_changed_files(session: &Session) -> Vec<String> {
    extract_session_change_evidence(session)
        .into_iter()
        .filter_map(|row| {
            row.get("path")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .collect()
}

async fn collect_workspace_file_snapshots(
    workspace_root: &str,
    changed_files: &[String],
) -> Vec<Value> {
    let mut snapshots = Vec::<Value>::new();
    let root = PathBuf::from(workspace_root);
    for path in changed_files.iter().take(20) {
        let rel = match crate::http::global::sanitize_relative_subpath(Some(path)) {
            Ok(value) => value,
            Err(_) => {
                snapshots.push(json!({
                    "path": path,
                    "exists": false,
                    "error": "invalid_relative_path",
                }));
                continue;
            }
        };
        let full_path = root.join(&rel);
        match tokio::fs::read(&full_path).await {
            Ok(bytes) => {
                let preview = change_preview_from_bytes(&bytes);
                let line_count = if bytes.is_empty() {
                    0
                } else {
                    bytes.iter().filter(|byte| **byte == b'\n').count() + 1
                };
                snapshots.push(json!({
                    "path": path,
                    "exists": true,
                    "byte_size": bytes.len(),
                    "line_count": line_count,
                    "preview": preview,
                }));
            }
            Err(error) => snapshots.push(json!({
                "path": path,
                "exists": false,
                "error": crate::truncate_text(&error.to_string(), 160),
            })),
        }
    }
    snapshots
}

async fn load_latest_coder_artifact_payload(
    state: &AppState,
    record: &CoderRunRecord,
    artifact_type: &str,
) -> Option<Value> {
    let blackboard = load_context_blackboard(state, &record.linked_context_run_id);
    let artifact = blackboard
        .artifacts
        .iter()
        .rev()
        .find(|artifact| artifact.artifact_type == artifact_type)?;
    let raw = tokio::fs::read_to_string(&artifact.path).await.ok()?;
    serde_json::from_str::<Value>(&raw).ok()
}

fn build_issue_fix_worker_prompt(
    record: &CoderRunRecord,
    run: &ContextRunState,
    memory_hits_used: &[String],
) -> String {
    let issue_number = record
        .github_ref
        .as_ref()
        .map(|row| row.number)
        .unwrap_or_default();
    let memory_hint = if memory_hits_used.is_empty() {
        "none".to_string()
    } else {
        memory_hits_used.join(", ")
    };
    format!(
        concat!(
            "You are the Tandem coder issue-fix worker.\n",
            "Repository: {repo_slug}\n",
            "Workspace root: {workspace_root}\n",
            "Issue number: #{issue_number}\n",
            "Context run ID: {context_run_id}\n",
            "Memory hits already surfaced: {memory_hint}\n\n",
            "Task:\n",
            "1. Inspect the repository and issue context.\n",
            "2. Propose a constrained fix plan.\n",
            "3. If safe, make the smallest useful code change.\n",
            "4. Run targeted validation.\n",
            "5. Respond with a concise fix report.\n\n",
            "Return a compact response with these headings:\n",
            "Summary:\n",
            "Root Cause:\n",
            "Fix Strategy:\n",
            "Changed Files:\n",
            "Validation:\n"
        ),
        repo_slug = record.repo_binding.repo_slug,
        workspace_root = record.repo_binding.workspace_root,
        issue_number = issue_number,
        context_run_id = run.run_id,
        memory_hint = memory_hint,
    )
}

fn build_pr_review_worker_prompt(
    record: &CoderRunRecord,
    run: &ContextRunState,
    memory_hits_used: &[String],
) -> String {
    let pull_number = record
        .github_ref
        .as_ref()
        .map(|row| row.number)
        .unwrap_or_default();
    let memory_hint = if memory_hits_used.is_empty() {
        "none".to_string()
    } else {
        memory_hits_used.join(", ")
    };
    format!(
        concat!(
            "You are the Tandem coder PR-review worker.\n",
            "Repository: {repo_slug}\n",
            "Workspace root: {workspace_root}\n",
            "Pull request number: #{pull_number}\n",
            "Context run ID: {context_run_id}\n",
            "Memory hits already surfaced: {memory_hint}\n\n",
            "Task:\n",
            "1. Inspect the pull request context and changed areas.\n",
            "2. Identify the highest-signal review findings.\n",
            "3. Call out blockers and requested changes.\n",
            "4. Flag any regression risk.\n\n",
            "Return a compact response with these headings:\n",
            "Summary:\n",
            "Verdict:\n",
            "Risk Level:\n",
            "Changed Files:\n",
            "Blockers:\n",
            "Requested Changes:\n",
            "Regression Signals:\n"
        ),
        repo_slug = record.repo_binding.repo_slug,
        workspace_root = record.repo_binding.workspace_root,
        pull_number = pull_number,
        context_run_id = run.run_id,
        memory_hint = memory_hint,
    )
}

fn build_merge_recommendation_worker_prompt(
    record: &CoderRunRecord,
    run: &ContextRunState,
    memory_hits_used: &[String],
) -> String {
    let pull_number = record
        .github_ref
        .as_ref()
        .map(|row| row.number)
        .unwrap_or_default();
    let memory_hint = if memory_hits_used.is_empty() {
        "none".to_string()
    } else {
        memory_hits_used.join(", ")
    };
    format!(
        concat!(
            "You are the Tandem coder merge-readiness worker.\n",
            "Repository: {repo_slug}\n",
            "Workspace root: {workspace_root}\n",
            "Pull request number: #{pull_number}\n",
            "Context run ID: {context_run_id}\n",
            "Memory hits already surfaced: {memory_hint}\n\n",
            "Task:\n",
            "1. Inspect the pull request and current review state.\n",
            "2. Assess merge readiness conservatively.\n",
            "3. List blockers, required checks, and required approvals.\n",
            "4. Return a compact merge recommendation.\n\n",
            "Return a compact response with these headings:\n",
            "Summary:\n",
            "Recommendation:\n",
            "Risk Level:\n",
            "Blockers:\n",
            "Required Checks:\n",
            "Required Approvals:\n"
        ),
        repo_slug = record.repo_binding.repo_slug,
        workspace_root = record.repo_binding.workspace_root,
        pull_number = pull_number,
        context_run_id = run.run_id,
        memory_hint = memory_hint,
    )
}

fn extract_labeled_section(text: &str, label: &str) -> Option<String> {
    let marker = format!("{label}:");
    let start = text.find(&marker)?;
    let after = &text[start + marker.len()..];
    let known_labels = [
        "Summary:",
        "Root Cause:",
        "Fix Strategy:",
        "Changed Files:",
        "Validation:",
        "Verdict:",
        "Risk Level:",
        "Blockers:",
        "Requested Changes:",
        "Regression Signals:",
        "Recommendation:",
        "Required Checks:",
        "Required Approvals:",
    ];
    let end = known_labels
        .iter()
        .filter_map(|candidate| {
            if *candidate == marker {
                return None;
            }
            after.find(candidate)
        })
        .min()
        .unwrap_or(after.len());
    let section = after[..end].trim();
    if section.is_empty() {
        return None;
    }
    Some(section.to_string())
}

fn parse_bulleted_lines(section: Option<String>) -> Vec<String> {
    section
        .map(|section| {
            section
                .lines()
                .map(str::trim)
                .map(|line| line.trim_start_matches("-").trim())
                .filter(|line| !line.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn parse_issue_fix_plan_from_worker_payload(worker_payload: &Value) -> Value {
    let assistant_text = worker_payload
        .get("assistant_text")
        .and_then(Value::as_str)
        .unwrap_or("");
    let summary = extract_labeled_section(assistant_text, "Summary").or_else(|| {
        (!assistant_text.trim().is_empty()).then(|| crate::truncate_text(assistant_text, 240))
    });
    let root_cause = extract_labeled_section(assistant_text, "Root Cause");
    let fix_strategy = extract_labeled_section(assistant_text, "Fix Strategy");
    let mut changed_files = extract_labeled_section(assistant_text, "Changed Files")
        .map(|section| {
            section
                .lines()
                .map(str::trim)
                .map(|line| line.trim_start_matches("-").trim())
                .filter(|line| !line.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if changed_files.is_empty() {
        changed_files = worker_payload
            .get("changed_files")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
    }
    let validation_steps = extract_labeled_section(assistant_text, "Validation")
        .map(|section| {
            section
                .lines()
                .map(str::trim)
                .map(|line| line.trim_start_matches("-").trim())
                .filter(|line| !line.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    json!({
        "summary": summary,
        "root_cause": root_cause,
        "fix_strategy": fix_strategy,
        "changed_files": changed_files,
        "validation_steps": validation_steps,
        "worker_session_id": worker_payload.get("session_id").cloned(),
        "worker_session_run_id": worker_payload.get("session_run_id").cloned(),
        "worker_model": worker_payload.get("model").cloned(),
        "assistant_text": worker_payload.get("assistant_text").cloned(),
    })
}

fn parse_pr_review_from_worker_payload(worker_payload: &Value) -> Value {
    let assistant_text = worker_payload
        .get("assistant_text")
        .and_then(Value::as_str)
        .unwrap_or("");
    let summary = extract_labeled_section(assistant_text, "Summary").or_else(|| {
        (!assistant_text.trim().is_empty()).then(|| crate::truncate_text(assistant_text, 240))
    });
    let verdict = extract_labeled_section(assistant_text, "Verdict");
    let risk_level = extract_labeled_section(assistant_text, "Risk Level");
    let changed_files =
        parse_bulleted_lines(extract_labeled_section(assistant_text, "Changed Files"));
    let blockers = parse_bulleted_lines(extract_labeled_section(assistant_text, "Blockers"));
    let requested_changes =
        parse_bulleted_lines(extract_labeled_section(assistant_text, "Requested Changes"));
    let regression_signals = parse_bulleted_lines(extract_labeled_section(
        assistant_text,
        "Regression Signals",
    ))
    .into_iter()
    .map(|summary| {
        json!({
            "kind": "worker_regression_signal",
            "summary": summary,
        })
    })
    .collect::<Vec<_>>();
    json!({
        "summary": summary,
        "verdict": verdict,
        "risk_level": risk_level,
        "changed_files": changed_files,
        "blockers": blockers,
        "requested_changes": requested_changes,
        "regression_signals": regression_signals,
        "worker_session_id": worker_payload.get("session_id").cloned(),
        "worker_session_run_id": worker_payload.get("session_run_id").cloned(),
        "worker_model": worker_payload.get("model").cloned(),
        "assistant_text": worker_payload.get("assistant_text").cloned(),
    })
}

fn parse_merge_recommendation_from_worker_payload(worker_payload: &Value) -> Value {
    let assistant_text = worker_payload
        .get("assistant_text")
        .and_then(Value::as_str)
        .unwrap_or("");
    let summary = extract_labeled_section(assistant_text, "Summary").or_else(|| {
        (!assistant_text.trim().is_empty()).then(|| crate::truncate_text(assistant_text, 240))
    });
    let recommendation = extract_labeled_section(assistant_text, "Recommendation");
    let risk_level = extract_labeled_section(assistant_text, "Risk Level");
    let blockers = parse_bulleted_lines(extract_labeled_section(assistant_text, "Blockers"));
    let required_checks =
        parse_bulleted_lines(extract_labeled_section(assistant_text, "Required Checks"));
    let required_approvals = parse_bulleted_lines(extract_labeled_section(
        assistant_text,
        "Required Approvals",
    ));
    json!({
        "summary": summary,
        "recommendation": recommendation,
        "risk_level": risk_level,
        "blockers": blockers,
        "required_checks": required_checks,
        "required_approvals": required_approvals,
        "worker_session_id": worker_payload.get("session_id").cloned(),
        "worker_session_run_id": worker_payload.get("session_run_id").cloned(),
        "worker_model": worker_payload.get("model").cloned(),
        "assistant_text": worker_payload.get("assistant_text").cloned(),
    })
}

async fn write_issue_fix_plan_artifact(
    state: &AppState,
    record: &CoderRunRecord,
    worker_payload: &Value,
    memory_hits_used: &[String],
    phase: Option<&str>,
) -> Result<ContextBlackboardArtifact, StatusCode> {
    let mut payload = parse_issue_fix_plan_from_worker_payload(worker_payload);
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("coder_run_id".to_string(), json!(record.coder_run_id));
        obj.insert(
            "linked_context_run_id".to_string(),
            json!(record.linked_context_run_id),
        );
        obj.insert("workflow_mode".to_string(), json!(record.workflow_mode));
        obj.insert("repo_binding".to_string(), json!(record.repo_binding));
        obj.insert("github_ref".to_string(), json!(record.github_ref));
        obj.insert("memory_hits_used".to_string(), json!(memory_hits_used));
        obj.insert("created_at_ms".to_string(), json!(crate::now_ms()));
    }
    let artifact = write_coder_artifact(
        state,
        &record.linked_context_run_id,
        &format!("issue-fix-plan-{}", Uuid::new_v4().simple()),
        "coder_issue_fix_plan",
        "artifacts/issue_fix.plan.json",
        &payload,
    )
    .await?;
    publish_coder_artifact_added(state, record, &artifact, phase, {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("issue_fix_plan"));
        if let Some(summary) = payload.get("summary").cloned() {
            extra.insert("summary".to_string(), summary);
        }
        extra
    });
    Ok(artifact)
}

async fn write_issue_fix_changed_file_evidence_artifact(
    state: &AppState,
    record: &CoderRunRecord,
    worker_payload: &Value,
    phase: Option<&str>,
) -> Result<Option<ContextBlackboardArtifact>, StatusCode> {
    let changed_files = worker_payload
        .get("changed_files")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if changed_files.is_empty() {
        return Ok(None);
    }
    let workspace_snapshots =
        collect_workspace_file_snapshots(&record.repo_binding.workspace_root, &changed_files).await;
    let payload = json!({
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "changed_files": changed_files,
        "entries": worker_payload.get("changed_file_entries").cloned().unwrap_or_else(|| json!([])),
        "workspace_snapshots": workspace_snapshots,
        "worker_session_id": worker_payload.get("session_id").cloned(),
        "worker_session_run_id": worker_payload.get("session_run_id").cloned(),
        "created_at_ms": crate::now_ms(),
    });
    let artifact = write_coder_artifact(
        state,
        &record.linked_context_run_id,
        &format!("issue-fix-changed-files-{}", Uuid::new_v4().simple()),
        "coder_changed_file_evidence",
        "artifacts/issue_fix.changed_files.json",
        &payload,
    )
    .await?;
    publish_coder_artifact_added(state, record, &artifact, phase, {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("changed_file_evidence"));
        extra.insert(
            "changed_file_count".to_string(),
            json!(payload["changed_files"]
                .as_array()
                .map(|rows| rows.len())
                .unwrap_or(0)),
        );
        extra
    });
    Ok(Some(artifact))
}

async fn write_issue_fix_patch_summary_artifact(
    state: &AppState,
    record: &CoderRunRecord,
    summary: Option<&str>,
    root_cause: Option<&str>,
    fix_strategy: Option<&str>,
    changed_files: &[String],
    validation_results: &[Value],
    worker_session: Option<&Value>,
    validation_session: Option<&Value>,
    phase: Option<&str>,
) -> Result<Option<ContextBlackboardArtifact>, StatusCode> {
    if changed_files.is_empty()
        && summary.map(str::trim).unwrap_or("").is_empty()
        && root_cause.map(str::trim).unwrap_or("").is_empty()
        && fix_strategy.map(str::trim).unwrap_or("").is_empty()
        && validation_results.is_empty()
        && validation_session.is_none()
    {
        return Ok(None);
    }
    let workspace_snapshots =
        collect_workspace_file_snapshots(&record.repo_binding.workspace_root, changed_files).await;
    let payload = json!({
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "summary": summary,
        "root_cause": root_cause,
        "fix_strategy": fix_strategy,
        "changed_files": changed_files,
        "changed_file_entries": worker_session
            .and_then(|payload| payload.get("changed_file_entries"))
            .cloned()
            .unwrap_or_else(|| json!([])),
        "workspace_snapshots": workspace_snapshots,
        "validation_results": validation_results,
        "worker_session_id": worker_session.and_then(|payload| payload.get("session_id")).cloned(),
        "worker_session_run_id": worker_session.and_then(|payload| payload.get("session_run_id")).cloned(),
        "validation_session_id": validation_session.and_then(|payload| payload.get("session_id")).cloned(),
        "validation_session_run_id": validation_session.and_then(|payload| payload.get("session_run_id")).cloned(),
        "created_at_ms": crate::now_ms(),
    });
    let artifact = write_coder_artifact(
        state,
        &record.linked_context_run_id,
        &format!("issue-fix-patch-summary-{}", Uuid::new_v4().simple()),
        "coder_patch_summary",
        "artifacts/issue_fix.patch_summary.json",
        &payload,
    )
    .await?;
    publish_coder_artifact_added(state, record, &artifact, phase, {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("patch_summary"));
        extra.insert("changed_file_count".to_string(), json!(changed_files.len()));
        if let Some(fix_strategy) = fix_strategy {
            extra.insert("fix_strategy".to_string(), json!(fix_strategy));
        }
        extra
    });
    Ok(Some(artifact))
}

fn build_issue_fix_pr_draft_title(
    record: &CoderRunRecord,
    input_title: Option<&str>,
    summary_payload: Option<&Value>,
) -> String {
    if let Some(title) = input_title
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
    {
        return title;
    }
    if let Some(summary) = summary_payload
        .and_then(|payload| payload.get("summary"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return crate::truncate_text(summary, 120);
    }
    let issue_number = record
        .github_ref
        .as_ref()
        .map(|row| row.number)
        .unwrap_or_default();
    format!(
        "Fix issue #{issue_number} in {}",
        record.repo_binding.repo_slug
    )
}

fn build_issue_fix_pr_draft_body(
    record: &CoderRunRecord,
    input_body: Option<&str>,
    summary_payload: Option<&Value>,
    patch_summary_payload: Option<&Value>,
    validation_payload: Option<&Value>,
    changed_files_override: &[String],
    notes: Option<&str>,
) -> String {
    if let Some(body) = input_body
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
    {
        return body;
    }
    let issue_number = record
        .github_ref
        .as_ref()
        .map(|row| row.number)
        .unwrap_or_default();
    let summary = summary_payload
        .and_then(|payload| payload.get("summary"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("No fix summary was recorded.");
    let root_cause = summary_payload
        .and_then(|payload| payload.get("root_cause"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Not recorded.");
    let fix_strategy = summary_payload
        .and_then(|payload| payload.get("fix_strategy"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Not recorded.");
    let changed_files = if !changed_files_override.is_empty() {
        changed_files_override.to_vec()
    } else {
        patch_summary_payload
            .and_then(|payload| payload.get("changed_files"))
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let validation_lines = validation_payload
        .and_then(|payload| payload.get("validation_results"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let status = row.get("status").and_then(Value::as_str)?;
                    let summary = row
                        .get("summary")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .unwrap_or(status);
                    Some(format!("- {status}: {summary}"))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let changed_files_block = if changed_files.is_empty() {
        "- No changed files were recorded.".to_string()
    } else {
        changed_files
            .iter()
            .map(|path| format!("- `{path}`"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let validation_block = if validation_lines.is_empty() {
        "- No validation results were recorded.".to_string()
    } else {
        validation_lines.join("\n")
    };
    let notes_block = notes
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| "None.".to_string());
    format!(
        concat!(
            "## Summary\n",
            "{summary}\n\n",
            "## Root Cause\n",
            "{root_cause}\n\n",
            "## Fix Strategy\n",
            "{fix_strategy}\n\n",
            "## Changed Files\n",
            "{changed_files}\n\n",
            "## Validation\n",
            "{validation}\n\n",
            "## Notes\n",
            "{notes}\n\n",
            "Closes #{issue_number}\n"
        ),
        summary = summary,
        root_cause = root_cause,
        fix_strategy = fix_strategy,
        changed_files = changed_files_block,
        validation = validation_block,
        notes = notes_block,
        issue_number = issue_number,
    )
}

fn split_owner_repo(repo: &str) -> Result<(&str, &str), StatusCode> {
    let mut parts = repo.split('/');
    let owner = parts
        .next()
        .filter(|value| !value.trim().is_empty())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let repo_name = parts
        .next()
        .filter(|value| !value.trim().is_empty())
        .ok_or(StatusCode::BAD_REQUEST)?;
    if parts.next().is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok((owner, repo_name))
}

fn map_namespaced_to_raw_tool(
    tools: &[McpRemoteTool],
    namespaced_name_or_raw_tool: &str,
) -> Result<String, StatusCode> {
    tools
        .iter()
        .find(|row| {
            row.namespaced_name == namespaced_name_or_raw_tool
                || row.tool_name == namespaced_name_or_raw_tool
        })
        .map(|row| row.tool_name.clone())
        .ok_or(StatusCode::BAD_GATEWAY)
}

async fn resolve_github_create_pr_tool(
    state: &AppState,
    preferred_server: Option<&str>,
) -> Result<(String, String), StatusCode> {
    let mut server_candidates = if let Some(server_name) = preferred_server
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        vec![server_name.to_string()]
    } else {
        let mut servers = state
            .mcp
            .list()
            .await
            .into_values()
            .filter(|server| server.enabled && server.connected)
            .map(|server| server.name)
            .collect::<Vec<_>>();
        servers.sort();
        servers
    };
    if server_candidates.is_empty() {
        return Err(StatusCode::CONFLICT);
    }
    for server_name in server_candidates.drain(..) {
        let server_tools = state.mcp.server_tools(&server_name).await;
        if server_tools.is_empty() {
            continue;
        }
        let discovered = state
            .capability_resolver
            .discover_from_runtime(server_tools.clone(), Vec::new())
            .await;
        let resolved = state
            .capability_resolver
            .resolve(
                crate::capability_resolver::CapabilityResolveInput {
                    workflow_id: Some("coder_issue_fix_pr_submit".to_string()),
                    required_capabilities: vec!["github.create_pull_request".to_string()],
                    optional_capabilities: Vec::new(),
                    provider_preference: vec!["mcp".to_string()],
                    available_tools: discovered,
                },
                Vec::new(),
            )
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY)?;
        let Some(namespaced) = resolved
            .resolved
            .iter()
            .find(|row| row.capability_id == "github.create_pull_request")
            .map(|row| row.tool_name.clone())
        else {
            continue;
        };
        let raw_tool = map_namespaced_to_raw_tool(&server_tools, &namespaced)?;
        return Ok((server_name, raw_tool));
    }
    Err(StatusCode::CONFLICT)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct GithubPullRequestSummary {
    number: u64,
    title: String,
    state: String,
    html_url: Option<String>,
    head_ref: Option<String>,
    base_ref: Option<String>,
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

fn extract_pull_requests_from_tool_result(
    result: &tandem_types::ToolResult,
) -> Vec<GithubPullRequestSummary> {
    let mut out = Vec::new();
    for candidate in tool_result_values(result) {
        collect_pull_requests(&candidate, &mut out);
    }
    dedupe_pull_requests(out)
}

fn collect_pull_requests(value: &Value, out: &mut Vec<GithubPullRequestSummary>) {
    match value {
        Value::Object(map) => {
            let number = map
                .get("number")
                .or_else(|| map.get("pull_number"))
                .and_then(Value::as_u64);
            let title = map
                .get("title")
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
                .map(ToString::to_string);
            let head_ref = map
                .get("head")
                .and_then(Value::as_object)
                .and_then(|head| head.get("ref"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| {
                    map.get("head_ref")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                });
            let base_ref = map
                .get("base")
                .and_then(Value::as_object)
                .and_then(|base| base.get("ref"))
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| {
                    map.get("base_ref")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                });
            if let Some(number) = number {
                out.push(GithubPullRequestSummary {
                    number,
                    title,
                    state,
                    html_url,
                    head_ref,
                    base_ref,
                });
            }
            for nested in map.values() {
                collect_pull_requests(nested, out);
            }
        }
        Value::Array(rows) => {
            for row in rows {
                collect_pull_requests(row, out);
            }
        }
        _ => {}
    }
}

fn dedupe_pull_requests(rows: Vec<GithubPullRequestSummary>) -> Vec<GithubPullRequestSummary> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        if seen.insert(row.number) {
            out.push(row);
        }
    }
    out
}

fn github_ref_from_pull_request(pull: &GithubPullRequestSummary) -> Value {
    json!({
        "kind": "pull_request",
        "number": pull.number,
        "url": pull.html_url,
    })
}

fn parse_coder_github_ref(value: &Value) -> Option<CoderGithubRef> {
    let kind = match value.get("kind").and_then(Value::as_str)? {
        "issue" => CoderGithubRefKind::Issue,
        "pull_request" => CoderGithubRefKind::PullRequest,
        _ => return None,
    };
    Some(CoderGithubRef {
        kind,
        number: value.get("number").and_then(Value::as_u64)?,
        url: value
            .get("url")
            .and_then(Value::as_str)
            .map(ToString::to_string),
    })
}

fn build_follow_on_run_templates(
    record: &CoderRunRecord,
    github_ref: &CoderGithubRef,
    mcp_servers: &[String],
) -> Vec<Value> {
    [
        CoderWorkflowMode::PrReview,
        CoderWorkflowMode::MergeRecommendation,
    ]
    .into_iter()
    .map(|workflow_mode| {
        let requires_explicit_auto_spawn =
            matches!(workflow_mode, CoderWorkflowMode::MergeRecommendation);
        json!({
            "workflow_mode": workflow_mode,
            "repo_binding": record.repo_binding,
            "github_ref": github_ref,
            "source_client": record.source_client,
            "model_provider": record.model_provider,
            "model_id": record.model_id,
            "mcp_servers": mcp_servers,
            "auto_spawn_allowed_by_default": !requires_explicit_auto_spawn,
            "requires_explicit_auto_spawn": requires_explicit_auto_spawn,
        })
    })
    .collect::<Vec<_>>()
}

fn normalize_follow_on_workflow_modes(requested: &[CoderWorkflowMode]) -> Vec<CoderWorkflowMode> {
    let wants_review = requested
        .iter()
        .any(|mode| matches!(mode, CoderWorkflowMode::PrReview));
    let wants_merge = requested
        .iter()
        .any(|mode| matches!(mode, CoderWorkflowMode::MergeRecommendation));
    let mut normalized = Vec::new();
    if wants_review || wants_merge {
        normalized.push(CoderWorkflowMode::PrReview);
    }
    if wants_merge {
        normalized.push(CoderWorkflowMode::MergeRecommendation);
    }
    normalized
}

fn split_auto_spawn_follow_on_workflow_modes(
    requested: &[CoderWorkflowMode],
    allow_auto_merge_recommendation: bool,
) -> (Vec<CoderWorkflowMode>, Vec<Value>) {
    let mut auto_spawn_modes = Vec::new();
    let mut skipped = Vec::new();
    for workflow_mode in normalize_follow_on_workflow_modes(requested) {
        if matches!(workflow_mode, CoderWorkflowMode::MergeRecommendation)
            && !allow_auto_merge_recommendation
        {
            skipped.push(json!({
                "workflow_mode": workflow_mode,
                "reason": "requires_explicit_auto_merge_recommendation_opt_in",
            }));
            continue;
        }
        auto_spawn_modes.push(workflow_mode);
    }
    (auto_spawn_modes, skipped)
}

fn build_follow_on_run_create_input(
    record: &CoderRunRecord,
    workflow_mode: CoderWorkflowMode,
    github_ref: CoderGithubRef,
    source_client: Option<String>,
    model_provider: Option<String>,
    model_id: Option<String>,
    mcp_servers: Option<Vec<String>>,
    parent_coder_run_id: Option<String>,
    origin: Option<String>,
    origin_artifact_type: Option<String>,
) -> CoderRunCreateInput {
    CoderRunCreateInput {
        coder_run_id: None,
        workflow_mode,
        repo_binding: record.repo_binding.clone(),
        github_ref: Some(github_ref),
        objective: None,
        source_client,
        workspace: None,
        model_provider,
        model_id,
        mcp_servers,
        parent_coder_run_id,
        origin,
        origin_artifact_type,
    }
}

async fn call_create_pull_request(
    state: &AppState,
    server_name: &str,
    tool_name: &str,
    owner: &str,
    repo: &str,
    title: &str,
    body: &str,
    base_branch: &str,
    head_branch: &str,
) -> Result<tandem_types::ToolResult, StatusCode> {
    let preferred = json!({
        "method": "create",
        "owner": owner,
        "repo": repo,
        "title": title,
        "body": body,
        "base": base_branch,
        "head": head_branch,
        "draft": true,
    });
    let fallback = json!({
        "owner": owner,
        "repo": repo,
        "title": title,
        "body": body,
        "base": base_branch,
        "head": head_branch,
        "draft": true,
    });
    let first = state.mcp.call_tool(server_name, tool_name, preferred).await;
    match first {
        Ok(result) => Ok(result),
        Err(_) => state
            .mcp
            .call_tool(server_name, tool_name, fallback)
            .await
            .map_err(|_| StatusCode::BAD_GATEWAY),
    }
}

pub(super) async fn coder_issue_fix_pr_draft_create(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderIssueFixPrDraftCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    let record = load_coder_run_record(&state, &id).await?;
    if !matches!(record.workflow_mode, CoderWorkflowMode::IssueFix) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let summary_payload =
        load_latest_coder_artifact_payload(&state, &record, "coder_issue_fix_summary").await;
    let patch_summary_payload =
        load_latest_coder_artifact_payload(&state, &record, "coder_patch_summary").await;
    let validation_payload =
        load_latest_coder_artifact_payload(&state, &record, "coder_validation_report").await;
    let title =
        build_issue_fix_pr_draft_title(&record, input.title.as_deref(), summary_payload.as_ref());
    let body = build_issue_fix_pr_draft_body(
        &record,
        input.body.as_deref(),
        summary_payload.as_ref(),
        patch_summary_payload.as_ref(),
        validation_payload.as_ref(),
        &input.changed_files,
        input.notes.as_deref(),
    );
    let head_branch = input
        .head_branch
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| {
            format!(
                "coder/issue-{}-fix",
                record
                    .github_ref
                    .as_ref()
                    .map(|row| row.number)
                    .unwrap_or_default()
            )
        });
    let base_branch = input
        .base_branch
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "main".to_string());
    let changed_files = if !input.changed_files.is_empty() {
        input.changed_files.clone()
    } else {
        patch_summary_payload
            .as_ref()
            .and_then(|payload| payload.get("changed_files"))
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(Value::as_str)
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let payload = json!({
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "title": title,
        "body": body,
        "base_branch": base_branch,
        "head_branch": head_branch,
        "changed_files": changed_files,
        "memory_hits_used": input.memory_hits_used,
        "approval_required": true,
        "summary_artifact_path": summary_payload
            .as_ref()
            .and_then(|_| load_context_blackboard(&state, &record.linked_context_run_id)
                .artifacts
                .iter()
                .rev()
                .find(|artifact| artifact.artifact_type == "coder_issue_fix_summary")
                .map(|artifact| artifact.path.clone())),
        "patch_summary_artifact_path": patch_summary_payload
            .as_ref()
            .and_then(|_| load_context_blackboard(&state, &record.linked_context_run_id)
                .artifacts
                .iter()
                .rev()
                .find(|artifact| artifact.artifact_type == "coder_patch_summary")
                .map(|artifact| artifact.path.clone())),
        "validation_artifact_path": validation_payload
            .as_ref()
            .and_then(|_| load_context_blackboard(&state, &record.linked_context_run_id)
                .artifacts
                .iter()
                .rev()
                .find(|artifact| artifact.artifact_type == "coder_validation_report")
                .map(|artifact| artifact.path.clone())),
        "created_at_ms": crate::now_ms(),
    });
    let artifact = write_coder_artifact(
        &state,
        &record.linked_context_run_id,
        &format!("issue-fix-pr-draft-{}", Uuid::new_v4().simple()),
        "coder_pr_draft",
        "artifacts/issue_fix.pr_draft.json",
        &payload,
    )
    .await?;
    publish_coder_artifact_added(&state, &record, &artifact, Some("approval"), {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("pr_draft"));
        extra.insert("title".to_string(), json!(payload["title"]));
        extra.insert("approval_required".to_string(), json!(true));
        extra
    });
    publish_coder_run_event(
        &state,
        "coder.approval.required",
        &record,
        Some("approval"),
        {
            let mut extra = serde_json::Map::new();
            extra.insert("event_type".to_string(), json!("pr_draft_ready"));
            extra.insert("artifact_id".to_string(), json!(artifact.id));
            extra.insert("title".to_string(), json!(payload["title"]));
            extra
        },
    );
    Ok(Json(json!({
        "ok": true,
        "artifact": artifact,
        "approval_required": true,
        "coder_run": coder_run_payload(
            &record,
            &load_context_run_state(&state, &record.linked_context_run_id).await?,
        ),
        "run": load_context_run_state(&state, &record.linked_context_run_id).await?,
    })))
}

pub(super) async fn coder_issue_fix_pr_submit(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderIssueFixPrSubmitInput>,
) -> Result<Json<Value>, StatusCode> {
    let record = load_coder_run_record(&state, &id).await?;
    if !matches!(record.workflow_mode, CoderWorkflowMode::IssueFix) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let approved_by = input
        .approved_by
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(StatusCode::BAD_REQUEST)?;
    let readiness = coder_pr_submit_readiness(&state, input.mcp_server.as_deref()).await?;
    if !readiness.runnable {
        return Ok(Json(json!({
            "ok": false,
            "code": "CODER_PR_SUBMIT_BLOCKED",
            "readiness": readiness,
        })));
    }
    let draft_payload = load_latest_coder_artifact_payload(&state, &record, "coder_pr_draft")
        .await
        .ok_or(StatusCode::CONFLICT)?;
    let title = draft_payload
        .get("title")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or(StatusCode::CONFLICT)?;
    let body = draft_payload
        .get("body")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or(StatusCode::CONFLICT)?;
    let base_branch = draft_payload
        .get("base_branch")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("main");
    let head_branch = draft_payload
        .get("head_branch")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("coder/issue-fix");
    let dry_run = input.dry_run.unwrap_or(true);
    let requested_follow_on_modes = normalize_follow_on_workflow_modes(&input.spawn_follow_on_runs);
    for workflow_mode in &requested_follow_on_modes {
        if !matches!(
            workflow_mode,
            CoderWorkflowMode::PrReview | CoderWorkflowMode::MergeRecommendation
        ) {
            return Err(StatusCode::BAD_REQUEST);
        }
    }
    let allow_auto_merge_recommendation = input.allow_auto_merge_recommendation.unwrap_or(false);
    let (auto_spawn_follow_on_modes, skipped_follow_on_runs) =
        split_auto_spawn_follow_on_workflow_modes(
            &input.spawn_follow_on_runs,
            allow_auto_merge_recommendation,
        );
    let (owner, repo_name) = split_owner_repo(&record.repo_binding.repo_slug)?;
    let mut submission_payload = json!({
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "owner": owner,
        "repo": repo_name,
        "approved_by": approved_by,
        "approval_reason": input.reason,
        "title": title,
        "body": body,
        "base_branch": base_branch,
        "head_branch": head_branch,
        "dry_run": dry_run,
        "requested_spawn_follow_on_runs": requested_follow_on_modes,
        "allow_auto_merge_recommendation": allow_auto_merge_recommendation,
        "submitted_github_ref": Value::Null,
        "skipped_follow_on_runs": skipped_follow_on_runs,
        "spawned_follow_on_runs": [],
        "created_at_ms": crate::now_ms(),
        "readiness": readiness,
    });
    if !dry_run {
        let (server_name, tool_name) =
            resolve_github_create_pr_tool(&state, input.mcp_server.as_deref()).await?;
        let result = call_create_pull_request(
            &state,
            &server_name,
            &tool_name,
            owner,
            repo_name,
            title,
            body,
            base_branch,
            head_branch,
        )
        .await?;
        let pull_request = extract_pull_requests_from_tool_result(&result)
            .into_iter()
            .next()
            .ok_or(StatusCode::BAD_GATEWAY)?;
        let submitted_github_ref =
            parse_coder_github_ref(&github_ref_from_pull_request(&pull_request))
                .ok_or(StatusCode::BAD_GATEWAY)?;
        let follow_on_templates =
            build_follow_on_run_templates(&record, &submitted_github_ref, &[server_name.clone()]);
        if let Some(obj) = submission_payload.as_object_mut() {
            obj.insert("server_name".to_string(), json!(server_name));
            obj.insert("tool_name".to_string(), json!(tool_name));
            obj.insert("submitted".to_string(), json!(true));
            obj.insert(
                "submitted_github_ref".to_string(),
                json!(submitted_github_ref),
            );
            obj.insert("pull_request".to_string(), json!(pull_request));
            obj.insert("follow_on_runs".to_string(), json!(follow_on_templates));
            obj.insert(
                "tool_result".to_string(),
                json!({
                    "output": result.output,
                    "metadata": result.metadata,
                }),
            );
        }
    } else if let Some(obj) = submission_payload.as_object_mut() {
        obj.insert("submitted".to_string(), json!(false));
        obj.insert("follow_on_runs".to_string(), json!([]));
        obj.insert(
            "dry_run_preview".to_string(),
            json!({
                "owner": owner,
                "repo": repo_name,
                "base": base_branch,
                "head": head_branch,
            }),
        );
    }
    let mut spawned_follow_on_runs = Vec::<Value>::new();
    if !dry_run {
        let submitted_github_ref = submission_payload
            .get("submitted_github_ref")
            .and_then(parse_coder_github_ref);
        if let Some(submitted_github_ref) = submitted_github_ref {
            for workflow_mode in &auto_spawn_follow_on_modes {
                let create_input = build_follow_on_run_create_input(
                    &record,
                    workflow_mode.clone(),
                    submitted_github_ref.clone(),
                    record.source_client.clone(),
                    record.model_provider.clone(),
                    record.model_id.clone(),
                    input
                        .mcp_server
                        .as_ref()
                        .map(|server| vec![server.clone()])
                        .or_else(|| Some(vec!["github".to_string()])),
                    Some(record.coder_run_id.clone()),
                    Some("issue_fix_pr_submit_auto".to_string()),
                    Some("coder_pr_submission".to_string()),
                );
                let response = coder_run_create(State(state.clone()), Json(create_input)).await?;
                let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                let payload: Value = serde_json::from_slice(&bytes)
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                spawned_follow_on_runs.push(payload);
            }
        }
    }
    if let Some(obj) = submission_payload.as_object_mut() {
        obj.insert(
            "spawned_follow_on_runs".to_string(),
            json!(spawned_follow_on_runs),
        );
    }
    let artifact = write_coder_artifact(
        &state,
        &record.linked_context_run_id,
        &format!("issue-fix-pr-submit-{}", Uuid::new_v4().simple()),
        "coder_pr_submission",
        "artifacts/issue_fix.pr_submission.json",
        &submission_payload,
    )
    .await?;
    publish_coder_artifact_added(&state, &record, &artifact, Some("approval"), {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("pr_submission"));
        extra.insert("dry_run".to_string(), json!(dry_run));
        extra.insert(
            "submitted".to_string(),
            json!(submission_payload
                .get("submitted")
                .and_then(Value::as_bool)
                .unwrap_or(false)),
        );
        extra
    });
    if !dry_run {
        publish_coder_run_event(&state, "coder.pr.submitted", &record, Some("approval"), {
            let mut extra = serde_json::Map::new();
            extra.insert("artifact_id".to_string(), json!(artifact.id));
            extra.insert("title".to_string(), json!(title));
            extra.insert(
                "submitted_github_ref".to_string(),
                submission_payload
                    .get("submitted_github_ref")
                    .cloned()
                    .unwrap_or(Value::Null),
            );
            extra.insert(
                "follow_on_runs".to_string(),
                submission_payload
                    .get("follow_on_runs")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
            );
            extra.insert(
                "spawned_follow_on_runs".to_string(),
                submission_payload
                    .get("spawned_follow_on_runs")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
            );
            extra.insert(
                "skipped_follow_on_runs".to_string(),
                submission_payload
                    .get("skipped_follow_on_runs")
                    .cloned()
                    .unwrap_or_else(|| json!([])),
            );
            if let Some(number) = submission_payload
                .get("pull_request")
                .and_then(|row| row.get("number"))
                .and_then(Value::as_u64)
            {
                extra.insert("pull_request_number".to_string(), json!(number));
            }
            extra
        });
    }
    let run = load_context_run_state(&state, &record.linked_context_run_id).await?;
    Ok(Json(json!({
        "ok": true,
        "artifact": artifact,
        "submitted": submission_payload
            .get("submitted")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        "dry_run": dry_run,
        "submitted_github_ref": submission_payload
            .get("submitted_github_ref")
            .cloned()
            .unwrap_or(Value::Null),
        "pull_request": submission_payload
            .get("pull_request")
            .cloned()
            .unwrap_or(Value::Null),
        "follow_on_runs": submission_payload
            .get("follow_on_runs")
            .cloned()
            .unwrap_or_else(|| json!([])),
        "spawned_follow_on_runs": submission_payload
            .get("spawned_follow_on_runs")
            .cloned()
            .unwrap_or_else(|| json!([])),
        "skipped_follow_on_runs": submission_payload
            .get("skipped_follow_on_runs")
            .cloned()
            .unwrap_or_else(|| json!([])),
        "coder_run": coder_run_payload(&record, &run),
        "run": run,
    })))
}

pub(super) async fn coder_follow_on_run_create(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderFollowOnRunCreateInput>,
) -> Result<Response, StatusCode> {
    let record = load_coder_run_record(&state, &id).await?;
    if !matches!(record.workflow_mode, CoderWorkflowMode::IssueFix) {
        return Err(StatusCode::BAD_REQUEST);
    }
    if !matches!(
        input.workflow_mode,
        CoderWorkflowMode::PrReview | CoderWorkflowMode::MergeRecommendation
    ) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let submission_payload =
        load_latest_coder_artifact_payload(&state, &record, "coder_pr_submission")
            .await
            .ok_or(StatusCode::CONFLICT)?;
    let submitted_github_ref = submission_payload
        .get("submitted_github_ref")
        .and_then(parse_coder_github_ref)
        .ok_or(StatusCode::CONFLICT)?;
    if !matches!(submitted_github_ref.kind, CoderGithubRefKind::PullRequest) {
        return Err(StatusCode::CONFLICT);
    }
    let create_input = CoderRunCreateInput {
        coder_run_id: input.coder_run_id,
        ..build_follow_on_run_create_input(
            &record,
            input.workflow_mode,
            submitted_github_ref,
            normalize_source_client(input.source_client.as_deref())
                .or_else(|| record.source_client.clone()),
            normalize_source_client(input.model_provider.as_deref())
                .or_else(|| record.model_provider.clone()),
            normalize_source_client(input.model_id.as_deref()).or_else(|| record.model_id.clone()),
            input
                .mcp_servers
                .or_else(|| Some(vec!["github".to_string()])),
            Some(record.coder_run_id.clone()),
            Some("issue_fix_pr_submit_manual_follow_on".to_string()),
            Some("coder_pr_submission".to_string()),
        )
    };
    coder_run_create(State(state), Json(create_input)).await
}

async fn run_issue_fix_worker_session(
    state: &AppState,
    record: &CoderRunRecord,
    prompt: String,
    worker_kind: &str,
    artifact_type: &str,
    relative_path: &str,
) -> Result<(ContextBlackboardArtifact, Value), StatusCode> {
    let model = resolve_coder_worker_model_spec(state, record)
        .await
        .unwrap_or(tandem_types::ModelSpec {
            provider_id: "local".to_string(),
            model_id: "echo-1".to_string(),
        });
    let workflow_label = match record.workflow_mode {
        CoderWorkflowMode::IssueTriage => "Issue Triage",
        CoderWorkflowMode::IssueFix => "Issue Fix",
        CoderWorkflowMode::PrReview => "PR Review",
        CoderWorkflowMode::MergeRecommendation => "Merge Recommendation",
    };
    let session_title = format!(
        "Coder {workflow_label} {} / {}",
        record.coder_run_id, worker_kind
    );
    let mut session = Session::new(
        Some(session_title),
        Some(record.repo_binding.workspace_root.clone()),
    );
    session.project_id = Some(record.repo_binding.project_id.clone());
    session.workspace_root = Some(record.repo_binding.workspace_root.clone());
    session.environment = Some(state.host_runtime_context());
    session.provider = Some(model.provider_id.clone());
    session.model = Some(model.clone());
    let session_id = session.id.clone();
    state
        .storage
        .save_session(session)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let run_id = Uuid::new_v4().to_string();
    let client_id = Some(record.coder_run_id.clone());
    let agent_id = Some("coder_issue_fix_worker".to_string());
    let active_run = state
        .run_registry
        .acquire(
            &session_id,
            run_id.clone(),
            client_id.clone(),
            agent_id.clone(),
            agent_id.clone(),
        )
        .await
        .map_err(|_| StatusCode::CONFLICT)?;
    state.event_bus.publish(EngineEvent::new(
        "session.run.started",
        json!({
            "sessionID": session_id,
            "runID": run_id,
            "startedAtMs": active_run.started_at_ms,
            "clientID": active_run.client_id,
            "agentID": active_run.agent_id,
            "agentProfile": active_run.agent_profile,
            "environment": state.host_runtime_context(),
        }),
    ));

    let request = SendMessageRequest {
        parts: vec![MessagePartInput::Text {
            text: prompt.clone(),
        }],
        model: Some(model.clone()),
        agent: agent_id.clone().or_else(|| Some(worker_kind.to_string())),
        tool_mode: Some(tandem_types::ToolMode::Auto),
        tool_allowlist: None,
        context_mode: Some(tandem_types::ContextMode::Full),
        write_required: Some(true),
    };

    state
        .engine_loop
        .set_session_allowed_tools(
            &session_id,
            crate::normalize_allowed_tools(vec!["*".to_string()]),
        )
        .await;
    let run_result = super::sessions::execute_run(
        state.clone(),
        session_id.clone(),
        run_id.clone(),
        request,
        Some(format!("coder:{}:{worker_kind}", record.coder_run_id)),
        client_id,
    )
    .await;
    state
        .engine_loop
        .clear_session_allowed_tools(&session_id)
        .await;

    let session = state
        .storage
        .get_session(&session_id)
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let assistant_text = latest_assistant_session_text(&session);
    let tool_invocation_count = count_session_tool_invocations(&session);
    let changed_file_entries = extract_session_change_evidence(&session);
    let changed_files = changed_file_entries
        .iter()
        .filter_map(|row| {
            row.get("path")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .collect::<Vec<_>>();
    let payload = json!({
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "worker_kind": worker_kind,
        "session_id": session_id,
        "session_run_id": run_id,
        "status": if run_result.is_ok() { "completed" } else { "error" },
        "model": model,
        "agent_id": agent_id,
        "prompt": prompt,
        "assistant_text": assistant_text,
        "tool_invocation_count": tool_invocation_count,
        "changed_files": changed_files,
        "changed_file_entries": changed_file_entries,
        "message_count": session.messages.len(),
        "messages": compact_session_messages(&session),
        "error": run_result.as_ref().err().map(|error| crate::truncate_text(&error.to_string(), 500)),
        "created_at_ms": crate::now_ms(),
    });
    let artifact = write_coder_artifact(
        state,
        &record.linked_context_run_id,
        &format!("{worker_kind}-worker-session-{}", Uuid::new_v4().simple()),
        artifact_type,
        relative_path,
        &payload,
    )
    .await?;
    publish_coder_artifact_added(state, record, &artifact, Some("analysis"), {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("worker_session"));
        if let Some(session_id) = payload.get("session_id").cloned() {
            extra.insert("session_id".to_string(), session_id);
        }
        if let Some(session_run_id) = payload.get("session_run_id").cloned() {
            extra.insert("session_run_id".to_string(), session_run_id);
        }
        extra.insert("worker_kind".to_string(), json!(worker_kind));
        extra
    });

    if run_result.is_err() {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    Ok((artifact, payload))
}

async fn run_issue_fix_prepare_worker(
    state: &AppState,
    record: &CoderRunRecord,
    run: &ContextRunState,
) -> Result<(ContextBlackboardArtifact, Value), StatusCode> {
    let prompt = build_issue_fix_worker_prompt(
        record,
        run,
        &summarize_workflow_memory_hits(record, run, "retrieve_memory"),
    );
    run_issue_fix_worker_session(
        state,
        record,
        prompt,
        "issue_fix_prepare",
        "coder_issue_fix_worker_session",
        "artifacts/issue_fix.worker_session.json",
    )
    .await
}

fn build_issue_fix_validation_worker_prompt(
    record: &CoderRunRecord,
    run: &ContextRunState,
    plan_payload: Option<&Value>,
    memory_hits_used: &[String],
) -> String {
    let issue_number = record
        .github_ref
        .as_ref()
        .map(|row| row.number)
        .unwrap_or_default();
    let plan_summary = plan_payload
        .and_then(|payload| payload.get("summary"))
        .and_then(Value::as_str)
        .unwrap_or("No structured fix summary was recorded.");
    let fix_strategy = plan_payload
        .and_then(|payload| payload.get("fix_strategy"))
        .and_then(Value::as_str)
        .unwrap_or("No fix strategy was recorded.");
    let validation_hints = plan_payload
        .and_then(|payload| payload.get("validation_steps"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "no explicit validation hints".to_string());
    let memory_hint = if memory_hits_used.is_empty() {
        "none".to_string()
    } else {
        memory_hits_used.join(", ")
    };
    format!(
        concat!(
            "You are the Tandem coder issue-fix validation worker.\n",
            "Repository: {repo_slug}\n",
            "Workspace root: {workspace_root}\n",
            "Issue number: #{issue_number}\n",
            "Context run ID: {context_run_id}\n",
            "Fix plan summary: {plan_summary}\n",
            "Fix strategy: {fix_strategy}\n",
            "Validation hints: {validation_hints}\n",
            "Memory hits already surfaced: {memory_hint}\n\n",
            "Task:\n",
            "1. Inspect the current workspace state.\n",
            "2. Run or describe targeted validation for the proposed fix.\n",
            "3. Report residual risks or follow-up work.\n\n",
            "Return a compact response with these headings:\n",
            "Summary:\n",
            "Validation:\n",
            "Risks:\n"
        ),
        repo_slug = record.repo_binding.repo_slug,
        workspace_root = record.repo_binding.workspace_root,
        issue_number = issue_number,
        context_run_id = run.run_id,
        plan_summary = plan_summary,
        fix_strategy = fix_strategy,
        validation_hints = validation_hints,
        memory_hint = memory_hint,
    )
}

async fn run_issue_fix_validation_worker(
    state: &AppState,
    record: &CoderRunRecord,
    run: &ContextRunState,
    plan_payload: Option<&Value>,
) -> Result<(ContextBlackboardArtifact, Value), StatusCode> {
    let prompt = build_issue_fix_validation_worker_prompt(
        record,
        run,
        plan_payload,
        &summarize_workflow_memory_hits(record, run, "retrieve_memory"),
    );
    run_issue_fix_worker_session(
        state,
        record,
        prompt,
        "issue_fix_validation",
        "coder_issue_fix_validation_session",
        "artifacts/issue_fix.validation_session.json",
    )
    .await
}

async fn run_pr_review_worker(
    state: &AppState,
    record: &CoderRunRecord,
    run: &ContextRunState,
) -> Result<(ContextBlackboardArtifact, Value), StatusCode> {
    let prompt = build_pr_review_worker_prompt(
        record,
        run,
        &summarize_workflow_memory_hits(record, run, "retrieve_memory"),
    );
    run_issue_fix_worker_session(
        state,
        record,
        prompt,
        "pr_review_analysis",
        "coder_pr_review_worker_session",
        "artifacts/pr_review.worker_session.json",
    )
    .await
}

async fn run_merge_recommendation_worker(
    state: &AppState,
    record: &CoderRunRecord,
    run: &ContextRunState,
) -> Result<(ContextBlackboardArtifact, Value), StatusCode> {
    let prompt = build_merge_recommendation_worker_prompt(
        record,
        run,
        &summarize_workflow_memory_hits(record, run, "retrieve_memory"),
    );
    run_issue_fix_worker_session(
        state,
        record,
        prompt,
        "merge_recommendation_analysis",
        "coder_merge_recommendation_worker_session",
        "artifacts/merge_recommendation.worker_session.json",
    )
    .await
}

fn coder_run_payload(record: &CoderRunRecord, context_run: &ContextRunState) -> Value {
    json!({
        "coder_run_id": record.coder_run_id,
        "workflow_mode": record.workflow_mode,
        "linked_context_run_id": record.linked_context_run_id,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "source_client": record.source_client,
        "model_provider": record.model_provider,
        "model_id": record.model_id,
        "parent_coder_run_id": record.parent_coder_run_id,
        "origin": record.origin,
        "origin_artifact_type": record.origin_artifact_type,
        "status": context_run.status,
        "phase": project_coder_phase(context_run),
        "created_at_ms": record.created_at_ms,
        "updated_at_ms": context_run.updated_at_ms,
    })
}

pub(super) async fn coder_run_create(
    State(state): State<AppState>,
    Json(input): Json<CoderRunCreateInput>,
) -> Result<Response, StatusCode> {
    if input.repo_binding.project_id.trim().is_empty()
        || input.repo_binding.workspace_id.trim().is_empty()
        || input.repo_binding.workspace_root.trim().is_empty()
        || input.repo_binding.repo_slug.trim().is_empty()
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    if matches!(input.workflow_mode, CoderWorkflowMode::IssueTriage)
        && !matches!(
            input.github_ref.as_ref().map(|row| &row.kind),
            Some(CoderGithubRefKind::Issue)
        )
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    if matches!(input.workflow_mode, CoderWorkflowMode::IssueFix)
        && !matches!(
            input.github_ref.as_ref().map(|row| &row.kind),
            Some(CoderGithubRefKind::Issue)
        )
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    if matches!(input.workflow_mode, CoderWorkflowMode::PrReview)
        && !matches!(
            input.github_ref.as_ref().map(|row| &row.kind),
            Some(CoderGithubRefKind::PullRequest)
        )
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    if matches!(input.workflow_mode, CoderWorkflowMode::MergeRecommendation)
        && !matches!(
            input.github_ref.as_ref().map(|row| &row.kind),
            Some(CoderGithubRefKind::PullRequest)
        )
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    if matches!(
        input.workflow_mode,
        CoderWorkflowMode::IssueTriage | CoderWorkflowMode::IssueFix
    ) {
        let readiness = coder_issue_triage_readiness(&state, &input).await?;
        if !readiness.runnable {
            return Ok((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": if matches!(input.workflow_mode, CoderWorkflowMode::IssueFix) {
                        "Coder issue fix is not ready to run"
                    } else {
                        "Coder issue triage is not ready to run"
                    },
                    "code": "CODER_READINESS_BLOCKED",
                    "readiness": readiness,
                })),
            )
                .into_response());
        }
    }
    if matches!(input.workflow_mode, CoderWorkflowMode::PrReview) {
        let readiness = coder_pr_review_readiness(&state, &input).await?;
        if !readiness.runnable {
            return Ok((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "Coder PR review is not ready to run",
                    "code": "CODER_READINESS_BLOCKED",
                    "readiness": readiness,
                })),
            )
                .into_response());
        }
    }
    if matches!(input.workflow_mode, CoderWorkflowMode::MergeRecommendation) {
        let readiness = coder_merge_recommendation_readiness(&state, &input).await?;
        if !readiness.runnable {
            return Ok((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "Coder merge recommendation is not ready to run",
                    "code": "CODER_READINESS_BLOCKED",
                    "readiness": readiness,
                })),
            )
                .into_response());
        }
    }

    let now = crate::now_ms();
    let coder_run_id = input
        .coder_run_id
        .clone()
        .unwrap_or_else(|| format!("coder-{}", Uuid::new_v4().simple()));
    let linked_context_run_id = format!("ctx-{coder_run_id}");
    let create_input = ContextRunCreateInput {
        run_id: Some(linked_context_run_id.clone()),
        objective: match input.workflow_mode {
            CoderWorkflowMode::IssueTriage => compose_issue_triage_objective(&input),
            CoderWorkflowMode::IssueFix => compose_issue_fix_objective(&input),
            CoderWorkflowMode::PrReview => compose_pr_review_objective(&input),
            CoderWorkflowMode::MergeRecommendation => {
                compose_merge_recommendation_objective(&input)
            }
        },
        run_type: Some(input.workflow_mode.as_context_run_type().to_string()),
        workspace: Some(derive_workspace(&input)),
        source_client: normalize_source_client(input.source_client.as_deref())
            .or_else(|| Some("coder_api".to_string())),
        model_provider: normalize_source_client(input.model_provider.as_deref()),
        model_id: normalize_source_client(input.model_id.as_deref()),
        mcp_servers: input.mcp_servers.clone(),
    };
    let created = context_run_create(State(state.clone()), Json(create_input)).await?;
    let _context_run: ContextRunState =
        serde_json::from_value(created.0.get("run").cloned().unwrap_or_default())
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut record = CoderRunRecord {
        coder_run_id: coder_run_id.clone(),
        workflow_mode: input.workflow_mode.clone(),
        linked_context_run_id: linked_context_run_id.clone(),
        repo_binding: input.repo_binding,
        github_ref: input.github_ref,
        source_client: normalize_source_client(input.source_client.as_deref())
            .or_else(|| Some("coder_api".to_string())),
        model_provider: normalize_source_client(input.model_provider.as_deref()),
        model_id: normalize_source_client(input.model_id.as_deref()),
        parent_coder_run_id: input.parent_coder_run_id,
        origin: normalize_source_client(input.origin.as_deref()),
        origin_artifact_type: normalize_source_client(input.origin_artifact_type.as_deref()),
        created_at_ms: now,
        updated_at_ms: now,
    };
    save_coder_run_record(&state, &record).await?;

    match record.workflow_mode {
        CoderWorkflowMode::IssueTriage => {
            seed_issue_triage_tasks(state.clone(), &record).await?;
            let memory_query = format!(
                "{} issue #{}",
                record.repo_binding.repo_slug,
                record
                    .github_ref
                    .as_ref()
                    .map(|row| row.number)
                    .unwrap_or_default()
            );
            let memory_hits =
                collect_issue_triage_memory_hits(&state, &record, &memory_query, 8).await?;
            let duplicate_matches = derive_failure_pattern_duplicate_matches(&memory_hits, None, 3);
            let artifact_id = format!("memory-hits-{}", Uuid::new_v4().simple());
            let payload = json!({
                "coder_run_id": record.coder_run_id,
                "linked_context_run_id": record.linked_context_run_id,
                "query": memory_query,
                "hits": memory_hits,
                "duplicate_candidates": duplicate_matches,
                "created_at_ms": crate::now_ms(),
            });
            let artifact = write_coder_artifact(
                &state,
                &record.linked_context_run_id,
                &artifact_id,
                "coder_memory_hits",
                "artifacts/memory_hits.json",
                &payload,
            )
            .await?;
            publish_coder_artifact_added(&state, &record, &artifact, Some("memory_retrieval"), {
                let mut extra = serde_json::Map::new();
                extra.insert("kind".to_string(), json!("memory_hits"));
                extra.insert("query".to_string(), json!(memory_query));
                extra
            });
            if !duplicate_matches.is_empty() {
                let duplicate_artifact = write_coder_artifact(
                    &state,
                    &record.linked_context_run_id,
                    &format!("duplicate-matches-{}", Uuid::new_v4().simple()),
                    "coder_duplicate_matches",
                    "artifacts/duplicate_matches.json",
                    &json!({
                        "coder_run_id": record.coder_run_id,
                        "linked_context_run_id": record.linked_context_run_id,
                        "query": memory_query,
                        "matches": duplicate_matches,
                        "created_at_ms": crate::now_ms(),
                    }),
                )
                .await?;
                publish_coder_artifact_added(
                    &state,
                    &record,
                    &duplicate_artifact,
                    Some("memory_retrieval"),
                    {
                        let mut extra = serde_json::Map::new();
                        extra.insert("kind".to_string(), json!("duplicate_matches"));
                        extra.insert("query".to_string(), json!(memory_query));
                        extra
                    },
                );
            }
            let run = bootstrap_coder_workflow_run(
                &state,
                &record,
                &["ingest_reference", "retrieve_memory"],
                &["inspect_repo"],
                "Inspect the repo, then attempt reproduction.",
            )
            .await?;
            record.updated_at_ms = run.updated_at_ms;
            save_coder_run_record(&state, &record).await?;
        }
        CoderWorkflowMode::IssueFix => {
            seed_issue_fix_tasks(state.clone(), &record).await?;
            let memory_query = default_coder_memory_query(&record);
            let memory_hits =
                collect_issue_triage_memory_hits(&state, &record, &memory_query, 8).await?;
            let artifact = write_coder_artifact(
                &state,
                &record.linked_context_run_id,
                &format!("issue-fix-memory-hits-{}", Uuid::new_v4().simple()),
                "coder_memory_hits",
                "artifacts/memory_hits.json",
                &json!({
                    "coder_run_id": record.coder_run_id,
                    "linked_context_run_id": record.linked_context_run_id,
                    "query": memory_query,
                    "hits": memory_hits,
                    "created_at_ms": crate::now_ms(),
                }),
            )
            .await?;
            publish_coder_artifact_added(&state, &record, &artifact, Some("memory_retrieval"), {
                let mut extra = serde_json::Map::new();
                extra.insert("kind".to_string(), json!("memory_hits"));
                extra.insert(
                    "query".to_string(),
                    json!(default_coder_memory_query(&record)),
                );
                extra
            });
            let run = bootstrap_coder_workflow_run(
                &state,
                &record,
                &["retrieve_memory"],
                &[],
                "Inspect the issue context, then prepare and validate a constrained patch.",
            )
            .await?;
            record.updated_at_ms = run.updated_at_ms;
            save_coder_run_record(&state, &record).await?;
        }
        CoderWorkflowMode::PrReview => {
            seed_pr_review_tasks(state.clone(), &record).await?;
            let memory_query = default_coder_memory_query(&record);
            let memory_hits =
                collect_issue_triage_memory_hits(&state, &record, &memory_query, 8).await?;
            let artifact = write_coder_artifact(
                &state,
                &record.linked_context_run_id,
                &format!("pr-review-memory-hits-{}", Uuid::new_v4().simple()),
                "coder_memory_hits",
                "artifacts/memory_hits.json",
                &json!({
                    "coder_run_id": record.coder_run_id,
                    "linked_context_run_id": record.linked_context_run_id,
                    "query": memory_query,
                    "hits": memory_hits,
                    "created_at_ms": crate::now_ms(),
                }),
            )
            .await?;
            publish_coder_artifact_added(&state, &record, &artifact, Some("memory_retrieval"), {
                let mut extra = serde_json::Map::new();
                extra.insert("kind".to_string(), json!("memory_hits"));
                extra.insert(
                    "query".to_string(),
                    json!(default_coder_memory_query(&record)),
                );
                extra
            });
            let run = bootstrap_coder_workflow_run(
                &state,
                &record,
                &["retrieve_memory"],
                &[],
                "Inspect the pull request, then analyze risk and requested changes.",
            )
            .await?;
            record.updated_at_ms = run.updated_at_ms;
            save_coder_run_record(&state, &record).await?;
        }
        CoderWorkflowMode::MergeRecommendation => {
            seed_merge_recommendation_tasks(state.clone(), &record).await?;
            let memory_query = default_coder_memory_query(&record);
            let memory_hits =
                collect_issue_triage_memory_hits(&state, &record, &memory_query, 8).await?;
            let artifact = write_coder_artifact(
                &state,
                &record.linked_context_run_id,
                &format!(
                    "merge-recommendation-memory-hits-{}",
                    Uuid::new_v4().simple()
                ),
                "coder_memory_hits",
                "artifacts/memory_hits.json",
                &json!({
                    "coder_run_id": record.coder_run_id,
                    "linked_context_run_id": record.linked_context_run_id,
                    "query": memory_query,
                    "hits": memory_hits,
                    "created_at_ms": crate::now_ms(),
                }),
            )
            .await?;
            publish_coder_artifact_added(&state, &record, &artifact, Some("memory_retrieval"), {
                let mut extra = serde_json::Map::new();
                extra.insert("kind".to_string(), json!("memory_hits"));
                extra.insert(
                    "query".to_string(),
                    json!(default_coder_memory_query(&record)),
                );
                extra
            });
            let run = bootstrap_coder_workflow_run(
                &state,
                &record,
                &["retrieve_memory"],
                &[],
                "Inspect the pull request, then assess merge readiness.",
            )
            .await?;
            record.updated_at_ms = run.updated_at_ms;
            save_coder_run_record(&state, &record).await?;
        }
    }

    let final_run = load_context_run_state(&state, &linked_context_run_id).await?;
    publish_coder_run_event(
        &state,
        "coder.run.created",
        &record,
        Some(project_coder_phase(&final_run)),
        serde_json::Map::new(),
    );

    Ok(Json(json!({
        "ok": true,
        "coder_run": coder_run_payload(&record, &final_run),
        "run": final_run,
    }))
    .into_response())
}

pub(super) async fn coder_run_list(
    State(state): State<AppState>,
    Query(query): Query<CoderRunListQuery>,
) -> Result<Json<Value>, StatusCode> {
    ensure_coder_runs_dir(&state).await?;
    let mut rows = Vec::<Value>::new();
    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let mut dir = tokio::fs::read_dir(coder_runs_root(&state))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    while let Ok(Some(entry)) = dir.next_entry().await {
        if !entry
            .file_type()
            .await
            .map(|row| row.is_file())
            .unwrap_or(false)
        {
            continue;
        }
        let raw = tokio::fs::read_to_string(entry.path())
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let Ok(record) = serde_json::from_str::<CoderRunRecord>(&raw) else {
            continue;
        };
        if query
            .workflow_mode
            .as_ref()
            .is_some_and(|mode| mode != &record.workflow_mode)
        {
            continue;
        }
        if query
            .repo_slug
            .as_deref()
            .map(str::trim)
            .filter(|row| !row.is_empty())
            .is_some_and(|repo_slug| repo_slug != record.repo_binding.repo_slug)
        {
            continue;
        }
        let Ok(run) = load_context_run_state(&state, &record.linked_context_run_id).await else {
            continue;
        };
        rows.push(coder_run_payload(&record, &run));
    }
    rows.sort_by(|a, b| {
        b.get("updated_at_ms")
            .and_then(Value::as_u64)
            .cmp(&a.get("updated_at_ms").and_then(Value::as_u64))
    });
    rows.truncate(limit);
    Ok(Json(json!({ "runs": rows })))
}

pub(super) async fn coder_run_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let record = load_coder_run_record(&state, &id).await?;
    let run = load_context_run_state(&state, &record.linked_context_run_id).await?;
    let blackboard = load_context_blackboard(&state, &record.linked_context_run_id);
    let memory_query = default_coder_memory_query(&record);
    let memory_hits = if matches!(
        record.workflow_mode,
        CoderWorkflowMode::IssueTriage
            | CoderWorkflowMode::IssueFix
            | CoderWorkflowMode::PrReview
            | CoderWorkflowMode::MergeRecommendation
    ) {
        collect_issue_triage_memory_hits(&state, &record, &memory_query, 8).await?
    } else {
        Vec::new()
    };
    let memory_candidates = list_repo_memory_candidates(
        &state,
        &record.repo_binding.repo_slug,
        record.github_ref.as_ref(),
        20,
    )
    .await?;
    Ok(Json(json!({
        "coder_run": coder_run_payload(&record, &run),
        "run": run,
        "artifacts": blackboard.artifacts,
        "memory_hits": {
            "query": memory_query,
            "hits": memory_hits,
        },
        "memory_candidates": memory_candidates,
    })))
}

async fn execute_coder_run_step(
    state: AppState,
    record: &mut CoderRunRecord,
    agent_id: &str,
) -> Result<Value, StatusCode> {
    if !matches!(
        record.workflow_mode,
        CoderWorkflowMode::IssueTriage
            | CoderWorkflowMode::IssueFix
            | CoderWorkflowMode::PrReview
            | CoderWorkflowMode::MergeRecommendation
    ) {
        return Ok(json!({
            "ok": false,
            "error": "execute_next is only wired for issue_triage, issue_fix, pr_review, and merge_recommendation right now",
            "code": "CODER_EXECUTION_UNSUPPORTED",
        }));
    }
    let claimed_task = claim_next_context_task(
        &state,
        &record.linked_context_run_id,
        agent_id,
        None,
        Some(record.workflow_mode.as_context_run_type()),
        Some(30_000),
        Some(format!(
            "coder:{}:execute-next:{}",
            record.coder_run_id,
            Uuid::new_v4().simple()
        )),
    )
    .await?;
    let Some(task) = claimed_task else {
        let run = load_context_run_state(&state, &record.linked_context_run_id).await?;
        return Ok(json!({
            "ok": true,
            "task": Value::Null,
            "run": run,
            "coder_run": coder_run_payload(record, &run),
            "dispatched": false,
            "reason": "no runnable coder task was available"
        }));
    };

    publish_coder_run_event(
        &state,
        "coder.run.phase_changed",
        record,
        Some(project_coder_phase(
            &load_context_run_state(&state, &record.linked_context_run_id).await?,
        )),
        {
            let mut extra = serde_json::Map::new();
            extra.insert("event_type".to_string(), json!("worker_task_claimed"));
            extra.insert("task_id".to_string(), json!(task.id.clone()));
            extra.insert(
                "workflow_node_id".to_string(),
                json!(task.workflow_node_id.clone()),
            );
            extra.insert("agent_id".to_string(), json!(agent_id));
            extra
        },
    );

    let dispatched = match record.workflow_mode {
        CoderWorkflowMode::IssueTriage => {
            dispatch_issue_triage_task(state.clone(), record, &task, agent_id).await?
        }
        CoderWorkflowMode::IssueFix => {
            dispatch_issue_fix_task(state.clone(), record, &task, agent_id).await?
        }
        CoderWorkflowMode::PrReview => {
            dispatch_pr_review_task(state.clone(), record, &task).await?
        }
        CoderWorkflowMode::MergeRecommendation => {
            dispatch_merge_recommendation_task(state.clone(), record, &task).await?
        }
    };
    let final_run = load_context_run_state(&state, &record.linked_context_run_id).await?;
    record.updated_at_ms = final_run.updated_at_ms;
    save_coder_run_record(&state, &record).await?;
    Ok(json!({
        "ok": true,
        "task": task,
        "dispatched": true,
        "dispatch_result": dispatched,
        "run": final_run,
        "coder_run": coder_run_payload(record, &final_run),
    }))
}

pub(super) async fn coder_run_execute_next(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderRunExecuteNextInput>,
) -> Result<Json<Value>, StatusCode> {
    let mut record = load_coder_run_record(&state, &id).await?;
    let agent_id = default_coder_worker_agent_id(input.agent_id.as_deref());
    Ok(Json(
        execute_coder_run_step(state, &mut record, &agent_id).await?,
    ))
}

pub(super) async fn coder_run_execute_all(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderRunExecuteAllInput>,
) -> Result<Json<Value>, StatusCode> {
    let mut record = load_coder_run_record(&state, &id).await?;
    let agent_id = default_coder_worker_agent_id(input.agent_id.as_deref());
    let max_steps = input.max_steps.unwrap_or(16).clamp(1, 64);
    let mut steps = Vec::<Value>::new();
    let mut stopped_reason = "max_steps_reached".to_string();

    for _ in 0..max_steps {
        let step = execute_coder_run_step(state.clone(), &mut record, &agent_id).await?;
        let no_task = step.get("task").is_none_or(Value::is_null);
        let run_status = step
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        steps.push(step);
        if no_task {
            stopped_reason = "no_runnable_task".to_string();
            break;
        }
        if matches!(run_status.as_str(), "completed" | "failed" | "cancelled") {
            stopped_reason = format!("run_{run_status}");
            break;
        }
    }

    let final_run = load_context_run_state(&state, &record.linked_context_run_id).await?;
    record.updated_at_ms = final_run.updated_at_ms;
    save_coder_run_record(&state, &record).await?;
    Ok(Json(json!({
        "ok": true,
        "executed_steps": steps
            .iter()
            .filter(|row| row.get("task").is_some_and(|task| !task.is_null()))
            .count(),
        "steps": steps,
        "stopped_reason": stopped_reason,
        "run": final_run,
        "coder_run": coder_run_payload(&record, &final_run),
    })))
}

async fn coder_run_transition(
    state: &AppState,
    record: &CoderRunRecord,
    event_type: &str,
    status: ContextRunStatus,
    reason: Option<String>,
) -> Result<Value, StatusCode> {
    let outcome = context_run_engine()
        .commit_run_event(
            state,
            &record.linked_context_run_id,
            ContextRunEventAppendInput {
                event_type: event_type.to_string(),
                status,
                step_id: None,
                payload: json!({
                    "why_next_step": reason,
                }),
            },
            None,
        )
        .await?;
    let run = load_context_run_state(state, &record.linked_context_run_id).await?;
    publish_coder_run_event(
        state,
        "coder.run.phase_changed",
        record,
        Some(project_coder_phase(&run)),
        {
            let mut extra = serde_json::Map::new();
            extra.insert("status".to_string(), json!(run.status));
            extra.insert("event_type".to_string(), json!(event_type));
            extra
        },
    );
    Ok(json!({
        "ok": true,
        "event": outcome.event,
        "coder_run": coder_run_payload(record, &run),
        "run": run,
    }))
}

pub(super) async fn coder_run_approve(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderRunControlInput>,
) -> Result<Json<Value>, StatusCode> {
    let record = load_coder_run_record(&state, &id).await?;
    let run = load_context_run_state(&state, &record.linked_context_run_id).await?;
    if !matches!(run.status, ContextRunStatus::AwaitingApproval) {
        return Ok(Json(json!({
            "ok": false,
            "error": "coder run is not awaiting approval",
            "code": "CODER_NOT_AWAITING_APPROVAL"
        })));
    }
    let why = input
        .reason
        .unwrap_or_else(|| "plan approved by operator".to_string());
    Ok(Json(
        coder_run_transition(
            &state,
            &record,
            "plan_approved",
            ContextRunStatus::Running,
            Some(why),
        )
        .await?,
    ))
}

pub(super) async fn coder_run_cancel(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderRunControlInput>,
) -> Result<Json<Value>, StatusCode> {
    let record = load_coder_run_record(&state, &id).await?;
    let why = input
        .reason
        .unwrap_or_else(|| "run cancelled by operator".to_string());
    Ok(Json(
        coder_run_transition(
            &state,
            &record,
            "run_cancelled",
            ContextRunStatus::Cancelled,
            Some(why),
        )
        .await?,
    ))
}

pub(super) async fn coder_run_artifacts(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let record = load_coder_run_record(&state, &id).await?;
    let blackboard = load_context_blackboard(&state, &record.linked_context_run_id);
    Ok(Json(json!({
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "artifacts": blackboard.artifacts,
    })))
}

pub(super) async fn coder_memory_hits_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<CoderMemoryHitsQuery>,
) -> Result<Json<Value>, StatusCode> {
    let record = load_coder_run_record(&state, &id).await?;
    let search_query = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|row| !row.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| default_coder_memory_query(&record));
    let hits =
        collect_issue_triage_memory_hits(&state, &record, &search_query, query.limit.unwrap_or(8))
            .await?;
    Ok(Json(json!({
        "coder_run_id": record.coder_run_id,
        "query": search_query,
        "hits": hits,
    })))
}

pub(super) async fn coder_memory_candidate_list(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let record = load_coder_run_record(&state, &id).await?;
    let candidates = list_repo_memory_candidates(
        &state,
        &record.repo_binding.repo_slug,
        record.github_ref.as_ref(),
        20,
    )
    .await?;
    Ok(Json(json!({
        "coder_run_id": record.coder_run_id,
        "candidates": candidates,
    })))
}

pub(super) async fn coder_memory_candidate_create(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderMemoryCandidateCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    let record = load_coder_run_record(&state, &id).await?;
    if !matches!(record.workflow_mode, CoderWorkflowMode::IssueTriage) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let (candidate_id, artifact) = write_coder_memory_candidate_artifact(
        &state,
        &record,
        input.kind,
        input.summary,
        input.task_id,
        input.payload,
    )
    .await?;
    Ok(Json(json!({
        "ok": true,
        "candidate_id": candidate_id,
        "artifact": artifact,
    })))
}

pub(super) async fn coder_memory_candidate_promote(
    State(state): State<AppState>,
    Path((id, candidate_id)): Path<(String, String)>,
    Json(input): Json<CoderMemoryCandidatePromoteInput>,
) -> Result<Json<Value>, StatusCode> {
    let record = load_coder_run_record(&state, &id).await?;
    let candidate_payload =
        load_coder_memory_candidate_payload(&state, &record, &candidate_id).await?;
    let kind: CoderMemoryCandidateKind = serde_json::from_value(
        candidate_payload
            .get("kind")
            .cloned()
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?,
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let content =
        build_governed_memory_content(&candidate_payload).ok_or(StatusCode::BAD_REQUEST)?;
    let to_tier = input.to_tier.unwrap_or(GovernedMemoryTier::Project);
    let session_partition = coder_memory_partition(&record, GovernedMemoryTier::Session);
    let capability = super::skills_memory::issue_run_memory_capability(
        &record.linked_context_run_id,
        record.source_client.as_deref(),
        &session_partition,
        super::skills_memory::RunMemoryCapabilityPolicy::CoderWorkflow,
    );
    let artifact_refs = vec![format!(
        "context_run:{}/coder_memory/{}.json",
        record.linked_context_run_id, candidate_id
    )];
    let put_response = super::skills_memory::memory_put_impl(
        &state,
        MemoryPutRequest {
            run_id: record.linked_context_run_id.clone(),
            partition: session_partition.clone(),
            kind: match kind {
                CoderMemoryCandidateKind::TriageMemory => MemoryContentKind::SolutionCapsule,
                CoderMemoryCandidateKind::FixPattern => MemoryContentKind::SolutionCapsule,
                CoderMemoryCandidateKind::ValidationMemory => MemoryContentKind::Fact,
                CoderMemoryCandidateKind::ReviewMemory => MemoryContentKind::SolutionCapsule,
                CoderMemoryCandidateKind::MergeRecommendationMemory => {
                    MemoryContentKind::SolutionCapsule
                }
                CoderMemoryCandidateKind::RegressionSignal => MemoryContentKind::Fact,
                CoderMemoryCandidateKind::FailurePattern => MemoryContentKind::Fact,
                CoderMemoryCandidateKind::RunOutcome => MemoryContentKind::Note,
            },
            content,
            artifact_refs: artifact_refs.clone(),
            classification: MemoryClassification::Internal,
            metadata: Some(json!({
                "kind": kind,
                "candidate_id": candidate_id,
                "coder_run_id": record.coder_run_id,
                "workflow_mode": record.workflow_mode,
                "repo_slug": record.repo_binding.repo_slug,
                "github_ref": record.github_ref,
                "failure_pattern_fingerprint": candidate_payload
                    .get("payload")
                    .and_then(|row| row.get("fingerprint"))
                    .cloned()
                    .unwrap_or(Value::Null),
                "linked_issue_numbers": candidate_payload
                    .get("payload")
                    .and_then(|row| row.get("linked_issue_numbers"))
                    .cloned()
                    .unwrap_or_else(|| json!([])),
            })),
        },
        Some(capability.clone()),
    )
    .await?;
    let promote_response =
        if input.approval_id.as_deref().is_some() && input.reviewer_id.as_deref().is_some() {
            Some(
                super::skills_memory::memory_promote_impl(
                    &state,
                    MemoryPromoteRequest {
                        run_id: record.linked_context_run_id.clone(),
                        source_memory_id: put_response.id.clone(),
                        from_tier: GovernedMemoryTier::Session,
                        to_tier,
                        partition: session_partition.clone(),
                        reason: input
                            .reason
                            .clone()
                            .unwrap_or_else(|| "approved reusable coder memory".to_string()),
                        review: PromotionReview {
                            required: true,
                            reviewer_id: input.reviewer_id.clone(),
                            approval_id: input.approval_id.clone(),
                        },
                    },
                    Some(capability),
                )
                .await?,
            )
        } else {
            None
        };
    let promoted = promote_response
        .as_ref()
        .map(|row| row.promoted)
        .unwrap_or(false);
    let artifact = write_coder_artifact(
        &state,
        &record.linked_context_run_id,
        &format!("memstore-{candidate_id}"),
        "coder_memory_promotion",
        &format!("artifacts/memory_promotions/{candidate_id}.json"),
        &json!({
            "candidate_id": candidate_id,
            "memory_id": put_response.id,
            "stored": put_response.stored,
            "deduped": false,
            "promoted": promoted,
            "to_tier": to_tier,
            "reviewer_id": input.reviewer_id,
            "approval_id": input.approval_id,
            "promotion": promote_response,
            "artifact_refs": artifact_refs,
        }),
    )
    .await?;
    publish_coder_artifact_added(&state, &record, &artifact, Some("artifact_write"), {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("memory_promotion"));
        extra.insert("candidate_id".to_string(), json!(candidate_id));
        extra.insert("memory_id".to_string(), json!(put_response.id));
        extra
    });
    publish_coder_run_event(
        &state,
        "coder.memory.promoted",
        &record,
        Some("artifact_write"),
        {
            let mut extra = coder_artifact_event_fields(&artifact, Some("memory_promotion"));
            extra.insert("candidate_id".to_string(), json!(candidate_id));
            extra.insert("memory_id".to_string(), json!(put_response.id));
            extra.insert("promoted".to_string(), json!(promoted));
            extra.insert("to_tier".to_string(), json!(to_tier));
            extra
        },
    );
    Ok(Json(json!({
        "ok": true,
        "memory_id": put_response.id,
        "stored": put_response.stored,
        "deduped": false,
        "promoted": promoted,
        "to_tier": to_tier,
        "promotion": promote_response,
        "artifact": artifact,
    })))
}

pub(super) async fn coder_triage_summary_create(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderTriageSummaryCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    let mut record = load_coder_run_record(&state, &id).await?;
    if !matches!(record.workflow_mode, CoderWorkflowMode::IssueTriage) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let summary_id = format!("triage-summary-{}", Uuid::new_v4().simple());
    let payload = json!({
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "summary": input.summary,
        "confidence": input.confidence,
        "affected_files": input.affected_files,
        "duplicate_candidates": input.duplicate_candidates,
        "memory_hits_used": input.memory_hits_used,
        "reproduction": input.reproduction,
        "notes": input.notes,
        "created_at_ms": crate::now_ms(),
    });
    let artifact = write_coder_artifact(
        &state,
        &record.linked_context_run_id,
        &summary_id,
        "coder_triage_summary",
        "artifacts/triage.summary.json",
        &payload,
    )
    .await?;
    publish_coder_artifact_added(&state, &record, &artifact, Some("artifact_write"), {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("triage_summary"));
        extra
    });
    let triage_summary = input
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|row| !row.is_empty())
        .map(ToString::to_string);
    let mut generated_candidates = Vec::<Value>::new();
    if let Some(summary_text) = triage_summary.clone() {
        let (triage_memory_id, triage_memory_artifact) = write_coder_memory_candidate_artifact(
            &state,
            &record,
            CoderMemoryCandidateKind::TriageMemory,
            Some(summary_text.clone()),
            Some("write_triage_artifact".to_string()),
            json!({
                "summary": summary_text,
                "confidence": input.confidence,
                "affected_files": input.affected_files,
                "duplicate_candidates": input.duplicate_candidates,
                "memory_hits_used": input.memory_hits_used,
                "reproduction": input.reproduction,
                "notes": input.notes,
                "summary_artifact_path": artifact.path,
            }),
        )
        .await?;
        generated_candidates.push(json!({
            "candidate_id": triage_memory_id,
            "kind": "triage_memory",
            "artifact_path": triage_memory_artifact.path,
        }));

        let (failure_pattern_id, failure_pattern_artifact) = write_coder_memory_candidate_artifact(
            &state,
            &record,
            CoderMemoryCandidateKind::FailurePattern,
            Some(format!("Failure pattern: {summary_text}")),
            Some("write_triage_artifact".to_string()),
            build_failure_pattern_payload(
                &record,
                &artifact.path,
                &summary_text,
                &input.affected_files,
                &input.duplicate_candidates,
                input.notes.as_deref(),
            ),
        )
        .await?;
        generated_candidates.push(json!({
            "candidate_id": failure_pattern_id,
            "kind": "failure_pattern",
            "artifact_path": failure_pattern_artifact.path,
        }));

        let outcome = if input.duplicate_candidates.is_empty() {
            "triaged"
        } else {
            "triaged_duplicate_candidate"
        };
        let (run_outcome_id, run_outcome_artifact) = write_coder_memory_candidate_artifact(
            &state,
            &record,
            CoderMemoryCandidateKind::RunOutcome,
            Some(format!("Issue triage completed: {outcome}")),
            Some("write_triage_artifact".to_string()),
            json!({
                "workflow_mode": "issue_triage",
                "result": outcome,
                "summary": summary_text,
                "successful_strategies": ["memory_retrieval", "repo_inspection"],
                "validations_attempted": [{
                    "kind": "reproduction",
                    "outcome": input
                        .reproduction
                        .as_ref()
                        .and_then(|row| row.get("outcome"))
                        .cloned()
                        .unwrap_or_else(|| json!("unknown"))
                }],
                "follow_up_recommended": true,
                "follow_up_mode": "issue_fix",
                "summary_artifact_path": artifact.path,
            }),
        )
        .await?;
        generated_candidates.push(json!({
            "candidate_id": run_outcome_id,
            "kind": "run_outcome",
            "artifact_path": run_outcome_artifact.path,
        }));
    }
    let final_run = finalize_coder_workflow_run(
        &state,
        &record,
        &[
            "ingest_reference",
            "retrieve_memory",
            "inspect_repo",
            "attempt_reproduction",
            "write_triage_artifact",
        ],
        "Issue triage summary recorded.",
    )
    .await?;
    record.updated_at_ms = final_run.updated_at_ms;
    save_coder_run_record(&state, &record).await?;
    Ok(Json(json!({
        "ok": true,
        "artifact": artifact,
        "generated_candidates": generated_candidates,
        "coder_run": coder_run_payload(&record, &final_run),
        "run": final_run,
    })))
}

pub(super) async fn coder_triage_reproduction_report_create(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderTriageReproductionReportCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    let mut record = load_coder_run_record(&state, &id).await?;
    if !matches!(record.workflow_mode, CoderWorkflowMode::IssueTriage) {
        return Err(StatusCode::BAD_REQUEST);
    }
    if input
        .summary
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty()
        && input.steps.is_empty()
        && input.observed_logs.is_empty()
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let artifact_id = format!("triage-reproduction-{}", Uuid::new_v4().simple());
    let payload = json!({
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "summary": input.summary,
        "outcome": input.outcome,
        "steps": input.steps,
        "observed_logs": input.observed_logs,
        "affected_files": input.affected_files,
        "memory_hits_used": input.memory_hits_used,
        "notes": input.notes,
        "created_at_ms": crate::now_ms(),
    });
    let artifact = write_coder_artifact(
        &state,
        &record.linked_context_run_id,
        &artifact_id,
        "coder_reproduction_report",
        "artifacts/triage.reproduction.json",
        &payload,
    )
    .await?;
    publish_coder_artifact_added(&state, &record, &artifact, Some("reproduction"), {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("reproduction_report"));
        if let Some(outcome) = input.outcome.clone() {
            extra.insert("outcome".to_string(), json!(outcome));
        }
        extra
    });
    let final_run = advance_coder_workflow_run(
        &state,
        &record,
        &["inspect_repo", "attempt_reproduction"],
        &["write_triage_artifact"],
        "Write the triage summary and capture duplicate candidates.",
    )
    .await?;
    record.updated_at_ms = final_run.updated_at_ms;
    save_coder_run_record(&state, &record).await?;
    Ok(Json(json!({
        "ok": true,
        "artifact": artifact,
        "coder_run": coder_run_payload(&record, &final_run),
        "run": final_run,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_session_changed_files_reads_tool_invocations() {
        let mut session = Session::new(Some("coder test".to_string()), Some(".".to_string()));
        session.messages.push(Message::new(
            MessageRole::Assistant,
            vec![
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({
                        "path": "crates/tandem-server/src/http/coder.rs",
                        "content": "fn main() {}"
                    }),
                    result: Some(json!({"ok": true})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "edit".to_string(),
                    args: json!({
                        "files": [
                            {"path": "src/App.tsx"},
                            {"path": "src/components/View.tsx"}
                        ]
                    }),
                    result: None,
                    error: None,
                },
            ],
        ));

        let changed_files = extract_session_changed_files(&session);
        assert_eq!(
            changed_files,
            vec![
                "crates/tandem-server/src/http/coder.rs".to_string(),
                "src/App.tsx".to_string(),
                "src/components/View.tsx".to_string(),
            ]
        );
        let evidence = extract_session_change_evidence(&session);
        assert_eq!(evidence.len(), 3);
        assert_eq!(
            evidence
                .first()
                .and_then(|row| row.get("tool"))
                .and_then(Value::as_str),
            Some("write")
        );
        assert!(evidence
            .first()
            .and_then(|row| row.get("preview"))
            .and_then(Value::as_str)
            .is_some_and(|preview| preview.contains("fn main()")));
    }

    #[tokio::test]
    async fn collect_workspace_file_snapshots_reads_workspace_files() {
        let root = std::env::temp_dir().join(format!("tandem-coder-snapshots-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("src")).expect("create snapshot dir");
        std::fs::write(
            root.join("src/app.rs"),
            "fn main() {\n    println!(\"hello\");\n}\n",
        )
        .expect("write workspace file");

        let snapshots = collect_workspace_file_snapshots(
            root.to_str().expect("snapshot root"),
            &["src/app.rs".to_string(), "../escape.rs".to_string()],
        )
        .await;
        assert_eq!(snapshots.len(), 2);
        assert_eq!(
            snapshots[0].get("path").and_then(Value::as_str),
            Some("src/app.rs")
        );
        assert_eq!(
            snapshots[0].get("exists").and_then(Value::as_bool),
            Some(true)
        );
        assert!(snapshots[0]
            .get("preview")
            .and_then(Value::as_str)
            .is_some_and(|preview| preview.contains("println!")));
        assert_eq!(
            snapshots[1].get("error").and_then(Value::as_str),
            Some("invalid_relative_path")
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn extract_pull_requests_from_tool_result_reads_result_shapes() {
        let result = tandem_types::ToolResult {
            output: json!({
                "pull_request": {
                    "number": 42,
                    "title": "Fix startup recovery",
                    "state": "open",
                    "html_url": "https://github.com/evan/tandem/pull/42",
                    "head": {"ref": "coder/issue-42-fix"},
                    "base": {"ref": "main"}
                }
            })
            .to_string(),
            metadata: json!({
                "result": {
                    "number": 42,
                    "title": "Fix startup recovery",
                    "state": "open",
                    "url": "https://github.com/evan/tandem/pull/42",
                    "head_ref": "coder/issue-42-fix",
                    "base_ref": "main"
                }
            }),
        };

        let pulls = extract_pull_requests_from_tool_result(&result);
        assert_eq!(pulls.len(), 1);
        assert_eq!(pulls[0].number, 42);
        assert_eq!(pulls[0].title, "Fix startup recovery");
        assert_eq!(pulls[0].state, "open");
        assert_eq!(
            pulls[0].html_url.as_deref(),
            Some("https://github.com/evan/tandem/pull/42")
        );
        assert_eq!(pulls[0].head_ref.as_deref(), Some("coder/issue-42-fix"));
        assert_eq!(pulls[0].base_ref.as_deref(), Some("main"));
    }

    #[test]
    fn extract_pull_requests_from_tool_result_accepts_minimal_identity_shape() {
        let result = tandem_types::ToolResult {
            output: json!({
                "result": {
                    "number": 91
                }
            })
            .to_string(),
            metadata: json!({}),
        };

        let pulls = extract_pull_requests_from_tool_result(&result);
        assert_eq!(pulls.len(), 1);
        assert_eq!(pulls[0].number, 91);
        assert_eq!(pulls[0].title, "");
        assert_eq!(pulls[0].state, "");
        assert!(pulls[0].html_url.is_none());
    }

    #[test]
    fn github_ref_from_pull_request_builds_canonical_pr_ref() {
        let pull = GithubPullRequestSummary {
            number: 77,
            title: "Guard startup recovery config loading".to_string(),
            state: "open".to_string(),
            html_url: Some("https://github.com/evan/tandem/pull/77".to_string()),
            head_ref: Some("coder/issue-313-fix".to_string()),
            base_ref: Some("main".to_string()),
        };

        assert_eq!(
            github_ref_from_pull_request(&pull),
            json!({
                "kind": "pull_request",
                "number": 77,
                "url": "https://github.com/evan/tandem/pull/77",
            })
        );
    }

    #[test]
    fn normalize_follow_on_workflow_modes_adds_review_before_merge() {
        assert_eq!(
            normalize_follow_on_workflow_modes(&[CoderWorkflowMode::MergeRecommendation]),
            vec![
                CoderWorkflowMode::PrReview,
                CoderWorkflowMode::MergeRecommendation,
            ]
        );
        assert_eq!(
            normalize_follow_on_workflow_modes(&[
                CoderWorkflowMode::PrReview,
                CoderWorkflowMode::MergeRecommendation,
                CoderWorkflowMode::PrReview,
            ]),
            vec![
                CoderWorkflowMode::PrReview,
                CoderWorkflowMode::MergeRecommendation,
            ]
        );
    }

    #[test]
    fn split_auto_spawn_follow_on_workflow_modes_requires_explicit_merge_opt_in() {
        let (auto_spawn, skipped) = split_auto_spawn_follow_on_workflow_modes(
            &[CoderWorkflowMode::MergeRecommendation],
            false,
        );
        assert_eq!(auto_spawn, vec![CoderWorkflowMode::PrReview]);
        assert_eq!(skipped.len(), 1);
        assert_eq!(
            skipped[0].get("workflow_mode").and_then(Value::as_str),
            Some("merge_recommendation")
        );
        let (auto_spawn, skipped) = split_auto_spawn_follow_on_workflow_modes(
            &[CoderWorkflowMode::MergeRecommendation],
            true,
        );
        assert_eq!(
            auto_spawn,
            vec![
                CoderWorkflowMode::PrReview,
                CoderWorkflowMode::MergeRecommendation
            ]
        );
        assert!(skipped.is_empty());
    }
}

pub(super) async fn coder_triage_inspection_report_create(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderTriageInspectionReportCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    let mut record = load_coder_run_record(&state, &id).await?;
    if !matches!(record.workflow_mode, CoderWorkflowMode::IssueTriage) {
        return Err(StatusCode::BAD_REQUEST);
    }
    if input
        .summary
        .as_deref()
        .map(str::trim)
        .unwrap_or("")
        .is_empty()
        && input.likely_areas.is_empty()
        && input.affected_files.is_empty()
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let artifact_id = format!("triage-inspection-{}", Uuid::new_v4().simple());
    let payload = json!({
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "summary": input.summary,
        "likely_areas": input.likely_areas,
        "affected_files": input.affected_files,
        "memory_hits_used": input.memory_hits_used,
        "notes": input.notes,
        "created_at_ms": crate::now_ms(),
    });
    let artifact = write_coder_artifact(
        &state,
        &record.linked_context_run_id,
        &artifact_id,
        "coder_repo_inspection_report",
        "artifacts/triage.inspection.json",
        &payload,
    )
    .await?;
    publish_coder_artifact_added(&state, &record, &artifact, Some("repo_inspection"), {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("inspection_report"));
        extra
    });
    let final_run = advance_coder_workflow_run(
        &state,
        &record,
        &["inspect_repo"],
        &["attempt_reproduction"],
        "Attempt constrained reproduction using the inspected repo context.",
    )
    .await?;
    record.updated_at_ms = final_run.updated_at_ms;
    save_coder_run_record(&state, &record).await?;
    Ok(Json(json!({
        "ok": true,
        "artifact": artifact,
        "coder_run": coder_run_payload(&record, &final_run),
        "run": final_run,
    })))
}

pub(super) async fn coder_pr_review_summary_create(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderPrReviewSummaryCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    let mut record = load_coder_run_record(&state, &id).await?;
    if !matches!(record.workflow_mode, CoderWorkflowMode::PrReview) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let summary_id = format!("pr-review-summary-{}", Uuid::new_v4().simple());
    let payload = json!({
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "verdict": input.verdict,
        "summary": input.summary,
        "risk_level": input.risk_level,
        "changed_files": input.changed_files,
        "blockers": input.blockers,
        "requested_changes": input.requested_changes,
        "regression_signals": input.regression_signals,
        "memory_hits_used": input.memory_hits_used,
        "notes": input.notes,
        "created_at_ms": crate::now_ms(),
    });
    let artifact = write_coder_artifact(
        &state,
        &record.linked_context_run_id,
        &summary_id,
        "coder_pr_review_summary",
        "artifacts/pr_review.summary.json",
        &payload,
    )
    .await?;
    publish_coder_artifact_added(&state, &record, &artifact, Some("artifact_write"), {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("pr_review_summary"));
        if let Some(verdict) = input.verdict.clone() {
            extra.insert("verdict".to_string(), json!(verdict));
        }
        if let Some(risk_level) = input.risk_level.clone() {
            extra.insert("risk_level".to_string(), json!(risk_level));
        }
        extra
    });

    let review_evidence_artifact = write_pr_review_evidence_artifact(
        &state,
        &record,
        input.verdict.as_deref(),
        input.summary.as_deref(),
        input.risk_level.as_deref(),
        &input.changed_files,
        &input.blockers,
        &input.requested_changes,
        &input.regression_signals,
        &input.memory_hits_used,
        input.notes.as_deref(),
        Some(&artifact.path),
        Some("artifact_write"),
    )
    .await?;

    let mut generated_candidates = Vec::<Value>::new();
    if let Some(summary_text) = input
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|row| !row.is_empty())
        .map(ToString::to_string)
    {
        let (review_memory_id, review_memory_artifact) = write_coder_memory_candidate_artifact(
            &state,
            &record,
            CoderMemoryCandidateKind::ReviewMemory,
            Some(summary_text.clone()),
            Some("write_review_artifact".to_string()),
            json!({
                "workflow_mode": "pr_review",
                "verdict": input.verdict,
                "summary": summary_text,
                "risk_level": input.risk_level,
                "changed_files": input.changed_files,
                "blockers": input.blockers,
                "requested_changes": input.requested_changes,
                "regression_signals": input.regression_signals,
                "memory_hits_used": input.memory_hits_used,
                "summary_artifact_path": artifact.path,
                "review_evidence_artifact_path": review_evidence_artifact.as_ref().map(|row| row.path.clone()),
            }),
        )
        .await?;
        generated_candidates.push(json!({
            "candidate_id": review_memory_id,
            "kind": "review_memory",
            "artifact_path": review_memory_artifact.path,
        }));

        if !input.regression_signals.is_empty() {
            let regression_summary = format!(
                "PR review regression signals: {}",
                input
                    .regression_signals
                    .iter()
                    .filter_map(|row| {
                        row.get("summary")
                            .and_then(Value::as_str)
                            .map(str::trim)
                            .filter(|value| !value.is_empty())
                            .map(ToString::to_string)
                            .or_else(|| {
                                row.get("kind")
                                    .and_then(Value::as_str)
                                    .map(str::trim)
                                    .filter(|value| !value.is_empty())
                                    .map(ToString::to_string)
                            })
                    })
                    .take(3)
                    .collect::<Vec<_>>()
                    .join("; ")
            );
            let (regression_signal_id, regression_signal_artifact) =
                write_coder_memory_candidate_artifact(
                    &state,
                    &record,
                    CoderMemoryCandidateKind::RegressionSignal,
                    Some(regression_summary),
                    Some("write_review_artifact".to_string()),
                    json!({
                        "workflow_mode": "pr_review",
                        "verdict": input.verdict,
                        "risk_level": input.risk_level,
                        "regression_signals": input.regression_signals,
                        "memory_hits_used": input.memory_hits_used,
                        "summary_artifact_path": artifact.path,
                        "review_evidence_artifact_path": review_evidence_artifact.as_ref().map(|row| row.path.clone()),
                    }),
                )
                .await?;
            generated_candidates.push(json!({
                "candidate_id": regression_signal_id,
                "kind": "regression_signal",
                "artifact_path": regression_signal_artifact.path,
            }));
        }

        let verdict = input
            .verdict
            .as_deref()
            .map(str::trim)
            .filter(|row| !row.is_empty())
            .unwrap_or("reviewed");
        let (run_outcome_id, run_outcome_artifact) = write_coder_memory_candidate_artifact(
            &state,
            &record,
            CoderMemoryCandidateKind::RunOutcome,
            Some(format!("PR review completed: {verdict}")),
            Some("write_review_artifact".to_string()),
            json!({
                "workflow_mode": "pr_review",
                "result": verdict,
                "summary": summary_text,
                "risk_level": input.risk_level,
                "changed_files": input.changed_files,
                "blockers": input.blockers,
                "requested_changes": input.requested_changes,
                "regression_signals": input.regression_signals,
                "memory_hits_used": input.memory_hits_used,
                "summary_artifact_path": artifact.path,
                "review_evidence_artifact_path": review_evidence_artifact.as_ref().map(|row| row.path.clone()),
            }),
        )
        .await?;
        generated_candidates.push(json!({
            "candidate_id": run_outcome_id,
            "kind": "run_outcome",
            "artifact_path": run_outcome_artifact.path,
        }));
    }

    let final_run = finalize_coder_workflow_run(
        &state,
        &record,
        &[
            "inspect_pull_request",
            "retrieve_memory",
            "review_pull_request",
            "write_review_artifact",
        ],
        "PR review summary recorded.",
    )
    .await?;
    record.updated_at_ms = final_run.updated_at_ms;
    save_coder_run_record(&state, &record).await?;
    Ok(Json(json!({
        "ok": true,
        "artifact": artifact,
        "review_evidence_artifact": review_evidence_artifact,
        "generated_candidates": generated_candidates,
        "coder_run": coder_run_payload(&record, &final_run),
        "run": final_run,
    })))
}

async fn write_pr_review_evidence_artifact(
    state: &AppState,
    record: &CoderRunRecord,
    verdict: Option<&str>,
    summary: Option<&str>,
    risk_level: Option<&str>,
    changed_files: &[String],
    blockers: &[String],
    requested_changes: &[String],
    regression_signals: &[Value],
    memory_hits_used: &[String],
    notes: Option<&str>,
    summary_artifact_path: Option<&str>,
    phase: Option<&str>,
) -> Result<Option<ContextBlackboardArtifact>, StatusCode> {
    if changed_files.is_empty()
        && blockers.is_empty()
        && requested_changes.is_empty()
        && regression_signals.is_empty()
        && summary.map(str::trim).unwrap_or("").is_empty()
        && notes.map(str::trim).unwrap_or("").is_empty()
    {
        return Ok(None);
    }
    let evidence_id = format!("pr-review-evidence-{}", Uuid::new_v4().simple());
    let evidence_payload = json!({
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "verdict": verdict,
        "summary": summary,
        "risk_level": risk_level,
        "changed_files": changed_files,
        "blockers": blockers,
        "requested_changes": requested_changes,
        "regression_signals": regression_signals,
        "memory_hits_used": memory_hits_used,
        "notes": notes,
        "summary_artifact_path": summary_artifact_path,
        "created_at_ms": crate::now_ms(),
    });
    let evidence_artifact = write_coder_artifact(
        state,
        &record.linked_context_run_id,
        &evidence_id,
        "coder_review_evidence",
        "artifacts/pr_review.evidence.json",
        &evidence_payload,
    )
    .await?;
    publish_coder_artifact_added(state, record, &evidence_artifact, phase, {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("review_evidence"));
        if let Some(verdict) = verdict {
            extra.insert("verdict".to_string(), json!(verdict));
        }
        if let Some(risk_level) = risk_level {
            extra.insert("risk_level".to_string(), json!(risk_level));
        }
        extra
    });
    Ok(Some(evidence_artifact))
}

pub(super) async fn coder_pr_review_evidence_create(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderPrReviewEvidenceCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    let mut record = load_coder_run_record(&state, &id).await?;
    if !matches!(record.workflow_mode, CoderWorkflowMode::PrReview) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let artifact = write_pr_review_evidence_artifact(
        &state,
        &record,
        input.verdict.as_deref(),
        input.summary.as_deref(),
        input.risk_level.as_deref(),
        &input.changed_files,
        &input.blockers,
        &input.requested_changes,
        &input.regression_signals,
        &input.memory_hits_used,
        input.notes.as_deref(),
        None,
        Some("analysis"),
    )
    .await?;
    let Some(artifact) = artifact else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let final_run = advance_coder_workflow_run(
        &state,
        &record,
        &[
            "inspect_pull_request",
            "retrieve_memory",
            "review_pull_request",
        ],
        &["write_review_artifact"],
        "Write the PR review summary and verdict.",
    )
    .await?;
    record.updated_at_ms = final_run.updated_at_ms;
    save_coder_run_record(&state, &record).await?;
    Ok(Json(json!({
        "ok": true,
        "artifact": artifact,
        "coder_run": coder_run_payload(&record, &final_run),
        "run": final_run,
    })))
}

pub(super) async fn coder_issue_fix_summary_create(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderIssueFixSummaryCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    let mut record = load_coder_run_record(&state, &id).await?;
    if !matches!(record.workflow_mode, CoderWorkflowMode::IssueFix) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let summary_id = format!("issue-fix-summary-{}", Uuid::new_v4().simple());
    let payload = json!({
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "summary": input.summary,
        "root_cause": input.root_cause,
        "fix_strategy": input.fix_strategy,
        "changed_files": input.changed_files,
        "validation_steps": input.validation_steps,
        "validation_results": input.validation_results,
        "memory_hits_used": input.memory_hits_used,
        "notes": input.notes,
        "created_at_ms": crate::now_ms(),
    });
    let artifact = write_coder_artifact(
        &state,
        &record.linked_context_run_id,
        &summary_id,
        "coder_issue_fix_summary",
        "artifacts/issue_fix.summary.json",
        &payload,
    )
    .await?;
    publish_coder_artifact_added(&state, &record, &artifact, Some("artifact_write"), {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("issue_fix_summary"));
        if let Some(fix_strategy) = input.fix_strategy.clone() {
            extra.insert("fix_strategy".to_string(), json!(fix_strategy));
        }
        extra
    });

    let (validation_artifact, mut generated_candidates) = write_issue_fix_validation_outputs(
        &state,
        &record,
        input.summary.as_deref(),
        input.root_cause.as_deref(),
        input.fix_strategy.as_deref(),
        &input.changed_files,
        &input.validation_steps,
        &input.validation_results,
        &input.memory_hits_used,
        input.notes.as_deref(),
        Some(&artifact.path),
    )
    .await?;
    let worker_session =
        load_latest_coder_artifact_payload(&state, &record, "coder_issue_fix_worker_session").await;
    let validation_session =
        load_latest_coder_artifact_payload(&state, &record, "coder_issue_fix_validation_session")
            .await;
    let patch_summary_artifact = write_issue_fix_patch_summary_artifact(
        &state,
        &record,
        input.summary.as_deref(),
        input.root_cause.as_deref(),
        input.fix_strategy.as_deref(),
        &input.changed_files,
        &input.validation_results,
        worker_session.as_ref(),
        validation_session.as_ref(),
        Some("artifact_write"),
    )
    .await?;

    if let Some(summary_text) = input
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|row| !row.is_empty())
        .map(ToString::to_string)
    {
        let strategy = input
            .fix_strategy
            .as_deref()
            .map(str::trim)
            .filter(|row| !row.is_empty())
            .unwrap_or("applied");
        let (fix_pattern_id, fix_pattern_artifact) = write_coder_memory_candidate_artifact(
            &state,
            &record,
            CoderMemoryCandidateKind::FixPattern,
            Some(format!("Fix pattern: {strategy} - {summary_text}")),
            Some("write_fix_artifact".to_string()),
            json!({
                "workflow_mode": "issue_fix",
                "result": strategy,
                "summary": summary_text,
                "root_cause": input.root_cause,
                "fix_strategy": input.fix_strategy,
                "changed_files": input.changed_files,
                "validation_steps": input.validation_steps,
                "validation_results": input.validation_results,
                "memory_hits_used": input.memory_hits_used,
                "summary_artifact_path": artifact.path,
            }),
        )
        .await?;
        generated_candidates.push(json!({
            "candidate_id": fix_pattern_id,
            "kind": "fix_pattern",
            "artifact_path": fix_pattern_artifact.path,
        }));

        let (run_outcome_id, run_outcome_artifact) = write_coder_memory_candidate_artifact(
            &state,
            &record,
            CoderMemoryCandidateKind::RunOutcome,
            Some(format!("Issue fix prepared: {strategy}")),
            Some("write_fix_artifact".to_string()),
            json!({
                "workflow_mode": "issue_fix",
                "result": strategy,
                "summary": summary_text,
                "root_cause": input.root_cause,
                "fix_strategy": input.fix_strategy,
                "changed_files": input.changed_files,
                "validation_steps": input.validation_steps,
                "validation_results": input.validation_results,
                "memory_hits_used": input.memory_hits_used,
                "summary_artifact_path": artifact.path,
            }),
        )
        .await?;
        generated_candidates.push(json!({
            "candidate_id": run_outcome_id,
            "kind": "run_outcome",
            "artifact_path": run_outcome_artifact.path,
        }));
    }

    let final_run = finalize_coder_workflow_run(
        &state,
        &record,
        &[
            "inspect_issue_context",
            "retrieve_memory",
            "prepare_fix",
            "validate_fix",
            "write_fix_artifact",
        ],
        "Issue fix summary recorded.",
    )
    .await?;
    record.updated_at_ms = final_run.updated_at_ms;
    save_coder_run_record(&state, &record).await?;
    Ok(Json(json!({
        "ok": true,
        "artifact": artifact,
        "validation_artifact": validation_artifact,
        "patch_summary_artifact": patch_summary_artifact,
        "generated_candidates": generated_candidates,
        "coder_run": coder_run_payload(&record, &final_run),
        "run": final_run,
    })))
}

pub(super) async fn coder_issue_fix_validation_report_create(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderIssueFixValidationReportCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    let mut record = load_coder_run_record(&state, &id).await?;
    if !matches!(record.workflow_mode, CoderWorkflowMode::IssueFix) {
        return Err(StatusCode::BAD_REQUEST);
    }
    if input.validation_steps.is_empty() && input.validation_results.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let (validation_artifact, generated_candidates) = write_issue_fix_validation_outputs(
        &state,
        &record,
        input.summary.as_deref(),
        input.root_cause.as_deref(),
        input.fix_strategy.as_deref(),
        &input.changed_files,
        &input.validation_steps,
        &input.validation_results,
        &input.memory_hits_used,
        input.notes.as_deref(),
        None,
    )
    .await?;
    let final_run = advance_coder_workflow_run(
        &state,
        &record,
        &[
            "inspect_issue_context",
            "retrieve_memory",
            "prepare_fix",
            "validate_fix",
        ],
        &["write_fix_artifact"],
        "Write the fix summary and patch rationale.",
    )
    .await?;
    record.updated_at_ms = final_run.updated_at_ms;
    save_coder_run_record(&state, &record).await?;
    Ok(Json(json!({
        "ok": true,
        "artifact": validation_artifact,
        "generated_candidates": generated_candidates,
        "coder_run": coder_run_payload(&record, &final_run),
        "run": final_run,
    })))
}

pub(super) async fn coder_merge_recommendation_summary_create(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderMergeRecommendationSummaryCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    let mut record = load_coder_run_record(&state, &id).await?;
    if !matches!(record.workflow_mode, CoderWorkflowMode::MergeRecommendation) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let summary_id = format!("merge-recommendation-summary-{}", Uuid::new_v4().simple());
    let payload = json!({
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "recommendation": input.recommendation,
        "summary": input.summary,
        "risk_level": input.risk_level,
        "blockers": input.blockers,
        "required_checks": input.required_checks,
        "required_approvals": input.required_approvals,
        "memory_hits_used": input.memory_hits_used,
        "notes": input.notes,
        "created_at_ms": crate::now_ms(),
    });
    let artifact = write_coder_artifact(
        &state,
        &record.linked_context_run_id,
        &summary_id,
        "coder_merge_recommendation_summary",
        "artifacts/merge_recommendation.summary.json",
        &payload,
    )
    .await?;
    publish_coder_artifact_added(&state, &record, &artifact, Some("artifact_write"), {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("merge_recommendation_summary"));
        if let Some(recommendation) = input.recommendation.clone() {
            extra.insert("recommendation".to_string(), json!(recommendation));
        }
        if let Some(risk_level) = input.risk_level.clone() {
            extra.insert("risk_level".to_string(), json!(risk_level));
        }
        extra
    });

    let readiness_artifact = write_merge_readiness_artifact(
        &state,
        &record,
        input.recommendation.as_deref(),
        input.summary.as_deref(),
        input.risk_level.as_deref(),
        &input.blockers,
        &input.required_checks,
        &input.required_approvals,
        &input.memory_hits_used,
        input.notes.as_deref(),
        Some(&artifact.path),
        Some("artifact_write"),
    )
    .await?;

    let mut generated_candidates = Vec::<Value>::new();
    if let Some(summary_text) = input
        .summary
        .as_deref()
        .map(str::trim)
        .filter(|row| !row.is_empty())
        .map(ToString::to_string)
    {
        let recommendation = input
            .recommendation
            .as_deref()
            .map(str::trim)
            .filter(|row| !row.is_empty())
            .unwrap_or("hold");
        let (merge_recommendation_memory_id, merge_recommendation_memory_artifact) =
            write_coder_memory_candidate_artifact(
                &state,
                &record,
                CoderMemoryCandidateKind::MergeRecommendationMemory,
                Some(summary_text.clone()),
                Some("write_merge_artifact".to_string()),
                json!({
                    "workflow_mode": "merge_recommendation",
                    "recommendation": recommendation,
                    "summary": summary_text,
                    "risk_level": input.risk_level,
                    "blockers": input.blockers,
                    "required_checks": input.required_checks,
                    "required_approvals": input.required_approvals,
                    "memory_hits_used": input.memory_hits_used,
                    "summary_artifact_path": artifact.path,
                    "readiness_artifact_path": readiness_artifact.as_ref().map(|row| row.path.clone()),
                }),
            )
            .await?;
        generated_candidates.push(json!({
            "candidate_id": merge_recommendation_memory_id,
            "kind": "merge_recommendation_memory",
            "artifact_path": merge_recommendation_memory_artifact.path,
        }));

        let (run_outcome_id, run_outcome_artifact) = write_coder_memory_candidate_artifact(
            &state,
            &record,
            CoderMemoryCandidateKind::RunOutcome,
            Some(format!("Merge recommendation completed: {recommendation}")),
            Some("write_merge_artifact".to_string()),
            json!({
                "workflow_mode": "merge_recommendation",
                "result": recommendation,
                "summary": summary_text,
                "risk_level": input.risk_level,
                "blockers": input.blockers,
                "required_checks": input.required_checks,
                "required_approvals": input.required_approvals,
                "memory_hits_used": input.memory_hits_used,
                "summary_artifact_path": artifact.path,
                "readiness_artifact_path": readiness_artifact.as_ref().map(|row| row.path.clone()),
            }),
        )
        .await?;
        generated_candidates.push(json!({
            "candidate_id": run_outcome_id,
            "kind": "run_outcome",
            "artifact_path": run_outcome_artifact.path,
        }));
    }
    let final_run = finalize_coder_workflow_run(
        &state,
        &record,
        &[
            "inspect_pull_request",
            "retrieve_memory",
            "assess_merge_readiness",
            "write_merge_artifact",
        ],
        "Merge recommendation summary recorded.",
    )
    .await?;
    record.updated_at_ms = final_run.updated_at_ms;
    save_coder_run_record(&state, &record).await?;
    Ok(Json(json!({
        "ok": true,
        "artifact": artifact,
        "readiness_artifact": readiness_artifact,
        "generated_candidates": generated_candidates,
        "coder_run": coder_run_payload(&record, &final_run),
        "run": final_run,
    })))
}

async fn write_merge_readiness_artifact(
    state: &AppState,
    record: &CoderRunRecord,
    recommendation: Option<&str>,
    summary: Option<&str>,
    risk_level: Option<&str>,
    blockers: &[String],
    required_checks: &[String],
    required_approvals: &[String],
    memory_hits_used: &[String],
    notes: Option<&str>,
    summary_artifact_path: Option<&str>,
    phase: Option<&str>,
) -> Result<Option<ContextBlackboardArtifact>, StatusCode> {
    if blockers.is_empty()
        && required_checks.is_empty()
        && required_approvals.is_empty()
        && summary.map(str::trim).unwrap_or("").is_empty()
        && notes.map(str::trim).unwrap_or("").is_empty()
    {
        return Ok(None);
    }
    let readiness_id = format!("merge-readiness-{}", Uuid::new_v4().simple());
    let readiness_payload = json!({
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "recommendation": recommendation,
        "summary": summary,
        "risk_level": risk_level,
        "blockers": blockers,
        "required_checks": required_checks,
        "required_approvals": required_approvals,
        "memory_hits_used": memory_hits_used,
        "notes": notes,
        "summary_artifact_path": summary_artifact_path,
        "created_at_ms": crate::now_ms(),
    });
    let readiness_artifact = write_coder_artifact(
        state,
        &record.linked_context_run_id,
        &readiness_id,
        "coder_merge_readiness_report",
        "artifacts/merge_recommendation.readiness.json",
        &readiness_payload,
    )
    .await?;
    publish_coder_artifact_added(state, record, &readiness_artifact, phase, {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("merge_readiness_report"));
        if let Some(recommendation) = recommendation {
            extra.insert("recommendation".to_string(), json!(recommendation));
        }
        if let Some(risk_level) = risk_level {
            extra.insert("risk_level".to_string(), json!(risk_level));
        }
        extra
    });
    Ok(Some(readiness_artifact))
}

pub(super) async fn coder_merge_readiness_report_create(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderMergeReadinessReportCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    let mut record = load_coder_run_record(&state, &id).await?;
    if !matches!(record.workflow_mode, CoderWorkflowMode::MergeRecommendation) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let artifact = write_merge_readiness_artifact(
        &state,
        &record,
        input.recommendation.as_deref(),
        input.summary.as_deref(),
        input.risk_level.as_deref(),
        &input.blockers,
        &input.required_checks,
        &input.required_approvals,
        &input.memory_hits_used,
        input.notes.as_deref(),
        None,
        Some("analysis"),
    )
    .await?;
    let Some(artifact) = artifact else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let final_run = advance_coder_workflow_run(
        &state,
        &record,
        &[
            "inspect_pull_request",
            "retrieve_memory",
            "assess_merge_readiness",
        ],
        &["write_merge_artifact"],
        "Write the merge recommendation summary.",
    )
    .await?;
    record.updated_at_ms = final_run.updated_at_ms;
    save_coder_run_record(&state, &record).await?;
    Ok(Json(json!({
        "ok": true,
        "artifact": artifact,
        "coder_run": coder_run_payload(&record, &final_run),
        "run": final_run,
    })))
}
