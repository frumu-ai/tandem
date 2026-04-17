
pub(super) fn apply_context_event_transition(
    run: &mut ContextRunState,
    input: &ContextRunEventAppendInput,
) {
    let event_type = input.event_type.trim().to_ascii_lowercase();
    let event_step_id = input
        .step_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string);
    match event_type.as_str() {
        "session_run_started" => {
            run.status = ContextRunStatus::Running;
            run.started_at_ms = run.started_at_ms.or(Some(crate::now_ms()));
            run.ended_at_ms = None;
            run.last_error = None;
            let target_step_id = event_step_id
                .clone()
                .unwrap_or_else(|| "session-run".to_string());
            if let Some(step) = run
                .steps
                .iter_mut()
                .find(|step| step.step_id == target_step_id)
            {
                step.status = ContextStepStatus::InProgress;
            }
            run.why_next_step = context_event_payload_text(&input.payload, "why_next_step")
                .or_else(|| Some("session run in progress".to_string()));
        }
        "session_run_finished" => {
            run.status = input.status.clone();
            let target_step_id = event_step_id
                .clone()
                .unwrap_or_else(|| "session-run".to_string());
            if let Some(step) = run
                .steps
                .iter_mut()
                .find(|step| step.step_id == target_step_id)
            {
                step.status = match &input.status {
                    ContextRunStatus::Completed => ContextStepStatus::Done,
                    ContextRunStatus::Cancelled
                    | ContextRunStatus::Blocked
                    | ContextRunStatus::Paused => ContextStepStatus::Blocked,
                    ContextRunStatus::Failed => ContextStepStatus::Failed,
                    _ => ContextStepStatus::Done,
                };
            }
            if let Some(err_text) = context_event_payload_text(&input.payload, "error") {
                run.last_error = Some(err_text);
            }
            run.why_next_step = context_event_payload_text(&input.payload, "why_next_step")
                .or_else(|| Some("session run finished".to_string()));
        }
        "planning_started" => {
            if run.steps.is_empty() {
                run.steps = materialize_plan_steps_from_objective(&run.objective);
            }
            run.status = ContextRunStatus::AwaitingApproval;
            run.why_next_step = Some("plan generated and awaiting approval".to_string());
            run.started_at_ms = run.started_at_ms.or(Some(crate::now_ms()));
            run.ended_at_ms = None;
            run.last_error = None;
        }
        "plan_approved" => {
            run.status = ContextRunStatus::Running;
            run.started_at_ms = run.started_at_ms.or(Some(crate::now_ms()));
            run.ended_at_ms = None;
            run.last_error = None;
            if let Some(step) = run.steps.iter_mut().find(|step| {
                matches!(
                    step.status,
                    ContextStepStatus::Pending | ContextStepStatus::Runnable
                )
            }) {
                step.status = ContextStepStatus::InProgress;
                run.why_next_step =
                    Some(format!("executing step `{}` after approval", step.step_id));
            }
        }
        "run_paused" => {
            run.status = ContextRunStatus::Paused;
            run.why_next_step = Some("run paused by operator".to_string());
        }
        "run_resumed" => {
            run.status = ContextRunStatus::Running;
            run.why_next_step = Some("run resumed by operator".to_string());
            run.ended_at_ms = None;
        }
        "run_cancelled" => {
            run.status = ContextRunStatus::Cancelled;
            run.why_next_step = Some("run cancelled by operator".to_string());
            run.ended_at_ms = Some(crate::now_ms());
        }
        "task_retry_requested" => {
            if let Some(step_id) = input
                .step_id
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                if let Some(step) = run.steps.iter_mut().find(|step| step.step_id == step_id) {
                    step.status = ContextStepStatus::Runnable;
                }
                run.why_next_step = Some(format!("retry requested for step `{step_id}`"));
            }
            run.status = ContextRunStatus::Running;
            run.ended_at_ms = None;
            run.last_error = None;
        }
        "revision_requested" => {
            run.status = ContextRunStatus::Blocked;
            let feedback =
                context_event_payload_text(&input.payload, "feedback").unwrap_or_default();
            run.why_next_step = Some(if feedback.is_empty() {
                "revision requested by operator".to_string()
            } else {
                format!("revision requested: {feedback}")
            });
        }
        "step_started" | "task_started" => {
            run.status = input.status.clone();
            if let Some(step_id) = event_step_id.clone() {
                if let Some(step) = run.steps.iter_mut().find(|step| step.step_id == step_id) {
                    step.status = context_event_step_status(&input.payload)
                        .unwrap_or(ContextStepStatus::InProgress);
                }
            }
            if let Some(why) = context_event_payload_text(&input.payload, "why_next_step") {
                run.why_next_step = Some(why);
            }
        }
        "step_completed" | "task_completed" | "step_done" => {
            run.status = input.status.clone();
            if let Some(step_id) = event_step_id.clone() {
                if let Some(step) = run.steps.iter_mut().find(|step| step.step_id == step_id) {
                    step.status = context_event_step_status(&input.payload)
                        .unwrap_or(ContextStepStatus::Done);
                }
            }
            if let Some(why) = context_event_payload_text(&input.payload, "why_next_step") {
                run.why_next_step = Some(why);
            }
        }
        "step_failed" | "task_failed" => {
            run.status = input.status.clone();
            if let Some(step_id) = event_step_id.clone() {
                if let Some(step) = run.steps.iter_mut().find(|step| step.step_id == step_id) {
                    step.status = context_event_step_status(&input.payload)
                        .unwrap_or(ContextStepStatus::Failed);
                }
            }
            if let Some(err_text) = context_event_payload_text(&input.payload, "error") {
                run.last_error = Some(err_text);
            }
            if let Some(why) = context_event_payload_text(&input.payload, "why_next_step") {
                run.why_next_step = Some(why);
            }
        }
        _ => {
            run.status = input.status.clone();
            if let Some(step_id) = event_step_id.clone() {
                if let Some(step_status) = context_event_step_status(&input.payload) {
                    if let Some(step) = run.steps.iter_mut().find(|step| step.step_id == step_id) {
                        step.status = step_status;
                    }
                }
            }
            if let Some(why) = context_event_payload_text(&input.payload, "why_next_step") {
                run.why_next_step = Some(why);
            }
            if let Some(err_text) = context_event_payload_text(&input.payload, "error") {
                run.last_error = Some(err_text);
            }
        }
    }

    if matches!(
        run.status,
        ContextRunStatus::Completed | ContextRunStatus::Failed | ContextRunStatus::Cancelled
    ) {
        run.ended_at_ms = Some(crate::now_ms());
    }
}

pub(super) async fn context_run_event_append(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<ContextRunEventAppendInput>,
) -> Result<Json<Value>, StatusCode> {
    if input.event_type.trim().starts_with("context.task.") {
        return Ok(Json(json!({
            "ok": false,
            "error": "task events must go through the run task endpoints",
            "code": "TASK_EVENT_APPEND_DISABLED"
        })));
    }
    let outcome = context_run_engine()
        .commit_run_event(&state, &run_id, input, None)
        .await?;
    Ok(Json(json!({ "ok": true, "event": outcome.event })))
}

pub(crate) async fn append_context_run_event(
    state: &AppState,
    run_id: &str,
    input: ContextRunEventAppendInput,
) -> Result<(), StatusCode> {
    let _ = context_run_engine()
        .commit_run_event(state, run_id, input, None)
        .await?;
    Ok(())
}

pub(super) async fn context_run_events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<super::RunEventsQuery>,
) -> Result<Json<Value>, StatusCode> {
    let rows = load_context_run_events_jsonl(
        &context_run_events_path(&state, &run_id),
        query.since_seq,
        query.tail,
    );
    Ok(Json(json!({ "events": rows })))
}

