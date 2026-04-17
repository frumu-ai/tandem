
async fn seed_pr_review_tasks(
    state: AppState,
    coder_run: &CoderRunRecord,
) -> Result<(), StatusCode> {
    let run_id = coder_run.linked_context_run_id.clone();
    let workflow_id = "coder_pr_review".to_string();
    let retrieval_query = default_coder_memory_query(coder_run);
    let memory_hits = collect_coder_memory_hits(&state, coder_run, &retrieval_query, 6).await?;
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
    let memory_hits = collect_coder_memory_hits(&state, coder_run, &retrieval_query, 6).await?;
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
    let memory_hits = collect_coder_memory_hits(&state, coder_run, &retrieval_query, 6).await?;
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
    let artifact = latest_coder_artifact(state, record, artifact_type)?;
    let raw = tokio::fs::read_to_string(&artifact.path).await.ok()?;
    serde_json::from_str::<Value>(&raw).ok()
}

async fn coder_run_has_run_outcome_candidate(state: &AppState, record: &CoderRunRecord) -> bool {
    let blackboard = load_context_blackboard(state, &record.linked_context_run_id);
    for artifact in blackboard.artifacts.iter().rev() {
        if artifact.artifact_type != "coder_memory_candidate" {
            continue;
        }
        let Ok(raw) = tokio::fs::read_to_string(&artifact.path).await else {
            continue;
        };
        let Ok(payload) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        if payload.get("coder_run_id").and_then(Value::as_str) != Some(record.coder_run_id.as_str())
        {
            continue;
        }
        if payload.get("kind").and_then(Value::as_str) == Some("run_outcome") {
            return true;
        }
    }
    false
}

fn coder_workflow_mode_label(mode: &CoderWorkflowMode) -> &'static str {
    match mode {
        CoderWorkflowMode::IssueTriage => "Issue triage",
        CoderWorkflowMode::IssueFix => "Issue fix",
        CoderWorkflowMode::PrReview => "PR review",
        CoderWorkflowMode::MergeRecommendation => "Merge recommendation",
    }
}

async fn ensure_terminal_run_outcome_candidate(
    state: &AppState,
    record: &CoderRunRecord,
    run: &ContextRunState,
    event_type: &str,
    reason: Option<&str>,
) -> Result<Option<Value>, StatusCode> {
    if !matches!(
        run.status,
        ContextRunStatus::Completed | ContextRunStatus::Failed | ContextRunStatus::Cancelled
    ) {
        return Ok(None);
    }
    if coder_run_has_run_outcome_candidate(state, record).await {
        return Ok(None);
    }
    let result = match run.status {
        ContextRunStatus::Completed => "completed",
        ContextRunStatus::Failed => "failed",
        ContextRunStatus::Cancelled => "cancelled",
        _ => return Ok(None),
    };
    let summary = reason
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            format!(
                "{} {} via {}",
                coder_workflow_mode_label(&record.workflow_mode),
                result,
                event_type
            )
        });
    let (candidate_id, artifact) = write_coder_memory_candidate_artifact(
        state,
        record,
        CoderMemoryCandidateKind::RunOutcome,
        Some(format!(
            "{} {}",
            coder_workflow_mode_label(&record.workflow_mode),
            result
        )),
        None,
        json!({
            "workflow_mode": record.workflow_mode,
            "result": result,
            "summary": summary,
            "event_type": event_type,
            "final_status": run.status,
            "final_phase": project_coder_phase(run),
            "reason": reason,
        }),
    )
    .await?;
    Ok(Some(json!({
        "candidate_id": candidate_id,
        "kind": "run_outcome",
        "artifact_path": artifact.path,
    })))
}

