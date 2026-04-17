
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
            let worker_result =
                run_issue_fix_prepare_worker(&state, record, &run, Some(task.id.as_str())).await;
            let (worker_artifact, worker_payload) = match worker_result {
                Ok(result) => result,
                Err(error) => {
                    let detail = format!(
                        "Issue-fix worker session failed during prepare_fix with status {}.",
                        error
                    );
                    let generated_candidate = write_worker_failure_run_outcome_candidate(
                        &state,
                        record,
                        "prepare_fix",
                        "coder_issue_fix_worker_session",
                        "issue_fix_prepare_failed",
                        &detail,
                    )
                    .await?;
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
                        "generated_candidates": generated_candidate
                            .map(|candidate| vec![candidate])
                            .unwrap_or_default(),
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
                "worker_session": normalize_session_run_payload(&worker_payload),
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
            let validation_worker = run_issue_fix_validation_worker(
                &state,
                record,
                &run,
                fix_plan.as_ref(),
                Some(task.id.as_str()),
            )
            .await;
            let (validation_worker_artifact, validation_worker_payload) = match validation_worker {
                Ok(result) => result,
                Err(error) => {
                    let detail = format!(
                        "Issue-fix validation worker session failed during validate_fix with status {}.",
                        error
                    );
                    let generated_candidate = write_worker_failure_run_outcome_candidate(
                        &state,
                        record,
                        "validate_fix",
                        "coder_issue_fix_validation_session",
                        "issue_fix_validation_failed",
                        &detail,
                    )
                    .await?;
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
                        "generated_candidates": generated_candidate
                            .map(|candidate| vec![candidate])
                            .unwrap_or_default(),
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
                        "worker_run_reference": worker_session
                            .as_ref()
                            .map(preferred_session_run_reference)
                            .unwrap_or(Value::Null),
                        "worker_session_id": worker_session.as_ref().and_then(|payload| payload.get("session_id")).cloned(),
                        "worker_session_run_id": worker_session.as_ref().and_then(|payload| payload.get("session_run_id")).cloned(),
                        "worker_session_context_run_id": worker_session.as_ref().and_then(|payload| payload.get("session_context_run_id")).cloned(),
                        "validation_run_reference": preferred_session_run_reference(&validation_worker_payload),
                        "validation_session_id": validation_worker_payload.get("session_id").cloned(),
                        "validation_session_run_id": validation_worker_payload.get("session_run_id").cloned(),
                        "validation_session_context_run_id": validation_worker_payload.get("session_context_run_id").cloned(),
                        "worker_assistant_excerpt": worker_summary,
                    })],
                    memory_hits_used,
                    notes: Some(format!(
                        "Auto-generated by coder engine worker dispatch. Worker run: {}. Validation run: {}. Plan artifact available: {}",
                        worker_session
                            .as_ref()
                            .map(preferred_session_run_reference)
                            .as_ref()
                            .and_then(Value::as_str)
                            .unwrap_or("unknown"),
                        preferred_session_run_reference(&validation_worker_payload)
                            .as_str()
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
                        "validation_run_reference": validation_session
                            .as_ref()
                            .map(preferred_session_run_reference)
                            .unwrap_or(Value::Null),
                        "validation_session_id": validation_session.as_ref().and_then(|payload| payload.get("session_id")).cloned(),
                        "validation_session_run_id": validation_session.as_ref().and_then(|payload| payload.get("session_run_id")).cloned(),
                        "validation_session_context_run_id": validation_session
                            .as_ref()
                            .and_then(|payload| payload.get("session_context_run_id"))
                            .cloned(),
                    })],
                    memory_hits_used,
                    notes: Some(format!(
                        "Auto-generated by coder engine worker dispatch. Plan artifact available: {}. Validation run: {}",
                        fix_plan.is_some(),
                        validation_session
                            .as_ref()
                            .map(preferred_session_run_reference)
                            .as_ref()
                            .and_then(Value::as_str)
                            .unwrap_or("unavailable")
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
            let (worker_artifact, worker_payload) = match run_pr_review_worker(
                &state,
                record,
                &run,
                Some(task.id.as_str()),
            )
            .await
            {
                Ok(result) => result,
                Err(error) => {
                    let detail = format!(
                        "PR-review worker session failed during review_pull_request with status {}.",
                        error
                    );
                    let generated_candidate = write_worker_failure_run_outcome_candidate(
                        &state,
                        record,
                        "review_pull_request",
                        "coder_pr_review_worker_session",
                        "pr_review_failed",
                        &detail,
                    )
                    .await?;
                    fail_claimed_coder_task(
                        &state,
                        record.linked_context_run_id.clone(),
                        task,
                        "coder_pr_review_worker",
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
                        "generated_candidates": generated_candidate
                            .map(|candidate| vec![candidate])
                            .unwrap_or_default(),
                        "run": failed.get("run").cloned().unwrap_or(Value::Null),
                        "coder_run": failed.get("coder_run").cloned().unwrap_or(Value::Null),
                    }));
                }
            };
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
                        "Auto-generated by coder engine worker dispatch. Worker run: {}. Worker artifact: {}.",
                        preferred_session_run_reference(&worker_payload)
                            .as_str()
                            .unwrap_or("unknown"),
                        worker_artifact.path
                    )),
                }),
            )
            .await?;
            Ok(attach_worker_dispatch_reference(
                response.0,
                Some(&worker_payload),
            ))
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
                    validation_steps: parsed_review
                        .as_ref()
                        .and_then(|payload| payload.get("validation_steps"))
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                    validation_results: parsed_review
                        .as_ref()
                        .and_then(|payload| payload.get("validation_results"))
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default(),
                    memory_hits_used,
                    notes: Some(format!(
                        "Auto-generated by coder engine worker dispatch. Review worker run: {}",
                        worker_payload
                            .as_ref()
                            .map(preferred_session_run_reference)
                            .as_ref()
                            .and_then(Value::as_str)
                            .unwrap_or("unavailable")
                    )),
                }),
            )
            .await?;
            Ok(attach_worker_dispatch_reference(
                response.0,
                worker_payload.as_ref(),
            ))
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
            let (worker_artifact, worker_payload) = match run_merge_recommendation_worker(
                &state,
                record,
                &run,
                Some(task.id.as_str()),
            )
            .await
            {
                Ok(result) => result,
                Err(error) => {
                    let detail = format!(
                        "Merge-recommendation worker session failed during assess_merge_readiness with status {}.",
                        error
                    );
                    let generated_candidate = write_worker_failure_run_outcome_candidate(
                        &state,
                        record,
                        "assess_merge_readiness",
                        "coder_merge_recommendation_worker_session",
                        "merge_recommendation_failed",
                        &detail,
                    )
                    .await?;
                    fail_claimed_coder_task(
                        &state,
                        record.linked_context_run_id.clone(),
                        task,
                        "coder_merge_recommendation_worker",
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
                        "generated_candidates": generated_candidate
                            .map(|candidate| vec![candidate])
                            .unwrap_or_default(),
                        "run": failed.get("run").cloned().unwrap_or(Value::Null),
                        "coder_run": failed.get("coder_run").cloned().unwrap_or(Value::Null),
                    }));
                }
            };
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
                        "Auto-generated by coder engine worker dispatch. Worker run: {}. Worker artifact: {}.",
                        preferred_session_run_reference(&worker_payload)
                            .as_str()
                            .unwrap_or("unknown"),
                        worker_artifact.path
                    )),
                }),
            )
            .await?;
            Ok(attach_worker_dispatch_reference(
                response.0,
                Some(&worker_payload),
            ))
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
                    validation_steps: parsed_merge
                        .as_ref()
                        .and_then(|payload| payload.get("validation_steps"))
                        .and_then(Value::as_array)
                        .map(|rows| {
                            rows.iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                    validation_results: parsed_merge
                        .as_ref()
                        .and_then(|payload| payload.get("validation_results"))
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default(),
                    memory_hits_used,
                    notes: Some(format!(
                        "Auto-generated by coder engine worker dispatch. Merge worker run: {}",
                        worker_payload
                            .as_ref()
                            .map(preferred_session_run_reference)
                            .as_ref()
                            .and_then(Value::as_str)
                            .unwrap_or("unavailable")
                    )),
                }),
            )
            .await?;
            Ok(attach_worker_dispatch_reference(
                response.0,
                worker_payload.as_ref(),
            ))
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
    let has_failed_validation = validation_results.iter().any(|row| {
        row.get("status")
            .and_then(Value::as_str)
            .map(str::trim)
            .is_some_and(|status| matches!(status, "failed" | "error" | "timed_out"))
    });
    let (validation_memory_id, validation_memory_artifact) = write_coder_memory_candidate_artifact(
        state,
        record,
        CoderMemoryCandidateKind::ValidationMemory,
        Some(validation_summary.clone()),
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
    if has_failed_validation {
        let (regression_signal_id, regression_signal_artifact) =
            write_coder_memory_candidate_artifact(
                state,
                record,
                CoderMemoryCandidateKind::RegressionSignal,
                Some(format!("Issue fix validation failed: {validation_summary}")),
                Some("validate_fix".to_string()),
                json!({
                    "workflow_mode": "issue_fix",
                    "summary": summary,
                    "root_cause": root_cause,
                    "fix_strategy": fix_strategy,
                    "changed_files": changed_files,
                    "validation_steps": validation_steps,
                    "validation_results": validation_results,
                    "regression_signals": validation_results
                        .iter()
                        .filter(|row| {
                            row.get("status")
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .is_some_and(|status| matches!(status, "failed" | "error" | "timed_out"))
                        })
                        .map(|row| {
                            json!({
                                "kind": row.get("kind").and_then(Value::as_str).unwrap_or("validation_failure"),
                                "status": row.get("status").cloned().unwrap_or_else(|| json!("failed")),
                                "summary": row
                                    .get("summary")
                                    .cloned()
                                    .unwrap_or_else(|| json!(validation_summary)),
                            })
                        })
                        .collect::<Vec<_>>(),
                    "memory_hits_used": memory_hits_used,
                    "notes": notes,
                    "summary_artifact_path": summary_artifact_path,
                    "validation_artifact_path": validation_artifact.path,
                }),
            )
            .await?;
        generated_candidates.push(json!({
            "candidate_id": regression_signal_id,
            "kind": "regression_signal",
            "artifact_path": regression_signal_artifact.path,
        }));
    }
    Ok((Some(validation_artifact), generated_candidates))
}

