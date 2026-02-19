use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::Context;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde::Serialize;
use serde_json::{json, Value};
use tandem_core::{
    SpawnAgentHook, SpawnAgentToolContext, SpawnAgentToolResult, ToolPolicyContext,
    ToolPolicyDecision, ToolPolicyHook,
};
use tandem_orchestrator::{
    AgentInstance, AgentInstanceStatus, AgentRole, AgentTemplate, BudgetLimit, SpawnDecision,
    SpawnDenyCode, SpawnPolicy, SpawnRequest, SpawnSource,
};
use tandem_skills::SkillService;
use tandem_types::{EngineEvent, Session};
use tokio::fs;
use tokio::sync::RwLock;
use uuid::Uuid;

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

impl ToolPolicyHook for ServerToolPolicyHook {
    fn evaluate_tool(
        &self,
        ctx: ToolPolicyContext,
    ) -> BoxFuture<'static, anyhow::Result<ToolPolicyDecision>> {
        let state = self.state.clone();
        Box::pin(async move {
            let Some(instance) = state
                .agent_teams
                .instance_for_session(&ctx.session_id)
                .await
            else {
                return Ok(ToolPolicyDecision {
                    allowed: true,
                    reason: None,
                });
            };
            let tool = normalize_tool_name(&ctx.tool);
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
                });
            }
            Ok(ToolPolicyDecision {
                allowed: true,
                reason: None,
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
                AgentInstanceStatus::Queued => row.queued_count = row.queued_count.saturating_add(1),
                AgentInstanceStatus::Running => {
                    row.running_count = row.running_count.saturating_add(1)
                }
                AgentInstanceStatus::Completed => {
                    row.completed_count = row.completed_count.saturating_add(1)
                }
                AgentInstanceStatus::Failed => row.failed_count = row.failed_count.saturating_add(1),
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
                row.steps_used_total = row.steps_used_total.saturating_add(
                    usage
                        .get("stepsUsed")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0),
                );
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
            role: req.role.clone(),
            system_prompt: None,
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

        let mut session = Session::new(
            Some(format!("Agent Team {}", template.template_id)),
            Some(workspace_root.clone()),
        );
        session.workspace_root = Some(workspace_root.clone());
        let session_id = session.id.clone();
        if let Err(err) = state.storage.save_session(session).await {
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
            instance_id: format!("ins_{}", Uuid::new_v4().simple()),
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
                return Some(format!("mission max_tokens exhausted ({}/{})", usage.tokens_used, max));
            }
        }
        if let Some(max) = limit.max_steps {
            if usage.steps_used >= u64::from(max) {
                return Some(format!("mission max_steps exhausted ({}/{})", usage.steps_used, max));
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

    async fn instance_id_for_session(&self, session_id: &str) -> Option<String> {
        self.instances
            .read()
            .await
            .values()
            .find(|instance| instance.session_id == session_id)
            .map(|instance| instance.instance_id.clone())
    }

    async fn apply_budget_delta(
        &self,
        state: &AppState,
        instance_id: &str,
        delta_tokens: u64,
        delta_steps: u32,
        delta_tool_calls: u32,
    ) -> bool {
        let policy = self.policy.read().await.clone().unwrap_or(SpawnPolicy {
            enabled: false,
            require_justification: false,
            max_agents: None,
            max_concurrent: None,
            child_budget_percent_of_parent_remaining: None,
            mission_total_budget: None,
            cost_per_1k_tokens_usd: None,
            spawn_edges: HashMap::new(),
            required_skills: HashMap::new(),
            role_defaults: HashMap::new(),
            skill_sources: Default::default(),
        });
        let mut budgets = self.budgets.write().await;
        let Some(usage) = budgets.get_mut(instance_id) else {
            return false;
        };
        if usage.exhausted {
            return true;
        }
        let prev_cost_used_usd = usage.cost_used_usd;
        usage.tokens_used = usage.tokens_used.saturating_add(delta_tokens);
        usage.steps_used = usage.steps_used.saturating_add(delta_steps);
        usage.tool_calls_used = usage.tool_calls_used.saturating_add(delta_tool_calls);
        if let Some(rate) = policy.cost_per_1k_tokens_usd {
            usage.cost_used_usd += (delta_tokens as f64 / 1000.0) * rate;
        }
        let elapsed_ms = usage
            .started_at
            .map(|started| started.elapsed().as_millis() as u64)
            .unwrap_or(0);

        let mut exhausted_reason: Option<&'static str> = None;
        let mut snapshot: Option<AgentInstance> = None;
        {
            let mut instances = self.instances.write().await;
            if let Some(instance) = instances.get_mut(instance_id) {
                instance.metadata = Some(merge_metadata_usage(
                    instance.metadata.take(),
                    usage.tokens_used,
                    usage.steps_used,
                    usage.tool_calls_used,
                    usage.cost_used_usd,
                    elapsed_ms,
                ));
                if let Some(limit) = instance.budget.max_tokens {
                    if usage.tokens_used >= limit {
                        exhausted_reason = Some("max_tokens");
                    }
                }
                if exhausted_reason.is_none() {
                    if let Some(limit) = instance.budget.max_steps {
                        if usage.steps_used >= limit {
                            exhausted_reason = Some("max_steps");
                        }
                    }
                }
                if exhausted_reason.is_none() {
                    if let Some(limit) = instance.budget.max_tool_calls {
                        if usage.tool_calls_used >= limit {
                            exhausted_reason = Some("max_tool_calls");
                        }
                    }
                }
                if exhausted_reason.is_none() {
                    if let Some(limit) = instance.budget.max_duration_ms {
                        if elapsed_ms >= limit {
                            exhausted_reason = Some("max_duration_ms");
                        }
                    }
                }
                if exhausted_reason.is_none() {
                    if let Some(limit) = instance.budget.max_cost_usd {
                        if usage.cost_used_usd >= limit {
                            exhausted_reason = Some("max_cost_usd");
                        }
                    }
                }
                snapshot = Some(instance.clone());
            }
        }
        let Some(instance) = snapshot else {
            return false;
        };
        emit_budget_usage(
            state,
            &instance,
            usage.tokens_used,
            usage.steps_used,
            usage.tool_calls_used,
            usage.cost_used_usd,
            elapsed_ms,
        );
        let mission_exhausted = self
            .apply_mission_budget_delta(
                state,
                &instance.mission_id,
                delta_tokens,
                u64::from(delta_steps),
                u64::from(delta_tool_calls),
                usage.cost_used_usd - prev_cost_used_usd,
                &policy,
            )
            .await;
        if mission_exhausted {
            usage.exhausted = true;
            let _ = self
                .cancel_mission(state, &instance.mission_id, "mission budget exhausted")
                .await;
            return true;
        }
        if let Some(reason) = exhausted_reason {
            usage.exhausted = true;
            emit_budget_exhausted(
                state,
                &instance,
                reason,
                usage.tokens_used,
                usage.steps_used,
                usage.tool_calls_used,
                usage.cost_used_usd,
                elapsed_ms,
            );
            return true;
        }
        false
    }

    async fn apply_exact_token_usage(
        &self,
        state: &AppState,
        instance_id: &str,
        total_tokens: u64,
        cost_used_usd: f64,
    ) -> bool {
        let policy = self.policy.read().await.clone().unwrap_or(SpawnPolicy {
            enabled: false,
            require_justification: false,
            max_agents: None,
            max_concurrent: None,
            child_budget_percent_of_parent_remaining: None,
            mission_total_budget: None,
            cost_per_1k_tokens_usd: None,
            spawn_edges: HashMap::new(),
            required_skills: HashMap::new(),
            role_defaults: HashMap::new(),
            skill_sources: Default::default(),
        });
        let mut budgets = self.budgets.write().await;
        let Some(usage) = budgets.get_mut(instance_id) else {
            return false;
        };
        if usage.exhausted {
            return true;
        }
        let prev_tokens = usage.tokens_used;
        let prev_cost_used_usd = usage.cost_used_usd;
        usage.tokens_used = usage.tokens_used.max(total_tokens);
        if cost_used_usd > 0.0 {
            usage.cost_used_usd = usage.cost_used_usd.max(cost_used_usd);
        } else if let Some(rate) = policy.cost_per_1k_tokens_usd {
            let delta = usage.tokens_used.saturating_sub(prev_tokens);
            usage.cost_used_usd += (delta as f64 / 1000.0) * rate;
        }
        let elapsed_ms = usage
            .started_at
            .map(|started| started.elapsed().as_millis() as u64)
            .unwrap_or(0);
        let mut exhausted_reason: Option<&'static str> = None;
        let mut snapshot: Option<AgentInstance> = None;
        {
            let mut instances = self.instances.write().await;
            if let Some(instance) = instances.get_mut(instance_id) {
                instance.metadata = Some(merge_metadata_usage(
                    instance.metadata.take(),
                    usage.tokens_used,
                    usage.steps_used,
                    usage.tool_calls_used,
                    usage.cost_used_usd,
                    elapsed_ms,
                ));
                if let Some(limit) = instance.budget.max_tokens {
                    if usage.tokens_used >= limit {
                        exhausted_reason = Some("max_tokens");
                    }
                }
                if exhausted_reason.is_none() {
                    if let Some(limit) = instance.budget.max_cost_usd {
                        if usage.cost_used_usd >= limit {
                            exhausted_reason = Some("max_cost_usd");
                        }
                    }
                }
                snapshot = Some(instance.clone());
            }
        }
        let Some(instance) = snapshot else {
            return false;
        };
        emit_budget_usage(
            state,
            &instance,
            usage.tokens_used,
            usage.steps_used,
            usage.tool_calls_used,
            usage.cost_used_usd,
            elapsed_ms,
        );
        let mission_exhausted = self
            .apply_mission_budget_delta(
                state,
                &instance.mission_id,
                usage.tokens_used.saturating_sub(prev_tokens),
                0,
                0,
                usage.cost_used_usd - prev_cost_used_usd,
                &policy,
            )
            .await;
        if mission_exhausted {
            usage.exhausted = true;
            let _ = self
                .cancel_mission(state, &instance.mission_id, "mission budget exhausted")
                .await;
            return true;
        }
        if let Some(reason) = exhausted_reason {
            usage.exhausted = true;
            emit_budget_exhausted(
                state,
                &instance,
                reason,
                usage.tokens_used,
                usage.steps_used,
                usage.tool_calls_used,
                usage.cost_used_usd,
                elapsed_ms,
            );
            return true;
        }
        false
    }

    async fn append_audit(&self, action: &str, instance: &AgentInstance) -> anyhow::Result<()> {
        let path = self.audit_path.read().await.clone();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let row = json!({
            "action": action,
            "missionID": instance.mission_id,
            "instanceID": instance.instance_id,
            "parentInstanceID": instance.parent_instance_id,
            "role": instance.role,
            "templateID": instance.template_id,
            "sessionID": instance.session_id,
            "skillHash": instance.skill_hash,
            "timestampMs": crate::now_ms(),
        });
        let mut existing = if path.exists() {
            fs::read_to_string(&path).await.unwrap_or_default()
        } else {
            String::new()
        };
        existing.push_str(&serde_json::to_string(&row)?);
        existing.push('\n');
        fs::write(path, existing).await?;
        Ok(())
    }

    async fn append_approval_audit(
        &self,
        action: &str,
        approval_id: &str,
        instance: Option<&AgentInstance>,
        reason: &str,
    ) -> anyhow::Result<()> {
        let path = self.audit_path.read().await.clone();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let row = json!({
            "action": action,
            "approvalID": approval_id,
            "reason": reason,
            "missionID": instance.map(|v| v.mission_id.clone()),
            "instanceID": instance.map(|v| v.instance_id.clone()),
            "parentInstanceID": instance.and_then(|v| v.parent_instance_id.clone()),
            "role": instance.map(|v| v.role.clone()),
            "templateID": instance.map(|v| v.template_id.clone()),
            "sessionID": instance.map(|v| v.session_id.clone()),
            "skillHash": instance.map(|v| v.skill_hash.clone()),
            "timestampMs": crate::now_ms(),
        });
        let mut existing = if path.exists() {
            fs::read_to_string(&path).await.unwrap_or_default()
        } else {
            String::new()
        };
        existing.push_str(&serde_json::to_string(&row)?);
        existing.push('\n');
        fs::write(path, existing).await?;
        Ok(())
    }

    async fn apply_mission_budget_delta(
        &self,
        state: &AppState,
        mission_id: &str,
        delta_tokens: u64,
        delta_steps: u64,
        delta_tool_calls: u64,
        delta_cost_used_usd: f64,
        policy: &SpawnPolicy,
    ) -> bool {
        let mut budgets = self.mission_budgets.write().await;
        let row = budgets.entry(mission_id.to_string()).or_default();
        row.tokens_used = row.tokens_used.saturating_add(delta_tokens);
        row.steps_used = row.steps_used.saturating_add(delta_steps);
        row.tool_calls_used = row.tool_calls_used.saturating_add(delta_tool_calls);
        row.cost_used_usd += delta_cost_used_usd.max(0.0);
        if row.exhausted {
            return true;
        }
        let Some(limit) = policy.mission_total_budget.as_ref() else {
            return false;
        };
        let mut exhausted_by: Option<&'static str> = None;
        if let Some(max) = limit.max_tokens {
            if row.tokens_used >= max {
                exhausted_by = Some("mission_max_tokens");
            }
        }
        if exhausted_by.is_none() {
            if let Some(max) = limit.max_steps {
                if row.steps_used >= u64::from(max) {
                    exhausted_by = Some("mission_max_steps");
                }
            }
        }
        if exhausted_by.is_none() {
            if let Some(max) = limit.max_tool_calls {
                if row.tool_calls_used >= u64::from(max) {
                    exhausted_by = Some("mission_max_tool_calls");
                }
            }
        }
        if exhausted_by.is_none() {
            if let Some(max) = limit.max_cost_usd {
                if row.cost_used_usd >= max {
                    exhausted_by = Some("mission_max_cost_usd");
                }
            }
        }
        if let Some(exhausted_by) = exhausted_by {
            row.exhausted = true;
            emit_mission_budget_exhausted(
                state,
                mission_id,
                exhausted_by,
                row.tokens_used,
                row.steps_used,
                row.tool_calls_used,
                row.cost_used_usd,
            );
            return true;
        }
        false
    }

    pub async fn set_for_test(
        &self,
        workspace_root: Option<String>,
        policy: Option<SpawnPolicy>,
        templates: Vec<AgentTemplate>,
    ) {
        *self.policy.write().await = policy;
        let mut by_id = HashMap::new();
        for template in templates {
            by_id.insert(template.template_id.clone(), template);
        }
        *self.templates.write().await = by_id;
        self.instances.write().await.clear();
        self.budgets.write().await.clear();
        self.mission_budgets.write().await.clear();
        self.spawn_approvals.write().await.clear();
        *self.loaded_workspace.write().await = workspace_root;
    }
}

fn resolve_budget(
    policy: &SpawnPolicy,
    parent_instance: Option<AgentInstance>,
    parent_usage: Option<InstanceBudgetState>,
    template: &AgentTemplate,
    override_budget: Option<BudgetLimit>,
    role: &AgentRole,
) -> BudgetLimit {
    let role_default = policy.role_defaults.get(role).cloned().unwrap_or_default();
    let mut chosen = merge_budget(
        merge_budget(role_default, template.default_budget.clone()),
        override_budget.unwrap_or_default(),
    );

    if let Some(parent) = parent_instance {
        let usage = parent_usage.unwrap_or_default();
        if let Some(pct) = policy.child_budget_percent_of_parent_remaining {
            if pct > 0 {
                chosen.max_tokens = cap_budget_remaining_u64(
                    chosen.max_tokens,
                    parent.budget.max_tokens,
                    usage.tokens_used,
                    pct,
                );
                chosen.max_steps = cap_budget_remaining_u32(
                    chosen.max_steps,
                    parent.budget.max_steps,
                    usage.steps_used,
                    pct,
                );
                chosen.max_tool_calls = cap_budget_remaining_u32(
                    chosen.max_tool_calls,
                    parent.budget.max_tool_calls,
                    usage.tool_calls_used,
                    pct,
                );
                chosen.max_duration_ms = cap_budget_remaining_u64(
                    chosen.max_duration_ms,
                    parent.budget.max_duration_ms,
                    usage
                        .started_at
                        .map(|started| started.elapsed().as_millis() as u64)
                        .unwrap_or(0),
                    pct,
                );
                chosen.max_cost_usd = cap_budget_remaining_f64(
                    chosen.max_cost_usd,
                    parent.budget.max_cost_usd,
                    usage.cost_used_usd,
                    pct,
                );
            }
        }
    }
    chosen
}

fn merge_budget(base: BudgetLimit, overlay: BudgetLimit) -> BudgetLimit {
    BudgetLimit {
        max_tokens: overlay.max_tokens.or(base.max_tokens),
        max_steps: overlay.max_steps.or(base.max_steps),
        max_tool_calls: overlay.max_tool_calls.or(base.max_tool_calls),
        max_duration_ms: overlay.max_duration_ms.or(base.max_duration_ms),
        max_cost_usd: overlay.max_cost_usd.or(base.max_cost_usd),
    }
}

fn cap_budget_remaining_u64(
    child: Option<u64>,
    parent_limit: Option<u64>,
    parent_used: u64,
    pct: u8,
) -> Option<u64> {
    match (child, parent_limit) {
        (Some(child), Some(parent_limit)) => {
            let remaining = parent_limit.saturating_sub(parent_used);
            Some(child.min(remaining.saturating_mul(pct as u64) / 100))
        }
        (None, Some(parent_limit)) => {
            let remaining = parent_limit.saturating_sub(parent_used);
            Some(remaining.saturating_mul(pct as u64) / 100)
        }
        (Some(child), None) => Some(child),
        (None, None) => None,
    }
}

fn cap_budget_remaining_u32(
    child: Option<u32>,
    parent_limit: Option<u32>,
    parent_used: u32,
    pct: u8,
) -> Option<u32> {
    match (child, parent_limit) {
        (Some(child), Some(parent_limit)) => {
            let remaining = parent_limit.saturating_sub(parent_used);
            Some(child.min(remaining.saturating_mul(pct as u32) / 100))
        }
        (None, Some(parent_limit)) => {
            let remaining = parent_limit.saturating_sub(parent_used);
            Some(remaining.saturating_mul(pct as u32) / 100)
        }
        (Some(child), None) => Some(child),
        (None, None) => None,
    }
}

fn cap_budget_remaining_f64(
    child: Option<f64>,
    parent_limit: Option<f64>,
    parent_used: f64,
    pct: u8,
) -> Option<f64> {
    match (child, parent_limit) {
        (Some(child), Some(parent_limit)) => {
            let remaining = (parent_limit - parent_used).max(0.0);
            Some(child.min(remaining * f64::from(pct) / 100.0))
        }
        (None, Some(parent_limit)) => {
            let remaining = (parent_limit - parent_used).max(0.0);
            Some(remaining * f64::from(pct) / 100.0)
        }
        (Some(child), None) => Some(child),
        (None, None) => None,
    }
}

async fn compute_skill_hash(
    workspace_root: &str,
    template: &AgentTemplate,
    policy: &SpawnPolicy,
) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut rows = Vec::new();
    let skill_service = SkillService::for_workspace(Some(PathBuf::from(workspace_root)));
    for skill in &template.skills {
        if let Some(path) = skill.path.as_deref() {
            validate_skill_source(skill.id.as_deref(), Some(path), policy)?;
            let skill_path = Path::new(workspace_root).join(path);
            let raw = fs::read_to_string(&skill_path)
                .await
                .map_err(|_| format!("missing required skill path `{}`", skill_path.display()))?;
            let digest = hash_hex(raw.as_bytes());
            validate_pinned_hash(skill.id.as_deref(), Some(path), &digest, policy)?;
            rows.push(format!("path:{}:{}", path, digest));
        } else if let Some(id) = skill.id.as_deref() {
            validate_skill_source(Some(id), None, policy)?;
            let loaded = skill_service
                .load_skill(id)
                .map_err(|err| format!("failed loading skill `{id}`: {err}"))?;
            let Some(loaded) = loaded else {
                return Err(format!("missing required skill id `{id}`"));
            };
            let digest = hash_hex(loaded.content.as_bytes());
            validate_pinned_hash(Some(id), None, &digest, policy)?;
            rows.push(format!("id:{}:{}", id, digest));
        }
    }
    rows.sort();
    let mut hasher = Sha256::new();
    for row in rows {
        hasher.update(row.as_bytes());
        hasher.update(b"\n");
    }
    let digest = hasher.finalize();
    Ok(format!("sha256:{}", hash_hex(digest.as_slice())))
}

