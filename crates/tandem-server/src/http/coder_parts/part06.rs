async fn run_issue_fix_worker_session(
    state: &AppState,
    record: &CoderRunRecord,
    task_id: Option<&str>,
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
    let managed_worktree = prepare_coder_worker_workspace(
        state,
        &record.repo_binding.workspace_root,
        task_id,
        &record.linked_context_run_id,
        worker_kind,
    )
    .await;
    let canonical_repo_root = managed_worktree
        .as_ref()
        .map(|result| result.record.repo_root.clone())
        .or_else(|| {
            crate::runtime::worktrees::resolve_git_repo_root(&record.repo_binding.workspace_root)
        })
        .unwrap_or_else(|| record.repo_binding.workspace_root.clone());
    let worker_workspace_root = managed_worktree
        .as_ref()
        .map(|result| result.record.path.clone())
        .unwrap_or_else(|| record.repo_binding.workspace_root.clone());
    let result = async {
        let mut session = Session::new(
            Some(session_title),
            Some(worker_workspace_root.clone()),
        );
        session.project_id = Some(record.repo_binding.project_id.clone());
        session.workspace_root = Some(worker_workspace_root.clone());
        session.environment = Some(state.host_runtime_context());
        session.provider = Some(model.provider_id.clone());
        session.model = Some(model.clone());
        let session_id = session.id.clone();
        state
            .storage
            .save_session(session.clone())
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let worker_context_run_id =
            super::context_runs::ensure_session_context_run(state, &session).await?;

        let run_id = Uuid::new_v4().to_string();
        let client_id = Some(record.coder_run_id.clone());
        let agent_id = Some("coder_issue_fix_worker".to_string());
        let tenant_context = session.tenant_context.clone();
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
            serde_json::json!({
                "sessionID": session_id,
                "runID": run_id,
                "startedAtMs": active_run.started_at_ms,
                "clientID": active_run.client_id,
                "agentID": active_run.agent_id,
                "agentProfile": active_run.agent_profile,
                "environment": state.host_runtime_context(),
                "tenantContext": tenant_context.clone(),
            }),
        ));

        let request = SendMessageRequest {
            parts: vec![MessagePartInput::Text {
                text: format!(
                    "Managed worker workspace: {worker_workspace_root}\nCanonical repo root: {canonical_repo_root}\n\n{}",
                    prompt
                ),
            }],
            model: Some(model.clone()),
            agent: agent_id.clone().or_else(|| Some(worker_kind.to_string())),
            tool_mode: Some(tandem_types::ToolMode::Auto),
            tool_allowlist: None,
            strict_kb_grounding: None,
            context_mode: Some(tandem_types::ContextMode::Full),
            write_required: Some(true),
            prewrite_requirements: None,
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
            tenant_context.clone(),
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
            "task_id": task_id,
            "worker_workspace_root": worker_workspace_root,
            "worker_workspace_repo_root": canonical_repo_root,
            "worker_workspace_branch": managed_worktree.as_ref().map(|row| row.record.branch.clone()),
            "worker_workspace_reused": managed_worktree.as_ref().map(|row| row.reused),
            "worker_workspace_cleanup_branch": managed_worktree.as_ref().map(|row| row.record.cleanup_branch),
            "session_id": session_id,
            "session_run_id": run_id,
            "session_context_run_id": worker_context_run_id,
            "worker_run_reference": worker_context_run_id,
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
            if let Some(session_context_run_id) = payload.get("session_context_run_id").cloned() {
                extra.insert("session_context_run_id".to_string(), session_context_run_id);
            }
            extra.insert("worker_kind".to_string(), json!(worker_kind));
            if let Some(branch) = payload.get("worker_workspace_branch").cloned() {
                extra.insert("worker_workspace_branch".to_string(), branch);
            }
            extra
        });

        Ok::<_, StatusCode>((artifact, payload, run_result.is_ok()))
    }
    .await;
    if let Some(worktree) = managed_worktree.as_ref() {
        let _ = crate::runtime::worktrees::delete_managed_worktree(state, &worktree.record).await;
    }
    let (artifact, payload, run_ok) = result?;
    if !run_ok {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    Ok((artifact, payload))
}

async fn prepare_coder_worker_workspace(
    state: &AppState,
    workspace_root: &str,
    task_id: Option<&str>,
    owner_run_id: &str,
    worker_kind: &str,
) -> Option<crate::runtime::worktrees::ManagedWorktreeEnsureResult> {
    let repo_root = crate::runtime::worktrees::resolve_git_repo_root(workspace_root)?;
    crate::runtime::worktrees::ensure_managed_worktree(
        state,
        crate::runtime::worktrees::ManagedWorktreeEnsureInput {
            repo_root,
            task_id: task_id.map(ToString::to_string),
            owner_run_id: Some(owner_run_id.to_string()),
            lease_id: None,
            branch_hint: Some(worker_kind.to_string()),
            base: "HEAD".to_string(),
            cleanup_branch: true,
        },
    )
    .await
    .ok()
}

