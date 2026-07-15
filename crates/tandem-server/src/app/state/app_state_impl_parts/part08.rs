// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

/// Goal Capability Learning integration methods.

impl AppState {
    /// Discover capabilities for a goal and record the discovery decision.
    pub async fn discover_goal_capabilities(
        &self,
        goal: tandem_types::GoalSpec,
        tenant_id: String,
    ) -> tandem_types::GoalCapabilityLearningResponse {
        self.goal_capability_learning_store
            .discover_for_goal(goal, tenant_id)
            .await
    }

    /// Retrieve a discovery decision by ID.
    pub async fn get_discovery_decision(
        &self,
        decision_id: &str,
    ) -> Option<crate::goal_capability_learning::DiscoveryDecision> {
        self.goal_capability_learning_store.get_decision(decision_id).await
    }

    /// List discovery decisions for a tenant.
    pub async fn list_discovery_decisions_for_tenant(
        &self,
        tenant_id: &str,
    ) -> Vec<crate::goal_capability_learning::DiscoveryDecision> {
        self.goal_capability_learning_store
            .list_for_tenant(tenant_id)
            .await
    }
}
