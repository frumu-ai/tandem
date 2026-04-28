fn merge_automation_capabilities_metadata(
    metadata: Option<Value>,
    capabilities: Option<crate::automation_v2::governance::AutomationDeclaredCapabilities>,
) -> Result<Option<Value>, (StatusCode, Json<Value>)> {
    let Some(capabilities) = capabilities else {
        return Ok(metadata);
    };
    match metadata {
        None => Ok(Some(json!({ "capabilities": capabilities }))),
        Some(Value::Object(mut map)) => {
            map.insert(
                "capabilities".to_string(),
                serde_json::to_value(capabilities).unwrap_or_else(|_| json!({})),
            );
            Ok(Some(Value::Object(map)))
        }
        Some(_) => Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "metadata must be an object when capabilities are declared",
                "code": "AUTOMATION_V2_INVALID_METADATA",
            })),
        )),
    }
}

pub(super) async fn automations_patch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<AutomationPatchInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut routine = state.get_routine(&id).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Automation not found",
                "code":"AUTOMATION_NOT_FOUND",
                "automationID": id,
            })),
        )
    })?;
    if let Some(name) = input.name.as_ref() {
        routine.name = name.clone();
    }
    if let Some(status) = input.status.as_ref() {
        routine.status = status.clone();
    }
    if let Some(schedule) = input.schedule.as_ref() {
        routine.schedule = schedule.clone();
    }
    if let Some(timezone) = input.timezone.as_ref() {
        routine.timezone = timezone.clone();
    }
    if let Some(misfire_policy) = input.misfire_policy.as_ref() {
        routine.misfire_policy = misfire_policy.clone();
    }
    if let Some(next_fire_at_ms) = input.next_fire_at_ms {
        routine.next_fire_at_ms = Some(next_fire_at_ms);
    }
    if let Some(output_targets) = input.output_targets.as_ref() {
        routine.output_targets = output_targets.clone();
    }
    if let Some(model_policy) = input.model_policy.as_ref() {
        let mut args = routine.args.as_object().cloned().unwrap_or_default();
        if model_policy
            .as_object()
            .map(|obj| obj.is_empty())
            .unwrap_or(false)
        {
            args.remove("model_policy");
        } else if model_policy.is_object() {
            validate_model_policy(model_policy).map_err(|detail| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "Invalid automation patch",
                        "code": "AUTOMATION_INVALID",
                        "detail": detail,
                    })),
                )
            })?;
            args.insert("model_policy".to_string(), model_policy.clone());
        } else {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Invalid automation patch",
                    "code": "AUTOMATION_INVALID",
                    "detail": "model_policy must be an object (use {} to clear)",
                })),
            ));
        }
        routine.args = Value::Object(args);
    }
    if let Some(policy) = input.policy.as_ref() {
        if let Some(allowed) = policy.tool.run_allowlist.as_ref() {
            routine.allowed_tools = allowed.clone();
        }
        if let Some(external_allowed) = policy.tool.external_integrations_allowed {
            routine.external_integrations_allowed = external_allowed;
        }
        if let Some(requires_approval) = policy.approval.requires_approval {
            routine.requires_approval = requires_approval;
        }
        if let Some(orchestrator_only) = policy.tool.orchestrator_only_tool_calls {
            let mut args = routine.args.as_object().cloned().unwrap_or_default();
            args.insert(
                "orchestrator_only_tool_calls".to_string(),
                Value::Bool(orchestrator_only),
            );
            routine.args = Value::Object(args);
        }
    }
    if let Some(mode) = input.mode.as_ref() {
        let normalized_mode = normalize_automation_mode(Some(mode.as_str())).map_err(|detail| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Invalid automation patch",
                    "code": "AUTOMATION_INVALID",
                    "detail": detail,
                })),
            )
        })?;
        let mut args = routine.args.as_object().cloned().unwrap_or_default();
        args.insert("mode".to_string(), Value::String(normalized_mode));
        routine.args = Value::Object(args);
    }
    if let Some(mission) = input.mission.as_ref() {
        let mut args = routine.args.as_object().cloned().unwrap_or_default();
        if let Some(objective) = mission.objective.as_ref() {
            args.insert("prompt".to_string(), Value::String(objective.clone()));
        }
        if let Some(success_criteria) = mission.success_criteria.as_ref() {
            args.insert("success_criteria".to_string(), json!(success_criteria));
        }
        if let Some(briefing) = mission.briefing.as_ref() {
            args.insert("briefing".to_string(), Value::String(briefing.clone()));
        }
        if let Some(entrypoint) = mission.entrypoint_compat.as_ref() {
            routine.entrypoint = entrypoint.clone();
        }
        routine.args = Value::Object(args);
    }
    let updated = state
        .put_routine(routine)
        .await
        .map_err(routine_error_response)?;
    Ok(Json(json!({
        "automation": routine_to_automation_wire(updated)
    })))
}

pub(super) async fn automations_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let deleted = state
        .delete_routine(&id)
        .await
        .map_err(routine_error_response)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error":"Automation not found",
                    "code":"AUTOMATION_NOT_FOUND",
                    "automationID": id,
                })),
            )
        })?;
    Ok(Json(json!({
        "ok": true,
        "automation": routine_to_automation_wire(deleted)
    })))
}