async fn run_issue_fix_prepare_worker(
    state: &AppState,
    record: &CoderRunRecord,
    run: &ContextRunState,
    task_id: Option<&str>,
) -> Result<(ContextBlackboardArtifact, Value), StatusCode> {
    let prompt = build_issue_fix_worker_prompt(
        record,
        run,
        &summarize_workflow_memory_hits(record, run, "retrieve_memory"),
    );
    run_issue_fix_worker_session(
        state,
        record,
        task_id,
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
    task_id: Option<&str>,
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
        task_id,
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
    task_id: Option<&str>,
) -> Result<(ContextBlackboardArtifact, Value), StatusCode> {
    let prompt = build_pr_review_worker_prompt(
        record,
        run,
        &summarize_workflow_memory_hits(record, run, "retrieve_memory"),
    );
    run_issue_fix_worker_session(
        state,
        record,
        task_id,
        prompt,
        "pr_review_analysis",
        "coder_pr_review_worker_session",
        "artifacts/pr_review.worker_session.json",
    )
    .await
}

async fn run_issue_triage_worker(
    state: &AppState,
    record: &CoderRunRecord,
    run: &ContextRunState,
    task_id: Option<&str>,
) -> Result<(ContextBlackboardArtifact, Value), StatusCode> {
    let prompt = build_issue_triage_worker_prompt(
        record,
        run,
        &summarize_workflow_memory_hits(record, run, "retrieve_memory"),
    );
    run_issue_fix_worker_session(
        state,
        record,
        task_id,
        prompt,
        "issue_triage_analysis",
        "coder_issue_triage_worker_session",
        "artifacts/triage.worker_session.json",
    )
    .await
}

async fn run_merge_recommendation_worker(
    state: &AppState,
    record: &CoderRunRecord,
    run: &ContextRunState,
    task_id: Option<&str>,
) -> Result<(ContextBlackboardArtifact, Value), StatusCode> {
    let prompt = build_merge_recommendation_worker_prompt(
        record,
        run,
        &summarize_workflow_memory_hits(record, run, "retrieve_memory"),
    );
    run_issue_fix_worker_session(
        state,
        record,
        task_id,
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
        "origin_policy": record.origin_policy,
        "github_project_ref": record.github_project_ref,
        "remote_sync_state": coder_run_sync_state(record),
        "status": context_run.status,
        "phase": project_coder_phase(context_run),
        "created_at_ms": record.created_at_ms,
        "updated_at_ms": context_run.updated_at_ms,
    })
}

fn same_coder_github_ref(left: Option<&CoderGithubRef>, right: Option<&CoderGithubRef>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.kind == right.kind && left.number == right.number,
        (None, None) => true,
        _ => false,
    }
}

async fn has_completed_follow_on_pr_review(
    state: &AppState,
    record: &CoderRunRecord,
) -> Result<bool, StatusCode> {
    Ok(find_completed_follow_on_pr_review(state, record)
        .await?
        .is_some())
}

async fn find_completed_follow_on_pr_review(
    state: &AppState,
    record: &CoderRunRecord,
) -> Result<Option<CoderRunRecord>, StatusCode> {
    let Some(parent_coder_run_id) = record.parent_coder_run_id.as_deref() else {
        return Ok(None);
    };
    let mut latest_completed: Option<(CoderRunRecord, u64)> = None;
    ensure_coder_runs_dir(state).await?;
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
        let Ok(candidate) = serde_json::from_str::<CoderRunRecord>(&raw) else {
            continue;
        };
        if candidate.coder_run_id == record.coder_run_id
            || candidate.parent_coder_run_id.as_deref() != Some(parent_coder_run_id)
            || candidate.workflow_mode != CoderWorkflowMode::PrReview
            || !same_coder_github_ref(candidate.github_ref.as_ref(), record.github_ref.as_ref())
        {
            continue;
        }
        let Ok(run) = load_context_run_state(state, &candidate.linked_context_run_id).await else {
            continue;
        };
        if matches!(run.status, ContextRunStatus::Completed) {
            let candidate_updated_at = run.updated_at_ms;
            if latest_completed
                .as_ref()
                .is_none_or(|(_, best_updated_at)| candidate_updated_at >= *best_updated_at)
            {
                latest_completed = Some((candidate, candidate_updated_at));
            }
        }
    }
    Ok(latest_completed.map(|(record, _)| record))
}