async fn write_worker_failure_run_outcome_candidate(
    state: &AppState,
    record: &CoderRunRecord,
    task_id: &str,
    worker_artifact_type: &str,
    result: &str,
    summary: &str,
) -> Result<Option<Value>, StatusCode> {
    if coder_run_has_run_outcome_candidate(state, record).await {
        return Ok(None);
    }
    let worker_artifact = latest_coder_artifact(state, record, worker_artifact_type);
    let worker_payload =
        load_latest_coder_artifact_payload(state, record, worker_artifact_type).await;
    let (candidate_id, artifact) = write_coder_memory_candidate_artifact(
        state,
        record,
        CoderMemoryCandidateKind::RunOutcome,
        Some(summary.to_string()),
        Some(task_id.to_string()),
        json!({
            "workflow_mode": record.workflow_mode,
            "result": result,
            "summary": summary,
            "worker_artifact_type": worker_artifact_type,
            "worker_artifact_path": worker_artifact.as_ref().map(|row| row.path.clone()),
            "worker_run_reference": worker_payload
                .as_ref()
                .map(preferred_session_run_reference)
                .unwrap_or(Value::Null),
            "worker_session_id": worker_payload
                .as_ref()
                .and_then(|row| row.get("session_id"))
                .cloned()
                .unwrap_or(Value::Null),
            "worker_session_run_id": worker_payload
                .as_ref()
                .and_then(|row| row.get("session_run_id"))
                .cloned()
                .unwrap_or(Value::Null),
            "worker_session_context_run_id": worker_payload
                .as_ref()
                .and_then(|row| row.get("session_context_run_id"))
                .cloned()
                .unwrap_or(Value::Null),
            "worker_error": worker_payload
                .as_ref()
                .and_then(|row| row.get("error"))
                .cloned()
                .unwrap_or(Value::Null),
            "worker_status": worker_payload
                .as_ref()
                .and_then(|row| row.get("status"))
                .cloned()
                .unwrap_or_else(|| json!("error")),
        }),
    )
    .await?;
    Ok(Some(json!({
        "candidate_id": candidate_id,
        "kind": "run_outcome",
        "artifact_path": artifact.path,
    })))
}

fn infer_triage_memory_hit_ids_from_hits(hits: &[Value], limit: usize) -> Vec<String> {
    let mut ids = Vec::<String>::new();
    let mut seen = HashSet::<String>::new();
    for hit in hits {
        let Some(id) =
            value_string(hit.get("candidate_id")).or_else(|| value_string(hit.get("memory_id")))
        else {
            continue;
        };
        if !seen.insert(id.clone()) {
            continue;
        }
        ids.push(id);
        if ids.len() >= limit.clamp(1, 20) {
            break;
        }
    }
    ids
}

fn infer_triage_prior_runs_from_hits(hits: &[Value], limit: usize) -> Vec<Value> {
    let mut rows = Vec::<Value>::new();
    let mut seen = HashSet::<String>::new();
    for hit in hits {
        let coder_run_id = value_string(hit.get("source_coder_run_id"));
        let run_id = value_string(hit.get("run_id"));
        let identity = coder_run_id
            .clone()
            .or_else(|| run_id.clone())
            .or_else(|| value_string(hit.get("candidate_id")))
            .or_else(|| value_string(hit.get("memory_id")));
        let Some(identity) = identity else {
            continue;
        };
        if !seen.insert(identity) {
            continue;
        }
        let mut row = serde_json::Map::new();
        if let Some(value) = coder_run_id {
            row.insert("coder_run_id".to_string(), json!(value));
        }
        if let Some(value) = run_id {
            row.insert("linked_context_run_id".to_string(), json!(value));
        }
        if let Some(kind) = memory_hit_kind(hit) {
            row.insert("kind".to_string(), json!(kind));
        }
        if let Some(source) = value_string(hit.get("source")) {
            row.insert("source".to_string(), json!(source));
        }
        if let Some(candidate_id) = value_string(hit.get("candidate_id")) {
            row.insert("candidate_id".to_string(), json!(candidate_id));
        }
        if let Some(memory_id) = value_string(hit.get("memory_id")) {
            row.insert("memory_id".to_string(), json!(memory_id));
        }
        if !row.is_empty() {
            rows.push(Value::Object(row));
        }
        if rows.len() >= limit.clamp(1, 20) {
            break;
        }
    }
    rows
}

