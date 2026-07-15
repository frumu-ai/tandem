// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::path::Path;

use anyhow::Context;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tandem_enterprise_contract::DataClass;
use tandem_memory::envelope::MemoryKeyScope;
use tandem_memory::types::MemoryTenantScope;
use tandem_types::TenantContext;
use tokio::fs;

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

impl GovernanceStoreFile {
    fn slug(self) -> &'static str {
        match self {
            Self::ProtectedAudit => "protected-audit",
            Self::MemoryAudit => "memory-audit",
            Self::PolicyDecisions => "policy-decisions",
            Self::OrgUnits => "org-units",
            Self::OrgUnitMemberships => "org-unit-memberships",
            Self::OrgUnitAccessGrants => "org-unit-access-grants",
            Self::CrossTenantGrants => "cross-tenant-grants",
            Self::SourceBindings => "source-bindings",
            Self::PolicyRules => "policy-rules",
        }
    }

    fn source_binding_id(self) -> String {
        format!("tandem-governance-{}", self.slug())
    }

    pub(crate) fn storage_context(self) -> crate::encrypted_file_store::ProtectedStoreContext {
        let tenant_scope = MemoryTenantScope {
            org_id: "tandem-system".to_string(),
            workspace_id: "governance-file-store".to_string(),
            deployment_id: None,
        };
        let key_scope = MemoryKeyScope::new(
            &tenant_scope,
            DataClass::Restricted,
            Some(self.source_binding_id()),
        )
        .with_org_unit(Some(self.slug().to_string()));
        crate::encrypted_file_store::ProtectedStoreContext::new(
            self.slug(),
            crate::encrypted_file_store::ProtectedRecordContext::new(
                key_scope.clone(),
                format!("tandem-governance-store:{}:manifest:v2", self.slug()),
                format!("tandem-governance-store:{}:manifest", self.slug()),
            ),
            crate::encrypted_file_store::ProtectedRecordContext::new(
                key_scope,
                format!("tandem-governance-store:{}:integrity-head:v2", self.slug()),
                format!("tandem-governance-store:{}:integrity-head", self.slug()),
            ),
        )
    }

    fn record_context(
        self,
        tenant_context: &TenantContext,
        owner_org_unit_id: Option<&str>,
        record_id: &str,
    ) -> crate::encrypted_file_store::ProtectedRecordContext {
        let tenant_scope = MemoryTenantScope {
            org_id: tenant_context.org_id.clone(),
            workspace_id: tenant_context.workspace_id.clone(),
            deployment_id: tenant_context.deployment_id.clone(),
        };
        let key_scope = MemoryKeyScope::new(
            &tenant_scope,
            DataClass::Restricted,
            Some(self.source_binding_id()),
        )
        .with_org_unit(owner_org_unit_id.map(ToOwned::to_owned));
        crate::encrypted_file_store::ProtectedRecordContext::new(
            key_scope,
            format!("tandem-governance-store:{}:v1", self.slug()),
            record_id,
        )
    }

    pub(crate) fn json_record<T>(
        self,
        key: impl Into<String>,
        value: &T,
        tenant_context: &TenantContext,
        owner_org_unit_id: Option<&str>,
    ) -> anyhow::Result<crate::encrypted_file_store::ProtectedJsonRecord>
    where
        T: Serialize,
    {
        let key = key.into();
        crate::encrypted_file_store::ProtectedJsonRecord::new(
            key.clone(),
            value,
            self.record_context(tenant_context, owner_org_unit_id, &key),
        )
    }
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

    fn is_jsonl(file: GovernanceStoreFile) -> bool {
        matches!(
            file,
            GovernanceStoreFile::ProtectedAudit | GovernanceStoreFile::MemoryAudit
        )
    }

    pub(crate) async fn read_text(
        &self,
        file: GovernanceStoreFile,
    ) -> anyhow::Result<Option<String>> {
        let path = self.path(file);
        if Self::is_jsonl(file) {
            return match fs::read_to_string(path).await {
                Ok(content) => Ok(Some(content)),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(err) => {
                    Err(err).with_context(|| format!("read governance store {}", path.display()))
                }
            };
        }

        match crate::encrypted_file_store::read_text_file(path, &file.storage_context()).await {
            Ok(content) => Ok(Some(content)),
            Err(err) => match err.downcast_ref::<std::io::Error>() {
                Some(io_err) if io_err.kind() == std::io::ErrorKind::NotFound => Ok(None),
                _ => Err(err).with_context(|| format!("read governance store {}", path.display())),
            },
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

    pub(crate) async fn write_json_records(
        &self,
        file: GovernanceStoreFile,
        records: &[crate::encrypted_file_store::ProtectedJsonRecord],
    ) -> anyhow::Result<()> {
        let path = self.path(file);
        if Self::is_jsonl(file) {
            anyhow::bail!(
                "write_json_records is unsupported for JSONL governance store {}; use append_jsonl_line",
                path.display()
            );
        }

        crate::encrypted_file_store::write_json_records_file(
            path,
            records,
            &file.storage_context(),
        )
        .await
        .with_context(|| format!("write governance store {}", path.display()))?;
        Ok(())
    }

    pub(crate) async fn append_jsonl_line(
        &self,
        file: GovernanceStoreFile,
        line: &str,
        tenant_context: &TenantContext,
        owner_org_unit_id: Option<&str>,
        record_id: &str,
        durable: bool,
    ) -> anyhow::Result<()> {
        let path = self.path(file);
        let context = file.record_context(tenant_context, owner_org_unit_id, record_id);
        crate::encrypted_file_store::append_jsonl_record_file(
            path,
            line,
            &context,
            &file.storage_context(),
            durable,
        )
        .await
        .with_context(|| format!("append governance JSONL store {}", path.display()))
    }

    pub(crate) async fn read_jsonl_lines(
        &self,
        file: GovernanceStoreFile,
    ) -> anyhow::Result<Option<Vec<String>>> {
        let path = self.path(file);
        match crate::encrypted_file_store::read_jsonl_records_file(path, &file.storage_context())
            .await
        {
            Ok(lines) => Ok(Some(lines)),
            Err(error) => match error.downcast_ref::<std::io::Error>() {
                Some(io_error) if io_error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                _ => Err(error)
                    .with_context(|| format!("read governance JSONL store {}", path.display())),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn file_store_json_and_jsonl_round_trip() {
        let state = crate::test_support::test_state().await;
        let store = for_state(&state);
        let tenant = TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a");
        let value = std::collections::HashMap::from([(
            "decision-a".to_string(),
            serde_json::json!({"result": "allow"}),
        )]);
        let records = vec![GovernanceStoreFile::PolicyDecisions
            .json_record(
                "decision-a",
                value.get("decision-a").expect("decision"),
                &tenant,
                None,
            )
            .expect("protected record")];

        store
            .write_json_records(GovernanceStoreFile::PolicyDecisions, &records)
            .await
            .expect("write json");
        let loaded: std::collections::HashMap<String, serde_json::Value> = store
            .read_json(GovernanceStoreFile::PolicyDecisions)
            .await
            .expect("read json")
            .expect("json exists");
        assert_eq!(loaded, value);

        store
            .append_jsonl_line(
                GovernanceStoreFile::ProtectedAudit,
                r#"{"seq":1}"#,
                &tenant,
                None,
                "audit-1",
                true,
            )
            .await
            .expect("append jsonl");
        let lines = store
            .read_jsonl_lines(GovernanceStoreFile::ProtectedAudit)
            .await
            .expect("read jsonl")
            .expect("jsonl exists");
        assert!(lines.iter().any(|line| line.contains(r#""seq":1"#)));
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
        let unit_records = units
            .iter()
            .map(|(key, unit)| {
                GovernanceStoreFile::OrgUnits.json_record(
                    key,
                    unit,
                    &unit.tenant_context,
                    Some(key),
                )
            })
            .collect::<anyhow::Result<Vec<_>>>()
            .expect("unit records");
        let membership_records = memberships
            .iter()
            .map(|(key, membership)| {
                GovernanceStoreFile::OrgUnitMemberships.json_record(
                    key,
                    membership,
                    &membership.tenant_context,
                    Some(&membership.unit.id),
                )
            })
            .collect::<anyhow::Result<Vec<_>>>()
            .expect("membership records");
        let grant_records = grants
            .iter()
            .map(|(key, grant)| {
                GovernanceStoreFile::OrgUnitAccessGrants.json_record(
                    key,
                    grant,
                    &grant.tenant_context,
                    Some(&grant.unit.id),
                )
            })
            .collect::<anyhow::Result<Vec<_>>>()
            .expect("grant records");

        store
            .write_json_records(GovernanceStoreFile::OrgUnits, &unit_records)
            .await
            .expect("write org units");
        store
            .write_json_records(GovernanceStoreFile::OrgUnitMemberships, &membership_records)
            .await
            .expect("write memberships");
        store
            .write_json_records(GovernanceStoreFile::OrgUnitAccessGrants, &grant_records)
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
