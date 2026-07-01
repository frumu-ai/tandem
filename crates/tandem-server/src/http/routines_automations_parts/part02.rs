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

fn automation_v2_not_found(id: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "Automation not found",
            "code": "AUTOMATION_V2_NOT_FOUND",
            "automationID": id,
        })),
    )
}

fn automation_v2_run_not_found(run_id: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": "Run not found",
            "code": "AUTOMATION_V2_RUN_NOT_FOUND",
            "runID": run_id,
        })),
    )
}

fn ensure_automation_v2_tenant(
    request_tenant: &TenantContext,
    automation: &AutomationV2Spec,
) -> Result<(), (StatusCode, Json<Value>)> {
    super::ensure_same_tenant(request_tenant, &automation.tenant_context())
        .map_err(|_| automation_v2_not_found(&automation.automation_id))
}

fn ensure_automation_v2_run_tenant(
    request_tenant: &TenantContext,
    run: &crate::automation_v2::types::AutomationV2RunRecord,
) -> Result<(), (StatusCode, Json<Value>)> {
    super::ensure_same_tenant(request_tenant, &run.tenant_context)
        .map_err(|_| automation_v2_run_not_found(&run.run_id))
}

#[derive(Default, Deserialize)]
pub(super) struct AutomationsV2ListQuery {
    #[serde(default)]
    view: Option<String>,
}

fn automation_v2_summary_value(automation: &AutomationV2Spec) -> Value {
    json!({
        "automation_id": automation.automation_id,
        "name": automation.name,
        "description": automation.description,
        "status": automation.status,
        "schedule": automation.schedule,
        "execution": automation.execution,
        "output_targets": automation.output_targets,
        "created_at_ms": automation.created_at_ms,
        "updated_at_ms": automation.updated_at_ms,
        "creator_id": automation.creator_id,
        "workspace_root": automation.workspace_root,
        "metadata": automation.metadata,
        "next_fire_at_ms": automation.next_fire_at_ms,
        "last_fired_at_ms": automation.last_fired_at_ms,
        "agent_count": automation.agents.len(),
        "node_count": automation.flow.nodes.len(),
    })
}

fn hosted_context_admin(verified: Option<&VerifiedTenantContext>) -> bool {
    let Some(verified) = verified else {
        return false;
    };
    verified.roles.iter().any(|role| {
        matches!(
            role.as_str(),
            "owner"
                | "admin"
                | "hosted:owner"
                | "hosted:admin"
                | "enterprise:admin"
                | "workspace:admin"
                | "organization:admin"
        )
    }) || verified.capabilities.iter().any(|capability| {
        matches!(
            capability.as_str(),
            "hosted.owner" | "hosted.admin" | "automation.write" | "automation.share"
        )
    })
}

fn hosted_context_actor_id(verified: Option<&VerifiedTenantContext>) -> Option<&str> {
    verified
        .map(|context| context.human_actor.actor_id.trim())
        .filter(|actor_id| !actor_id.is_empty())
}

fn automation_v2_access_metadata(
    automation: &AutomationV2Spec,
) -> Option<&serde_json::Map<String, Value>> {
    automation
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("resource_access"))
        .and_then(Value::as_object)
}

fn automation_v2_access_visibility(automation: &AutomationV2Spec) -> Option<&str> {
    automation_v2_access_metadata(automation)
        .and_then(|metadata| metadata.get("visibility"))
        .and_then(Value::as_str)
}

fn automation_v2_access_owner(automation: &AutomationV2Spec) -> Option<&str> {
    automation_v2_access_metadata(automation)
        .and_then(|metadata| metadata.get("owner_principal"))
        .and_then(Value::as_object)
        .and_then(|owner| owner.get("id"))
        .and_then(Value::as_str)
}

