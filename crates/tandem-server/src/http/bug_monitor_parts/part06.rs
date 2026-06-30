fn automation_node_timeout_from_reason(reason: &str) -> Option<(String, u64)> {
    let marker = "automation node `";
    let start = reason.find(marker)? + marker.len();
    let rest = &reason[start..];
    let end = rest.find('`')?;
    let node_id = rest[..end].trim();
    if node_id.is_empty() {
        return None;
    }
    let after = rest[end..].to_ascii_lowercase();
    let timeout_marker = "timed out after ";
    let timeout_start = after.find(timeout_marker)? + timeout_marker.len();
    let timeout_digits = after[timeout_start..]
        .chars()
        .take_while(|ch| ch.is_ascii_digit())
        .collect::<String>();
    let timeout_ms = timeout_digits.parse::<u64>().ok()?;
    Some((node_id.to_string(), timeout_ms))
}

async fn synthesize_bug_monitor_triage_summary(
    state: &AppState,
    draft: &BugMonitorDraftRecord,
    triage_run_id: &str,
) -> anyhow::Result<BugMonitorTriageSummaryInput> {
    let config = state.bug_monitor_config().await;
    let incident = latest_bug_monitor_incident_for_draft(state, &draft.draft_id).await;
    let incident_payload = incident
        .as_ref()
        .and_then(|row| row.event_payload.clone())
        .unwrap_or(Value::Null);
    let title = draft
        .title
        .clone()
        .or_else(|| incident.as_ref().map(|row| row.title.clone()))
        .unwrap_or_else(|| "Bug Monitor failure".to_string());
    let detail = draft
        .detail
        .clone()
        .or_else(|| incident.as_ref().and_then(|row| row.detail.clone()))
        .unwrap_or_default();
    let reason = bug_monitor_value_string(
        &incident_payload,
        &[
            "reason",
            "error",
            "detail",
            "message",
            "failureCode",
            "blockedReasonCode",
        ],
    )
    .or_else(|| {
        incident
            .as_ref()
            .and_then(|row| row.last_error.clone())
            .or_else(|| normalize_issue_draft_line(&detail))
    })
    .unwrap_or_else(|| title.clone());
    let event_type = incident
        .as_ref()
        .map(|row| row.event_type.clone())
        .or_else(|| bug_monitor_value_string(&incident_payload, &["event_type", "event", "type"]))
        .unwrap_or_else(|| "bug_monitor.failure".to_string());
    let failure_type = bug_monitor_failure_type(&reason, &event_type);
    let automation_node_timeout = automation_node_timeout_from_reason(&reason);
    let workflow_id = bug_monitor_value_string(&incident_payload, &["workflow_id", "workflowID"]);
    let run_id = incident
        .as_ref()
        .and_then(|row| row.run_id.clone())
        .or_else(|| bug_monitor_value_string(&incident_payload, &["run_id", "runID"]));
    let task_id = bug_monitor_value_string(
        &incident_payload,
        &[
            "task_id", "taskID", "stage_id", "stageID", "node_id", "nodeID",
        ],
    );
    let artifact_refs = bug_monitor_value_strings(
        &incident_payload,
        &["artifact_refs", "artifactRefs", "artifacts"],
        20,
    );
    let files_touched =
        bug_monitor_value_strings(&incident_payload, &["files_touched", "filesTouched"], 20);
    let duplicate_matches = bug_monitor_failure_pattern_matches(
        state,
        &draft.repo,
        &draft.fingerprint,
        draft.title.as_deref(),
        draft.detail.as_deref(),
        &incident
            .as_ref()
            .map(|row| row.excerpt.clone())
            .unwrap_or_default(),
        5,
    )
    .await;
    let default_workspace_root = state.workspace_index.snapshot().await.root;
    let workspace_root = config
        .workspace_root
        .clone()
        .or_else(|| incident.as_ref().map(|row| row.workspace_root.clone()))
        .filter(|row| !row.trim().is_empty())
        .unwrap_or(default_workspace_root);
    let terms = bug_monitor_candidate_search_terms(draft, incident.as_ref(), &incident_payload);
    let mut file_references = bug_monitor_search_repo_file_references(&workspace_root, &terms);
    if file_references.is_empty() {
        file_references = bug_monitor_fallback_file_references(&format!("{reason}\n{detail}"));
    }
    for file in files_touched.iter().take(10) {
        if !file_references
            .iter()
            .any(|row| row.get("path").and_then(Value::as_str) == Some(file.as_str()))
        {
            file_references.push(json!({
                "path": file,
                "line": Value::Null,
                "excerpt": Value::Null,
                "reason": "The failure event reported this file as touched or relevant.",
                "confidence": "medium",
            }));
        }
    }
    let likely_files_to_edit = file_references
        .iter()
        .filter_map(|row| row.get("path").and_then(Value::as_str))
        .map(str::to_string)
        .take(12)
        .collect::<Vec<_>>();
    let affected_components = [
        bug_monitor_value_string(&incident_payload, &["component"]),
        workflow_id.clone(),
        task_id.clone(),
    ]
    .into_iter()
    .flatten()
    .take(8)
    .collect::<Vec<_>>();
    let confidence = if !likely_files_to_edit.is_empty() {
        "medium"
    } else {
        "low"
    };
    let suggested_title = match (workflow_id.as_deref(), task_id.as_deref()) {
        (Some(workflow), Some(task)) => {
            format!(
                "Workflow {workflow} failed at {task}: {}",
                crate::truncate_text(&reason, 120)
            )
        }
        (_, Some(task)) => format!("{task} failed: {}", crate::truncate_text(&reason, 120)),
        _ => title.clone(),
    };
    let what_happened = [
        Some(title.clone()),
        Some(format!("Event: {event_type}")),
        run_id.as_ref().map(|run| format!("Run: {run}")),
        task_id.as_ref().map(|task| format!("Task/stage: {task}")),
        Some(format!("Reason: {reason}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("\n");
    let why = if let Some((node_id, timeout_ms)) = automation_node_timeout.as_ref() {
        format!(
            "The target workflow node `{node_id}` exhausted its {timeout_ms} ms node timeout. That is the reportable failure even when deeper task-specific logs are unavailable. For generated workflows, especially `execute_goal`, this usually means the runtime budget was too short for real work or the node stalled without producing progress evidence before the timeout."
        )
    } else if likely_files_to_edit.is_empty() {
        format!(
            "The failure is classified as `{failure_type}` from the reported event and error text, but local file evidence was not strong enough to mark this coder-ready."
        )
    } else {
        format!(
            "The failure is classified as `{failure_type}`. Local repository research found likely implementation points connected to the reported event, error text, or artifact validation path."
        )
    };
    let recommended_fix = match failure_type.as_str() {
        "validation_error" => {
            "Tighten the failing artifact/output validation path so terminal failures include the exact missing or invalid output, and ensure the node writes a completed artifact before it can finish.".to_string()
        }
        "timeout" => {
            if let Some((node_id, timeout_ms)) = automation_node_timeout.as_ref() {
                format!(
                    "Increase or explicitly materialize the timeout budget for workflow node `{node_id}` beyond {timeout_ms} ms when it performs long-running work, and preserve the node timeout reason in Bug Monitor issue drafts so operators can distinguish a slow/stuck workflow from a generic triage failure."
                )
            } else {
                "Identify why the node exceeded its timeout, add a fast readiness/failure path for unavailable dependencies, and make retry output deterministic.".to_string()
            }
        }
        "tool_error" => {
            "Route the failing tool call through the shared readiness/resolution path, preserve the typed tool error, and add a regression fixture for the selected tool alias.".to_string()
        }
        _ => {
            "Use the referenced files and artifacts to isolate the failing path, add a narrow regression test, and update the responsible validator or runtime branch.".to_string()
        }
    };
    let acceptance_criteria = vec![
        "The same failure event produces one Bug Monitor draft with a completed triage summary.".to_string(),
        "The triage summary includes file references, a suspected cause, a bounded fix, and verification steps.".to_string(),
        "Workflow node timeout reports preserve the node id and timeout budget in the generated issue draft.".to_string(),
        "Issue draft generation remains blocked when research or validation artifacts are missing.".to_string(),
    ];
    let verification_steps = vec![
        "Run the Bug Monitor triage-summary endpoint for the affected draft and confirm completed inspection/research/validation/fix artifacts are written.".to_string(),
        "Regenerate the issue draft and confirm the proposal quality gate passes only with non-placeholder artifacts.".to_string(),
        "Retry the affected workflow or fixture event and confirm it does not publish a low-signal GitHub issue.".to_string(),
    ];
    let research_sources = file_references
        .iter()
        .take(12)
        .map(|row| {
            json!({
                "source": "local_repo",
                "path": row.get("path").cloned().unwrap_or(Value::Null),
                "line": row.get("line").cloned().unwrap_or(Value::Null),
                "reason": row.get("reason").cloned().unwrap_or(Value::Null),
            })
        })
        .collect::<Vec<_>>();
    let fix_points = vec![json!({
        "component": affected_components.first().cloned().unwrap_or_else(|| "Bug Monitor triage".to_string()),
        "problem": reason,
        "likely_files": likely_files_to_edit,
        "proposed_change": recommended_fix,
        "verification": verification_steps,
        "confidence": confidence,
    })];
    let inspection = json!({
        "draft_id": draft.draft_id,
        "repo": draft.repo,
        "triage_run_id": triage_run_id,
        "title": title.clone(),
        "detail": detail.clone(),
        "event_type": event_type.clone(),
        "reason": reason.clone(),
        "incident": incident.clone(),
        "incident_payload": incident_payload.clone(),
        "workflow_id": workflow_id.clone(),
        "run_id": run_id.clone(),
        "task_id": task_id.clone(),
        "artifact_refs": artifact_refs.clone(),
        "files_touched": files_touched.clone(),
        "created_at_ms": crate::now_ms(),
    });
    let research = json!({
        "draft_id": draft.draft_id,
        "repo": draft.repo,
        "summary": why,
        "search_terms": terms,
        "research_sources": research_sources.clone(),
        "file_references": file_references.clone(),
        "related_failure_patterns": duplicate_matches.clone(),
        "artifact_refs": artifact_refs.clone(),
        "created_at_ms": crate::now_ms(),
    });
    let validation = json!({
        "draft_id": draft.draft_id,
        "repo": draft.repo,
        "summary": "Deterministic triage validated the failure scope from the terminal event, draft detail, artifact refs, and local source references.",
        "failure_scope": failure_type,
        "evidence": [what_happened],
        "steps_to_reproduce": [
            "Replay or re-run the workflow/run identified in the Bug Monitor incident.",
            "Observe the same terminal failure reason and generated artifact refs."
        ],
        "created_at_ms": crate::now_ms(),
    });
    let fix = json!({
        "draft_id": draft.draft_id,
        "repo": draft.repo,
        "recommended_fix": recommended_fix.clone(),
        "fix_points": fix_points.clone(),
        "likely_files_to_edit": likely_files_to_edit.clone(),
        "acceptance_criteria": acceptance_criteria.clone(),
        "verification_steps": verification_steps.clone(),
        "risk_level": "medium",
        "coder_ready": confidence != "low",
        "created_at_ms": crate::now_ms(),
    });
    for (artifact_id, artifact_type, path, payload) in [
        (
            format!("bug-monitor-inspection-{}", Uuid::new_v4().simple()),
            "bug_monitor_inspection",
            "artifacts/bug_monitor.inspection.json",
            inspection,
        ),
        (
            format!("bug-monitor-research-{}", Uuid::new_v4().simple()),
            "bug_monitor_research",
            "artifacts/bug_monitor.research.json",
            research,
        ),
        (
            format!("bug-monitor-validation-{}", Uuid::new_v4().simple()),
            "bug_monitor_validation",
            "artifacts/bug_monitor.validation.json",
            validation,
        ),
        (
            format!("bug-monitor-fix-proposal-{}", Uuid::new_v4().simple()),
            "bug_monitor_fix_proposal",
            "artifacts/bug_monitor.fix_proposal.json",
            fix,
        ),
    ] {
        write_bug_monitor_artifact(
            state,
            triage_run_id,
            &artifact_id,
            artifact_type,
            path,
            &payload,
        )
        .await
        .map_err(|status| {
            anyhow::anyhow!("Failed to write synthesized triage artifact: HTTP {status}")
        })?;
    }
    Ok(BugMonitorTriageSummaryInput {
        suggested_title: Some(suggested_title),
        what_happened: Some(what_happened),
        why_it_likely_happened: Some(why),
        root_cause_confidence: Some(confidence.to_string()),
        failure_type: Some(failure_type),
        affected_components,
        likely_files_to_edit,
        expected_behavior: Some("The workflow or runtime step should complete or fail with a single actionable, deduped Bug Monitor report.".to_string()),
        steps_to_reproduce: vec![
            "Replay or re-run the workflow/run identified in the Bug Monitor incident.".to_string(),
            "Observe the terminal failure reason and associated artifact refs.".to_string(),
        ],
        environment: vec![
            format!("Repo: {}", draft.repo),
            format!("Workspace: {workspace_root}"),
            "Process: tandem-engine".to_string(),
        ],
        logs: vec![crate::truncate_text(
            &format!("{}\n\n{}", draft.detail.clone().unwrap_or_default(), reason),
            1_500,
        )],
        related_existing_issues: Vec::new(),
        related_failure_patterns: duplicate_matches,
        research_sources,
        file_references,
        fix_points,
        recommended_fix: Some(recommended_fix),
        acceptance_criteria,
        verification_steps,
        coder_ready: Some(confidence != "low"),
        risk_level: Some("medium".to_string()),
        required_tool_scopes: Vec::new(),
        missing_tool_scopes: Vec::new(),
        permissions_available: Some(true),
        notes: Some("Generated by deterministic Bug Monitor triage synthesis from the incident, draft, artifact refs, memory matches, and local repository references.".to_string()),
    })
}

pub(super) async fn create_bug_monitor_triage_summary(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<BugMonitorTriageSummaryInput>,
) -> Response {
    let mut draft = match state.get_bug_monitor_draft(&id).await {
        Some(draft) => draft,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "Bug Monitor draft not found",
                    "code": "BUG_MONITOR_DRAFT_NOT_FOUND",
                    "draft_id": id,
                })),
            )
                .into_response();
        }
    };
    let Some(triage_run_id) = draft.triage_run_id.clone() else {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Bug Monitor draft needs a triage run before a triage summary can be written",
                "code": "BUG_MONITOR_TRIAGE_SUMMARY_REQUIRES_RUN",
                "draft_id": id,
            })),
        )
            .into_response();
    };
    let input = if bug_monitor_triage_summary_input_has_substance(&input) {
        input
    } else {
        match synthesize_bug_monitor_triage_summary(&state, &draft, &triage_run_id).await {
            Ok(synthesized) => synthesized,
            Err(error) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "Failed to synthesize Bug Monitor triage summary",
                        "code": "BUG_MONITOR_TRIAGE_SYNTHESIS_FAILED",
                        "draft_id": id,
                        "triage_run_id": triage_run_id,
                        "detail": error.to_string(),
                    })),
                )
                    .into_response();
            }
        }
    };
    let what_happened = input
        .what_happened
        .as_deref()
        .and_then(normalize_issue_draft_line)
        .or_else(|| draft.title.as_deref().and_then(normalize_issue_draft_line))
        .unwrap_or_else(|| "Bug Monitor detected a failure that needs triage.".to_string());
    let expected_behavior = input
        .expected_behavior
        .as_deref()
        .and_then(normalize_issue_draft_line)
        .unwrap_or_else(|| "The failing flow should complete without an error.".to_string());
    let steps_to_reproduce = input
        .steps_to_reproduce
        .into_iter()
        .filter_map(normalize_issue_draft_line)
        .take(8)
        .collect::<Vec<_>>();
    let environment = input
        .environment
        .into_iter()
        .filter_map(normalize_issue_draft_line)
        .take(12)
        .collect::<Vec<_>>();
    let logs = input
        .logs
        .into_iter()
        .filter_map(normalize_issue_draft_line)
        .take(20)
        .collect::<Vec<_>>();
    let affected_components = input
        .affected_components
        .into_iter()
        .filter_map(normalize_issue_draft_line)
        .take(20)
        .collect::<Vec<_>>();
    let likely_files_to_edit = input
        .likely_files_to_edit
        .into_iter()
        .filter_map(normalize_issue_draft_line)
        .take(30)
        .collect::<Vec<_>>();
    let acceptance_criteria = input
        .acceptance_criteria
        .into_iter()
        .filter_map(normalize_issue_draft_line)
        .take(20)
        .collect::<Vec<_>>();
    let verification_steps = input
        .verification_steps
        .into_iter()
        .filter_map(normalize_issue_draft_line)
        .take(20)
        .collect::<Vec<_>>();
    let required_tool_scopes = input
        .required_tool_scopes
        .into_iter()
        .filter_map(normalize_issue_draft_line)
        .take(20)
        .collect::<Vec<_>>();
    let missing_tool_scopes = input
        .missing_tool_scopes
        .into_iter()
        .filter_map(normalize_issue_draft_line)
        .take(20)
        .collect::<Vec<_>>();
    let confidence = input
        .root_cause_confidence
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .filter(|value| matches!(value.as_str(), "high" | "medium" | "low"))
        .unwrap_or_else(|| "low".to_string());
    let failure_type = input
        .failure_type
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .filter(|value| {
            matches!(
                value.as_str(),
                "code_defect"
                    | "missing_config"
                    | "missing_capability"
                    | "model_error"
                    | "tool_error"
                    | "validation_error"
                    | "timeout"
                    | "external_dependency"
                    | "unknown"
            )
        })
        .unwrap_or_else(|| "unknown".to_string());
    let risk_level = input
        .risk_level
        .as_deref()
        .map(str::trim)
        .map(str::to_ascii_lowercase)
        .filter(|value| matches!(value.as_str(), "low" | "medium" | "high"))
        .unwrap_or_else(|| "medium".to_string());
    let (coder_ready, coder_ready_gate) = bug_monitor_coder_ready_gate(
        input.coder_ready,
        &confidence,
        &likely_files_to_edit,
        &affected_components,
        &acceptance_criteria,
        &verification_steps,
        &risk_level,
        false,
        &required_tool_scopes,
        &missing_tool_scopes,
        input.permissions_available,
    );
    let payload = json!({
        "draft_id": draft.draft_id,
        "repo": draft.repo,
        "triage_run_id": triage_run_id,
        "suggested_title": input.suggested_title.as_deref().and_then(normalize_issue_draft_line),
        "what_happened": what_happened,
        "why_it_likely_happened": input.why_it_likely_happened.as_deref().and_then(normalize_issue_draft_line),
        "root_cause_confidence": confidence,
        "failure_type": failure_type,
        "affected_components": affected_components,
        "likely_files_to_edit": likely_files_to_edit,
        "expected_behavior": expected_behavior,
        "steps_to_reproduce": steps_to_reproduce,
        "environment": environment,
        "logs": logs,
        "related_existing_issues": input.related_existing_issues,
        "related_failure_patterns": input.related_failure_patterns,
        "research_sources": input.research_sources,
        "file_references": input.file_references,
        "fix_points": input.fix_points,
        "recommended_fix": input.recommended_fix.as_deref().and_then(normalize_issue_draft_line),
        "acceptance_criteria": acceptance_criteria,
        "verification_steps": verification_steps,
        "coder_ready": coder_ready,
        "coder_ready_gate": coder_ready_gate,
        "risk_level": risk_level,
        "required_tool_scopes": required_tool_scopes,
        "missing_tool_scopes": missing_tool_scopes,
        "permissions_available": input.permissions_available,
        "notes": input.notes.as_deref().and_then(normalize_issue_draft_line),
        "created_at_ms": crate::now_ms(),
    });
    let artifact_id = format!("bug-monitor-triage-summary-{}", Uuid::new_v4().simple());
    match write_bug_monitor_artifact(
        &state,
        &triage_run_id,
        &artifact_id,
        "bug_monitor_triage_summary",
        "artifacts/bug_monitor.triage_summary.json",
        &payload,
    )
    .await
    {
        Ok(()) => {}
        Err(status) => {
            return (
                status,
                Json(json!({
                    "error": "Failed to write Bug Monitor triage summary",
                    "code": "BUG_MONITOR_TRIAGE_SUMMARY_WRITE_FAILED",
                    "draft_id": id,
                })),
            )
                .into_response();
        }
    }

    let summary_artifact_path = context_run_dir(&state, &triage_run_id)
        .join("artifacts/bug_monitor.triage_summary.json")
        .to_string_lossy()
        .to_string();
    let failure_pattern_memory = match persist_bug_monitor_failure_pattern_memory(
        &state,
        &draft,
        &triage_run_id,
        &payload,
        &summary_artifact_path,
    )
    .await
    {
        Ok(memory) => {
            if memory
                .get("stored")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                let memory_artifact_id = format!(
                    "bug-monitor-failure-pattern-memory-{}",
                    Uuid::new_v4().simple()
                );
                let _ = write_bug_monitor_artifact(
                    &state,
                    &triage_run_id,
                    &memory_artifact_id,
                    "bug_monitor_failure_pattern_memory",
                    "artifacts/bug_monitor.failure_pattern_memory.json",
                    &memory,
                )
                .await;
            }
            Some(memory)
        }
        Err(_) => None,
    };
    let regression_signal_memory = match persist_bug_monitor_regression_signal_memory(
        &state,
        &draft,
        &triage_run_id,
        &payload,
        &summary_artifact_path,
    )
    .await
    {
        Ok(memory) => {
            if memory
                .get("stored")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                let memory_artifact_id = format!(
                    "bug-monitor-regression-signal-memory-{}",
                    Uuid::new_v4().simple()
                );
                let _ = write_bug_monitor_artifact(
                    &state,
                    &triage_run_id,
                    &memory_artifact_id,
                    "bug_monitor_regression_signal_memory",
                    "artifacts/bug_monitor.regression_signal_memory.json",
                    &memory,
                )
                .await;
            }
            Some(memory)
        }
        Err(_) => None,
    };

    draft.github_status = Some("triage_summary_ready".to_string());
    if draft.status.eq_ignore_ascii_case("triage_queued")
        || draft.status.eq_ignore_ascii_case("github_post_failed")
        || draft.status.eq_ignore_ascii_case("proposal_blocked")
    {
        draft.status = "draft_ready".to_string();
    }
    let draft = match state.put_bug_monitor_draft(draft).await {
        Ok(draft) => draft,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Failed to update Bug Monitor draft after triage summary",
                    "code": "BUG_MONITOR_TRIAGE_SUMMARY_DRAFT_UPDATE_FAILED",
                    "draft_id": id,
                    "detail": error.to_string(),
                })),
            )
                .into_response();
        }
    };
    let (triage_summary_artifact, _issue_draft_artifact, duplicate_matches_artifact) =
        bug_monitor_triage_artifacts(&state, Some(&triage_run_id));
    if let Err(status) =
        ensure_bug_monitor_phase_artifacts_from_summary(&state, &triage_run_id, &payload).await
    {
        return (
            status,
            Json(json!({
                "error": "Bug Monitor triage summary was written, but phase artifact materialization failed",
                "code": "BUG_MONITOR_TRIAGE_PHASE_ARTIFACT_WRITE_FAILED",
                "draft": draft,
                "triage_summary": payload,
                "triage_summary_artifact": triage_summary_artifact,
                "failure_pattern_memory": failure_pattern_memory,
                "regression_signal_memory": regression_signal_memory,
                "duplicate_matches_artifact": duplicate_matches_artifact,
            })),
        )
            .into_response();
    }
    match ensure_bug_monitor_issue_draft(state.clone(), &id, true).await {
        Ok(issue_draft) => {
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                bug_monitor_triage_artifacts(&state, Some(&triage_run_id));
            Json(json!({
                "ok": true,
                "draft": draft,
                "triage_summary": payload,
                "triage_summary_artifact": triage_summary_artifact,
                "failure_pattern_memory": failure_pattern_memory,
                "regression_signal_memory": regression_signal_memory,
                "issue_draft": issue_draft,
                "issue_draft_artifact": issue_draft_artifact,
                "duplicate_matches_artifact": duplicate_matches_artifact,
            }))
            .into_response()
        }
        Err(error) => (
            StatusCode::BAD_REQUEST,
            {
                let proposal_quality_gate =
                    load_bug_monitor_proposal_quality_gate_artifact(&state, &triage_run_id).await;
                let proposal_quality_gate_artifact = latest_bug_monitor_artifact(
                    &state,
                    &triage_run_id,
                    "bug_monitor_proposal_quality_gate",
                );
                Json(json!({
                    "error": "Bug Monitor triage summary was written, but issue draft regeneration failed",
                    "code": "BUG_MONITOR_TRIAGE_SUMMARY_ISSUE_DRAFT_FAILED",
                    "draft": draft,
                    "triage_summary": payload,
                    "triage_summary_artifact": triage_summary_artifact,
                    "failure_pattern_memory": failure_pattern_memory,
                    "regression_signal_memory": regression_signal_memory,
                    "duplicate_matches_artifact": duplicate_matches_artifact,
                    "proposal_quality_gate": proposal_quality_gate,
                    "proposal_quality_gate_artifact": proposal_quality_gate_artifact,
                    "detail": error.to_string(),
                }))
            },
        )
            .into_response(),
    }
}

