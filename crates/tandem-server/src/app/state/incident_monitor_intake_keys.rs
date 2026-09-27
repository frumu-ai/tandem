// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

impl AppState {
    pub async fn persist_incident_monitor_intake_keys(&self) -> anyhow::Result<()> {
        let guard = self
            .incident_monitor_intake_keys
            .clone()
            .write_owned()
            .await;
        let path = self.incident_monitor_intake_keys_path.clone();
        // Own the guard in the blocking task: dropping the caller must not let
        // another snapshot race a file write that is still running.
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let payload = serde_json::to_string_pretty(&*guard)?;
            let result =
                write_state_file_atomically_blocking(&path, &payload).map_err(anyhow::Error::from);
            drop(guard);
            result
        })
        .await?
    }

    pub async fn validate_incident_monitor_intake_key(
        &self,
        raw_key: &str,
        project_id: &str,
        required_scope: &str,
    ) -> Option<IncidentMonitorProjectIntakeKey> {
        let key_hash = crate::sha256_hex(&[raw_key.trim()]);
        let matched = {
            let mut current = self.incident_monitor_intake_keys.write().await;
            let matched = current.values_mut().find(|row| {
                row.enabled
                    && row.project_id == project_id
                    && crate::constant_time_str_eq(&row.key_hash, &key_hash)
                    && row.scopes.iter().any(|scope| scope == required_scope)
            })?;
            // Usage bookkeeping changes only the current row, never a stale
            // copy that could restore a concurrently disabled credential.
            matched.last_used_at_ms = Some(now_ms());
            matched.clone()
        };
        let _ = self.persist_incident_monitor_intake_keys().await;
        Some(matched)
    }

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