pub(super) fn context_run_events_sse_stream(
    state: AppState,
    run_id: String,
    query: super::RunEventsQuery,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(256);
    tokio::spawn(async move {
        let events_path = context_run_events_path(&state, &run_id);
        let ready = serde_json::to_string(&json!({
            "status":"ready",
            "stream":"context_run_events",
            "runID": run_id,
            "timestamp_ms": crate::now_ms(),
            "path": events_path.to_string_lossy().to_string(),
        }))
        .unwrap_or_default();
        if tx.send(ready).await.is_err() {
            return;
        }

        let initial = load_context_run_events_jsonl(&events_path, query.since_seq, query.tail);
        let mut last_seq = query.since_seq.unwrap_or(0);
        for row in initial {
            last_seq = last_seq.max(row.seq);
            let payload = serde_json::to_string(&json!({
                "type":"run.event",
                "properties": row
            }))
            .unwrap_or_default();
            if tx.send(payload).await.is_err() {
                return;
            }
        }

        let mut interval = tokio::time::interval(Duration::from_millis(1000));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let updates = load_context_run_events_jsonl(&events_path, Some(last_seq), None);
            for row in updates {
                last_seq = last_seq.max(row.seq);
                let payload = serde_json::to_string(&json!({
                    "type":"run.event",
                    "properties": row
                }))
                .unwrap_or_default();
                if tx.send(payload).await.is_err() {
                    return;
                }
            }
        }
    });
    ReceiverStream::new(rx).map(|payload| Ok(Event::default().data(payload)))
}

pub(super) async fn context_run_events_stream(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<super::RunEventsQuery>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    Sse::new(context_run_events_sse_stream(state, run_id, query))
        .keep_alive(axum::response::sse::KeepAlive::new().interval(Duration::from_secs(10)))
}

