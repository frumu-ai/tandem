use std::collections::HashMap;

pub use tandem_enterprise_contract::governance::*;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeletedAutomationRecord {
    pub automation: crate::AutomationV2Spec,
    pub deleted_at_ms: u64,
    pub deleted_by: GovernanceActorRef,
    pub restore_until_ms: u64,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct GovernanceState {
    #[serde(default)]
    pub records: HashMap<String, AutomationGovernanceRecord>,
    #[serde(default)]
    pub approvals: HashMap<String, GovernanceApprovalRequest>,
    #[serde(default)]
    pub deleted_automations: HashMap<String, DeletedAutomationRecord>,
    #[serde(default)]
    pub paused_agents: Vec<String>,
    #[serde(default)]
    pub spend_paused_agents: Vec<String>,
    #[serde(default)]
    pub agent_spend: HashMap<String, AgentSpendSummary>,
    #[serde(default)]
    pub agent_creation_reviews: HashMap<String, AgentCreationReviewSummary>,
    #[serde(default)]
    pub limits: GovernanceLimits,
    #[serde(default)]
    pub updated_at_ms: u64,
}

impl GovernanceState {
    pub fn snapshot(&self) -> GovernanceContextSnapshot {
        GovernanceContextSnapshot {
            records: self.records.clone(),
            approvals: self.approvals.clone(),
            paused_agents: self.paused_agents.clone(),
            spend_paused_agents: self.spend_paused_agents.clone(),
            agent_spend: self.agent_spend.clone(),
            agent_creation_reviews: self.agent_creation_reviews.clone(),
            limits: self.limits.clone(),
        }
    }

    pub fn is_agent_paused(&self, actor_id: &str) -> bool {
        self.paused_agents.iter().any(|value| value == actor_id)
    }

    pub fn is_agent_spend_paused(&self, actor_id: &str) -> bool {
        self.spend_paused_agents
            .iter()
            .any(|value| value == actor_id)
    }

    pub fn has_approved_agent_capability(&self, agent_id: &str, capability_key: &str) -> bool {
        self.snapshot()
            .has_approved_agent_capability(agent_id, capability_key, crate::now_ms())
    }

    pub fn has_approved_agent_quota_override(&self, agent_id: &str) -> bool {
        self.snapshot()
            .has_approved_agent_quota_override(agent_id, crate::now_ms())
    }

    pub fn has_pending_agent_quota_override(&self, agent_id: &str) -> bool {
        self.snapshot()
            .has_pending_agent_quota_override(agent_id, crate::now_ms())
    }

    pub fn has_pending_approval_request(
        &self,
        request_type: GovernanceApprovalRequestType,
        resource_type: &str,
        resource_id: &str,
    ) -> bool {
        self.snapshot().has_pending_approval_request(
            request_type,
            resource_type,
            resource_id,
            crate::now_ms(),
        )
    }

    pub fn agent_spend_summary(&self, agent_id: &str) -> Option<AgentSpendSummary> {
        self.agent_spend.get(agent_id).cloned()
    }

    pub fn agent_spend_summaries(&self) -> Vec<AgentSpendSummary> {
        let mut rows = self.agent_spend.values().cloned().collect::<Vec<_>>();
        rows.sort_by(|a, b| b.weekly.cost_usd.total_cmp(&a.weekly.cost_usd));
        rows
    }

    pub fn agent_creation_review_summary(
        &self,
        agent_id: &str,
    ) -> Option<AgentCreationReviewSummary> {
        self.agent_creation_reviews.get(agent_id).cloned()
    }

    pub fn agent_creation_review_summaries(&self) -> Vec<AgentCreationReviewSummary> {
        let mut rows = self
            .agent_creation_reviews
            .values()
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| {
            b.review_required
                .cmp(&a.review_required)
                .then_with(|| b.updated_at_ms.cmp(&a.updated_at_ms))
        });
        rows
    }
}
