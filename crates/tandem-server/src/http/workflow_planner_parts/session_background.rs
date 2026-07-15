// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

pub(super) async fn workflow_planner_session_start_background(
    state: AppState,
    session_id: String,
    input: WorkflowPlannerSessionStartRequest,
    request_id: String,
    tenant_context: tandem_types::TenantContext,
    verified_tenant_context: Option<tandem_types::VerifiedTenantContext>,
) {
    let result = workflow_planner_session_start(
        State(state.clone()),
        Path(session_id.clone()),
        Extension(tenant_context),
        verified_tenant_context.map(Extension),
        Json(input),
    )
    .await;
    workflow_planner_session_store_operation_result(
        &state,
        &session_id,
        &request_id,
        "start",
        result,
    )
    .await;
}

pub(super) async fn workflow_planner_session_message_background(
    state: AppState,
    session_id: String,
    input: WorkflowPlannerSessionMessageRequest,
    request_id: String,
    tenant_context: tandem_types::TenantContext,
    verified_tenant_context: Option<tandem_types::VerifiedTenantContext>,
) {
    let result = workflow_planner_session_message(
        State(state.clone()),
        Path(session_id.clone()),
        Extension(tenant_context),
        verified_tenant_context.map(Extension),
        Json(input),
    )
    .await;
    workflow_planner_session_store_operation_result(
        &state,
        &session_id,
        &request_id,
        "message",
        result,
    )
    .await;
}