pub(super) fn context_runs_events_multiplex_sse_stream(
    state: AppState,
    workspace: String,
    subscribed_run_ids: Vec<String>,
    cursor: ContextRunsStreamCursor,
    tail: Option<usize>,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(512);
    tokio::spawn(async move {
        let subscribed_set: HashSet<String> = subscribed_run_ids.iter().cloned().collect();
        let ready = serde_json::to_string(&json!({
            "kind":"ready",
            "workspace": workspace,
            "subscribed_run_ids": subscribed_run_ids,
            "timestamp_ms": crate::now_ms(),
        }))
        .unwrap_or_default();
        if tx.send(ready).await.is_err() {
            return;
        }

        let mut replay = Vec::<ContextRunsStreamEnvelope>::new();
        for run_id in &subscribed_set {
            let run_events = load_context_run_events_jsonl(
                &context_run_events_path(&state, run_id),
                cursor.events.get(run_id).copied(),
                if cursor.events.get(run_id).is_some() {
                    None
                } else {
                    tail
                },
            );
            for row in run_events {
                replay.push(ContextRunsStreamEnvelope {
                    kind: "context_run_event".to_string(),
                    run_id: run_id.clone(),
                    workspace: workspace.clone(),
                    seq: row.seq,
                    ts_ms: row.ts_ms,
                    payload: serde_json::to_value(row).unwrap_or_else(|_| json!({})),
                });
            }
            let run_patches = load_context_blackboard_patches(
                &state,
                run_id,
                cursor.patches.get(run_id).copied(),
                if cursor.patches.get(run_id).is_some() {
                    None
                } else {
                    tail
                },
            );
            for patch in run_patches {
                replay.push(ContextRunsStreamEnvelope {
                    kind: "blackboard_patch".to_string(),
                    run_id: run_id.clone(),
                    workspace: workspace.clone(),
                    seq: patch.seq,
                    ts_ms: patch.ts_ms,
                    payload: serde_json::to_value(patch).unwrap_or_else(|_| json!({})),
                });
            }
        }
        replay.sort_by(|a, b| {
            a.ts_ms
                .cmp(&b.ts_ms)
                .then_with(|| a.run_id.cmp(&b.run_id))
                .then_with(|| a.kind.cmp(&b.kind))
                .then_with(|| a.seq.cmp(&b.seq))
        });
        for row in replay {
            let payload = serde_json::to_string(&row).unwrap_or_default();
            if tx.send(payload).await.is_err() {
                return;
            }
        }

        let mut live = state.event_bus.subscribe();
        loop {
            match live.recv().await {
                Ok(event) => {
                    if event.event_type != "context.run.stream" {
                        continue;
                    }
                    let run_id = event
                        .properties
                        .get("run_id")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or_default();
                    if run_id.is_empty() || !subscribed_set.contains(run_id) {
                        continue;
                    }
                    let event_workspace = event
                        .properties
                        .get("workspace")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .unwrap_or_default();
                    if event_workspace != workspace {
                        continue;
                    }
                    let payload = serde_json::to_string(&event.properties).unwrap_or_default();
                    if tx.send(payload).await.is_err() {
                        return;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });
    ReceiverStream::new(rx).map(|payload| Ok(Event::default().data(payload)))
}

pub(super) async fn context_runs_events_stream(
    State(state): State<AppState>,
    Query(query): Query<ContextRunsEventsStreamQuery>,
) -> Result<Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>>, StatusCode> {
    let workspace = query
        .workspace
        .as_deref()
        .and_then(tandem_core::normalize_workspace_path)
        .ok_or(StatusCode::BAD_REQUEST)?;
    let requested = parse_context_run_ids_csv(query.run_ids.as_deref());
    let subscribed_run_ids = if requested.is_empty() {
        list_context_runs_for_workspace(&state, &workspace, 1000)
            .await?
            .into_iter()
            .map(|run| run.run_id)
            .collect::<Vec<_>>()
    } else {
        let mut accepted = Vec::<String>::new();
        for run_id in requested {
            let run = load_context_run_state(&state, &run_id)
                .await
                .map_err(|_| StatusCode::BAD_REQUEST)?;
            if run.workspace.canonical_path.trim() != workspace {
                return Err(StatusCode::BAD_REQUEST);
            }
            accepted.push(run_id);
        }
        accepted.sort();
        accepted.dedup();
        accepted
    };
    let cursor = decode_context_stream_cursor(query.cursor.as_deref());
    let tail = query.tail.map(|value| value.clamp(1, 2000));
    Ok(Sse::new(context_runs_events_multiplex_sse_stream(
        state,
        workspace,
        subscribed_run_ids,
        cursor,
        tail,
    ))
    .keep_alive(axum::response::sse::KeepAlive::new().interval(Duration::from_secs(10))))
}

pub(super) async fn context_run_lease_validate(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<ContextLeaseValidateInput>,
) -> Result<Json<Value>, StatusCode> {
    let mut run = load_context_run_state(&state, &run_id).await?;
    if run.workspace.canonical_path.trim().is_empty() {
        run.workspace.canonical_path = input.current_path.clone();
        if run.workspace.lease_epoch == 0 {
            run.workspace.lease_epoch = 1;
        }
        let _ = context_run_engine()
            .commit_snapshot_with_event(
                &state,
                &run_id,
                run.clone(),
                ContextRunEventAppendInput {
                    event_type: "workspace_validated".to_string(),
                    status: run.status.clone(),
                    step_id: input.step_id.clone(),
                    payload: json!({
                        "phase": input.phase,
                        "expected_path": run.workspace.canonical_path,
                        "actual_path": input.current_path,
                        "run": run,
                    }),
                },
                None,
            )
            .await?;
        return Ok(Json(json!({ "ok": true, "mismatch": false })));
    }

    if run.workspace.canonical_path != input.current_path {
        run.status = ContextRunStatus::Paused;
        let _ = context_run_engine()
            .commit_snapshot_with_event(
                &state,
                &run_id,
                run.clone(),
                ContextRunEventAppendInput {
                    event_type: "workspace_mismatch".to_string(),
                    status: ContextRunStatus::Paused,
                    step_id: input.step_id.clone(),
                    payload: json!({
                        "phase": input.phase,
                        "expected_path": run.workspace.canonical_path,
                        "actual_path": input.current_path,
                        "run": run,
                    }),
                },
                None,
            )
            .await?;
        return Ok(Json(json!({
            "ok": false,
            "mismatch": true,
            "status": "paused"
        })));
    }

    Ok(Json(json!({ "ok": true, "mismatch": false })))
}

pub(super) async fn context_run_blackboard_get(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let blackboard = load_projected_context_blackboard(&state, &run_id);
    Ok(Json(json!({ "blackboard": blackboard })))
}

pub(super) fn context_run_events_have_command_id(
    state: &AppState,
    run_id: &str,
    command_id: &str,
) -> bool {
    if command_id.trim().is_empty() {
        return false;
    }
    load_context_run_events_jsonl(&context_run_events_path(state, run_id), None, Some(500))
        .iter()
        .any(|event| event.command_id.as_deref() == Some(command_id))
}

pub(super) async fn context_run_blackboard_patches_get(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<ContextBlackboardPatchesQuery>,
) -> Result<Json<Value>, StatusCode> {
    let patches = load_context_blackboard_patches(&state, &run_id, query.since_seq, query.tail);
    Ok(Json(json!({ "patches": patches })))
}

pub(super) async fn context_run_blackboard_patch(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<ContextBlackboardPatchInput>,
) -> Result<Json<Value>, StatusCode> {
    if matches!(
        input.op,
        ContextBlackboardPatchOp::AddTask
            | ContextBlackboardPatchOp::UpdateTaskLease
            | ContextBlackboardPatchOp::UpdateTaskState
    ) {
        return Ok(Json(json!({
            "ok": false,
            "error": "task mutations must go through the run task endpoints",
            "code": "TASK_PATCH_OP_DISABLED"
        })));
    }
    let outcome = context_run_engine()
        .commit_blackboard_patch(&state, &run_id, input.op, input.payload)
        .await?;
    Ok(Json(json!({
        "ok": true,
        "patch": outcome.patch,
        "blackboard": outcome.blackboard
    })))
}

pub(super) async fn context_run_tasks_create(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<ContextTaskCreateBatchInput>,
) -> Result<Json<Value>, StatusCode> {
    let lock = context_run_lock_for(&run_id).await;
    let _guard = lock.lock().await;
    if input.tasks.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let run_status = load_context_run_state(&state, &run_id)
        .await
        .ok()
        .map(|run| run.status)
        .unwrap_or(ContextRunStatus::Planning);
    let mut created = Vec::<ContextBlackboardTask>::new();
    let mut patches = Vec::<ContextBlackboardPatchRecord>::new();

    for row in input.tasks {
        let command_id = row.command_id.clone();
        if row.task_type.trim().is_empty() {
            return Err(StatusCode::BAD_REQUEST);
        }
        if let Some(command_id) = command_id.as_deref() {
            if context_run_events_have_command_id(&state, &run_id, command_id) {
                continue;
            }
        }
        let now = crate::now_ms();
        let normalized_contract = match normalize_context_task_payload(&row.task_type, &row.payload)
        {
            Ok(value) => value,
            Err((code, error)) => {
                return Ok(Json(json!({
                    "ok": false,
                    "code": code,
                    "error": error,
                    "task_id": row.id.clone().unwrap_or_default(),
                })));
            }
        };
        let (task_type, payload) = normalized_contract
            .unwrap_or_else(|| (row.task_type.trim().to_string(), row.payload.clone()));
        let task = ContextBlackboardTask {
            id: row
                .id
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| format!("task-{}", Uuid::new_v4())),
            task_type,
            payload,
            status: row.status.unwrap_or(ContextBlackboardTaskStatus::Pending),
            workflow_id: row
                .workflow_id
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            workflow_node_id: row
                .workflow_node_id
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            parent_task_id: row
                .parent_task_id
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            depends_on_task_ids: row
                .depends_on_task_ids
                .into_iter()
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
                .collect::<Vec<_>>(),
            decision_ids: row.decision_ids,
            artifact_ids: row.artifact_ids,
            assigned_agent: None,
            priority: row.priority.unwrap_or(0),
            attempt: 0,
            max_attempts: row.max_attempts.unwrap_or(3),
            last_error: None,
            next_retry_at_ms: None,
            lease_owner: None,
            lease_token: None,
            lease_expires_at_ms: None,
            task_rev: 1,
            created_ts: now,
            updated_ts: now,
        };
        let mut payload =
            serde_json::to_value(&task).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if let Some(command_id) = command_id.clone() {
            payload["command_id"] = json!(command_id);
        }
        let event_payload = json!({
            "task_id": task.id,
            "task_type": task.task_type,
            "task_rev": task.task_rev,
            "workflow_id": task.workflow_id,
        });
        let outcome = context_run_engine()
            .commit_task_mutation(
                &state,
                &run_id,
                task.clone(),
                ContextBlackboardPatchOp::AddTask,
                payload,
                "context.task.created".to_string(),
                run_status.clone(),
                command_id,
                event_payload,
            )
            .await?;
        if let Some(patch) = outcome.patch {
            patches.push(patch);
        }
        created.push(task);
    }

    let blackboard = load_projected_context_blackboard(&state, &run_id);
    Ok(Json(
        json!({ "ok": true, "tasks": created, "patches": patches, "blackboard": blackboard }),
    ))
}

pub(super) async fn context_run_tasks_claim(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<ContextTaskClaimInput>,
) -> Result<Json<Value>, StatusCode> {
    let task = claim_next_context_task(
        &state,
        &run_id,
        &input.agent_id,
        input.task_type.as_deref(),
        input.workflow_id.as_deref(),
        input.lease_ms,
        input.command_id.clone(),
    )
    .await?;
    Ok(Json(json!({
        "ok": true,
        "task": task,
        "blackboard": load_projected_context_blackboard(&state, &run_id),
    })))
}

pub(super) async fn claim_next_context_task(
    state: &AppState,
    run_id: &str,
    agent_id: &str,
    task_type: Option<&str>,
    workflow_id: Option<&str>,
    lease_ms: Option<u64>,
    command_id: Option<String>,
) -> Result<Option<ContextBlackboardTask>, StatusCode> {
    let lock = context_run_lock_for(&run_id).await;
    let _guard = lock.lock().await;
    if agent_id.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if let Some(command_id) = command_id.as_deref() {
        if context_run_events_have_command_id(state, run_id, command_id) {
            return Ok(None);
        }
    }
    let run_status = load_context_run_state(state, run_id)
        .await
        .ok()
        .map(|run| run.status)
        .unwrap_or(ContextRunStatus::Running);
    let run = load_context_run_state(state, run_id).await?;
    let _ = requeue_stale_context_tasks_locked(state, run_id, run_status.clone(), &run).await?;
    let run = load_context_run_state(state, run_id).await?;
    let now = crate::now_ms();
    let mut task_idx = run
        .tasks
        .iter()
        .enumerate()
        .filter(|(_, task)| context_task_is_claimable(&run, task, now, task_type, workflow_id))
        .map(|(idx, _)| idx)
        .collect::<Vec<_>>();
    task_idx.sort_by(|a, b| {
        let left = &run.tasks[*a];
        let right = &run.tasks[*b];
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| left.created_ts.cmp(&right.created_ts))
    });
    let Some(selected_idx) = task_idx.first().copied() else {
        return Ok(None);
    };
    let selected = run.tasks[selected_idx].clone();
    let claimed_task = commit_context_task_claim(
        state, run_id, run_status, &selected, agent_id, lease_ms, command_id,
    )
    .await?;
    Ok(Some(claimed_task))
}

pub(super) async fn claim_context_task_by_id(
    state: &AppState,
    run_id: &str,
    task_id: &str,
    agent_id: &str,
    lease_ms: Option<u64>,
    command_id: Option<String>,
) -> Result<Option<ContextBlackboardTask>, StatusCode> {
    let lock = context_run_lock_for(run_id).await;
    let _guard = lock.lock().await;
    if agent_id.trim().is_empty() || task_id.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if let Some(command_id) = command_id.as_deref() {
        if context_run_events_have_command_id(state, run_id, command_id) {
            return Ok(None);
        }
    }
    let run_status = load_context_run_state(state, run_id)
        .await
        .ok()
        .map(|run| run.status)
        .unwrap_or(ContextRunStatus::Running);
    let run = load_context_run_state(state, run_id).await?;
    let _ = requeue_stale_context_tasks_locked(state, run_id, run_status.clone(), &run).await?;
    let run = load_context_run_state(state, run_id).await?;
    let now = crate::now_ms();
    let Some(selected) = run.tasks.iter().find(|task| task.id == task_id).cloned() else {
        return Ok(None);
    };
    if !context_task_is_claimable(&run, &selected, now, None, None) {
        return Ok(None);
    }
    let claimed_task = commit_context_task_claim(
        state, run_id, run_status, &selected, agent_id, lease_ms, command_id,
    )
    .await?;
    Ok(Some(claimed_task))
}

pub(super) async fn requeue_context_task_by_id(
    state: &AppState,
    run_id: &str,
    task_id: &str,
    command_id: Option<String>,
    detail: Option<String>,
) -> Result<Option<ContextBlackboardTask>, StatusCode> {
    let lock = context_run_lock_for(run_id).await;
    let _guard = lock.lock().await;
    if task_id.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if let Some(command_id) = command_id.as_deref() {
        if context_run_events_have_command_id(state, run_id, command_id) {
            return Ok(None);
        }
    }
    let run_status = load_context_run_state(state, run_id)
        .await
        .ok()
        .map(|run| run.status)
        .unwrap_or(ContextRunStatus::Running);
    let run = load_context_run_state(state, run_id).await?;
    let Some(current) = run.tasks.iter().find(|task| task.id == task_id).cloned() else {
        return Ok(None);
    };
    if matches!(current.status, ContextBlackboardTaskStatus::Done) {
        return Ok(None);
    }

    let now = crate::now_ms();
    let next_rev = current.task_rev.saturating_add(1);
    let next_task = ContextBlackboardTask {
        status: ContextBlackboardTaskStatus::Runnable,
        lease_owner: None,
        lease_token: None,
        lease_expires_at_ms: None,
        next_retry_at_ms: Some(now),
        task_rev: next_rev,
        updated_ts: now,
        last_error: detail.clone().or(current.last_error.clone()),
        ..current.clone()
    };
    let mut payload = json!({
        "task_id": current.id,
        "status": ContextBlackboardTaskStatus::Runnable,
        "lease_owner": Value::Null,
        "lease_token": Value::Null,
        "lease_expires_at_ms": Value::Null,
        "task_rev": next_rev,
        "attempt": current.attempt,
        "next_retry_at_ms": now,
    });
    if let Some(detail) = detail.clone() {
        payload["error"] = json!(detail);
    }
    if let Some(command_id) = command_id.clone() {
        payload["command_id"] = json!(command_id);
    }
    context_run_engine()
        .commit_task_mutation(
            state,
            run_id,
            next_task.clone(),
            ContextBlackboardPatchOp::UpdateTaskState,
            payload,
            "context.task.requeued".to_string(),
            run_status,
            command_id,
            json!({
                "task_id": current.id,
                "workflow_id": current.workflow_id,
                "manual_requeue": true,
            }),
        )
        .await?;
    Ok(Some(next_task))
}

pub(super) async fn context_run_task_transition(
    State(state): State<AppState>,
    Path((run_id, task_id)): Path<(String, String)>,
    Json(input): Json<ContextTaskTransitionInput>,
) -> Result<Json<Value>, StatusCode> {
    let lock = context_run_lock_for(&run_id).await;
    let _guard = lock.lock().await;
    let command_id = input.command_id.clone();
    if let Some(command_id) = command_id.as_deref() {
        if context_run_events_have_command_id(&state, &run_id, command_id) {
            let run = load_context_run_state(&state, &run_id).await.ok();
            let blackboard = load_projected_context_blackboard(&state, &run_id);
            return Ok(Json(json!({
                "ok": true,
                "deduped": true,
                "task": run
                    .as_ref()
                    .map(|row| row.tasks.clone())
                    .unwrap_or_default()
                    .into_iter()
                    .find(|row| row.id == task_id),
                "blackboard": blackboard,
            })));
        }
    }
    let run_status = load_context_run_state(&state, &run_id)
        .await
        .ok()
        .map(|run| run.status)
        .unwrap_or(ContextRunStatus::Running);
    let run = load_context_run_state(&state, &run_id).await?;
    let Some(current) = run.tasks.iter().find(|row| row.id == task_id).cloned() else {
        return Err(StatusCode::NOT_FOUND);
    };
    if let Some(expected_rev) = input.expected_task_rev {
        if expected_rev != current.task_rev {
            return Ok(Json(json!({
                "ok": false,
                "error": "task revision mismatch",
                "code": "TASK_REV_MISMATCH",
                "task_rev": current.task_rev
            })));
        }
    }
    let action = input.action.trim().to_ascii_lowercase();
    let requested_status = input.status.as_ref();
    if context_task_transition_requires_valid_lease(&current, &action, requested_status) {
        let Some(token) = input.lease_token.as_deref() else {
            return Ok(Json(json!({
                "ok": false,
                "error": "lease token required",
                "code": "TASK_LEASE_REQUIRED"
            })));
        };
        if current.lease_token.as_deref() != Some(token) {
            return Ok(Json(json!({
                "ok": false,
                "error": "invalid lease token",
                "code": "TASK_LEASE_INVALID"
            })));
        }
    }
    match action.as_str() {
        "heartbeat" if current.status != ContextBlackboardTaskStatus::InProgress => {
            return Ok(Json(json!({
                "ok": false,
                "error": "heartbeat requires an in_progress task",
                "code": "TASK_TRANSITION_INVALID",
                "current_status": current.status.clone(),
            })));
        }
        "status" => {
            let Some(next_status) = requested_status else {
                return Err(StatusCode::BAD_REQUEST);
            };
            if !context_task_status_transition_allowed(&current.status, next_status) {
                return Ok(Json(json!({
                    "ok": false,
                    "error": "invalid task status transition",
                    "code": "TASK_TRANSITION_INVALID",
                    "current_status": current.status.clone(),
                    "requested_status": next_status.clone(),
                })));
            }
        }
        "complete" if current.status != ContextBlackboardTaskStatus::InProgress => {
            return Ok(Json(json!({
                "ok": false,
                "error": "task must be in_progress before completion",
                "code": "TASK_TRANSITION_INVALID",
                "current_status": current.status.clone(),
            })));
        }
        "fail"
            if !matches!(
                current.status,
                ContextBlackboardTaskStatus::InProgress | ContextBlackboardTaskStatus::Blocked
            ) =>
        {
            return Ok(Json(json!({
                "ok": false,
                "error": "task must be active before failure is recorded",
                "code": "TASK_TRANSITION_INVALID",
                "current_status": current.status.clone(),
            })));
        }
        "release"
            if !matches!(
                current.status,
                ContextBlackboardTaskStatus::InProgress | ContextBlackboardTaskStatus::Blocked
            ) =>
        {
            return Ok(Json(json!({
                "ok": false,
                "error": "task must be active before release",
                "code": "TASK_TRANSITION_INVALID",
                "current_status": current.status.clone(),
            })));
        }
        "retry"
            if !matches!(
                current.status,
                ContextBlackboardTaskStatus::Failed | ContextBlackboardTaskStatus::Blocked
            ) =>
        {
            return Ok(Json(json!({
                "ok": false,
                "error": "task must be failed or blocked before retry",
                "code": "TASK_TRANSITION_INVALID",
                "current_status": current.status.clone(),
            })));
        }
        _ => {}
    }
    let next_rev = current.task_rev.saturating_add(1);
    let now = crate::now_ms();
    let (op, mut payload, next_task) = match action.as_str() {
        "heartbeat" => {
            let lease_expires_at_ms =
                now.saturating_add(input.lease_ms.unwrap_or(30_000).clamp(5_000, 300_000));
            let assigned_agent = input.agent_id.clone().or(current.assigned_agent.clone());
            let lease_owner = input.agent_id.clone().or(current.lease_owner.clone());
            let lease_token = input.lease_token.clone().or(current.lease_token.clone());
            (
                ContextBlackboardPatchOp::UpdateTaskLease,
                json!({
                    "task_id": task_id,
                    "lease_owner": lease_owner.clone(),
                    "assigned_agent": assigned_agent.clone(),
                    "lease_token": lease_token.clone(),
                    "lease_expires_at_ms": lease_expires_at_ms,
                    "task_rev": next_rev,
                }),
                ContextBlackboardTask {
                    assigned_agent,
                    lease_owner,
                    lease_token,
                    lease_expires_at_ms: Some(lease_expires_at_ms),
                    task_rev: next_rev,
                    updated_ts: now,
                    ..current.clone()
                },
            )
        }
        "status" => {
            let status = input.status.clone().ok_or(StatusCode::BAD_REQUEST)?;
            let assigned_agent = input.agent_id.clone().or(current.assigned_agent.clone());
            (
                ContextBlackboardPatchOp::UpdateTaskState,
                json!({
                    "task_id": task_id,
                    "status": status.clone(),
                    "lease_owner": current.lease_owner.clone(),
                    "lease_token": current.lease_token.clone(),
                    "lease_expires_at_ms": current.lease_expires_at_ms,
                    "assigned_agent": assigned_agent.clone(),
                    "task_rev": next_rev,
                    "error": input.error.clone(),
                    "attempt": current.attempt,
                }),
                ContextBlackboardTask {
                    status,
                    assigned_agent,
                    last_error: input.error.clone().or(current.last_error.clone()),
                    task_rev: next_rev,
                    updated_ts: now,
                    ..current.clone()
                },
            )
        }
        "complete" => {
            let assigned_agent = input.agent_id.clone().or(current.assigned_agent.clone());
            (
                ContextBlackboardPatchOp::UpdateTaskState,
                json!({
                    "task_id": task_id,
                    "status": ContextBlackboardTaskStatus::Done,
                    "lease_owner": Value::Null,
                    "lease_token": Value::Null,
                    "lease_expires_at_ms": Value::Null,
                    "assigned_agent": assigned_agent.clone(),
                    "task_rev": next_rev,
                    "attempt": current.attempt,
                }),
                ContextBlackboardTask {
                    status: ContextBlackboardTaskStatus::Done,
                    assigned_agent,
                    lease_owner: None,
                    lease_token: None,
                    lease_expires_at_ms: None,
                    task_rev: next_rev,
                    updated_ts: now,
                    ..current.clone()
                },
            )
        }
        "fail" => {
            let assigned_agent = input.agent_id.clone().or(current.assigned_agent.clone());
            let last_error = input.error.clone().or(current.last_error.clone());
            let attempt = current.attempt.saturating_add(1);
            (
                ContextBlackboardPatchOp::UpdateTaskState,
                json!({
                    "task_id": task_id,
                    "status": ContextBlackboardTaskStatus::Failed,
                    "lease_owner": Value::Null,
                    "lease_token": Value::Null,
                    "lease_expires_at_ms": Value::Null,
                    "assigned_agent": assigned_agent.clone(),
                    "task_rev": next_rev,
                    "error": last_error.clone(),
                    "attempt": attempt,
                }),
                ContextBlackboardTask {
                    status: ContextBlackboardTaskStatus::Failed,
                    assigned_agent,
                    lease_owner: None,
                    lease_token: None,
                    lease_expires_at_ms: None,
                    last_error,
                    attempt,
                    task_rev: next_rev,
                    updated_ts: now,
                    ..current.clone()
                },
            )
        }
        "release" => (
            ContextBlackboardPatchOp::UpdateTaskState,
            json!({
                "task_id": task_id,
                "status": ContextBlackboardTaskStatus::Runnable,
                "lease_owner": Value::Null,
                "lease_token": Value::Null,
                "lease_expires_at_ms": Value::Null,
                "assigned_agent": current.assigned_agent.clone(),
                "task_rev": next_rev,
                "attempt": current.attempt,
            }),
            ContextBlackboardTask {
                status: ContextBlackboardTaskStatus::Runnable,
                lease_owner: None,
                lease_token: None,
                lease_expires_at_ms: None,
                task_rev: next_rev,
                updated_ts: now,
                ..current.clone()
            },
        ),
        "retry" => {
            let attempt = current.attempt.saturating_add(1);
            (
                ContextBlackboardPatchOp::UpdateTaskState,
                json!({
                    "task_id": task_id,
                    "status": ContextBlackboardTaskStatus::Runnable,
                    "lease_owner": Value::Null,
                    "lease_token": Value::Null,
                    "lease_expires_at_ms": Value::Null,
                    "assigned_agent": current.assigned_agent.clone(),
                    "task_rev": next_rev,
                    "error": Value::Null,
                    "attempt": attempt,
                    "next_retry_at_ms": now,
                }),
                ContextBlackboardTask {
                    status: ContextBlackboardTaskStatus::Runnable,
                    lease_owner: None,
                    lease_token: None,
                    lease_expires_at_ms: None,
                    last_error: None,
                    attempt,
                    next_retry_at_ms: Some(now),
                    task_rev: next_rev,
                    updated_ts: now,
                    ..current.clone()
                },
            )
        }
        _ => return Err(StatusCode::BAD_REQUEST),
    };
    if let Some(command_id) = command_id.clone() {
        payload["command_id"] = json!(command_id);
    }
    let task = Some(next_task.clone());
    let (event_type, event_payload) = if action == "heartbeat" {
        (
            "context.task.heartbeat".to_string(),
            json!({
                "task_id": task_id,
                "task_rev": task.as_ref().map(|row| row.task_rev),
            }),
        )
    } else {
        let status = task
            .as_ref()
            .map(|row| row.status.clone())
            .unwrap_or(ContextBlackboardTaskStatus::Blocked);
        (
            context_task_status_event_name(&status).to_string(),
            json!({
                "task_id": task_id,
                "task_rev": task.as_ref().map(|row| row.task_rev),
                "status": status,
                "error": input.error,
            }),
        )
    };
    let outcome = context_run_engine()
        .commit_task_mutation(
            &state,
            &run_id,
            next_task.clone(),
            op,
            payload,
            event_type,
            run_status,
            command_id,
            event_payload,
        )
        .await?;
    Ok(Json(json!({
        "ok": true,
        "task": task,
        "patch": outcome.patch,
        "blackboard": outcome.blackboard
    })))
}

pub(super) async fn context_run_checkpoint_create(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<ContextCheckpointCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    let run_state = load_context_run_state(&state, &run_id).await?;
    let blackboard = load_projected_context_blackboard(&state, &run_id);
    let events_path = context_run_events_path(&state, &run_id);
    let seq = latest_context_run_event_seq(&events_path);
    let checkpoint = ContextCheckpointRecord {
        checkpoint_id: format!("cp-{}", Uuid::new_v4()),
        run_id: run_id.clone(),
        seq,
        ts_ms: crate::now_ms(),
        reason: input.reason,
        run_state,
        blackboard,
    };
    let dir = context_run_checkpoints_dir(&state, &run_id);
    std::fs::create_dir_all(&dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let path = dir.join(format!("{:020}.json", seq));
    let payload =
        serde_json::to_string_pretty(&checkpoint).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    std::fs::write(path, payload).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "ok": true, "checkpoint": checkpoint })))
}