pub(super) async fn get_bug_monitor_config(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let config = state.bug_monitor_config().await;
    Json(json!({
        "bug_monitor": config
    }))
}

pub(super) async fn patch_bug_monitor_config(
    State(state): State<AppState>,
    Json(input): Json<BugMonitorConfigInput>,
) -> Response {
    let Some(config) = input.bug_monitor else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "bug_monitor object is required",
                "code": "BUG_MONITOR_CONFIG_REQUIRED",
            })),
        )
            .into_response();
    };
    match state.put_bug_monitor_config(config).await {
        Ok(saved) => {
            emit_bug_monitor_config_audit(&state, &saved).await;
            Json(json!({ "bug_monitor": saved })).into_response()
        }
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid bug monitor config",
                "code": "BUG_MONITOR_CONFIG_INVALID",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn get_bug_monitor_status(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let status = state.bug_monitor_status().await;
    Json(json!({
        "status": status
    }))
}

pub(super) async fn recompute_bug_monitor_status(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let status = state.bug_monitor_status().await;
    Json(json!({
        "status": status
    }))
}

pub(super) async fn get_bug_monitor_debug(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let status = state.bug_monitor_status().await;
    let selected_server_tools = if let Some(server_name) = status.config.mcp_server.as_deref() {
        state.mcp.server_tools(server_name).await
    } else {
        Vec::new()
    };
    let canonicalized_discovered_tools = selected_server_tools
        .iter()
        .map(|tool| {
            json!({
                "server_name": tool.server_name,
                "tool_name": tool.tool_name,
                "namespaced_name": tool.namespaced_name,
                "canonical_name": canonicalize_tool_name(&tool.namespaced_name),
            })
        })
        .collect::<Vec<_>>();
    Json(json!({
        "status": status,
        "selected_server_tools": selected_server_tools,
        "canonicalized_discovered_tools": canonicalized_discovered_tools,
    }))
}

pub(super) async fn list_bug_monitor_incidents(
    State(state): State<AppState>,
    Query(query): Query<BugMonitorIncidentsQuery>,
) -> Json<serde_json::Value> {
    let incidents = state
        .list_bug_monitor_incidents(query.limit.unwrap_or(50))
        .await;
    Json(json!({
        "incidents": incidents,
        "count": incidents.len(),
    }))
}

pub(super) async fn get_bug_monitor_incident(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match state.get_bug_monitor_incident(&id).await {
        Some(incident) => Json(json!({ "incident": incident })).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug monitor incident not found",
                "code": "BUG_MONITOR_INCIDENT_NOT_FOUND",
                "incident_id": id,
            })),
        )
            .into_response(),
    }
}