fn automation_v2_access_audiences(automation: &AutomationV2Spec) -> Vec<String> {
    automation_v2_access_metadata(automation)
        .and_then(|metadata| metadata.get("audience_principals"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn automation_v2_visible_to_context(
    automation: &AutomationV2Spec,
    verified: Option<&VerifiedTenantContext>,
) -> bool {
    if verified.is_none() || automation_v2_access_metadata(automation).is_none() {
        return true;
    }
    if hosted_context_admin(verified) {
        return true;
    }
    let Some(actor_id) = hosted_context_actor_id(verified) else {
        return false;
    };
    if automation_v2_access_owner(automation) == Some(actor_id) {
        return true;
    }
    match automation_v2_access_visibility(automation).unwrap_or("private") {
        "org" => true,
        "group" => {
            let audience = automation_v2_access_audiences(automation);
            let groups = verified
                .map(|context| context.org_units.as_slice())
                .unwrap_or(&[]);
            groups
                .iter()
                .any(|group| audience.iter().any(|entry| entry == group))
        }
        _ => false,
    }
}

fn ensure_automation_v2_visible_to_context(
    automation: &AutomationV2Spec,
    verified: Option<&VerifiedTenantContext>,
) -> Result<(), (StatusCode, Json<Value>)> {
    if automation_v2_visible_to_context(automation, verified) {
        Ok(())
    } else {
        Err(automation_v2_not_found(&automation.automation_id))
    }
}

fn ensure_automation_v2_owner_or_admin(
    automation: &AutomationV2Spec,
    verified: Option<&VerifiedTenantContext>,
) -> Result<(), (StatusCode, Json<Value>)> {
    if verified.is_none() || automation_v2_access_metadata(automation).is_none() {
        return Ok(());
    }
    let actor_id = hosted_context_actor_id(verified);
    if hosted_context_admin(verified) || actor_id == automation_v2_access_owner(automation) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "error": "Automation access denied",
                "code": "AUTOMATION_V2_ACCESS_DENIED",
            })),
        ))
    }
}

async fn automation_v2_run_automation_for_access(
    state: &AppState,
    run: &crate::automation_v2::types::AutomationV2RunRecord,
) -> Result<AutomationV2Spec, (StatusCode, Json<Value>)> {
    state
        .get_automation_v2(&run.automation_id)
        .await
        .or_else(|| run.automation_snapshot.clone())
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error":"Automation not found",
                    "code":"AUTOMATION_V2_NOT_FOUND",
                    "automationID": run.automation_id,
                })),
            )
        })
}

async fn ensure_automation_v2_run_visible_to_context(
    state: &AppState,
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    verified: Option<&VerifiedTenantContext>,
) -> Result<AutomationV2Spec, (StatusCode, Json<Value>)> {
    let automation = automation_v2_run_automation_for_access(state, run).await?;
    ensure_automation_v2_visible_to_context(&automation, verified)?;
    Ok(automation)
}

async fn ensure_automation_v2_run_owner_or_admin(
    state: &AppState,
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    verified: Option<&VerifiedTenantContext>,
) -> Result<AutomationV2Spec, (StatusCode, Json<Value>)> {
    let automation = automation_v2_run_automation_for_access(state, run).await?;
    ensure_automation_v2_owner_or_admin(&automation, verified)?;
    Ok(automation)
}

fn with_automation_v2_private_access_metadata(
    metadata: Option<Value>,
    verified: Option<&VerifiedTenantContext>,
) -> Option<Value> {
    let Some(actor_id) = hosted_context_actor_id(verified) else {
        return metadata;
    };
    let mut obj = metadata
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    obj.entry("resource_access".to_string()).or_insert_with(|| {
        json!({
            "owner_principal": {
                "kind": "human_user",
                "id": actor_id,
            },
            "visibility": "private",
            "audience_principals": [],
            "created_by": actor_id,
            "updated_by": actor_id,
        })
    });
    Some(Value::Object(obj))
}

