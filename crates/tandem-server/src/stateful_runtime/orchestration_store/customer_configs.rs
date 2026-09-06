// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Customer-owned configuration persistence, separate from install/activation.
//! Callers must authorize the selected installation and load current host-owned
//! bindings before calling this store. A document never supplies authority.

use anyhow::{bail, ensure};
use serde::{Deserialize, Serialize};
use tandem_enterprise_contract::VerifiedTenantContext;
use tandem_solutions::{
    blueprint_hash, customer_config_revision, prepare_customer_config,
    validate_customer_config_scope, CustomerConfig, CustomerConfigInput, CustomerScope,
    SolutionBlueprint,
};
use tandem_types::TenantContext;

use super::{protected_records, OrchestrationStateStore};
use crate::stateful_runtime::backend::{params, Executor, OptionalExtension, TransactionBehavior};

pub const CUSTOMER_CONFIG_CONFLICT: &str = "customer configuration changed concurrently";
const RECORD_KIND: &str = "solution-customer-config";

/// Generation prevents an A -> B -> A edit from accepting an old A preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CustomerConfigVersion {
    pub generation: u64,
    pub sha256: String,
}

/// Sensitive customer state. Never return this as a reusable pack export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StoredCustomerConfig {
    pub config: CustomerConfig,
    pub version: CustomerConfigVersion,
    pub blueprint_sha256: String,
    pub updated_by: String,
    pub updated_at_ms: u64,
}

fn record_id(scope: &CustomerScope, generation: u64) -> String {
    format!("{}:{generation}", scope.instance_id)
}

