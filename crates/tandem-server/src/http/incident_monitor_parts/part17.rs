// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

pub(super) async fn approve_incident_monitor_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<IncidentMonitorDecisionInput>,
) -> Response {
    match state
        .update_incident_monitor_draft_status(&id, "draft_ready", input.reason.as_deref())
        .await
    {
        Ok(draft) => {
            let had_triage_run = draft.triage_run_id.is_some();
            let approved_draft = if draft.triage_run_id.is_none() {
                ensure_incident_monitor_triage_run(state.clone(), &draft.draft_id, true)
                    .await
                    .map(|(draft, _, _)| draft)
                    .unwrap_or(draft)
            } else {
                draft
            };
            let approval_failure_pattern_memory = if !had_triage_run {
                persist_incident_monitor_failure_pattern_from_approved_draft(&state, &approved_draft)
                    .await
                    .ok()
            } else {
                None
            };
            let _ =
                ensure_incident_monitor_approval_triage_summary_artifact(&state, &approved_draft).await;
            let issue_draft =
                ensure_incident_monitor_issue_draft(state.clone(), &approved_draft.draft_id, true)
                    .await
                    .ok();
            let (duplicate_summary, duplicate_matches) = incident_monitor_duplicate_match_context(
                &state,
                approved_draft.triage_run_id.as_deref(),
            )
            .await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                incident_monitor_triage_artifacts(&state, approved_draft.triage_run_id.as_deref());
            match crate::incident_monitor::router::publish_draft(
                &state,
                crate::incident_monitor::router::IncidentMonitorPublishRequest {
                    draft_id: approved_draft.draft_id.clone(),
                    incident_id: None,
                    mode: incident_monitor_github::PublishMode::Auto,
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
                        .get_incident_monitor_draft(&approved_draft.draft_id)
                        .await
                        .unwrap_or(approved_draft);
                    updated_draft.last_post_error = Some(detail.clone());
                    updated_draft
                        .github_status
                        .get_or_insert_with(|| "publish_blocked".to_string());
                    let updated_draft = state
                        .put_incident_monitor_draft(updated_draft.clone())
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
        Err(error) => map_incident_monitor_draft_update_error(id, error).into_response(),
    }
}

pub(super) async fn draft_incident_monitor_issue(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match ensure_incident_monitor_issue_draft(state.clone(), &id, true).await {
        Ok(issue_draft) => {
            let triage_run_id = issue_draft.get("triage_run_id").and_then(Value::as_str);
            let draft = state.get_incident_monitor_draft(&id).await;
            let triage_summary = triage_run_id.map(|run_id| async {
                load_incident_monitor_triage_summary_artifact(&state, run_id).await
            });
            let (duplicate_summary, duplicate_matches) =
                incident_monitor_duplicate_match_context(&state, triage_run_id).await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                incident_monitor_triage_artifacts(&state, triage_run_id);
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
            let draft = state.get_incident_monitor_draft(&id).await;
            let triage_run_id = draft.as_ref().and_then(|row| row.triage_run_id.clone());
            let proposal_quality_gate = match triage_run_id.as_deref() {
                Some(run_id) => {
                    load_incident_monitor_proposal_quality_gate_artifact(&state, run_id).await
                }
                None => None,
            };
            let proposal_quality_gate_artifact = triage_run_id.as_deref().and_then(|run_id| {
                latest_incident_monitor_artifact(&state, run_id, "incident_monitor_proposal_quality_gate")
            });
            Json(json!({
                "error": "Failed to generate Incident Monitor issue draft",
                "code": "INCIDENT_MONITOR_ISSUE_DRAFT_FAILED",
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

pub(super) async fn deny_incident_monitor_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<IncidentMonitorDecisionInput>,
) -> Response {
    match state
        .update_incident_monitor_draft_status(&id, "denied", input.reason.as_deref())
        .await
    {
        Ok(draft) => Json(json!({ "ok": true, "draft": draft })).into_response(),
        Err(error) => map_incident_monitor_draft_update_error(id, error).into_response(),
    }
}

pub(super) async fn create_incident_monitor_triage_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match ensure_incident_monitor_triage_run(state.clone(), &id, false).await {
        Ok((draft, run_id, deduped)) => {
            let triage_run_id = draft.triage_run_id.as_deref().unwrap_or(run_id.as_str());
            let run = if let Some(automation_run_id) =
                incident_monitor_automation_run_id_from_triage_run_id(triage_run_id)
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
                load_incident_monitor_triage_summary_artifact(&state, triage_run_id).await;
            let issue_draft = ensure_incident_monitor_issue_draft(state.clone(), &id, true)
                .await
                .ok();
            let (duplicate_summary, duplicate_matches) =
                incident_monitor_duplicate_match_context(&state, Some(triage_run_id)).await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                incident_monitor_triage_artifacts(&state, Some(triage_run_id));
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
                    "error": "Failed to create Incident Monitor triage run",
                    "code": "INCIDENT_MONITOR_TRIAGE_RUN_CREATE_FAILED",
                    "draft_id": id,
                    "detail": detail,
                })),
            )
                .into_response()
        }
    }
}
