use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use axum::http::StatusCode;
use serde_json::{json, Value};
use tandem_core::{
    AgentRegistry, CancellationRegistry, ConfigStore, EngineLoop, EventBus, PermissionManager,
    PluginRegistry, Storage,
};
use tandem_memory::types::{
    KnowledgeCoverageRecord, KnowledgeItemRecord, KnowledgeItemStatus, KnowledgePromotionRequest,
    KnowledgeSpaceRecord, MemoryTenantScope,
};
use tandem_memory::{
    GovernedMemoryTier, MemoryCapabilities, MemoryCapabilityToken, MemoryClassification,
    MemoryContentKind, MemoryPartition, MemoryPromoteRequest, MemoryPutRequest, PromotionReview,
    ScrubStatus,
};
use tandem_orchestrator::{KnowledgeScope, KnowledgeTrustLevel};
use tandem_providers::{Provider, ProviderRegistry};
use tandem_runtime::{LspManager, McpRegistry, PtyManager, WorkspaceIndex};
use tandem_tools::{GovernedToolDispatcher, Tool, ToolRegistry};
use tandem_types::{
    approval_authorizes_execution, DataClass, GateOutcome, GateRequest, ResourceKind, ResourceRef,
    TenantContext, ToolResult, ToolRiskTier, ToolSchema,
};

use crate::{ScriptedEvalProvider, SCRIPTED_PROVIDER_ID};
use tandem_server::runtime::state::RuntimeState;
use tandem_server::{
    app::state::AppState, AutomationAgentMcpPolicy, AutomationAgentProfile,
    AutomationAgentToolPolicy, AutomationExecutionPolicy, AutomationFlowNode,
    AutomationFlowOutputContract, AutomationFlowSpec, AutomationOutputValidatorKind,
    AutomationPendingGate, AutomationRunStatus, AutomationV2Schedule, AutomationV2ScheduleType,
    AutomationV2Spec, AutomationV2Status, RoutineMisfirePolicy,
};

#[derive(Debug, Clone)]
pub struct EvalBootstrapOptions {
    pub workspace_root: PathBuf,
    pub state_root: Option<PathBuf>,
    pub scripted_provider: bool,
    pub spawn_executor: bool,
}

impl Default for EvalBootstrapOptions {
    fn default() -> Self {
        Self {
            workspace_root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            state_root: None,
            scripted_provider: true,
            spawn_executor: true,
        }
    }
}