pub(super) async fn list_bug_monitor_drafts(
    State(state): State<AppState>,
    Query(query): Query<BugMonitorDraftsQuery>,
) -> Json<serde_json::Value> {
    let drafts = state
        .list_bug_monitor_drafts(query.limit.unwrap_or(50))
        .await;
    Json(json!({
        "drafts": drafts,
        "count": drafts.len(),
    }))
}

pub(super) async fn list_bug_monitor_posts(
    State(state): State<AppState>,
    Query(query): Query<BugMonitorPostsQuery>,
) -> Json<serde_json::Value> {
    let limit = query.limit.unwrap_or(50);
    let posts = if let Some(destination_id) = query
        .destination_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        state
            .list_bug_monitor_posts_by_destination(limit, destination_id)
            .await
    } else {
        state.list_bug_monitor_posts(limit).await
    };
    Json(json!({
        "posts": posts,
        "count": posts.len(),
    }))
}

pub(super) async fn delete_bug_monitor_incident(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match state.delete_bug_monitor_incidents(&[id.clone()]).await {
        Ok(0) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug monitor incident not found",
                "code": "BUG_MONITOR_INCIDENT_NOT_FOUND",
                "incident_id": id,
            })),
        )
            .into_response(),
        Ok(_) => Json(json!({ "ok": true, "deleted": 1 })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to delete Bug Monitor incident",
                "code": "BUG_MONITOR_INCIDENT_DELETE_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn bulk_delete_bug_monitor_incidents(
    State(state): State<AppState>,
    Json(input): Json<BugMonitorBulkDeleteInput>,
) -> Response {
    let result = if input.all {
        state.clear_bug_monitor_incidents().await
    } else {
        state.delete_bug_monitor_incidents(&input.ids).await
    };
    match result {
        Ok(deleted) => Json(json!({ "ok": true, "deleted": deleted })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to delete Bug Monitor incidents",
                "code": "BUG_MONITOR_INCIDENTS_DELETE_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn delete_bug_monitor_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match state.delete_bug_monitor_drafts(&[id.clone()]).await {
        Ok(0) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug monitor draft not found",
                "code": "BUG_MONITOR_DRAFT_NOT_FOUND",
                "draft_id": id,
            })),
        )
            .into_response(),
        Ok(_) => Json(json!({ "ok": true, "deleted": 1 })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to delete Bug Monitor draft",
                "code": "BUG_MONITOR_DRAFT_DELETE_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn bulk_delete_bug_monitor_drafts(
    State(state): State<AppState>,
    Json(input): Json<BugMonitorBulkDeleteInput>,
) -> Response {
    let result = if input.all {
        state.clear_bug_monitor_drafts().await
    } else {
        state.delete_bug_monitor_drafts(&input.ids).await
    };
    match result {
        Ok(deleted) => Json(json!({ "ok": true, "deleted": deleted })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to delete Bug Monitor drafts",
                "code": "BUG_MONITOR_DRAFTS_DELETE_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn delete_bug_monitor_post(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match state.delete_bug_monitor_posts(&[id.clone()]).await {
        Ok(0) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug monitor post not found",
                "code": "BUG_MONITOR_POST_NOT_FOUND",
                "post_id": id,
            })),
        )
            .into_response(),
        Ok(_) => Json(json!({ "ok": true, "deleted": 1 })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to delete Bug Monitor post",
                "code": "BUG_MONITOR_POST_DELETE_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn bulk_delete_bug_monitor_posts(
    State(state): State<AppState>,
    Json(input): Json<BugMonitorBulkDeleteInput>,
) -> Response {
    let result = if input.all {
        state.clear_bug_monitor_posts().await
    } else {
        state.delete_bug_monitor_posts(&input.ids).await
    };
    match result {
        Ok(deleted) => Json(json!({ "ok": true, "deleted": deleted })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to delete Bug Monitor posts",
                "code": "BUG_MONITOR_POSTS_DELETE_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn pause_bug_monitor(State(state): State<AppState>) -> Response {
    let mut config = state.bug_monitor_config().await;
    config.paused = true;
    match state.put_bug_monitor_config(config).await {
        Ok(saved) => Json(json!({ "ok": true, "bug_monitor": saved })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to pause Bug Monitor",
                "code": "BUG_MONITOR_PAUSE_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn resume_bug_monitor(State(state): State<AppState>) -> Response {
    let mut config = state.bug_monitor_config().await;
    config.paused = false;
    match state.put_bug_monitor_config(config).await {
        Ok(saved) => Json(json!({ "ok": true, "bug_monitor": saved })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to resume Bug Monitor",
                "code": "BUG_MONITOR_RESUME_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn replay_bug_monitor_incident(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let Some(incident) = state.get_bug_monitor_incident(&id).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug monitor incident not found",
                "code": "BUG_MONITOR_INCIDENT_NOT_FOUND",
                "incident_id": id,
            })),
        )
            .into_response();
    };
    let Some(draft_id) = incident.draft_id.as_deref() else {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Bug monitor incident has no associated draft",
                "code": "BUG_MONITOR_INCIDENT_NO_DRAFT",
                "incident_id": id,
            })),
        )
            .into_response();
    };
    match ensure_bug_monitor_triage_run(state.clone(), draft_id, true).await {
        Ok((draft, run, deduped)) => {
            let triage_run_id = draft.triage_run_id.as_deref().unwrap_or(run.as_str());
            refresh_bug_monitor_duplicate_matches_artifact(&state, &draft, triage_run_id).await;
            let run = load_context_run_state(&state, triage_run_id).await.ok();
            let triage_summary =
                load_bug_monitor_triage_summary_artifact(&state, triage_run_id).await;
            let issue_draft = ensure_bug_monitor_issue_draft(state.clone(), draft_id, true)
                .await
                .ok();
            let (duplicate_summary, duplicate_matches) =
                bug_monitor_duplicate_match_context(&state, Some(triage_run_id)).await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                bug_monitor_triage_artifacts(&state, Some(triage_run_id));
            Json(json!({
                "ok": true,
                "incident": incident,
                "draft": draft,
                "run": run,
                "deduped": deduped,
                "triage_summary": triage_summary,
                "triage_summary_artifact": triage_summary_artifact,
                "issue_draft": issue_draft,
                "issue_draft_artifact": issue_draft_artifact,
                "duplicate_summary": duplicate_summary,
                "duplicate_matches": duplicate_matches,
                "duplicate_matches_artifact": duplicate_matches_artifact,
            }))
            .into_response()
        }
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to replay Bug Monitor incident",
                "code": "BUG_MONITOR_INCIDENT_REPLAY_FAILED",
                "incident_id": id,
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn get_bug_monitor_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let draft = state.get_bug_monitor_draft(&id).await;
    match draft {
        Some(draft) => Json(json!({ "draft": draft })).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug monitor draft not found",
                "code": "BUG_MONITOR_DRAFT_NOT_FOUND",
            })),
        )
            .into_response(),
    }
}

