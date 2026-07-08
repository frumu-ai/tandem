use std::path::Path;

use anyhow::Context;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::fs::{self, OpenOptions};
use tokio::io::AsyncWriteExt;

use crate::AppState;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum GovernanceStoreFile {
    ProtectedAudit,
    MemoryAudit,
    PolicyDecisions,
    OrgUnits,
    OrgUnitMemberships,
    OrgUnitAccessGrants,
    CrossTenantGrants,
    SourceBindings,
    PolicyRules,
}

/// Logical governance store facade. Today this is file-backed; the enum keeps
/// call sites tied to store intent rather than concrete file paths so a DB
/// backend can replace these operations in one module.
pub(crate) struct GovernanceStore<'a> {
    state: &'a AppState,
}

pub(crate) fn for_state(state: &AppState) -> GovernanceStore<'_> {
    GovernanceStore { state }
}

impl<'a> GovernanceStore<'a> {
    fn path(&self, file: GovernanceStoreFile) -> &Path {
        match file {
            GovernanceStoreFile::ProtectedAudit => &self.state.protected_audit_path,
            GovernanceStoreFile::MemoryAudit => &self.state.memory_audit_path,
            GovernanceStoreFile::PolicyDecisions => &self.state.policy_decisions_path,
            GovernanceStoreFile::OrgUnits => &self.state.enterprise.org_units_path,
            GovernanceStoreFile::OrgUnitMemberships => {
                &self.state.enterprise.org_unit_memberships_path
            }
            GovernanceStoreFile::OrgUnitAccessGrants => {
                &self.state.enterprise.org_unit_access_grants_path
            }
            GovernanceStoreFile::CrossTenantGrants => {
                &self.state.enterprise.cross_tenant_grants_path
            }
            GovernanceStoreFile::SourceBindings => &self.state.enterprise.source_bindings_path,
            GovernanceStoreFile::PolicyRules => &self.state.enterprise.policy_rules_path,
        }
    }

    pub(crate) async fn read_text(
        &self,
        file: GovernanceStoreFile,
    ) -> anyhow::Result<Option<String>> {
        let path = self.path(file);
        match fs::read_to_string(path).await {
            Ok(content) => Ok(Some(content)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => {
                Err(err).with_context(|| format!("read governance store {}", path.display()))
            }
        }
    }

    pub(crate) async fn read_json<T>(&self, file: GovernanceStoreFile) -> anyhow::Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let Some(content) = self.read_text(file).await? else {
            return Ok(None);
        };
        let path = self.path(file);
        let parsed = serde_json::from_str(&content)
            .with_context(|| format!("parse governance store JSON {}", path.display()))?;
        Ok(Some(parsed))
    }

    pub(crate) async fn write_json<T>(
        &self,
        file: GovernanceStoreFile,
        value: &T,
    ) -> anyhow::Result<()>
    where
        T: Serialize,
    {
        let payload = serde_json::to_string_pretty(value)?;
        self.write_text(file, &payload).await
    }

    pub(crate) async fn write_text(
        &self,
        file: GovernanceStoreFile,
        payload: &str,
    ) -> anyhow::Result<()> {
        let path = self.path(file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(path, payload)
            .await
            .with_context(|| format!("write governance store {}", path.display()))?;
        Ok(())
    }

    pub(crate) async fn append_jsonl_line(
        &self,
        file: GovernanceStoreFile,
        line: &str,
        durable: bool,
    ) -> anyhow::Result<()> {
        let path = self.path(file);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await
            .with_context(|| format!("open governance JSONL store {}", path.display()))?;
        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        file.flush().await?;
        if durable {
            file.sync_all().await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn file_store_json_and_jsonl_round_trip() {
        let state = crate::test_support::test_state().await;
        let store = for_state(&state);
        let value = std::collections::HashMap::from([(
            "decision-a".to_string(),
            serde_json::json!({"result": "allow"}),
        )]);

        store
            .write_json(GovernanceStoreFile::PolicyDecisions, &value)
            .await
            .expect("write json");
        let loaded: std::collections::HashMap<String, serde_json::Value> = store
            .read_json(GovernanceStoreFile::PolicyDecisions)
            .await
            .expect("read json")
            .expect("json exists");
        assert_eq!(loaded, value);

        store
            .append_jsonl_line(GovernanceStoreFile::ProtectedAudit, "{\"seq\":1}", true)
            .await
            .expect("append jsonl");
        let content = store
            .read_text(GovernanceStoreFile::ProtectedAudit)
            .await
            .expect("read jsonl")
            .expect("jsonl exists");
        assert!(content.contains("\"seq\":1"));
    }
    #[tokio::test]
    async fn org_unit_registries_load_through_store() {
        let state = crate::test_support::test_state().await;
        let store = for_state(&state);
        let fixture = tandem_enterprise_contract::authority::fixtures::acme_company();
        let units = fixture
            .graph
            .units
            .iter()
            .map(|unit| {
                (
                    format!("{}/{}", unit.taxonomy_id, unit.unit_id),
                    unit.clone(),
                )
            })
            .collect::<std::collections::HashMap<_, _>>();
        let memberships = fixture
            .graph
            .memberships
            .iter()
            .map(|membership| (membership.membership_id.clone(), membership.clone()))
            .collect::<std::collections::HashMap<_, _>>();
        let grants = fixture
            .graph
            .unit_access_grants
            .iter()
            .map(|grant| (grant.grant_id.clone(), grant.clone()))
            .collect::<std::collections::HashMap<_, _>>();

        store
            .write_json(GovernanceStoreFile::OrgUnits, &units)
            .await
            .expect("write org units");
        store
            .write_json(GovernanceStoreFile::OrgUnitMemberships, &memberships)
            .await
            .expect("write memberships");
        store
            .write_json(GovernanceStoreFile::OrgUnitAccessGrants, &grants)
            .await
            .expect("write grants");

        state.load_enterprise_org_units().await.expect("load units");
        state
            .load_enterprise_org_unit_memberships()
            .await
            .expect("load memberships");
        state
            .load_enterprise_org_unit_access_grants()
            .await
            .expect("load grants");

        assert_eq!(state.enterprise.org_units.read().await.len(), units.len());
        assert_eq!(
            state.enterprise.org_unit_memberships.read().await.len(),
            memberships.len()
        );
        assert_eq!(
            state.enterprise.org_unit_access_grants.read().await.len(),
            grants.len()
        );
    }
}
