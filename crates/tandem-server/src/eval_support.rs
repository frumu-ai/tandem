use axum::http::StatusCode;
use axum::Json;
use serde_json::Value;
use tandem_memory::{
    MemoryCapabilityToken, MemoryPromoteRequest, MemoryPromoteResponse, MemoryPutRequest,
    MemoryPutResponse,
};
use tandem_types::{TenantContext, VerifiedTenantContext};

use crate::automation_v2::governance::GovernanceActorRef;
use crate::AppState;

#[derive(Debug, Clone)]
pub struct EvalAutomationV2GateDecisionInput {
    pub decision: String,
    pub reason: Option<String>,
}

pub async fn run_automation_v2_executor(state: AppState) {
    crate::app::state::automation::run_automation_v2_executor(state).await;
}

pub async fn automations_v2_run_gate_decide_inner(
    state: AppState,
    tenant_context: TenantContext,
    verified_tenant_context: Option<VerifiedTenantContext>,
    run_id: String,
    input: EvalAutomationV2GateDecisionInput,
    decider: GovernanceActorRef,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    crate::http::routines_automations::automations_v2_run_gate_decide_inner(
        state,
        tenant_context,
        verified_tenant_context,
        run_id,
        crate::http::routines_automations::AutomationV2GateDecisionInput {
            decision: input.decision,
            reason: input.reason,
        },
        decider,
    )
    .await
}

pub async fn memory_put_impl(
    state: &AppState,
    tenant_context: &TenantContext,
    request: MemoryPutRequest,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryPutResponse, StatusCode> {
    crate::http::memory_put_impl(state, tenant_context, request, capability).await
}

pub async fn memory_promote_impl(
    state: &AppState,
    tenant_context: &TenantContext,
    request: MemoryPromoteRequest,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryPromoteResponse, StatusCode> {
    crate::http::memory_promote_impl(state, tenant_context, request, capability).await
}

pub async fn enrich_verified_context_with_inbound_cross_tenant_grants(
    state: &AppState,
    verified: &mut VerifiedTenantContext,
) {
    crate::http::cross_tenant_grants::enrich_verified_context_with_inbound_cross_tenant_grants(
        state, verified,
    )
    .await;
}