fn validate_skill_source(
    id: Option<&str>,
    path: Option<&str>,
    policy: &SpawnPolicy,
) -> Result<(), String> {
    use tandem_orchestrator::SkillSourceMode;
    match policy.skill_sources.mode {
        SkillSourceMode::Any => Ok(()),
        SkillSourceMode::ProjectOnly => {
            if id.is_some() {
                return Err("skill source denied: project_only forbids skill IDs".to_string());
            }
            let Some(path) = path else {
                return Err("skill source denied: project_only requires skill path".to_string());
            };
            let p = PathBuf::from(path);
            if p.is_absolute() {
                return Err("skill source denied: absolute skill paths are forbidden".to_string());
            }
            Ok(())
        }
        SkillSourceMode::Allowlist => {
            if let Some(id) = id {
                if policy.skill_sources.allowlist_ids.iter().any(|v| v == id) {
                    return Ok(());
                }
            }
            if let Some(path) = path {
                if policy
                    .skill_sources
                    .allowlist_paths
                    .iter()
                    .any(|v| v == path)
                {
                    return Ok(());
                }
            }
            Err("skill source denied: not present in allowlist".to_string())
        }
    }
}

fn validate_pinned_hash(
    id: Option<&str>,
    path: Option<&str>,
    digest: &str,
    policy: &SpawnPolicy,
) -> Result<(), String> {
    let by_id = id.and_then(|id| policy.skill_sources.pinned_hashes.get(&format!("id:{id}")));
    let by_path =
        path.and_then(|path| policy.skill_sources.pinned_hashes.get(&format!("path:{path}")));
    let expected = by_id.or(by_path);
    if let Some(expected) = expected {
        let normalized = expected.strip_prefix("sha256:").unwrap_or(expected);
        if normalized != digest {
            return Err("pinned hash mismatch for skill reference".to_string());
        }
    }
    Ok(())
}