pub(super) async fn context_run_checkpoint_latest(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let dir = context_run_checkpoints_dir(&state, &run_id);
    if !dir.exists() {
        return Ok(Json(json!({ "checkpoint": null })));
    }
    let mut latest_name: Option<String> = None;
    let mut entries = std::fs::read_dir(&dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    while let Some(Ok(entry)) = entries.next() {
        if !entry
            .file_type()
            .map(|kind| kind.is_file())
            .unwrap_or(false)
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".json") {
            continue;
        }
        let should_replace = match latest_name.as_ref() {
            Some(current) => name > *current,
            None => true,
        };
        if should_replace {
            latest_name = Some(name);
        }
    }
    let Some(file_name) = latest_name else {
        return Ok(Json(json!({ "checkpoint": null })));
    };
    let raw = std::fs::read_to_string(dir.join(file_name))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let checkpoint = serde_json::from_str::<ContextCheckpointRecord>(&raw)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "checkpoint": checkpoint })))
}

pub(super) fn parse_context_step_status_text(value: &str) -> Option<ContextStepStatus> {
    match value.trim().to_ascii_lowercase().as_str() {
        "pending" => Some(ContextStepStatus::Pending),
        "runnable" => Some(ContextStepStatus::Runnable),
        "in_progress" | "in-progress" | "working" | "active" => Some(ContextStepStatus::InProgress),
        "blocked" => Some(ContextStepStatus::Blocked),
        "done" | "completed" | "complete" => Some(ContextStepStatus::Done),
        "failed" | "error" => Some(ContextStepStatus::Failed),
        _ => None,
    }
}