pub(super) async fn automations_run_now(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<RoutineRunNowInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_now(State(state.clone()), Path(id), Json(input)).await?;
    let payload = response.0;
    let run_id = payload
        .get("runID")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Run ID missing", "code": "AUTOMATION_RUN_MAPPING_FAILED"})),
            )
        })?;
    let run = state.get_routine_run(run_id).await.ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Run lookup failed", "code": "AUTOMATION_RUN_MAPPING_FAILED"})),
        )
    })?;
    let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &run)
        .await
        .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run.run_id));
    Ok(Json(json!({
        "ok": true,
        "status": payload.get("status").cloned().unwrap_or(Value::String("queued".to_string())),
        "run": routine_run_to_automation_wire(run),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<RoutineHistoryQuery>,
) -> Json<Value> {
    let response = routines_history(State(state), Path(id.clone()), Query(query)).await;
    let mut payload = response.0;
    if let Some(object) = payload.as_object_mut() {
        object.insert("automationID".to_string(), Value::String(id));
        object.remove("routineID");
    }
    Json(payload)
}

pub(super) async fn automations_runs(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<RoutineRunsQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(25).clamp(1, 200);
    let runs = state.list_routine_runs(Some(&id), limit).await;
    for run in &runs {
        let _ = super::context_runs::sync_routine_run_blackboard(&state, run).await;
    }
    let rows = runs
        .into_iter()
        .map(routine_run_to_automation_wire)
        .collect::<Vec<_>>();
    Json(json!({
        "runs": rows,
        "count": rows.len(),
    }))
}

pub(super) async fn automations_runs_all(
    State(state): State<AppState>,
    Query(query): Query<RoutineRunsQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(25).clamp(1, 200);
    let runs = state
        .list_routine_runs(query.routine_id.as_deref(), limit)
        .await;
    for run in &runs {
        let _ = super::context_runs::sync_routine_run_blackboard(&state, run).await;
    }
    let rows = runs
        .into_iter()
        .map(routine_run_to_automation_wire)
        .collect::<Vec<_>>();
    Json(json!({
        "runs": rows,
        "count": rows.len(),
    }))
}

pub(super) async fn automations_run_get(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let run = state.get_routine_run(&run_id).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Automation run not found",
                "code":"AUTOMATION_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        )
    })?;
    let context_run_id = super::context_runs::sync_routine_run_blackboard(&state, &run)
        .await
        .unwrap_or_else(|_| super::context_runs::routine_context_run_id(&run.run_id));
    Ok(Json(json!({
        "run": routine_run_to_automation_wire(run),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_run_approve(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_approve(State(state), Path(run_id), Json(input)).await?;
    let run = response
        .0
        .get("run")
        .and_then(|v| serde_json::from_value::<RoutineRunRecord>(v.clone()).ok())
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error": "Run mapping failed", "code": "AUTOMATION_RUN_MAPPING_FAILED"}),
                ),
            )
        })?;
    let context_run_id = response
        .0
        .get("contextRunID")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| super::context_runs::routine_context_run_id(&run.run_id));
    Ok(Json(json!({
        "ok": true,
        "run": routine_run_to_automation_wire(run),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_run_deny(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_deny(State(state), Path(run_id), Json(input)).await?;
    let run = response
        .0
        .get("run")
        .and_then(|v| serde_json::from_value::<RoutineRunRecord>(v.clone()).ok())
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error": "Run mapping failed", "code": "AUTOMATION_RUN_MAPPING_FAILED"}),
                ),
            )
        })?;
    let context_run_id = response
        .0
        .get("contextRunID")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| super::context_runs::routine_context_run_id(&run.run_id));
    Ok(Json(json!({
        "ok": true,
        "run": routine_run_to_automation_wire(run),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_run_pause(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_pause(State(state), Path(run_id), Json(input)).await?;
    let run = response
        .0
        .get("run")
        .and_then(|v| serde_json::from_value::<RoutineRunRecord>(v.clone()).ok())
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error": "Run mapping failed", "code": "AUTOMATION_RUN_MAPPING_FAILED"}),
                ),
            )
        })?;
    let context_run_id = response
        .0
        .get("contextRunID")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| super::context_runs::routine_context_run_id(&run.run_id));
    Ok(Json(json!({
        "ok": true,
        "run": routine_run_to_automation_wire(run),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_run_resume(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_resume(State(state), Path(run_id), Json(input)).await?;
    let run = response
        .0
        .get("run")
        .and_then(|v| serde_json::from_value::<RoutineRunRecord>(v.clone()).ok())
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error": "Run mapping failed", "code": "AUTOMATION_RUN_MAPPING_FAILED"}),
                ),
            )
        })?;
    let context_run_id = response
        .0
        .get("contextRunID")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| super::context_runs::routine_context_run_id(&run.run_id));
    Ok(Json(json!({
        "ok": true,
        "run": routine_run_to_automation_wire(run),
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_run_artifacts(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_artifacts(State(state), Path(run_id.clone())).await?;
    let mut payload = response.0;
    if let Some(object) = payload.as_object_mut() {
        object.insert("automationRunID".to_string(), Value::String(run_id));
        object.remove("runID");
    }
    Ok(Json(payload))
}

pub(super) async fn automations_run_artifact_add(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunArtifactInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_artifact_add(State(state), Path(run_id), Json(input)).await?;
    let run = response
        .0
        .get("run")
        .and_then(|v| serde_json::from_value::<RoutineRunRecord>(v.clone()).ok())
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error": "Run mapping failed", "code": "AUTOMATION_RUN_MAPPING_FAILED"}),
                ),
            )
        })?;
    let context_run_id = response
        .0
        .get("contextRunID")
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| super::context_runs::routine_context_run_id(&run.run_id));
    let artifact = response
        .0
        .get("artifact")
        .cloned()
        .unwrap_or_else(|| json!({}));
    Ok(Json(json!({
        "ok": true,
        "run": routine_run_to_automation_wire(run),
        "artifact": artifact,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

fn automations_sse_stream(
    state: AppState,
    automation_id: Option<String>,
    run_id: Option<String>,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let ready = tokio_stream::once(Ok(Event::default().data(
        serde_json::to_string(&json!({
            "status": "ready",
            "stream": "automations",
            "timestamp_ms": crate::now_ms(),
        }))
        .unwrap_or_default(),
    )));
    let rx = state.event_bus.subscribe();
    let live = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(event) => {
            let mapped = routine_event_to_run_event(&event)?;
            if let Some(automation_id) = automation_id.as_deref() {
                let event_automation_id = mapped
                    .properties
                    .get("automationID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if event_automation_id != automation_id {
                    return None;
                }
            }
            if let Some(run_id) = run_id.as_deref() {
                let event_run_id = mapped
                    .properties
                    .get("runID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if event_run_id != run_id {
                    return None;
                }
            }
            let payload = serde_json::to_string(&mapped).unwrap_or_default();
            Some(Ok(Event::default().data(payload)))
        }
        Err(_) => None,
    });
    ready.chain(live)
}

pub(super) async fn automations_events(
    State(state): State<AppState>,
    Query(query): Query<AutomationEventsQuery>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    Sse::new(automations_sse_stream(
        state,
        query.automation_id,
        query.run_id,
    ))
    .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
}

pub(super) fn normalize_automation_v2_agent(
    mut agent: AutomationAgentProfile,
) -> AutomationAgentProfile {
    if agent.display_name.trim().is_empty() {
        agent.display_name = agent.agent_id.clone();
    }
    if agent.tool_policy.allowlist.is_empty() {
        agent.tool_policy = AutomationAgentToolPolicy {
            allowlist: vec!["read".to_string()],
            denylist: Vec::new(),
        };
    }
    if agent.mcp_policy.allowed_servers.is_empty() {
        agent.mcp_policy = AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
        };
    }
    agent
}

fn normalize_sorted_strings(values: &[String]) -> Vec<String> {
    let mut values = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn removed_strings(before: &[String], after: &[String]) -> Vec<String> {
    let after = after.iter().collect::<std::collections::HashSet<_>>();
    let mut removed = before
        .iter()
        .filter(|value| !after.contains(value))
        .cloned()
        .collect::<Vec<_>>();
    removed.sort();
    removed.dedup();
    removed
}

fn mcp_policy_dependency_revocation_details(
    before_agents: &[AutomationAgentProfile],
    after_agents: &[AutomationAgentProfile],
) -> Option<Value> {
    let before_map = before_agents
        .iter()
        .map(|agent| (&agent.agent_id, agent))
        .collect::<std::collections::HashMap<_, _>>();
    let after_map = after_agents
        .iter()
        .map(|agent| (&agent.agent_id, agent))
        .collect::<std::collections::HashMap<_, _>>();

    let mut changes = Vec::new();
    for (agent_id, previous) in before_map {
        let Some(next) = after_map.get(agent_id) else {
            changes.push(json!({
                "agentID": agent_id,
                "changeType": "agent_removed",
                "previousPolicy": &previous.mcp_policy,
                "nextPolicy": Value::Null,
                "removedServers": normalize_sorted_strings(&previous.mcp_policy.allowed_servers),
                "removedTools": previous
                    .mcp_policy
                    .allowed_tools
                    .as_ref()
                    .map(|tools| normalize_sorted_strings(tools))
                    .unwrap_or_default(),
                "allowedToolsNarrowedFromUnrestricted": previous.mcp_policy.allowed_tools.is_none(),
            }));
            continue;
        };

        let removed_servers = removed_strings(
            &previous.mcp_policy.allowed_servers,
            &next.mcp_policy.allowed_servers,
        );
        let previous_tools = previous
            .mcp_policy
            .allowed_tools
            .as_ref()
            .map(|tools| normalize_sorted_strings(tools));
        let next_tools = next
            .mcp_policy
            .allowed_tools
            .as_ref()
            .map(|tools| normalize_sorted_strings(tools));
        let removed_tools = match (&previous_tools, &next_tools) {
            (None, None) => Vec::new(),
            (None, Some(_)) => Vec::new(),
            (Some(previous), None) => previous.clone(),
            (Some(previous), Some(next)) => removed_strings(previous, next),
        };
        let allowed_tools_narrowed_from_unrestricted =
            previous.mcp_policy.allowed_tools.is_none() && next.mcp_policy.allowed_tools.is_some();
        if removed_servers.is_empty()
            && removed_tools.is_empty()
            && !allowed_tools_narrowed_from_unrestricted
        {
            continue;
        }
        changes.push(json!({
            "agentID": agent_id,
            "changeType": "mcp_policy_narrowed",
            "previousPolicy": &previous.mcp_policy,
            "nextPolicy": &next.mcp_policy,
            "removedServers": removed_servers,
            "removedTools": removed_tools,
            "allowedToolsNarrowedFromUnrestricted": allowed_tools_narrowed_from_unrestricted,
        }));
    }

    if changes.is_empty() {
        None
    } else {
        Some(json!({
            "trigger": "mcp_policy_narrowed",
            "dependencyChanges": changes,
        }))
    }
}

pub(super) async fn automations_v2_create(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Json(input): Json<AutomationV2CreateInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let now = crate::now_ms();
    let provenance = super::governance::resolve_governance_provenance(
        &headers,
        &tenant_context,
        &request_principal,
    );
    let workspace_root = input
        .workspace_root
        .as_deref()
        .map(crate::normalize_absolute_workspace_root)
        .transpose()
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error,
                    "code": "AUTOMATION_V2_CREATE_FAILED",
                })),
            )
        })?;
    let metadata = merge_automation_capabilities_metadata(input.metadata, input.capabilities)?;
    let declared_capabilities =
        crate::automation_v2::governance::AutomationDeclaredCapabilities::from_metadata(
            metadata.as_ref(),
        );
    state
        .can_create_automation_for_actor(&provenance.creator, &provenance, &declared_capabilities)
        .await
        .map_err(super::governance::governance_error_response)?;
    let automation = AutomationV2Spec {
        automation_id: input
            .automation_id
            .unwrap_or_else(|| format!("automation-v2-{}", Uuid::new_v4())),
        name: input.name,
        description: input.description,
        status: input.status.unwrap_or(AutomationV2Status::Draft),
        schedule: input.schedule,
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: input
            .agents
            .into_iter()
            .map(normalize_automation_v2_agent)
            .collect(),
        flow: input.flow,
        execution: input.execution.unwrap_or(AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        }),
        output_targets: input.output_targets.unwrap_or_default(),
        created_at_ms: now,
        updated_at_ms: now,
        creator_id: provenance
            .creator
            .actor_id
            .clone()
            .or(input.creator_id)
            .unwrap_or_else(|| "unknown".to_string()),
        workspace_root,
        metadata,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: input.scope_policy,
        watch_conditions: input.watch_conditions.unwrap_or_default(),
        handoff_config: input.handoff_config,
    };
    validate_shared_context_pack_bindings(
        &state,
        automation.workspace_root.as_deref(),
        automation.metadata.as_ref(),
    )
    .await?;
    let stored = state.put_automation_v2(automation).await.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error.to_string(),
                "code": "AUTOMATION_V2_CREATE_FAILED",
            })),
        )
    })?;
    let _ = state
        .set_automation_governance_provenance(&stored.automation_id, provenance.clone())
        .await;
    let _ = crate::audit::append_protected_audit_event(
        &state,
        "automation.governance.created",
        &tenant_context,
        provenance
            .creator
            .actor_id
            .clone()
            .or_else(|| provenance.creator.source.clone()),
        json!({
            "automationID": stored.automation_id.clone(),
            "provenance": provenance.clone(),
        }),
    )
    .await;
    Ok(Json(json!({ "automation": stored })))
}

