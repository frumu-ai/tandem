use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Orchestrator,
    Delegator,
    Worker,
    Watcher,
    Reviewer,
    Tester,
    Committer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpawnSource {
    OrchestratorRuntime,
    UiAction,
    ToolCall,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpawnBehavior {
    Allow,
    Deny,
    RequestOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpawnDenyCode {
    SpawnPolicyMissing,
    SpawnPolicyDisabled,
    SpawnDeniedEdge,
    SpawnJustificationRequired,
    SpawnMaxAgentsExceeded,
    SpawnMaxConcurrentExceeded,
    SpawnMissionBudgetExceeded,
    SpawnRequiresApproval,
    SpawnRequiredSkillMissing,
    SpawnSkillSourceDenied,
    SpawnSkillHashMismatch,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BudgetLimit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_steps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FsScopes {
    #[serde(default)]
    pub read: Vec<String>,
    #[serde(default)]
    pub write: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetScopes {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub allow_hosts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GitCaps {
    #[serde(default)]
    pub read: bool,
    #[serde(default)]
    pub commit: bool,
    #[serde(default)]
    pub push: bool,
    #[serde(default)]
    pub push_requires_approval: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CapabilitySpec {
    #[serde(default)]
    pub tool_allowlist: Vec<String>,
    #[serde(default)]
    pub tool_denylist: Vec<String>,
    #[serde(default)]
    pub fs_scopes: FsScopes,
    #[serde(default)]
    pub net_scopes: NetScopes,
    #[serde(default)]
    pub secrets_scopes: Vec<String>,
    #[serde(default)]
    pub git_caps: GitCaps,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SkillRequirement {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTemplate {
    #[serde(rename = "templateID")]
    pub template_id: String,
    pub role: AgentRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub skills: Vec<SkillRef>,
    #[serde(default)]
    pub default_budget: BudgetLimit,
    #[serde(default)]
    pub capabilities: CapabilitySpec,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoleSpawnRule {
    #[serde(default)]
    pub behavior: Option<SpawnBehavior>,
    #[serde(default)]
    pub can_spawn: Vec<AgentRole>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SkillSourceMode {
    ProjectOnly,
    Allowlist,
    #[default]
    Any,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillSourcePolicy {
    #[serde(default)]
    pub mode: SkillSourceMode,
    #[serde(default)]
    pub allowlist_ids: Vec<String>,
    #[serde(default)]
    pub allowlist_paths: Vec<String>,
    #[serde(default)]
    pub pinned_hashes: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub require_justification: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_agents: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_concurrent: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_budget_percent_of_parent_remaining: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission_total_budget: Option<BudgetLimit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_per_1k_tokens_usd: Option<f64>,
    #[serde(default)]
    pub spawn_edges: HashMap<AgentRole, RoleSpawnRule>,
    #[serde(default)]
    pub required_skills: HashMap<AgentRole, Vec<SkillRequirement>>,
    #[serde(default)]
    pub role_defaults: HashMap<AgentRole, BudgetLimit>,
    #[serde(default)]
    pub skill_sources: SkillSourcePolicy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnRequest {
    #[serde(rename = "missionID", default, skip_serializing_if = "Option::is_none")]
    pub mission_id: Option<String>,
    #[serde(
        rename = "parentInstanceID",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub parent_instance_id: Option<String>,
    pub source: SpawnSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_role: Option<AgentRole>,
    pub role: AgentRole,
    #[serde(
        rename = "templateID",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub template_id: Option<String>,
    pub justification: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_override: Option<BudgetLimit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnDecision {
    pub allowed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<SpawnDenyCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default)]
    pub requires_user_approval: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentInstanceStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInstance {
    #[serde(rename = "instanceID")]
    pub instance_id: String,
    #[serde(rename = "missionID")]
    pub mission_id: String,
    #[serde(
        rename = "parentInstanceID",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub parent_instance_id: Option<String>,
    pub role: AgentRole,
    #[serde(rename = "templateID")]
    pub template_id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "runID", default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub status: AgentInstanceStatus,
    pub budget: BudgetLimit,
    #[serde(rename = "skillHash")]
    pub skill_hash: String,
    #[serde(default)]
    pub capabilities: CapabilitySpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl SpawnPolicy {
    pub fn evaluate(
        &self,
        req: &SpawnRequest,
        total_agents: usize,
        running_agents: usize,
        template: Option<&AgentTemplate>,
    ) -> SpawnDecision {
        if !self.enabled {
            return deny(
                SpawnDenyCode::SpawnPolicyDisabled,
                "spawn policy is disabled".to_string(),
            );
        }
        if self.require_justification && req.justification.trim().is_empty() {
            return deny(
                SpawnDenyCode::SpawnJustificationRequired,
                "spawn requires a non-empty justification".to_string(),
            );
        }
        if let Some(max_agents) = self.max_agents {
            if total_agents as u32 >= max_agents {
                return deny(
                    SpawnDenyCode::SpawnMaxAgentsExceeded,
                    format!("maxAgents exceeded ({total_agents}/{max_agents})"),
                );
            }
        }
        if let Some(max_concurrent) = self.max_concurrent {
            if running_agents as u32 >= max_concurrent {
                return deny(
                    SpawnDenyCode::SpawnMaxConcurrentExceeded,
                    format!("maxConcurrent exceeded ({running_agents}/{max_concurrent})"),
                );
            }
        }
        if let Some(parent_role) = req.parent_role.as_ref() {
            let Some(edge) = self.spawn_edges.get(parent_role) else {
                return deny(
                    SpawnDenyCode::SpawnDeniedEdge,
                    format!("parent role `{parent_role:?}` cannot spawn children"),
                );
            };
            let behavior = edge.behavior.clone().unwrap_or(SpawnBehavior::Deny);
            if !edge.can_spawn.contains(&req.role) {
                return deny(
                    SpawnDenyCode::SpawnDeniedEdge,
                    format!(
                        "role `{parent_role:?}` cannot spawn `{}`",
                        role_name(&req.role)
                    ),
                );
            }
            if behavior == SpawnBehavior::Deny {
                return deny(
                    SpawnDenyCode::SpawnDeniedEdge,
                    format!("spawn edge denied for role `{parent_role:?}`"),
                );
            }
            if behavior == SpawnBehavior::RequestOnly {
                return deny(
                    SpawnDenyCode::SpawnRequiresApproval,
                    format!("spawn for role `{parent_role:?}` requires user approval"),
                )
                .with_requires_user_approval();
            }
        }
        if let Some(required) = self.required_skills.get(&req.role) {
            let Some(template) = template else {
                return deny(
                    SpawnDenyCode::SpawnRequiredSkillMissing,
                    "template required to validate skills".to_string(),
                );
            };
            for requirement in required {
                if !template_has_requirement(template, requirement) {
                    return deny(
                        SpawnDenyCode::SpawnRequiredSkillMissing,
                        format!(
                            "template `{}` is missing required skill for role `{}`",
                            template.template_id,
                            role_name(&req.role)
                        ),
                    );
                }
            }
        }
        SpawnDecision {
            allowed: true,
            code: None,
            reason: None,
            requires_user_approval: false,
        }
    }
}

fn template_has_requirement(template: &AgentTemplate, req: &SkillRequirement) -> bool {
    template.skills.iter().any(|skill| {
        let id_match = match (&req.id, &skill.id) {
            (Some(required), Some(actual)) => required == actual,
            (None, _) => true,
            _ => false,
        };
        let path_match = match (&req.path, &skill.path) {
            (Some(required), Some(actual)) => required == actual,
            (None, _) => true,
            _ => false,
        };
        id_match && path_match
    })
}

fn role_name(role: &AgentRole) -> &'static str {
    match role {
        AgentRole::Orchestrator => "orchestrator",
        AgentRole::Delegator => "delegator",
        AgentRole::Worker => "worker",
        AgentRole::Watcher => "watcher",
        AgentRole::Reviewer => "reviewer",
        AgentRole::Tester => "tester",
        AgentRole::Committer => "committer",
    }
}

fn deny(code: SpawnDenyCode, reason: String) -> SpawnDecision {
    SpawnDecision {
        allowed: false,
        code: Some(code),
        reason: Some(reason),
        requires_user_approval: false,
    }
}

trait SpawnDecisionExt {
    fn with_requires_user_approval(self) -> Self;
}

impl SpawnDecisionExt for SpawnDecision {
    fn with_requires_user_approval(mut self) -> Self {
        self.requires_user_approval = true;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_policy() -> SpawnPolicy {
        let mut edges = HashMap::new();
        edges.insert(
            AgentRole::Orchestrator,
            RoleSpawnRule {
                behavior: Some(SpawnBehavior::Allow),
                can_spawn: vec![
                    AgentRole::Delegator,
                    AgentRole::Worker,
                    AgentRole::Watcher,
                    AgentRole::Reviewer,
                    AgentRole::Tester,
                    AgentRole::Committer,
                ],
            },
        );
        SpawnPolicy {
            enabled: true,
            require_justification: true,
            max_agents: Some(10),
            max_concurrent: Some(3),
            child_budget_percent_of_parent_remaining: Some(40),
            mission_total_budget: None,
            cost_per_1k_tokens_usd: None,
            spawn_edges: edges,
            required_skills: HashMap::new(),
            role_defaults: HashMap::new(),
            skill_sources: SkillSourcePolicy::default(),
        }
    }

    #[test]
    fn policy_requires_justification() {
        let policy = base_policy();
        let req = SpawnRequest {
            mission_id: Some("m1".to_string()),
            parent_instance_id: Some("p1".to_string()),
            source: SpawnSource::UiAction,
            parent_role: Some(AgentRole::Orchestrator),
            role: AgentRole::Worker,
            template_id: Some("worker-default".to_string()),
            justification: "".to_string(),
            budget_override: None,
        };
        let decision = policy.evaluate(&req, 0, 0, None);
        assert!(!decision.allowed);
        assert_eq!(
            decision.code,
            Some(SpawnDenyCode::SpawnJustificationRequired)
        );
    }

    #[test]
    fn policy_enforces_edges() {
        let policy = base_policy();
        let req = SpawnRequest {
            mission_id: Some("m1".to_string()),
            parent_instance_id: Some("p1".to_string()),
            source: SpawnSource::ToolCall,
            parent_role: Some(AgentRole::Worker),
            role: AgentRole::Tester,
            template_id: Some("tester-default".to_string()),
            justification: "needs validation".to_string(),
            budget_override: None,
        };
        let decision = policy.evaluate(&req, 1, 1, None);
        assert!(!decision.allowed);
        assert_eq!(decision.code, Some(SpawnDenyCode::SpawnDeniedEdge));
    }

    #[test]
    fn policy_enforces_required_skills() {
        let mut policy = base_policy();
        policy.required_skills.insert(
            AgentRole::Worker,
            vec![SkillRequirement {
                id: Some("rust-editing".to_string()),
                path: None,
            }],
        );
        let req = SpawnRequest {
            mission_id: Some("m1".to_string()),
            parent_instance_id: Some("p1".to_string()),
            source: SpawnSource::UiAction,
            parent_role: Some(AgentRole::Orchestrator),
            role: AgentRole::Worker,
            template_id: Some("worker-default".to_string()),
            justification: "implement patch".to_string(),
            budget_override: None,
        };
        let template = AgentTemplate {
            template_id: "worker-default".to_string(),
            role: AgentRole::Worker,
            system_prompt: None,
            skills: vec![],
            default_budget: BudgetLimit::default(),
            capabilities: CapabilitySpec::default(),
        };
        let decision = policy.evaluate(&req, 1, 1, Some(&template));
        assert!(!decision.allowed);
        assert_eq!(
            decision.code,
            Some(SpawnDenyCode::SpawnRequiredSkillMissing)
        );
    }
}