pub(super) fn replay_step_status_from_event(
    event: &ContextRunEventRecord,
) -> Option<ContextStepStatus> {
    if let Some(status_text) = event.payload.get("step_status").and_then(Value::as_str) {
        if let Some(status) = parse_context_step_status_text(status_text) {
            return Some(status);
        }
    }
    let ty = event.event_type.to_ascii_lowercase();
    if ty.contains("step_started") || ty.contains("step_running") {
        return Some(ContextStepStatus::InProgress);
    }
    if ty.contains("step_blocked") {
        return Some(ContextStepStatus::Blocked);
    }
    if ty.contains("step_failed") {
        return Some(ContextStepStatus::Failed);
    }
    if ty.contains("step_done") || ty.contains("step_completed") {
        return Some(ContextStepStatus::Done);
    }
    None
}

pub(super) fn latest_context_checkpoint_record(
    state: &AppState,
    run_id: &str,
) -> Option<ContextCheckpointRecord> {
    let dir = context_run_checkpoints_dir(state, run_id);
    if !dir.exists() {
        return None;
    }
    let mut latest_name: Option<String> = None;
    let mut entries = std::fs::read_dir(&dir).ok()?;
    while let Some(Ok(entry)) = entries.next() {
        if !entry
            .file_type()
            .ok()
            .map(|kind| kind.is_file())
            .unwrap_or(false)
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".json") {
            continue;
        }
        let should_replace = match latest_name.as_ref() {
            Some(current) => name > *current,
            None => true,
        };
        if should_replace {
            latest_name = Some(name);
        }
    }
    let file_name = latest_name?;
    let raw = std::fs::read_to_string(dir.join(file_name)).ok()?;
    serde_json::from_str::<ContextCheckpointRecord>(&raw).ok()
}