fn apply_automation_v2_share_metadata(
    automation: &mut AutomationV2Spec,
    input: AutomationV2ShareInput,
    verified: Option<&VerifiedTenantContext>,
) -> Result<(), (StatusCode, Json<Value>)> {
    let visibility = input.visibility.unwrap_or_else(|| "private".to_string());
    if !matches!(visibility.as_str(), "private" | "group" | "org") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid automation visibility",
                "code": "AUTOMATION_V2_INVALID_VISIBILITY",
            })),
        ));
    }
    let actor_id = hosted_context_actor_id(verified)
        .or_else(|| automation_v2_access_owner(automation))
        .unwrap_or("unknown")
        .to_string();
    let owner_id = automation_v2_access_owner(automation)
        .or_else(|| hosted_context_actor_id(verified))
        .unwrap_or(&automation.creator_id)
        .to_string();
    let audience = input.audience_principals.unwrap_or_default();
    let mut obj = automation
        .metadata
        .take()
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    obj.insert(
        "resource_access".to_string(),
        json!({
            "owner_principal": {
                "kind": "human_user",
                "id": owner_id,
            },
            "visibility": visibility,
            "audience_principals": audience,
            "updated_by": actor_id,
        }),
    );
    automation.metadata = Some(Value::Object(obj));
    Ok(())
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
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<RoutineRunNowInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_now(
        State(state.clone()),
        Extension(tenant_context),
        Extension(request_principal),
        headers,
        Path(id),
        Json(input),
    )
    .await?;
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
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_approve(
        State(state),
        Extension(tenant_context),
        Extension(request_principal),
        headers,
        Path(run_id),
        Json(input),
    )
    .await?;
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
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_deny(
        State(state),
        Extension(tenant_context),
        Extension(request_principal),
        headers,
        Path(run_id),
        Json(input),
    )
    .await?;
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
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_pause(
        State(state),
        Extension(tenant_context),
        Extension(request_principal),
        headers,
        Path(run_id),
        Json(input),
    )
    .await?;
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
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_resume(
        State(state),
        Extension(tenant_context),
        Extension(request_principal),
        headers,
        Path(run_id),
        Json(input),
    )
    .await?;
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
    agent.mcp_policy.normalize();
    agent
}

#[cfg(test)]
mod normalize_automation_v2_agent_tests {
    use super::*;