pub(super) async fn automations_v2_list(State(state): State<AppState>) -> Json<Value> {
    let rows = state.list_automations_v2().await;
    Json(json!({
        "automations": rows,
        "count": rows.len(),
    }))
}

pub(super) async fn automations_v2_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(automation) = state.get_automation_v2(&id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": id}),
            ),
        ));
    };
    Ok(Json(json!({ "automation": automation })))
}

pub(super) async fn automations_v2_patch(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<AutomationV2PatchInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut automation) = state.get_automation_v2(&id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": id}),
            ),
        ));
    };
    let actor =
        super::governance::resolve_governance_actor(&headers, &tenant_context, &request_principal);
    let governance = state
        .get_or_bootstrap_automation_governance(&automation)
        .await;
    state
        .can_mutate_automation(&id, &actor, false)
        .await
        .map_err(super::governance::governance_error_response)?;
    let previous_declared_capabilities = governance.declared_capabilities.clone();
    let before = automation.clone();
    let input_agents = input.agents.clone();
    if let Some(name) = input.name {
        automation.name = name;
    }
    if let Some(description) = input.description {
        automation.description = Some(description);
    }
    if let Some(status) = input.status {
        automation.status = status;
    }
    if let Some(schedule) = input.schedule {
        automation.schedule = schedule;
    }
    if let Some(agents) = input_agents.clone() {
        automation.agents = agents
            .into_iter()
            .map(normalize_automation_v2_agent)
            .collect();
    }
    if let Some(flow) = input.flow {
        automation.flow = flow;
    }
    if let Some(execution) = input.execution {
        automation.execution = execution;
    }
    if let Some(output_targets) = input.output_targets {
        automation.output_targets = output_targets;
    }
    if let Some(workspace_root) = input.workspace_root {
        let normalized =
            crate::normalize_absolute_workspace_root(&workspace_root).map_err(|error| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": error,
                        "code": "AUTOMATION_V2_UPDATE_FAILED",
                    })),
                )
            })?;
        automation.workspace_root = Some(normalized);
    }
    let current_metadata = automation.metadata.clone();
    automation.metadata = merge_automation_capabilities_metadata(
        input.metadata.or_else(|| current_metadata),
        input.capabilities,
    )?;
    if let Some(scope_policy) = input.scope_policy {
        automation.scope_policy = Some(scope_policy);
    }
    if let Some(watch_conditions) = input.watch_conditions {
        automation.watch_conditions = watch_conditions;
    }
    if let Some(handoff_config) = input.handoff_config {
        automation.handoff_config = Some(handoff_config);
    }
    let next_declared_capabilities =
        crate::automation_v2::governance::AutomationDeclaredCapabilities::from_metadata(
            automation.metadata.as_ref(),
        );
    state
        .can_escalate_declared_capabilities(
            &actor,
            &previous_declared_capabilities,
            &next_declared_capabilities,
        )
        .await
        .map_err(super::governance::governance_error_response)?;
    validate_shared_context_pack_bindings(
        &state,
        automation.workspace_root.as_deref(),
        automation.metadata.as_ref(),
    )
    .await?;
    let stored = state.put_automation_v2(automation).await.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error.to_string(),
                "code": "AUTOMATION_V2_UPDATE_FAILED",
            })),
        )
    })?;
    let dependency_revocation_evidence = input_agents
        .as_ref()
        .and_then(|_| mcp_policy_dependency_revocation_details(&before.agents, &stored.agents));
    let _ = crate::audit::append_protected_audit_event(
        &state,
        "automation.governance.updated",
        &tenant_context,
        actor.actor_id.clone().or_else(|| actor.source.clone()),
        json!({
            "automationID": id,
            "before": before,
            "after": stored.clone(),
        }),
    )
    .await;
    if let Some(evidence) = dependency_revocation_evidence {
        state
            .pause_automation_for_dependency_revocation(
                &id,
                "mcp capabilities were narrowed".to_string(),
                evidence,
            )
            .await
            .map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": error.to_string(),
                        "code": "AUTOMATION_GOVERNANCE_DEPENDENCY_PAUSE_FAILED",
                    })),
                )
            })?;
    }
    Ok(Json(json!({ "automation": stored })))
}