fn map_bug_monitor_draft_update_error(
    draft_id: String,
    error: anyhow::Error,
) -> (StatusCode, Json<serde_json::Value>) {
    let detail = error.to_string();
    if detail.contains("not found") {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug Monitor draft not found",
                "code": "BUG_MONITOR_DRAFT_NOT_FOUND",
                "draft_id": draft_id,
            })),
        )
    } else if detail.contains("not waiting for approval") {
        (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Bug Monitor draft is not waiting for approval",
                "code": "BUG_MONITOR_DRAFT_NOT_PENDING_APPROVAL",
                "draft_id": draft_id,
                "detail": detail,
            })),
        )
    } else {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to update Bug Monitor draft",
                "code": "BUG_MONITOR_DRAFT_UPDATE_FAILED",
                "draft_id": draft_id,
                "detail": detail,
            })),
        )
    }
}

pub(super) async fn report_bug_monitor_issue(
    State(state): State<AppState>,
    Json(input): Json<BugMonitorSubmissionInput>,
) -> Response {
    let Some(mut report) = input.report else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "report object is required",
                "code": "BUG_MONITOR_REPORT_REQUIRED",
            })),
        )
            .into_response();
    };
    let config = state.bug_monitor_config().await;
    let effective_repo = report
        .repo
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or(config.repo.as_deref())
        .unwrap_or_default()
        .to_string();
    apply_bug_monitor_report_source_approval_binding(&config, &mut report);
    let duplicate_matches = bug_monitor_failure_pattern_matches(
        &state,
        &effective_repo,
        report.fingerprint.as_deref().unwrap_or_default(),
        report.title.as_deref(),
        report.detail.as_deref(),
        &report.excerpt,
        3,
    )
    .await;
    if !duplicate_matches.is_empty() {
        let duplicate_summary = build_bug_monitor_duplicate_summary(&duplicate_matches);
        return Json(json!({
            "suppressed": true,
            "reason": "duplicate_failure_pattern",
            "duplicate_summary": duplicate_summary,
            "duplicate_matches": duplicate_matches,
        }))
        .into_response();
    }
    let report_excerpt = report.excerpt.clone();
    match state.submit_bug_monitor_draft(report.clone()).await {
        Ok(draft) => {
            let duplicate_matches = bug_monitor_failure_pattern_matches(
                &state,
                &draft.repo,
                &draft.fingerprint,
                draft.title.as_deref(),
                draft.detail.as_deref(),
                &report_excerpt,
                3,
            )
            .await;
            Json(json!({
                "draft": draft,
                "duplicate_summary": build_bug_monitor_duplicate_summary(&duplicate_matches),
                "duplicate_matches": duplicate_matches,
            }))
            .into_response()
        }
        Err(error) => {
            let detail = error.to_string();
            let blocked_incident = if detail.contains("signal quality gate") {
                persist_blocked_bug_monitor_report_observation(
                    &state,
                    &report,
                    &effective_repo,
                    &detail,
                )
                .await
            } else {
                None
            };
            let quality_gate = blocked_incident
                .as_ref()
                .and_then(|incident| incident.quality_gate.clone());
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Failed to create Bug Monitor draft",
                    "code": "BUG_MONITOR_REPORT_INVALID",
                    "detail": detail,
                    "incident": blocked_incident,
                    "quality_gate": quality_gate,
                })),
            )
                .into_response()
        }
    }
}