fn hash_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{:02x}", byte);
    }
    out
}

fn estimate_tokens(text: &str) -> u64 {
    let chars = text.chars().count() as u64;
    (chars / 4).max(1)
}

fn extract_session_id(event: &EngineEvent) -> Option<String> {
    event
        .properties
        .get("sessionID")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
        .or_else(|| {
            event
                .properties
                .get("part")
                .and_then(|v| v.get("sessionID"))
                .and_then(|v| v.as_str())
                .map(|v| v.to_string())
        })
}

fn merge_metadata_usage(
    metadata: Option<Value>,
    tokens_used: u64,
    steps_used: u32,
    tool_calls_used: u32,
    cost_used_usd: f64,
    elapsed_ms: u64,
) -> Value {
    let mut base = metadata
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();
    base.insert(
        "budgetUsage".to_string(),
        json!({
            "tokensUsed": tokens_used,
            "stepsUsed": steps_used,
            "toolCallsUsed": tool_calls_used,
            "costUsedUsd": cost_used_usd,
            "elapsedMs": elapsed_ms
        }),
    );
    Value::Object(base)
}

fn normalize_tool_name(name: &str) -> String {
    match name.trim().to_lowercase().replace('-', "_").as_str() {
        "todowrite" | "update_todo_list" | "update_todos" => "todo_write".to_string(),
        other => other.to_string(),
    }
}

