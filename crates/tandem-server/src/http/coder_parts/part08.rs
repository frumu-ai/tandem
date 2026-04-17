
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
        ContextRunStatus::Completed,
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
    let validation_artifact = write_workflow_validation_artifact(
        &state,
        &record,
        "merge-readiness-validation",
        "artifacts/merge_recommendation.validation.json",
        input.summary.as_deref(),
        &input.validation_steps,
        &input.validation_results,
        &input.memory_hits_used,
        input.notes.as_deref(),
        Some(&artifact.path),
        json!({
            "recommendation": input.recommendation.clone(),
            "risk_level": input.risk_level.clone(),
            "blockers": input.blockers.clone(),
            "required_checks": input.required_checks.clone(),
            "required_approvals": input.required_approvals.clone(),
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
    let approval_required = input
        .recommendation
        .as_deref()
        .is_some_and(|row| row.eq_ignore_ascii_case("merge"))
        && input.blockers.is_empty()
        && input.required_checks.is_empty()
        && input.required_approvals.is_empty();
    let completion_reason = if approval_required {
        "Merge recommendation recorded and awaiting operator approval."
    } else {
        "Merge recommendation summary recorded."
    };
    let final_status = if approval_required {
        ContextRunStatus::AwaitingApproval
    } else {
        ContextRunStatus::Completed
    };
    let final_run = finalize_coder_workflow_run(
        &state,
        &record,
        &[
            "inspect_pull_request",
            "retrieve_memory",
            "assess_merge_readiness",
            "write_merge_artifact",
        ],
        final_status,
        completion_reason,
    )
    .await?;
    let merge_submit_policy = if approval_required {
        coder_merge_submit_policy_summary(&state, &record).await?
    } else {
        Value::Null
    };
    if approval_required {
        publish_coder_run_event(
            &state,
            "coder.approval.required",
            &record,
            Some("approval"),
            {
                let mut extra = serde_json::Map::new();
                extra.insert(
                    "event_type".to_string(),
                    json!("merge_recommendation_ready"),
                );
                extra.insert("artifact_id".to_string(), json!(artifact.id));
                if let Some(recommendation) = input.recommendation.clone() {
                    extra.insert("recommendation".to_string(), json!(recommendation));
                }
                if !matches!(merge_submit_policy, Value::Null) {
                    extra.insert(
                        "merge_submit_policy".to_string(),
                        merge_submit_policy.clone(),
                    );
                }
                extra
            },
        );
    }
    record.updated_at_ms = final_run.updated_at_ms;
    save_coder_run_record(&state, &record).await?;
    let worker_payload = load_latest_coder_artifact_payload(
        &state,
        &record,
        "coder_merge_recommendation_worker_session",
    )
    .await;
    Ok(Json(attach_worker_reference_fields(
        json!({
            "ok": true,
            "artifact": artifact,
            "readiness_artifact": readiness_artifact,
            "validation_artifact": validation_artifact,
            "generated_candidates": generated_candidates,
            "approval_required": approval_required,
            "coder_run": coder_run_payload(&record, &final_run),
            "merge_submit_policy": merge_submit_policy,
            "run": final_run,
        }),
        worker_payload.as_ref(),
        None,
    )))
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
    let worker_payload = load_latest_coder_artifact_payload(
        &state,
        &record,
        "coder_merge_recommendation_worker_session",
    )
    .await;
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