pub(super) fn load(
    executor: &impl Executor,
    tenant: &TenantContext,
    scope: &CustomerScope,
) -> anyhow::Result<Option<StoredCustomerConfig>> {
    let row = executor
        .query_row(
            "SELECT generation, revision, record_json FROM solution_customer_configs
         WHERE org_id=?1 AND workspace_id=?2 AND deployment_id=?3 AND instance_id=?4",
            params![
                scope.org_id,
                scope.workspace_id,
                scope.deployment_id,
                scope.instance_id
            ],
            |row| {
                Ok((
                    row.get::<_, u64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((generation, revision, payload)) = row else {
        return Ok(None);
    };
    let stored: StoredCustomerConfig =
        protected_records::decode(tenant, RECORD_KIND, &record_id(scope, generation), &payload)?;
    ensure!(
        stored.config.scope == *scope
            && stored.version.generation == generation
            && stored.version.sha256 == revision
            && customer_config_revision(&stored.config)? == revision,
        "customer configuration persisted binding mismatch"
    );
    Ok(Some(stored))
}

impl OrchestrationStateStore {
    /// Read only after the service authorizes this installation for this caller.
    /// Scope and verified-identity freshness are also checked at the store seam.
    pub fn customer_configuration(
        &self,
        context: &VerifiedTenantContext,
        selected_scope: &CustomerScope,
        now_ms: u64,
    ) -> anyhow::Result<Option<StoredCustomerConfig>> {
        validate_customer_config_scope(context, selected_scope, now_ms)?;
        self.with_connection(|connection| load(connection, &context.tenant_context, selected_scope))
    }

    /// Atomically validate the current revision, write the protected document
    /// and append its immutable version. This does not install any component.
    /// `input.current_revision` is replaced by authoritative database state.
    pub fn save_customer_configuration(
        &self,
        blueprint: &SolutionBlueprint,
        config: &CustomerConfig,
        input: CustomerConfigInput<'_>,
        expected: Option<&CustomerConfigVersion>,
    ) -> anyhow::Result<StoredCustomerConfig> {
        ensure!(
            serde_json::to_vec(config)?.len() <= tandem_solutions::MAX_BLUEPRINT_BYTES,
            "customer configuration exceeds the document size limit"
        );
        ensure!(
            input.expected_revision == expected.map(|version| version.sha256.as_str()),
            "customer configuration expected version mismatch"
        );
        validate_customer_config_scope(input.verified_context, input.selected_scope, input.now_ms)?;
        self.with_connection(|connection| {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let tenant = &input.verified_context.tenant_context;
            let scope = input.selected_scope;
            let current = load(&transaction, tenant, scope)?;
            if current.as_ref().map(|stored| &stored.version) != expected {
                bail!(CUSTOMER_CONFIG_CONFLICT);
            }
            let prepared = prepare_customer_config(blueprint, config, CustomerConfigInput {
                current_revision: current.as_ref().map(|stored| stored.version.sha256.as_str()),
                ..input
            })?;
            let blueprint_sha256 = blueprint_hash(blueprint)?;
            if let Some(stored) = current.as_ref().filter(|stored|
                stored.version.sha256 == prepared.request.customer_config_revision
                && stored.blueprint_sha256 == blueprint_sha256) {
                transaction.commit()?;
                return Ok(stored.clone());
            }
            let generation = current.as_ref().map_or(0, |stored| stored.version.generation)
                .checked_add(1).filter(|value| *value <= i64::MAX as u64)
                .ok_or_else(|| anyhow::anyhow!("customer configuration generation exhausted"))?;
            let stored = StoredCustomerConfig {
                config: config.clone(),
                version: CustomerConfigVersion { generation, sha256: prepared.request.customer_config_revision },
                blueprint_sha256,
                updated_by: input.verified_context.human_actor.actor_id.clone(),
                updated_at_ms: input.now_ms,
            };
            let payload = protected_records::encode(tenant, RECORD_KIND,
                &record_id(scope, generation), &stored)?;
            let changed = if let Some(expected) = expected {
                transaction.execute(
                    "UPDATE solution_customer_configs SET generation=?5, revision=?6, record_json=?7
                     WHERE org_id=?1 AND workspace_id=?2 AND deployment_id=?3 AND instance_id=?4
                       AND generation=?8 AND revision=?9",
                    params![scope.org_id, scope.workspace_id, scope.deployment_id, scope.instance_id,
                        generation, stored.version.sha256, payload, expected.generation, expected.sha256],
                )?
            } else {
                transaction.execute(
                    "INSERT INTO solution_customer_configs
                     (org_id, workspace_id, deployment_id, instance_id, generation, revision, record_json)
                     VALUES (?1,?2,?3,?4,?5,?6,?7)
                     ON CONFLICT (org_id, workspace_id, deployment_id, instance_id) DO NOTHING",
                    params![scope.org_id, scope.workspace_id, scope.deployment_id, scope.instance_id,
                        generation, stored.version.sha256, payload],
                )?
            };
            if changed != 1 { bail!(CUSTOMER_CONFIG_CONFLICT); }
            transaction.execute(
                "INSERT INTO solution_customer_config_versions
                 (org_id, workspace_id, deployment_id, instance_id, generation, revision, record_json)
                 VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![scope.org_id, scope.workspace_id, scope.deployment_id, scope.instance_id,
                    generation, stored.version.sha256, payload],
            )?;
            transaction.commit()?;
            Ok(stored)
        })
    }
}

// Both backends support this additive schema. BIGINT retains the full signed
// generation range in PostgreSQL and SQLite. The caller owns the transaction.
pub(crate) const SCHEMA_V6: &str = "
CREATE TABLE solution_customer_configs (
    org_id TEXT NOT NULL, workspace_id TEXT NOT NULL, deployment_id TEXT NOT NULL,
    instance_id TEXT NOT NULL, generation BIGINT NOT NULL CHECK (generation > 0),
    revision TEXT NOT NULL, record_json TEXT NOT NULL,
    PRIMARY KEY (org_id, workspace_id, deployment_id, instance_id)
);
CREATE TABLE solution_customer_config_versions (
    org_id TEXT NOT NULL, workspace_id TEXT NOT NULL, deployment_id TEXT NOT NULL,
    instance_id TEXT NOT NULL, generation BIGINT NOT NULL CHECK (generation > 0),
    revision TEXT NOT NULL, record_json TEXT NOT NULL,
    PRIMARY KEY (org_id, workspace_id, deployment_id, instance_id, generation)
);
UPDATE schema_metadata SET schema_version = 6;
";

#[cfg(feature = "storage-sqlite")]
pub(super) fn migrate_sqlite(connection: &mut rusqlite::Connection) -> anyhow::Result<()> {
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    transaction.execute_batch(SCHEMA_V6)?;
    transaction.commit()?;
    Ok(())
}