async fn merge_submit_review_policy_block(
    state: &AppState,
    record: &CoderRunRecord,
) -> Result<Option<Value>, StatusCode> {
    let source = record
        .origin_policy
        .as_ref()
        .and_then(|row| row.get("source"))
        .and_then(Value::as_str);
    if source != Some("issue_fix_pr_submit") {
        return Ok(None);
    }
    let Some(review_record) = find_completed_follow_on_pr_review(state, record).await? else {
        return Ok(Some(json!({
            "reason": "requires_approved_pr_review_follow_on",
            "required_workflow_mode": "pr_review",
            "parent_coder_run_id": record.parent_coder_run_id,
            "review_completed": false,
        })));
    };
    let Some(review_summary) =
        load_latest_coder_artifact_payload(state, &review_record, "coder_pr_review_summary").await
    else {
        return Ok(Some(json!({
            "reason": "requires_approved_pr_review_follow_on",
            "required_workflow_mode": "pr_review",
            "parent_coder_run_id": record.parent_coder_run_id,
            "review_completed": true,
            "review_summary_present": false,
        })));
    };
    let verdict = review_summary
        .get("verdict")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let has_blockers = review_summary
        .get("blockers")
        .and_then(Value::as_array)
        .is_some_and(|rows| !rows.is_empty());
    let has_requested_changes = review_summary
        .get("requested_changes")
        .and_then(Value::as_array)
        .is_some_and(|rows| !rows.is_empty());
    if verdict == "approve" && !has_blockers && !has_requested_changes {
        return Ok(None);
    }
    Ok(Some(json!({
        "reason": "requires_approved_pr_review_follow_on",
        "required_workflow_mode": "pr_review",
        "parent_coder_run_id": record.parent_coder_run_id,
        "review_completed": true,
        "review_summary_present": true,
        "review_verdict": review_summary.get("verdict").cloned().unwrap_or(Value::Null),
        "has_blockers": has_blockers,
        "has_requested_changes": has_requested_changes,
    })))
}