pub(super) async fn report_bug_monitor_intake(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<BugMonitorIntakeReportInput>,
) -> Response {
    let project_id = input
        .project_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
        .to_string();
    if project_id.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "project_id is required",
                "code": "BUG_MONITOR_INTAKE_PROJECT_REQUIRED",
            })),
        )
            .into_response();
    }
    let Some(raw_key) = bug_monitor_intake_key_from_headers(&headers) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Bug Monitor intake key is required",
                "code": "BUG_MONITOR_INTAKE_KEY_REQUIRED",
            })),
        )
            .into_response();
    };
    let Some(_key) = state
        .validate_bug_monitor_intake_key(&raw_key, &project_id, "bug_monitor:report")
        .await
    else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Bug Monitor intake key is invalid for this project or scope",
                "code": "BUG_MONITOR_INTAKE_KEY_INVALID",
            })),
        )
            .into_response();
    };
    let config = state.bug_monitor_config().await;
    let Some(project) = config
        .monitored_projects
        .iter()
        .find(|project| project.project_id == project_id)
        .cloned()
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "monitored project is not configured",
                "code": "BUG_MONITOR_INTAKE_PROJECT_UNKNOWN",
            })),
        )
            .into_response();
    };
    let Some(mut report) = input.report else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "report object is required",
                "code": "BUG_MONITOR_REPORT_REQUIRED",
            })),
        )
            .into_response();
    };
    let source_id = input
        .source_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("external");
    let configured_source = project
        .log_sources
        .iter()
        .find(|source| source.source_id == source_id);
    if !project.log_sources.is_empty() && configured_source.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "log source is not configured for monitored project",
                "code": "BUG_MONITOR_INTAKE_SOURCE_UNKNOWN",
            })),
        )
            .into_response();
    }
    let binding = project.source_binding(configured_source);
    report.project_id = Some(project.project_id.clone());
    report.workspace_root = Some(project.workspace_root.clone());
    report.log_source_id = Some(source_id.to_string());
    report.source_kind = Some(binding.source_kind.clone());
    report.repo = Some(project.repo.clone());
    report.route_tags = binding.default_route_tags.clone();
    report.allowed_destination_ids = binding.allowed_destination_ids.clone();
    report.default_destination_ids = binding.default_destination_ids.clone();
    report.tenant_id = binding.tenant_id.clone();
    report.workspace_id = binding.workspace_id.clone();
    report.event_schema_version = binding.event_schema_version.clone();
    report.source_approval_policy = Some(binding.approval_policy.clone());
    report.redaction_profile = binding.redaction_profile.clone();
    report.retention_profile = binding.retention_profile.clone();
    if report.source.is_none() {
        report.source = Some(format!("bug_monitor.intake.{source_id}"));
    }
    match state.submit_bug_monitor_draft(report.clone()).await {
        Ok(draft) => {
            let now = crate::now_ms();
            let incident_id = format!("failure-incident-{}", uuid::Uuid::new_v4().simple());
            let incident = BugMonitorIncidentRecord {
                incident_id,
                fingerprint: draft.fingerprint.clone(),
                event_type: report
                    .event
                    .clone()
                    .unwrap_or_else(|| "bug_monitor.external_report".to_string()),
                status: "draft_created".to_string(),
                repo: project.repo.clone(),
                workspace_root: project.workspace_root.clone(),
                title: draft
                    .title
                    .clone()
                    .unwrap_or_else(|| "External failure report".to_string()),
                project_id: Some(project.project_id.clone()),
                log_source_id: Some(source_id.to_string()),
                source_kind: Some(binding.source_kind.clone()),
                detail: draft.detail.clone(),
                excerpt: report.excerpt.clone(),
                source: report.source.clone(),
                component: report.component.clone(),
                level: report.level.clone(),
                occurrence_count: 1,
                created_at_ms: now,
                updated_at_ms: now,
                last_seen_at_ms: Some(now),
                draft_id: Some(draft.draft_id.clone()),
                confidence: draft.confidence.clone(),
                risk_level: draft.risk_level.clone(),
                expected_destination: draft.expected_destination.clone(),
                route_tags: draft.route_tags.clone(),
                allowed_destination_ids: draft.allowed_destination_ids.clone(),
                default_destination_ids: draft.default_destination_ids.clone(),
                tenant_id: draft.tenant_id.clone(),
                workspace_id: draft.workspace_id.clone(),
                event_schema_version: draft.event_schema_version.clone(),
                source_approval_policy: draft.source_approval_policy.clone(),
                redaction_profile: draft.redaction_profile.clone(),
                retention_profile: draft.retention_profile.clone(),
                evidence_refs: draft.evidence_refs.clone(),
                quality_gate: draft.quality_gate.clone(),
                event_payload: Some(json!({
                    "project_id": project.project_id,
                    "source_id": source_id,
                    "source_kind": binding.source_kind.as_str(),
                    "workspace_root": project.workspace_root,
                    "mcp_server": project.mcp_server,
                    "model_policy": project.model_policy,
                    "allowed_destination_ids": binding.allowed_destination_ids,
                    "default_destination_ids": binding.default_destination_ids,
                    "default_route_tags": binding.default_route_tags,
                    "tenant_id": binding.tenant_id,
                    "workspace_id": binding.workspace_id,
                    "event_schema_version": binding.event_schema_version,
                    "approval_policy": binding.approval_policy,
                    "redaction_profile": binding.redaction_profile,
                    "retention_profile": binding.retention_profile,
                    "intake": true,
                })),
                ..BugMonitorIncidentRecord::default()
            };
            let _ = state.put_bug_monitor_incident(incident.clone()).await;
            Json(json!({
                "draft": draft,
                "incident": incident,
            }))
            .into_response()
        }
        Err(error) => {
            let detail = error.to_string();
            let blocked_incident = if detail.contains("signal quality gate") {
                persist_blocked_bug_monitor_report_observation(
                    &state,
                    &report,
                    &project.repo,
                    &detail,
                )
                .await
            } else {
                None
            };
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Failed to create Bug Monitor draft",
                    "code": "BUG_MONITOR_REPORT_INVALID",
                    "detail": detail,
                    "incident": blocked_incident,
                })),
            )
                .into_response()
        }
    }
}

