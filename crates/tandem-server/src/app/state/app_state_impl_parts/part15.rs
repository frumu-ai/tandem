// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

impl AppState {
    fn hosted_governance_readiness_required() -> anyhow::Result<bool> {
        let config = tandem_memory::decrypt_broker::MemoryDecryptBrokerConfig::from_env()
            .context("resolve memory crypto mode for hosted governance readiness")?;
        Ok(config.crypto_mode().is_hosted())
    }

    pub(crate) async fn validate_hosted_governance_readiness(&self) -> anyhow::Result<()> {
        let probe_context =
            crate::governance_store::GovernanceStoreFile::PolicyDecisions.storage_context();
        crate::encrypted_file_store::validate_hosted_crypto_ready(&probe_context.manifest)
            .context("validate hosted governance KMS readiness")?;
        self.load_enterprise_org_units()
            .await
            .context("load hosted organization-unit store")?;
        self.load_enterprise_org_unit_memberships()
            .await
            .context("load hosted organization-unit membership store")?;
        self.load_enterprise_org_unit_access_grants()
            .await
            .context("load hosted organization-unit access-grant store")?;
        self.load_enterprise_cross_tenant_grants()
            .await
            .context("load hosted cross-tenant grant store")?;
        self.load_enterprise_source_bindings()
            .await
            .context("load hosted source-binding store")?;
        self.load_policy_decisions()
            .await
            .context("load hosted policy-decision store")?;
        self.load_enterprise_policy_rules_if_needed()
            .await
            .context("load hosted policy-rule store")?;
        crate::audit::validate_protected_audit_ledger_if_present(&self.protected_audit_path)
            .await
            .context("validate hosted protected-audit store")?;
        crate::http::memory_audit_store::load_memory_audit_events_strict(self)
            .await
            .context("validate hosted memory-audit store")?;
        Ok(())
    }
}