fn merge_submit_auto_mode_policy_block(record: &CoderRunRecord) -> Option<Value> {
    let origin_policy = record.origin_policy.as_ref();
    let merge_auto_spawn_opted_in = origin_policy
        .and_then(|row| row.get("merge_auto_spawn_opted_in"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !merge_auto_spawn_opted_in {
        return Some(json!({
            "reason": "requires_explicit_auto_merge_submit_opt_in",
            "submit_mode": "auto",
            "merge_auto_spawn_opted_in": false,
        }));
    }
    let spawn_mode = origin_policy
        .and_then(|row| row.get("spawn_mode"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    if spawn_mode != "auto" {
        return Some(json!({
            "reason": "requires_auto_spawned_merge_follow_on",
            "submit_mode": "auto",
            "merge_auto_spawn_opted_in": true,
            "spawn_mode": spawn_mode,
        }));
    }
    None
}

fn merge_submit_request_readiness_block(merge_request_payload: &Value) -> Option<Value> {
    let recommendation = merge_request_payload
        .get("recommendation")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let has_blockers = merge_request_payload
        .get("blockers")
        .and_then(Value::as_array)
        .is_some_and(|rows| !rows.is_empty());
    let has_required_checks = merge_request_payload
        .get("required_checks")
        .and_then(Value::as_array)
        .is_some_and(|rows| !rows.is_empty());
    let has_required_approvals = merge_request_payload
        .get("required_approvals")
        .and_then(Value::as_array)
        .is_some_and(|rows| !rows.is_empty());
    if recommendation == "merge" && !has_blockers && !has_required_checks && !has_required_approvals
    {
        return None;
    }
    Some(json!({
        "reason": "merge_execution_request_not_merge_ready",
        "recommendation": merge_request_payload.get("recommendation").cloned().unwrap_or(Value::Null),
        "has_blockers": has_blockers,
        "has_required_checks": has_required_checks,
        "has_required_approvals": has_required_approvals,
    }))
}

fn blocked_merge_submit_policy(mode: &str, policy: Value) -> Value {
    json!({
        "blocked": true,
        "code": "CODER_MERGE_SUBMIT_POLICY_BLOCKED",
        "submit_mode": mode,
        "policy": policy,
    })
}

fn allowed_merge_submit_policy(mode: &str) -> Value {
    json!({
        "blocked": false,
        "submit_mode": mode,
        "eligible": true,
    })
}

fn merge_submit_policy_envelope(
    manual: Value,
    auto: Value,
    preferred_submit_mode: &str,
    auto_execute_eligible: bool,
    auto_execute_policy_enabled: bool,
    auto_execute_block_reason: &str,
) -> Value {
    json!({
        "manual": manual,
        "auto": auto,
        "preferred_submit_mode": preferred_submit_mode,
        "explicit_submit_required": true,
        "auto_execute_after_approval": false,
        "auto_execute_eligible": auto_execute_eligible,
        "auto_execute_policy_enabled": auto_execute_policy_enabled,
        "auto_execute_block_reason": auto_execute_block_reason,
    })
}

fn blocked_policy_reason(policy: &Value) -> Option<&str> {
    policy.get("reason").and_then(Value::as_str).or_else(|| {
        policy
            .get("policy")
            .and_then(|row| row.get("reason"))
            .and_then(Value::as_str)
    })
}

async fn coder_merge_submit_policy_summary(
    state: &AppState,
    record: &CoderRunRecord,
) -> Result<Value, StatusCode> {
    if record.workflow_mode != CoderWorkflowMode::MergeRecommendation {
        return Ok(Value::Null);
    }
    let project_policy = load_coder_project_policy(state, &record.repo_binding.project_id).await?;
    let Some(merge_request_payload) =
        load_latest_coder_artifact_payload(state, record, "coder_merge_execution_request").await
    else {
        return Ok(merge_submit_policy_envelope(
            blocked_merge_submit_policy(
                "manual",
                json!({
                    "reason": "requires_merge_execution_request",
                }),
            ),
            blocked_merge_submit_policy(
                "auto",
                json!({
                    "reason": "requires_merge_execution_request",
                    "merge_auto_spawn_opted_in": record
                        .origin_policy
                        .as_ref()
                        .and_then(|row| row.get("merge_auto_spawn_opted_in"))
                        .cloned()
                        .unwrap_or_else(|| json!(false)),
                }),
            ),
            "manual",
            false,
            project_policy.auto_merge_enabled,
            "requires_merge_execution_request",
        ));
    };
    if let Some(policy) = merge_submit_request_readiness_block(&merge_request_payload) {
        let block_reason = blocked_policy_reason(&policy)
            .unwrap_or("merge_submit_blocked")
            .to_string();
        return Ok(merge_submit_policy_envelope(
            blocked_merge_submit_policy("manual", policy.clone()),
            blocked_merge_submit_policy("auto", policy),
            "manual",
            false,
            project_policy.auto_merge_enabled,
            &block_reason,
        ));
    }
    if let Some(policy) = merge_submit_review_policy_block(state, record).await? {
        let auto_policy =
            merge_submit_auto_mode_policy_block(record).unwrap_or_else(|| policy.clone());
        let block_reason = blocked_policy_reason(&policy)
            .unwrap_or("merge_submit_blocked")
            .to_string();
        return Ok(merge_submit_policy_envelope(
            blocked_merge_submit_policy("manual", policy),
            blocked_merge_submit_policy("auto", auto_policy),
            "manual",
            false,
            project_policy.auto_merge_enabled,
            &block_reason,
        ));
    }
    let auto = if let Some(policy) = merge_submit_auto_mode_policy_block(record) {
        blocked_merge_submit_policy("auto", policy)
    } else {
        allowed_merge_submit_policy("auto")
    };
    let preferred_submit_mode = if auto
        .get("blocked")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        "manual"
    } else {
        "auto"
    };
    let auto_execute_eligible =
        project_policy.auto_merge_enabled && preferred_submit_mode == "auto";
    let auto_execute_block_reason = if !project_policy.auto_merge_enabled {
        "project_auto_merge_policy_disabled".to_string()
    } else if preferred_submit_mode == "manual" {
        blocked_policy_reason(&auto)
            .unwrap_or("preferred_submit_mode_manual")
            .to_string()
    } else {
        "explicit_submit_required_policy".to_string()
    };
    Ok(merge_submit_policy_envelope(
        allowed_merge_submit_policy("manual"),
        auto,
        preferred_submit_mode,
        auto_execute_eligible,
        project_policy.auto_merge_enabled,
        &auto_execute_block_reason,
    ))
}

async fn coder_execution_policy_block(
    state: &AppState,
    record: &CoderRunRecord,
) -> Result<Option<Value>, StatusCode> {
    if record.workflow_mode != CoderWorkflowMode::MergeRecommendation {
        return Ok(None);
    }
    let source = record
        .origin_policy
        .as_ref()
        .and_then(|row| row.get("source"))
        .and_then(Value::as_str);
    if source != Some("issue_fix_pr_submit") {
        return Ok(None);
    }
    if has_completed_follow_on_pr_review(state, record).await? {
        return Ok(None);
    }
    Ok(Some(json!({
        "ok": false,
        "error": "merge recommendation is blocked until a sibling pr_review run completes",
        "code": "CODER_EXECUTION_POLICY_BLOCKED",
        "policy": {
            "reason": "requires_completed_pr_review_follow_on",
            "required_workflow_mode": "pr_review",
            "parent_coder_run_id": record.parent_coder_run_id,
        }
    })))
}

async fn coder_execution_policy_summary(
    state: &AppState,
    record: &CoderRunRecord,
) -> Result<Value, StatusCode> {
    if let Some(blocked) = coder_execution_policy_block(state, record).await? {
        let policy = blocked.get("policy").cloned().unwrap_or_else(|| json!({}));
        return Ok(json!({
            "blocked": true,
            "code": blocked.get("code").cloned().unwrap_or_else(|| json!("CODER_EXECUTION_POLICY_BLOCKED")),
            "error": blocked.get("error").cloned().unwrap_or_else(|| json!("coder execution blocked by policy")),
            "policy": policy,
        }));
    }
    Ok(json!({
        "blocked": false,
    }))
}

async fn emit_coder_execution_policy_block(
    state: &AppState,
    record: &CoderRunRecord,
    blocked: &Value,
) -> Result<(), StatusCode> {
    publish_coder_run_event(
        state,
        "coder.run.phase_changed",
        record,
        Some("policy_blocked"),
        {
            let mut extra = serde_json::Map::new();
            extra.insert("event_type".to_string(), json!("execution_policy_blocked"));
            extra.insert(
                "code".to_string(),
                blocked
                    .get("code")
                    .cloned()
                    .unwrap_or_else(|| json!("CODER_EXECUTION_POLICY_BLOCKED")),
            );
            extra.insert(
                "policy".to_string(),
                blocked.get("policy").cloned().unwrap_or_else(|| json!({})),
            );
            extra
        },
    );
    Ok(())
}

fn follow_on_execution_policy_preview(
    workflow_mode: &CoderWorkflowMode,
    required_completed_workflow_modes: &[Value],
) -> Value {
    if matches!(workflow_mode, CoderWorkflowMode::MergeRecommendation)
        && !required_completed_workflow_modes.is_empty()
    {
        return json!({
            "blocked": true,
            "code": "CODER_EXECUTION_POLICY_BLOCKED",
            "error": "merge recommendation is blocked until required review follow-ons complete",
            "policy": {
                "reason": "requires_completed_pr_review_follow_on",
                "required_completed_workflow_modes": required_completed_workflow_modes,
            }
        });
    }
    json!({
        "blocked": false,
    })
}

async fn coder_run_create_inner(
    state: AppState,
    input: CoderRunCreateInput,
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
    let created = super::context_runs::context_run_create_impl(
        state.clone(),
        tandem_types::TenantContext::local_implicit(),
        create_input,
    )
    .await?;
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
        origin_policy: input.origin_policy,
        github_project_ref: None,
        remote_sync_state: None,
        created_at_ms: now,
        updated_at_ms: now,
    };
    save_coder_run_record(&state, &record).await?;

    let follow_on_duplicate_linkage =
        maybe_write_follow_on_duplicate_linkage_candidate(&state, &record).await?;

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
            let memory_hits = collect_coder_memory_hits(&state, &record, &memory_query, 8).await?;
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
            let memory_hits = collect_coder_memory_hits(&state, &record, &memory_query, 8).await?;
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
            let memory_hits = collect_coder_memory_hits(&state, &record, &memory_query, 8).await?;
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
            let memory_hits = collect_coder_memory_hits(&state, &record, &memory_query, 8).await?;
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
    maybe_sync_github_project_status(&state, &mut record, &final_run).await?;
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
        "generated_candidates": follow_on_duplicate_linkage
            .map(|candidate| vec![candidate])
            .unwrap_or_default(),
        "execution_policy": coder_execution_policy_summary(&state, &record).await?,
        "merge_submit_policy": coder_merge_submit_policy_summary(&state, &record).await?,
        "run": final_run,
    }))
    .into_response())
}

