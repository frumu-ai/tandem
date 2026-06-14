pub(super) async fn coder_project_run_create(
    State(state): State<AppState>,
    axum::extract::Extension(tenant_context): axum::extract::Extension<tandem_types::TenantContext>,
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
        tenant_context,
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
    axum::extract::Extension(tenant_context): axum::extract::Extension<tandem_types::TenantContext>,
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
        if !super::tenant_matches(&tenant_context, &run.tenant_context) {
            continue;
        }
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
    axum::extract::Extension(tenant_context): axum::extract::Extension<tandem_types::TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let (record, run) =
        load_coder_run_with_context_for_tenant(&state, &id, &tenant_context).await?;
    let blackboard = load_context_blackboard(&state, &record.linked_context_run_id);
    let memory_query = default_coder_memory_query(&record);
    let memory_hits = if matches!(
        record.workflow_mode,
        CoderWorkflowMode::IssueTriage
            | CoderWorkflowMode::IssueFix
            | CoderWorkflowMode::PrReview
            | CoderWorkflowMode::MergeRecommendation
    ) {
        collect_coder_memory_hits(&state, &record, Some(&run.tenant_context), &memory_query, 8)
            .await?
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
        let title_lower = item.title.to_lowercase();
        let is_parent =
            title_lower.contains("[aca slice parent]") || title_lower.contains("slice parent");
        let phase = item
            .raw
            .get("phase")
            .or_else(|| item.raw.get("Phase"))
            .and_then(Value::as_u64);
        let blocked_by = item
            .raw
            .get("blocked_by")
            .or_else(|| item.raw.get("blockedBy"))
            .cloned()
            .unwrap_or_else(|| json!([]));
        let actionable = item.issue.is_some()
            && !is_parent
            && status_alias_matches(
                &item.status_name,
                &[&github_project_binding.status_mapping.todo.name],
            );
        let run_state = linked.as_ref().and_then(|(_, run)| {
            serde_json::to_value(&run.status)
                .ok()
                .and_then(|value| value.as_str().map(ToString::to_string))
        });
        let active_run_id = linked
            .as_ref()
            .filter(|(_, run)| !is_terminal_context_status(&run.status))
            .map(|(record, _)| record.coder_run_id.clone());
        let handoff_url = linked
            .as_ref()
            .and_then(|(record, _)| record.pr_url.clone());
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
            "issue_number": item.issue.as_ref().map(|issue| issue.number),
            "issue_url": item.issue.as_ref().and_then(|issue| issue.html_url.clone()),
            "is_parent": is_parent,
            "phase": phase,
            "blocked_by": blocked_by,
            "scheduler_rank": if actionable { Some(0_u64) } else { None },
            "runnable_now": actionable,
            "run_state": run_state,
            "active_run_id": active_run_id,
            "handoff_url": handoff_url,
            "launch_state": if actionable { "next" } else if is_parent { "parent" } else { "waiting" },
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
    axum::extract::Extension(tenant_context): axum::extract::Extension<tandem_types::TenantContext>,
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
        tenant_context,
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