pub(super) async fn automations_v2_delete(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(automation) = state.get_automation_v2(&id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": id}),
            ),
        ));
    };
    let actor =
        super::governance::resolve_governance_actor(&headers, &tenant_context, &request_principal);
    let _ = state
        .get_or_bootstrap_automation_governance(&automation)
        .await;
    state
        .can_mutate_automation(&id, &actor, true)
        .await
        .map_err(super::governance::governance_error_response)?;
    let deleted = state
        .delete_automation_v2_with_governance(&id, actor)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": error.to_string(),
                    "code": "AUTOMATION_V2_DELETE_FAILED",
                })),
            )
        })?;
    let _ = crate::audit::append_protected_audit_event(
        &state,
        "automation.governance.deleted",
        &tenant_context,
        request_principal
            .actor_id
            .clone()
            .or_else(|| tenant_context.actor_id.clone()),
        json!({
            "automationID": id,
            "automation": deleted,
        }),
    )
    .await;
    Ok(Json(
        json!({ "ok": true, "deleted": true, "automationID": id }),
    ))
}

pub(super) async fn automations_v2_run_now(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<AutomationV2RunNowInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(automation) = state.get_automation_v2(&id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": id}),
            ),
        ));
    };
    let actor =
        super::governance::resolve_governance_actor(&headers, &tenant_context, &request_principal);
    let _ = state
        .get_or_bootstrap_automation_governance(&automation)
        .await;
    state
        .can_mutate_automation(&id, &actor, false)
        .await
        .map_err(super::governance::governance_error_response)?;
    let dry_run = input.dry_run;
    let run = if dry_run {
        state
            .create_automation_v2_dry_run(&automation, "manual")
            .await
            .map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": error.to_string(),
                        "code": "AUTOMATION_V2_RUN_CREATE_FAILED",
                    })),
                )
            })?
    } else {
        state
            .create_automation_v2_run(&automation, "manual")
            .await
            .map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": error.to_string(),
                        "code": "AUTOMATION_V2_RUN_CREATE_FAILED",
                    })),
                )
            })?
    };
    if let Some(automation_with_trigger) =
        automation_v2_with_manual_trigger_record(&automation, &run.run_id, dry_run)
    {
        let _ = state
            .put_automation_v2(automation_with_trigger.clone())
            .await;
        let _ = state
            .update_automation_v2_run(&run.run_id, |row| {
                row.automation_snapshot = Some(automation_with_trigger);
            })
            .await;
    }
    let run = state
        .get_automation_v2_run(&run.run_id)
        .await
        .unwrap_or(run);
    let _ = super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, &run).await;
    let _ = crate::audit::append_protected_audit_event(
        &state,
        "automation.governance.run_requested",
        &tenant_context,
        request_principal
            .actor_id
            .clone()
            .or_else(|| tenant_context.actor_id.clone()),
        json!({
            "automationID": id,
            "runID": run.run_id,
            "dryRun": dry_run,
            "requestedBy": actor,
        }),
    )
    .await;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run.run_id);
    Ok(Json(json!({
        "ok": true,
        "dry_run": dry_run,
        "run": automation_v2_run_with_context_links(&state, &run).await,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_v2_pause(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut automation) = state.get_automation_v2(&id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": id}),
            ),
        ));
    };
    let actor =
        super::governance::resolve_governance_actor(&headers, &tenant_context, &request_principal);
    let _ = state
        .get_or_bootstrap_automation_governance(&automation)
        .await;
    state
        .can_mutate_automation(&id, &actor, false)
        .await
        .map_err(super::governance::governance_error_response)?;
    automation.status = AutomationV2Status::Paused;
    let stored = state.put_automation_v2(automation).await.map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string(), "code":"AUTOMATION_V2_UPDATE_FAILED"})),
        )
    })?;
    let reason = reason_or_default(input.reason, "paused by operator");
    let runs = state.list_automation_v2_runs(Some(&id), 100).await;
    for run in runs {
        if run.status == AutomationRunStatus::Running {
            let session_ids = run.active_session_ids.clone();
            let _ = state
                .update_automation_v2_run(&run.run_id, |row| {
                    row.status = AutomationRunStatus::Pausing;
                    row.pause_reason = Some(reason.clone());
                })
                .await;
            for session_id in run.active_session_ids {
                let _ = state.cancellations.cancel(&session_id).await;
            }
            for instance_id in run.active_instance_ids {
                let _ = state
                    .agent_teams
                    .cancel_instance(&state, &instance_id, "paused by operator")
                    .await;
            }
            state.forget_automation_v2_sessions(&session_ids).await;
            let _ = state
                .update_automation_v2_run(&run.run_id, |row| {
                    row.status = AutomationRunStatus::Paused;
                    row.active_session_ids.clear();
                    row.active_instance_ids.clear();
                    crate::record_automation_lifecycle_event(
                        row,
                        "run_paused",
                        row.pause_reason.clone(),
                        None,
                    );
                })
                .await;
        }
    }
    let _ = crate::audit::append_protected_audit_event(
        &state,
        "automation.governance.paused",
        &tenant_context,
        request_principal
            .actor_id
            .clone()
            .or_else(|| tenant_context.actor_id.clone()),
        json!({
            "automationID": id,
            "reason": reason,
            "automation": stored.clone(),
        }),
    )
    .await;
    Ok(Json(json!({ "ok": true, "automation": stored })))
}

pub(super) async fn automations_v2_resume(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut automation) = state.get_automation_v2(&id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": id}),
            ),
        ));
    };
    let actor =
        super::governance::resolve_governance_actor(&headers, &tenant_context, &request_principal);
    let _ = state
        .get_or_bootstrap_automation_governance(&automation)
        .await;
    state
        .can_mutate_automation(&id, &actor, false)
        .await
        .map_err(super::governance::governance_error_response)?;
    automation.status = AutomationV2Status::Active;
    let stored = state.put_automation_v2(automation).await.map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string(), "code":"AUTOMATION_V2_UPDATE_FAILED"})),
        )
    })?;
    let _ = crate::audit::append_protected_audit_event(
        &state,
        "automation.governance.resumed",
        &tenant_context,
        request_principal
            .actor_id
            .clone()
            .or_else(|| tenant_context.actor_id.clone()),
        json!({
            "automationID": id,
            "automation": stored.clone(),
        }),
    )
    .await;
    Ok(Json(json!({ "ok": true, "automation": stored })))
}