pub(super) async fn coder_run_create(
    State(state): State<AppState>,
    Json(input): Json<CoderRunCreateInput>,
) -> Result<Response, StatusCode> {
    coder_run_create_inner(state, input).await
}

pub(super) async fn coder_project_run_create(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(input): Json<CoderProjectRunCreateInput>,
) -> Result<Response, StatusCode> {
    let project_id = project_id.trim();
    if project_id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let Some(binding) = load_coder_project_binding(&state, project_id).await? else {
        return Ok((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Coder project binding is required before creating a project-scoped run",
                "code": "CODER_PROJECT_BINDING_REQUIRED",
                "project_id": project_id,
            })),
        )
            .into_response());
    };
    coder_run_create_inner(
        state,
        CoderRunCreateInput {
            coder_run_id: input.coder_run_id,
            workflow_mode: input.workflow_mode,
            repo_binding: binding.repo_binding,
            github_ref: input.github_ref,
            objective: input.objective,
            source_client: input.source_client,
            workspace: input.workspace,
            model_provider: input.model_provider,
            model_id: input.model_id,
            mcp_servers: input.mcp_servers,
            parent_coder_run_id: input.parent_coder_run_id,
            origin: input.origin,
            origin_artifact_type: input.origin_artifact_type,
            origin_policy: input.origin_policy,
        },
    )
    .await
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
        let mut row = coder_run_payload(&record, &run);
        if let Some(obj) = row.as_object_mut() {
            obj.insert(
                "execution_policy".to_string(),
                coder_execution_policy_summary(&state, &record).await?,
            );
        }
        rows.push(row);
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
        collect_coder_memory_hits(&state, &record, &memory_query, 8).await?
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
    let serialized_artifacts = serialize_coder_artifacts(&blackboard.artifacts).await;
    Ok(Json(json!({
        "coder_run": coder_run_payload(&record, &run),
        "execution_policy": coder_execution_policy_summary(&state, &record).await?,
        "merge_submit_policy": coder_merge_submit_policy_summary(&state, &record).await?,
        "run": run,
        "artifacts": blackboard.artifacts,
        "coder_artifacts": serialized_artifacts,
        "memory_hits": {
            "query": memory_query,
            "retrieval_policy": coder_memory_retrieval_policy(&record, &memory_query, 8),
            "hits": memory_hits,
        },
        "memory_candidates": memory_candidates,
    })))
}