pub(super) async fn list_bug_monitor_intake_keys(State(state): State<AppState>) -> Response {
    let keys = state
        .list_bug_monitor_intake_keys()
        .await
        .into_iter()
        .map(|mut key| {
            key.key_hash = "[redacted]".to_string();
            key
        })
        .collect::<Vec<_>>();
    Json(json!({ "keys": keys })).into_response()
}

pub(super) async fn create_bug_monitor_intake_key(
    State(state): State<AppState>,
    Json(input): Json<BugMonitorCreateIntakeKeyInput>,
) -> Response {
    let project_id = input.project_id.trim().to_string();
    let name = input.name.trim().to_string();
    let config = state.bug_monitor_config().await;
    if !config
        .monitored_projects
        .iter()
        .any(|project| project.project_id == project_id)
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "monitored project is not configured",
                "code": "BUG_MONITOR_INTAKE_PROJECT_UNKNOWN",
            })),
        )
            .into_response();
    }
    if name.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "name is required",
                "code": "BUG_MONITOR_INTAKE_KEY_NAME_REQUIRED",
            })),
        )
            .into_response();
    }
    let raw_key = format!(
        "tbm_intake_{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let scopes = if input.scopes.is_empty() {
        vec!["bug_monitor:report".to_string()]
    } else {
        input
            .scopes
            .into_iter()
            .map(|scope| scope.trim().to_string())
            .filter(|scope| !scope.is_empty())
            .collect()
    };
    let key = crate::BugMonitorProjectIntakeKey {
        key_id: format!("intake-key-{}", uuid::Uuid::new_v4().simple()),
        project_id,
        name,
        key_hash: crate::sha256_hex(&[&raw_key]),
        enabled: true,
        scopes,
        created_at_ms: Some(crate::now_ms()),
        last_used_at_ms: None,
    };
    match state.put_bug_monitor_intake_key(key.clone()).await {
        Ok(mut key) => {
            emit_bug_monitor_intake_key_audit(&state, "bug_monitor.intake_key.created", &key).await;
            key.key_hash = "[redacted]".to_string();
            Json(json!({ "key": key, "raw_key": raw_key })).into_response()
        }
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to create Bug Monitor intake key",
                "code": "BUG_MONITOR_INTAKE_KEY_CREATE_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn disable_bug_monitor_intake_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let Some(mut key) = state.bug_monitor_intake_keys.read().await.get(&id).cloned() else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug Monitor intake key not found",
                "code": "BUG_MONITOR_INTAKE_KEY_NOT_FOUND",
            })),
        )
            .into_response();
    };
    key.enabled = false;
    match state.put_bug_monitor_intake_key(key.clone()).await {
        Ok(mut key) => {
            emit_bug_monitor_intake_key_audit(&state, "bug_monitor.intake_key.disabled", &key)
                .await;
            key.key_hash = "[redacted]".to_string();
            Json(json!({ "key": key })).into_response()
        }
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to disable Bug Monitor intake key",
                "code": "BUG_MONITOR_INTAKE_KEY_DISABLE_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

async fn emit_bug_monitor_config_audit(state: &AppState, config: &BugMonitorConfig) {
    emit_bug_monitor_admin_audit_event(
        state,
        "bug_monitor.config.updated",
        json!({
            "enabled": config.enabled,
            "paused": config.paused,
            "destination_count": config.destinations.len(),
            "route_count": config.routes.len(),
            "default_destination_ids": &config.default_destination_ids,
            "destinations": config.destinations.iter().map(|destination| {
                json!({
                    "destination_id": destination.destination_id.as_str(),
                    "kind": format!("{:?}", &destination.kind),
                    "enabled": destination.enabled,
                    "require_approval": destination.require_approval,
                    "has_webhook_secret_ref": destination.webhook_secret_ref.is_some(),
                    "has_webhook_url": destination.webhook_url.is_some(),
                    "has_mcp_tool": destination.mcp_tool.is_some(),
                    "has_custom_config": destination.config.is_some(),
                })
            }).collect::<Vec<_>>(),
            "routes": config.routes.iter().map(|route| {
                json!({
                    "route_id": route.route_id.as_str(),
                    "priority": route.priority,
                    "destination_ids": &route.destination_ids,
                    "approval_policy": format!("{:?}", &route.approval_policy),
                    "match_source_kinds": &route.match_source_kinds,
                    "match_tenant_ids": &route.match_tenant_ids,
                    "match_workspace_ids": &route.match_workspace_ids,
                })
            }).collect::<Vec<_>>(),
            "monitored_project_ids": config.monitored_projects.iter()
                .map(|project| project.project_id.as_str())
                .collect::<Vec<_>>(),
            "safety_defaults": {
                "require_approval_for_high_risk": config.safety_defaults.require_approval_for_high_risk,
                "redact_secrets": config.safety_defaults.redact_secrets,
                "block_unready_destinations": config.safety_defaults.block_unready_destinations,
                "retention_days": config.safety_defaults.retention_days,
            },
        }),
    )
    .await;
}

async fn emit_bug_monitor_intake_key_audit(
    state: &AppState,
    event_type: &'static str,
    key: &crate::BugMonitorProjectIntakeKey,
) {
    emit_bug_monitor_admin_audit_event(
        state,
        event_type,
        json!({
            "key_id": key.key_id.as_str(),
            "project_id": key.project_id.as_str(),
            "name": key.name.as_str(),
            "enabled": key.enabled,
            "scopes": &key.scopes,
            "created_at_ms": key.created_at_ms,
            "last_used_at_ms": key.last_used_at_ms,
        }),
    )
    .await;
}

async fn emit_bug_monitor_admin_audit_event(
    state: &AppState,
    event_type: &'static str,
    payload: serde_json::Value,
) {
    state
        .event_bus
        .publish(tandem_types::EngineEvent::new(event_type, payload.clone()));
    let _ = crate::audit::append_protected_audit_event(
        state,
        event_type,
        &tandem_types::TenantContext::local_implicit(),
        None,
        payload,
    )
    .await;
}

pub(super) async fn reset_bug_monitor_log_source_offset(
    State(state): State<AppState>,
    Path((project_id, source_id)): Path<(String, String)>,
) -> Response {
    let Some((project, source)) =
        configured_bug_monitor_log_source(&state, &project_id, &source_id).await
    else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug Monitor log source not found",
                "code": "BUG_MONITOR_LOG_SOURCE_NOT_FOUND",
            })),
        )
            .into_response();
    };
    match crate::bug_monitor::log_watcher::reset_log_source_offset(
        &state,
        &project,
        &source,
        crate::now_ms(),
    )
    .await
    {
        Ok(source_state) => Json(json!({
            "project_id": project.project_id,
            "source_id": source.source_id,
            "state": source_state,
        }))
        .into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to reset Bug Monitor log source offset",
                "code": "BUG_MONITOR_LOG_SOURCE_RESET_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn replay_latest_bug_monitor_log_source_candidate(
    State(state): State<AppState>,
    Path((project_id, source_id)): Path<(String, String)>,
) -> Response {
    let Some((project, source)) =
        configured_bug_monitor_log_source(&state, &project_id, &source_id).await
    else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug Monitor log source not found",
                "code": "BUG_MONITOR_LOG_SOURCE_NOT_FOUND",
            })),
        )
            .into_response();
    };
    match crate::bug_monitor::log_watcher::replay_latest_log_source_candidate(
        &state, &project, &source,
    )
    .await
    {
        Ok(Some(result)) => Json(json!({
            "project_id": project.project_id,
            "source_id": source.source_id,
            "incident": result.incident,
            "draft": result.draft,
        }))
        .into_response(),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "No replayable Bug Monitor log candidate was found for this source",
                "code": "BUG_MONITOR_LOG_SOURCE_REPLAY_NOT_FOUND",
            })),
        )
            .into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to replay latest Bug Monitor log source candidate",
                "code": "BUG_MONITOR_LOG_SOURCE_REPLAY_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

