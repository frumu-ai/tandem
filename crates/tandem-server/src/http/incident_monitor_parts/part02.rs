// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

pub(super) async fn publish_incident_monitor_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
    payload: Option<Json<IncidentMonitorPublishInput>>,
) -> Response {
    let existing_draft = state.get_incident_monitor_draft(&id).await;
    let destination_ids = incident_monitor_publish_destination_ids(payload.map(|Json(input)| input));
    match crate::incident_monitor::router::publish_draft(
        &state,
        crate::incident_monitor::router::IncidentMonitorPublishRequest {
            draft_id: id.clone(),
            incident_id: None,
            mode: incident_monitor_github::PublishMode::ManualPublish,
            destination_ids,
        },
    )
    .await
    {
        Ok(outcome) => {
            let triage_summary =
                outcome
                    .draft
                    .triage_run_id
                    .as_deref()
                    .map(|triage_run_id| async {
                        load_incident_monitor_triage_summary_artifact(&state, triage_run_id).await
                    });
            let issue_draft = if outcome.draft.triage_run_id.is_some() {
                ensure_incident_monitor_issue_draft(state.clone(), &outcome.draft.draft_id, false)
                    .await
                    .ok()
            } else {
                None
            };
            let (duplicate_summary, duplicate_matches) =
                incident_monitor_duplicate_match_context(&state, outcome.draft.triage_run_id.as_deref())
                    .await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                incident_monitor_triage_artifacts(&state, outcome.draft.triage_run_id.as_deref());
            let triage_summary = match triage_summary {
                Some(loader) => loader.await,
                None => None,
            };
            let external_action = match outcome.post.as_ref() {
                Some(post) => state.get_external_action(&post.post_id).await,
                None => None,
            };
            let post_id = outcome.post.as_ref().map(|post| post.post_id.clone());
            let issue_number = outcome
                .post
                .as_ref()
                .and_then(|post| post.issue_number)
                .or(outcome.draft.issue_number)
                .or(outcome.draft.matched_issue_number);
            let issue_url = outcome
                .post
                .as_ref()
                .and_then(|post| post.issue_url.clone())
                .or_else(|| outcome.draft.github_issue_url.clone());
            let comment_id = outcome
                .post
                .as_ref()
                .and_then(|post| post.comment_id.clone());
            let comment_url = outcome
                .post
                .as_ref()
                .and_then(|post| post.comment_url.clone());
            Json(json!({
                "ok": true,
                "draft": outcome.draft,
                "action": outcome.action,
                "triage_summary": triage_summary,
                "issue_draft": issue_draft,
                "duplicate_summary": duplicate_summary,
                "duplicate_matches": duplicate_matches,
                "triage_summary_artifact": triage_summary_artifact,
                "issue_draft_artifact": issue_draft_artifact,
                "duplicate_matches_artifact": duplicate_matches_artifact,
                "post_id": post_id,
                "issue_number": issue_number,
                "issue_url": issue_url,
                "comment_id": comment_id,
                "comment_url": comment_url,
                "post": outcome.post,
                "external_action": external_action,
            }))
            .into_response()
        }
        Err(error) => {
            let draft = state.get_incident_monitor_draft(&id).await.or(existing_draft);
            let triage_summary = if let Some(triage_run_id) =
                draft.as_ref().and_then(|row| row.triage_run_id.as_deref())
            {
                load_incident_monitor_triage_summary_artifact(&state, triage_run_id).await
            } else {
                None
            };
            let issue_draft = if draft
                .as_ref()
                .and_then(|row| row.triage_run_id.as_ref())
                .is_some()
            {
                ensure_incident_monitor_issue_draft(state.clone(), &id, false)
                    .await
                    .ok()
            } else {
                None
            };
            let (duplicate_summary, duplicate_matches) = incident_monitor_duplicate_match_context(
                &state,
                draft.as_ref().and_then(|row| row.triage_run_id.as_deref()),
            )
            .await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                incident_monitor_triage_artifacts(
                    &state,
                    draft.as_ref().and_then(|row| row.triage_run_id.as_deref()),
                );
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Failed to publish Incident Monitor draft to destination",
                    "code": "INCIDENT_MONITOR_DRAFT_PUBLISH_FAILED",
                    "draft_id": id,
                    "draft": draft,
                    "triage_summary": triage_summary,
                    "issue_draft": issue_draft,
                    "duplicate_summary": duplicate_summary,
                    "duplicate_matches": duplicate_matches,
                    "triage_summary_artifact": triage_summary_artifact,
                    "issue_draft_artifact": issue_draft_artifact,
                    "duplicate_matches_artifact": duplicate_matches_artifact,
                    "detail": format!("{error:#}"),
                })),
            )
                .into_response()
        }
    }
}