pub(super) async fn coder_project_policy_get(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    if project_id.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let policy = load_coder_project_policy(&state, project_id.trim()).await?;
    Ok(Json(json!({
        "project_policy": policy,
    })))
}

pub(super) async fn coder_project_get(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let project_id = project_id.trim();
    if project_id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    ensure_coder_runs_dir(&state).await?;
    let project_policy = load_coder_project_policy(&state, project_id).await?;
    let explicit_binding = load_coder_project_binding(&state, project_id).await?;
    let mut run_records = Vec::<CoderRunRecord>::new();
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
        if record.repo_binding.project_id == project_id {
            run_records.push(record);
        }
    }
    run_records.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    let summary_repo_binding = explicit_binding
        .as_ref()
        .map(|row| row.repo_binding.clone())
        .or_else(|| run_records.first().map(|row| row.repo_binding.clone()));
    let Some(repo_binding) = summary_repo_binding else {
        return Ok(Json(json!({
            "project": null,
            "binding": explicit_binding,
            "project_policy": project_policy,
            "recent_runs": [],
        })));
    };
    let mut workflow_modes = run_records
        .iter()
        .map(|row| row.workflow_mode.clone())
        .collect::<Vec<_>>();
    workflow_modes.sort_by_key(|mode| match mode {
        CoderWorkflowMode::IssueFix => 0,
        CoderWorkflowMode::IssueTriage => 1,
        CoderWorkflowMode::MergeRecommendation => 2,
        CoderWorkflowMode::PrReview => 3,
    });
    workflow_modes.dedup();
    let summary = CoderProjectSummary {
        project_id: project_id.to_string(),
        repo_binding,
        latest_coder_run_id: run_records.first().map(|row| row.coder_run_id.clone()),
        latest_updated_at_ms: run_records
            .first()
            .map(|row| row.updated_at_ms)
            .unwrap_or(0),
        run_count: run_records.len() as u64,
        workflow_modes,
        project_policy: project_policy.clone(),
    };
    let mut recent_runs = Vec::new();
    for record in run_records.iter().take(10) {
        let run = load_context_run_state(&state, &record.linked_context_run_id).await?;
        recent_runs.push(json!({
            "coder_run": coder_run_payload(record, &run),
            "execution_policy": coder_execution_policy_summary(&state, record).await?,
            "merge_submit_policy": coder_merge_submit_policy_summary(&state, record).await?,
        }));
    }
    Ok(Json(json!({
        "project": summary,
        "binding": explicit_binding,
        "project_policy": project_policy,
        "recent_runs": recent_runs,
    })))
}

pub(super) async fn coder_project_list(
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    ensure_coder_runs_dir(&state).await?;
    let mut projects = std::collections::BTreeMap::<String, CoderProjectSummary>::new();
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
        let project_id = record.repo_binding.project_id.clone();
        let project_policy = load_coder_project_policy(&state, &project_id).await?;
        let explicit_binding = load_coder_project_binding(&state, &project_id).await?;
        let entry = projects
            .entry(project_id.clone())
            .or_insert_with(|| CoderProjectSummary {
                project_id: project_id.clone(),
                repo_binding: explicit_binding
                    .as_ref()
                    .map(|row| row.repo_binding.clone())
                    .unwrap_or_else(|| record.repo_binding.clone()),
                latest_coder_run_id: Some(record.coder_run_id.clone()),
                latest_updated_at_ms: record.updated_at_ms,
                run_count: 0,
                workflow_modes: Vec::new(),
                project_policy,
            });
        entry.run_count += 1;
        if !entry.workflow_modes.contains(&record.workflow_mode) {
            entry.workflow_modes.push(record.workflow_mode.clone());
        }
        if record.updated_at_ms >= entry.latest_updated_at_ms {
            entry.latest_updated_at_ms = record.updated_at_ms;
            entry.latest_coder_run_id = Some(record.coder_run_id.clone());
            entry.repo_binding = explicit_binding
                .as_ref()
                .map(|row| row.repo_binding.clone())
                .unwrap_or_else(|| record.repo_binding.clone());
        }
    }
    let mut rows = projects.into_values().collect::<Vec<_>>();
    for row in &mut rows {
        row.workflow_modes.sort_by_key(|mode| match mode {
            CoderWorkflowMode::IssueFix => 0,
            CoderWorkflowMode::IssueTriage => 1,
            CoderWorkflowMode::MergeRecommendation => 2,
            CoderWorkflowMode::PrReview => 3,
        });
    }
    rows.sort_by(|a, b| b.latest_updated_at_ms.cmp(&a.latest_updated_at_ms));
    Ok(Json(json!({
        "projects": rows,
    })))
}