/// GET /automations/v2/{id}/handoffs
///
/// Returns the inbox, approved, and archived handoff artifacts for a given automation.
/// Scans the directories defined in the automation's `handoff_config` (or defaults)
/// relative to the automation's `workspace_root`.
///
/// Response shape:
/// ```json
/// { "inbox": [...], "approved": [...], "archived": [...],
///   "counts": { "inbox": 0, "approved": 0, "archived": 0 } }
/// ```
pub(super) async fn automations_v2_handoffs(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    use crate::automation_v2::types::HandoffArtifact;

    let Some(automation) = state.get_automation_v2(&id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found","code":"AUTOMATION_V2_NOT_FOUND","automationID": id}),
            ),
        ));
    };

    let workspace_root = match automation.workspace_root.as_deref() {
        Some(root) if !root.is_empty() => root.to_string(),
        _ => state.workspace_index.snapshot().await.root,
    };

    let handoff_cfg = automation.effective_handoff_config();
    let root = std::path::Path::new(&workspace_root);

    let inbox_dir = root.join(&handoff_cfg.inbox_dir);
    let approved_dir = root.join(&handoff_cfg.approved_dir);
    let archived_dir = root.join(&handoff_cfg.archived_dir);

    async fn scan_dir(dir: &std::path::Path) -> Vec<HandoffArtifact> {
        if !dir.exists() {
            return vec![];
        }
        let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
            return vec![];
        };
        let mut items: Vec<HandoffArtifact> = Vec::new();
        let mut scanned = 0usize;
        while let Ok(Some(entry)) = entries.next_entry().await {
            scanned += 1;
            if scanned > 512 {
                break; // cap scan
            }
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(bytes) = tokio::fs::read(&path).await {
                if let Ok(artifact) = serde_json::from_slice::<HandoffArtifact>(&bytes) {
                    items.push(artifact);
                }
            }
        }
        // Sort oldest-first by created_at_ms
        items.sort_by_key(|a| a.created_at_ms);
        items
    }

    let (inbox, approved, archived) = tokio::join!(
        scan_dir(&inbox_dir),
        scan_dir(&approved_dir),
        scan_dir(&archived_dir),
    );

    let inbox_count = inbox.len();
    let approved_count = approved.len();
    let archived_count = archived.len();

    Ok(Json(json!({
        "automation_id": id,
        "workspace_root": workspace_root,
        "handoff_config": {
            "inbox_dir":    handoff_cfg.inbox_dir,
            "approved_dir": handoff_cfg.approved_dir,
            "archived_dir": handoff_cfg.archived_dir,
            "auto_approve": handoff_cfg.auto_approve,
        },
        "inbox":    inbox,
        "approved": approved,
        "archived": archived,
        "counts": {
            "inbox":    inbox_count,
            "approved": approved_count,
            "archived": archived_count,
            "total":    inbox_count + approved_count + archived_count,
        },
    })))
}

pub(super) async fn automations_v2_runs(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<RoutineRunsQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(50);
    let rows = state.list_automation_v2_runs(Some(&id), limit).await;
    if let Some(automation) = state.get_automation_v2(&id).await {
        for run in &rows {
            let _ =
                super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, run)
                    .await;
        }
    }
    let mut runs = Vec::with_capacity(rows.len());
    for run in &rows {
        runs.push(automation_v2_run_with_context_links(&state, run).await);
    }
    Json(json!({ "automationID": id, "runs": runs, "count": rows.len() }))
}

pub(super) async fn automations_v2_runs_all(
    State(state): State<AppState>,
    Query(query): Query<RoutineRunsQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let rows = state.list_automation_v2_runs(None, limit).await;
    for run in &rows {
        if let Some(automation) = state.get_automation_v2(&run.automation_id).await {
            let _ =
                super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, run)
                    .await;
        }
    }
    let mut runs = Vec::with_capacity(rows.len());
    for run in &rows {
        runs.push(automation_v2_run_with_context_links(&state, run).await);
    }
    Json(json!({ "runs": runs, "count": rows.len() }))
}

pub(super) async fn automations_v2_run_get(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(run) = state.get_automation_v2_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Run not found", "code":"AUTOMATION_V2_RUN_NOT_FOUND", "runID": run_id}),
            ),
        ));
    };
    if let Some(automation) = state.get_automation_v2(&run.automation_id).await {
        let _ =
            super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, &run).await;
    }
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(json!({
        "run": automation_v2_run_with_context_links(&state, &run).await,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}

pub(super) async fn automations_v2_run_pause(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Run not found", "code":"AUTOMATION_V2_RUN_NOT_FOUND", "runID": run_id}),
            ),
        ));
    };
    if !matches!(
        current.status,
        AutomationRunStatus::Running | AutomationRunStatus::Queued
    ) {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run is not pausable", "code":"AUTOMATION_V2_RUN_NOT_PAUSABLE", "runID": run_id}),
            ),
        ));
    }
    let reason = reason_or_default(input.reason, "paused by operator");
    let session_ids = current.active_session_ids.clone();
    let instance_ids = current.active_instance_ids.clone();
    let _ = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Paused;
            run.pause_reason = Some(reason.clone());
            run.active_session_ids.clear();
            run.active_instance_ids.clear();
            crate::record_automation_lifecycle_event(
                run,
                "run_pause_requested",
                Some(reason.clone()),
                None,
            );
            crate::record_automation_lifecycle_event(
                run,
                "run_paused",
                run.pause_reason.clone(),
                None,
            );
        })
        .await;
    state.forget_automation_v2_sessions(&session_ids).await;
    spawn_automation_v2_run_cleanup(
        state.clone(),
        session_ids,
        instance_ids,
        "paused by operator",
    );
    let updated = state.get_automation_v2_run(&run_id).await.ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"})),
        )
    })?;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(
        json!({ "ok": true, "run": automation_v2_run_with_context_links(&state, &updated).await, "contextRunID": context_run_id, "linked_context_run_id": context_run_id }),
    ))
}

pub(super) async fn automations_v2_run_resume(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Run not found", "code":"AUTOMATION_V2_RUN_NOT_FOUND", "runID": run_id}),
            ),
        ));
    };
    if current.status != AutomationRunStatus::Paused {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run is not paused", "code":"AUTOMATION_V2_RUN_NOT_PAUSED", "runID": run_id}),
            ),
        ));
    }
    let reason = reason_or_default(input.reason, "resumed by operator");
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Queued;
            run.resume_reason = Some(reason.clone());
            run.stop_kind = None;
            run.stop_reason = None;
            crate::record_automation_lifecycle_event(
                run,
                "run_resumed",
                Some(reason.clone()),
                None,
            );
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"}),
                ),
            )
        })?;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(
        json!({ "ok": true, "run": automation_v2_run_with_context_links(&state, &updated).await, "contextRunID": context_run_id, "linked_context_run_id": context_run_id }),
    ))
}