fn incident_monitor_publish_destination_ids(input: Option<IncidentMonitorPublishInput>) -> Vec<String> {
    let Some(input) = input else {
        return Vec::new();
    };
    let mut destination_ids = input
        .destination_ids
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if let Some(destination_id) = input
        .destination_id
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    {
        if !destination_ids
            .iter()
            .any(|existing| existing == &destination_id)
        {
            destination_ids.insert(0, destination_id);
        }
    }
    destination_ids
}

pub(super) async fn recheck_incident_monitor_draft_match(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let existing_draft = state.get_incident_monitor_draft(&id).await;
    match crate::incident_monitor::router::publish_draft(
        &state,
        crate::incident_monitor::router::IncidentMonitorPublishRequest {
            draft_id: id.clone(),
            incident_id: None,
            mode: incident_monitor_github::PublishMode::RecheckOnly,
            destination_ids: Vec::new(),
        },
    )
    .await
    {
        Ok(outcome) => {
            let triage_summary =
                outcome
                    .draft
                    .triage_run_id
                    .as_deref()
                    .map(|triage_run_id| async {
                        load_incident_monitor_triage_summary_artifact(&state, triage_run_id).await
                    });
            let issue_draft = if outcome.draft.triage_run_id.is_some() {
                ensure_incident_monitor_issue_draft(state.clone(), &outcome.draft.draft_id, false)
                    .await
                    .ok()
            } else {
                None
            };
            let (duplicate_summary, duplicate_matches) =
                incident_monitor_duplicate_match_context(&state, outcome.draft.triage_run_id.as_deref())
                    .await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                incident_monitor_triage_artifacts(&state, outcome.draft.triage_run_id.as_deref());
            let triage_summary = match triage_summary {
                Some(loader) => loader.await,
                None => None,
            };
            let post_id = outcome.post.as_ref().map(|post| post.post_id.clone());
            let issue_number = outcome
                .post
                .as_ref()
                .and_then(|post| post.issue_number)
                .or(outcome.draft.issue_number)
                .or(outcome.draft.matched_issue_number);
            let issue_url = outcome
                .post
                .as_ref()
                .and_then(|post| post.issue_url.clone())
                .or_else(|| outcome.draft.github_issue_url.clone());
            let comment_id = outcome
                .post
                .as_ref()
                .and_then(|post| post.comment_id.clone());
            let comment_url = outcome
                .post
                .as_ref()
                .and_then(|post| post.comment_url.clone());
            Json(json!({
                "ok": true,
                "draft": outcome.draft,
                "action": outcome.action,
                "triage_summary": triage_summary,
                "issue_draft": issue_draft,
                "duplicate_summary": duplicate_summary,
                "duplicate_matches": duplicate_matches,
                "triage_summary_artifact": triage_summary_artifact,
                "issue_draft_artifact": issue_draft_artifact,
                "duplicate_matches_artifact": duplicate_matches_artifact,
                "post_id": post_id,
                "issue_number": issue_number,
                "issue_url": issue_url,
                "comment_id": comment_id,
                "comment_url": comment_url,
                "post": outcome.post,
            }))
            .into_response()
        }
        Err(error) => {
            let draft = state.get_incident_monitor_draft(&id).await.or(existing_draft);
            let triage_summary = if let Some(triage_run_id) =
                draft.as_ref().and_then(|row| row.triage_run_id.as_deref())
            {
                load_incident_monitor_triage_summary_artifact(&state, triage_run_id).await
            } else {
                None
            };
            let issue_draft = if draft
                .as_ref()
                .and_then(|row| row.triage_run_id.as_ref())
                .is_some()
            {
                ensure_incident_monitor_issue_draft(state.clone(), &id, false)
                    .await
                    .ok()
            } else {
                None
            };
            let (duplicate_summary, duplicate_matches) = incident_monitor_duplicate_match_context(
                &state,
                draft.as_ref().and_then(|row| row.triage_run_id.as_deref()),
            )
            .await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                incident_monitor_triage_artifacts(
                    &state,
                    draft.as_ref().and_then(|row| row.triage_run_id.as_deref()),
                );
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Failed to recheck Incident Monitor draft against destination",
                    "code": "INCIDENT_MONITOR_DRAFT_RECHECK_FAILED",
                    "draft_id": id,
                    "draft": draft,
                    "triage_summary": triage_summary,
                    "issue_draft": issue_draft,
                    "duplicate_summary": duplicate_summary,
                    "duplicate_matches": duplicate_matches,
                    "triage_summary_artifact": triage_summary_artifact,
                    "issue_draft_artifact": issue_draft_artifact,
                    "duplicate_matches_artifact": duplicate_matches_artifact,
                    "detail": format!("{error:#}"),
                })),
            )
                .into_response()
        }
    }
}