pub(super) fn context_run_replay_materialize(
    run_id: &str,
    persisted: &ContextRunState,
    checkpoint: Option<&ContextCheckpointRecord>,
    events: &[ContextRunEventRecord],
) -> ContextRunState {
    let mut replay = if let Some(cp) = checkpoint {
        cp.run_state.clone()
    } else {
        let mut base = persisted.clone();
        base.status = ContextRunStatus::Queued;
        base.why_next_step = None;
        base.revision = 1;
        base.updated_at_ms = base.created_at_ms;
        for step in &mut base.steps {
            step.status = ContextStepStatus::Pending;
        }
        base
    };
    replay.run_id = run_id.to_string();
    let mut step_index: HashMap<String, usize> = replay
        .steps
        .iter()
        .enumerate()
        .map(|(idx, step)| (step.step_id.clone(), idx))
        .collect();

    for event in events {
        replay.status = event.status.clone();
        replay.updated_at_ms = replay.updated_at_ms.max(event.ts_ms);
        if let Some(why) = event.payload.get("why_next_step").and_then(Value::as_str) {
            if !why.trim().is_empty() {
                replay.why_next_step = Some(why.to_string());
            }
        }
        if let Some(items) = event.payload.get("steps").and_then(Value::as_array) {
            for item in items {
                let Some(step_id) = item.get("step_id").and_then(Value::as_str) else {
                    continue;
                };
                let step_title = item
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or(step_id)
                    .to_string();
                let step_status = item
                    .get("status")
                    .and_then(Value::as_str)
                    .and_then(parse_context_step_status_text)
                    .unwrap_or(ContextStepStatus::Pending);
                let index = if let Some(existing) = step_index.get(step_id).copied() {
                    existing
                } else {
                    replay.steps.push(ContextRunStep {
                        step_id: step_id.to_string(),
                        title: step_title.clone(),
                        status: ContextStepStatus::Pending,
                    });
                    let idx = replay.steps.len().saturating_sub(1);
                    step_index.insert(step_id.to_string(), idx);
                    idx
                };
                replay.steps[index].title = step_title;
                replay.steps[index].status = step_status;
            }
        }
        if let Some(step_id) = &event.step_id {
            let index = if let Some(existing) = step_index.get(step_id).copied() {
                existing
            } else {
                replay.steps.push(ContextRunStep {
                    step_id: step_id.clone(),
                    title: event
                        .payload
                        .get("step_title")
                        .and_then(Value::as_str)
                        .unwrap_or(step_id.as_str())
                        .to_string(),
                    status: ContextStepStatus::Pending,
                });
                let idx = replay.steps.len().saturating_sub(1);
                step_index.insert(step_id.clone(), idx);
                idx
            };
            if let Some(step_status) = replay_step_status_from_event(event) {
                replay.steps[index].status = step_status;
            }
        }
    }

    let checkpoint_revision = checkpoint.map(|cp| cp.run_state.revision).unwrap_or(1);
    replay.revision = checkpoint_revision.saturating_add(events.len() as u64);
    replay
}

