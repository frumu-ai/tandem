use tandem_core::{
    fintech_policy_decision_payload, fintech_protected_action_hash, fintech_strict_tool_decision,
    metadata_enables_fintech_strict, FintechProtectedActionCategory,
    FintechToolPolicyClassification,
};
use tandem_orchestrator::{
    AgentInstance, AgentInstanceStatus, AgentRole, AgentTemplate, BudgetLimit, SpawnDecision,
    SpawnDenyCode, SpawnPolicy, SpawnRequest, SpawnSource,
};
use tandem_skills::SkillService;
use tandem_types::{EngineEvent, RuntimeAuthMode, Session, TenantContext, VerifiedTenantContext};
use tokio::fs;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::config::env::resolve_runtime_auth_mode;
use crate::AppState;

#[derive(Clone, Default)]
pub struct AgentTeamRuntime {
    policy: Arc<RwLock<Option<SpawnPolicy>>>,
    templates: Arc<RwLock<HashMap<String, AgentTemplate>>>,
    instances: Arc<RwLock<HashMap<String, AgentInstance>>>,
    budgets: Arc<RwLock<HashMap<String, InstanceBudgetState>>>,
    mission_budgets: Arc<RwLock<HashMap<String, MissionBudgetState>>>,
    spawn_approvals: Arc<RwLock<HashMap<String, PendingSpawnApproval>>>,
    loaded_workspace: Arc<RwLock<Option<String>>>,
    audit_path: Arc<RwLock<PathBuf>>,
}