pub(crate) async fn ensure_incident_monitor_triage_run(
    state: AppState,
    id: &str,
    bypass_approval_gate: bool,
) -> anyhow::Result<(IncidentMonitorDraftRecord, String, bool)> {
    let config = state.incident_monitor_config().await;
    let draft = state
        .get_incident_monitor_draft(id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Incident Monitor draft not found"))?;

    if draft.status.eq_ignore_ascii_case("denied") {
        anyhow::bail!("Denied Incident Monitor drafts cannot create triage runs");
    }
    if !bypass_approval_gate
        && config.require_approval_for_new_issues
        && draft.status.eq_ignore_ascii_case("approval_required")
    {
        anyhow::bail!("Incident Monitor draft must be approved before triage run creation");
    }

    if let Some(existing_run_id) = draft.triage_run_id.clone() {
        if let Some(automation_run_id) =
            incident_monitor_automation_run_id_from_triage_run_id(&existing_run_id)
        {
            if let Some(run) = state.get_automation_v2_run(&automation_run_id).await {
                let stale_contract = if let Some(automation) = run.automation_snapshot.as_ref() {
                    incident_monitor_triage_flow_has_stale_output_contracts(&automation.flow)
                } else if let Some(automation) = state.get_automation_v2(&run.automation_id).await {
                    incident_monitor_triage_flow_has_stale_output_contracts(&automation.flow)
                } else {
                    false
                };
                if stale_contract {
                    tracing::warn!(
                        draft_id = %draft.draft_id,
                        triage_run_id = %existing_run_id,
                        run_id = %automation_run_id,
                        "Incident Monitor triage run has stale output contracts; recreating run",
                    );
                } else if incident_monitor_triage_run_is_reusable(&state, &existing_run_id).await {
                    return Ok((draft, existing_run_id, true));
                }
            }
        } else if incident_monitor_triage_run_is_reusable(&state, &existing_run_id).await {
            return Ok((draft, existing_run_id, true));
        }
    }

    let duplicate_matches = super::coder::query_failure_pattern_matches(
        &state,
        &draft.repo,
        &draft.fingerprint,
        draft.title.as_deref(),
        draft.detail.as_deref(),
        &[],
        3,
        // Incident monitor is local/system-scoped (see local_implicit tenant).
        None,
    )
    .await
    .map_err(|status| {
        anyhow::anyhow!("Failed to query duplicate failure patterns: HTTP {status}")
    })?;
    let incident = latest_incident_monitor_incident_for_draft(&state, &draft.draft_id).await;
    let project_config = incident_monitor_project_for_incident(&config, incident.as_ref());
    let resolved_workspace_root =
        workspace_root_for_incident_monitor_triage(&config, incident.as_ref(), project_config).await;
    let model_policy = project_config
        .and_then(|project| project.model_policy.clone())
        .or_else(|| {
            incident
                .as_ref()
                .and_then(|row| row.event_payload.as_ref())
                .and_then(|payload| payload.get("model_policy").cloned())
        })
        .or_else(|| config.model_policy.clone());
    let mcp_servers = project_config
        .and_then(|project| project.mcp_server.clone())
        .or_else(|| {
            incident
                .as_ref()
                .and_then(|row| row.event_payload.as_ref())
                .and_then(|payload| payload.get("mcp_server"))
                .and_then(|value| value.as_str())
                .map(ToString::to_string)
        })
        .or_else(|| config.mcp_server.clone())
        .map(|row| vec![row])
        .filter(|row| !row.is_empty());
    let incident_payload = incident
        .as_ref()
        .and_then(|row| row.event_payload.clone())
        .unwrap_or(Value::Null);
    let workflow_run_task_ids = json!({
        "workflow_id": incident_payload.get("workflow_id").or_else(|| incident_payload.get("workflowID")).cloned().unwrap_or(Value::Null),
        "workflow_name": incident_payload.get("workflow_name").or_else(|| incident_payload.get("workflowName")).cloned().unwrap_or(Value::Null),
        "run_id": incident
            .as_ref()
            .and_then(|row| row.run_id.clone())
            .map(Value::String)
            .or_else(|| incident_payload.get("run_id").or_else(|| incident_payload.get("runID")).cloned())
            .unwrap_or(Value::Null),
        "session_id": incident
            .as_ref()
            .and_then(|row| row.session_id.clone())
            .map(Value::String)
            .or_else(|| incident_payload.get("session_id").or_else(|| incident_payload.get("sessionID")).cloned())
            .unwrap_or(Value::Null),
        "task_id": incident_payload.get("task_id").or_else(|| incident_payload.get("taskID")).cloned().unwrap_or(Value::Null),
        "stage_id": incident_payload.get("stage_id").or_else(|| incident_payload.get("stageID")).cloned().unwrap_or(Value::Null),
        "node_id": incident_payload.get("node_id").or_else(|| incident_payload.get("nodeID")).cloned().unwrap_or(Value::Null),
        "automation_id": incident_payload.get("automation_id").or_else(|| incident_payload.get("automationID")).cloned().unwrap_or(Value::Null),
        "routine_id": incident_payload.get("routine_id").or_else(|| incident_payload.get("routineID")).cloned().unwrap_or(Value::Null),
    });
    let artifact_refs = incident_payload
        .get("artifact_refs")
        .or_else(|| incident_payload.get("artifactRefs"))
        .cloned()
        .unwrap_or(Value::Array(Vec::new()));
    let files_touched = incident_payload
        .get("files_touched")
        .or_else(|| incident_payload.get("filesTouched"))
        .cloned()
        .unwrap_or(Value::Array(Vec::new()));

    // Pre-compute deterministic error-string → workspace-source hits
    // and pass them to the triage agents. This grounds the LLM in
    // real code locations and gives it a starting point for the
    // recommended-fix step instead of letting it hallucinate file
    // references from a fuzzy match on the workflow name.
    let error_provenance_payload = compute_error_provenance_payload(
        resolved_workspace_root.as_deref(),
        &draft,
        incident.as_ref(),
    )
    .await;

    let inspection_payload = json!({
        "task_kind": "inspection",
        "title": "Inspect failure report and affected area",
        "draft_id": draft.draft_id,
        "repo": draft.repo,
        "workspace_root": resolved_workspace_root,
        "summary": draft.title,
        "detail": draft.detail,
        "duplicate_matches": duplicate_matches,
        "incident": incident,
        "incident_payload": incident_payload,
        "artifact_refs": artifact_refs,
        "files_touched": files_touched,
        "workflow_run_task_ids": workflow_run_task_ids,
        "error_provenance": error_provenance_payload,
        "expected_artifact": "incident_monitor_inspection",
    });
    let research_payload = json!({
        "task_kind": "research",
        "title": "Research likely root cause and related failures",
        "draft_id": draft.draft_id,
        "repo": draft.repo,
        "workspace_root": resolved_workspace_root,
        "depends_on": "inspect_failure_report",
        "research_requirements": {
            "search_repo": true,
            "search_failure_memory": true,
            "search_github_issues": true,
            "inspect_artifacts": true,
            "web_research_when_external_error": true,
            "first_step": "Before any other research, treat the literal error string in `error_provenance.error_message` as the primary anchor. If `error_provenance.hints` is non-empty, read those files at the indicated lines first; they are the deterministic emission sites for this exact failure message. Only after reading those files should you grep more broadly for related code paths. Do not list files in `Files likely involved` unless you have read them and confirmed they reference the failure path."
        },
        "duplicate_matches": duplicate_matches,
        "artifact_refs": artifact_refs,
        "files_touched": files_touched,
        "error_provenance": error_provenance_payload,
        "expected_artifact": "incident_monitor_research_report",
    });
    let validation_payload = json!({
        "task_kind": "validation",
        "title": "Validate or reproduce failure scope",
        "draft_id": draft.draft_id,
        "repo": draft.repo,
        "workspace_root": resolved_workspace_root,
        "depends_on": "research_likely_root_cause",
        "validation_requirements": {
            "confirm_failure_scope": true,
            "classify_failure_type": true,
            "avoid_destructive_actions": true,
            "produce_evidence": true
        },
        "expected_artifact": "incident_monitor_validation",
    });
    let fix_payload = json!({
        "task_kind": "fix_proposal",
        "title": "Propose fix and verification plan",
        "draft_id": draft.draft_id,
        "repo": draft.repo,
        "workspace_root": resolved_workspace_root,
        "depends_on": "validate_failure_scope",
        "proposal_requirements": {
            "suspected_root_cause": true,
            "likely_files_to_edit": true,
            "recommended_fix": true,
            "acceptance_criteria": true,
            "smoke_test_steps": true,
            "coder_ready_assessment": true,
            "suggested_labels": true,
            "risk_level": true
        },
        "expected_artifact": "incident_monitor_fix_proposal",
    });
    let triage_spec = incident_monitor_triage_spec(
        &draft,
        resolved_workspace_root.clone(),
        model_policy,
        mcp_servers.unwrap_or_default(),
        inspection_payload.clone(),
        research_payload.clone(),
        validation_payload.clone(),
        fix_payload.clone(),
    );
    let mut triage_spec = triage_spec;
    normalize_incident_monitor_triage_output_contracts(&mut triage_spec);
    let stored_spec = state.put_automation_v2(triage_spec).await?;
    let automation_run = state
        .create_automation_v2_run(&stored_spec, "incident_monitor_triage")
        .await?;
    let run_id = incident_monitor_triage_context_run_id(&automation_run.run_id);

    if !duplicate_matches.is_empty() {
        write_incident_monitor_artifact(
            &state,
            &run_id,
            "failure-duplicate-matches",
            "failure_duplicate_matches",
            "artifacts/failure_duplicate_matches.json",
            &json!({
                "draft_id": draft.draft_id,
                "repo": draft.repo,
                "fingerprint": draft.fingerprint,
                "matches": duplicate_matches,
                "created_at_ms": crate::now_ms(),
            }),
        )
        .await
        .map_err(|status| {
            anyhow::anyhow!("Failed to write duplicate matches artifact: HTTP {status}")
        })?;
    }

    for (artifact_id, artifact_type, path, payload) in [
        (
            "incident-monitor-inspection-brief",
            "incident_monitor_inspection_task_spec",
            "artifacts/incident_monitor.inspection.task_spec.json",
            inspection_payload,
        ),
        (
            "incident-monitor-research-brief",
            "incident_monitor_research_task_spec",
            "artifacts/incident_monitor.research.task_spec.json",
            research_payload,
        ),
        (
            "incident-monitor-validation-brief",
            "incident_monitor_validation_task_spec",
            "artifacts/incident_monitor.validation.task_spec.json",
            validation_payload,
        ),
        (
            "incident-monitor-fix-proposal-brief",
            "incident_monitor_fix_proposal_task_spec",
            "artifacts/incident_monitor.fix_proposal.task_spec.json",
            fix_payload,
        ),
    ] {
        write_incident_monitor_artifact(&state, &run_id, artifact_id, artifact_type, path, &payload)
            .await
            .map_err(|status| anyhow::anyhow!("Failed to write triage artifact: HTTP {status}"))?;
    }
    let mut updated_draft = draft.clone();
    updated_draft.triage_run_id = Some(run_id.clone());
    updated_draft.status = "triage_queued".to_string();
    {
        let mut drafts = state.incident_monitor_drafts.write().await;
        drafts.insert(updated_draft.draft_id.clone(), updated_draft.clone());
    }
    state.persist_incident_monitor_drafts().await?;

    ensure_context_run_dir(&state, &run_id)
        .await
        .map_err(|status| {
            anyhow::anyhow!("Failed to finalize triage run workspace: HTTP {status}")
        })?;
    if let Ok(mut context_run) = load_context_run_state(&state, &run_id).await {
        context_run.run_type = "incident_monitor_triage".to_string();
        context_run.source_client = Some("incident_monitor_triage".to_string());
        save_context_run_state(&state, &context_run)
            .await
            .map_err(|status| {
                anyhow::anyhow!("Failed to persist triage run context metadata: HTTP {status}")
            })?;
    }
    state.event_bus.publish(tandem_types::EngineEvent::new(
        "incident_monitor.triage_run.created",
        json!({
            "draft_id": updated_draft.draft_id,
            "run_id": run_id,
            "automation_run_id": automation_run.run_id,
            "repo": updated_draft.repo,
        }),
    ));

    Ok((updated_draft, run_id, false))
}

/// Run the deterministic error-string → workspace grep at triage
/// kickoff and shape the result for the LLM agents. Returns a JSON
/// object with `error_message`, `hints` (array of {path, line,
/// snippet}), and an instructional `note`. Prefers
/// `config.workspace_root` and falls back to `incident.workspace_root`
/// (which the submission builder populates from the workspace index
/// when no explicit config root is set). When neither is accessible
/// or no error message can be picked, returns JSON null so payload
/// consumers can `is_null()` cheaply.
fn incident_monitor_project_for_incident<'a>(
    config: &'a IncidentMonitorConfig,
    incident: Option<&crate::IncidentMonitorIncidentRecord>,
) -> Option<&'a crate::IncidentMonitorMonitoredProject> {
    let project_id = incident
        .and_then(|row| row.event_payload.as_ref())
        .and_then(|payload| payload.get("project_id"))
        .and_then(|value| value.as_str())?;
    config
        .monitored_projects
        .iter()
        .find(|project| project.project_id == project_id)
}