async fn write_workflow_validation_artifact(
    state: &AppState,
    record: &CoderRunRecord,
    validation_id_prefix: &str,
    artifact_relpath: &str,
    summary: Option<&str>,
    validation_steps: &[String],
    validation_results: &[Value],
    memory_hits_used: &[String],
    notes: Option<&str>,
    summary_artifact_path: Option<&str>,
    extra_payload: Value,
    phase: Option<&str>,
) -> Result<Option<ContextBlackboardArtifact>, StatusCode> {
    if validation_steps.is_empty() && validation_results.is_empty() {
        return Ok(None);
    }
    let validation_id = format!("{validation_id_prefix}-{}", Uuid::new_v4().simple());
    let mut payload = serde_json::Map::new();
    payload.insert("coder_run_id".to_string(), json!(record.coder_run_id));
    payload.insert(
        "linked_context_run_id".to_string(),
        json!(record.linked_context_run_id),
    );
    payload.insert("workflow_mode".to_string(), json!(record.workflow_mode));
    payload.insert("repo_binding".to_string(), json!(record.repo_binding));
    payload.insert("github_ref".to_string(), json!(record.github_ref));
    payload.insert("summary".to_string(), json!(summary));
    payload.insert("validation_steps".to_string(), json!(validation_steps));
    payload.insert("validation_results".to_string(), json!(validation_results));
    payload.insert("memory_hits_used".to_string(), json!(memory_hits_used));
    payload.insert("notes".to_string(), json!(notes));
    payload.insert(
        "summary_artifact_path".to_string(),
        json!(summary_artifact_path),
    );
    payload.insert("created_at_ms".to_string(), json!(crate::now_ms()));
    if let Value::Object(extra_rows) = extra_payload {
        for (key, value) in extra_rows {
            payload.insert(key, value);
        }
    }
    let validation_artifact = write_coder_artifact(
        state,
        &record.linked_context_run_id,
        &validation_id,
        "coder_validation_report",
        artifact_relpath,
        &Value::Object(payload),
    )
    .await?;
    publish_coder_artifact_added(state, record, &validation_artifact, phase, {
        let mut extra = serde_json::Map::new();
        extra.insert("kind".to_string(), json!("validation_report"));
        extra.insert("workflow_mode".to_string(), json!(record.workflow_mode));
        extra
    });
    Ok(Some(validation_artifact))
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

async fn coder_merge_submit_readiness(
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
            workflow_id: Some("coder_merge_submit".to_string()),
            required_capabilities: vec!["github.merge_pull_request".to_string()],
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
    let memory_hits = collect_coder_memory_hits(&state, coder_run, &retrieval_query, 6).await?;
    let duplicate_candidates = derive_failure_pattern_duplicate_matches(&memory_hits, None, 3);
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
                "duplicate_candidates": duplicate_candidates,
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
