//! Decision persistence for Goal Capability Learning discovery.

use std::collections::HashMap;
use std::sync::Arc;
use tandem_types::{CapabilityDiscoveryReport, GoalCapabilityLearningResponse, GoalSpec};
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::goal_capability_learning::discovery::discover_capabilities_for_goal;
use crate::util::time::now_ms;

/// A recorded discovery decision.
#[derive(Debug, Clone)]
pub struct DiscoveryDecision {
    pub decision_id: String,
    pub goal: GoalSpec,
    pub report: CapabilityDiscoveryReport,
    pub tenant_id: String,
    pub created_at_ms: u64,
}

/// Stores and retrieves Goal Capability Learning discovery decisions.
pub struct GoalCapabilityLearningDecisionStore {
    decisions: Arc<RwLock<HashMap<String, DiscoveryDecision>>>,
}

impl GoalCapabilityLearningDecisionStore {
    pub fn new() -> Self {
        Self {
            decisions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Discover capabilities and record the decision.
    pub async fn discover_for_goal(
        &self,
        goal: GoalSpec,
        tenant_id: String,
    ) -> GoalCapabilityLearningResponse {
        let report = discover_capabilities_for_goal(&goal);
        let uuid_str = Uuid::new_v4().to_string().replace('-', "");
        let decision_id = format!("gcl_{}", &uuid_str[..12]);

        let decision = DiscoveryDecision {
            decision_id: decision_id.clone(),
            goal,
            report: report.clone(),
            tenant_id,
            created_at_ms: now_ms(),
        };

        self.decisions
            .write()
            .await
            .insert(decision_id.clone(), decision);

        GoalCapabilityLearningResponse {
            request_id: decision_id,
            report,
        }
    }

    /// Retrieve a discovery decision.
    pub async fn get_decision(&self, decision_id: &str) -> Option<DiscoveryDecision> {
        self.decisions.read().await.get(decision_id).cloned()
    }

    /// List decisions for a tenant.
    pub async fn list_for_tenant(&self, tenant_id: &str) -> Vec<DiscoveryDecision> {
        self.decisions
            .read()
            .await
            .values()
            .filter(|d| d.tenant_id == tenant_id)
            .cloned()
            .collect()
    }
}

impl Default for GoalCapabilityLearningDecisionStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo_goal() -> GoalSpec {
        GoalSpec {
            goal_id: "demo".to_string(),
            title: "Read and parse CSV".to_string(),
            description: "Demo CSV parsing".to_string(),
            input_parameters: vec![],
            expected_output_format: "JSON records".to_string(),
            constraints: vec![],
        }
    }

    #[tokio::test]
    async fn discover_and_store() {
        let store = GoalCapabilityLearningDecisionStore::new();
        let goal = demo_goal();
        let tenant = "tenant_1".to_string();

        let response = store.discover_for_goal(goal.clone(), tenant.clone()).await;

        assert!(response.request_id.starts_with("gcl_"));
        assert!(!response.report.composition_candidates.is_empty());
    }

    #[tokio::test]
    async fn retrieve_decision() {
        let store = GoalCapabilityLearningDecisionStore::new();
        let goal = demo_goal();
        let tenant = "tenant_1".to_string();

        let response = store.discover_for_goal(goal, tenant).await;
        let id = response.request_id.clone();

        let decision = store.get_decision(&id).await;
        assert!(decision.is_some());
        assert_eq!(decision.unwrap().decision_id, id);
    }

    #[tokio::test]
    async fn list_tenant_decisions() {
        let store = GoalCapabilityLearningDecisionStore::new();
        let goal = demo_goal();

        store
            .discover_for_goal(goal.clone(), "t1".to_string())
            .await;
        store
            .discover_for_goal(goal.clone(), "t1".to_string())
            .await;
        store.discover_for_goal(goal, "t2".to_string()).await;

        let t1_decisions = store.list_for_tenant("t1").await;
        let t2_decisions = store.list_for_tenant("t2").await;

        assert_eq!(t1_decisions.len(), 2);
        assert_eq!(t2_decisions.len(), 1);
    }
}
