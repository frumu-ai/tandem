use std::sync::Arc;

use axum::http::StatusCode;
use axum::Json;
use serde_json::Value;
use tandem_core::{
    AgentRegistry, CancellationRegistry, ConfigStore, EngineLoop, EventBus, PermissionManager,
    PluginRegistry, Storage,
};
use tandem_memory::{
    MemoryCapabilityToken, MemoryPromoteRequest, MemoryPromoteResponse, MemoryPutRequest,
    MemoryPutResponse,
};
use tandem_providers::ProviderRegistry;
use tandem_runtime::{LspManager, McpRegistry, PtyManager, WorkspaceIndex};
use tandem_tools::{GovernedToolDispatcher, ToolRegistry};
use tandem_types::HostRuntimeContext;
use tandem_types::{TenantContext, VerifiedTenantContext};
use tokio::sync::RwLock;

use crate::automation_v2::governance::GovernanceActorRef;
use crate::{AppState, RuntimeState};

#[derive(Debug, Clone)]
pub struct EvalAutomationV2GateDecisionInput {
    pub decision: String,
    pub reason: Option<String>,
}

pub struct EvalRuntimeStateParts {
    pub storage: Arc<Storage>,
    pub config: ConfigStore,
    pub event_bus: EventBus,
    pub providers: ProviderRegistry,
    pub plugins: PluginRegistry,
    pub agents: AgentRegistry,
    pub tool_dispatcher: GovernedToolDispatcher,
    pub tools: ToolRegistry,
    pub permissions: PermissionManager,
    pub mcp: McpRegistry,
    pub pty: PtyManager,
    pub lsp: LspManager,
    pub auth: Arc<RwLock<std::collections::HashMap<String, String>>>,
    pub logs: Arc<RwLock<Vec<Value>>>,
    pub workspace_index: WorkspaceIndex,
    pub cancellations: CancellationRegistry,
    pub engine_loop: EngineLoop,
    pub host_runtime_context: HostRuntimeContext,
}

pub async fn mark_eval_state_ready(
    state: &mut AppState,
    parts: EvalRuntimeStateParts,
) -> anyhow::Result<()> {
    let EvalRuntimeStateParts {
        storage,
        config,
        event_bus,
        providers,
        plugins,
        agents,
        tool_dispatcher,
        tools,
        permissions,
        mcp,
        pty,
        lsp,
        auth,
        logs,
        workspace_index,
        cancellations,
        engine_loop,
        host_runtime_context,
    } = parts;

    #[cfg(feature = "browser")]
    let browser = {
        let app_config = config.get().await;
        let browser = crate::BrowserSubsystem::new(app_config.browser.clone());
        let _ = browser.refresh_status().await;
        browser
    };

    state
        .mark_ready(RuntimeState {
            storage,
            config,
            event_bus,
            providers,
            plugins,
            agents,
            tool_dispatcher,
            tools,
            permissions,
            mcp,
            pty,
            lsp,
            auth,
            logs,
            workspace_index,
            cancellations,
            engine_loop,
            host_runtime_context,
            #[cfg(feature = "browser")]
            browser,
        })
        .await
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
            approval_request_id: None,
            transition_id: None,
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
