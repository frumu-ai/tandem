// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

async fn workflow_pack_export_plan_id(
    state: &AppState,
    tenant_context: &tandem_types::TenantContext,
    verified: Option<&tandem_types::VerifiedTenantContext>,
    plan_id: &str,
) -> Result<(crate::WorkflowPlan, u32), (StatusCode, Json<Value>)> {
    ensure_workflow_plan_id_access(state, tenant_context, verified, plan_id, false).await?;
    if let Some(draft) = state
        .get_workflow_plan_draft_scoped(plan_id, tenant_context, verified)
        .await
    {
        return Ok((draft.current_plan, draft.plan_revision));
    }
    if tenant_context.is_local_implicit() {
        if let Some(plan) = state.get_workflow_plan(plan_id).await {
            return Ok((plan, 1));
        }
    }
    Err((
        StatusCode::NOT_FOUND,
        Json(json!({"error": "workflow plan not found"})),
    ))
}

async fn workflow_plan_pack_export_bundle(
    state: &AppState,
    tenant_context: &tandem_types::TenantContext,
    verified: Option<&tandem_types::VerifiedTenantContext>,
    input: &WorkflowPlanPackExportRequest,
) -> Result<(crate::WorkflowPlan, u32), (StatusCode, Json<Value>)> {
    if let Some(session_id) = input
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let session = state
            .get_workflow_planner_session(session_id)
            .await
            .ok_or_else(|| {
                (
                    StatusCode::NOT_FOUND,
                    Json(json!({"error": "workflow planner session not found"})),
                )
            })?;
        ensure_workflow_planner_session_access(state, &session, tenant_context, verified, false)
            .await?;
        if let Some(plan_id) = session
            .draft
            .as_ref()
            .map(|draft| draft.current_plan.plan_id.as_str())
            .or(session.current_plan_id.as_deref())
        {
            return workflow_pack_export_plan_id(state, tenant_context, verified, plan_id).await;
        }
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "workflow session does not contain an exportable plan"})),
        ));
    }
    if let Some(plan_id) = input
        .plan_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return workflow_pack_export_plan_id(state, tenant_context, verified, plan_id).await;
    }
    Err((
        StatusCode::BAD_REQUEST,
        Json(json!({"error": "export requires session_id or plan_id"})),
    ))
}

fn workflow_pack_export_actor_scope(
    tenant_context: &tandem_types::TenantContext,
    verified: Option<&tandem_types::VerifiedTenantContext>,
    plan_id: &str,
) -> Result<Option<String>, (StatusCode, Json<Value>)> {
    if tenant_context.is_local_implicit() {
        return Ok(None);
    }
    let actor_id = workflow_plan_mutation_actor_id(tenant_context, verified)?;
    let identity = serde_json::to_string(&(
        "workflow-pack-export-v1",
        &tenant_context.org_id,
        &tenant_context.workspace_id,
        &tenant_context.deployment_id,
        &actor_id,
        plan_id,
    ))
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "failed to scope workflow pack export"})),
        )
    })?;
    Ok(Some(crate::sha256_hex(&[&identity])))
}

async fn workflow_pack_authorize_hosted_export_path(
    state: &AppState,
    tenant_context: &tandem_types::TenantContext,
    verified: Option<&tandem_types::VerifiedTenantContext>,
    requested: &FsPath,
    plan_id: Option<&str>,
) -> Result<PathBuf, StatusCode> {
    let plan_id = plan_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(StatusCode::NOT_FOUND)?;
    ensure_workflow_plan_id_access(state, tenant_context, verified, plan_id, false)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let scope = workflow_pack_export_actor_scope(tenant_context, verified, plan_id)
        .map_err(|_| StatusCode::NOT_FOUND)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let root = state
        .pack_manager
        .workflow_pack_exports_root()
        .canonicalize()
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let scoped_root = root
        .join(scope)
        .canonicalize()
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let path = requested
        .canonicalize()
        .map_err(|_| StatusCode::NOT_FOUND)?;
    if !scoped_root.starts_with(&root)
        || path.parent() != Some(scoped_root.as_path())
        || path.extension().and_then(|value| value.to_str()) != Some("zip")
    {
        return Err(StatusCode::NOT_FOUND);
    }
    Ok(path)
}