fn fallback_failure_pattern_duplicates_from_hits(hits: &[Value], limit: usize) -> Vec<Value> {
    let mut rows = Vec::<Value>::new();
    let mut seen = HashSet::<String>::new();
    for hit in hits {
        if memory_hit_kind(hit).as_deref() != Some("failure_pattern") {
            continue;
        }
        let identity = value_string(hit.get("candidate_id"))
            .or_else(|| value_string(hit.get("memory_id")))
            .or_else(|| value_string(hit.get("summary")))
            .or_else(|| value_string(hit.get("content")));
        let Some(identity) = identity else {
            continue;
        };
        if !seen.insert(identity) {
            continue;
        }
        rows.push(json!({
            "kind": "failure_pattern",
            "source": hit.get("source").cloned().unwrap_or(Value::Null),
            "match_reason": "historical_failure_pattern",
            "score": hit.get("score").cloned().unwrap_or_else(|| json!(0)),
            "summary": hit.get("summary").cloned().unwrap_or_else(|| hit.get("content").cloned().unwrap_or(Value::Null)),
            "candidate_id": hit.get("candidate_id").cloned().unwrap_or(Value::Null),
            "memory_id": hit.get("memory_id").cloned().unwrap_or(Value::Null),
            "artifact_path": hit.get("path").cloned().unwrap_or(Value::Null),
            "run_id": hit.get("run_id").cloned().unwrap_or_else(|| hit.get("source_coder_run_id").cloned().unwrap_or(Value::Null)),
        }));
        if rows.len() >= limit.clamp(1, 8) {
            break;
        }
    }
    rows
}

async fn infer_triage_summary_enrichment(
    state: &AppState,
    record: &CoderRunRecord,
) -> (Vec<Value>, Vec<Value>, Vec<String>) {
    let memory_hits_payload =
        load_latest_coder_artifact_payload(state, record, "coder_memory_hits")
            .await
            .unwrap_or(Value::Null);
    let hits = memory_hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let duplicate_matches_payload =
        load_latest_coder_artifact_payload(state, record, "coder_duplicate_matches").await;
    let mut duplicate_candidates = duplicate_matches_payload
        .as_ref()
        .and_then(|payload| payload.get("matches"))
        .and_then(Value::as_array)
        .cloned()
        .or_else(|| {
            memory_hits_payload
                .get("duplicate_candidates")
                .and_then(Value::as_array)
                .cloned()
        })
        .unwrap_or_else(|| derive_failure_pattern_duplicate_matches(&hits, None, 3));
    if duplicate_candidates.is_empty() {
        duplicate_candidates = fallback_failure_pattern_duplicates_from_hits(&hits, 3);
    }
    let inferred_linkage_candidates = derive_duplicate_linkage_candidates_from_hits(&hits, 3);
    if !inferred_linkage_candidates.is_empty() {
        duplicate_candidates.extend(inferred_linkage_candidates);
        let mut seen_pairs = HashSet::<(Option<u64>, Vec<u64>)>::new();
        duplicate_candidates.retain(|candidate| {
            let pr_number = candidate.get("number").and_then(Value::as_u64);
            let mut linked_prs = candidate_linked_numbers(candidate, "linked_pr_numbers");
            linked_prs.sort_unstable();
            seen_pairs.insert((pr_number, linked_prs))
        });
    }
    let prior_runs_considered = infer_triage_prior_runs_from_hits(&hits, 8);
    let memory_hits_used = infer_triage_memory_hit_ids_from_hits(&hits, 8);
    (
        duplicate_candidates,
        prior_runs_considered,
        memory_hits_used,
    )
}

fn latest_coder_artifact(
    state: &AppState,
    record: &CoderRunRecord,
    artifact_type: &str,
) -> Option<ContextBlackboardArtifact> {
    let blackboard = load_context_blackboard(state, &record.linked_context_run_id);
    blackboard
        .artifacts
        .iter()
        .rev()
        .find(|artifact| artifact.artifact_type == artifact_type)
        .cloned()
}