    #[test]
    fn preserves_mcp_allowed_tools_without_server_grant() {
        let agent = AutomationAgentProfile {
            agent_id: "gmail-draft-creator".to_string(),
            display_name: "Gmail Draft Creator".to_string(),
            template_id: None,
            avatar_url: None,
            model_policy: None,
            skills: Vec::new(),
            tool_policy: crate::AutomationAgentToolPolicy {
                allowlist: vec![
                    "read".to_string(),
                    "mcp.reddit_gmail.gmail_create_email_draft".to_string(),
                ],
                denylist: Vec::new(),
            },
            mcp_policy: crate::AutomationAgentMcpPolicy {
                allowed_servers: Vec::new(),
                allowed_tools: Some(vec!["mcp.reddit_gmail.gmail_create_email_draft".to_string()]),
                allowed_connections: Vec::new(),
            },
            approval_policy: None,
        };

        let normalized = normalize_automation_v2_agent(agent);

        assert_eq!(normalized.mcp_policy.allowed_servers, Vec::<String>::new());
        assert_eq!(
            normalized.mcp_policy.allowed_tools.as_deref(),
            Some(&["mcp.reddit_gmail.gmail_create_email_draft".to_string()][..])
        );
    }
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
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
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
    let metadata = merge_automation_capabilities_metadata(
        with_automation_v2_private_access_metadata(
            input.metadata,
            verified_tenant_context.as_ref().map(|context| &context.0),
        ),
        input.capabilities,
    )?;
    let declared_capabilities =
        crate::automation_v2::governance::AutomationDeclaredCapabilities::from_metadata(
            metadata.as_ref(),
        );
    state
        .can_create_automation_for_actor(
            &tenant_context,
            &provenance.creator,
            &provenance,
            &declared_capabilities,
        )
        .await
        .map_err(super::governance::governance_error_response)?;
    let mut automation = AutomationV2Spec {
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
            profile: None,
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
    automation.set_tenant_context(&tenant_context);
    automation.stamp_enterprise_scope_metadata();
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

pub(super) async fn automations_v2_list(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Query(query): Query<AutomationsV2ListQuery>,
) -> Json<Value> {
    let verified = verified_tenant_context.as_ref().map(|context| &context.0);
    let rows = state
        .list_automations_v2()
        .await
        .into_iter()
        .filter(|automation| super::tenant_matches(&tenant_context, &automation.tenant_context()))
        .filter(|automation| automation_v2_visible_to_context(automation, verified))
        .collect::<Vec<_>>();
    if query
        .view
        .as_deref()
        .is_some_and(|view| view.eq_ignore_ascii_case("summary"))
    {
        let summaries = rows
            .iter()
            .map(automation_v2_summary_value)
            .collect::<Vec<_>>();
        return Json(json!({
            "automations": summaries,
            "count": summaries.len(),
            "view": "summary",
        }));
    }
    Json(json!({
        "automations": rows,
        "count": rows.len(),
    }))
}

pub(super) async fn automations_v2_get(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(automation) = state.get_automation_v2(&id).await else {
        return Err(automation_v2_not_found(&id));
    };
    ensure_automation_v2_tenant(&tenant_context, &automation)?;
    ensure_automation_v2_visible_to_context(
        &automation,
        verified_tenant_context.as_ref().map(|context| &context.0),
    )?;
    Ok(Json(json!({ "automation": automation })))
}

pub(super) async fn automations_v2_patch(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<AutomationV2PatchInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut automation) = state.get_automation_v2(&id).await else {
        return Err(automation_v2_not_found(&id));
    };
    ensure_automation_v2_tenant(&tenant_context, &automation)?;
    ensure_automation_v2_owner_or_admin(
        &automation,
        verified_tenant_context.as_ref().map(|context| &context.0),
    )?;
    let actor =
        super::governance::resolve_governance_actor(&headers, &tenant_context, &request_principal);
    let governance = state
        .get_or_bootstrap_automation_governance(&automation)
        .await;
    super::governance::enforce_mutation_or_audit(
        &state,
        &tenant_context,
        &id,
        &actor,
        state.can_mutate_automation(&id, &actor, false).await,
    )
    .await?;
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
    automation.set_tenant_context(&tenant_context);
    automation.stamp_enterprise_scope_metadata();
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
        if let Err(error) = state
            .pause_automation_for_dependency_revocation(
                &id,
                "mcp capabilities were narrowed".to_string(),
                evidence,
            )
            .await
        {
            if error
                .to_string()
                .contains("premium governance dependency revocation is not available in this build")
            {
                let _ = crate::audit::append_protected_audit_event(
                    &state,
                    "automation.governance.dependency_revocation_pause_skipped",
                    &tenant_context,
                    actor.actor_id.clone().or_else(|| actor.source.clone()),
                    json!({
                        "automationID": id,
                        "reason": "premium governance dependency revocation is not available in this build",
                        "code": "PREMIUM_FEATURE_REQUIRED",
                    }),
                )
                .await;
            } else {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": error.to_string(),
                        "code": "AUTOMATION_GOVERNANCE_DEPENDENCY_PAUSE_FAILED",
                    })),
                ));
            }
        }
    }
    Ok(Json(json!({ "automation": stored })))
}