pub(super) async fn automations_v2_run_cancel(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Run not found", "code":"AUTOMATION_V2_RUN_NOT_FOUND", "runID": run_id}),
            ),
        ));
    };
    if matches!(
        current.status,
        AutomationRunStatus::Cancelled
            | AutomationRunStatus::Completed
            | AutomationRunStatus::Failed
    ) {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run already terminal", "code":"AUTOMATION_V2_RUN_TERMINAL", "runID": run_id}),
            ),
        ));
    }
    let session_ids = current.active_session_ids.clone();
    let instance_ids = current.active_instance_ids.clone();
    state.forget_automation_v2_sessions(&session_ids).await;
    let reason = reason_or_default(input.reason, "cancelled by operator");
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Cancelled;
            run.detail = Some(reason.clone());
            run.stop_kind = Some(crate::AutomationStopKind::OperatorStopped);
            run.stop_reason = Some(reason.clone());
            run.active_session_ids.clear();
            run.active_instance_ids.clear();
            crate::record_automation_lifecycle_event(
                run,
                "run_stopped",
                Some(reason.clone()),
                Some(crate::AutomationStopKind::OperatorStopped),
            );
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"}),
                ),
            )
        })?;
    spawn_automation_v2_run_cleanup(
        state.clone(),
        session_ids,
        instance_ids,
        "cancelled by operator",
    );
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(
        json!({ "ok": true, "run": automation_v2_run_with_context_links(&state, &updated).await, "contextRunID": context_run_id, "linked_context_run_id": context_run_id }),
    ))
}

pub(crate) async fn automations_v2_run_gate_decide(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<AutomationV2GateDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Run not found", "code":"AUTOMATION_V2_RUN_NOT_FOUND", "runID": run_id}),
            ),
        ));
    };
    if current.status != AutomationRunStatus::AwaitingApproval {
        // Race UX: when a second surface tries to decide a gate that has just
        // been resolved by another surface (Slack click + control-panel click,
        // etc.), surface the winner's decision so the loser's UI can render
        // "already decided by …" instead of a raw error. The winner's record
        // is the most recently appended gate_history entry.
        let winner = current.checkpoint.gate_history.last();
        let winner_payload = winner.map(|record| {
            json!({
                "node_id": record.node_id,
                "decision": record.decision,
                "reason": record.reason,
                "decided_at_ms": record.decided_at_ms,
            })
        });
        let mut body = json!({
            "error": "Run is not awaiting approval",
            "code": "AUTOMATION_V2_RUN_NOT_AWAITING_APPROVAL",
            "runID": run_id,
            "currentStatus": current.status,
        });
        if let Some(winner_value) = winner_payload {
            if let Some(obj) = body.as_object_mut() {
                obj.insert("winningDecision".to_string(), winner_value);
            }
        }
        return Err((StatusCode::CONFLICT, Json(body)));
    }
    let Some(gate) = current.checkpoint.awaiting_gate.clone() else {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run has no pending gate", "code":"AUTOMATION_V2_RUN_GATE_MISSING", "runID": run_id}),
            ),
        ));
    };
    let decision = input.decision.trim().to_ascii_lowercase();
    if !["approve", "rework", "cancel"].contains(&decision.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({"error":"decision must be approve, rework, or cancel", "code":"AUTOMATION_V2_GATE_INVALID_DECISION"}),
            ),
        ));
    }
    let Some(automation) = state.get_automation_v2(&current.automation_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": current.automation_id}),
            ),
        ));
    };
    let Some(node) = automation
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == gate.node_id)
        .cloned()
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Gate node not found", "code":"AUTOMATION_V2_GATE_NODE_NOT_FOUND", "nodeID": gate.node_id}),
            ),
        ));
    };
    let reason = input
        .reason
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            run.checkpoint
                .gate_history
                .push(crate::AutomationGateDecisionRecord {
                    node_id: gate.node_id.clone(),
                    decision: decision.clone(),
                    reason: reason.clone(),
                    decided_at_ms: crate::now_ms(),
                });
            run.checkpoint.awaiting_gate = None;
            match decision.as_str() {
                "approve" => {
                    run.status = AutomationRunStatus::Queued;
                    run.detail = Some(format!("gate `{}` approved", gate.node_id));
                    run.stop_kind = None;
                    run.stop_reason = None;
                    run.checkpoint
                        .pending_nodes
                        .retain(|node_id| node_id != &gate.node_id);
                    if !run
                        .checkpoint
                        .completed_nodes
                        .iter()
                        .any(|node_id| node_id == &gate.node_id)
                    {
                        run.checkpoint.completed_nodes.push(gate.node_id.clone());
                    }
                    run.checkpoint.node_outputs.insert(
                        gate.node_id.clone(),
                        json!({
                            "contract_kind": "approval_gate",
                            "summary": format!("Gate `{}` approved.", gate.node_id),
                            "content": {
                                "decision": "approve",
                                "reason": reason,
                            },
                            "created_at_ms": crate::now_ms(),
                            "node_id": gate.node_id.clone(),
                        }),
                    );
                }
                "rework" => {
                    run.status = AutomationRunStatus::Queued;
                    run.detail = Some(format!("gate `{}` sent work back for rework", gate.node_id));
                    run.stop_kind = None;
                    run.stop_reason = None;
                    let mut roots = gate
                        .rework_targets
                        .iter()
                        .cloned()
                        .collect::<std::collections::HashSet<_>>();
                    if roots.is_empty() {
                        roots.extend(gate.upstream_node_ids.iter().cloned());
                    }
                    roots.insert(gate.node_id.clone());
                    let reset_nodes = crate::collect_automation_descendants(&automation, &roots);
                    for node_id in &reset_nodes {
                        run.checkpoint.node_outputs.remove(node_id);
                        run.checkpoint.node_attempts.remove(node_id);
                    }
                    run.checkpoint
                        .completed_nodes
                        .retain(|node_id| !reset_nodes.contains(node_id));
                    let mut pending = run.checkpoint.pending_nodes.clone();
                    for node_id in reset_nodes {
                        if !pending.iter().any(|existing| existing == &node_id) {
                            pending.push(node_id);
                        }
                    }
                    pending.sort();
                    pending.dedup();
                    run.checkpoint.pending_nodes = pending;
                }
                "cancel" => {
                    run.status = AutomationRunStatus::Cancelled;
                    let stop_reason = reason
                        .clone()
                        .unwrap_or_else(|| format!("gate `{}` cancelled the run", gate.node_id));
                    run.detail = Some(stop_reason.clone());
                    run.stop_kind = Some(crate::AutomationStopKind::Cancelled);
                    run.stop_reason = Some(stop_reason.clone());
                    crate::record_automation_lifecycle_event(
                        run,
                        "run_cancelled",
                        Some(stop_reason),
                        Some(crate::AutomationStopKind::Cancelled),
                    );
                }
                _ => {}
            }
            if decision != "cancel" {
                run.resume_reason = Some(format!("gate `{}` decision: {}", gate.node_id, decision));
                clear_automation_run_execution_handles(run);
                crate::refresh_automation_runtime_state(&automation, run);
            }
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"}),
                ),
            )
        })?;
    let _ =
        super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, &updated).await;
    let _ = node;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(
        json!({ "ok": true, "run": automation_v2_run_with_context_links(&state, &updated).await, "contextRunID": context_run_id, "linked_context_run_id": context_run_id }),
    ))
}