async fn serialize_coder_artifacts(artifacts: &[ContextBlackboardArtifact]) -> Vec<Value> {
    let mut serialized = Vec::with_capacity(artifacts.len());
    for artifact in artifacts {
        let mut row = json!({
            "id": artifact.id,
            "ts_ms": artifact.ts_ms,
            "path": artifact.path,
            "artifact_type": artifact.artifact_type,
            "step_id": artifact.step_id,
            "source_event_id": artifact.source_event_id,
        });
        match tokio::fs::read_to_string(&artifact.path).await {
            Ok(raw) => {
                let mut extras = serde_json::Map::new();
                extras.insert("exists".to_string(), json!(true));
                extras.insert("byte_size".to_string(), json!(raw.len()));
                match serde_json::from_str::<Value>(&raw) {
                    Ok(payload) => {
                        extras.insert("payload_format".to_string(), json!("json"));
                        extras.insert("payload".to_string(), payload);
                    }
                    Err(_) => {
                        extras.insert("payload_format".to_string(), json!("text"));
                        extras.insert(
                            "payload_text".to_string(),
                            json!(crate::truncate_text(&raw, 8_000)),
                        );
                    }
                }
                if let Some(obj) = row.as_object_mut() {
                    obj.extend(extras);
                }
            }
            Err(error) => {
                if let Some(obj) = row.as_object_mut() {
                    obj.insert("exists".to_string(), json!(false));
                    obj.insert(
                        "load_error".to_string(),
                        json!(crate::truncate_text(&error.to_string(), 240)),
                    );
                }
            }
        }
        serialized.push(row);
    }
    serialized
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

fn build_issue_triage_worker_prompt(
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
            "You are the Tandem coder issue-triage worker.\n",
            "Repository: {repo_slug}\n",
            "Workspace root: {workspace_root}\n",
            "Issue number: #{issue_number}\n",
            "Context run ID: {context_run_id}\n",
            "Memory hits already surfaced: {memory_hint}\n\n",
            "Task:\n",
            "1. Inspect the repository and issue context.\n",
            "2. Identify likely affected areas.\n",
            "3. Attempt a constrained reproduction plan.\n",
            "4. Report the most likely next triage conclusion.\n\n",
            "Return a compact response with these headings:\n",
            "Summary:\n",
            "Confidence:\n",
            "Likely Areas:\n",
            "Affected Files:\n",
            "Reproduction Outcome:\n",
            "Reproduction Steps:\n",
            "Observed Logs:\n"
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
        "Confidence:",
        "Likely Areas:",
        "Affected Files:",
        "Reproduction Outcome:",
        "Reproduction Steps:",
        "Observed Logs:",
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

fn summarize_workflow_prior_runs_considered(
    _record: &CoderRunRecord,
    run: &ContextRunState,
    workflow_node_id: &str,
) -> Vec<Value> {
    let mut seen = std::collections::HashSet::<String>::new();
    run.tasks
        .iter()
        .find(|task| task.workflow_node_id.as_deref() == Some(workflow_node_id))
        .and_then(|task| task.payload.get("memory_hits"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let run_id = row
                        .get("source_coder_run_id")
                        .or_else(|| row.get("run_id"))
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())?;
                    if !seen.insert(run_id.to_string()) {
                        return None;
                    }
                    Some(json!({
                        "coder_run_id": run_id,
                        "linked_context_run_id": row.get("linked_context_run_id").cloned().unwrap_or(Value::Null),
                        "kind": row.get("kind").cloned().unwrap_or(Value::Null),
                        "tier": row.get("tier").cloned().unwrap_or(Value::Null),
                    }))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn summarize_workflow_duplicate_candidates(
    _record: &CoderRunRecord,
    run: &ContextRunState,
    workflow_node_id: &str,
) -> Vec<Value> {
    run.tasks
        .iter()
        .find(|task| task.workflow_node_id.as_deref() == Some(workflow_node_id))
        .and_then(|task| task.payload.get("duplicate_candidates"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

fn preferred_session_run_reference(session_payload: &Value) -> Value {
    session_payload
        .get("session_context_run_id")
        .cloned()
        .or_else(|| session_payload.get("session_id").cloned())
        .unwrap_or(Value::Null)
}

fn normalize_session_run_payload(session_payload: &Value) -> Value {
    let mut normalized = session_payload.clone();
    if let Some(obj) = normalized.as_object_mut() {
        obj.entry("worker_run_reference".to_string())
            .or_insert_with(|| preferred_session_run_reference(session_payload));
    }
    normalized
}

fn attach_worker_dispatch_reference(payload: Value, worker_payload: Option<&Value>) -> Value {
    let Some(worker_payload) = worker_payload else {
        return payload;
    };
    let mut payload = payload;
    if let Some(obj) = payload.as_object_mut() {
        obj.entry("worker_run_reference".to_string())
            .or_insert_with(|| preferred_session_run_reference(worker_payload));
        obj.entry("worker_session_id".to_string())
            .or_insert_with(|| {
                worker_payload
                    .get("session_id")
                    .cloned()
                    .unwrap_or(Value::Null)
            });
        obj.entry("worker_session_run_id".to_string())
            .or_insert_with(|| {
                worker_payload
                    .get("session_run_id")
                    .cloned()
                    .unwrap_or(Value::Null)
            });
        obj.entry("worker_session_context_run_id".to_string())
            .or_insert_with(|| {
                worker_payload
                    .get("session_context_run_id")
                    .cloned()
                    .unwrap_or(Value::Null)
            });
    }
    payload
}

fn attach_worker_reference_fields(
    payload: Value,
    worker_payload: Option<&Value>,
    validation_payload: Option<&Value>,
) -> Value {
    let mut payload = payload;
    if let Some(obj) = payload.as_object_mut() {
        obj.insert(
            "worker_run_reference".to_string(),
            worker_payload
                .map(preferred_session_run_reference)
                .unwrap_or(Value::Null),
        );
        obj.insert(
            "worker_session_id".to_string(),
            worker_payload
                .and_then(|row| row.get("session_id"))
                .cloned()
                .unwrap_or(Value::Null),
        );
        obj.insert(
            "worker_session_run_id".to_string(),
            worker_payload
                .and_then(|row| row.get("session_run_id"))
                .cloned()
                .unwrap_or(Value::Null),
        );
        obj.insert(
            "worker_session_context_run_id".to_string(),
            worker_payload
                .and_then(|row| row.get("session_context_run_id"))
                .cloned()
                .unwrap_or(Value::Null),
        );
        obj.insert(
            "validation_run_reference".to_string(),
            validation_payload
                .map(preferred_session_run_reference)
                .unwrap_or(Value::Null),
        );
        obj.insert(
            "validation_session_id".to_string(),
            validation_payload
                .and_then(|row| row.get("session_id"))
                .cloned()
                .unwrap_or(Value::Null),
        );
        obj.insert(
            "validation_session_run_id".to_string(),
            validation_payload
                .and_then(|row| row.get("session_run_id"))
                .cloned()
                .unwrap_or(Value::Null),
        );
        obj.insert(
            "validation_session_context_run_id".to_string(),
            validation_payload
                .and_then(|row| row.get("session_context_run_id"))
                .cloned()
                .unwrap_or(Value::Null),
        );
    }
    payload
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
        "worker_run_reference": preferred_session_run_reference(worker_payload),
        "worker_session_id": worker_payload.get("session_id").cloned(),
        "worker_session_run_id": worker_payload.get("session_run_id").cloned(),
        "worker_session_context_run_id": worker_payload.get("session_context_run_id").cloned(),
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
        "worker_run_reference": preferred_session_run_reference(worker_payload),
        "worker_session_id": worker_payload.get("session_id").cloned(),
        "worker_session_run_id": worker_payload.get("session_run_id").cloned(),
        "worker_session_context_run_id": worker_payload.get("session_context_run_id").cloned(),
        "worker_model": worker_payload.get("model").cloned(),
        "assistant_text": worker_payload.get("assistant_text").cloned(),
    })
}

fn parse_issue_triage_from_worker_payload(worker_payload: &Value) -> Value {
    let assistant_text = worker_payload
        .get("assistant_text")
        .and_then(Value::as_str)
        .unwrap_or("");
    let summary = extract_labeled_section(assistant_text, "Summary").or_else(|| {
        (!assistant_text.trim().is_empty()).then(|| crate::truncate_text(assistant_text, 240))
    });
    let confidence = extract_labeled_section(assistant_text, "Confidence");
    let likely_areas =
        parse_bulleted_lines(extract_labeled_section(assistant_text, "Likely Areas"));
    let affected_files =
        parse_bulleted_lines(extract_labeled_section(assistant_text, "Affected Files"));
    let reproduction_outcome = extract_labeled_section(assistant_text, "Reproduction Outcome");
    let reproduction_steps = parse_bulleted_lines(extract_labeled_section(
        assistant_text,
        "Reproduction Steps",
    ));
    let observed_logs =
        parse_bulleted_lines(extract_labeled_section(assistant_text, "Observed Logs"));
    json!({
        "summary": summary,
        "confidence": confidence,
        "likely_areas": likely_areas,
        "affected_files": affected_files,
        "reproduction_outcome": reproduction_outcome,
        "reproduction_steps": reproduction_steps,
        "observed_logs": observed_logs,
        "worker_run_reference": preferred_session_run_reference(worker_payload),
        "worker_session_id": worker_payload.get("session_id").cloned(),
        "worker_session_run_id": worker_payload.get("session_run_id").cloned(),
        "worker_session_context_run_id": worker_payload.get("session_context_run_id").cloned(),
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
        "worker_run_reference": preferred_session_run_reference(worker_payload),
        "worker_session_id": worker_payload.get("session_id").cloned(),
        "worker_session_run_id": worker_payload.get("session_run_id").cloned(),
        "worker_session_context_run_id": worker_payload.get("session_context_run_id").cloned(),
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
        "worker_run_reference": preferred_session_run_reference(worker_payload),
        "worker_session_id": worker_payload.get("session_id").cloned(),
        "worker_session_run_id": worker_payload.get("session_run_id").cloned(),
        "worker_session_context_run_id": worker_payload.get("session_context_run_id").cloned(),
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
        "worker_run_reference": worker_session
            .map(preferred_session_run_reference)
            .unwrap_or(Value::Null),
        "worker_session_id": worker_session.and_then(|payload| payload.get("session_id")).cloned(),
        "worker_session_run_id": worker_session.and_then(|payload| payload.get("session_run_id")).cloned(),
        "worker_session_context_run_id": worker_session.and_then(|payload| payload.get("session_context_run_id")).cloned(),
        "validation_run_reference": validation_session
            .map(preferred_session_run_reference)
            .unwrap_or(Value::Null),
        "validation_session_id": validation_session.and_then(|payload| payload.get("session_id")).cloned(),
        "validation_session_run_id": validation_session.and_then(|payload| payload.get("session_run_id")).cloned(),
        "validation_session_context_run_id": validation_session.and_then(|payload| payload.get("session_context_run_id")).cloned(),
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

fn normalize_status_alias(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-', '_'], "")
}

fn status_alias_matches(name: &str, aliases: &[&str]) -> bool {
    let normalized = normalize_status_alias(name);
    aliases
        .iter()
        .any(|alias| normalized == normalize_status_alias(alias))
}

fn hash_json_fingerprint(value: &Value) -> Result<String, StatusCode> {
    let bytes = serde_json::to_vec(value).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let digest = sha2::Sha256::digest(bytes);
    Ok(format!("{digest:x}"))
}

fn context_status_to_project_option(
    mapping: &CoderGithubProjectStatusMapping,
    status: &ContextRunStatus,
) -> CoderGithubProjectStatusOption {
    match status {
        ContextRunStatus::Queued | ContextRunStatus::Planning => mapping.todo.clone(),
        ContextRunStatus::Running | ContextRunStatus::Paused => mapping.in_progress.clone(),
        ContextRunStatus::AwaitingApproval => mapping.in_review.clone(),
        ContextRunStatus::Completed => mapping.done.clone(),
        ContextRunStatus::Blocked | ContextRunStatus::Failed | ContextRunStatus::Cancelled => {
            mapping.blocked.clone()
        }
    }
}

fn is_terminal_context_status(status: &ContextRunStatus) -> bool {
    matches!(
        status,
        ContextRunStatus::Completed
            | ContextRunStatus::Failed
            | ContextRunStatus::Cancelled
            | ContextRunStatus::Blocked
    )
}

fn coder_run_sync_state(record: &CoderRunRecord) -> CoderRemoteSyncState {
    record
        .remote_sync_state
        .clone()
        .unwrap_or(CoderRemoteSyncState::InSync)
}
