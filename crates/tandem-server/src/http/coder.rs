use super::context_runs::{
    context_run_create, context_run_engine, context_run_tasks_create, ensure_context_run_dir,
    load_context_blackboard, load_context_run_state, save_context_run_state,
};
use super::context_types::{
    ContextBlackboardArtifact, ContextBlackboardPatchOp, ContextBlackboardTaskStatus,
    ContextRunCreateInput, ContextRunState, ContextRunStatus, ContextTaskCreateBatchInput,
    ContextTaskCreateInput, ContextWorkspaceLease,
};
use super::*;
use axum::extract::Path;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tandem_memory::{types::MemoryTier, MemoryManager};

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
pub(super) struct CoderMemoryHitsQuery {
    #[serde(default)]
    pub(super) q: Option<String>,
    #[serde(default)]
    pub(super) limit: Option<usize>,
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
                "candidate_id": candidate_payload.get("candidate_id").cloned().unwrap_or(Value::Null),
                "kind": candidate_kind,
                "repo_slug": repo_slug,
                "same_issue": same_issue,
                "summary": candidate_payload.get("summary").cloned().unwrap_or(Value::Null),
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

async fn collect_issue_triage_memory_hits(
    state: &AppState,
    record: &CoderRunRecord,
    query: &str,
    limit: usize,
) -> Result<Vec<Value>, StatusCode> {
    let issue_number = record.github_ref.as_ref().map(|row| row.number);
    let mut hits =
        list_repo_memory_candidates(state, &record.repo_binding.repo_slug, issue_number, limit)
            .await?;
    let mut project_hits = list_project_memory_hits(&record.repo_binding, query, limit).await;
    hits.append(&mut project_hits);
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
    state.event_bus.publish(EngineEvent::new(
        "coder.memory.candidate_added",
        json!({
            "coder_run_id": record.coder_run_id,
            "linked_context_run_id": record.linked_context_run_id,
            "candidate_id": candidate_id,
            "kind": kind,
            "artifact_path": artifact.path,
        }),
    ));
    Ok((candidate_id, artifact))
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
                Some("attempt_reproduction") => "reproduction",
                Some("write_triage_artifact") => "artifact_write",
                _ => "analysis",
            };
        }
    }
    "analysis"
}