async fn workspace_root_for_incident_monitor_triage(
    config: &IncidentMonitorConfig,
    incident: Option<&crate::IncidentMonitorIncidentRecord>,
    project: Option<&crate::IncidentMonitorMonitoredProject>,
) -> Option<String> {
    let candidates = [
        incident.map(|row| row.workspace_root.clone()),
        project.map(|row| row.workspace_root.clone()),
        incident
            .and_then(|row| row.event_payload.as_ref())
            .and_then(|payload| payload.get("workspace_root"))
            .and_then(|value| value.as_str())
            .map(ToString::to_string),
        config.workspace_root.clone(),
    ];
    candidates
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_string())
        .find(|value| {
            let path = std::path::Path::new(value);
            path.is_absolute() && path.exists()
        })
}

async fn compute_error_provenance_payload(
    config_workspace_root: Option<&str>,
    draft: &IncidentMonitorDraftRecord,
    incident: Option<&crate::IncidentMonitorIncidentRecord>,
) -> serde_json::Value {
    let Some(error_message) = pick_error_message_for_triage(draft, incident) else {
        return serde_json::Value::Null;
    };
    let Some(workspace_root) = pick_workspace_root_for_triage(config_workspace_root, incident)
    else {
        return serde_json::json!({
            "error_message": error_message,
            "hints": [],
            "note": "workspace_root not configured; LLM should grep the workspace itself for `error_message`."
        });
    };
    let path = std::path::Path::new(&workspace_root);
    if !path.is_absolute() || !path.exists() {
        return serde_json::json!({
            "error_message": error_message,
            "hints": [],
            "note": "workspace_root not accessible; LLM should grep the workspace itself for `error_message`."
        });
    }
    let hits =
        crate::incident_monitor::error_provenance::locate_error_provenance(path, &error_message).await;
    let hints = hits
        .into_iter()
        .map(|hit| {
            json!({
                "path": hit.path,
                "line": hit.line,
                "snippet": hit.snippet,
            })
        })
        .collect::<Vec<_>>();
    let note = if hints.is_empty() {
        "No exact match for `error_message` in tracked source files. The LLM should grep more broadly (e.g. for fragments of the message) before listing any files in `Files likely involved`.".to_string()
    } else {
        "`hints` are deterministic, server-side grep results for the literal `error_message`. Read these files at the indicated lines first; they are the emission sites for this exact failure. Use them as the anchor for `Files likely involved` rather than fuzzy-matching on the workflow name.".to_string()
    };
    json!({
        "error_message": error_message,
        "hints": hints,
        "note": note,
    })
}