#[derive(Debug, Clone)]
pub struct SpawnResult {
    pub decision: SpawnDecision,
    pub instance: Option<AgentInstance>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentMissionSummary {
    #[serde(rename = "missionID")]
    pub mission_id: String,
    #[serde(rename = "instanceCount")]
    pub instance_count: usize,
    #[serde(rename = "runningCount")]
    pub running_count: usize,
    #[serde(rename = "completedCount")]
    pub completed_count: usize,
    #[serde(rename = "failedCount")]
    pub failed_count: usize,
    #[serde(rename = "cancelledCount")]
    pub cancelled_count: usize,
    #[serde(rename = "queuedCount")]
    pub queued_count: usize,
    #[serde(rename = "tokenUsedTotal")]
    pub token_used_total: u64,
    #[serde(rename = "toolCallsUsedTotal")]
    pub tool_calls_used_total: u64,
    #[serde(rename = "stepsUsedTotal")]
    pub steps_used_total: u64,
    #[serde(rename = "costUsedUsdTotal")]
    pub cost_used_usd_total: f64,
}

#[derive(Debug, Clone, Default)]
struct InstanceBudgetState {
    tokens_used: u64,
    steps_used: u32,
    tool_calls_used: u32,
    cost_used_usd: f64,
    started_at: Option<Instant>,
    exhausted: bool,
}

#[derive(Debug, Clone, Default)]
struct MissionBudgetState {
    tokens_used: u64,
    steps_used: u64,
    tool_calls_used: u64,
    cost_used_usd: f64,
    exhausted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PendingSpawnApproval {
    #[serde(rename = "approvalID")]
    pub approval_id: String,
    #[serde(rename = "createdAtMs")]
    pub created_at_ms: u64,
    pub request: SpawnRequest,
    #[serde(rename = "decisionCode")]
    pub decision_code: Option<SpawnDenyCode>,
    pub reason: Option<String>,
}

#[derive(Clone)]
pub struct ServerSpawnAgentHook {
    state: AppState,
}

#[derive(Debug, Deserialize)]
struct SpawnAgentToolInput {
    #[serde(rename = "missionID")]
    mission_id: Option<String>,
    #[serde(rename = "parentInstanceID")]
    parent_instance_id: Option<String>,
    #[serde(rename = "templateID")]
    template_id: Option<String>,
    role: AgentRole,
    source: Option<SpawnSource>,
    justification: String,
    #[serde(rename = "budgetOverride", default)]
    budget_override: Option<BudgetLimit>,
}

impl ServerSpawnAgentHook {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

impl SpawnAgentHook for ServerSpawnAgentHook {
    fn spawn_agent(
        &self,
        ctx: SpawnAgentToolContext,
    ) -> BoxFuture<'static, anyhow::Result<SpawnAgentToolResult>> {
        let state = self.state.clone();
        Box::pin(async move {
            let parsed = serde_json::from_value::<SpawnAgentToolInput>(ctx.args.clone());
            let input = match parsed {
                Ok(input) => input,
                Err(err) => {
                    return Ok(SpawnAgentToolResult {
                        output: format!("spawn_agent denied: invalid args ({err})"),
                        metadata: json!({
                            "ok": false,
                            "code": "SPAWN_INVALID_ARGS",
                            "error": err.to_string(),
                        }),
                    });
                }
            };
            let req = SpawnRequest {
                mission_id: input.mission_id,
                parent_instance_id: input.parent_instance_id,
                source: input.source.unwrap_or(SpawnSource::ToolCall),
                parent_role: None,
                role: input.role,
                template_id: input.template_id,
                justification: input.justification,
                budget_override: input.budget_override,
            };

            let event_ctx = SpawnEventContext {
                session_id: Some(ctx.session_id.as_str()),
                message_id: Some(ctx.message_id.as_str()),
                run_id: None,
            };
            emit_spawn_requested_with_context(&state, &req, &event_ctx);
            let result = state.agent_teams.spawn(&state, req.clone()).await;
            if !result.decision.allowed || result.instance.is_none() {
                emit_spawn_denied_with_context(&state, &req, &result.decision, &event_ctx);
                return Ok(SpawnAgentToolResult {
                    output: result
                        .decision
                        .reason
                        .clone()
                        .unwrap_or_else(|| "spawn_agent denied".to_string()),
                    metadata: json!({
                        "ok": false,
                        "code": result.decision.code,
                        "error": result.decision.reason,
                        "requiresUserApproval": result.decision.requires_user_approval,
                    }),
                });
            }
            let instance = result.instance.expect("checked is_some");
            emit_spawn_approved_with_context(&state, &req, &instance, &event_ctx);
            Ok(SpawnAgentToolResult {
                output: format!(
                    "spawned {} as instance {} (session {})",
                    instance.template_id, instance.instance_id, instance.session_id
                ),
                metadata: json!({
                    "ok": true,
                    "missionID": instance.mission_id,
                    "instanceID": instance.instance_id,
                    "sessionID": instance.session_id,
                    "runID": instance.run_id,
                    "status": instance.status,
                    "skillHash": instance.skill_hash,
                    "workspaceRoot": instance_workspace_root(&instance),
                    "workspaceRepoRoot": instance_workspace_repo_root(&instance),
                    "managedWorktree": instance_managed_worktree(&instance),
                }),
            })
        })
    }
}

#[derive(Clone)]
pub struct ServerToolPolicyHook {
    state: AppState,
}

impl ServerToolPolicyHook {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

fn automation_tool_target_paths(tool: &str, args: &Value) -> Vec<String> {
    let mut paths = Vec::new();
    match tool {
        "write" | "edit" => {
            if let Some(path) = args.get("path").and_then(Value::as_str) {
                let trimmed = path.trim();
                if !trimmed.is_empty() {
                    paths.push(trimmed.to_string());
                }
            }
        }
        "apply_patch" => {
            let patch = args
                .get("patchText")
                .and_then(Value::as_str)
                .or_else(|| args.as_str());
            if let Some(patch) = patch {
                for line in patch.lines() {
                    for prefix in [
                        "*** Update File: ",
                        "*** Delete File: ",
                        "*** Add File: ",
                        "*** Move to: ",
                    ] {
                        if let Some(path) = line.strip_prefix(prefix) {
                            let trimmed = path.trim();
                            if !trimmed.is_empty() {
                                paths.push(trimmed.to_string());
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
    paths.sort();
    paths.dedup();
    paths
}

fn automation_path_references_read_only_source_of_truth(
    path: &str,
    read_only_names: &std::collections::HashSet<String>,
    workspace_root: Option<&str>,
) -> bool {
    let trimmed = path.trim().trim_matches('`');
    if trimmed.is_empty() {
        return false;
    }
    let lowered = trimmed.to_ascii_lowercase();
    if read_only_names.contains(&lowered) {
        return true;
    }
    if let Some(filename) = std::path::Path::new(trimmed)
        .file_name()
        .and_then(|value| value.to_str())
    {
        if read_only_names.contains(&filename.to_ascii_lowercase()) {
            return true;
        }
    }
    workspace_root
        .and_then(|root| {
            crate::app::state::automation::normalize_workspace_display_path(root, trimmed)
        })
        .is_some_and(|normalized| {
            let normalized_lower = normalized.to_ascii_lowercase();
            if read_only_names.contains(&normalized_lower) {
                return true;
            }
            std::path::Path::new(&normalized)
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|filename| read_only_names.contains(&filename.to_ascii_lowercase()))
        })
}

async fn evaluate_automation_read_only_write_deny(
    state: &AppState,
    session_id: &str,
    tool: &str,
    args: &Value,
) -> Option<String> {
    if !matches!(tool, "write" | "edit" | "apply_patch") {
        return None;
    }
    let run_id = state
        .automation_v2_session_runs
        .read()
        .await
        .get(session_id)
        .cloned()?;
    let run = state.get_automation_v2_run(&run_id).await?;
    let automation = run.automation_snapshot?;
    let read_only_names =
        crate::app::state::automation::enforcement::automation_read_only_source_of_truth_name_variants_for_automation(
            &automation,
        );
    if read_only_names.is_empty() {
        return None;
    }
    let workspace_root = args
        .get("__workspace_root")
        .and_then(Value::as_str)
        .or(automation.workspace_root.as_deref());
    let blocked_paths = automation_tool_target_paths(tool, args)
        .into_iter()
        .filter(|path| {
            automation_path_references_read_only_source_of_truth(
                path,
                &read_only_names,
                workspace_root,
            )
        })
        .collect::<Vec<_>>();
    if blocked_paths.is_empty() {
        None
    } else {
        Some(format!(
            "write denied for automation `{}` (run `{}`): read-only source-of-truth file(s) cannot be mutated: {}",
            automation.automation_id,
            run_id,
            blocked_paths.join(", ")
        ))
    }
}

async fn evaluate_fintech_strict_tool_policy(
    state: &AppState,
    session_id: &str,
    message_id: &str,
    tenant_context: Option<&TenantContext>,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    runtime_auth_mode: RuntimeAuthMode,
    tool: &str,
    args: &Value,
) -> Option<ToolPolicyDecision> {
    let run_id = state
        .automation_v2_session_runs
        .read()
        .await
        .get(session_id)
        .cloned()?;
    let run = state.get_automation_v2_run(&run_id).await?;
    let automation = run.automation_snapshot.as_ref()?;
    if !metadata_enables_fintech_strict(automation.metadata.as_ref()) {
        return None;
    }
    if let Some(ctx) = tenant_context {
        if ctx.org_id != run.tenant_context.org_id
            || ctx.workspace_id != run.tenant_context.workspace_id
            || ctx.deployment_id != run.tenant_context.deployment_id
        {
            let reason =
                "tool denied by runtime policy: session tenant context does not match automation run tenant context"
                    .to_string();
            let policy_decision = record_runtime_policy_decision(
                state,
                &run,
                session_id,
                message_id,
                tool,
                PolicyDecisionEffect::Deny,
                "tenant_context_mismatch",
                &reason,
                None,
                verified_tenant_context,
                json!({"runtime_profile": FINTECH_STRICT_PROFILE}),
            )
            .await;
            let policy_decision_id = policy_decision
                .as_ref()
                .map(|record| record.decision_id.clone());
            return Some(ToolPolicyDecision {
                allowed: false,
                reason: Some(reason),
                policy_decision_id,
            });
        }
    }

    let decision = fintech_strict_tool_decision(tool);
    if decision.allowed {
        return None;
    }
    if runtime_auth_mode_requires_verified_tool_context(runtime_auth_mode) {
        if let Some(reason) = strict_tool_context_deny_reason(
            tenant_context,
            verified_tenant_context,
            crate::now_ms(),
        ) {
            let policy_decision = record_runtime_policy_decision(
                state,
                &run,
                session_id,
                message_id,
                tool,
                PolicyDecisionEffect::Deny,
                "strict_tenant_context_required",
                &reason,
                None,
                verified_tenant_context,
                json!({"runtime_profile": FINTECH_STRICT_PROFILE}),
            )
            .await;
            let policy_decision_id = policy_decision
                .as_ref()
                .map(|record| record.decision_id.clone());
            return Some(ToolPolicyDecision {
                allowed: false,
                reason: Some(reason),
                policy_decision_id,
            });
        }
    }
    let reason = decision.reason.unwrap_or_else(|| {
        "fintech strict mode blocked tool execution by runtime policy".to_string()
    });
    let payload = fintech_policy_decision_payload(tool, &decision.classification, &reason);
    if let FintechToolPolicyClassification::RequiresApproval(category) = decision.classification {
        if let Some(approval) =
            matching_fintech_protected_action_approval(&run, tool, args, category)
        {
            let audit_payload = json!({
                "run_id": run.run_id,
                "automation_id": automation.automation_id,
                "session_id": session_id,
                "message_id": message_id,
                "tool": tool,
                "policy": payload,
                "requester_context": verified_tenant_context
                    .and_then(tandem_types::GovernanceRequesterContext::from_verified_context),
                "approval_verification": {
                    "status": "verified",
                    "effect": "allow",
                    "approval": approval,
                },
            });
            if let Err(error) = crate::audit::append_protected_audit_event(
                state,
                "fintech.protected_action.approved",
                &run.tenant_context,
                run.tenant_context.actor_id.clone(),
                audit_payload,
            )
            .await
            {
                tracing::error!(
                    error = ?error,
                    "fintech protected action denied because allow audit persistence failed"
                );
                return Some(ToolPolicyDecision {
                    allowed: false,
                    reason: Some(
                        "tool denied by runtime policy: required allow audit could not be persisted"
                            .to_string(),
                    ),
                    policy_decision_id: None,
                });
            }
            if state.is_ready() {
                state.event_bus.publish(EngineEvent::new(
                    "fintech.protected_action.approved",
                    json!({
                        "sessionID": session_id,
                        "messageID": message_id,
                        "runID": run.run_id,
                        "automationID": automation.automation_id,
                        "tool": tool,
                        "category": category.as_str(),
                        "approval": approval,
                        "timestampMs": crate::now_ms(),
                        "tenantContext": run.tenant_context.clone(),
                    }),
                ));
            }
            let policy_decision = record_runtime_policy_decision(
                state,
                &run,
                session_id,
                message_id,
                tool,
                PolicyDecisionEffect::Allow,
                "matching_approval_receipt",
                "protected action approved by matching receipt",
                Some(&approval),
                verified_tenant_context,
                json!({
                    "policy": payload,
                    "approval_verification": {
                        "status": "verified",
                        "effect": "allow",
                        "approval": approval,
                    },
                }),
            )
            .await;
            let Some(policy_decision) = policy_decision else {
                return Some(ToolPolicyDecision {
                    allowed: false,
                    reason: Some(
                        "tool denied by runtime policy: policy decision could not be recorded"
                            .to_string(),
                    ),
                    policy_decision_id: None,
                });
            };
            let policy_decision_id = Some(policy_decision.decision_id.clone());
            if !matches!(policy_decision.decision, PolicyDecisionEffect::Allow) {
                return Some(ToolPolicyDecision {
                    allowed: false,
                    reason: Some(format!(
                        "tool blocked by enterprise policy ({}): {}",
                        policy_decision.reason_code, policy_decision.reason
                    )),
                    policy_decision_id,
                });
            }
            return Some(ToolPolicyDecision {
                allowed: true,
                reason: None,
                policy_decision_id,
            });
        }
    }
    if state.is_ready() {
        state.event_bus.publish(EngineEvent::new(
            "fintech.protected_action.denied",
            json!({
                "sessionID": session_id,
                "messageID": message_id,
                "runID": run.run_id,
                "automationID": automation.automation_id,
                "tool": tool,
                "classification": payload.get("classification").cloned().unwrap_or(Value::Null),
                "category": payload.get("category").cloned().unwrap_or(Value::Null),
                "reason": reason,
                "timestampMs": crate::now_ms(),
                "tenantContext": run.tenant_context.clone(),
            }),
        ));
    }
    let audit_payload = json!({
        "run_id": run.run_id,
        "automation_id": automation.automation_id,
        "session_id": session_id,
        "message_id": message_id,
        "tool": tool,
        "policy": payload,
        "requester_context": verified_tenant_context
            .and_then(tandem_types::GovernanceRequesterContext::from_verified_context),
        "approval_verification": {
            "status": "unavailable",
            "effect": "fail_closed",
            "reason": "call-site protected-action approval/policy verification is not implemented"
        },
    });
    crate::audit::append_protected_audit_event_best_effort(
        state,
        "fintech.protected_action.denied",
        &run.tenant_context,
        run.tenant_context.actor_id.clone(),
        audit_payload,
    )
    .await;

    let effect = if matches!(
        decision.classification,
        FintechToolPolicyClassification::RequiresApproval(_)
    ) {
        PolicyDecisionEffect::ApprovalRequired
    } else {
        PolicyDecisionEffect::Deny
    };
    let reason_code = if matches!(effect, PolicyDecisionEffect::ApprovalRequired) {
        "approval_required_unverified"
    } else {
        "blocked_unknown_mutation"
    };
    let policy_reason = match decision.classification {
        FintechToolPolicyClassification::RequiresApproval(category) => format!(
            "{} Runtime denied protected fintech action `{}` fail-closed; approval gates are not treated as authorization until a matching approval/policy record can be verified at the tool call.",
            reason,
            category.as_str()
        ),
        FintechToolPolicyClassification::BlockedUnknownMutation => reason.clone(),
        FintechToolPolicyClassification::Safe => reason.clone(),
    };
    let policy_decision = record_runtime_policy_decision(
        state,
        &run,
        session_id,
        message_id,
        tool,
        effect,
        reason_code,
        &policy_reason,
        None,
        verified_tenant_context,
        json!({
            "policy": payload,
            "approval_verification": {
                "status": "unavailable",
                "effect": "fail_closed",
                "reason": "call-site protected-action approval/policy verification is not implemented"
            },
        }),
    )
    .await;
    let policy_decision_id = policy_decision
        .as_ref()
        .map(|record| record.decision_id.clone());

    Some(ToolPolicyDecision {
        allowed: false,
        reason: Some(policy_reason),
        policy_decision_id,
    })
}

#[allow(clippy::too_many_arguments)]
async fn record_runtime_policy_decision(
    state: &AppState,
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    session_id: &str,
    message_id: &str,
    tool: &str,
    effect: PolicyDecisionEffect,
    reason_code: &str,
    reason: &str,
    approval: Option<&Value>,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    metadata: Value,
) -> Option<PolicyDecisionRecord> {
    let decision_id = format!("policy_decision_{}", Uuid::new_v4().simple());
    let approval_id = approval
        .and_then(|value| value.get("approval_id").or_else(|| value.get("approvalID")))
        .and_then(Value::as_str)
        .or_else(|| {
            approval
                .and_then(|value| value.get("gate_node_id"))
                .and_then(Value::as_str)
        })
        .map(str::to_string);
    let node_id = approval
        .and_then(|value| value.get("gate_node_id"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let risk_tier = metadata
        .pointer("/policy/risk_tier")
        .or_else(|| metadata.pointer("/risk_tier"))
        .or_else(|| metadata.pointer("/policy/category"))
        .or_else(|| metadata.pointer("/policy/classification"))
        .and_then(Value::as_str)
        .map(str::to_string);
    let record = PolicyDecisionRecord {
        decision_id: decision_id.clone(),
        tenant_context: run.tenant_context.clone(),
        requester_context: verified_tenant_context
            .and_then(tandem_types::GovernanceRequesterContext::from_verified_context),
        actor_id: run.tenant_context.actor_id.clone(),
        session_id: Some(session_id.to_string()),
        message_id: Some(message_id.to_string()),
        run_id: Some(run.run_id.clone()),
        automation_id: Some(run.automation_id.clone()),
        node_id,
        tool: Some(tool.to_string()),
        resource: None,
        data_classes: Vec::new(),
        risk_tier,
        decision: effect,
        reason_code: reason_code.to_string(),
        reason: reason.to_string(),
        policy_id: Some(FINTECH_STRICT_PROFILE.to_string()),
        grant_id: None,
        approval_id,
        audit_event_id: None,
        created_at_ms: crate::now_ms(),
        metadata,
    };
    match state.record_policy_decision(record).await {
        Ok(record) => Some(record),
        Err(error) => {
            tracing::warn!("failed to record policy decision: {error:?}");
            None
        }
    }
}

fn runtime_auth_mode_requires_verified_tool_context(mode: RuntimeAuthMode) -> bool {
    matches!(
        mode,
        RuntimeAuthMode::HostedSingleTenant | RuntimeAuthMode::EnterpriseRequired
    )
}

/// The security descriptor of the registered tool with this (normalized)
/// name, or `None` when the name is not in the registry.
async fn registered_tool_security_descriptor(
    state: &AppState,
    tool: &str,
) -> Option<ToolSecurityDescriptor> {
    let normalized = normalize_tool_name(tool);
    state
        .tools
        .list()
        .await
        .into_iter()
        .find(|schema| normalize_tool_name(&schema.name) == normalized)
        .map(|schema| tandem_core::tool_schema_security_descriptor(&schema))
}

/// CT-20: resolve a tool against the declarative approval gate matrix.
///
/// Classifies the tool into a risk tier and, for tiers that require approval by
/// default (external sends, financial/credential access, destructive deletes,
/// money movement), runs the gate matrix via `enforce_action_gate` — which
/// records a policy decision and protected audit evidence. A non-allow outcome
/// pauses the tool (`allowed: false`). Lower-risk tiers are not gated here and
/// fall through to the existing capability checks.
pub(crate) async fn evaluate_action_gate_tool_policy(
    state: &AppState,
    ctx: &ToolPolicyContext,
    tool: &str,
) -> Option<ToolPolicyDecision> {
    // Classify with the registered tool's actual security descriptor: an
    // explicitly tagged tier (e.g. FinancialRecordAccess on a financial read
    // whose name trips no heuristic) must gate exactly like a name-derived
    // one. Unregistered names fall back to the name-only heuristics.
    let descriptor = registered_tool_security_descriptor(state, tool)
        .await
        .unwrap_or_default();
    let risk_tier = tool_risk_tier_from_name_and_descriptor(tool, &descriptor);
    if !risk_tier.approval_required_by_default() {
        return None;
    }

    let tenant_context = ctx.tenant_context.clone().unwrap_or_default();
    let actor_id = tenant_context.actor_id.clone();
    let request = GateRequest::new(Some(risk_tier), None);
    let (outcome, policy_decision_id) = state
        .enforce_action_gate(
            &tenant_context,
            &request,
            Some(tool.to_string()),
            actor_id,
            crate::now_ms(),
        )
        .await;

    if outcome.is_allowed() {
        return None;
    }

    let reason = format!(
        "tool `{tool}` paused by runtime approval gate ({}): {}",
        outcome.reviewer_eligibility.as_str(),
        outcome.reason
    );
    if state.is_ready() {
        state.event_bus.publish(EngineEvent::new(
            "approval.gate.tool.gated",
            json!({
                "sessionID": ctx.session_id,
                "messageID": ctx.message_id,
                "tool": tool,
                "riskTier": risk_tier.as_str(),
                "effect": outcome.effect,
                "reviewerEligibility": outcome.reviewer_eligibility.as_str(),
                "reasonCode": outcome.reason_code,
                "timestampMs": crate::now_ms(),
                "tenantContext": tenant_context.clone(),
            }),
        ));
    }

    Some(ToolPolicyDecision {
        allowed: false,
        reason: Some(reason),
        policy_decision_id,
    })
}

fn strict_tool_context_deny_reason(
    tenant_context: Option<&TenantContext>,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    now_ms: u64,
) -> Option<String> {
    let Some(ctx) = tenant_context else {
        return Some(
            "tool denied by runtime policy: strict runtime auth mode requires verified tenant context for protected tools"
                .to_string(),
        );
    };
    if ctx.is_local_implicit() {
        return Some(
            "tool denied by runtime policy: strict runtime auth mode rejects local implicit tenant context for protected tools"
                .to_string(),
        );
    }
    if ctx.actor_id.as_deref().map_or(true, str::is_empty) {
        return Some(
            "tool denied by runtime policy: strict runtime auth mode requires a verified human actor for protected tools"
                .to_string(),
        );
    }
    let Some(verified) = verified_tenant_context else {
        return Some(
            "tool denied by runtime policy: strict runtime auth mode requires signed tenant assertion metadata for protected tools"
                .to_string(),
        );
    };
    if verified.is_expired_at(now_ms) {
        return Some(
            "tool denied by runtime policy: strict runtime auth mode rejects expired tenant context assertions for protected tools"
                .to_string(),
        );
    }
    if !verified.tenant_matches(ctx) {
        return Some(
            "tool denied by runtime policy: signed tenant assertion does not match session tenant context for protected tools"
                .to_string(),
        );
    }
    None
}

fn matching_fintech_protected_action_approval(
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    tool: &str,
    args: &Value,
    category: FintechProtectedActionCategory,
) -> Option<Value> {
    let action_hash = fintech_protected_action_hash(tool, args);
    let now_ms = crate::now_ms();
    run.checkpoint
        .gate_history
        .iter()
        .rev()
        .filter(|record| record.decision.eq_ignore_ascii_case("approve"))
        .find_map(|record| {
            let metadata = record.metadata.as_ref()?;
            let receipt = metadata.get("fintech_protected_action").unwrap_or(metadata);
            let receipt_category = receipt.get("category").and_then(Value::as_str)?;
            if receipt_category != category.as_str() {
                return None;
            }
            let receipt_tool = receipt.get("tool").and_then(Value::as_str)?;
            if !receipt_tool.eq_ignore_ascii_case(tool) {
                return None;
            }
            if receipt.get("action_hash").and_then(Value::as_str) != Some(action_hash.as_str()) {
                return None;
            }
            if !receipt_matches_tenant(receipt, run) {
                return None;
            }
            if receipt
                .get("expires_at_ms")
                .and_then(Value::as_u64)
                .is_some_and(|expires_at_ms| expires_at_ms <= now_ms)
            {
                return None;
            }
            Some(json!({
                "gate_node_id": record.node_id,
                "decided_at_ms": record.decided_at_ms,
                "category": category.as_str(),
                "tool": receipt_tool,
                "action_hash": action_hash,
                "expires_at_ms": receipt.get("expires_at_ms").cloned().unwrap_or(Value::Null),
            }))
        })
}

fn receipt_matches_tenant(
    receipt: &Value,
    run: &crate::automation_v2::types::AutomationV2RunRecord,
) -> bool {
    let Some(tenant) = receipt.get("tenant") else {
        return false;
    };
    tenant.get("org_id").and_then(Value::as_str) == Some(run.tenant_context.org_id.as_str())
        && tenant.get("workspace_id").and_then(Value::as_str)
            == Some(run.tenant_context.workspace_id.as_str())
}

impl ToolPolicyHook for ServerToolPolicyHook {
    fn evaluate_tool(
        &self,
        ctx: ToolPolicyContext,
    ) -> BoxFuture<'static, anyhow::Result<ToolPolicyDecision>> {
        let state = self.state.clone();
        Box::pin(async move {
            let tool = normalize_tool_name(&ctx.tool);
            if session_allowlist_would_deny_non_automation_tool(&state, &ctx, &tool).await {
                // Let the core engine's session allowlist produce its side-effect-free denial
                // for ordinary scoped runs. Automation-v2 sessions stay in this hook so the
                // phase policy can record policy/audit evidence for denied tool calls.
                return Ok(ToolPolicyDecision {
                    allowed: true,
                    reason: None,
                    policy_decision_id: None,
                });
            }
            // EAA-02 (TAN-27): discovery filtering hides unauthorized MCP
            // tools from the offered list, but a hand-crafted or
            // model-generated invocation can still name them. Re-check the
            // exact same authorization at execution time, failing closed
            // only when a strict tenant projection is present so
            // local/unscoped sessions are unchanged.
            if let Some(strict) = ctx
                .verified_tenant_context
                .as_ref()
                .and_then(|verified| verified.strict_projection.as_ref())
            {
                if let Some(security) =
                    crate::http::mcp_inventory::mcp_tool_security_for_invocation(&state, &tool)
                        .await
                {
                    // Authorize the fully namespaced invocation name with the
                    // inner security descriptor — identical inputs to the
                    // discovery filter, so a grant that makes a tool visible
                    // also authorizes its execution.
                    if !crate::http::mcp_inventory::mcp_tool_authorized_for_discovery(
                        Some(strict),
                        &tool,
                        security.as_ref(),
                        crate::now_ms(),
                    ) {
                        let reason = format!(
                            "tool `{tool}` is not authorized for this principal (execution-time authorization)"
                        );
                        state.event_bus.publish(EngineEvent::new(
                            "tool.execution.denied",
                            json!({
                                "sessionID": ctx.session_id,
                                "messageID": ctx.message_id,
                                "tool": tool,
                                "reason": reason,
                                "principal": strict.principal.id,
                                "timestampMs": crate::now_ms(),
                            }),
                        ));
                        crate::audit::append_protected_audit_event_best_effort(
                            &state,
                            "tool.execution.denied",
                            &strict.tenant_context,
                            Some(strict.principal.id.clone()),
                            json!({
                                "sessionID": ctx.session_id,
                                "messageID": ctx.message_id,
                                "tool": tool,
                                "reason": reason,
                            }),
                        )
                        .await;
                        return Ok(ToolPolicyDecision {
                            allowed: false,
                            reason: Some(reason),
                            policy_decision_id: None,
                        });
                    }
                }
            }
            if let Some(decision) = evaluate_automation_phase_tool_policy(&state, &ctx, &tool).await
            {
                return Ok(decision);
            }

            if let Some(policy) = state.routine_session_policy(&ctx.session_id).await {
                let allowed_patterns = policy
                    .allowed_tools
                    .iter()
                    .map(|name| normalize_tool_name(name))
                    .collect::<Vec<_>>();
                if !policy.allowed_tools.is_empty() && !any_policy_matches(&allowed_patterns, &tool)
                {
                    let reason = format!(
                        "tool `{}` is not allowed for routine `{}` (run `{}`)",
                        tool, policy.routine_id, policy.run_id
                    );
                    state
                        .event_bus
                        .publish(crate::routines::types::tenant_scoped_engine_event(
                            "routine.tool.denied",
                            &policy.tenant_context,
                            json!({
                                "sessionID": ctx.session_id,
                                "messageID": ctx.message_id,
                                "runID": policy.run_id,
                                "routineID": policy.routine_id,
                                "tool": tool,
                                "reason": reason,
                                "timestampMs": crate::now_ms(),
                            }),
                        ));
                    return Ok(ToolPolicyDecision {
                        allowed: false,
                        reason: Some(reason),
                        policy_decision_id: None,
                    });
                }
            }

            if let Some(reason) =
                evaluate_automation_read_only_write_deny(&state, &ctx.session_id, &tool, &ctx.args)
                    .await
            {
                state.event_bus.publish(EngineEvent::new(
                    "automation.read_only_write.denied",
                    json!({
                        "sessionID": ctx.session_id,
                        "messageID": ctx.message_id,
                        "tool": tool,
                        "reason": reason,
                        "timestampMs": crate::now_ms(),
                    }),
                ));
                return Ok(ToolPolicyDecision {
                    allowed: false,
                    reason: Some(reason),
                    policy_decision_id: None,
                });
            }

            let runtime_auth_mode = resolve_runtime_auth_mode();
            if runtime_auth_mode_requires_verified_tool_context(runtime_auth_mode) {
                if let Some(decision) =
                    evaluate_egress_preflight_tool_policy(&state, &ctx, &tool).await
                {
                    return Ok(decision);
                }
            }

            if let Some(decision) = evaluate_fintech_strict_tool_policy(
                &state,
                &ctx.session_id,
                &ctx.message_id,
                ctx.tenant_context.as_ref(),
                ctx.verified_tenant_context.as_ref(),
                runtime_auth_mode,
                &tool,
                &ctx.args,
            )
            .await
            {
                return Ok(decision);
            }

            // CT-20: resolve high-risk tools against the declarative approval
            // gate matrix. Only enforces under strict runtime auth modes so
            // local/single-tenant deployments remain a no-op.
            if runtime_auth_mode_requires_verified_tool_context(runtime_auth_mode) {
                if let Some(decision) = evaluate_action_gate_tool_policy(&state, &ctx, &tool).await
                {
                    return Ok(decision);
                }
            }

            let Some(instance) = state
                .agent_teams
                .instance_for_session(&ctx.session_id)
                .await
            else {
                return Ok(ToolPolicyDecision {
                    allowed: true,
                    reason: None,
                    policy_decision_id: None,
                });
            };
            let caps = instance.capabilities.clone();
            let deny = evaluate_capability_deny(
                &state,
                &instance,
                &tool,
                &ctx.args,
                &caps,
                &ctx.session_id,
                &ctx.message_id,
            )
            .await;
            if let Some(reason) = deny {
                state.event_bus.publish(EngineEvent::new(
                    "agent_team.capability.denied",
                    json!({
                        "sessionID": ctx.session_id,
                        "messageID": ctx.message_id,
                        "runID": instance.run_id,
                        "missionID": instance.mission_id,
                        "instanceID": instance.instance_id,
                        "tool": tool,
                        "reason": reason,
                        "timestampMs": crate::now_ms(),
                    }),
                ));
                return Ok(ToolPolicyDecision {
                    allowed: false,
                    reason: Some(reason),
                    policy_decision_id: None,
                });
            }
            Ok(ToolPolicyDecision {
                allowed: true,
                reason: None,
                policy_decision_id: None,
            })
        })
    }
}

impl AgentTeamRuntime {
    pub fn new(audit_path: PathBuf) -> Self {
        Self {
            policy: Arc::new(RwLock::new(None)),
            templates: Arc::new(RwLock::new(HashMap::new())),
            instances: Arc::new(RwLock::new(HashMap::new())),
            budgets: Arc::new(RwLock::new(HashMap::new())),
            mission_budgets: Arc::new(RwLock::new(HashMap::new())),
            spawn_approvals: Arc::new(RwLock::new(HashMap::new())),
            loaded_workspace: Arc::new(RwLock::new(None)),
            audit_path: Arc::new(RwLock::new(audit_path)),
        }
    }

    pub async fn set_audit_path(&self, path: PathBuf) {
        *self.audit_path.write().await = path;
    }

    pub async fn list_templates(&self) -> Vec<AgentTemplate> {
        let mut rows = self
            .templates
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.template_id.cmp(&b.template_id));
        rows
    }

    async fn templates_dir_for_loaded_workspace(&self) -> anyhow::Result<PathBuf> {
        let workspace = self
            .loaded_workspace
            .read()
            .await
            .clone()
            .ok_or_else(|| anyhow::anyhow!("agent team workspace not loaded"))?;
        Ok(PathBuf::from(workspace)
            .join(".tandem")
            .join("agent-team")
            .join("templates"))
    }

    fn template_filename(template_id: &str) -> String {
        let safe = template_id
            .trim()
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                    ch
                } else {
                    '_'
                }
            })
            .collect::<String>();
        let fallback = if safe.is_empty() {
            "template".to_string()
        } else {
            safe
        };
        format!("{fallback}.yaml")
    }

    pub async fn upsert_template(
        &self,
        workspace_root: &str,
        template: AgentTemplate,
    ) -> anyhow::Result<AgentTemplate> {
        self.ensure_loaded_for_workspace(workspace_root).await?;
        let templates_dir = self.templates_dir_for_loaded_workspace().await?;
        fs::create_dir_all(&templates_dir).await?;
        let path = templates_dir.join(Self::template_filename(&template.template_id));
        let payload = serde_yaml::to_string(&template)?;
        fs::write(path, payload).await?;
        self.templates
            .write()
            .await
            .insert(template.template_id.clone(), template.clone());
        Ok(template)
    }

    pub async fn delete_template(
        &self,
        workspace_root: &str,
        template_id: &str,
    ) -> anyhow::Result<bool> {
        self.ensure_loaded_for_workspace(workspace_root).await?;
        let templates_dir = self.templates_dir_for_loaded_workspace().await?;
        let path = templates_dir.join(Self::template_filename(template_id));
        let existed = self.templates.write().await.remove(template_id).is_some();
        if path.exists() {
            let _ = fs::remove_file(path).await;
        }
        Ok(existed)
    }

    pub async fn get_template_for_workspace(
        &self,
        workspace_root: &str,
        template_id: &str,
    ) -> anyhow::Result<Option<AgentTemplate>> {
        self.ensure_loaded_for_workspace(workspace_root).await?;
        Ok(self.templates.read().await.get(template_id).cloned())
    }

    pub async fn list_instances(
        &self,
        mission_id: Option<&str>,
        parent_instance_id: Option<&str>,
        status: Option<AgentInstanceStatus>,
    ) -> Vec<AgentInstance> {
        let mut rows = self
            .instances
            .read()
            .await
            .values()
            .filter(|instance| {
                if let Some(mission_id) = mission_id {
                    if instance.mission_id != mission_id {
                        return false;
                    }
                }
                if let Some(parent_id) = parent_instance_id {
                    if instance.parent_instance_id.as_deref() != Some(parent_id) {
                        return false;
                    }
                }
                if let Some(status) = &status {
                    if &instance.status != status {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.instance_id.cmp(&b.instance_id));
        rows
    }

    pub async fn list_mission_summaries(&self) -> Vec<AgentMissionSummary> {
        let instances = self.instances.read().await;
        let mut by_mission: HashMap<String, AgentMissionSummary> = HashMap::new();
        for instance in instances.values() {
            let row = by_mission
                .entry(instance.mission_id.clone())
                .or_insert_with(|| AgentMissionSummary {
                    mission_id: instance.mission_id.clone(),
                    instance_count: 0,
                    running_count: 0,
                    completed_count: 0,
                    failed_count: 0,
                    cancelled_count: 0,
                    queued_count: 0,
                    token_used_total: 0,
                    tool_calls_used_total: 0,
                    steps_used_total: 0,
                    cost_used_usd_total: 0.0,
                });
            row.instance_count = row.instance_count.saturating_add(1);
            match instance.status {
                AgentInstanceStatus::Queued => {
                    row.queued_count = row.queued_count.saturating_add(1)
                }
                AgentInstanceStatus::Running => {
                    row.running_count = row.running_count.saturating_add(1)
                }
                AgentInstanceStatus::Completed => {
                    row.completed_count = row.completed_count.saturating_add(1)
                }
                AgentInstanceStatus::Failed => {
                    row.failed_count = row.failed_count.saturating_add(1)
                }
                AgentInstanceStatus::Cancelled => {
                    row.cancelled_count = row.cancelled_count.saturating_add(1)
                }
            }
            if let Some(usage) = instance
                .metadata
                .as_ref()
                .and_then(|m| m.get("budgetUsage"))
                .and_then(|u| u.as_object())
            {
                row.token_used_total = row.token_used_total.saturating_add(
                    usage
                        .get("tokensUsed")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                );
                row.tool_calls_used_total = row.tool_calls_used_total.saturating_add(
                    usage
                        .get("toolCallsUsed")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                );
                row.steps_used_total = row
                    .steps_used_total
                    .saturating_add(usage.get("stepsUsed").and_then(|v| v.as_u64()).unwrap_or(0));
                row.cost_used_usd_total += usage
                    .get("costUsedUsd")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
            }
        }
        let mut rows = by_mission.into_values().collect::<Vec<_>>();
        rows.sort_by(|a, b| a.mission_id.cmp(&b.mission_id));
        rows
    }

    pub async fn instance_for_session(&self, session_id: &str) -> Option<AgentInstance> {
        self.instances
            .read()
            .await
            .values()
            .find(|instance| instance.session_id == session_id)
            .cloned()
    }

    pub async fn list_spawn_approvals(&self) -> Vec<PendingSpawnApproval> {
        let mut rows = self
            .spawn_approvals
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.created_at_ms.cmp(&b.created_at_ms));
        rows
    }

    pub async fn ensure_loaded_for_workspace(&self, workspace_root: &str) -> anyhow::Result<()> {
        let normalized = workspace_root.trim().to_string();
        let already_loaded = self
            .loaded_workspace
            .read()
            .await
            .as_ref()
            .map(|s| s == &normalized)
            .unwrap_or(false);
        if already_loaded {
            return Ok(());
        }

        let root = PathBuf::from(&normalized);
        let policy_path = root
            .join(".tandem")
            .join("agent-team")
            .join("spawn-policy.yaml");
        let templates_dir = root.join(".tandem").join("agent-team").join("templates");

        let mut next_policy = None;
        if policy_path.exists() {
            let raw = fs::read_to_string(&policy_path)
                .await
                .with_context(|| format!("failed reading {}", policy_path.display()))?;
            let parsed = serde_yaml::from_str::<SpawnPolicy>(&raw)
                .with_context(|| format!("failed parsing {}", policy_path.display()))?;
            next_policy = Some(parsed);
        }

        let mut next_templates = HashMap::new();
        if templates_dir.exists() {
            let mut entries = fs::read_dir(&templates_dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let ext = path
                    .extension()
                    .and_then(|v| v.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if ext != "yaml" && ext != "yml" && ext != "json" {
                    continue;
                }
                let raw = fs::read_to_string(&path).await?;
                let template = serde_yaml::from_str::<AgentTemplate>(&raw)
                    .with_context(|| format!("failed parsing {}", path.display()))?;
                next_templates.insert(template.template_id.clone(), template);
            }
        }

        *self.policy.write().await = next_policy;
        *self.templates.write().await = next_templates;
        *self.loaded_workspace.write().await = Some(normalized);
        Ok(())
    }

    pub async fn spawn(&self, state: &AppState, req: SpawnRequest) -> SpawnResult {
        self.spawn_with_approval_override(state, req, false).await
    }

    async fn spawn_with_approval_override(
        &self,
        state: &AppState,
        mut req: SpawnRequest,
        approval_override: bool,
    ) -> SpawnResult {
        let workspace_root = state.workspace_index.snapshot().await.root;
        if let Err(err) = self.ensure_loaded_for_workspace(&workspace_root).await {
            return SpawnResult {
                decision: SpawnDecision {
                    allowed: false,
                    code: Some(SpawnDenyCode::SpawnPolicyMissing),
                    reason: Some(format!("spawn policy load failed: {}", err)),
                    requires_user_approval: false,
                },
                instance: None,
            };
        }

        let Some(policy) = self.policy.read().await.clone() else {
            return SpawnResult {
                decision: SpawnDecision {
                    allowed: false,
                    code: Some(SpawnDenyCode::SpawnPolicyMissing),
                    reason: Some("spawn policy file missing".to_string()),
                    requires_user_approval: false,
                },
                instance: None,
            };
        };

        let template = {
            let templates = self.templates.read().await;
            req.template_id
                .as_deref()
                .and_then(|template_id| templates.get(template_id).cloned())
        };
        if req.template_id.is_none() {
            if let Some(found) = self
                .templates
                .read()
                .await
                .values()
                .find(|t| t.role == req.role)
                .cloned()
            {
                req.template_id = Some(found.template_id.clone());
            }
        }
        let template = if template.is_some() {
            template
        } else {
            let templates = self.templates.read().await;
            req.template_id
                .as_deref()
                .and_then(|id| templates.get(id).cloned())
        };

        if req.parent_role.is_none() {
            if let Some(parent_id) = req.parent_instance_id.as_deref() {
                let instances = self.instances.read().await;
                req.parent_role = instances
                    .get(parent_id)
                    .map(|instance| instance.role.clone());
            }
        }

        let instances = self.instances.read().await;
        let total_agents = instances.len();
        let running_agents = instances
            .values()
            .filter(|instance| instance.status == AgentInstanceStatus::Running)
            .count();
        drop(instances);

        let mut decision = policy.evaluate(&req, total_agents, running_agents, template.as_ref());
        if approval_override
            && !decision.allowed
            && decision.requires_user_approval
            && matches!(decision.code, Some(SpawnDenyCode::SpawnRequiresApproval))
        {
            decision = SpawnDecision {
                allowed: true,
                code: None,
                reason: None,
                requires_user_approval: false,
            };
        }
        if !decision.allowed {
            if decision.requires_user_approval && !approval_override {
                self.queue_spawn_approval(&req, &decision).await;
            }
            return SpawnResult {
                decision,
                instance: None,
            };
        }

        let mission_id = req
            .mission_id
            .clone()
            .unwrap_or_else(|| "mission-default".to_string());

        if let Some(reason) = self
            .mission_budget_exceeded_reason(&policy, &mission_id)
            .await
        {
            return SpawnResult {
                decision: SpawnDecision {
                    allowed: false,
                    code: Some(SpawnDenyCode::SpawnMissionBudgetExceeded),
                    reason: Some(reason),
                    requires_user_approval: false,
                },
                instance: None,
            };
        }

        let template = template.unwrap_or_else(|| AgentTemplate {
            template_id: "default-template".to_string(),
            display_name: None,
            avatar_url: None,
            role: req.role.clone(),
            system_prompt: None,
            default_model: None,
            skills: Vec::new(),
            default_budget: BudgetLimit::default(),
            capabilities: Default::default(),
        });

        let skill_hash = match compute_skill_hash(&workspace_root, &template, &policy).await {
            Ok(hash) => hash,
            Err(err) => {
                let lowered = err.to_ascii_lowercase();
                let code = if lowered.contains("pinned hash mismatch") {
                    SpawnDenyCode::SpawnSkillHashMismatch
                } else if lowered.contains("skill source denied") {
                    SpawnDenyCode::SpawnSkillSourceDenied
                } else {
                    SpawnDenyCode::SpawnRequiredSkillMissing
                };
                return SpawnResult {
                    decision: SpawnDecision {
                        allowed: false,
                        code: Some(code),
                        reason: Some(err),
                        requires_user_approval: false,
                    },
                    instance: None,
                };
            }
        };

        let parent_snapshot = {
            let instances = self.instances.read().await;
            req.parent_instance_id
                .as_deref()
                .and_then(|id| instances.get(id).cloned())
        };
        let parent_usage = if let Some(parent_id) = req.parent_instance_id.as_deref() {
            self.budgets.read().await.get(parent_id).cloned()
        } else {
            None
        };

        let budget = resolve_budget(
            &policy,
            parent_snapshot,
            parent_usage,
            &template,
            req.budget_override.clone(),
            &req.role,
        );

        let instance_id = format!("ins_{}", Uuid::new_v4().simple());
        let managed_worktree = prepare_agent_instance_workspace(
            state,
            &workspace_root,
            req.mission_id.as_deref(),
            &instance_id,
            &template.template_id,
        )
        .await;
        let workspace_repo_root = managed_worktree
            .as_ref()
            .map(|row| row.record.repo_root.clone())
            .or_else(|| crate::runtime::worktrees::resolve_git_repo_root(&workspace_root))
            .unwrap_or_else(|| workspace_root.clone());
        let worker_workspace_root = managed_worktree
            .as_ref()
            .map(|row| row.record.path.clone())
            .unwrap_or_else(|| workspace_root.clone());
        let mut session = Session::new(
            Some(format!("Agent Team {}", template.template_id)),
            Some(worker_workspace_root.clone()),
        );
        session.workspace_root = Some(worker_workspace_root.clone());
        let session_id = session.id.clone();
        if let Err(err) = state.storage.save_session(session).await {
            if let Some(worktree) = managed_worktree.as_ref() {
                let _ = crate::runtime::worktrees::delete_managed_worktree(state, &worktree.record)
                    .await;
            }
            return SpawnResult {
                decision: SpawnDecision {
                    allowed: false,
                    code: Some(SpawnDenyCode::SpawnPolicyMissing),
                    reason: Some(format!("failed creating child session: {}", err)),
                    requires_user_approval: false,
                },
                instance: None,
            };
        }

        let instance = AgentInstance {
            instance_id: instance_id.clone(),
            mission_id: mission_id.clone(),
            parent_instance_id: req.parent_instance_id.clone(),
            role: template.role.clone(),
            template_id: template.template_id.clone(),
            session_id: session_id.clone(),
            run_id: None,
            status: AgentInstanceStatus::Running,
            budget,
            skill_hash: skill_hash.clone(),
            capabilities: template.capabilities.clone(),
            metadata: Some(json!({
                "source": req.source,
                "justification": req.justification,
                "workspaceRoot": worker_workspace_root,
                "workspaceRepoRoot": workspace_repo_root,
                "managedWorktree": managed_worktree.as_ref().map(|row| json!({
                    "path": row.record.path,
                    "branch": row.record.branch,
                    "repoRoot": row.record.repo_root,
                    "cleanupBranch": row.record.cleanup_branch,
                    "reused": row.reused,
                })).unwrap_or(Value::Null),
            })),
        };

        self.instances
            .write()
            .await
            .insert(instance.instance_id.clone(), instance.clone());
        self.budgets.write().await.insert(
            instance.instance_id.clone(),
            InstanceBudgetState {
                started_at: Some(Instant::now()),
                ..InstanceBudgetState::default()
            },
        );
        let _ = self.append_audit("spawn.approved", &instance).await;

        SpawnResult {
            decision: SpawnDecision {
                allowed: true,
                code: None,
                reason: None,
                requires_user_approval: false,
            },
            instance: Some(instance),
        }
    }

    pub async fn approve_spawn_approval(
        &self,
        state: &AppState,
        approval_id: &str,
        reason: Option<&str>,
    ) -> Option<SpawnResult> {
        let approval = self.spawn_approvals.write().await.remove(approval_id)?;
        let result = self
            .spawn_with_approval_override(state, approval.request.clone(), true)
            .await;
        if let Some(instance) = result.instance.as_ref() {
            let note = reason.unwrap_or("approved by operator");
            let _ = self
                .append_approval_audit("spawn.approval.approved", approval_id, Some(instance), note)
                .await;
        } else {
            let note = reason.unwrap_or("approval replay failed policy checks");
            let _ = self
                .append_approval_audit("spawn.approval.rejected_on_replay", approval_id, None, note)
                .await;
        }
        Some(result)
    }

    pub async fn deny_spawn_approval(
        &self,
        approval_id: &str,
        reason: Option<&str>,
    ) -> Option<PendingSpawnApproval> {
        let approval = self.spawn_approvals.write().await.remove(approval_id)?;
        let note = reason.unwrap_or("denied by operator");
        let _ = self
            .append_approval_audit("spawn.approval.denied", approval_id, None, note)
            .await;
        Some(approval)
    }

    pub async fn cancel_instance(
        &self,
        state: &AppState,
        instance_id: &str,
        reason: &str,
    ) -> Option<AgentInstance> {
        let mut instances = self.instances.write().await;
        let instance = instances.get_mut(instance_id)?;
        if matches!(
            instance.status,
            AgentInstanceStatus::Completed
                | AgentInstanceStatus::Failed
                | AgentInstanceStatus::Cancelled
        ) {
            return Some(instance.clone());
        }
        instance.status = AgentInstanceStatus::Cancelled;
        let snapshot = instance.clone();
        drop(instances);
        let _ = state.cancellations.cancel(&snapshot.session_id).await;
        cleanup_instance_managed_worktree(state, &snapshot).await;
        let _ = self.append_audit("instance.cancelled", &snapshot).await;
        emit_instance_cancelled(state, &snapshot, reason);
        Some(snapshot)
    }

    async fn queue_spawn_approval(&self, req: &SpawnRequest, decision: &SpawnDecision) {
        let approval = PendingSpawnApproval {
            approval_id: format!("spawn_{}", Uuid::new_v4().simple()),
            created_at_ms: crate::now_ms(),
            request: req.clone(),
            decision_code: decision.code.clone(),
            reason: decision.reason.clone(),
        };
        self.spawn_approvals
            .write()
            .await
            .insert(approval.approval_id.clone(), approval);
    }

    async fn mission_budget_exceeded_reason(
        &self,
        policy: &SpawnPolicy,
        mission_id: &str,
    ) -> Option<String> {
        let Some(limit) = policy.mission_total_budget.as_ref() else {
            return None;
        };
        let usage = self
            .mission_budgets
            .read()
            .await
            .get(mission_id)
            .cloned()
            .unwrap_or_default();
        if let Some(max) = limit.max_tokens {
            if usage.tokens_used >= max {
                return Some(format!(
                    "mission max_tokens exhausted ({}/{})",
                    usage.tokens_used, max
                ));
            }
        }
        if let Some(max) = limit.max_steps {
            if usage.steps_used >= u64::from(max) {
                return Some(format!(
                    "mission max_steps exhausted ({}/{})",
                    usage.steps_used, max
                ));
            }
        }
        if let Some(max) = limit.max_tool_calls {
            if usage.tool_calls_used >= u64::from(max) {
                return Some(format!(
                    "mission max_tool_calls exhausted ({}/{})",
                    usage.tool_calls_used, max
                ));
            }
        }
        if let Some(max) = limit.max_cost_usd {
            if usage.cost_used_usd >= max {
                return Some(format!(
                    "mission max_cost_usd exhausted ({:.6}/{:.6})",
                    usage.cost_used_usd, max
                ));
            }
        }
        None
    }

    pub async fn cancel_mission(&self, state: &AppState, mission_id: &str, reason: &str) -> usize {
        let instance_ids = self
            .instances
            .read()
            .await
            .values()
            .filter(|instance| instance.mission_id == mission_id)
            .map(|instance| instance.instance_id.clone())
            .collect::<Vec<_>>();
        let mut count = 0usize;
        for instance_id in instance_ids {
            if self
                .cancel_instance(state, &instance_id, reason)
                .await
                .is_some()
            {
                count = count.saturating_add(1);
            }
        }
        count
    }

    async fn mark_instance_terminal(
        &self,
        state: &AppState,
        instance_id: &str,
        status: AgentInstanceStatus,
    ) -> Option<AgentInstance> {
        let mut instances = self.instances.write().await;
        let instance = instances.get_mut(instance_id)?;
        if matches!(
            instance.status,
            AgentInstanceStatus::Completed
                | AgentInstanceStatus::Failed
                | AgentInstanceStatus::Cancelled
        ) {
            return Some(instance.clone());
        }
        instance.status = status.clone();
        let snapshot = instance.clone();
        drop(instances);
        cleanup_instance_managed_worktree(state, &snapshot).await;
        match status {
            AgentInstanceStatus::Completed => emit_instance_completed(state, &snapshot),
            AgentInstanceStatus::Failed => emit_instance_failed(state, &snapshot),
            _ => {}
        }
        Some(snapshot)
    }

    pub async fn handle_engine_event(&self, state: &AppState, event: &EngineEvent) {
        let Some(session_id) = extract_session_id(event) else {
            return;
        };
        let Some(instance_id) = self.instance_id_for_session(&session_id).await else {
            return;
        };
        if event.event_type == "provider.usage" {
            let total_tokens = event
                .properties
                .get("totalTokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let cost_used_usd = event
                .properties
                .get("costUsd")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            if total_tokens > 0 {
                let exhausted = self
                    .apply_exact_token_usage(state, &instance_id, total_tokens, cost_used_usd)
                    .await;
                if exhausted {
                    let _ = self
                        .cancel_instance(state, &instance_id, "budget exhausted")
                        .await;
                }
            }
            return;
        }
        let mut delta_tokens = 0u64;
        let mut delta_steps = 0u32;
        let mut delta_tool_calls = 0u32;
        if event.event_type == "message.part.updated" {
            if let Some(part) = event.properties.get("part") {
                let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if part_type == "tool-invocation" {
                    delta_tool_calls = 1;
                } else if part_type == "text" {
                    let delta = event
                        .properties
                        .get("delta")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !delta.is_empty() {
                        delta_tokens = estimate_tokens(delta);
                    }
                }
            }
        } else if event.event_type == "session.run.finished" {
            delta_steps = 1;
            let run_status = event
                .properties
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if run_status == "completed" {
                let _ = self
                    .mark_instance_terminal(state, &instance_id, AgentInstanceStatus::Completed)
                    .await;
            } else if run_status == "failed" || run_status == "error" {
                let _ = self
                    .mark_instance_terminal(state, &instance_id, AgentInstanceStatus::Failed)
                    .await;
            }
        }
        if delta_tokens == 0 && delta_steps == 0 && delta_tool_calls == 0 {
            return;
        }
        let exhausted = self
            .apply_budget_delta(
                state,
                &instance_id,
                delta_tokens,
                delta_steps,
                delta_tool_calls,
            )
            .await;
        if exhausted {
            let _ = self
                .cancel_instance(state, &instance_id, "budget exhausted")
                .await;
        }
    }
}
