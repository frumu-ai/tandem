
pub(super) async fn publish_bug_monitor_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let existing_draft = state.get_bug_monitor_draft(&id).await;
    match bug_monitor_github::publish_draft(
        &state,
        &id,
        None,
        bug_monitor_github::PublishMode::ManualPublish,
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
                        load_bug_monitor_triage_summary_artifact(&state, triage_run_id).await
                    });
            let issue_draft = if outcome.draft.triage_run_id.is_some() {
                ensure_bug_monitor_issue_draft(state.clone(), &outcome.draft.draft_id, false)
                    .await
                    .ok()
            } else {
                None
            };
            let (duplicate_summary, duplicate_matches) =
                bug_monitor_duplicate_match_context(&state, outcome.draft.triage_run_id.as_deref())
                    .await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                bug_monitor_triage_artifacts(&state, outcome.draft.triage_run_id.as_deref());
            let triage_summary = match triage_summary {
                Some(loader) => loader.await,
                None => None,
            };
            let external_action = match outcome.post.as_ref() {
                Some(post) => state.get_external_action(&post.post_id).await,
                None => None,
            };
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
                "post": outcome.post,
                "external_action": external_action,
            }))
            .into_response()
        }
        Err(error) => {
            let draft = state.get_bug_monitor_draft(&id).await.or(existing_draft);
            let triage_summary = if let Some(triage_run_id) =
                draft.as_ref().and_then(|row| row.triage_run_id.as_deref())
            {
                load_bug_monitor_triage_summary_artifact(&state, triage_run_id).await
            } else {
                None
            };
            let issue_draft = if draft
                .as_ref()
                .and_then(|row| row.triage_run_id.as_ref())
                .is_some()
            {
                ensure_bug_monitor_issue_draft(state.clone(), &id, false)
                    .await
                    .ok()
            } else {
                None
            };
            let (duplicate_summary, duplicate_matches) = bug_monitor_duplicate_match_context(
                &state,
                draft.as_ref().and_then(|row| row.triage_run_id.as_deref()),
            )
            .await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                bug_monitor_triage_artifacts(
                    &state,
                    draft.as_ref().and_then(|row| row.triage_run_id.as_deref()),
                );
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Failed to publish Bug Monitor draft to GitHub",
                    "code": "BUG_MONITOR_DRAFT_PUBLISH_FAILED",
                    "draft_id": id,
                    "draft": draft,
                    "triage_summary": triage_summary,
                    "issue_draft": issue_draft,
                    "duplicate_summary": duplicate_summary,
                    "duplicate_matches": duplicate_matches,
                    "triage_summary_artifact": triage_summary_artifact,
                    "issue_draft_artifact": issue_draft_artifact,
                    "duplicate_matches_artifact": duplicate_matches_artifact,
                    "detail": error.to_string(),
                })),
            )
                .into_response()
        }
    }
}

pub(super) async fn recheck_bug_monitor_draft_match(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let existing_draft = state.get_bug_monitor_draft(&id).await;
    match bug_monitor_github::publish_draft(
        &state,
        &id,
        None,
        bug_monitor_github::PublishMode::RecheckOnly,
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
                        load_bug_monitor_triage_summary_artifact(&state, triage_run_id).await
                    });
            let issue_draft = if outcome.draft.triage_run_id.is_some() {
                ensure_bug_monitor_issue_draft(state.clone(), &outcome.draft.draft_id, false)
                    .await
                    .ok()
            } else {
                None
            };
            let (duplicate_summary, duplicate_matches) =
                bug_monitor_duplicate_match_context(&state, outcome.draft.triage_run_id.as_deref())
                    .await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                bug_monitor_triage_artifacts(&state, outcome.draft.triage_run_id.as_deref());
            let triage_summary = match triage_summary {
                Some(loader) => loader.await,
                None => None,
            };
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
                "post": outcome.post,
            }))
            .into_response()
        }
        Err(error) => {
            let draft = state.get_bug_monitor_draft(&id).await.or(existing_draft);
            let triage_summary = if let Some(triage_run_id) =
                draft.as_ref().and_then(|row| row.triage_run_id.as_deref())
            {
                load_bug_monitor_triage_summary_artifact(&state, triage_run_id).await
            } else {
                None
            };
            let issue_draft = if draft
                .as_ref()
                .and_then(|row| row.triage_run_id.as_ref())
                .is_some()
            {
                ensure_bug_monitor_issue_draft(state.clone(), &id, false)
                    .await
                    .ok()
            } else {
                None
            };
            let (duplicate_summary, duplicate_matches) = bug_monitor_duplicate_match_context(
                &state,
                draft.as_ref().and_then(|row| row.triage_run_id.as_deref()),
            )
            .await;
            let (triage_summary_artifact, issue_draft_artifact, duplicate_matches_artifact) =
                bug_monitor_triage_artifacts(
                    &state,
                    draft.as_ref().and_then(|row| row.triage_run_id.as_deref()),
                );
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Failed to recheck Bug Monitor draft against GitHub",
                    "code": "BUG_MONITOR_DRAFT_RECHECK_FAILED",
                    "draft_id": id,
                    "draft": draft,
                    "triage_summary": triage_summary,
                    "issue_draft": issue_draft,
                    "duplicate_summary": duplicate_summary,
                    "duplicate_matches": duplicate_matches,
                    "triage_summary_artifact": triage_summary_artifact,
                    "issue_draft_artifact": issue_draft_artifact,
                    "duplicate_matches_artifact": duplicate_matches_artifact,
                    "detail": error.to_string(),
                })),
            )
                .into_response()
        }
    }
}

