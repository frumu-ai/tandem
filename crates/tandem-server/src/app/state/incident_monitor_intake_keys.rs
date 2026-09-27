// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

impl AppState {
    pub(crate) async fn list_incident_monitor_intake_keys_checked(
        &self,
        authorize: impl FnOnce() -> anyhow::Result<()>,
    ) -> anyhow::Result<Vec<IncidentMonitorProjectIntakeKey>> {
        let mut rows = {
            let current = self.incident_monitor_intake_keys.read().await;
            authorize()?;
            current.values().cloned().collect::<Vec<_>>()
        };
        rows.sort_by(|a, b| a.project_id.cmp(&b.project_id).then(a.name.cmp(&b.name)));
        Ok(rows)
    }

    pub(crate) async fn put_incident_monitor_intake_key_checked(
        &self,
        key: IncidentMonitorProjectIntakeKey,
        authorize: impl FnOnce() -> anyhow::Result<()>,
    ) -> anyhow::Result<IncidentMonitorProjectIntakeKey> {
        {
            let mut current = self.incident_monitor_intake_keys.write().await;
            authorize()?;
            current.insert(key.key_id.clone(), key.clone());
        }
        self.persist_incident_monitor_intake_keys().await?;
        Ok(key)
    }

    pub(crate) async fn disable_incident_monitor_intake_key_checked(
        &self,
        id: &str,
        authorize: impl FnOnce() -> anyhow::Result<()>,
    ) -> anyhow::Result<Option<IncidentMonitorProjectIntakeKey>> {
        let key = {
            let mut current = self.incident_monitor_intake_keys.write().await;
            // Check before looking up the key: denial must not reveal existence.
            // Read and disable the current row under the same lock.
            authorize()?;
            let Some(key) = current.get_mut(id) else {
                return Ok(None);
            };
            key.enabled = false;
            key.clone()
        };
        self.persist_incident_monitor_intake_keys().await?;
        Ok(Some(key))
    }
}