async fn evaluate_capability_deny(
    state: &AppState,
    instance: &AgentInstance,
    tool: &str,
    args: &Value,
    caps: &tandem_orchestrator::CapabilitySpec,
    session_id: &str,
    message_id: &str,
) -> Option<String> {
    if !caps.tool_denylist.is_empty()
        && caps
            .tool_denylist
            .iter()
            .any(|name| normalize_tool_name(name) == *tool)
    {
        return Some(format!("tool `{tool}` denied by agent capability policy"));
    }
    if !caps.tool_allowlist.is_empty()
        && !caps
            .tool_allowlist
            .iter()
            .any(|name| normalize_tool_name(name) == *tool)
    {
        return Some(format!("tool `{tool}` not in agent allowlist"));
    }

    if matches!(tool, "websearch" | "webfetch" | "webfetch_document") {
        if !caps.net_scopes.enabled {
            return Some("network disabled for this agent instance".to_string());
        }
        if !caps.net_scopes.allow_hosts.is_empty() {
            if tool == "websearch" {
                return Some(
                    "websearch blocked: host allowlist cannot be verified for search tool"
                        .to_string(),
                );
            }
            if let Some(host) = extract_url_host(args) {
                let allowed = caps.net_scopes.allow_hosts.iter().any(|h| {
                    let allowed = h.trim().to_ascii_lowercase();
                    !allowed.is_empty()
                        && (host == allowed || host.ends_with(&format!(".{allowed}")))
                });
                if !allowed {
                    return Some(format!("network host `{host}` not in allow_hosts"));
                }
            }
        }
    }

    if tool == "bash" {
        let cmd = args
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if cmd.contains("git push") {
            if !caps.git_caps.push {
                return Some("git push disabled for this agent instance".to_string());
            }
            if caps.git_caps.push_requires_approval {
                let action = state.permissions.evaluate("git_push", "git_push").await;
                match action {
                    tandem_core::PermissionAction::Allow => {}
                    tandem_core::PermissionAction::Deny => {
                        return Some("git push denied by policy rule".to_string());
                    }
                    tandem_core::PermissionAction::Ask => {
                        let pending = state
                            .permissions
                            .ask_for_session_with_context(
                                Some(session_id),
                                "git_push",
                                args.clone(),
                                Some(tandem_core::PermissionArgsContext {
                                    args_source: "agent_team.git_push".to_string(),
                                    args_integrity: "runtime-checked".to_string(),
                                    query: Some(format!(
                                        "instanceID={} messageID={}",
                                        instance.instance_id, message_id
                                    )),
                                }),
                            )
                            .await;
                        return Some(format!(
                            "git push requires explicit user approval (approvalID={})",
                            pending.id
                        ));
                    }
                }
            }
        }
        if cmd.contains("git commit") && !caps.git_caps.commit {
            return Some("git commit disabled for this agent instance".to_string());
        }
    }

    let access_kind = tool_fs_access_kind(tool);
    if let Some(kind) = access_kind {
        let Some(session) = state.storage.get_session(session_id).await else {
            return Some("session not found for capability evaluation".to_string());
        };
        let Some(root) = session.workspace_root.clone() else {
            return Some("workspace root missing for capability evaluation".to_string());
        };
        let requested = extract_tool_candidate_paths(tool, args);
        if !requested.is_empty() {
            let allowed_scopes = if kind == "read" {
                &caps.fs_scopes.read
            } else {
                &caps.fs_scopes.write
            };
            if allowed_scopes.is_empty() {
                return Some(format!("fs {kind} access blocked: no scopes configured"));
            }
            for candidate in requested {
                if !is_path_allowed_by_scopes(&root, &candidate, allowed_scopes) {
                    return Some(format!(
                        "fs {kind} access denied for path `{}`",
                        candidate
                    ));
                }
            }
        }
    }

    denied_secrets_reason(tool, caps, args)
}

