use super::context_runs::{
    context_run_create, context_run_engine, context_run_tasks_create, ensure_context_run_dir,
    load_context_blackboard, load_context_run_state, save_context_run_state,
};
use super::context_types::{
    ContextBlackboardArtifact, ContextBlackboardPatchOp, ContextBlackboardTaskStatus,
    ContextRunCreateInput, ContextRunEventAppendInput, ContextRunState, ContextRunStatus,
    ContextTaskCreateBatchInput, ContextTaskCreateInput, ContextWorkspaceLease,
};
use super::*;
use axum::extract::Path;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use tandem_memory::{
    types::MemoryTier, GovernedMemoryTier, MemoryClassification, MemoryContentKind, MemoryManager,
    MemoryPartition, MemoryPromoteRequest, MemoryPutRequest, PromotionReview,
};

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
    ReviewMemory,
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
    github_issue_number: Option<u64>,
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
            let same_issue = github_issue_number.is_some_and(|issue_number| {
                candidate_payload
                    .get("github_ref")
                    .and_then(|row| row.get("number"))
                    .and_then(Value::as_u64)
                    == Some(issue_number)
            });
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
        let a_same_issue = a
            .get("same_issue")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let b_same_issue = b
            .get("same_issue")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        b_same_issue.cmp(&a_same_issue).then_with(|| {
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
    let issue_number = record
        .github_ref
        .as_ref()
        .filter(|reference| matches!(reference.kind, CoderGithubRefKind::Issue))
        .map(|row| row.number);
    let mut hits =
        list_repo_memory_candidates(state, &record.repo_binding.repo_slug, issue_number, limit)
            .await?;
    let mut project_hits = list_project_memory_hits(&record.repo_binding, query, limit).await;
    let mut governed_hits = list_governed_memory_hits(record, query, limit).await;
    hits.append(&mut project_hits);
    hits.append(&mut governed_hits);
    hits.sort_by(|a, b| {
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
        b_same_issue
            .cmp(&a_same_issue)
            .then_with(|| {
                b_score
                    .partial_cmp(&a_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| {
                b.get("created_at_ms")
                    .and_then(Value::as_u64)
                    .cmp(&a.get("created_at_ms").and_then(Value::as_u64))
            })
    });
    hits.truncate(limit.clamp(1, 20));
    Ok(hits)
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
        let kind = value_string(hit.get("kind"))
            .or_else(|| value_string(hit.get("metadata").and_then(|row| row.get("kind"))))
            .unwrap_or_default();
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
            let mut extra = serde_json::Map::new();
            extra.insert("candidate_id".to_string(), json!(candidate_id));
            extra.insert("candidate_kind".to_string(), json!(kind));
            extra.insert("artifact_id".to_string(), json!(artifact.id));
            extra.insert("artifact_path".to_string(), json!(artifact.path));
            extra
        },
    );
    Ok((candidate_id, artifact))
}

fn build_governed_memory_content(candidate_payload: &Value) -> Option<String> {
    candidate_payload
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
        })
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
                _ => "analysis",
            };
        }
    }
    "analysis"
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
    let mut payload = serde_json::Map::new();
    payload.insert("artifact_id".to_string(), json!(artifact.id));
    payload.insert("artifact_type".to_string(), json!(artifact.artifact_type));
    payload.insert("artifact_path".to_string(), json!(artifact.path));
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