async fn configured_bug_monitor_log_source(
    state: &AppState,
    project_id: &str,
    source_id: &str,
) -> Option<(
    crate::BugMonitorMonitoredProject,
    crate::BugMonitorLogSource,
)> {
    let config = state.bug_monitor_config().await;
    let project = config
        .monitored_projects
        .iter()
        .find(|project| project.project_id == project_id)?
        .clone();
    let source = project
        .log_sources
        .iter()
        .find(|source| source.source_id == source_id)?
        .clone();
    Some((project, source))
}

pub(super) async fn approve_bug_monitor_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<BugMonitorDecisionInput>,
) -> Response {
    match state
        .update_bug_monitor_draft_status(&id, "draft_ready", input.reason.as_deref())
        .await
    {
        Ok(draft) => {
            let had_triage_run = draft.triage_run_id.is_some();
            let approved_draft = if draft.triage_run_id.is_none() {
                ensure_bug_monitor_triage_run(state.clone(), &draft.draft_id, true)
                    .await
                    .map(|(draft, _, _)| draft)
                    .unwrap_or(draft)
            } else {
                draft
            };
            let approval_failure_pattern_memory = if !had_triage_run {
                persist_bug_monitor_failure_pattern_from_approved_draft(&state, &approved_draft)
                    .await
                    .ok()
            } else {
                None
            };
            let _ =
                ensure_bug_monitor_approval_triage_summary_artifact(&state, &approved_draft).await;
            let issue_draft =
                ensure_bug_monitor_issue_draft(state.clone(), &approved_draft.draft_id, true)
                    .await
                    .ok();
            let (duplicate_summary, duplicate_matches) = bug_monitor_duplicate_match_context(
                &state,
                approved_draft.triage_run_id.as_deref(),
            )
            .await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                bug_monitor_triage_artifacts(&state, approved_draft.triage_run_id.as_deref());
            match crate::bug_monitor::router::publish_draft(
                &state,
                crate::bug_monitor::router::BugMonitorPublishRequest {
                    draft_id: approved_draft.draft_id.clone(),
                    incident_id: None,
                    mode: bug_monitor_github::PublishMode::Auto,
                    destination_ids: Vec::new(),
                },
            )
            .await
            {
                Ok(outcome) => {
                    let external_action = match outcome.post.as_ref() {
                        Some(post) => state.get_external_action(&post.post_id).await,
                        None => None,
                    };
                    Json(json!({
                        "ok": true,
                        "draft": outcome.draft,
                        "action": outcome.action,
                        "failure_pattern_memory": approval_failure_pattern_memory,
                        "issue_draft": issue_draft,
                        "duplicate_summary": duplicate_summary,
                        "duplicate_matches": duplicate_matches,
                        "triage_summary_artifact": triage_summary_artifact,
                        "issue_draft_artifact": issue_draft_artifact,
                        "duplicate_matches_artifact": duplicate_matches_artifact,
                        "post": outcome.post,
                        "external_action": external_action,
                    }))
                    .into_response()
                }
                Err(error) => {
                    let detail = error.to_string();
                    let mut updated_draft = state
                        .get_bug_monitor_draft(&approved_draft.draft_id)
                        .await
                        .unwrap_or(approved_draft);
                    updated_draft.last_post_error = Some(detail.clone());
                    updated_draft
                        .github_status
                        .get_or_insert_with(|| "publish_blocked".to_string());
                    let updated_draft = state
                        .put_bug_monitor_draft(updated_draft.clone())
                        .await
                        .unwrap_or(updated_draft);
                    Json(json!({
                        "ok": true,
                        "draft": updated_draft,
                        "action": "approved",
                        "failure_pattern_memory": approval_failure_pattern_memory,
                        "issue_draft": issue_draft,
                        "duplicate_summary": duplicate_summary,
                        "duplicate_matches": duplicate_matches,
                        "triage_summary_artifact": triage_summary_artifact,
                        "issue_draft_artifact": issue_draft_artifact,
                        "duplicate_matches_artifact": duplicate_matches_artifact,
                        "publish_error": detail,
                    }))
                    .into_response()
                }
            }
        }
        Err(error) => map_bug_monitor_draft_update_error(id, error).into_response(),
    }
}

