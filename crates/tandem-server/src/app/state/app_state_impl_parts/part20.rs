// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

impl AppState {
    /// Insert a new campaign without allowing a caller-selected ID to replace
    /// an existing campaign belonging to a different source workflow.
    pub async fn create_optimization_campaign(
        &self,
        mut campaign: OptimizationCampaignRecord,
    ) -> anyhow::Result<OptimizationCampaignRecord> {
        if campaign.optimization_id.trim().is_empty() {
            anyhow::bail!("optimization_id is required");
        }
        if campaign.source_workflow_id.trim().is_empty() {
            anyhow::bail!("source_workflow_id is required");
        }
        if campaign.name.trim().is_empty() {
            anyhow::bail!("name is required");
        }
        let now = now_ms();
        if campaign.created_at_ms == 0 {
            campaign.created_at_ms = now;
        }
        campaign.updated_at_ms = now;
        campaign.source_workflow_snapshot_hash =
            optimization_snapshot_hash(&campaign.source_workflow_snapshot);
        campaign.baseline_snapshot_hash = optimization_snapshot_hash(&campaign.baseline_snapshot);
        {
            let mut campaigns = self.optimization_campaigns.write().await;
            if campaigns.contains_key(&campaign.optimization_id) {
                anyhow::bail!("optimization id already exists");
            }
            campaigns.insert(campaign.optimization_id.clone(), campaign.clone());
        }
        self.persist_optimization_campaigns().await?;
        Ok(campaign)
    }
}