pub(super) async fn coder_project_binding_get(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    if project_id.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(Json(json!({
        "binding": load_coder_project_binding(&state, project_id.trim()).await?,
    })))
}

pub(super) async fn coder_project_run_list(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Query(query): Query<CoderProjectRunListQuery>,
) -> Result<Json<Value>, StatusCode> {
    let project_id = project_id.trim();
    if project_id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    ensure_coder_runs_dir(&state).await?;
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let mut rows = Vec::<Value>::new();
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
        if record.repo_binding.project_id != project_id {
            continue;
        }
        let Ok(run) = load_context_run_state(&state, &record.linked_context_run_id).await else {
            continue;
        };
        rows.push(json!({
            "coder_run": coder_run_payload(&record, &run),
            "execution_policy": coder_execution_policy_summary(&state, &record).await?,
            "merge_submit_policy": coder_merge_submit_policy_summary(&state, &record).await?,
            "run": run,
        }));
    }
    rows.sort_by(|a, b| {
        b.get("coder_run")
            .and_then(|row| row.get("updated_at_ms"))
            .and_then(Value::as_u64)
            .cmp(
                &a.get("coder_run")
                    .and_then(|row| row.get("updated_at_ms"))
                    .and_then(Value::as_u64),
            )
    });
    rows.truncate(limit);
    Ok(Json(json!({
        "project_id": project_id,
        "runs": rows,
    })))
}

pub(super) async fn coder_project_binding_put(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(input): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    let project_id = project_id.trim().to_string();
    if project_id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let parsed = parse_coder_project_binding_put_input(&project_id, input)?;
    let existing = load_coder_project_binding(&state, &project_id).await?;
    let mut repo_binding = parsed
        .repo_binding
        .or_else(|| existing.as_ref().map(|row| row.repo_binding.clone()))
        .ok_or(StatusCode::BAD_REQUEST)?;
    if repo_binding.workspace_id.trim().is_empty()
        || repo_binding.workspace_root.trim().is_empty()
        || repo_binding.repo_slug.trim().is_empty()
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    repo_binding.project_id = project_id.to_string();
    let github_project_binding = match parsed.github_project_binding {
        Some(request) => Some(
            GithubProjectsAdapter::new(&state)
                .discover_binding(&request)
                .await?,
        ),
        None => existing.and_then(|row| row.github_project_binding),
    };
    let binding = CoderProjectBinding {
        project_id: project_id.to_string(),
        repo_binding,
        github_project_binding,
        updated_at_ms: crate::now_ms(),
    };
    save_coder_project_binding(&state, &binding).await?;
    Ok(Json(json!({
        "ok": true,
        "binding": binding,
    })))
}

pub(super) async fn coder_project_github_project_inbox(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let project_id = project_id.trim();
    if project_id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let binding = load_coder_project_binding(&state, project_id)
        .await?
        .ok_or(StatusCode::NOT_FOUND)?;
    let github_project_binding = binding
        .github_project_binding
        .clone()
        .ok_or(StatusCode::CONFLICT)?;
    let adapter = GithubProjectsAdapter::new(&state);
    let live_binding = adapter
        .discover_binding(&CoderGithubProjectBindingRequest {
            owner: github_project_binding.owner.clone(),
            project_number: github_project_binding.project_number,
            repo_slug: github_project_binding.repo_slug.clone(),
            mcp_server: github_project_binding.mcp_server.clone(),
        })
        .await?;
    let schema_drift = live_binding.schema_fingerprint != github_project_binding.schema_fingerprint;
    let items = adapter.list_inbox_items(&github_project_binding).await?;
    let mut rows = Vec::new();
    for item in items {
        let linked = find_latest_project_item_run(&state, &item.project_item_id).await?;
        let actionable = item.issue.is_some()
            && status_alias_matches(
                &item.status_name,
                &[&github_project_binding.status_mapping.todo.name],
            );
        let remote_sync_state = if schema_drift {
            CoderRemoteSyncState::SchemaDrift
        } else if let Some((record, run)) = linked.as_ref() {
            let expected = context_status_to_project_option(
                &record
                    .github_project_ref
                    .as_ref()
                    .map(|row| row.status_mapping.clone())
                    .unwrap_or_else(|| github_project_binding.status_mapping.clone()),
                &run.status,
            );
            if item.status_option_id.as_deref() == Some(expected.id.as_str()) {
                coder_run_sync_state(record)
            } else {
                CoderRemoteSyncState::RemoteStateDiverged
            }
        } else {
            CoderRemoteSyncState::InSync
        };
        rows.push(json!({
            "project_item_id": item.project_item_id,
            "title": item.title,
            "status_name": item.status_name,
            "status_option_id": item.status_option_id,
            "issue": item.issue,
            "actionable": actionable,
            "unsupported_reason": if item.issue.is_none() { Some("unsupported_item_type") } else { None::<&str> },
            "linked_run": linked.as_ref().map(|(record, run)| json!({
                "coder_run": coder_run_payload(record, run),
                "active": !is_terminal_context_status(&run.status),
            })),
            "remote_sync_state": remote_sync_state,
        }));
    }
    Ok(Json(json!({
        "project_id": project_id,
        "binding": github_project_binding,
        "schema_drift": schema_drift,
        "live_schema_fingerprint": live_binding.schema_fingerprint,
        "items": rows,
    })))
}

