use super::context_types::{
    ContextBlackboardArtifact, ContextBlackboardPatchOp, ContextBlackboardTaskStatus,
    ContextRunCreateInput, ContextRunEventAppendInput, ContextRunState, ContextRunStatus,
    ContextTaskCreateBatchInput, ContextTaskCreateInput, ContextTaskTransitionInput,
    ContextWorkspaceLease,
};
use super::*;
use crate::ExternalActionRecord;
use axum::extract::Path;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::OnceLock;
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
    #[serde(default)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) origin_policy: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) github_project_ref: Option<CoderGithubProjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) remote_sync_state: Option<CoderRemoteSyncState>,
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
    #[serde(default)]
    pub(super) origin_policy: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CoderProjectRunCreateInput {
    #[serde(default)]
    pub(super) coder_run_id: Option<String>,
    pub(super) workflow_mode: CoderWorkflowMode,
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
    #[serde(default)]
    pub(super) origin_policy: Option<Value>,
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

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderProjectRunListQuery {
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
    DuplicateLinkage,
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
    pub(super) prior_runs_considered: Vec<Value>,
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
    pub(super) validation_steps: Vec<String>,
    #[serde(default)]
    pub(super) validation_results: Vec<Value>,
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
    pub(super) validation_steps: Vec<String>,
    #[serde(default)]
    pub(super) validation_results: Vec<Value>,
    #[serde(default)]
    pub(super) memory_hits_used: Vec<String>,
    #[serde(default)]
    pub(super) notes: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderMergeSubmitInput {
    #[serde(default)]
    pub(super) approved_by: Option<String>,
    #[serde(default)]
    pub(super) reason: Option<String>,
    #[serde(default)]
    pub(super) mcp_server: Option<String>,
    #[serde(default)]
    pub(super) dry_run: Option<bool>,
    #[serde(default)]
    pub(super) submit_mode: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct CoderProjectPolicy {
    pub(super) project_id: String,
    #[serde(default)]
    pub(super) auto_merge_enabled: bool,
    #[serde(default)]
    pub(super) updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CoderProjectBinding {
    pub(super) project_id: String,
    pub(super) repo_binding: CoderRepoBinding,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) github_project_binding: Option<CoderGithubProjectBinding>,
    #[serde(default)]
    pub(super) updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum CoderRemoteSyncState {
    InSync,
    SchemaDrift,
    RemoteStateDiverged,
    ProjectionUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct CoderGithubProjectStatusOption {
    pub(super) id: String,
    pub(super) name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct CoderGithubProjectStatusMapping {
    pub(super) field_id: String,
    pub(super) field_name: String,
    pub(super) todo: CoderGithubProjectStatusOption,
    pub(super) in_progress: CoderGithubProjectStatusOption,
    pub(super) in_review: CoderGithubProjectStatusOption,
    pub(super) blocked: CoderGithubProjectStatusOption,
    pub(super) done: CoderGithubProjectStatusOption,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct CoderGithubProjectBinding {
    pub(super) owner: String,
    pub(super) project_number: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) repo_slug: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) mcp_server: Option<String>,
    pub(super) schema_snapshot: Value,
    pub(super) schema_fingerprint: String,
    pub(super) status_mapping: CoderGithubProjectStatusMapping,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(super) struct CoderGithubProjectRef {
    pub(super) owner: String,
    pub(super) project_number: u64,
    pub(super) project_item_id: String,
    pub(super) issue_number: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) issue_url: Option<String>,
    pub(super) schema_fingerprint: String,
    pub(super) status_mapping: CoderGithubProjectStatusMapping,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderProjectBindingPutInput {
    #[serde(default)]
    pub(super) repo_binding: Option<CoderRepoBinding>,
    #[serde(default)]
    pub(super) github_project_binding: Option<CoderGithubProjectBindingRequest>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct CoderGithubProjectBindingRequest {
    pub(super) owner: String,
    pub(super) project_number: u64,
    #[serde(default)]
    pub(super) repo_slug: Option<String>,
    #[serde(default)]
    pub(super) mcp_server: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CoderGithubProjectIntakeInput {
    pub(super) project_item_id: String,
    #[serde(default)]
    pub(super) coder_run_id: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CoderProjectSummary {
    pub(super) project_id: String,
    pub(super) repo_binding: CoderRepoBinding,
    pub(super) latest_coder_run_id: Option<String>,
    pub(super) latest_updated_at_ms: u64,
    pub(super) run_count: u64,
    pub(super) workflow_modes: Vec<CoderWorkflowMode>,
    pub(super) project_policy: CoderProjectPolicy,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct CoderProjectPolicyPutInput {
    #[serde(default)]
    pub(super) auto_merge_enabled: bool,
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

#[derive(Clone)]
struct GithubProjectsAdapter<'a> {
    state: &'a AppState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GithubProjectIssueSummary {
    number: u64,
    title: String,
    html_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GithubProjectInboxItemRecord {
    project_item_id: String,
    title: String,
    status_name: String,
    status_option_id: Option<String>,
    issue: Option<GithubProjectIssueSummary>,
    raw: Value,
}

impl<'a> GithubProjectsAdapter<'a> {
    fn new(state: &'a AppState) -> Self {
        Self { state }
    }
}

fn coder_project_intake_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

fn coder_runs_root(state: &AppState) -> PathBuf {
    state
        .shared_resources_path
        .parent()
        .map(|parent| parent.join("coder_runs"))
        .unwrap_or_else(|| PathBuf::from(".tandem").join("coder_runs"))
}

fn coder_project_policies_root(state: &AppState) -> PathBuf {
    state
        .shared_resources_path
        .parent()
        .map(|parent| parent.join("coder_project_policies"))
        .unwrap_or_else(|| PathBuf::from(".tandem").join("coder_project_policies"))
}

fn coder_project_bindings_root(state: &AppState) -> PathBuf {
    state
        .shared_resources_path
        .parent()
        .map(|parent| parent.join("coder_project_bindings"))
        .unwrap_or_else(|| PathBuf::from(".tandem").join("coder_project_bindings"))
}

fn coder_project_policy_path(state: &AppState, project_id: &str) -> PathBuf {
    coder_project_policies_root(state).join(format!("{project_id}.json"))
}

fn coder_project_binding_path(state: &AppState, project_id: &str) -> PathBuf {
    coder_project_bindings_root(state).join(format!("{project_id}.json"))
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

async fn ensure_coder_project_policies_dir(state: &AppState) -> Result<(), StatusCode> {
    tokio::fs::create_dir_all(coder_project_policies_root(state))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn ensure_coder_project_bindings_dir(state: &AppState) -> Result<(), StatusCode> {
    tokio::fs::create_dir_all(coder_project_bindings_root(state))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn load_coder_project_policy(
    state: &AppState,
    project_id: &str,
) -> Result<CoderProjectPolicy, StatusCode> {
    let path = coder_project_policy_path(state, project_id);
    if !path.exists() {
        return Ok(CoderProjectPolicy {
            project_id: project_id.to_string(),
            auto_merge_enabled: false,
            updated_at_ms: 0,
        });
    }
    let raw = tokio::fs::read_to_string(path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut policy = serde_json::from_str::<CoderProjectPolicy>(&raw)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if policy.project_id.trim().is_empty() {
        policy.project_id = project_id.to_string();
    }
    Ok(policy)
}

async fn save_coder_project_policy(
    state: &AppState,
    policy: &CoderProjectPolicy,
) -> Result<(), StatusCode> {
    ensure_coder_project_policies_dir(state).await?;
    let payload =
        serde_json::to_string_pretty(policy).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::fs::write(
        coder_project_policy_path(state, &policy.project_id),
        payload,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn load_coder_project_binding(
    state: &AppState,
    project_id: &str,
) -> Result<Option<CoderProjectBinding>, StatusCode> {
    let path = coder_project_binding_path(state, project_id);
    if !path.exists() {
        return Ok(None);
    }
    let raw = tokio::fs::read_to_string(path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut binding = serde_json::from_str::<CoderProjectBinding>(&raw)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if binding.project_id.trim().is_empty() {
        binding.project_id = project_id.to_string();
    }
    if binding.repo_binding.project_id.trim().is_empty() {
        binding.repo_binding.project_id = project_id.to_string();
    }
    Ok(Some(binding))
}

async fn save_coder_project_binding(
    state: &AppState,
    binding: &CoderProjectBinding,
) -> Result<(), StatusCode> {
    ensure_coder_project_bindings_dir(state).await?;
    let payload =
        serde_json::to_string_pretty(binding).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::fs::write(
        coder_project_binding_path(state, &binding.project_id),
        payload,
    )
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

fn parse_coder_project_binding_put_input(
    project_id: &str,
    value: Value,
) -> Result<CoderProjectBindingPutInput, StatusCode> {
    if value.get("repo_binding").is_some() || value.get("github_project_binding").is_some() {
        let mut parsed = serde_json::from_value::<CoderProjectBindingPutInput>(value)
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        if let Some(repo_binding) = parsed.repo_binding.as_mut() {
            repo_binding.project_id = project_id.to_string();
        }
        return Ok(parsed);
    }
    let mut repo_binding =
        serde_json::from_value::<CoderRepoBinding>(value).map_err(|_| StatusCode::BAD_REQUEST)?;
    repo_binding.project_id = project_id.to_string();
    Ok(CoderProjectBindingPutInput {
        repo_binding: Some(repo_binding),
        github_project_binding: None,
    })
}

async fn find_latest_project_item_run(
    state: &AppState,
    project_item_id: &str,
) -> Result<Option<(CoderRunRecord, ContextRunState)>, StatusCode> {
    ensure_coder_runs_dir(state).await?;
    let mut latest: Option<(CoderRunRecord, ContextRunState)> = None;
    let mut dir = tokio::fs::read_dir(coder_runs_root(state))
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
        if record
            .github_project_ref
            .as_ref()
            .map(|row| row.project_item_id.as_str())
            != Some(project_item_id)
        {
            continue;
        }
        let Ok(run) = load_context_run_state(state, &record.linked_context_run_id).await else {
            continue;
        };
        let replace = latest
            .as_ref()
            .map(|(_, existing_run)| run.updated_at_ms >= existing_run.updated_at_ms)
            .unwrap_or(true);
        if replace {
            latest = Some((record, run));
        }
    }
    Ok(latest)
}

async fn maybe_sync_github_project_status(
    state: &AppState,
    record: &mut CoderRunRecord,
    context_run: &ContextRunState,
) -> Result<(), StatusCode> {
    let Some(project_ref) = record.github_project_ref.clone() else {
        return Ok(());
    };
    let Some(project_binding) = load_coder_project_binding(state, &record.repo_binding.project_id)
        .await?
        .and_then(|row| row.github_project_binding)
    else {
        record.remote_sync_state = Some(CoderRemoteSyncState::ProjectionUnavailable);
        save_coder_run_record(state, record).await?;
        return Ok(());
    };
    if project_binding.schema_fingerprint != project_ref.schema_fingerprint {
        record.remote_sync_state = Some(CoderRemoteSyncState::SchemaDrift);
        save_coder_run_record(state, record).await?;
        return Ok(());
    }
    let target_option =
        context_status_to_project_option(&project_ref.status_mapping, &context_run.status);
    let adapter = GithubProjectsAdapter::new(state);
    match adapter
        .update_project_item_status(
            &project_binding,
            &project_ref.project_item_id,
            &target_option,
        )
        .await
    {
        Ok(_) => {
            record.remote_sync_state = Some(CoderRemoteSyncState::InSync);
            save_coder_run_record(state, record).await?;
        }
        Err(_) => {
            record.remote_sync_state = Some(CoderRemoteSyncState::ProjectionUnavailable);
            save_coder_run_record(state, record).await?;
        }
    }
    Ok(())
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
            let same_linked_issue = github_ref
                .filter(|reference| matches!(reference.kind, CoderGithubRefKind::Issue))
                .map(|reference| {
                    candidate_linked_numbers(&candidate_payload, "linked_issue_numbers")
                        .contains(&reference.number)
                })
                .unwrap_or(false);
            let same_linked_pr = github_ref
                .filter(|reference| matches!(reference.kind, CoderGithubRefKind::PullRequest))
                .map(|reference| {
                    candidate_linked_numbers(&candidate_payload, "linked_pr_numbers")
                        .contains(&reference.number)
                })
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
                "same_linked_issue": same_linked_issue,
                "same_linked_pr": same_linked_pr,
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

fn build_duplicate_linkage_payload(
    record: &CoderRunRecord,
    submitted_github_ref: &CoderGithubRef,
    pull_request: &GithubPullRequestSummary,
    submission_artifact_path: &str,
) -> Value {
    let issue_number = record
        .github_ref
        .as_ref()
        .filter(|reference| matches!(reference.kind, CoderGithubRefKind::Issue))
        .map(|reference| reference.number);
    json!({
        "type": "duplicate.issue_pr_linkage",
        "repo_slug": record.repo_binding.repo_slug,
        "project_id": record.repo_binding.project_id,
        "summary": issue_number.map(|number| format!(
            "{} issue #{} is linked to pull request #{}",
            record.repo_binding.repo_slug, number, pull_request.number
        )),
        "issue_ref": record.github_ref,
        "pull_request_ref": submitted_github_ref,
        "linked_issue_numbers": issue_number.into_iter().collect::<Vec<_>>(),
        "linked_pr_numbers": [pull_request.number],
        "relationship": "issue_fix_pr_submit",
        "pull_request_title": pull_request.title,
        "pull_request_url": pull_request.html_url,
        "artifact_refs": [submission_artifact_path],
    })
}

fn build_inferred_duplicate_linkage_payload(
    record: &CoderRunRecord,
    duplicate_candidates: &[Value],
    artifact_path: &str,
) -> Option<Value> {
    let mut linked_issue_numbers = record
        .github_ref
        .as_ref()
        .filter(|reference| matches!(reference.kind, CoderGithubRefKind::Issue))
        .map(|reference| vec![reference.number])
        .unwrap_or_default();
    for number in duplicate_candidates
        .iter()
        .flat_map(|candidate| candidate_linked_numbers(candidate, "linked_issue_numbers"))
    {
        linked_issue_numbers.push(number);
    }
    linked_issue_numbers.sort_unstable();
    linked_issue_numbers.dedup();

    let mut linked_pr_numbers = duplicate_candidates
        .iter()
        .flat_map(|candidate| candidate_linked_numbers(candidate, "linked_pr_numbers"))
        .collect::<Vec<_>>();
    for number in duplicate_candidates.iter().filter_map(|candidate| {
        (candidate.get("kind").and_then(Value::as_str) == Some("pull_request"))
            .then(|| candidate.get("number").and_then(Value::as_u64))
            .flatten()
    }) {
        linked_pr_numbers.push(number);
    }
    linked_pr_numbers.sort_unstable();
    linked_pr_numbers.dedup();

    if linked_issue_numbers.is_empty() || linked_pr_numbers.is_empty() {
        return None;
    }

    Some(json!({
        "type": "duplicate.issue_pr_linkage",
        "repo_slug": record.repo_binding.repo_slug,
        "project_id": record.repo_binding.project_id,
        "summary": format!(
            "{} duplicate triage links issues {:?} to pull requests {:?}",
            record.repo_binding.repo_slug, linked_issue_numbers, linked_pr_numbers
        ),
        "issue_ref": record.github_ref,
        "linked_issue_numbers": linked_issue_numbers,
        "linked_pr_numbers": linked_pr_numbers,
        "relationship": "issue_triage_duplicate_inference",
        "artifact_refs": [artifact_path],
    }))
}

async fn maybe_write_follow_on_duplicate_linkage_candidate(
    state: &AppState,
    record: &CoderRunRecord,
) -> Result<Option<Value>, StatusCode> {
    if !matches!(
        record.workflow_mode,
        CoderWorkflowMode::PrReview | CoderWorkflowMode::MergeRecommendation
    ) {
        return Ok(None);
    }
    let Some(parent_coder_run_id) = record.parent_coder_run_id.as_deref() else {
        return Ok(None);
    };
    let Ok(parent_record) = load_coder_run_record(state, parent_coder_run_id).await else {
        return Ok(None);
    };
    if !matches!(parent_record.workflow_mode, CoderWorkflowMode::IssueFix) {
        return Ok(None);
    }
    let Some(issue_ref) = parent_record
        .github_ref
        .as_ref()
        .filter(|reference| matches!(reference.kind, CoderGithubRefKind::Issue))
    else {
        return Ok(None);
    };
    let Some(pull_request_ref) = record
        .github_ref
        .as_ref()
        .filter(|reference| matches!(reference.kind, CoderGithubRefKind::PullRequest))
    else {
        return Ok(None);
    };
    let payload = json!({
        "type": "duplicate.issue_pr_linkage",
        "repo_slug": record.repo_binding.repo_slug,
        "project_id": record.repo_binding.project_id,
        "summary": format!(
            "{} issue #{} is linked to pull request #{}",
            record.repo_binding.repo_slug, issue_ref.number, pull_request_ref.number
        ),
        "issue_ref": issue_ref,
        "pull_request_ref": pull_request_ref,
        "linked_issue_numbers": [issue_ref.number],
        "linked_pr_numbers": [pull_request_ref.number],
        "relationship": "issue_fix_follow_on",
        "artifact_refs": Vec::<String>::new(),
    });
    let (candidate_id, artifact) = write_coder_memory_candidate_artifact(
        state,
        record,
        CoderMemoryCandidateKind::DuplicateLinkage,
        Some(format!(
            "{} issue #{} linked to PR #{}",
            record.repo_binding.repo_slug, issue_ref.number, pull_request_ref.number
        )),
        Some("retrieve_memory".to_string()),
        payload,
    )
    .await?;
    Ok(Some(json!({
        "candidate_id": candidate_id,
        "kind": "duplicate_linkage",
        "artifact_path": artifact.path,
    })))
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

fn candidate_linked_numbers(candidate_payload: &Value, key: &str) -> Vec<u64> {
    candidate_payload
        .get("payload")
        .and_then(|row| row.get(key))
        .or_else(|| {
            candidate_payload
                .get("metadata")
                .and_then(|row| row.get(key))
        })
        .and_then(Value::as_array)
        .map(|rows| rows.iter().filter_map(Value::as_u64).collect::<Vec<_>>())
        .unwrap_or_default()
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
            let same_linked_issue = record
                .github_ref
                .as_ref()
                .filter(|reference| matches!(reference.kind, CoderGithubRefKind::Issue))
                .map(|reference| {
                    candidate_linked_numbers(
                        &json!({ "metadata": hit.record.metadata.clone() }),
                        "linked_issue_numbers",
                    )
                    .contains(&reference.number)
                })
                .unwrap_or(false);
            let same_linked_pr = record
                .github_ref
                .as_ref()
                .filter(|reference| matches!(reference.kind, CoderGithubRefKind::PullRequest))
                .map(|reference| {
                    candidate_linked_numbers(
                        &json!({ "metadata": hit.record.metadata.clone() }),
                        "linked_pr_numbers",
                    )
                    .contains(&reference.number)
                })
                .unwrap_or(false);
            hits.push(json!({
                "source": "governed_memory",
                "memory_id": hit.record.id,
                "score": hit.score,
                "content": hit.record.content,
                "metadata": hit.record.metadata,
                "same_linked_issue": same_linked_issue,
                "same_linked_pr": same_linked_pr,
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

fn coder_memory_retrieval_policy(record: &CoderRunRecord, query: &str, limit: usize) -> Value {
    let prioritized_kinds = match record.workflow_mode {
        CoderWorkflowMode::IssueTriage => {
            vec![
                "failure_pattern",
                "regression_signal",
                "duplicate_linkage",
                "triage_memory",
                "fix_pattern",
                "run_outcome",
            ]
        }
        CoderWorkflowMode::IssueFix => {
            vec![
                "fix_pattern",
                "validation_memory",
                "regression_signal",
                "duplicate_linkage",
                "run_outcome",
                "triage_memory",
            ]
        }
        CoderWorkflowMode::PrReview => {
            vec![
                "review_memory",
                "merge_recommendation_memory",
                "duplicate_linkage",
                "regression_signal",
                "run_outcome",
            ]
        }
        CoderWorkflowMode::MergeRecommendation => {
            vec![
                "merge_recommendation_memory",
                "review_memory",
                "duplicate_linkage",
                "run_outcome",
                "regression_signal",
            ]
        }
    };
    json!({
        "workflow_mode": record.workflow_mode,
        "query": query,
        "limit": limit.clamp(1, 20),
        "sources": [
            "repo_memory_candidates",
            "project_memory",
            "governed_memory"
        ],
        "prioritized_kinds": prioritized_kinds,
        "same_ref_priority": true,
        "same_issue_priority": matches!(
            record.workflow_mode,
            CoderWorkflowMode::IssueTriage | CoderWorkflowMode::IssueFix
        ),
        "governed_cross_ref_priority": true,
    })
}

async fn collect_coder_memory_hits(
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
