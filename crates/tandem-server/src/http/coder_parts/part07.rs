
pub(super) async fn coder_status(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    ensure_coder_runs_dir(&state).await?;
    let mut total_runs = 0_u64;
    let mut active_runs = 0_u64;
    let mut awaiting_approval_runs = 0_u64;
    let mut workflow_counts = serde_json::Map::<String, Value>::new();
    let mut status_counts = serde_json::Map::<String, Value>::new();
    let mut projects = std::collections::BTreeSet::<String>::new();
    let mut latest_run: Option<Value> = None;
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
        let Ok(run) = load_context_run_state(&state, &record.linked_context_run_id).await else {
            continue;
        };
        total_runs += 1;
        projects.insert(record.repo_binding.project_id.clone());
        let workflow_key = serde_json::to_value(&record.workflow_mode)
            .ok()
            .and_then(|row| row.as_str().map(ToString::to_string))
            .unwrap_or_else(|| "unknown".to_string());
        let workflow_count = workflow_counts
            .entry(workflow_key)
            .or_insert_with(|| json!(0))
            .as_u64()
            .unwrap_or(0);
        *workflow_counts
            .entry(
                serde_json::to_value(&record.workflow_mode)
                    .ok()
                    .and_then(|row| row.as_str().map(ToString::to_string))
                    .unwrap_or_else(|| "unknown".to_string()),
            )
            .or_insert_with(|| json!(0)) = json!(workflow_count + 1);
        let status_key = match run.status {
            ContextRunStatus::Queued => "queued",
            ContextRunStatus::Planning => "planning",
            ContextRunStatus::Running => "running",
            ContextRunStatus::AwaitingApproval => "awaiting_approval",
            ContextRunStatus::Completed => "completed",
            ContextRunStatus::Failed => "failed",
            ContextRunStatus::Paused => "paused",
            ContextRunStatus::Blocked => "blocked",
            ContextRunStatus::Cancelled => "cancelled",
        }
        .to_string();
        let status_count = status_counts
            .entry(status_key.clone())
            .or_insert_with(|| json!(0))
            .as_u64()
            .unwrap_or(0);
        *status_counts
            .entry(status_key.clone())
            .or_insert_with(|| json!(0)) = json!(status_count + 1);
        if matches!(run.status, ContextRunStatus::Running) {
            active_runs += 1;
        }
        if matches!(run.status, ContextRunStatus::AwaitingApproval) {
            awaiting_approval_runs += 1;
            active_runs += 1;
        }
        let summary = json!({
            "coder_run_id": record.coder_run_id,
            "workflow_mode": record.workflow_mode,
            "status": run.status,
            "phase": project_coder_phase(&run),
            "project_id": record.repo_binding.project_id,
            "repo_slug": record.repo_binding.repo_slug,
            "updated_at_ms": run.updated_at_ms,
        });
        if latest_run
            .as_ref()
            .and_then(|row| row.get("updated_at_ms"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
            <= run.updated_at_ms
        {
            latest_run = Some(summary);
        }
    }
    Ok(Json(json!({
        "status": {
            "total_runs": total_runs,
            "active_runs": active_runs,
            "awaiting_approval_runs": awaiting_approval_runs,
            "project_count": projects.len(),
            "workflow_counts": workflow_counts,
            "run_status_counts": status_counts,
            "latest_run": latest_run,
        }
    })))
}

pub(super) async fn coder_project_policy_put(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(input): Json<CoderProjectPolicyPutInput>,
) -> Result<Json<Value>, StatusCode> {
    let project_id = project_id.trim();
    if project_id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let policy = CoderProjectPolicy {
        project_id: project_id.to_string(),
        auto_merge_enabled: input.auto_merge_enabled,
        updated_at_ms: crate::now_ms(),
    };
    save_coder_project_policy(&state, &policy).await?;
    Ok(Json(json!({
        "ok": true,
        "project_policy": policy,
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
    maybe_sync_github_project_status(&state, record, &final_run).await?;
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
    if let Some(blocked) = coder_execution_policy_block(&state, &record).await? {
        emit_coder_execution_policy_block(&state, &record, &blocked).await?;
        let run = load_context_run_state(&state, &record.linked_context_run_id).await?;
        let mut payload = blocked;
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("coder_run".to_string(), coder_run_payload(&record, &run));
            obj.insert(
                "execution_policy".to_string(),
                coder_execution_policy_summary(&state, &record).await?,
            );
            obj.insert("run".to_string(), json!(run));
        }
        return Ok(Json(payload));
    }
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
    if let Some(blocked) = coder_execution_policy_block(&state, &record).await? {
        emit_coder_execution_policy_block(&state, &record, &blocked).await?;
        let run = load_context_run_state(&state, &record.linked_context_run_id).await?;
        let mut payload = blocked;
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("coder_run".to_string(), coder_run_payload(&record, &run));
            obj.insert(
                "execution_policy".to_string(),
                coder_execution_policy_summary(&state, &record).await?,
            );
            obj.insert("run".to_string(), json!(run));
            obj.insert("steps".to_string(), json!([]));
            obj.insert("executed_steps".to_string(), json!(0));
            obj.insert(
                "stopped_reason".to_string(),
                json!("execution_policy_blocked"),
            );
        }
        return Ok(Json(payload));
    }
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
    let audit_status = status.clone();
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
    let mut sync_record = record.clone();
    maybe_sync_github_project_status(state, &mut sync_record, &run).await?;
    let generated_candidate = ensure_terminal_run_outcome_candidate(
        state,
        &sync_record,
        &run,
        event_type,
        reason.as_deref(),
    )
    .await?;
    publish_coder_run_event(
        state,
        "coder.run.phase_changed",
        &sync_record,
        Some(project_coder_phase(&run)),
        {
            let mut extra = serde_json::Map::new();
            extra.insert("status".to_string(), json!(run.status));
            extra.insert("event_type".to_string(), json!(event_type));
            extra
        },
    );
    let _ = crate::audit::append_protected_audit_event(
        state,
        event_type,
        &tandem_types::TenantContext::local_implicit(),
        None,
        json!({
            "coderRunID": record.coder_run_id,
            "linkedContextRunID": record.linked_context_run_id,
            "status": audit_status,
            "reason": reason,
        }),
    )
    .await;
    Ok(json!({
        "ok": true,
        "event": outcome.event,
        "generated_candidates": generated_candidate
            .into_iter()
            .collect::<Vec<_>>(),
        "coder_run": coder_run_payload(&sync_record, &run),
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
    if record.workflow_mode == CoderWorkflowMode::MergeRecommendation {
        let summary_artifact =
            latest_coder_artifact(&state, &record, "coder_merge_recommendation_summary");
        let readiness_artifact =
            latest_coder_artifact(&state, &record, "coder_merge_readiness_report");
        let summary_payload = load_latest_coder_artifact_payload(
            &state,
            &record,
            "coder_merge_recommendation_summary",
        )
        .await;
        let recommendation = summary_payload
            .as_ref()
            .and_then(|row| row.get("recommendation"))
            .cloned()
            .unwrap_or_else(|| json!("merge"));
        let merge_execution_payload = json!({
            "coder_run_id": record.coder_run_id,
            "linked_context_run_id": record.linked_context_run_id,
            "workflow_mode": record.workflow_mode,
            "repo_binding": record.repo_binding,
            "github_ref": record.github_ref,
            "approved_by_reason": why,
            "recommendation": recommendation,
            "summary": summary_payload.as_ref().and_then(|row| row.get("summary")).cloned().unwrap_or(Value::Null),
            "risk_level": summary_payload.as_ref().and_then(|row| row.get("risk_level")).cloned().unwrap_or(Value::Null),
            "blockers": summary_payload.as_ref().and_then(|row| row.get("blockers")).cloned().unwrap_or_else(|| json!([])),
            "required_checks": summary_payload.as_ref().and_then(|row| row.get("required_checks")).cloned().unwrap_or_else(|| json!([])),
            "required_approvals": summary_payload.as_ref().and_then(|row| row.get("required_approvals")).cloned().unwrap_or_else(|| json!([])),
            "worker_run_reference": summary_payload.as_ref().and_then(|row| row.get("worker_run_reference")).cloned().unwrap_or(Value::Null),
            "worker_session_id": summary_payload.as_ref().and_then(|row| row.get("worker_session_id")).cloned().unwrap_or(Value::Null),
            "worker_session_run_id": summary_payload.as_ref().and_then(|row| row.get("worker_session_run_id")).cloned().unwrap_or(Value::Null),
            "worker_session_context_run_id": summary_payload.as_ref().and_then(|row| row.get("worker_session_context_run_id")).cloned().unwrap_or(Value::Null),
            "validation_run_reference": summary_payload.as_ref().and_then(|row| row.get("validation_run_reference")).cloned().unwrap_or(Value::Null),
            "validation_session_id": summary_payload.as_ref().and_then(|row| row.get("validation_session_id")).cloned().unwrap_or(Value::Null),
            "validation_session_run_id": summary_payload.as_ref().and_then(|row| row.get("validation_session_run_id")).cloned().unwrap_or(Value::Null),
            "validation_session_context_run_id": summary_payload.as_ref().and_then(|row| row.get("validation_session_context_run_id")).cloned().unwrap_or(Value::Null),
            "summary_artifact_path": summary_artifact.as_ref().map(|artifact| artifact.path.clone()),
            "readiness_artifact_path": readiness_artifact.as_ref().map(|artifact| artifact.path.clone()),
            "created_at_ms": crate::now_ms(),
        });
        let artifact = write_coder_artifact(
            &state,
            &record.linked_context_run_id,
            &format!("merge-execution-request-{}", Uuid::new_v4().simple()),
            "coder_merge_execution_request",
            "artifacts/merge_recommendation.merge_execution_request.json",
            &merge_execution_payload,
        )
        .await?;
        let merge_submit_policy = coder_merge_submit_policy_summary(&state, &record).await?;
        if !matches!(merge_submit_policy, Value::Null) {
            let mut payload = merge_execution_payload
                .as_object()
                .cloned()
                .unwrap_or_default();
            payload.insert(
                "merge_submit_policy_preview".to_string(),
                merge_submit_policy.clone(),
            );
            tokio::fs::write(
                &artifact.path,
                serde_json::to_string_pretty(&Value::Object(payload))
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
            )
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        }
        publish_coder_artifact_added(&state, &record, &artifact, Some("approval"), {
            let mut extra = serde_json::Map::new();
            extra.insert("kind".to_string(), json!("merge_execution_request"));
            extra.insert("recommendation".to_string(), recommendation.clone());
            extra
        });
        publish_coder_run_event(
            &state,
            "coder.merge.recommended",
            &record,
            Some("approval"),
            {
                let mut extra = serde_json::Map::new();
                extra.insert(
                    "event_type".to_string(),
                    json!("merge_execution_request_ready"),
                );
                extra.insert("artifact_id".to_string(), json!(artifact.id));
                extra.insert("recommendation".to_string(), recommendation);
                extra.insert(
                    "merge_submit_policy".to_string(),
                    merge_submit_policy.clone(),
                );
                extra
            },
        );
        let mut response = coder_run_transition(
            &state,
            &record,
            "merge_recommendation_approved",
            ContextRunStatus::Completed,
            Some(
                merge_execution_payload
                    .get("approved_by_reason")
                    .and_then(Value::as_str)
                    .unwrap_or("merge recommendation approved by operator")
                    .to_string(),
            ),
        )
        .await?;
        if let Some(obj) = response.as_object_mut() {
            obj.insert(
                "merge_execution_request".to_string(),
                merge_execution_payload,
            );
            obj.insert("merge_execution_artifact".to_string(), json!(artifact));
            obj.insert("merge_submit_policy".to_string(), merge_submit_policy);
        }
        return Ok(Json(attach_worker_reference_fields(
            response,
            summary_payload.as_ref(),
            summary_payload.as_ref(),
        )));
    }
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
        collect_coder_memory_hits(&state, &record, &search_query, query.limit.unwrap_or(8)).await?;
    Ok(Json(json!({
        "coder_run_id": record.coder_run_id,
        "query": search_query,
        "retrieval_policy": coder_memory_retrieval_policy(
            &record,
            &search_query,
            query.limit.unwrap_or(8),
        ),
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
    if !coder_memory_candidate_promotion_allowed(&kind, &candidate_payload) {
        return Err(StatusCode::BAD_REQUEST);
    }
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
    let tenant_context = tandem_types::TenantContext::local_implicit();
    let put_response = super::skills_memory::memory_put_impl(
        &state,
        &tenant_context,
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
                CoderMemoryCandidateKind::DuplicateLinkage => MemoryContentKind::Fact,
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
                "linked_pr_numbers": candidate_payload
                    .get("payload")
                    .and_then(|row| row.get("linked_pr_numbers"))
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
                    &tenant_context,
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
    let (inferred_duplicate_candidates, inferred_prior_runs_considered, inferred_memory_hits_used) =
        infer_triage_summary_enrichment(&state, &record).await;
    let duplicate_candidates = if input.duplicate_candidates.is_empty() {
        inferred_duplicate_candidates
    } else {
        input.duplicate_candidates.clone()
    };
    let prior_runs_considered = if input.prior_runs_considered.is_empty() {
        inferred_prior_runs_considered
    } else {
        input.prior_runs_considered.clone()
    };
    let memory_hits_used = if input.memory_hits_used.is_empty() {
        inferred_memory_hits_used
    } else {
        input.memory_hits_used.clone()
    };
    let payload = json!({
        "coder_run_id": record.coder_run_id,
        "linked_context_run_id": record.linked_context_run_id,
        "workflow_mode": record.workflow_mode,
        "repo_binding": record.repo_binding,
        "github_ref": record.github_ref,
        "summary": input.summary,
        "confidence": input.confidence,
        "affected_files": input.affected_files,
        "duplicate_candidates": duplicate_candidates.clone(),
        "prior_runs_considered": prior_runs_considered.clone(),
        "memory_hits_used": memory_hits_used.clone(),
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
    let reproduction_outcome = input
        .reproduction
        .as_ref()
        .and_then(|row| row.get("outcome"))
        .and_then(Value::as_str)
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
                "duplicate_candidates": duplicate_candidates.clone(),
                "prior_runs_considered": prior_runs_considered.clone(),
                "memory_hits_used": memory_hits_used.clone(),
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
                &duplicate_candidates,
                input.notes.as_deref(),
            ),
        )
        .await?;
        generated_candidates.push(json!({
            "candidate_id": failure_pattern_id,
            "kind": "failure_pattern",
            "artifact_path": failure_pattern_artifact.path,
        }));

        if let Some(duplicate_linkage_payload) =
            build_inferred_duplicate_linkage_payload(&record, &duplicate_candidates, &artifact.path)
        {
            let (duplicate_linkage_id, duplicate_linkage_artifact) =
                write_coder_memory_candidate_artifact(
                    &state,
                    &record,
                    CoderMemoryCandidateKind::DuplicateLinkage,
                    Some(format!("Issue triage duplicate linkage: {summary_text}")),
                    Some("write_triage_artifact".to_string()),
                    duplicate_linkage_payload,
                )
                .await?;
            generated_candidates.push(json!({
                "candidate_id": duplicate_linkage_id,
                "kind": "duplicate_linkage",
                "artifact_path": duplicate_linkage_artifact.path,
            }));
        }
    }
    let outcome = if duplicate_candidates.is_empty() {
        "triaged"
    } else {
        "triaged_duplicate_candidate"
    };
    let outcome_summary = triage_summary
        .clone()
        .or_else(|| {
            reproduction_outcome
                .as_ref()
                .map(|outcome_text| format!("Issue triage reproduction outcome: {outcome_text}"))
        })
        .or_else(|| {
            input
                .notes
                .as_deref()
                .map(str::trim)
                .filter(|row| !row.is_empty())
                .map(ToString::to_string)
        });
    if let Some(summary_text) = outcome_summary {
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
                "prior_runs_considered": prior_runs_considered.clone(),
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
        ContextRunStatus::Completed,
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
    let (inferred_duplicate_candidates, inferred_prior_runs_considered, inferred_memory_hits_used) =
        infer_triage_summary_enrichment(&state, &record).await;
    let memory_hits_used = if input.memory_hits_used.is_empty() {
        inferred_memory_hits_used
    } else {
        input.memory_hits_used.clone()
    };
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
        "memory_hits_used": memory_hits_used.clone(),
        "duplicate_candidates": inferred_duplicate_candidates,
        "prior_runs_considered": inferred_prior_runs_considered,
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
    let mut generated_candidates = Vec::<Value>::new();
    if triage_reproduction_outcome_failed(input.outcome.as_deref()) {
        let outcome_text = input
            .outcome
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("failed_to_reproduce");
        let summary_text = input
            .summary
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                input
                    .notes
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(ToString::to_string)
            })
            .unwrap_or_else(|| format!("Issue triage reproduction outcome: {outcome_text}"));
        let (regression_signal_id, regression_signal_artifact) =
            write_coder_memory_candidate_artifact(
                &state,
                &record,
                CoderMemoryCandidateKind::RegressionSignal,
                Some(format!("Issue triage regression signal: {outcome_text}")),
                Some("attempt_reproduction".to_string()),
                json!({
                    "workflow_mode": "issue_triage",
                    "result": "triage_reproduction_failed",
                    "summary": summary_text,
                    "regression_signals": [{
                        "kind": "triage_reproduction_failed",
                        "summary": summary_text,
                        "observed_logs": input.observed_logs,
                        "steps": input.steps,
                    }],
                    "affected_files": input.affected_files,
                    "memory_hits_used": memory_hits_used,
                    "reproduction_artifact_path": artifact.path,
                }),
            )
            .await?;
        generated_candidates.push(json!({
            "candidate_id": regression_signal_id,
            "kind": "regression_signal",
            "artifact_path": regression_signal_artifact.path,
        }));
        let (run_outcome_id, run_outcome_artifact) = write_coder_memory_candidate_artifact(
            &state,
            &record,
            CoderMemoryCandidateKind::RunOutcome,
            Some(format!("Issue triage reproduction failed: {outcome_text}")),
            Some("attempt_reproduction".to_string()),
            json!({
                "workflow_mode": "issue_triage",
                "result": "triage_reproduction_failed",
                "summary": summary_text,
                "reproduction": {
                    "outcome": outcome_text,
                    "steps": input.steps,
                    "observed_logs": input.observed_logs,
                },
                "affected_files": input.affected_files,
                "memory_hits_used": memory_hits_used,
                "follow_up_recommended": true,
                "follow_up_mode": "issue_triage",
                "reproduction_artifact_path": artifact.path,
            }),
        )
        .await?;
        generated_candidates.push(json!({
            "candidate_id": run_outcome_id,
            "kind": "run_outcome",
            "artifact_path": run_outcome_artifact.path,
        }));
    }
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
        "generated_candidates": generated_candidates,
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
                    "html_url": "https://github.com/user123/tandem/pull/42",
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
                    "url": "https://github.com/user123/tandem/pull/42",
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
            Some("https://github.com/user123/tandem/pull/42")
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
            html_url: Some("https://github.com/user123/tandem/pull/77".to_string()),
            head_ref: Some("coder/issue-313-fix".to_string()),
            base_ref: Some("main".to_string()),
        };

        assert_eq!(
            github_ref_from_pull_request(&pull),
            json!({
                "kind": "pull_request",
                "number": 77,
                "url": "https://github.com/user123/tandem/pull/77",
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
    let validation_artifact = write_workflow_validation_artifact(
        &state,
        &record,
        "pr-review-validation",
        "artifacts/pr_review.validation.json",
        input.summary.as_deref(),
        &input.validation_steps,
        &input.validation_results,
        &input.memory_hits_used,
        input.notes.as_deref(),
        Some(&artifact.path),
        json!({
            "verdict": input.verdict.clone(),
            "risk_level": input.risk_level.clone(),
            "changed_files": input.changed_files.clone(),
            "blockers": input.blockers.clone(),
            "requested_changes": input.requested_changes.clone(),
            "regression_signals": input.regression_signals.clone(),
        }),
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
        ContextRunStatus::Completed,
        "PR review summary recorded.",
    )
    .await?;
    record.updated_at_ms = final_run.updated_at_ms;
    save_coder_run_record(&state, &record).await?;
    let worker_payload =
        load_latest_coder_artifact_payload(&state, &record, "coder_pr_review_worker_session").await;
    Ok(Json(attach_worker_reference_fields(
        json!({
            "ok": true,
            "artifact": artifact,
            "review_evidence_artifact": review_evidence_artifact,
            "validation_artifact": validation_artifact,
            "generated_candidates": generated_candidates,
            "coder_run": coder_run_payload(&record, &final_run),
            "run": final_run,
        }),
        worker_payload.as_ref(),
        None,
    )))
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
    let worker_payload =
        load_latest_coder_artifact_payload(&state, &record, "coder_pr_review_worker_session").await;
    Ok(Json(attach_worker_reference_fields(
        json!({
            "ok": true,
            "artifact": artifact,
            "coder_run": coder_run_payload(&record, &final_run),
            "run": final_run,
        }),
        worker_payload.as_ref(),
        None,
    )))
}