pub(super) async fn automations_v2_share(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<AutomationV2ShareInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut automation) = state.get_automation_v2(&id).await else {
        return Err(automation_v2_not_found(&id));
    };
    let verified = verified_tenant_context.as_ref().map(|context| &context.0);
    ensure_automation_v2_tenant(&tenant_context, &automation)?;
    ensure_automation_v2_owner_or_admin(&automation, verified)?;
    // GOV-B7: sharing changes visibility/exposure (e.g. private -> org), so it is a
    // governed mutation rather than an ungoverned metadata write. Resolve the actor
    // and run it through the governance layer (which rejects agent-context callers)
    // before persisting, and write tamper-evident audit evidence.
    let actor =
        super::governance::resolve_governance_actor(&headers, &tenant_context, &request_principal);
    let _ = state
        .get_or_bootstrap_automation_governance(&automation)
        .await;
    super::governance::enforce_mutation_or_audit(
        &state,
        &tenant_context,
        &id,
        &actor,
        state.can_mutate_automation(&id, &actor, false).await,
    )
    .await?;
    apply_automation_v2_share_metadata(&mut automation, input, verified)?;
    automation.stamp_enterprise_scope_metadata();
    automation.updated_at_ms = crate::now_ms();
    let visibility = automation_v2_access_metadata(&automation)
        .and_then(|access| access.get("visibility").and_then(Value::as_str))
        .map(ToString::to_string);
    let stored = state.put_automation_v2(automation).await.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error.to_string(),
                "code": "AUTOMATION_V2_SHARE_FAILED",
            })),
        )
    })?;
    let _ = crate::audit::append_protected_audit_event(
        &state,
        "automation.governance.shared",
        &tenant_context,
        actor.actor_id.clone().or_else(|| actor.source.clone()),
        json!({
            "automationID": stored.automation_id.clone(),
            "visibility": visibility,
            "actor": actor.clone(),
        }),
    )
    .await;
    Ok(Json(json!({ "automation": stored })))
}

pub(super) async fn automations_v2_delete(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(automation) = state.get_automation_v2(&id).await else {
        return Err(automation_v2_not_found(&id));
    };
    ensure_automation_v2_tenant(&tenant_context, &automation)?;
    ensure_automation_v2_owner_or_admin(
        &automation,
        verified_tenant_context.as_ref().map(|context| &context.0),
    )?;
    let actor =
        super::governance::resolve_governance_actor(&headers, &tenant_context, &request_principal);
    let _ = state
        .get_or_bootstrap_automation_governance(&automation)
        .await;
    super::governance::enforce_mutation_or_audit(
        &state,
        &tenant_context,
        &id,
        &actor,
        state.can_mutate_automation(&id, &actor, true).await,
    )
    .await?;
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
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<AutomationV2RunNowInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(automation) = state.get_automation_v2(&id).await else {
        return Err(automation_v2_not_found(&id));
    };
    ensure_automation_v2_tenant(&tenant_context, &automation)?;
    // GOV-B9: executing a run is a mutation-altitude action, not a read. A user with
    // only read visibility (e.g. org-wide visibility) must not be able to trigger a
    // run; require owner/admin.
    ensure_automation_v2_owner_or_admin(
        &automation,
        verified_tenant_context.as_ref().map(|context| &context.0),
    )?;
    let actor =
        super::governance::resolve_governance_actor(&headers, &tenant_context, &request_principal);
    let _ = state
        .get_or_bootstrap_automation_governance(&automation)
        .await;
    super::governance::enforce_mutation_or_audit(
        &state,
        &tenant_context,
        &id,
        &actor,
        state.can_mutate_automation(&id, &actor, false).await,
    )
    .await?;
    let dry_run = input.dry_run;
    let requested_execution_profile = input.execution_profile;
    let run = if dry_run {
        state
            .create_automation_v2_dry_run_with_profile(
                &automation,
                "manual",
                requested_execution_profile,
            )
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
            .create_automation_v2_run_with_profile(
                &automation,
                "manual",
                requested_execution_profile,
            )
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
                crate::stateful_runtime::stamp_automation_run_definition_metadata(row);
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
            "requestedExecutionProfile": requested_execution_profile.map(|p| p.as_str()),
            "effectiveExecutionProfile": run.effective_execution_profile.as_str(),
        }),
    )
    .await;
    let context_run_id = super::context_runs::automation_v2_context_run_id(&run.run_id);
    state.event_bus.publish(crate::EngineEvent::new(
        "automation.v2.run.created",
        json!({
            "automationID": id,
            "runID": run.run_id,
            "run": run.clone(),
            "tenantContext": tenant_context,
            "triggerType": "manual",
            "dryRun": dry_run,
        }),
    ));
    Ok(Json(json!({
        "ok": true,
        "dry_run": dry_run,
        "run": automation_v2_run_with_context_links(&state, &run).await,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
    })))
}