async fn coder_issue_triage_readiness(
    state: &AppState,
    input: &CoderRunCreateInput,
) -> Result<CapabilityReadinessOutput, StatusCode> {
    let required_capabilities = vec![
        "github.list_issues".to_string(),
        "github.get_issue".to_string(),
    ];
    let bindings = state
        .capability_resolver
        .list_bindings()
        .await
        .unwrap_or_default();
    let mut missing_required_capabilities = Vec::new();
    for capability_id in &required_capabilities {
        let has_binding = bindings
            .bindings
            .iter()
            .any(|row| row.capability_id == *capability_id);
        if !has_binding {
            missing_required_capabilities.push(capability_id.clone());
        }
    }
    let unbound_capabilities = missing_required_capabilities.clone();
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
    let provider_preference = input
        .mcp_servers
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(|row| row.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let mut missing_servers = Vec::new();
    let mut disconnected_servers = Vec::new();
    for provider in &provider_preference {
        let any_enabled = enabled_servers
            .iter()
            .any(|server| server.name.eq_ignore_ascii_case(provider));
        if !any_enabled {
            missing_servers.push(provider.clone());
            continue;
        }
        if !connected_servers.contains(provider) {
            disconnected_servers.push(provider.clone());
        }
    }
    let mut blocking_issues = Vec::<CapabilityBlockingIssue>::new();
    if !missing_required_capabilities.is_empty() {
        blocking_issues.push(CapabilityBlockingIssue {
            code: "missing_required_capabilities".to_string(),
            message: "Some required capabilities do not have any bindings.".to_string(),
            capability_ids: missing_required_capabilities.clone(),
            providers: Vec::new(),
            tools: Vec::new(),
        });
    }
    if !unbound_capabilities.is_empty() {
        let providers = unbound_capabilities
            .iter()
            .flat_map(|capability_id| {
                crate::capability_resolver::providers_for_capability(&bindings, capability_id)
            })
            .collect::<Vec<_>>();
        blocking_issues.push(CapabilityBlockingIssue {
            code: "unbound_capabilities".to_string(),
            message: "Some required capabilities have bindings, but no available runtime tools."
                .to_string(),
            capability_ids: unbound_capabilities.clone(),
            providers,
            tools: Vec::new(),
        });
    }
    if !missing_servers.is_empty() {
        blocking_issues.push(CapabilityBlockingIssue {
            code: "missing_mcp_servers".to_string(),
            message: "Preferred MCP servers are not configured.".to_string(),
            capability_ids: Vec::new(),
            providers: missing_servers.clone(),
            tools: Vec::new(),
        });
    }
    if !disconnected_servers.is_empty() {
        blocking_issues.push(CapabilityBlockingIssue {
            code: "disconnected_mcp_servers".to_string(),
            message: "Preferred MCP servers are configured but disconnected.".to_string(),
            capability_ids: Vec::new(),
            providers: disconnected_servers.clone(),
            tools: Vec::new(),
        });
    }
    Ok(CapabilityReadinessOutput {
        workflow_id: "coder_issue_triage".to_string(),
        runnable: blocking_issues.is_empty(),
        resolved: Vec::new(),
        missing_required_capabilities,
        unbound_capabilities,
        missing_optional_capabilities: Vec::new(),
        missing_servers,
        disconnected_servers,
        auth_pending_tools: Vec::new(),
        missing_secret_refs: Vec::new(),
        considered_bindings: bindings.bindings.len(),
        recommendations: Vec::new(),
        blocking_issues,
    })
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

    let now = crate::now_ms();
    let coder_run_id = input
        .coder_run_id
        .clone()
        .unwrap_or_else(|| format!("coder-{}", Uuid::new_v4().simple()));
    let linked_context_run_id = format!("ctx-{coder_run_id}");
    let create_input = ContextRunCreateInput {
        run_id: Some(linked_context_run_id.clone()),
        objective: compose_issue_triage_objective(&input),
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
            let artifact_id = format!("memory-hits-{}", Uuid::new_v4().simple());
            let payload = json!({
                "coder_run_id": record.coder_run_id,
                "linked_context_run_id": record.linked_context_run_id,
                "query": memory_query,
                "hits": memory_hits,
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
            state.event_bus.publish(EngineEvent::new(
                "coder.artifact.added",
                json!({
                    "coder_run_id": record.coder_run_id,
                    "linked_context_run_id": record.linked_context_run_id,
                    "artifact_id": artifact.id,
                    "artifact_type": artifact.artifact_type,
                    "artifact_path": artifact.path,
                }),
            ));
            let mut run = load_context_run_state(&state, &linked_context_run_id).await?;
            run.status = ContextRunStatus::Planning;
            run.why_next_step = Some(
                "Normalize the issue reference, retrieve relevant memory, then inspect the repo."
                    .to_string(),
            );
            ensure_context_run_dir(&state, &linked_context_run_id).await?;
            save_context_run_state(&state, &run).await?;
        }
        _ => {}
    }

    let final_run = load_context_run_state(&state, &linked_context_run_id).await?;
    state.event_bus.publish(EngineEvent::new(
        "coder.run.created",
        json!({
            "coder_run_id": record.coder_run_id,
            "linked_context_run_id": record.linked_context_run_id,
            "workflow_mode": record.workflow_mode,
            "repo_slug": record.repo_binding.repo_slug,
            "github_ref": record.github_ref,
        }),
    ));

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
    Ok(Json(json!({
        "coder_run": coder_run_payload(&record, &run),
        "run": run,
    })))
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
        .unwrap_or_else(|| {
            let issue_number = record
                .github_ref
                .as_ref()
                .map(|row| row.number)
                .unwrap_or_default();
            format!("{} issue #{}", record.repo_binding.repo_slug, issue_number)
        });
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
    let issue_number = record.github_ref.as_ref().map(|row| row.number);
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
    state.event_bus.publish(EngineEvent::new(
        "coder.artifact.added",
        json!({
            "coder_run_id": record.coder_run_id,
            "linked_context_run_id": record.linked_context_run_id,
            "artifact_id": artifact.id,
            "artifact_type": artifact.artifact_type,
            "artifact_path": artifact.path,
        }),
    ));
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