pub(super) async fn draft_bug_monitor_issue(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match ensure_bug_monitor_issue_draft(state.clone(), &id, true).await {
        Ok(issue_draft) => {
            let triage_run_id = issue_draft.get("triage_run_id").and_then(Value::as_str);
            let draft = state.get_bug_monitor_draft(&id).await;
            let triage_summary = triage_run_id.map(|run_id| async {
                load_bug_monitor_triage_summary_artifact(&state, run_id).await
            });
            let (duplicate_summary, duplicate_matches) =
                bug_monitor_duplicate_match_context(&state, triage_run_id).await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                bug_monitor_triage_artifacts(&state, triage_run_id);
            let triage_summary = match triage_summary {
                Some(loader) => loader.await,
                None => None,
            };
            Json(json!({
                "ok": true,
                "draft": draft,
                "triage_summary": triage_summary,
                "issue_draft": issue_draft,
                "duplicate_summary": duplicate_summary,
                "duplicate_matches": duplicate_matches,
                "triage_summary_artifact": triage_summary_artifact,
                "issue_draft_artifact": issue_draft_artifact,
                "duplicate_matches_artifact": duplicate_matches_artifact,
            }))
            .into_response()
        }
        Err(error) => (StatusCode::BAD_REQUEST, {
            let draft = state.get_bug_monitor_draft(&id).await;
            let triage_run_id = draft.as_ref().and_then(|row| row.triage_run_id.clone());
            let proposal_quality_gate = match triage_run_id.as_deref() {
                Some(run_id) => {
                    load_bug_monitor_proposal_quality_gate_artifact(&state, run_id).await
                }
                None => None,
            };
            let proposal_quality_gate_artifact = triage_run_id.as_deref().and_then(|run_id| {
                latest_bug_monitor_artifact(&state, run_id, "bug_monitor_proposal_quality_gate")
            });
            Json(json!({
                "error": "Failed to generate Bug Monitor issue draft",
                "code": "BUG_MONITOR_ISSUE_DRAFT_FAILED",
                "draft_id": id,
                "draft": draft,
                "proposal_quality_gate": proposal_quality_gate,
                "proposal_quality_gate_artifact": proposal_quality_gate_artifact,
                "detail": error.to_string(),
            }))
        })
            .into_response(),
    }
}

pub(super) async fn deny_bug_monitor_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<BugMonitorDecisionInput>,
) -> Response {
    match state
        .update_bug_monitor_draft_status(&id, "denied", input.reason.as_deref())
        .await
    {
        Ok(draft) => Json(json!({ "ok": true, "draft": draft })).into_response(),
        Err(error) => map_bug_monitor_draft_update_error(id, error).into_response(),
    }
}

pub(super) async fn create_bug_monitor_triage_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match ensure_bug_monitor_triage_run(state.clone(), &id, false).await {
        Ok((draft, run_id, deduped)) => {
            let triage_run_id = draft.triage_run_id.as_deref().unwrap_or(run_id.as_str());
            let run = if let Some(automation_run_id) =
                bug_monitor_automation_run_id_from_triage_run_id(triage_run_id)
            {
                state
                    .get_automation_v2_run(&automation_run_id)
                    .await
                    .and_then(|run| serde_json::to_value(run).ok())
                    .map(|mut run| {
                        if let Some(object) = run.as_object_mut() {
                            object.insert(
                                "automation_run_id".to_string(),
                                Value::String(automation_run_id),
                            );
                            object.insert(
                                "run_id".to_string(),
                                Value::String(triage_run_id.to_string()),
                            );
                        }
                        run
                    })
            } else {
                load_context_run_state(&state, triage_run_id)
                    .await
                    .ok()
                    .and_then(|run| serde_json::to_value(run).ok())
            };
            let triage_summary =
                load_bug_monitor_triage_summary_artifact(&state, triage_run_id).await;
            let issue_draft = ensure_bug_monitor_issue_draft(state.clone(), &id, true)
                .await
                .ok();
            let (duplicate_summary, duplicate_matches) =
                bug_monitor_duplicate_match_context(&state, Some(triage_run_id)).await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                bug_monitor_triage_artifacts(&state, Some(triage_run_id));
            Json(json!({
                "ok": true,
                "draft": draft,
                "run": run,
                "deduped": deduped,
                "triage_summary": triage_summary,
                "triage_summary_artifact": triage_summary_artifact,
                "issue_draft": issue_draft,
                "issue_draft_artifact": issue_draft_artifact,
                "duplicate_summary": duplicate_summary,
                "duplicate_matches": duplicate_matches,
                "duplicate_matches_artifact": duplicate_matches_artifact,
            }))
            .into_response()
        }
        Err(error) => {
            let detail = error.to_string();
            let status = if detail.contains("not found") {
                StatusCode::NOT_FOUND
            } else if detail.contains("approved") || detail.contains("Denied") {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_REQUEST
            };
            (
                status,
                Json(json!({
                    "error": "Failed to create Bug Monitor triage run",
                    "code": "BUG_MONITOR_TRIAGE_RUN_CREATE_FAILED",
                    "draft_id": id,
                    "detail": detail,
                })),
            )
                .into_response()
        }
    }
}
