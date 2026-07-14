pub(super) async fn workflow_plan_apply(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<tandem_types::TenantContext>,
    verified_tenant_context: Option<Extension<tandem_types::VerifiedTenantContext>>,
    Json(input): Json<WorkflowPlanApplyRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let requested_creator_id = input.creator_id.clone();
    let apply_idempotency_key = input
        .idempotency_key
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if apply_idempotency_key
        .as_ref()
        .is_some_and(|key| key.len() > 256)
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "idempotency_key must not exceed 256 characters",
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        ));
    }
    let creator_id = workflow_plan_mutation_actor_id(
        &tenant_context,
        verified_tenant_context
            .as_ref()
            .map(|Extension(verified)| verified),
    )?;
    let plan_id = input
        .plan_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let plan = match (input.plan, plan_id.as_deref()) {
        (Some(plan), _) => plan,
        (None, Some(plan_id)) => state.get_workflow_plan(plan_id).await.ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "workflow plan not found",
                    "code": "WORKFLOW_PLAN_NOT_FOUND",
                    "plan_id": plan_id,
                })),
            )
        })?,
        (None, None) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "plan or plan_id is required",
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            ));
        }
    };
    if compiler_api::workflow_plan_generated_task_budget_exceeded(&plan) {
        return Err(workflow_plan_task_budget_exceeded_error(&plan));
    }
    compiler_api::validate_workflow_plan(&plan).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error,
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        )
    })?;
    let draft_context = if let Some(plan_id) = plan_id.as_deref() {
        state.get_workflow_plan_draft(plan_id).await
    } else {
        None
    };
    let apply_revision = draft_context
        .as_ref()
        .map(|draft| draft.plan_revision)
        .unwrap_or(1);
    let planner_diagnostics = draft_context
        .as_ref()
        .and_then(|draft| draft.planner_diagnostics.clone());
    let plan_json = compiler_api::workflow_plan_to_json(&plan).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error,
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        )
    })?;
    let mut plan_package = compiler_api::compile_workflow_plan_preview_package_with_revision(
        &plan_json,
        Some("workflow_planner"),
        apply_revision,
    );
    let plan_package_validation = compiler_api::validate_plan_package(&plan_package);
    if plan_package_validation.blocker_count > 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "plan package validation failed",
                "code": "WORKFLOW_PLAN_INVALID",
                "plan_package": plan_package,
                "plan_package_validation": plan_package_validation,
            })),
        ));
    }
    let requested_overlap_decision = parse_overlap_decision(input.overlap_decision.as_deref())
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error,
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    let plan_json_text = serde_json::to_string(&plan_json).map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": format!("failed to fingerprint workflow plan: {error}"),
                "code": "WORKFLOW_PLAN_APPLY_FAILED",
            })),
        )
    })?;
    let apply_revision_text = apply_revision.to_string();
    let pack_builder_export_text = serde_json::to_string(&input.pack_builder_export)
        .unwrap_or_else(|_| "null".to_string());
    let materialization_mode = if input.materialize_as_draft {
        "draft"
    } else {
        "active"
    };
    let apply_idempotency_fingerprint = apply_idempotency_key.as_ref().map(|_| {
        crate::sha256_hex(&[
            plan_id.as_deref().unwrap_or(&plan.plan_id),
            &plan_json_text,
            &apply_revision_text,
            input.overlap_decision.as_deref().unwrap_or(""),
            materialization_mode,
            &pack_builder_export_text,
            &creator_id,
        ])
    });
    if let (Some(key), Some(fingerprint)) = (
        apply_idempotency_key.as_deref(),
        apply_idempotency_fingerprint.as_deref(),
    ) {
        if let Some(record) = state
            .get_idempotency_key(&tenant_context, "workflow_plan.apply", key)
            .await
        {
            if record.request_fingerprint != fingerprint {
                return Err((
                    StatusCode::CONFLICT,
                    Json(json!({
                        "error": "idempotency key is already bound to a different workflow apply request",
                        "code": "WORKFLOW_PLAN_IDEMPOTENCY_CONFLICT",
                    })),
                ));
            }
            if let Some(outcome) = record.outcome {
                return Ok(Json(outcome.details));
            }
            return Err((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "workflow plan apply is already in progress",
                    "code": "WORKFLOW_PLAN_APPLY_IN_PROGRESS",
                    "retryable": true,
                })),
            ));
        }
    }
    let mut overlap_analysis = compile_preview_plan_overlap(&state, &plan_package).await;
    if overlap_analysis.requires_user_confirmation && requested_overlap_decision.is_none() {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "overlap confirmation is required before apply",
                "code": "WORKFLOW_PLAN_OVERLAP_CONFIRMATION_REQUIRED",
                "plan_package": plan_package,
                "plan_package_validation": plan_package_validation,
                "overlap_analysis": overlap_analysis,
            })),
        ));
    }
    if overlap_analysis.matched_plan_id.is_none() && requested_overlap_decision.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "overlap_decision was provided but no prior overlap was detected",
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        ));
    }
    if let Some(decision) = requested_overlap_decision {
        overlap_analysis.decision = decision;
        overlap_analysis.requires_user_confirmation = false;
    }
    if let Some(entry) = compiler_api::overlap_log_entry_from_analysis(
        &overlap_analysis,
        &creator_id,
        &chrono::Utc::now().to_rfc3339(),
    ) {
        plan_package
            .overlap_policy
            .get_or_insert_with(Default::default)
            .overlap_log
            .push(entry);
    }

    if let (Some(key), Some(fingerprint)) = (
        apply_idempotency_key.as_deref(),
        apply_idempotency_fingerprint.as_deref(),
    ) {
        let reservation = state
            .reserve_idempotency_key(crate::app::state::IdempotencyReservationInput {
                tenant_context: tenant_context.clone(),
                operation: "workflow_plan.apply".to_string(),
                key: key.to_string(),
                owner: creator_id.clone(),
                request_fingerprint: fingerprint.to_string(),
                first_seen_event_id: None,
                now_ms: crate::now_ms(),
                expires_at_ms: None,
            })
            .await
            .map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": format!("failed to reserve workflow apply: {error}"),
                        "code": "WORKFLOW_PLAN_APPLY_FAILED",
                    })),
                )
            })?;
        match reservation {
            crate::app::state::IdempotencyReservation::Reserved(_) => {}
            crate::app::state::IdempotencyReservation::Duplicate(record) => {
                if let Some(outcome) = record.outcome {
                    return Ok(Json(outcome.details));
                }
                return Err((
                    StatusCode::CONFLICT,
                    Json(json!({
                        "error": "workflow plan apply is already in progress",
                        "code": "WORKFLOW_PLAN_APPLY_IN_PROGRESS",
                        "retryable": true,
                    })),
                ));
            }
            crate::app::state::IdempotencyReservation::Conflict(_) => {
                return Err((
                    StatusCode::CONFLICT,
                    Json(json!({
                        "error": "idempotency key is already bound to a different workflow apply request",
                        "code": "WORKFLOW_PLAN_IDEMPOTENCY_CONFLICT",
                    })),
                ));
            }
        }
    }

    let mut automation =
        compile_plan_to_automation_v2(&plan, Some(&plan_package), &creator_id);
    if input.materialize_as_draft {
        automation.status = crate::AutomationV2Status::Draft;
        automation.next_fire_at_ms = None;
    }
    let approved_plan_materialization = compiler_api::approved_plan_materialization(&plan_package);
    let approved_plan_success_memory =
        compiler_api::approved_plan_success_memory_value(&plan_package);
    let plan_package_bundle = compiler_api::export_plan_package_bundle(&plan_package);
    if let Some(metadata) = automation.metadata.as_mut().and_then(Value::as_object_mut) {
        metadata.insert(
            "plan_source".to_string(),
            serde_json::to_value(&plan.plan_source).unwrap_or(Value::Null),
        );
        metadata.insert(
            "plan_package".to_string(),
            serde_json::to_value(&plan_package).unwrap_or(Value::Null),
        );
        metadata.insert(
            "plan_package_bundle".to_string(),
            serde_json::to_value(&plan_package_bundle).unwrap_or(Value::Null),
        );
        metadata.insert(
            "plan_package_validation".to_string(),
            serde_json::to_value(&plan_package_validation).unwrap_or(Value::Null),
        );
        metadata.insert(
            "overlap_analysis".to_string(),
            serde_json::to_value(&overlap_analysis).unwrap_or(Value::Null),
        );
        metadata.insert(
            "approved_plan_materialization".to_string(),
            approved_plan_success_memory.clone(),
        );
        metadata.insert(
            "planner_diagnostics".to_string(),
            planner_diagnostics.clone().unwrap_or(Value::Null),
        );
        metadata.insert(
            "authoring_actor_id".to_string(),
            json!(creator_id.clone()),
        );
        metadata.insert(
            "requested_creator_id".to_string(),
            requested_creator_id.clone().map(Value::String).unwrap_or(Value::Null),
        );
    } else {
        automation.metadata = Some(json!({
            "plan_package": plan_package,
            "plan_package_bundle": plan_package_bundle.clone(),
            "plan_package_validation": plan_package_validation,
            "overlap_analysis": overlap_analysis,
            "approved_plan_materialization": approved_plan_success_memory.clone(),
            "planner_diagnostics": planner_diagnostics,
            "authoring_actor_id": creator_id.clone(),
            "requested_creator_id": requested_creator_id.clone(),
        }));
    }
    automation.set_tenant_context(&tenant_context);
    let mut recovered_automation = None;
    if let (Some(key), Some(fingerprint)) = (
        apply_idempotency_key.as_deref(),
        apply_idempotency_fingerprint.as_deref(),
    ) {
        let stable_digest = crate::sha256_hex(&[
            &tenant_context.org_id,
            &tenant_context.workspace_id,
            tenant_context.deployment_id.as_deref().unwrap_or(""),
            "workflow_plan.apply",
            key,
            fingerprint,
        ]);
        automation.automation_id = format!("automation-v2-idem-{}", &stable_digest[..32]);
        if let Some(metadata) = automation.metadata.as_mut().and_then(Value::as_object_mut) {
            metadata.insert(
                "workflow_plan_apply_idempotency_fingerprint".to_string(),
                Value::String(fingerprint.to_string()),
            );
        }
        if let Some(existing) = state.get_automation_v2(&automation.automation_id).await {
            let same_tenant = super::ensure_same_tenant(
                &tenant_context,
                &existing.tenant_context(),
            )
            .is_ok();
            let same_request = existing
                .metadata
                .as_ref()
                .and_then(|metadata| {
                    metadata
                        .get("workflow_plan_apply_idempotency_fingerprint")
                        .and_then(Value::as_str)
                })
                == Some(fingerprint);
            if !same_tenant || !same_request {
                return Err((
                    StatusCode::CONFLICT,
                    Json(json!({
                        "error": "stable materialization identifier is already in use",
                        "code": "WORKFLOW_PLAN_IDEMPOTENCY_CONFLICT",
                    })),
                ));
            }
            recovered_automation = Some(existing);
        }
    }
    let stored = match recovered_automation {
        Some(existing) => existing,
        None => match state.put_automation_v2(automation).await {
            Ok(stored) => stored,
            Err(error) => {
                if let (Some(key), Some(fingerprint)) = (
                    apply_idempotency_key.as_deref(),
                    apply_idempotency_fingerprint.as_deref(),
                ) {
                    let _ = state
                        .release_reserved_idempotency_key(
                            &tenant_context,
                            "workflow_plan.apply",
                            key,
                            fingerprint,
                        )
                        .await;
                }
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": error.to_string(),
                        "code": "WORKFLOW_PLAN_APPLY_FAILED",
                    })),
                ));
            }
        },
    };
    if let Err(audit_error) = append_workflow_plan_materialization_audit(
        &state,
        &tenant_context,
        &creator_id,
        requested_creator_id.as_deref(),
        plan_id.as_deref(),
        &stored.automation_id,
        &plan.plan_source,
    )
    .await
    {
        let release_error = if let (Some(key), Some(fingerprint)) = (
            apply_idempotency_key.as_deref(),
            apply_idempotency_fingerprint.as_deref(),
        ) {
            state
                .release_reserved_idempotency_key(
                    &tenant_context,
                    "workflow_plan.apply",
                    key,
                    fingerprint,
                )
                .await
                .err()
        } else {
            None
        };
        let rollback_error = state
            .rollback_automation_v2_creation(&stored.automation_id)
            .await
            .err();
        if rollback_error.is_none() && release_error.is_none() {
            tracing::error!(
                automation_id = %stored.automation_id,
                error = ?audit_error,
                "workflow plan materialization rolled back after protected audit failure"
            );
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Automation creation was rolled back because its required audit record could not be persisted",
                    "code": "PROTECTED_AUDIT_PERSISTENCE_FAILED",
                    "retryable": true,
                    "operationApplied": false,
                })),
            ));
        }
        let operation_applied = rollback_error.is_some();
        tracing::error!(
            automation_id = %stored.automation_id,
            error = ?audit_error,
            rollback_error = ?rollback_error,
            idempotency_release_error = ?release_error,
            operation_applied,
            "workflow plan materialization compensation was incomplete after protected audit failure"
        );
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": if operation_applied {
                    "The operation was applied, but its required audit record could not be persisted and rollback did not complete"
                } else {
                    "Automation creation was rolled back after its required audit record failed, but retry state could not be released"
                },
                "code": "PROTECTED_AUDIT_PERSISTENCE_FAILED",
                "retryable": release_error.is_none(),
                "operationApplied": operation_applied,
            })),
        ));
    }
    if let Some(plan_id) = plan_id.as_deref() {
        if let Some(mut draft) = state.get_workflow_plan_draft(plan_id).await {
            draft.last_success_materialization = Some(approved_plan_success_memory);
            state.put_workflow_plan_draft(draft).await;
        }
    }
    let pack_builder_export = match input.pack_builder_export {
        Some(export) if export.enabled.unwrap_or(true) => {
            Some(export_workflow_plan_to_pack_builder(&state, &plan, &export).await)
        }
        _ => None,
    };
    let response = json!({
        "ok": true,
        "plan": plan,
        "plan_package": plan_package,
        "plan_package_bundle": plan_package_bundle,
        "overlap_analysis": overlap_analysis,
        "approved_plan_materialization": approved_plan_materialization,
        "automation": stored,
        "pack_builder_export": pack_builder_export,
    });
    if let (Some(key), Some(fingerprint)) = (
        apply_idempotency_key.as_deref(),
        apply_idempotency_fingerprint.as_deref(),
    ) {
        state
            .complete_idempotency_key(
                &tenant_context,
                "workflow_plan.apply",
                key,
                crate::app::state::IdempotencyKeyOutcome {
                    outcome_kind: "materialized".to_string(),
                    completed_at_ms: crate::now_ms(),
                    primary_ref_kind: Some("automation".to_string()),
                    primary_ref_id: response
                        .pointer("/automation/automation_id")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    secondary_ref_kind: Some("workflow_plan".to_string()),
                    secondary_ref_id: Some(plan_id.unwrap_or_else(|| {
                        response
                            .pointer("/plan/plan_id")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string()
                    })),
                    details: response.clone(),
                },
                crate::now_ms(),
            )
            .await
            .map_err(|error| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": format!("failed to persist workflow apply outcome: {error}"),
                        "code": "WORKFLOW_PLAN_APPLY_FAILED",
                        "idempotency_fingerprint": fingerprint,
                    })),
                )
            })?;
    }
    Ok(Json(response))
}