fn coder_run_payload(record: &CoderRunRecord, context_run: &ContextRunState) -> Value {
    json!({
        "coder_run_id": record.coder_run_id,
        "workflow_mode": record.workflow_mode,
        "linked_context_run_id": record.linked_context_run_id,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "source_client": record.source_client,
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
    if matches!(input.workflow_mode, CoderWorkflowMode::IssueTriage) {
        let readiness = coder_issue_triage_readiness(&state, &input).await?;
        if !readiness.runnable {
            return Ok((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "Coder issue triage is not ready to run",
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
            CoderWorkflowMode::PrReview => compose_pr_review_objective(&input),
            CoderWorkflowMode::MergeRecommendation => {
                compose_merge_recommendation_objective(&input)
            }
            _ => compose_issue_triage_objective(&input),
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

    let record = CoderRunRecord {
        coder_run_id: coder_run_id.clone(),
        workflow_mode: input.workflow_mode.clone(),
        linked_context_run_id: linked_context_run_id.clone(),
        repo_binding: input.repo_binding,
        github_ref: input.github_ref,
        source_client: normalize_source_client(input.source_client.as_deref())
            .or_else(|| Some("coder_api".to_string())),
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
            let mut run = load_context_run_state(&state, &linked_context_run_id).await?;
            run.status = ContextRunStatus::Planning;
            run.why_next_step = Some(
                "Normalize the issue reference, retrieve relevant memory, then inspect the repo."
                    .to_string(),
            );
            ensure_context_run_dir(&state, &linked_context_run_id).await?;
            save_context_run_state(&state, &run).await?;
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
            let mut run = load_context_run_state(&state, &linked_context_run_id).await?;
            run.status = ContextRunStatus::Planning;
            run.why_next_step = Some(
                "Inspect the pull request, retrieve review memory, then analyze risk.".to_string(),
            );
            ensure_context_run_dir(&state, &linked_context_run_id).await?;
            save_context_run_state(&state, &run).await?;
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
            let mut run = load_context_run_state(&state, &linked_context_run_id).await?;
            run.status = ContextRunStatus::Planning;
            run.why_next_step = Some(
                "Inspect the pull request, retrieve merge memory, then assess readiness."
                    .to_string(),
            );
            ensure_context_run_dir(&state, &linked_context_run_id).await?;
            save_context_run_state(&state, &run).await?;
        }
        _ => {}
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
            | CoderWorkflowMode::PrReview
            | CoderWorkflowMode::MergeRecommendation
    ) {
        collect_issue_triage_memory_hits(&state, &record, &memory_query, 8).await?
    } else {
        Vec::new()
    };
    let issue_number = record
        .github_ref
        .as_ref()
        .filter(|reference| matches!(reference.kind, CoderGithubRefKind::Issue))
        .map(|row| row.number);
    let memory_candidates =
        list_repo_memory_candidates(&state, &record.repo_binding.repo_slug, issue_number, 20)
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
    let issue_number = record
        .github_ref
        .as_ref()
        .filter(|reference| matches!(reference.kind, CoderGithubRefKind::Issue))
        .map(|row| row.number);
    let candidates =
        list_repo_memory_candidates(&state, &record.repo_binding.repo_slug, issue_number, 20)
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
                CoderMemoryCandidateKind::ReviewMemory => MemoryContentKind::SolutionCapsule,
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
            let mut extra = serde_json::Map::new();
            extra.insert("candidate_id".to_string(), json!(candidate_id));
            extra.insert("memory_id".to_string(), json!(put_response.id));
            extra.insert("promoted".to_string(), json!(promoted));
            extra.insert("to_tier".to_string(), json!(to_tier));
            extra.insert("artifact_id".to_string(), json!(artifact.id));
            extra.insert("artifact_path".to_string(), json!(artifact.path));
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
    let record = load_coder_run_record(&state, &id).await?;
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
    Ok(Json(json!({
        "ok": true,
        "artifact": artifact,
        "generated_candidates": generated_candidates,
    })))
}

pub(super) async fn coder_pr_review_summary_create(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderPrReviewSummaryCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    let record = load_coder_run_record(&state, &id).await?;
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
            }),
        )
        .await?;
        generated_candidates.push(json!({
            "candidate_id": run_outcome_id,
            "kind": "run_outcome",
            "artifact_path": run_outcome_artifact.path,
        }));
    }

    Ok(Json(json!({
        "ok": true,
        "artifact": artifact,
        "generated_candidates": generated_candidates,
    })))
}

pub(super) async fn coder_merge_recommendation_summary_create(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<CoderMergeRecommendationSummaryCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    let record = load_coder_run_record(&state, &id).await?;
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
    Ok(Json(json!({
        "ok": true,
        "artifact": artifact,
    })))
}