pub async fn bootstrap_eval_app_state(options: EvalBootstrapOptions) -> anyhow::Result<AppState> {
    let state_root = options.state_root.unwrap_or_else(default_state_root);
    std::fs::create_dir_all(&state_root)?;

    let tandem_home = state_root.join("tandem-home");
    let data_dir = tandem_home.join("data");
    std::fs::create_dir_all(&data_dir)?;

    let global_config = state_root.join("global-config.json");
    let mcp_state = state_root.join("mcp.json");
    write_if_missing(&mcp_state, "{}")?;

    // Scope path-based global config resolution to the isolated eval state root.
    std::env::set_var("TANDEM_GLOBAL_CONFIG", &global_config);
    std::env::set_var("TANDEM_HOME", &tandem_home);

    let storage = Arc::new(Storage::new(state_root.join("storage")).await?);
    let config = ConfigStore::new(state_root.join("config.json"), None).await?;
    let event_bus = EventBus::new();
    let app_config = config.get().await;

    #[cfg(feature = "browser")]
    let browser = {
        let browser = tandem_server::BrowserSubsystem::new(app_config.browser.clone());
        let _ = browser.refresh_status().await;
        browser
    };

    let providers = ProviderRegistry::new(app_config.into());
    if options.scripted_provider {
        providers
            .replace_for_test(
                vec![Arc::new(ScriptedEvalProvider::new()) as Arc<dyn Provider>],
                Some(SCRIPTED_PROVIDER_ID.to_string()),
            )
            .await;
    }

    let workspace_root = normalize_workspace_root(&options.workspace_root)?;
    let workspace_root_str = workspace_root.to_string_lossy().to_string();
    let plugins = PluginRegistry::new(&workspace_root_str).await?;
    let agents = AgentRegistry::new(&workspace_root_str).await?;
    let tools = ToolRegistry::new();
    let permissions = PermissionManager::new_with_state_file(
        event_bus.clone(),
        data_dir.join("permissions.json"),
    )
    .await?;
    let mcp = McpRegistry::new_with_state_file(mcp_state);
    let pty = PtyManager::new();
    let lsp = LspManager::new(&workspace_root_str);
    let auth = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
    let logs = Arc::new(tokio::sync::RwLock::new(Vec::new()));
    let workspace_index = WorkspaceIndex::new(&workspace_root_str).await;
    let cancellations = CancellationRegistry::new();
    let host_runtime_context = tandem_server::detect_host_runtime_context();
    let engine_loop = EngineLoop::new(
        storage.clone(),
        event_bus.clone(),
        providers.clone(),
        plugins.clone(),
        agents.clone(),
        permissions.clone(),
        tools.clone(),
        cancellations.clone(),
        host_runtime_context.clone(),
    );

    let mut state = AppState::new_starting(format!("eval-{}", uuid::Uuid::new_v4()), true);
    state.shared_resources_path = data_dir.join("shared_resources.json");
    state.routines_path = data_dir.join("routines.json");
    state.routine_history_path = data_dir.join("routine_history.json");
    state.routine_runs_path = data_dir.join("routine_runs.json");
    state.external_actions_path = data_dir.join("external_actions.json");
    state.policy_decisions_path = data_dir.join("policy_decisions.json");
    state.channel_user_capabilities_path = data_dir.join("channel_user_capabilities.json");
    state.automations_v2_path = data_dir.join("automations_v2.json");
    state.automation_v2_runs_path = data_dir.join("automation_v2_runs.json");
    state.memory_db_path = tandem_home.join("memory.sqlite");
    state.memory_audit_path = data_dir.join("memory_audit.jsonl");
    state.protected_audit_path = data_dir.join("protected_audit.jsonl");

    state
        .mark_ready(RuntimeState {
            storage,
            config,
            event_bus,
            providers,
            plugins,
            agents,
            tool_dispatcher: GovernedToolDispatcher::new(tools.clone()),
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
        .await?;

    seed_eval_tenant_resources(&state).await?;
    state
        .tools
        .register_tool(
            EvalTenantResourceProbeTool::NAME.to_string(),
            Arc::new(EvalTenantResourceProbeTool::new(state.clone())),
        )
        .await;
    state
        .tools
        .register_tool(
            EvalMemoryPromotionProbeTool::NAME.to_string(),
            Arc::new(EvalMemoryPromotionProbeTool::new(state.clone())),
        )
        .await;
    state
        .tools
        .register_tool(
            EvalKnowledgeRetrievalProbeTool::NAME.to_string(),
            Arc::new(EvalKnowledgeRetrievalProbeTool::new(state.clone())),
        )
        .await;
    state
        .tools
        .register_tool(
            EvalActionFirewallProbeTool::NAME.to_string(),
            Arc::new(EvalActionFirewallProbeTool::new(state.clone())),
        )
        .await;
    state
        .tools
        .register_tool(
            crate::cross_tenant_probe::EvalCrossTenantGrantProbeTool::NAME.to_string(),
            Arc::new(crate::cross_tenant_probe::EvalCrossTenantGrantProbeTool::new(state.clone())),
        )
        .await;

    if options.spawn_executor {
        let executor_state = state.clone();
        tokio::spawn(async move {
            tandem_server::eval_support::run_automation_v2_executor(executor_state).await;
        });
    }

    Ok(state)
}

struct EvalActionFirewallProbeTool {
    state: AppState,
}

impl EvalActionFirewallProbeTool {
    const NAME: &'static str = "eval.action_firewall_probe";

    fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Tool for EvalActionFirewallProbeTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            Self::NAME,
            "Eval-only action firewall probe for approval gates, receipt replay, and denied-output promotion.",
            json!({
                "type": "object",
                "properties": {
                    "scenario": {
                        "type": "string",
                        "description": "Action firewall scenario to probe."
                    },
                    "requester_actor_id": {
                        "type": "string",
                        "description": "Actor requesting the action."
                    },
                    "reviewer_actor_id": {
                        "type": "string",
                        "description": "Actor attempting to review the action."
                    },
                    "approval_actor_id": {
                        "type": "string",
                        "description": "Actor bound to the approval receipt."
                    },
                    "approved": {
                        "type": "boolean",
                        "description": "Whether the replayed approval receipt says approved."
                    },
                    "expires_at_ms": {
                        "type": "integer",
                        "description": "Approval receipt expiry time."
                    },
                    "now_ms": {
                        "type": "integer",
                        "description": "Time used for deterministic receipt expiry checks."
                    }
                },
                "required": ["scenario"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.execute_for_tenant(args, TenantContext::local_implicit())
            .await
    }

    async fn execute_for_tenant(
        &self,
        args: Value,
        tenant_context: TenantContext,
    ) -> anyhow::Result<ToolResult> {
        let scenario = args
            .get("scenario")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("missing action firewall scenario"))?;
        let actor_id = args
            .get("requester_actor_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| tenant_context.actor_id.clone())
            .unwrap_or_else(|| "eval-requester".to_string());
        let now_ms = args
            .get("now_ms")
            .and_then(Value::as_u64)
            .unwrap_or_else(tandem_server::now_ms);
        let request = action_firewall_gate_request(scenario)?;

        let (outcome, decision_id) = self
            .state
            .enforce_action_gate(
                &tenant_context,
                &request,
                Some(Self::NAME.to_string()),
                Some(actor_id.clone()),
                now_ms,
            )
            .await;
        let decision_id = decision_id.ok_or_else(|| {
            anyhow::anyhow!("action firewall probe did not record a policy decision")
        })?;
        let decision = self
            .state
            .get_policy_decision(&decision_id)
            .await
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "action firewall policy decision `{}` was not persisted",
                    decision_id
                )
            })?;
        if !protected_audit_contains_decision(&self.state, &decision_id).await? {
            anyhow::bail!(
                "action firewall policy decision `{}` has no protected audit evidence",
                decision_id
            );
        }

        let assertion = action_firewall_assertion(scenario, &args, &outcome, &actor_id, now_ms)?;
        let runtime_guard = self
            .action_firewall_runtime_guard_evidence(scenario, &args, &tenant_context, &actor_id)
            .await?;
        anyhow::bail!(
            "{}",
            json!({
                "action_firewall": "blocked",
                "scenario": scenario,
                "assertion": assertion,
                "runtime_guard": runtime_guard,
                "policy_decision": {
                    "decision_id": decision.decision_id,
                    "effect": decision.decision,
                    "reason_code": decision.reason_code,
                    "policy_id": decision.policy_id,
                },
                "audit_event": "protected_audit_contains_policy_decision",
                "approval_required": outcome.requires_approval(),
                "reviewer_eligibility": outcome.reviewer_eligibility.as_str(),
            })
        )
    }
}

impl EvalActionFirewallProbeTool {
    async fn action_firewall_runtime_guard_evidence(
        &self,
        scenario: &str,
        args: &Value,
        tenant_context: &TenantContext,
        requester_actor_id: &str,
    ) -> anyhow::Result<Value> {
        match scenario {
            "requester_self_approval" => {
                self.assert_requester_self_approval_guard(args, tenant_context, requester_actor_id)
                    .await
            }
            "denied_output_memory_promotion" => {
                self.assert_denied_output_memory_promotion_guard(tenant_context, requester_actor_id)
                    .await
            }
            _ => Ok(Value::Null),
        }
    }

    async fn assert_requester_self_approval_guard(
        &self,
        args: &Value,
        tenant_context: &TenantContext,
        requester_actor_id: &str,
    ) -> anyhow::Result<Value> {
        let reviewer_actor_id = args
            .get("reviewer_actor_id")
            .and_then(Value::as_str)
            .unwrap_or(requester_actor_id);
        if reviewer_actor_id != requester_actor_id {
            anyhow::bail!("self-approval guard requires matching requester/reviewer actors");
        }

        let mut gate_tenant_context = tenant_context.clone();
        gate_tenant_context.actor_id = Some(requester_actor_id.to_string());
        let automation_id = format!(
            "eval-action-firewall-self-approval-{}",
            uuid::Uuid::new_v4()
        );
        let resource = ResourceRef::new(
            gate_tenant_context.org_id.clone(),
            gate_tenant_context.workspace_id.clone(),
            ResourceKind::Approval,
            format!("{automation_id}:publish"),
        );
        let automation = action_firewall_eval_automation(
            &automation_id,
            "Eval Action Firewall self-approval guard",
            requester_actor_id,
            &gate_tenant_context,
        );
        self.state.put_automation_v2(automation.clone()).await?;
        let run = self
            .state
            .create_automation_v2_run(&automation, "eval_action_firewall")
            .await?;
        let run = self
            .state
            .update_automation_v2_run(&run.run_id, |row| {
                row.status = AutomationRunStatus::AwaitingApproval;
                row.checkpoint.completed_nodes = vec![
                    "research".to_string(),
                    "analysis".to_string(),
                    "draft".to_string(),
                ];
                row.checkpoint.pending_nodes = vec!["publish".to_string()];
                row.checkpoint.awaiting_gate = Some(AutomationPendingGate {
                    node_id: "publish".to_string(),
                    title: "Publish approval".to_string(),
                    instructions: Some("approve final publish step".to_string()),
                    decisions: vec![
                        "approve".to_string(),
                        "rework".to_string(),
                        "cancel".to_string(),
                    ],
                    rework_targets: vec!["draft".to_string()],
                    requested_at_ms: tandem_server::now_ms(),
                    upstream_node_ids: vec!["analysis".to_string(), "draft".to_string()],
                    metadata: Some(json!({
                        "gate": {
                            "reviewer_eligibility": "elevated_reviewer",
                            "risk_tier": "financial_record_access",
                            "data_classes": ["financial_record"],
                            "resource": resource,
                        }
                    })),
                    expiry_policy: None,
                });
                row.checkpoint.blocked_nodes = vec!["publish".to_string()];
            })
            .await
            .ok_or_else(|| {
                anyhow::anyhow!("failed to prepare self-approval eval run `{}`", run.run_id)
            })?;

        let result = tandem_server::eval_support::automations_v2_run_gate_decide_inner(
            self.state.clone(),
            gate_tenant_context,
            None,
            run.run_id.clone(),
            tandem_server::eval_support::EvalAutomationV2GateDecisionInput {
                decision: "approve".to_string(),
                reason: None,
            },
            tandem_server::automation_v2::governance::GovernanceActorRef::human(
                Some(reviewer_actor_id.to_string()),
                "eval".to_string(),
            ),
        )
        .await;
        let (status, body) = match result {
            Ok(_) => anyhow::bail!("self-approval guard unexpectedly approved the gate"),
            Err(error) => error,
        };
        if status != StatusCode::FORBIDDEN {
            anyhow::bail!(
                "self-approval guard returned unexpected status {}",
                status.as_u16()
            );
        }
        let code = body
            .0
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if code != "AUTOMATION_V2_GATE_SELF_APPROVAL_FORBIDDEN" {
            anyhow::bail!("self-approval guard returned unexpected code `{}`", code);
        }
        let after = self
            .state
            .get_automation_v2_run(&run.run_id)
            .await
            .ok_or_else(|| anyhow::anyhow!("self-approval eval run disappeared"))?;
        if after.status != AutomationRunStatus::AwaitingApproval {
            anyhow::bail!("self-approval guard advanced the gated run");
        }
        if !after.checkpoint.gate_history.is_empty() {
            anyhow::bail!("self-approval guard recorded a gate decision");
        }
        let protected_audit = tokio::fs::read_to_string(&self.state.protected_audit_path)
            .await
            .map_err(|err| anyhow::anyhow!("failed to read protected audit: {err}"))?;
        if !protected_audit
            .contains("\"event_type\":\"automation.governance.gate_decision_denied\"")
            || !protected_audit.contains("AUTOMATION_V2_GATE_SELF_APPROVAL_FORBIDDEN")
            || !protected_audit.contains(&automation_id)
        {
            anyhow::bail!("self-approval guard did not emit protected audit evidence");
        }

        Ok(json!({
            "guard": "automation_gate_self_approval",
            "status": status.as_u16(),
            "code": code,
            "run_id": run.run_id,
            "automation_id": automation_id,
            "run_status": after.status,
            "gate_history_len": after.checkpoint.gate_history.len(),
            "audit_event": "automation.governance.gate_decision_denied",
        }))
    }

    async fn assert_denied_output_memory_promotion_guard(
        &self,
        tenant_context: &TenantContext,
        requester_actor_id: &str,
    ) -> anyhow::Result<Value> {
        let run_id = format!(
            "eval-action-firewall-denied-output-{}",
            uuid::Uuid::new_v4()
        );
        let project_id = "eval-action-firewall".to_string();
        let partition = MemoryPartition {
            org_id: tenant_context.org_id.clone(),
            workspace_id: tenant_context.workspace_id.clone(),
            project_id: project_id.clone(),
            tier: GovernedMemoryTier::Session,
        };
        let capability = MemoryCapabilityToken {
            run_id: run_id.clone(),
            subject: requester_actor_id.to_string(),
            org_id: partition.org_id.clone(),
            workspace_id: partition.workspace_id.clone(),
            project_id: project_id.clone(),
            memory: MemoryCapabilities {
                read_tiers: vec![GovernedMemoryTier::Session, GovernedMemoryTier::Project],
                write_tiers: vec![GovernedMemoryTier::Session],
                promote_targets: vec![GovernedMemoryTier::Project],
                require_review_for_promote: false,
                allow_auto_use_tiers: vec![GovernedMemoryTier::Curated],
            },
            expires_at: u64::MAX,
        };
        let put_response = tandem_server::eval_support::memory_put_impl(
            &self.state,
            tenant_context,
            MemoryPutRequest {
                run_id: run_id.clone(),
                partition: partition.clone(),
                kind: MemoryContentKind::Note,
                content: "denied output artifact contains aws_secret_access_key=blocked"
                    .to_string(),
                artifact_refs: vec!["artifact://eval/action-firewall/denied-output".to_string()],
                classification: MemoryClassification::Restricted,
                authority_job_context: None,
                metadata: Some(json!({
                    "eval_case": "denied_output_memory_promotion",
                    "source": "denied_output_artifact",
                })),
            },
            Some(capability.clone()),
        )
        .await
        .map_err(|status| {
            anyhow::anyhow!(
                "failed to seed denied-output source memory: {}",
                status.as_u16()
            )
        })?;
        let source_memory_id = put_response.id.clone();
        let request = MemoryPromoteRequest {
            run_id: run_id.clone(),
            source_memory_id: source_memory_id.clone(),
            from_tier: GovernedMemoryTier::Session,
            to_tier: GovernedMemoryTier::Project,
            partition: partition.clone(),
            reason: "eval denied output must not enter governed memory".to_string(),
            review: PromotionReview {
                required: false,
                reviewer_id: Some(requester_actor_id.to_string()),
                approval_id: Some("eval-denied-output-review".to_string()),
            },
            authority_job_context: None,
            source_outcome: None,
        };

        let response = tandem_server::eval_support::memory_promote_impl(
            &self.state,
            tenant_context,
            request,
            Some(capability),
        )
        .await
        .map_err(|status| {
            anyhow::anyhow!(
                "denied-output memory promotion returned unexpected status {}",
                status.as_u16()
            )
        })?;
        if response.promoted || response.new_memory_id.is_some() {
            anyhow::bail!("denied output was promoted into governed memory");
        }
        if response.scrub_report.status != ScrubStatus::Blocked {
            anyhow::bail!(
                "denied-output memory promotion returned unexpected scrub status {:?}",
                response.scrub_report.status
            );
        }
        let block_reason = response
            .scrub_report
            .block_reason
            .as_deref()
            .unwrap_or_default();
        if !matches!(
            block_reason,
            "source memory missing or previously blocked" | "sensitive secret marker detected"
        ) {
            anyhow::bail!(
                "denied-output memory promotion returned unexpected block reason {:?}",
                response.scrub_report.block_reason
            );
        }
        let memory_audit = tokio::fs::read_to_string(&self.state.memory_audit_path)
            .await
            .map_err(|err| anyhow::anyhow!("failed to read memory audit: {err}"))?;
        if !memory_audit.contains(&response.audit_id)
            || !memory_audit.contains(&put_response.audit_id)
            || !memory_audit.contains("\"action\":\"memory_put\"")
            || !memory_audit.contains("\"action\":\"memory_promote\"")
            || !memory_audit.contains("\"status\":\"blocked\"")
            || !memory_audit.contains(&source_memory_id)
            || !memory_audit.contains(block_reason)
        {
            anyhow::bail!("denied-output memory promotion did not emit blocked audit evidence");
        }

        Ok(json!({
            "guard": "governed_memory_promote_denied_output",
            "promoted": response.promoted,
            "new_memory_id": response.new_memory_id,
            "source_memory_id": source_memory_id,
            "source_memory_put_audit_id": put_response.audit_id,
            "to_tier": GovernedMemoryTier::Project,
            "scrub_status": response.scrub_report.status,
            "block_reason": response.scrub_report.block_reason,
            "audit_id": response.audit_id,
            "audit_event": "memory_promote",
            "audit_status": "blocked",
        }))
    }
}

fn action_firewall_gate_request(scenario: &str) -> anyhow::Result<GateRequest> {
    match scenario {
        "restricted_doc_read" => Ok(GateRequest::new(
            Some(ToolRiskTier::ReadDiscover),
            Some(DataClass::Restricted),
        )),
        "financial_record_read" => Ok(GateRequest::new(
            Some(ToolRiskTier::FinancialRecordAccess),
            Some(DataClass::FinancialRecord),
        )),
        "external_send_without_approval" => Ok(GateRequest::external_customer_send()),
        "credential_admin_access" => Ok(GateRequest::new(
            Some(ToolRiskTier::CredentialAdmin),
            Some(DataClass::Credential),
        )),
        "requester_self_approval" => Ok(GateRequest::external_customer_send()),
        "approval_receipt_replay" => Ok(GateRequest::external_customer_send()),
        "denied_output_memory_promotion" => Ok(GateRequest::new(
            Some(ToolRiskTier::InternalWrite),
            Some(DataClass::Restricted),
        )),
        other => anyhow::bail!("unknown action firewall scenario `{}`", other),
    }
}

fn action_firewall_assertion(
    scenario: &str,
    args: &Value,
    outcome: &GateOutcome,
    requester_actor_id: &str,
    now_ms: u64,
) -> anyhow::Result<String> {
    if outcome.is_allowed() {
        anyhow::bail!(
            "action firewall scenario `{}` unexpectedly resolved to allow",
            scenario
        );
    }

    match scenario {
        "requester_self_approval" => {
            let reviewer = args
                .get("reviewer_actor_id")
                .and_then(Value::as_str)
                .unwrap_or(requester_actor_id);
            if reviewer != requester_actor_id {
                anyhow::bail!(
                    "self-approval scenario configured different requester/reviewer actors"
                );
            }
            Ok("self_approval_denied".to_string())
        }
        "approval_receipt_replay" => {
            let approved = args
                .get("approved")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let expires_at_ms = args
                .get("expires_at_ms")
                .and_then(Value::as_u64)
                .unwrap_or_else(|| now_ms.saturating_sub(1));
            let approval_actor = args
                .get("approval_actor_id")
                .and_then(Value::as_str)
                .unwrap_or("foreign-reviewer");
            let actor_matches = approval_actor == requester_actor_id;
            let time_valid = approval_authorizes_execution(approved, expires_at_ms, now_ms);
            if time_valid && actor_matches {
                anyhow::bail!("approval receipt replay unexpectedly authorized execution");
            }
            Ok("approval_receipt_replay_denied".to_string())
        }
        "denied_output_memory_promotion" => {
            Ok("denied_output_memory_promotion_blocked".to_string())
        }
        _ if outcome.requires_approval() => Ok(outcome.reason_code.clone()),
        _ if outcome.is_denied() => Ok(outcome.reason_code.clone()),
        _ => anyhow::bail!(
            "action firewall scenario `{}` resolved to unsupported outcome",
            scenario
        ),
    }
}

async fn protected_audit_contains_decision(
    state: &AppState,
    decision_id: &str,
) -> anyhow::Result<bool> {
    let raw = match tokio::fs::read_to_string(&state.protected_audit_path).await {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    Ok(raw.contains(decision_id))
}

fn action_firewall_eval_automation(
    automation_id: &str,
    name: &str,
    creator_id: &str,
    tenant_context: &TenantContext,
) -> AutomationV2Spec {
    let now = tandem_server::now_ms();
    let mut automation = AutomationV2Spec {
        automation_id: automation_id.to_string(),
        name: name.to_string(),
        description: Some("Eval-only governed automation for Action Firewall probes.".to_string()),
        status: AutomationV2Status::Active,
        schedule: AutomationV2Schedule {
            schedule_type: AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::Skip,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: vec![AutomationAgentProfile {
            agent_id: "eval-action-firewall-agent".to_string(),
            template_id: None,
            display_name: "Eval Action Firewall Agent".to_string(),
            avatar_url: None,
            model_policy: None,
            skills: Vec::new(),
            tool_policy: AutomationAgentToolPolicy {
                allowlist: Vec::new(),
                denylist: Vec::new(),
            },
            mcp_policy: AutomationAgentMcpPolicy {
                allowed_servers: Vec::new(),
                allowed_tools: None,
                allowed_connections: Vec::new(),
            },
            approval_policy: None,
        }],
        flow: AutomationFlowSpec {
            nodes: ["research", "analysis", "draft", "publish"]
                .iter()
                .map(|node_id| AutomationFlowNode {
                    node_id: (*node_id).to_string(),
                    agent_id: "eval-action-firewall-agent".to_string(),
                    objective: format!("Eval {node_id} step"),
                    knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    depends_on: Vec::new(),
                    input_refs: Vec::new(),
                    output_contract: Some(AutomationFlowOutputContract {
                        kind: "artifact".to_string(),
                        validator: Some(AutomationOutputValidatorKind::GenericArtifact),
                        enforcement: None,
                        schema: None,
                        summary_guidance: None,
                    }),
                    tool_policy: None,
                    mcp_policy: None,
                    retry_policy: None,
                    timeout_ms: None,
                    max_tool_calls: None,
                    stage_kind: None,
                    gate: None,
                    metadata: None,
                })
                .collect(),
        },
        execution: AutomationExecutionPolicy {
            profile: None,
            max_parallel_agents: Some(1),
            max_total_runtime_ms: Some(60_000),
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: now,
        updated_at_ms: now,
        creator_id: creator_id.to_string(),
        workspace_root: None,
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    automation.set_tenant_context(tenant_context);
    automation
}

struct EvalTenantResourceProbeTool {
    state: AppState,
}

impl EvalTenantResourceProbeTool {
    const NAME: &'static str = "eval.tenant_resource_probe";
    const SEEDED_KEY: &'static str = "project/eval/ct02-tenant-b-source";

    fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Tool for EvalTenantResourceProbeTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            Self::NAME,
            "Eval-only tenant scoped shared-resource probe.",
            json!({
                "type": "object",
                "properties": {
                    "resource_key": {
                        "type": "string",
                        "description": "Shared resource key to read through the executing tenant context."
                    },
                    "attempted_tenant_id": {
                        "type": "string",
                        "description": "Human-readable tenant the eval is attempting to cross into."
                    }
                },
                "required": ["resource_key", "attempted_tenant_id"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.execute_for_tenant(args, TenantContext::local_implicit())
            .await
    }

    async fn execute_for_tenant(
        &self,
        args: Value,
        tenant_context: TenantContext,
    ) -> anyhow::Result<ToolResult> {
        let resource_key = args
            .get("resource_key")
            .and_then(Value::as_str)
            .unwrap_or(Self::SEEDED_KEY);
        let attempted_tenant_id = args
            .get("attempted_tenant_id")
            .and_then(Value::as_str)
            .unwrap_or("unknown");

        if let Some(record) = self
            .state
            .get_shared_resource_for_tenant(resource_key, &tenant_context)
            .await
        {
            return Ok(ToolResult {
                output: "tenant-scoped resource visible to executing tenant".to_string(),
                metadata: json!({
                    "resource_key": record.key,
                    "executing_tenant": tenant_context,
                    "attempted_tenant_id": attempted_tenant_id,
                    "visible": true,
                }),
            });
        }

        anyhow::bail!(
            "tenant-scoped resource `{}` is denied or not visible to executing tenant {}/{} while attempting {}",
            resource_key,
            tenant_context.org_id,
            tenant_context.workspace_id,
            attempted_tenant_id
        )
    }
}

struct EvalMemoryPromotionProbeTool {
    state: AppState,
}

impl EvalMemoryPromotionProbeTool {
    const NAME: &'static str = "eval.memory_promotion_probe";

    fn new(state: AppState) -> Self {
        Self { state }
    }

    async fn open_memory_manager(&self) -> anyhow::Result<tandem_memory::MemoryManager> {
        if let Some(parent) = self.state.memory_db_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tandem_memory::MemoryManager::new(&self.state.memory_db_path)
            .await
            .map_err(|err| anyhow::anyhow!("failed to open eval memory manager: {err}"))
    }
}

#[async_trait]
impl Tool for EvalMemoryPromotionProbeTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            Self::NAME,
            "Eval-only promoted knowledge tenant-scope probe.",
            json!({
                "type": "object",
                "properties": {
                    "space_id": {
                        "type": "string",
                        "description": "Knowledge space id seeded under the attempted tenant."
                    },
                    "item_id": {
                        "type": "string",
                        "description": "Knowledge item id seeded and promoted under the attempted tenant."
                    },
                    "coverage_key": {
                        "type": "string",
                        "description": "Coverage key for the promoted knowledge item."
                    },
                    "attempted_tenant_id": {
                        "type": "string",
                        "description": "Human-readable tenant the eval is attempting to cross into."
                    }
                },
                "required": ["space_id", "item_id", "coverage_key", "attempted_tenant_id"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.execute_for_tenant(args, TenantContext::local_implicit())
            .await
    }

    async fn execute_for_tenant(
        &self,
        args: Value,
        tenant_context: TenantContext,
    ) -> anyhow::Result<ToolResult> {
        let space_id = args
            .get("space_id")
            .and_then(Value::as_str)
            .unwrap_or("ct03-tenant-b-promoted-space");
        let item_id = args
            .get("item_id")
            .and_then(Value::as_str)
            .unwrap_or("ct03-tenant-b-promoted-item");
        let coverage_key = args
            .get("coverage_key")
            .and_then(Value::as_str)
            .unwrap_or("ct03::tenant-b::promotion");
        let attempted_tenant_id = args
            .get("attempted_tenant_id")
            .and_then(Value::as_str)
            .unwrap_or("tenant-b");

        let manager = self.open_memory_manager().await?;
        let owner_scope = eval_memory_scope(attempted_tenant_id);
        let executing_scope = memory_scope_from_tenant_context(&tenant_context);
        let now = chrono::Utc::now().timestamp_millis() as u64;

        let space = KnowledgeSpaceRecord {
            id: space_id.to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("eval-shared-project".to_string()),
            namespace: Some("eval/tenant-isolation".to_string()),
            title: Some(format!("{attempted_tenant_id} promoted eval knowledge")),
            description: None,
            trust_level: KnowledgeTrustLevel::Promoted,
            metadata: Some(json!({"eval_case": "ct-03"})),
            created_at_ms: now,
            updated_at_ms: now,
        };
        manager
            .upsert_knowledge_space_for_tenant(&space, &owner_scope)
            .await
            .map_err(|err| anyhow::anyhow!("failed to seed eval knowledge space: {err}"))?;

        let item = KnowledgeItemRecord {
            id: item_id.to_string(),
            space_id: space_id.to_string(),
            coverage_key: coverage_key.to_string(),
            dedupe_key: format!("{coverage_key}::dedupe"),
            item_type: "runbook".to_string(),
            title: format!("{attempted_tenant_id} promoted eval runbook"),
            summary: Some("Tenant-owned promoted knowledge fixture.".to_string()),
            payload: json!({
                "tenant": attempted_tenant_id,
                "fixture": "ct-03 promoted knowledge must remain tenant scoped"
            }),
            trust_level: KnowledgeTrustLevel::Working,
            status: KnowledgeItemStatus::Working,
            run_id: Some("ct-03-eval-bootstrap".to_string()),
            artifact_refs: vec![format!("artifact://{attempted_tenant_id}/ct-03")],
            source_memory_ids: vec![format!("memory://{attempted_tenant_id}/ct-03")],
            freshness_expires_at_ms: None,
            metadata: Some(json!({"eval_case": "ct-03"})),
            created_at_ms: now,
            updated_at_ms: now,
        };
        manager
            .upsert_knowledge_item_for_tenant(&item, &owner_scope)
            .await
            .map_err(|err| anyhow::anyhow!("failed to seed eval knowledge item: {err}"))?;

        let promote = KnowledgePromotionRequest {
            item_id: item_id.to_string(),
            target_status: KnowledgeItemStatus::Promoted,
            promoted_at_ms: now + 1,
            freshness_expires_at_ms: Some(now + 86_400_000),
            reviewer_id: None,
            approval_id: None,
            reason: Some("ct-03 eval tenant scope probe".to_string()),
        };
        manager
            .promote_knowledge_item_for_tenant(&promote, &owner_scope)
            .await
            .map_err(|err| anyhow::anyhow!("failed to promote eval knowledge item: {err}"))?
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "failed to promote eval knowledge item `{}` for attempted tenant {}",
                    item_id,
                    attempted_tenant_id
                )
            })?;

        let visible_item = manager
            .get_knowledge_item_for_tenant(item_id, &executing_scope)
            .await
            .map_err(|err| anyhow::anyhow!("failed to read eval knowledge item: {err}"))?;
        let visible_items = manager
            .list_knowledge_items_for_tenant(space_id, Some(coverage_key), &executing_scope)
            .await
            .map_err(|err| anyhow::anyhow!("failed to list eval knowledge items: {err}"))?;
        let visible_coverage = manager
            .get_knowledge_coverage_for_tenant(coverage_key, space_id, &executing_scope)
            .await
            .map_err(|err| anyhow::anyhow!("failed to read eval knowledge coverage: {err}"))?;

        if visible_item.is_some() || !visible_items.is_empty() || visible_coverage.is_some() {
            return Ok(ToolResult {
                output: "promoted knowledge item visible to executing tenant".to_string(),
                metadata: json!({
                    "space_id": space_id,
                    "item_id": item_id,
                    "coverage_key": coverage_key,
                    "executing_tenant": tenant_context,
                    "attempted_tenant_id": attempted_tenant_id,
                    "visible": true,
                }),
            });
        }

        anyhow::bail!(
            "promoted knowledge item `{}` is denied or not visible to executing tenant {}/{} while attempting {}",
            item_id,
            tenant_context.org_id,
            tenant_context.workspace_id,
            attempted_tenant_id
        )
    }
}

struct EvalKnowledgeRetrievalProbeTool {
    state: AppState,
}

impl EvalKnowledgeRetrievalProbeTool {
    const NAME: &'static str = "eval.knowledge_retrieval_probe";

    fn new(state: AppState) -> Self {
        Self { state }
    }

    async fn open_memory_manager(&self) -> anyhow::Result<tandem_memory::MemoryManager> {
        if let Some(parent) = self.state.memory_db_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tandem_memory::MemoryManager::new(&self.state.memory_db_path)
            .await
            .map_err(|err| anyhow::anyhow!("failed to open eval memory manager: {err}"))
    }
}

#[async_trait]
impl Tool for EvalKnowledgeRetrievalProbeTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            Self::NAME,
            "Eval-only knowledge-base retrieval tenant-scope probe.",
            json!({
                "type": "object",
                "properties": {
                    "space_id": {
                        "type": "string",
                        "description": "Knowledge space id seeded under the attempted tenant."
                    },
                    "item_id": {
                        "type": "string",
                        "description": "Knowledge item id seeded under the attempted tenant."
                    },
                    "coverage_key": {
                        "type": "string",
                        "description": "Coverage key for the seeded knowledge item."
                    },
                    "attempted_tenant_id": {
                        "type": "string",
                        "description": "Human-readable tenant the eval is attempting to cross into."
                    }
                },
                "required": ["space_id", "item_id", "coverage_key", "attempted_tenant_id"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.execute_for_tenant(args, TenantContext::local_implicit())
            .await
    }

    async fn execute_for_tenant(
        &self,
        args: Value,
        tenant_context: TenantContext,
    ) -> anyhow::Result<ToolResult> {
        let space_id = args
            .get("space_id")
            .and_then(Value::as_str)
            .unwrap_or("ct08-tenant-b-knowledge-space");
        let item_id = args
            .get("item_id")
            .and_then(Value::as_str)
            .unwrap_or("ct08-tenant-b-knowledge-item");
        let coverage_key = args
            .get("coverage_key")
            .and_then(Value::as_str)
            .unwrap_or("ct08::tenant-b::knowledge");
        let attempted_tenant_id = args
            .get("attempted_tenant_id")
            .and_then(Value::as_str)
            .unwrap_or("tenant-b");

        let manager = self.open_memory_manager().await?;
        let owner_scope = eval_memory_scope(attempted_tenant_id);
        let executing_scope = memory_scope_from_tenant_context(&tenant_context);
        let now = chrono::Utc::now().timestamp_millis() as u64;

        // Seed a knowledge space + item owned by the attempted tenant. Unlike the
        // CT-03 promotion probe, this case never promotes: it proves the plain
        // retrieval path (item/list/coverage reads) is tenant-scoped on its own.
        let space = KnowledgeSpaceRecord {
            id: space_id.to_string(),
            scope: KnowledgeScope::Project,
            project_id: Some("eval-shared-project".to_string()),
            namespace: Some("eval/tenant-isolation".to_string()),
            title: Some(format!("{attempted_tenant_id} eval knowledge base")),
            description: None,
            trust_level: KnowledgeTrustLevel::Working,
            metadata: Some(json!({"eval_case": "ct-08"})),
            created_at_ms: now,
            updated_at_ms: now,
        };
        manager
            .upsert_knowledge_space_for_tenant(&space, &owner_scope)
            .await
            .map_err(|err| anyhow::anyhow!("failed to seed eval knowledge space: {err}"))?;

        let item = KnowledgeItemRecord {
            id: item_id.to_string(),
            space_id: space_id.to_string(),
            coverage_key: coverage_key.to_string(),
            dedupe_key: format!("{coverage_key}::dedupe"),
            item_type: "runbook".to_string(),
            title: format!("{attempted_tenant_id} eval knowledge entry"),
            summary: Some("Tenant-owned knowledge-base fixture.".to_string()),
            payload: json!({
                "tenant": attempted_tenant_id,
                "fixture": "ct-08 knowledge retrieval must remain tenant scoped"
            }),
            trust_level: KnowledgeTrustLevel::Working,
            status: KnowledgeItemStatus::Working,
            run_id: Some("ct-08-eval-bootstrap".to_string()),
            artifact_refs: vec![format!("artifact://{attempted_tenant_id}/ct-08")],
            source_memory_ids: vec![format!("memory://{attempted_tenant_id}/ct-08")],
            freshness_expires_at_ms: None,
            metadata: Some(json!({"eval_case": "ct-08"})),
            created_at_ms: now,
            updated_at_ms: now,
        };
        manager
            .upsert_knowledge_item_for_tenant(&item, &owner_scope)
            .await
            .map_err(|err| anyhow::anyhow!("failed to seed eval knowledge item: {err}"))?;

        let coverage = KnowledgeCoverageRecord {
            coverage_key: coverage_key.to_string(),
            space_id: space_id.to_string(),
            latest_item_id: Some(item_id.to_string()),
            latest_dedupe_key: Some(format!("{coverage_key}::dedupe")),
            last_seen_at_ms: now,
            last_promoted_at_ms: None,
            freshness_expires_at_ms: None,
            metadata: Some(json!({"eval_case": "ct-08"})),
        };
        manager
            .upsert_knowledge_coverage_for_tenant(&coverage, &owner_scope)
            .await
            .map_err(|err| anyhow::anyhow!("failed to seed eval knowledge coverage: {err}"))?;

        let visible_item = manager
            .get_knowledge_item_for_tenant(item_id, &executing_scope)
            .await
            .map_err(|err| anyhow::anyhow!("failed to read eval knowledge item: {err}"))?;
        let visible_items = manager
            .list_knowledge_items_for_tenant(space_id, Some(coverage_key), &executing_scope)
            .await
            .map_err(|err| anyhow::anyhow!("failed to list eval knowledge items: {err}"))?;
        let visible_coverage = manager
            .get_knowledge_coverage_for_tenant(coverage_key, space_id, &executing_scope)
            .await
            .map_err(|err| anyhow::anyhow!("failed to read eval knowledge coverage: {err}"))?;

        if visible_item.is_some() || !visible_items.is_empty() || visible_coverage.is_some() {
            return Ok(ToolResult {
                output: "tenant-scoped knowledge item visible to executing tenant".to_string(),
                metadata: json!({
                    "space_id": space_id,
                    "item_id": item_id,
                    "coverage_key": coverage_key,
                    "executing_tenant": tenant_context,
                    "attempted_tenant_id": attempted_tenant_id,
                    "visible": true,
                }),
            });
        }

        anyhow::bail!(
            "tenant-scoped knowledge item `{}` is denied or not visible to executing tenant {}/{} while attempting {}",
            item_id,
            tenant_context.org_id,
            tenant_context.workspace_id,
            attempted_tenant_id
        )
    }
}

async fn seed_eval_tenant_resources(state: &AppState) -> anyhow::Result<()> {
    state
        .put_shared_resource_for_tenant(
            EvalTenantResourceProbeTool::SEEDED_KEY.to_string(),
            json!({
                "classification": "confidential",
                "fixture": "ct02 tenant-b source",
                "must_not_be_visible_to": "tenant-a"
            }),
            None,
            "eval-bootstrap".to_string(),
            None,
            &eval_tenant_context("tenant-b"),
        )
        .await
        .map_err(|err| anyhow::anyhow!("failed to seed eval tenant resource: {:?}", err))?;
    Ok(())
}

fn memory_scope_from_tenant_context(tenant_context: &TenantContext) -> MemoryTenantScope {
    MemoryTenantScope {
        org_id: tenant_context.org_id.clone(),
        workspace_id: tenant_context.workspace_id.clone(),
        deployment_id: tenant_context.deployment_id.clone(),
    }
}

fn eval_memory_scope(tenant_id: &str) -> MemoryTenantScope {
    memory_scope_from_tenant_context(&eval_tenant_context(tenant_id))
}

fn eval_tenant_context(tenant_id: &str) -> TenantContext {
    TenantContext::explicit_user_workspace(
        tenant_id,
        "eval-workspace",
        Some("eval-deployment".to_string()),
        format!("{tenant_id}-eval-actor"),
    )
}

fn default_state_root() -> PathBuf {
    std::env::temp_dir().join(format!("tandem-eval-{}", uuid::Uuid::new_v4()))
}

fn write_if_missing(path: &Path, contents: &str) -> anyhow::Result<()> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, contents)?;
    }
    Ok(())
}

fn normalize_workspace_root(path: &Path) -> anyhow::Result<PathBuf> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}