pub(crate) async fn ensure_bug_monitor_triage_run(
    state: AppState,
    id: &str,
    bypass_approval_gate: bool,
) -> anyhow::Result<(BugMonitorDraftRecord, String, bool)> {
    let config = state.bug_monitor_config().await;
    let draft = state
        .get_bug_monitor_draft(id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Bug Monitor draft not found"))?;

    if draft.status.eq_ignore_ascii_case("denied") {
        anyhow::bail!("Denied Bug Monitor drafts cannot create triage runs");
    }
    if !bypass_approval_gate
        && config.require_approval_for_new_issues
        && draft.status.eq_ignore_ascii_case("approval_required")
    {
        anyhow::bail!("Bug Monitor draft must be approved before triage run creation");
    }

    if let Some(existing_run_id) = draft.triage_run_id.clone() {
        match load_context_run_state(&state, &existing_run_id).await {
            Ok(_) => return Ok((draft, existing_run_id, true)),
            Err(_) => {}
        }
    }

    let run_id = format!("failure-triage-{}", Uuid::new_v4().simple());
    let objective = format!(
        "Triage bug monitor draft {} for {}: {}",
        draft.draft_id,
        draft.repo,
        draft
            .title
            .clone()
            .unwrap_or_else(|| "Untitled failure".to_string())
    );
    let workspace = config
        .workspace_root
        .as_ref()
        .map(|root| ContextWorkspaceLease {
            workspace_id: root.clone(),
            canonical_path: root.clone(),
            lease_epoch: crate::now_ms(),
        });
    let model_provider = config
        .model_policy
        .as_ref()
        .and_then(|policy| policy.get("default_model"))
        .and_then(|row| row.get("provider_id"))
        .and_then(|row| row.as_str())
        .map(|row| row.trim().to_string())
        .filter(|row| !row.is_empty());
    let model_id = config
        .model_policy
        .as_ref()
        .and_then(|policy| policy.get("default_model"))
        .and_then(|row| row.get("model_id"))
        .and_then(|row| row.as_str())
        .map(|row| row.trim().to_string())
        .filter(|row| !row.is_empty());
    let mcp_servers = config
        .mcp_server
        .as_ref()
        .map(|row| vec![row.clone()])
        .filter(|row| !row.is_empty());

    let duplicate_matches = super::coder::query_failure_pattern_matches(
        &state,
        &draft.repo,
        &draft.fingerprint,
        draft.title.as_deref(),
        draft.detail.as_deref(),
        &[],
        3,
    )
    .await
    .map_err(|status| {
        anyhow::anyhow!("Failed to query duplicate failure patterns: HTTP {status}")
    })?;

    let create_input = ContextRunCreateInput {
        run_id: Some(run_id.clone()),
        objective,
        run_type: Some("bug_monitor_triage".to_string()),
        workspace,
        source_client: Some("bug_monitor".to_string()),
        model_provider,
        model_id,
        mcp_servers,
    };
    let created_run = match super::context_runs::context_run_create_impl(
        state.clone(),
        tandem_types::TenantContext::local_implicit(),
        create_input,
    )
    .await
    {
        Ok(Json(payload)) => match serde_json::from_value::<ContextRunState>(
            payload.get("run").cloned().unwrap_or_default(),
        ) {
            Ok(run) => run,
            Err(_) => anyhow::bail!("Failed to deserialize triage context run"),
        },
        Err(status) => anyhow::bail!("Failed to create triage context run: HTTP {status}"),
    };

    let inspect_task_id = format!("triage-inspect-{}", Uuid::new_v4().simple());
    let validate_task_id = format!("triage-validate-{}", Uuid::new_v4().simple());
    let tasks_input = ContextTaskCreateBatchInput {
        tasks: vec![
            ContextTaskCreateInput {
                command_id: Some(format!("failure-triage:{run_id}:inspect")),
                id: Some(inspect_task_id.clone()),
                task_type: "inspection".to_string(),
                payload: json!({
                    "task_kind": "inspection",
                    "title": "Inspect failure report and affected area",
                    "draft_id": draft.draft_id,
                    "repo": draft.repo,
                    "summary": draft.title,
                    "detail": draft.detail,
                    "duplicate_matches": duplicate_matches,
                }),
                status: Some(ContextBlackboardTaskStatus::Runnable),
                workflow_id: Some("bug_monitor_triage".to_string()),
                workflow_node_id: Some("inspect_failure_report".to_string()),
                parent_task_id: None,
                depends_on_task_ids: Vec::new(),
                decision_ids: Vec::new(),
                artifact_ids: Vec::new(),
                priority: Some(10),
                max_attempts: Some(2),
            },
            ContextTaskCreateInput {
                command_id: Some(format!("failure-triage:{run_id}:validate")),
                id: Some(validate_task_id.clone()),
                task_type: "validation".to_string(),
                payload: json!({
                    "task_kind": "validation",
                    "title": "Reproduce or validate failure scope",
                    "draft_id": draft.draft_id,
                    "repo": draft.repo,
                    "depends_on": inspect_task_id,
                }),
                status: Some(ContextBlackboardTaskStatus::Pending),
                workflow_id: Some("bug_monitor_triage".to_string()),
                workflow_node_id: Some("validate_failure_scope".to_string()),
                parent_task_id: None,
                depends_on_task_ids: vec![inspect_task_id.clone()],
                decision_ids: Vec::new(),
                artifact_ids: Vec::new(),
                priority: Some(5),
                max_attempts: Some(2),
            },
        ],
    };
    let tasks_response = context_run_tasks_create(
        State(state.clone()),
        Path(run_id.clone()),
        Json(tasks_input),
    )
    .await;
    if tasks_response.is_err() {
        anyhow::bail!("Failed to seed triage tasks");
    }

    if !duplicate_matches.is_empty() {
        write_bug_monitor_artifact(
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

    let mut updated_draft = draft.clone();
    updated_draft.triage_run_id = Some(run_id.clone());
    updated_draft.status = "triage_queued".to_string();
    {
        let mut drafts = state.bug_monitor_drafts.write().await;
        drafts.insert(updated_draft.draft_id.clone(), updated_draft.clone());
    }
    state.persist_bug_monitor_drafts().await?;

    let mut run = match load_context_run_state(&state, &run_id).await {
        Ok(row) => row,
        Err(_) => created_run,
    };
    run.status = ContextRunStatus::Planning;
    run.why_next_step =
        Some("Inspect the failure report, then validate the failure scope.".to_string());
    ensure_context_run_dir(&state, &run_id)
        .await
        .map_err(|status| {
            anyhow::anyhow!("Failed to finalize triage run workspace: HTTP {status}")
        })?;
    save_context_run_state(&state, &run)
        .await
        .map_err(|status| anyhow::anyhow!("Failed to finalize triage run state: HTTP {status}"))?;
    state.event_bus.publish(tandem_types::EngineEvent::new(
        "bug_monitor.triage_run.created",
        json!({
            "draft_id": updated_draft.draft_id,
            "run_id": run.run_id,
            "repo": updated_draft.repo,
        }),
    ));

    Ok((updated_draft, run.run_id, false))
}