pub(super) fn context_blackboard_replay_materialize(
    persisted: &ContextBlackboardState,
    checkpoint: Option<&ContextCheckpointRecord>,
    patches: &[ContextBlackboardPatchRecord],
) -> ContextBlackboardState {
    let mut replay = checkpoint
        .map(|cp| cp.blackboard.clone())
        .unwrap_or_else(ContextBlackboardState::default);
    if checkpoint.is_none() && replay.revision == 0 && persisted.revision > 0 {
        replay.revision = 0;
    }
    for patch in patches {
        let _ = apply_context_blackboard_patch(&mut replay, patch);
    }
    replay.tasks = persisted.tasks.clone();
    replay
}

pub(super) async fn context_run_replay(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<ContextRunReplayQuery>,
) -> Result<Json<Value>, StatusCode> {
    let persisted = load_context_run_state(&state, &run_id).await?;
    let from_checkpoint = query.from_checkpoint.unwrap_or(true);
    let checkpoint = if from_checkpoint {
        latest_context_checkpoint_record(&state, &run_id)
    } else {
        None
    };
    let start_seq = checkpoint.as_ref().map(|cp| cp.seq).unwrap_or(0);
    let mut events = load_context_run_events_jsonl(
        &context_run_events_path(&state, &run_id),
        Some(start_seq),
        None,
    );
    if let Some(upto_seq) = query.upto_seq {
        events.retain(|row| row.seq <= upto_seq);
    }
    let replay = context_run_replay_materialize(&run_id, &persisted, checkpoint.as_ref(), &events);
    let persisted_blackboard = load_projected_context_blackboard(&state, &run_id);
    let mut patches = load_context_blackboard_patches(&state, &run_id, None, None);
    if let Some(cp) = checkpoint.as_ref() {
        patches.retain(|patch| patch.ts_ms > cp.ts_ms);
    }
    let replay_blackboard =
        context_blackboard_replay_materialize(&persisted_blackboard, checkpoint.as_ref(), &patches);
    let blackboard_revision_mismatch = replay_blackboard.revision != persisted_blackboard.revision;
    let blackboard_task_count_mismatch =
        replay_blackboard.tasks.len() != persisted_blackboard.tasks.len();
    let mut replay_task_status = HashMap::<String, ContextBlackboardTaskStatus>::new();
    for task in &replay_blackboard.tasks {
        replay_task_status.insert(task.id.clone(), task.status.clone());
    }
    let blackboard_task_status_mismatch = persisted_blackboard.tasks.iter().any(|task| {
        replay_task_status
            .get(&task.id)
            .map(|status| status != &task.status)
            .unwrap_or(true)
    });
    let status_mismatch = replay.status != persisted.status;
    let why_next_step_mismatch =
        persisted.why_next_step.is_some() && replay.why_next_step != persisted.why_next_step;
    let step_count_mismatch =
        !persisted.steps.is_empty() && replay.steps.len() != persisted.steps.len();
    let drift = ContextReplayDrift {
        mismatch: status_mismatch
            || why_next_step_mismatch
            || step_count_mismatch
            || blackboard_revision_mismatch
            || blackboard_task_count_mismatch
            || blackboard_task_status_mismatch,
        status_mismatch,
        why_next_step_mismatch,
        step_count_mismatch,
        blackboard_revision_mismatch,
        blackboard_task_count_mismatch,
        blackboard_task_status_mismatch,
    };
    Ok(Json(json!({
        "ok": true,
        "run_id": run_id,
        "from_checkpoint": checkpoint.is_some(),
        "checkpoint_seq": checkpoint.as_ref().map(|cp| cp.seq),
        "events_applied": events.len(),
        "blackboard_patches_applied": patches.len(),
        "replay": replay,
        "replay_blackboard": replay_blackboard,
        "persisted": persisted,
        "persisted_blackboard": persisted_blackboard,
        "drift": drift
    })))
}

pub(crate) async fn sync_automation_v2_run_blackboard(
    state: &AppState,
    automation: &crate::AutomationV2Spec,
    run: &crate::AutomationV2RunRecord,
) -> Result<(), StatusCode> {
    let run_id = automation_v2_context_run_id(&run.run_id);

    if load_context_run_state(state, &run_id).await.is_err() {
        let now = crate::now_ms();
        let context_run = ContextRunState {
            run_id: run_id.clone(),
            run_type: "automation_v2".to_string(),
            tenant_context: TenantContext::local_implicit(),
            source_client: Some("automation_v2_scheduler".to_string()),
            model_provider: None,
            model_id: None,
            mcp_servers: Vec::new(),
            status: automation_run_status_to_context(&run.status),
            objective: automation
                .description
                .clone()
                .unwrap_or_else(|| automation.name.clone()),
            workspace: ContextWorkspaceLease::default(),
            steps: automation
                .flow
                .nodes
                .iter()
                .map(|node| ContextRunStep {
                    step_id: node.node_id.clone(),
                    title: node.objective.clone(),
                    status: ContextStepStatus::Pending,
                })
                .collect(),
            tasks: Vec::new(),
            why_next_step: Some("Track automation v2 flow via blackboard tasks".to_string()),
            revision: 1,
            last_event_seq: 0,
            created_at_ms: now,
            started_at_ms: Some(now),
            ended_at_ms: if matches!(
                run.status,
                crate::AutomationRunStatus::Completed
                    | crate::AutomationRunStatus::Failed
                    | crate::AutomationRunStatus::Cancelled
            ) {
                Some(now)
            } else {
                None
            },
            last_error: run.detail.clone(),
            updated_at_ms: now,
        };
        save_context_run_state(state, &context_run).await?;
    }

    let mut run_state = load_context_run_state(state, &run_id).await?;
    let now = crate::now_ms();

    for node in &automation.flow.nodes {
        let task_id = format!("node-{}", node.node_id);
        let depends_on = node
            .depends_on
            .iter()
            .map(|dep| format!("node-{dep}"))
            .collect::<Vec<_>>();

        let status = automation_node_task_status(run, &node.node_id, &depends_on);

        let output = run.checkpoint.node_outputs.get(&node.node_id);
        let payload = automation_node_task_payload(node, output);
        let attempt = run
            .checkpoint
            .node_attempts
            .get(&node.node_id)
            .copied()
            .unwrap_or(0);
        let max_attempts = crate::app::state::automation_node_max_attempts(node);
        let last_error = output
            .and_then(|value| {
                value
                    .get("blocked_reason")
                    .and_then(Value::as_str)
                    .or_else(|| {
                        value
                            .get("validator_summary")
                            .and_then(|summary| summary.get("reason"))
                            .and_then(Value::as_str)
                    })
                    .or_else(|| {
                        value
                            .get("artifact_validation")
                            .and_then(|validation| validation.get("semantic_block_reason"))
                            .and_then(Value::as_str)
                    })
            })
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        let existing = run_state.tasks.iter().find(|t| t.id == task_id).cloned();
        if let Some(task) = existing {
            if task.status != status
                || task.payload != payload
                || task.attempt != attempt
                || task.max_attempts != max_attempts
                || task.last_error != last_error
            {
                let next_task = ContextBlackboardTask {
                    payload: payload.clone(),
                    status: status.clone(),
                    attempt,
                    max_attempts,
                    last_error: last_error.clone(),
                    task_rev: task.task_rev.saturating_add(1),
                    updated_ts: now,
                    ..task.clone()
                };
                let event_type = if task.status != status {
                    context_task_status_event_name(&status).to_string()
                } else {
                    "context.task.updated".to_string()
                };
                let _ = context_run_engine()
                    .commit_task_mutation(
                        state,
                        &run_id,
                        next_task.clone(),
                        ContextBlackboardPatchOp::UpdateTaskState,
                        json!({
                            "task_id": task_id,
                            "status": status,
                            "payload": payload,
                            "attempt": attempt,
                            "max_attempts": max_attempts,
                            "last_error": last_error,
                            "task_rev": next_task.task_rev,
                        }),
                        event_type,
                        automation_run_status_to_context(&run.status),
                        None,
                        json!({
                            "task_id": task_id,
                            "status": status,
                            "payload": next_task.payload,
                            "attempt": next_task.attempt,
                            "max_attempts": next_task.max_attempts,
                            "last_error": next_task.last_error,
                            "task_rev": next_task.task_rev,
                            "source": "automation_v2",
                            "automation_id": automation.automation_id,
                        }),
                    )
                    .await?;
                if let Some(existing_task) =
                    run_state.tasks.iter_mut().find(|row| row.id == task_id)
                {
                    *existing_task = next_task.clone();
                }
            }
        } else {
            let task = ContextBlackboardTask {
                id: task_id.clone(),
                task_type: "automation_node".to_string(),
                payload,
                status,
                workflow_id: Some(automation.automation_id.clone()),
                workflow_node_id: Some(node.node_id.clone()),
                parent_task_id: None,
                depends_on_task_ids: depends_on,
                decision_ids: Vec::new(),
                artifact_ids: Vec::new(),
                assigned_agent: Some(node.agent_id.clone()),
                priority: 0,
                attempt,
                max_attempts,
                last_error,
                next_retry_at_ms: None,
                lease_owner: None,
                lease_token: None,
                lease_expires_at_ms: None,
                task_rev: 1,
                created_ts: now,
                updated_ts: now,
            };
            let _ = context_run_engine()
                .commit_task_mutation(
                    state,
                    &run_id,
                    task.clone(),
                    ContextBlackboardPatchOp::AddTask,
                    serde_json::to_value(&task).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                    "context.task.created".to_string(),
                    automation_run_status_to_context(&run.status),
                    None,
                    json!({
                        "task_id": task.id,
                        "task_type": task.task_type,
                        "task_rev": task.task_rev,
                        "source": "automation_v2",
                        "automation_id": automation.automation_id,
                    }),
                )
                .await?;
            run_state.tasks.push(task.clone());
        }
    }

    for node in &automation.flow.nodes {
        for projected_task in parse_backlog_projection_tasks(automation, node, run, now) {
            let existing = run_state
                .tasks
                .iter()
                .find(|task| task.id == projected_task.id)
                .cloned();
            let needs_update = match existing.as_ref() {
                Some(task) => {
                    serde_json::to_value(task).ok() != serde_json::to_value(&projected_task).ok()
                }
                None => true,
            };
            if !needs_update {
                continue;
            }
            let payload = serde_json::to_value(&projected_task)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let event_name = if existing.is_some() {
                "context.task.projected".to_string()
            } else {
                "context.task.created".to_string()
            };
            let _ = context_run_engine()
                .commit_task_mutation(
                    state,
                    &run_id,
                    projected_task.clone(),
                    ContextBlackboardPatchOp::AddTask,
                    payload,
                    event_name,
                    automation_run_status_to_context(&run.status),
                    None,
                    json!({
                        "task_id": projected_task.id,
                        "task_type": projected_task.task_type,
                        "task_rev": projected_task.task_rev,
                        "source": "automation_v2_backlog_projection",
                        "automation_id": automation.automation_id,
                        "workflow_node_id": node.node_id,
                    }),
                )
                .await?;
            if let Some(existing_task) = run_state
                .tasks
                .iter_mut()
                .find(|task| task.id == projected_task.id)
            {
                *existing_task = projected_task.clone();
            } else {
                run_state.tasks.push(projected_task.clone());
            }
        }
    }

    Ok(())
}

