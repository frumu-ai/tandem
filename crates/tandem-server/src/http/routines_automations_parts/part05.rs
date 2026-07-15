// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

// Continuation split from part01.rs for the file-size gate (same module via include!).

pub(super) fn automation_create_to_routine(
    input: AutomationCreateInput,
) -> Result<RoutineSpec, String> {
    if input.mission.objective.trim().is_empty() {
        return Err("mission.objective is required".to_string());
    }
    let mode = normalize_automation_mode(input.mode.as_deref())?;
    let mut args = json!({
        "prompt": input.mission.objective.trim(),
        "success_criteria": input.mission.success_criteria,
        "mode": mode,
    });
    if let Some(briefing) = input.mission.briefing {
        if let Some(obj) = args.as_object_mut() {
            obj.insert("briefing".to_string(), Value::String(briefing));
        }
    }
    if let Some(policy) = input.policy.as_ref() {
        if let Some(value) = policy.tool.orchestrator_only_tool_calls {
            if let Some(obj) = args.as_object_mut() {
                obj.insert(
                    "orchestrator_only_tool_calls".to_string(),
                    Value::Bool(value),
                );
            }
        }
    }
    if let Some(model_policy) = input.model_policy {
        validate_model_policy(&model_policy)?;
        if let Some(obj) = args.as_object_mut() {
            obj.insert("model_policy".to_string(), model_policy);
        }
    }
    let (allowed_tools, external_integrations_allowed, requires_approval) =
        if let Some(policy) = input.policy {
            (
                policy.tool.run_allowlist.unwrap_or_default(),
                policy.tool.external_integrations_allowed.unwrap_or(false),
                policy.approval.requires_approval.unwrap_or(true),
            )
        } else {
            (Vec::new(), false, true)
        };
    Ok(RoutineSpec {
        routine_id: input
            .automation_id
            .unwrap_or_else(|| format!("automation-{}", uuid::Uuid::new_v4().simple())),
        tenant_context: TenantContext::local_implicit(),
        name: input.name,
        status: RoutineStatus::Active,
        schedule: input.schedule,
        timezone: input.timezone.unwrap_or_else(|| "UTC".to_string()),
        misfire_policy: input
            .misfire_policy
            .unwrap_or(RoutineMisfirePolicy::RunOnce),
        entrypoint: input
            .mission
            .entrypoint_compat
            .unwrap_or_else(|| "mission.default".to_string()),
        args: Value::Object(args.as_object().cloned().unwrap_or_default()),
        allowed_tools,
        output_targets: input.output_targets.unwrap_or_default(),
        creator_type: input.creator_type.unwrap_or_else(|| "user".to_string()),
        creator_id: input.creator_id.unwrap_or_else(|| "desktop".to_string()),
        requires_approval,
        external_integrations_allowed,
        next_fire_at_ms: input.next_fire_at_ms,
        last_fired_at_ms: None,
    })
}

pub(super) async fn automations_create(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Json(input): Json<AutomationCreateInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut routine = automation_create_to_routine(input).map_err(|detail| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid automation definition",
                "code": "AUTOMATION_INVALID",
                "detail": detail,
            })),
        )
    })?;
    routine.tenant_context = tenant_context;
    let saved = state
        .put_routine(routine)
        .await
        .map_err(routine_error_response)?;
    state
        .event_bus
        .publish(crate::routines::types::tenant_scoped_engine_event(
            "automation.updated",
            &saved.tenant_context,
            json!({
                "automationID": saved.routine_id,
            }),
        ));
    Ok(Json(json!({
        "automation": routine_to_automation_wire(saved)
    })))
}

pub(super) async fn automations_list(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
) -> Json<Value> {
    let rows = state
        .list_routines_for_tenant(&tenant_context)
        .await
        .into_iter()
        .map(routine_to_automation_wire)
        .collect::<Vec<_>>();
    Json(json!({
        "automations": rows,
        "count": rows.len(),
    }))
}
