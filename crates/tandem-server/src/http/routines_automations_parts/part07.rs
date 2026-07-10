// Automation wire conversion helpers split from part01.rs for the file-size gate.

pub(super) async fn routines_run_artifacts(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(run) = routine_run_for_tenant(&state, &run_id, &tenant_context).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine run not found",
                "code": "ROUTINE_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        ));
    };
    let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &run)
        .await
        .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run_id));
    Ok(Json(json!({
        "runID": run_id,
        "artifacts": run.artifacts,
        "count": run.artifacts.len(),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn routines_run_artifact_add(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunArtifactInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if routine_run_for_tenant(&state, &run_id, &tenant_context)
        .await
        .is_none()
    {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Routine run not found","code":"ROUTINE_RUN_NOT_FOUND","runID":run_id}),
            ),
        ));
    }
    if input.uri.trim().is_empty() || input.kind.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error":"Artifact requires uri and kind",
                "code":"ROUTINE_ARTIFACT_INVALID",
            })),
        ));
    }
    let artifact = RoutineRunArtifact {
        artifact_id: format!("artifact-{}", Uuid::new_v4()),
        uri: input.uri.trim().to_string(),
        kind: input.kind.trim().to_string(),
        label: input
            .label
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        created_at_ms: crate::now_ms(),
        metadata: input.metadata,
    };
    let updated = state
        .append_routine_run_artifact_for_tenant(&run_id, &tenant_context, artifact.clone())
        .await
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error":"Routine run not found",
                    "code":"ROUTINE_RUN_NOT_FOUND",
                    "runID": run_id,
                })),
            )
        })?;
    state
        .event_bus
        .publish(crate::routines::types::tenant_scoped_engine_event(
            "routine.run.artifact_added",
            &updated.tenant_context,
            json!({
                "runID": run_id,
                "routineID": updated.routine_id,
                "artifact": artifact,
            }),
        ));
    let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &updated)
        .await
        .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run_id));
    Ok(Json(json!({
        "ok": true,
        "run": routine_run_with_context_links(&updated),
        "artifact": artifact,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

fn routines_sse_stream(
    state: AppState,
    routine_id: Option<String>,
    tenant_context: TenantContext,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let ready = tokio_stream::once(Ok(Event::default().data(
        serde_json::to_string(&json!({
            "status": "ready",
            "stream": "routines",
            "timestamp_ms": crate::now_ms(),
        }))
        .unwrap_or_default(),
    )));
    let rx = state.event_bus.subscribe();
    let live = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(event) => {
            if !event.event_type.starts_with("routine.") {
                return None;
            }
            if !super::global::event_visible_to_tenant(&event, &tenant_context) {
                return None;
            }
            let event_routine_id = event
                .properties
                .get("routineID")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if let Some(routine_id) = routine_id.as_deref() {
                if event_routine_id != routine_id {
                    return None;
                }
            }
            let payload = serde_json::to_string(&event).unwrap_or_default();
            Some(Ok(Event::default().data(payload)))
        }
        Err(_) => None,
    });
    ready.chain(live)
}

pub(super) async fn routines_events(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Query(query): Query<RoutineEventsQuery>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    Sse::new(routines_sse_stream(state, query.routine_id, tenant_context))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
}

pub(super) fn routine_to_automation_wire(routine: RoutineSpec) -> Value {
    json!({
        "automation_id": routine.routine_id,
        "name": routine.name,
        "status": routine.status,
        "schedule": routine.schedule,
        "timezone": routine.timezone,
        "misfire_policy": routine.misfire_policy,
        "mode": mode_from_args(&routine.args),
        "mission": {
            "objective": objective_from_args(&routine.args, &routine.routine_id, &routine.entrypoint),
            "success_criteria": success_criteria_from_args(&routine.args),
            "briefing": routine.args.get("briefing").cloned(),
            "entrypoint_compat": routine.entrypoint,
        },
        "policy": {
            "tool": {
                "run_allowlist": routine.allowed_tools,
                "external_integrations_allowed": routine.external_integrations_allowed,
                "orchestrator_only_tool_calls": routine
                    .args
                    .get("orchestrator_only_tool_calls")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            },
            "approval": {
                "requires_approval": routine.requires_approval
            }
        },
        "model_policy": routine.args.get("model_policy").cloned(),
        "output_targets": routine.output_targets,
        "creator_type": routine.creator_type,
        "creator_id": routine.creator_id,
        "next_fire_at_ms": routine.next_fire_at_ms,
        "last_fired_at_ms": routine.last_fired_at_ms
    })
}

pub(super) fn routine_run_to_automation_wire(run: RoutineRunRecord) -> Value {
    let context_run_id = super::context_runs::routine_context_run_id(&run.run_id);
    let latest_session_id = run
        .latest_session_id
        .clone()
        .or_else(|| run.active_session_ids.last().cloned());
    let attach_event_stream = latest_session_id
        .as_ref()
        .map(|session_id| format!("/event?sessionID={session_id}&runID={}", run.run_id));
    json!({
        "run_id": run.run_id,
        "automation_id": run.routine_id,
        "trigger_type": run.trigger_type,
        "run_count": run.run_count,
        "status": run.status,
        "created_at_ms": run.created_at_ms,
        "updated_at_ms": run.updated_at_ms,
        "fired_at_ms": run.fired_at_ms,
        "started_at_ms": run.started_at_ms,
        "finished_at_ms": run.finished_at_ms,
        "mode": mode_from_args(&run.args),
        "mission_snapshot": {
            "objective": objective_from_args(&run.args, &run.routine_id, &run.entrypoint),
            "success_criteria": success_criteria_from_args(&run.args),
            "entrypoint_compat": run.entrypoint,
        },
        "policy_snapshot": {
            "tool": {
                "run_allowlist": run.allowed_tools,
            },
            "approval": {
                "requires_approval": run.requires_approval
            }
        },
        "model_policy": run.args.get("model_policy").cloned(),
        "requires_approval": run.requires_approval,
        "approval_reason": run.approval_reason,
        "denial_reason": run.denial_reason,
        "paused_reason": run.paused_reason,
        "detail": run.detail,
        "output_targets": run.output_targets,
        "artifacts": run.artifacts,
        "correlation_id": run.run_id,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
        "active_session_ids": run.active_session_ids,
        "latest_session_id": latest_session_id,
        "attach_event_stream": attach_event_stream,
    })
}

pub(super) fn routine_event_to_run_event(event: &EngineEvent) -> Option<EngineEvent> {
    let mut props = event.properties.clone();
    let event_type = match event.event_type.as_str() {
        "routine.run.created" => "run.started",
        "routine.run.started" => "run.step",
        "routine.run.completed" => "run.completed",
        "routine.run.failed" => "run.failed",
        "routine.approval_required" => "approval.required",
        "routine.run.artifact_added" => "run.step",
        "routine.run.model_selected" => "run.step",
        "routine.blocked" => "run.failed",
        _ => return None,
    };
    if let Some(routine_id) = props
        .get("routineID")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
    {
        props
            .as_object_mut()
            .expect("object")
            .insert("automationID".to_string(), Value::String(routine_id));
    }
    if event.event_type == "routine.run.started"
        || event.event_type == "routine.run.artifact_added"
        || event.event_type == "routine.run.model_selected"
    {
        props
            .as_object_mut()
            .expect("object")
            .insert("phase".to_string(), Value::String("do".to_string()));
    }
    Some(EngineEvent::new(event_type, props))
}