pub(super) fn automation_v2_context_run_id(run_id: &str) -> String {
    format!("automation-v2-{run_id}")
}

pub(crate) fn routine_context_run_id(run_id: &str) -> String {
    format!("routine-{run_id}")
}

pub(crate) fn session_context_run_id(session_id: &str) -> String {
    format!("session-{session_id}")
}

pub(crate) fn session_run_status_to_context(status: &str) -> ContextRunStatus {
    match status.trim().to_ascii_lowercase().as_str() {
        "completed" => ContextRunStatus::Completed,
        "cancelled" => ContextRunStatus::Cancelled,
        "timeout" | "error" | "failed" => ContextRunStatus::Failed,
        _ => ContextRunStatus::Running,
    }
}

pub(crate) async fn ensure_session_context_run(
    state: &AppState,
    session: &tandem_types::Session,
) -> Result<String, StatusCode> {
    let run_id = session_context_run_id(&session.id);
    if load_context_run_state(state, &run_id).await.is_ok() {
        return Ok(run_id);
    }
    let now = crate::now_ms();
    let workspace = session
        .workspace_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|path| ContextWorkspaceLease {
            workspace_id: session
                .project_id
                .clone()
                .unwrap_or_else(|| session.id.clone()),
            canonical_path: path.to_string(),
            lease_epoch: 0,
        })
        .unwrap_or_default();
    let run = ContextRunState {
        run_id: run_id.clone(),
        run_type: "session".to_string(),
        tenant_context: session.tenant_context.clone(),
        source_client: Some("session_api".to_string()),
        model_provider: session.provider.clone(),
        model_id: session.model.as_ref().map(|model| model.model_id.clone()),
        mcp_servers: Vec::new(),
        status: ContextRunStatus::Queued,
        objective: {
            let title = session.title.trim();
            if title.is_empty() {
                format!("Interactive session {}", session.id)
            } else {
                format!("Interactive session: {title}")
            }
        },
        workspace,
        steps: vec![ContextRunStep {
            step_id: "session-run".to_string(),
            title: "Execute interactive session work".to_string(),
            status: ContextStepStatus::Pending,
        }],
        tasks: Vec::new(),
        why_next_step: Some("waiting for session run activity".to_string()),
        revision: 1,
        last_event_seq: 0,
        created_at_ms: now,
        started_at_ms: None,
        ended_at_ms: None,
        last_error: None,
        updated_at_ms: now,
    };
    save_context_run_state(state, &run).await?;
    Ok(run_id)
}

pub(crate) fn workflow_context_run_id(run_id: &str) -> String {
    format!("workflow-{run_id}")
}

pub(super) fn automation_run_status_to_context(
    status: &crate::AutomationRunStatus,
) -> ContextRunStatus {
    match status {
        crate::AutomationRunStatus::Queued => ContextRunStatus::Queued,
        crate::AutomationRunStatus::Running
        | crate::AutomationRunStatus::Pausing
        | crate::AutomationRunStatus::Paused
        | crate::AutomationRunStatus::AwaitingApproval => ContextRunStatus::Running,
        crate::AutomationRunStatus::Completed => ContextRunStatus::Completed,
        crate::AutomationRunStatus::Blocked => ContextRunStatus::Blocked,
        crate::AutomationRunStatus::Failed => ContextRunStatus::Failed,
        crate::AutomationRunStatus::Cancelled => ContextRunStatus::Cancelled,
    }
}

fn routine_run_status_to_context(status: &crate::RoutineRunStatus) -> ContextRunStatus {
    match status {
        crate::RoutineRunStatus::Queued => ContextRunStatus::Queued,
        crate::RoutineRunStatus::PendingApproval
        | crate::RoutineRunStatus::Running
        | crate::RoutineRunStatus::Paused => ContextRunStatus::Running,
        crate::RoutineRunStatus::BlockedPolicy => ContextRunStatus::Blocked,
        crate::RoutineRunStatus::Denied => ContextRunStatus::Cancelled,
        crate::RoutineRunStatus::Completed => ContextRunStatus::Completed,
        crate::RoutineRunStatus::Failed => ContextRunStatus::Failed,
        crate::RoutineRunStatus::Cancelled => ContextRunStatus::Cancelled,
    }
}