pub(super) async fn automations_v2_run_recover(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Run not found", "code":"AUTOMATION_V2_RUN_NOT_FOUND", "runID": run_id}),
            ),
        ));
    };
    let blocked_node_ids = automation_v2_blocked_node_ids(&current);
    let blocked_run_is_recoverable = matches!(current.status, AutomationRunStatus::Blocked)
        || (matches!(current.status, AutomationRunStatus::Completed)
            && !blocked_node_ids.is_empty());
    if !matches!(
        current.status,
        AutomationRunStatus::Failed | AutomationRunStatus::Paused
    ) && !blocked_run_is_recoverable
    {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run is not recoverable", "code":"AUTOMATION_V2_RUN_NOT_RECOVERABLE", "runID": run_id}),
            ),
        ));
    }
    let Some(automation) = state.get_automation_v2(&current.automation_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": current.automation_id}),
            ),
        ));
    };
    let reset_nodes = if current.status == AutomationRunStatus::Failed {
        let Some(failure_node_id) = automation_v2_recoverable_failure_node_id(&current) else {
            return Err((
                StatusCode::CONFLICT,
                Json(
                    json!({"error":"Run has no recoverable failed node", "code":"AUTOMATION_V2_RUN_FAILURE_CONTEXT_MISSING", "runID": run_id}),
                ),
            ));
        };
        let mut roots = blocked_node_ids
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        roots.insert(failure_node_id);
        crate::collect_automation_descendants(&automation, &roots)
    } else if blocked_run_is_recoverable {
        if blocked_node_ids.is_empty() {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({
                    "error":"Run has no recoverable blocked node",
                    "code":"AUTOMATION_V2_RUN_BLOCKED_CONTEXT_MISSING",
                    "runID": run_id
                })),
            ));
        }
        let roots = blocked_node_ids
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        crate::collect_automation_descendants(&automation, &roots)
    } else {
        std::collections::HashSet::new()
    };
    let reason = if current.status == AutomationRunStatus::Paused {
        reason_or_default(input.reason, "recovered from paused state by operator")
    } else {
        reason_or_default(input.reason, "recovered by operator")
    };
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Queued;
            run.finished_at_ms = None;
            run.detail = Some(reason.clone());
            run.resume_reason = Some(reason.clone());
            run.stop_kind = None;
            run.stop_reason = None;
            run.checkpoint.awaiting_gate = None;
            clear_automation_run_execution_handles(run);
            if run.pause_reason.as_deref() == Some("stale_no_provider_activity")
                && reset_nodes.is_empty()
            {
                for node_id in run.checkpoint.pending_nodes.clone() {
                    run.checkpoint.node_outputs.remove(&node_id);
                    run.checkpoint.node_attempts.remove(&node_id);
                }
            }
            if !reset_nodes.is_empty() {
                for node_id in &reset_nodes {
                    run.checkpoint.node_outputs.remove(node_id);
                    run.checkpoint.node_attempts.remove(node_id);
                }
                run.checkpoint
                    .blocked_nodes
                    .retain(|node_id| !reset_nodes.contains(node_id));
                run.checkpoint
                    .completed_nodes
                    .retain(|node_id| !reset_nodes.contains(node_id));
                let mut pending = run.checkpoint.pending_nodes.clone();
                for node_id in &reset_nodes {
                    if !pending.iter().any(|existing| existing == node_id) {
                        pending.push(node_id.clone());
                    }
                }
                pending.sort();
                pending.dedup();
                run.checkpoint.pending_nodes = pending;
                run.checkpoint.last_failure = None;
            }
            crate::record_automation_lifecycle_event(
                run,
                if reset_nodes.is_empty() {
                    "run_recovered_from_pause"
                } else {
                    "run_recovered"
                },
                Some(reason.clone()),
                None,
            );
            crate::refresh_automation_runtime_state(&automation, run);
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"}),
                ),
            )
        })?;
    let _ =
        super::context_runs::sync_automation_v2_run_blackboard(&state, &automation, &updated).await;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(
        json!({ "ok": true, "run": automation_v2_run_with_context_links(&state, &updated).await, "contextRunID": context_run_id, "linked_context_run_id": context_run_id }),
    ))
}