fn denied_secrets_reason(
    tool: &str,
    caps: &tandem_orchestrator::CapabilitySpec,
    args: &Value,
) -> Option<String> {
    if tool == "auth" {
        if caps.secrets_scopes.is_empty() {
            return Some("secrets are disabled for this agent instance".to_string());
        }
        let alias = args
            .get("id")
            .or_else(|| args.get("provider"))
            .or_else(|| args.get("providerID"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        if !alias.is_empty() && !caps.secrets_scopes.iter().any(|allowed| allowed == alias) {
            return Some(format!(
                "secret alias `{alias}` is not in agent secretsScopes allowlist"
            ));
        }
    }
    None
}

fn tool_fs_access_kind(tool: &str) -> Option<&'static str> {
    match tool {
        "read" | "glob" | "grep" | "codesearch" | "lsp" => Some("read"),
        "write" | "edit" | "apply_patch" => Some("write"),
        _ => None,
    }
}

fn extract_tool_candidate_paths(tool: &str, args: &Value) -> Vec<String> {
    let Some(obj) = args.as_object() else {
        return Vec::new();
    };
    let keys: &[&str] = match tool {
        "read" | "write" | "edit" | "grep" | "codesearch" => &["path", "filePath", "cwd"],
        "glob" => &["pattern"],
        "lsp" => &["filePath", "path"],
        "bash" => &["cwd"],
        "apply_patch" => &["path"],
        _ => &["path", "cwd"],
    };
    keys.iter()
        .filter_map(|key| obj.get(*key))
        .filter_map(|value| value.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(|raw| strip_glob_tokens(raw).to_string())
        .collect()
}

fn strip_glob_tokens(path: &str) -> &str {
    let mut end = path.len();
    for (idx, ch) in path.char_indices() {
        if ch == '*' || ch == '?' || ch == '[' {
            end = idx;
            break;
        }
    }
    &path[..end]
}

fn is_path_allowed_by_scopes(root: &str, candidate: &str, scopes: &[String]) -> bool {
    let root_path = PathBuf::from(root);
    let candidate_path = resolve_path(&root_path, candidate);
    scopes.iter().any(|scope| {
        let scope_path = resolve_path(&root_path, scope);
        candidate_path.starts_with(scope_path)
    })
}

fn resolve_path(root: &Path, raw: &str) -> PathBuf {
    let raw = raw.trim();
    if raw.is_empty() {
        return root.to_path_buf();
    }
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn extract_url_host(args: &Value) -> Option<String> {
    let url = args
        .get("url")
        .or_else(|| args.get("uri"))
        .or_else(|| args.get("link"))
        .and_then(|v| v.as_str())?;
    let raw = url.trim();
    let (_, after_scheme) = raw.split_once("://")?;
    let host_port = after_scheme.split('/').next().unwrap_or_default();
    let host = host_port.split('@').next_back().unwrap_or_default();
    let host = host.split(':').next().unwrap_or_default().to_ascii_lowercase();
    if host.is_empty() {
        None
    } else {
        Some(host)
    }
}

pub fn emit_spawn_requested(state: &AppState, req: &SpawnRequest) {
    emit_spawn_requested_with_context(state, req, &SpawnEventContext::default());
}

pub fn emit_spawn_denied(state: &AppState, req: &SpawnRequest, decision: &SpawnDecision) {
    emit_spawn_denied_with_context(state, req, decision, &SpawnEventContext::default());
}

pub fn emit_spawn_approved(state: &AppState, req: &SpawnRequest, instance: &AgentInstance) {
    emit_spawn_approved_with_context(state, req, instance, &SpawnEventContext::default());
}

#[derive(Default)]
pub struct SpawnEventContext<'a> {
    pub session_id: Option<&'a str>,
    pub message_id: Option<&'a str>,
    pub run_id: Option<&'a str>,
}

pub fn emit_spawn_requested_with_context(
    state: &AppState,
    req: &SpawnRequest,
    ctx: &SpawnEventContext<'_>,
) {
    state.event_bus.publish(EngineEvent::new(
        "agent_team.spawn.requested",
        json!({
            "sessionID": ctx.session_id,
            "messageID": ctx.message_id,
            "runID": ctx.run_id,
            "missionID": req.mission_id,
            "instanceID": Value::Null,
            "parentInstanceID": req.parent_instance_id,
            "source": req.source,
            "requestedRole": req.role,
            "templateID": req.template_id,
            "justification": req.justification,
            "timestampMs": crate::now_ms(),
        }),
    ));
}

pub fn emit_spawn_denied_with_context(
    state: &AppState,
    req: &SpawnRequest,
    decision: &SpawnDecision,
    ctx: &SpawnEventContext<'_>,
) {
    state.event_bus.publish(EngineEvent::new(
        "agent_team.spawn.denied",
        json!({
            "sessionID": ctx.session_id,
            "messageID": ctx.message_id,
            "runID": ctx.run_id,
            "missionID": req.mission_id,
            "instanceID": Value::Null,
            "parentInstanceID": req.parent_instance_id,
            "source": req.source,
            "requestedRole": req.role,
            "templateID": req.template_id,
            "code": decision.code,
            "error": decision.reason,
            "timestampMs": crate::now_ms(),
        }),
    ));
}

pub fn emit_spawn_approved_with_context(
    state: &AppState,
    req: &SpawnRequest,
    instance: &AgentInstance,
    ctx: &SpawnEventContext<'_>,
) {
    state.event_bus.publish(EngineEvent::new(
        "agent_team.spawn.approved",
        json!({
            "sessionID": ctx.session_id.unwrap_or(&instance.session_id),
            "messageID": ctx.message_id,
            "runID": ctx.run_id.or(instance.run_id.as_deref()),
            "missionID": instance.mission_id,
            "instanceID": instance.instance_id,
            "parentInstanceID": instance.parent_instance_id,
            "source": req.source,
            "requestedRole": req.role,
            "templateID": instance.template_id,
            "skillHash": instance.skill_hash,
            "timestampMs": crate::now_ms(),
        }),
    ));
    state.event_bus.publish(EngineEvent::new(
        "agent_team.instance.started",
        json!({
            "sessionID": ctx.session_id.unwrap_or(&instance.session_id),
            "messageID": ctx.message_id,
            "runID": ctx.run_id.or(instance.run_id.as_deref()),
            "missionID": instance.mission_id,
            "instanceID": instance.instance_id,
            "parentInstanceID": instance.parent_instance_id,
            "role": instance.role,
            "status": instance.status,
            "budgetLimit": instance.budget,
            "skillHash": instance.skill_hash,
            "timestampMs": crate::now_ms(),
        }),
    ));
}

pub fn emit_budget_usage(
    state: &AppState,
    instance: &AgentInstance,
    tokens_used: u64,
    steps_used: u32,
    tool_calls_used: u32,
    cost_used_usd: f64,
    elapsed_ms: u64,
) {
    state.event_bus.publish(EngineEvent::new(
        "agent_team.budget.usage",
        json!({
            "sessionID": instance.session_id,
            "messageID": Value::Null,
            "runID": instance.run_id,
            "missionID": instance.mission_id,
            "instanceID": instance.instance_id,
            "tokensUsed": tokens_used,
            "stepsUsed": steps_used,
            "toolCallsUsed": tool_calls_used,
            "costUsedUsd": cost_used_usd,
            "elapsedMs": elapsed_ms,
            "timestampMs": crate::now_ms(),
        }),
    ));
}

pub fn emit_budget_exhausted(
    state: &AppState,
    instance: &AgentInstance,
    exhausted_by: &str,
    tokens_used: u64,
    steps_used: u32,
    tool_calls_used: u32,
    cost_used_usd: f64,
    elapsed_ms: u64,
) {
    state.event_bus.publish(EngineEvent::new(
        "agent_team.budget.exhausted",
        json!({
            "sessionID": instance.session_id,
            "messageID": Value::Null,
            "runID": instance.run_id,
            "missionID": instance.mission_id,
            "instanceID": instance.instance_id,
            "exhaustedBy": exhausted_by,
            "tokensUsed": tokens_used,
            "stepsUsed": steps_used,
            "toolCallsUsed": tool_calls_used,
            "costUsedUsd": cost_used_usd,
            "elapsedMs": elapsed_ms,
            "timestampMs": crate::now_ms(),
        }),
    ));
}

pub fn emit_instance_cancelled(state: &AppState, instance: &AgentInstance, reason: &str) {
    state.event_bus.publish(EngineEvent::new(
        "agent_team.instance.cancelled",
        json!({
            "sessionID": instance.session_id,
            "messageID": Value::Null,
            "runID": instance.run_id,
            "missionID": instance.mission_id,
            "instanceID": instance.instance_id,
            "parentInstanceID": instance.parent_instance_id,
            "role": instance.role,
            "status": instance.status,
            "reason": reason,
            "timestampMs": crate::now_ms(),
        }),
    ));
}

pub fn emit_instance_completed(state: &AppState, instance: &AgentInstance) {
    state.event_bus.publish(EngineEvent::new(
        "agent_team.instance.completed",
        json!({
            "sessionID": instance.session_id,
            "messageID": Value::Null,
            "runID": instance.run_id,
            "missionID": instance.mission_id,
            "instanceID": instance.instance_id,
            "parentInstanceID": instance.parent_instance_id,
            "role": instance.role,
            "status": instance.status,
            "timestampMs": crate::now_ms(),
        }),
    ));
}

pub fn emit_instance_failed(state: &AppState, instance: &AgentInstance) {
    state.event_bus.publish(EngineEvent::new(
        "agent_team.instance.failed",
        json!({
            "sessionID": instance.session_id,
            "messageID": Value::Null,
            "runID": instance.run_id,
            "missionID": instance.mission_id,
            "instanceID": instance.instance_id,
            "parentInstanceID": instance.parent_instance_id,
            "role": instance.role,
            "status": instance.status,
            "timestampMs": crate::now_ms(),
        }),
    ));
}

pub fn emit_mission_budget_exhausted(
    state: &AppState,
    mission_id: &str,
    exhausted_by: &str,
    tokens_used: u64,
    steps_used: u64,
    tool_calls_used: u64,
    cost_used_usd: f64,
) {
    state.event_bus.publish(EngineEvent::new(
        "agent_team.mission.budget.exhausted",
        json!({
            "sessionID": Value::Null,
            "messageID": Value::Null,
            "runID": Value::Null,
            "missionID": mission_id,
            "instanceID": Value::Null,
            "exhaustedBy": exhausted_by,
            "tokensUsed": tokens_used,
            "stepsUsed": steps_used,
            "toolCallsUsed": tool_calls_used,
            "costUsedUsd": cost_used_usd,
            "timestampMs": crate::now_ms(),
        }),
    ));
}