pub(super) async fn coder_project_github_project_intake(
    State(state): State<AppState>,
    Path(project_id): Path<String>,
    Json(input): Json<CoderGithubProjectIntakeInput>,
) -> Result<Response, StatusCode> {
    let project_id = project_id.trim();
    if project_id.is_empty() || input.project_item_id.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let _guard = coder_project_intake_lock().lock().await;
    let Some(binding) = load_coder_project_binding(&state, project_id).await? else {
        return Err(StatusCode::NOT_FOUND);
    };
    let Some(github_project_binding) = binding.github_project_binding.clone() else {
        return Ok((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "GitHub Project binding is required before intake",
                "code": "CODER_GITHUB_PROJECT_BINDING_REQUIRED",
            })),
        )
            .into_response());
    };
    if let Some((record, run)) =
        find_latest_project_item_run(&state, &input.project_item_id).await?
    {
        if !is_terminal_context_status(&run.status) {
            return Ok(Json(json!({
                "ok": true,
                "deduped": true,
                "coder_run": coder_run_payload(&record, &run),
                "run": run,
            }))
            .into_response());
        }
    }
    let adapter = GithubProjectsAdapter::new(&state);
    let items = adapter.list_inbox_items(&github_project_binding).await?;
    let item = items
        .into_iter()
        .find(|row| row.project_item_id == input.project_item_id)
        .ok_or(StatusCode::NOT_FOUND)?;
    let issue = item.issue.ok_or(StatusCode::CONFLICT)?;
    if !status_alias_matches(
        &item.status_name,
        &[&github_project_binding.status_mapping.todo.name],
    ) {
        return Ok((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Project item is not in the configured TODO state",
                "code": "CODER_GITHUB_PROJECT_ITEM_NOT_TODO",
                "status_name": item.status_name,
            })),
        )
            .into_response());
    }
    let response = coder_run_create_inner(
        state.clone(),
        CoderRunCreateInput {
            coder_run_id: input.coder_run_id,
            workflow_mode: CoderWorkflowMode::IssueTriage,
            repo_binding: binding.repo_binding.clone(),
            github_ref: Some(CoderGithubRef {
                kind: CoderGithubRefKind::Issue,
                number: issue.number,
                url: issue.html_url.clone(),
            }),
            objective: None,
            source_client: input.source_client,
            workspace: input.workspace,
            model_provider: input.model_provider,
            model_id: input.model_id,
            mcp_servers: input.mcp_servers.or_else(|| {
                github_project_binding
                    .mcp_server
                    .clone()
                    .map(|row| vec![row])
            }),
            parent_coder_run_id: None,
            origin: Some("github_project_intake".to_string()),
            origin_artifact_type: Some("github_project_item".to_string()),
            origin_policy: Some(json!({
                "source": "github_project_intake",
                "project_item_id": item.project_item_id,
            })),
        },
    )
    .await?;
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut payload: Value =
        serde_json::from_slice(&body).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let coder_run_id = payload
        .get("coder_run")
        .and_then(|row| row.get("coder_run_id"))
        .and_then(Value::as_str)
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut record = load_coder_run_record(&state, coder_run_id).await?;
    record.github_project_ref = Some(CoderGithubProjectRef {
        owner: github_project_binding.owner.clone(),
        project_number: github_project_binding.project_number,
        project_item_id: item.project_item_id.clone(),
        issue_number: issue.number,
        issue_url: issue.html_url.clone(),
        schema_fingerprint: github_project_binding.schema_fingerprint.clone(),
        status_mapping: github_project_binding.status_mapping.clone(),
    });
    record.remote_sync_state = Some(CoderRemoteSyncState::InSync);
    save_coder_run_record(&state, &record).await?;
    let run = load_context_run_state(&state, &record.linked_context_run_id).await?;
    maybe_sync_github_project_status(&state, &mut record, &run).await?;
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("coder_run".to_string(), coder_run_payload(&record, &run));
        obj.insert("run".to_string(), json!(run));
        obj.insert("deduped".to_string(), json!(false));
    }
    Ok(Json(payload).into_response())
}