pub(super) async fn automations_v2_run_repair(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<AutomationV2RunRepairInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let node_id = input.node_id.trim().to_string();
    if node_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(
                json!({"error":"node_id is required", "code":"AUTOMATION_V2_REPAIR_NODE_REQUIRED"}),
            ),
        ));
    }
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Run not found", "code":"AUTOMATION_V2_RUN_NOT_FOUND", "runID": run_id}),
            ),
        ));
    };
    if matches!(
        current.status,
        AutomationRunStatus::Running | AutomationRunStatus::Queued | AutomationRunStatus::Pausing
    ) {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run must be paused, failed, awaiting approval, or cancelled before repair", "code":"AUTOMATION_V2_RUN_NOT_REPAIRABLE", "runID": run_id}),
            ),
        ));
    }
    let Some(mut automation) = state
        .get_automation_v2(&current.automation_id)
        .await
        .or_else(|| current.automation_snapshot.clone())
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": current.automation_id}),
            ),
        ));
    };
    let Some(node) = automation
        .flow
        .nodes
        .iter_mut()
        .find(|node| node.node_id == node_id)
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Node not found", "code":"AUTOMATION_V2_REPAIR_NODE_NOT_FOUND", "nodeID": node_id}),
            ),
        ));
    };
    let agent_id = node.agent_id.clone();
    let previous_prompt = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(|builder| builder.get("prompt"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let prompt = input
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let template_id = input
        .template_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let model_policy = input.model_policy.clone();
    if let Some(prompt_value) = prompt.as_ref() {
        let metadata = node.metadata.get_or_insert_with(|| json!({}));
        let builder = metadata
            .as_object_mut()
            .and_then(|root| root.entry("builder").or_insert_with(|| json!({})).as_object_mut())
            .ok_or_else(|| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error":"Node metadata is not repairable", "code":"AUTOMATION_V2_REPAIR_METADATA_INVALID"})),
                )
            })?;
        builder.insert("prompt".to_string(), Value::String(prompt_value.clone()));
    }
    let previous_agent = automation
        .agents
        .iter()
        .find(|agent| agent.agent_id == agent_id)
        .cloned();
    if template_id.is_some() || model_policy.is_some() {
        let Some(agent) = automation
            .agents
            .iter_mut()
            .find(|agent| agent.agent_id == agent_id)
        else {
            return Err((
                StatusCode::NOT_FOUND,
                Json(
                    json!({"error":"Node agent not found", "code":"AUTOMATION_V2_REPAIR_AGENT_NOT_FOUND", "agentID": agent_id}),
                ),
            ));
        };
        if let Some(template_value) = template_id.clone() {
            agent.template_id = Some(template_value);
        }
        if let Some(model_policy_value) = model_policy.clone() {
            agent.model_policy = Some(model_policy_value);
        }
    }
    automation.updated_at_ms = crate::now_ms();
    let stored_automation = state.put_automation_v2(automation.clone()).await.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": error.to_string(), "code":"AUTOMATION_V2_REPAIR_PERSIST_FAILED"})),
        )
    })?;
    let roots = std::iter::once(node_id.clone()).collect::<std::collections::HashSet<_>>();
    let reset_nodes = crate::collect_automation_descendants(&stored_automation, &roots);
    let cleared_outputs = crate::clear_automation_subtree_outputs(
        &state,
        &stored_automation,
        &run_id,
        &reset_nodes,
    )
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": error.to_string(), "code":"AUTOMATION_V2_REPAIR_OUTPUT_RESET_FAILED"})),
            )
        })?;
    let reason = reason_or_default(
        input.reason,
        &format!("repaired node `{}` and reset affected subtree", node_id),
    );
    let updated_agent = stored_automation
        .agents
        .iter()
        .find(|agent| agent.agent_id == agent_id)
        .cloned();
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Queued;
            run.finished_at_ms = None;
            run.detail = Some(reason.clone());
            run.resume_reason = Some(reason.clone());
            run.stop_kind = None;
            run.stop_reason = None;
            run.pause_reason = None;
            run.checkpoint.awaiting_gate = None;
            clear_automation_run_execution_handles(run);
            for reset_node_id in &reset_nodes {
                run.checkpoint.node_outputs.remove(reset_node_id);
                run.checkpoint.node_attempts.remove(reset_node_id);
            }
            run.checkpoint
                .blocked_nodes
                .retain(|blocked_id| !reset_nodes.contains(blocked_id));
            run.checkpoint
                .completed_nodes
                .retain(|completed_id| !reset_nodes.contains(completed_id));
            let mut pending = run.checkpoint.pending_nodes.clone();
            for reset_node_id in &reset_nodes {
                if !pending.iter().any(|existing| existing == reset_node_id) {
                    pending.push(reset_node_id.clone());
                }
            }
            pending.sort();
            pending.dedup();
            run.checkpoint.pending_nodes = pending;
            run.checkpoint.last_failure = None;
            run.automation_snapshot = Some(stored_automation.clone());
            crate::record_automation_lifecycle_event_with_metadata(
                run,
                "run_step_repaired",
                Some(reason.clone()),
                None,
                Some(json!({
                    "node_id": node_id,
                    "reset_nodes": reset_nodes.iter().cloned().collect::<Vec<_>>(),
                    "prompt_updated": prompt.is_some(),
                    "template_updated": template_id.is_some(),
                    "model_policy_updated": model_policy.is_some(),
                    "reset_only": prompt.is_none() && template_id.is_none() && model_policy.is_none(),
                    "cleared_outputs": cleared_outputs,
                    "previous_prompt": previous_prompt,
                    "new_prompt": prompt,
                    "previous_template_id": previous_agent.as_ref().and_then(|agent| agent.template_id.clone()),
                    "new_template_id": updated_agent.as_ref().and_then(|agent| agent.template_id.clone()),
                    "previous_model_policy": previous_agent.as_ref().and_then(|agent| agent.model_policy.clone()),
                    "new_model_policy": updated_agent.as_ref().and_then(|agent| agent.model_policy.clone()),
                })),
            );
            crate::refresh_automation_runtime_state(&stored_automation, run);
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"}),
                ),
            )
        })?;
    let _ = super::context_runs::sync_automation_v2_run_blackboard(
        &state,
        &stored_automation,
        &updated,
    )
    .await;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run_id);
    Ok(Json(
        json!({ "ok": true, "run": automation_v2_run_with_context_links(&state, &updated).await, "automation": stored_automation, "contextRunID": context_run_id, "linked_context_run_id": context_run_id }),
    ))
}

async fn automation_v2_reset_task_subtree(
    state: &AppState,
    run_id: &str,
    node_id: &str,
    reason: String,
    lifecycle_event: &str,
) -> Result<
    (
        AutomationV2Spec,
        crate::AutomationV2RunRecord,
        Vec<String>,
        Vec<String>,
    ),
    (StatusCode, Json<Value>),
> {
    let Some(current) = state.get_automation_v2_run(run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Run not found",
                "code":"AUTOMATION_V2_RUN_NOT_FOUND",
                "runID": run_id
            })),
        ));
    };
    if matches!(
        current.status,
        AutomationRunStatus::Running | AutomationRunStatus::Queued | AutomationRunStatus::Pausing
    ) {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error":"Run must be paused, blocked, failed, awaiting approval, completed, or cancelled before task reset",
                "code":"AUTOMATION_V2_RUN_TASK_NOT_MUTABLE",
                "runID": run_id
            })),
        ));
    }
    let Some(automation) = state
        .get_automation_v2(&current.automation_id)
        .await
        .or_else(|| current.automation_snapshot.clone())
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Automation not found",
                "code":"AUTOMATION_V2_NOT_FOUND",
                "automationID": current.automation_id
            })),
        ));
    };
    if !automation
        .flow
        .nodes
        .iter()
        .any(|node| node.node_id == node_id)
    {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Node not found",
                "code":"AUTOMATION_V2_TASK_NODE_NOT_FOUND",
                "nodeID": node_id
            })),
        ));
    }
    let roots = std::iter::once(node_id.to_string()).collect::<std::collections::HashSet<_>>();
    let reset_nodes = crate::collect_automation_descendants(&automation, &roots);
    let cleared_outputs =
        crate::clear_automation_subtree_outputs(state, &automation, run_id, &reset_nodes)
            .await
            .map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": error.to_string(),
                        "code":"AUTOMATION_V2_TASK_RESET_OUTPUT_CLEAR_FAILED"
                    })),
                )
            })?;
    let mut reset_nodes_list = reset_nodes.iter().cloned().collect::<Vec<_>>();
    reset_nodes_list.sort();
    let updated = state
        .update_automation_v2_run(run_id, |run| {
            run.status = AutomationRunStatus::Queued;
            run.finished_at_ms = None;
            run.detail = Some(reason.clone());
            run.resume_reason = Some(reason.clone());
            run.stop_kind = None;
            run.stop_reason = None;
            run.pause_reason = None;
            run.checkpoint.awaiting_gate = None;
            clear_automation_run_execution_handles(run);
            for reset_node_id in &reset_nodes {
                run.checkpoint.node_outputs.remove(reset_node_id);
                run.checkpoint.node_attempts.remove(reset_node_id);
            }
            run.checkpoint
                .blocked_nodes
                .retain(|blocked_id| !reset_nodes.contains(blocked_id));
            run.checkpoint
                .completed_nodes
                .retain(|completed_id| !reset_nodes.contains(completed_id));
            let mut pending = run.checkpoint.pending_nodes.clone();
            for reset_node_id in &reset_nodes {
                if !pending.iter().any(|existing| existing == reset_node_id) {
                    pending.push(reset_node_id.clone());
                }
            }
            pending.sort();
            pending.dedup();
            run.checkpoint.pending_nodes = pending;
            run.checkpoint.last_failure = None;
            run.automation_snapshot = Some(automation.clone());
            crate::record_automation_lifecycle_event_with_metadata(
                run,
                lifecycle_event,
                Some(reason.clone()),
                None,
                Some(json!({
                    "node_id": node_id,
                    "reset_nodes": reset_nodes_list.clone(),
                    "cleared_outputs": cleared_outputs.clone(),
                })),
            );
            crate::refresh_automation_runtime_state(&automation, run);
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error":"Run update failed",
                    "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"
                })),
            )
        })?;
    Ok((automation, updated, cleared_outputs, reset_nodes_list))
}