fn pick_workspace_root_for_triage(
    config_workspace_root: Option<&str>,
    incident: Option<&crate::IncidentMonitorIncidentRecord>,
) -> Option<String> {
    let candidates = [
        incident.map(|row| row.workspace_root.clone()),
        config_workspace_root.map(str::to_string),
    ];
    candidates
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}

fn pick_error_message_for_triage(
    draft: &IncidentMonitorDraftRecord,
    incident: Option<&crate::IncidentMonitorIncidentRecord>,
) -> Option<String> {
    // Mirror pick_error_message_for_provenance in incident_monitor_github:
    // prefer fields populated at incident/draft creation. Avoid
    // last_error because the triage deadline path rewrites it with
    // the multi-line diagnostics, which would poison any future
    // provenance lookup (and prompt grounding) on that draft.
    let candidates = [
        incident.and_then(|row| {
            row.excerpt
                .iter()
                .find(|line| !line.trim().is_empty())
                .cloned()
        }),
        draft.detail.clone(),
        incident.and_then(|row| row.detail.clone()),
        incident.and_then(|row| {
            row.title
                .split_once(':')
                .map(|(_, suffix)| suffix.trim().to_string())
                .filter(|s| !s.is_empty() && s.split_whitespace().count() >= 3)
        }),
        incident.map(|row| row.title.clone()),
        draft.title.clone(),
    ];
    candidates
        .into_iter()
        .flatten()
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
}
