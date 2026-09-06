// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Durable staging journal, not an installer or a grant of authority. The
//! service must authorize the selected installation and supply current trusted
//! host facts. Runtime adapters must reconcile claimed effects after a crash;
//! a journal retry never grants permission to repeat an unknown external effect.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, ensure};
use serde::{Deserialize, Serialize};
use tandem_enterprise_contract::VerifiedTenantContext;
use tandem_solutions::{
    prepare_customer_config, resolve, validate_customer_config_scope, CustomerConfigInput,
    CustomerScope, ModelBinding, ResolutionInput, ResolvedPlan, SolutionBlueprint,
};

use super::{customer_configs, protected_records, CustomerConfigVersion, OrchestrationStateStore};
use crate::stateful_runtime::backend::{params, Executor, OptionalExtension, TransactionBehavior};

const RECORD_KIND: &str = "solution-installation";
pub const SOLUTION_INSTALLATION_CONFLICT: &str =
    "solution installation changed; review current state";

/// Host-owned inputs, never deserialized from an HTTP install request. Every
/// transition resolves again against the configuration read in its transaction.
pub struct SolutionInstallationInput<'a> {
    pub configuration: CustomerConfigInput<'a>,
    pub expected_config: &'a CustomerConfigVersion,
    pub blueprint: &'a SolutionBlueprint,
    pub engine_version: &'a str,
    pub available_deployment_requirements: &'a BTreeSet<String>,
    pub approved_models: &'a BTreeMap<String, ModelBinding>,
    pub artifacts: &'a BTreeMap<String, Vec<u8>>,
    pub reviewed_composition: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum SolutionComponentProgress {
    Pending,
    /// No timeout takeover: the adapter must inspect this stable resource ID.
    Claimed {
        attempt_id: String,
    },
    Staged {
        attempt_id: String,
        resource_sha256: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolutionInstallation {
    pub generation: u64,
    pub config_version: CustomerConfigVersion,
    pub composition_sha256: String,
    pub plan: ResolvedPlan,
    pub components: BTreeMap<String, SolutionComponentProgress>,
    pub updated_at_ms: u64,
}

impl SolutionInstallation {
    /// Staged resources still need separate explicit, currently authorized
    /// activation. This predicate is never solution readiness or activation.
    pub fn all_components_staged(&self) -> bool {
        !self.components.is_empty()
            && self
                .components
                .values()
                .all(|value| matches!(value, SolutionComponentProgress::Staged { .. }))
    }
}

pub enum SolutionInstallationTransition<'a> {
    Begin,
    Claim {
        component_id: &'a str,
        attempt_id: &'a str,
    },
    /// The adapter supplies the fingerprint of the observed disabled resource,
    /// after checking its stable ID, ownership and exact content. Also used for
    /// reconciliation of a successful effect whose receipt was lost in a crash.
    RecordStaged {
        component_id: &'a str,
        attempt_id: &'a str,
        resource_sha256: &'a str,
    },
}

fn load(
    executor: &impl Executor,
    context: &VerifiedTenantContext,
    scope: &CustomerScope,
) -> anyhow::Result<Option<SolutionInstallation>> {
    let row = executor
        .query_row(
            "SELECT generation, record_json FROM solution_installations
         WHERE org_id=?1 AND workspace_id=?2 AND deployment_id=?3 AND instance_id=?4",
            params![
                scope.org_id,
                scope.workspace_id,
                scope.deployment_id,
                scope.instance_id
            ],
            |row| Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((generation, payload)) = row else {
        return Ok(None);
    };
    let record: SolutionInstallation = protected_records::decode(
        &context.tenant_context,
        RECORD_KIND,
        &format!("{}:{generation}", scope.instance_id),
        &payload,
    )?;
    ensure!(
        record.generation == generation
            && record.plan.instance_id == scope.instance_id
            && record.plan.authority.org_id == scope.org_id
            && record.plan.authority.workspace_id == scope.workspace_id
            && record.plan.authority.deployment_id == scope.deployment_id
            && record.plan.composition_hash()? == record.composition_sha256
            && record.plan.customer_config_revision == record.config_version.sha256
            && record.components.keys().eq(record.plan.components.keys()),
        "solution installation persisted binding mismatch"
    );
    Ok(Some(record))
}

impl OrchestrationStateStore {
    /// Service must authorize the selected installation before exposing it.
    /// Returns progress even if configuration has since changed, for diagnosis.
    pub fn solution_installation(
        &self,
        context: &VerifiedTenantContext,
        scope: &CustomerScope,
        now_ms: u64,
    ) -> anyhow::Result<Option<SolutionInstallation>> {
        validate_customer_config_scope(context, scope, now_ms)?;
        self.with_connection(|connection| load(connection, context, scope))
    }

    pub fn transition_solution_installation(
        &self,
        input: SolutionInstallationInput<'_>,
        expected_generation: Option<u64>,
        transition: SolutionInstallationTransition<'_>,
    ) -> anyhow::Result<SolutionInstallation> {
        let config_input = input.configuration;
        let context = config_input.verified_context;
        let scope = config_input.selected_scope;
        validate_customer_config_scope(context, scope, config_input.now_ms)?;
        self.with_connection(|connection| {
            let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let config = customer_configs::load(&transaction, &context.tenant_context, scope)?
                .ok_or_else(|| anyhow::anyhow!("save customer configuration before installation"))?;
            ensure!(&config.version == input.expected_config, SOLUTION_INSTALLATION_CONFLICT);
            let now_ms = config_input.now_ms;
            let prepared = prepare_customer_config(input.blueprint, &config.config, CustomerConfigInput {
                current_revision: Some(&config.version.sha256),
                expected_revision: Some(&config.version.sha256),
                ..config_input
            })?;
            let plan = resolve(input.blueprint, ResolutionInput {
                request: &prepared.request, verified_context: context, now_ms,
                engine_version: input.engine_version, deployment_policy: &prepared.deployment_policy,
                available_deployment_requirements: input.available_deployment_requirements,
                approved_models: input.approved_models, artifacts: input.artifacts,
            })?;
            let composition = plan.composition_hash()?;
            ensure!(composition == input.reviewed_composition
                && plan.blueprint_sha256 == config.blueprint_sha256,
                "solution preview is stale; resolve and review again");
            let current = load(&transaction, context, scope)?;
            if let Some(current) = &current {
                ensure!(current.composition_sha256 == composition
                    && current.config_version == config.version, SOLUTION_INSTALLATION_CONFLICT);
                // Repeating Begin is a read of the same intent, not permission
                // to perform effects. All authority/binding checks above repeat.
                if matches!(transition, SolutionInstallationTransition::Begin) {
                    ensure!(expected_generation.is_none()
                        || expected_generation == Some(current.generation), SOLUTION_INSTALLATION_CONFLICT);
                    transaction.commit()?;
                    return Ok(current.clone());
                }
            }
            ensure!(current.as_ref().map(|record| record.generation) == expected_generation,
                SOLUTION_INSTALLATION_CONFLICT);
            let mut next = match current {
                Some(record) => record,
                None => {
                    ensure!(matches!(transition, SolutionInstallationTransition::Begin),
                        "begin installation before staging components");
                    SolutionInstallation {
                        generation: 0, config_version: config.version,
                        composition_sha256: composition,
                        components: plan.components.keys().map(|id|
                            (id.clone(), SolutionComponentProgress::Pending)).collect(),
                        plan, updated_at_ms: now_ms,
                    }
                }
            };
            apply_transition(&mut next, transition)?;
            let previous_generation = next.generation;
            next.generation = previous_generation.checked_add(1)
                .filter(|value| *value <= i64::MAX as u64)
                .ok_or_else(|| anyhow::anyhow!("solution installation generation exhausted"))?;
            next.updated_at_ms = now_ms;
            let payload = protected_records::encode(&context.tenant_context, RECORD_KIND,
                &format!("{}:{}", scope.instance_id, next.generation), &next)?;
            let changed = if previous_generation == 0 {
                transaction.execute("INSERT INTO solution_installations
                    (org_id,workspace_id,deployment_id,instance_id,generation,record_json)
                    VALUES (?1,?2,?3,?4,?5,?6)
                    ON CONFLICT (org_id,workspace_id,deployment_id,instance_id) DO NOTHING",
                    params![scope.org_id,scope.workspace_id,scope.deployment_id,scope.instance_id,
                        next.generation,payload])?
            } else {
                transaction.execute("UPDATE solution_installations SET generation=?5,record_json=?6
                    WHERE org_id=?1 AND workspace_id=?2 AND deployment_id=?3 AND instance_id=?4 AND generation=?7",
                    params![scope.org_id,scope.workspace_id,scope.deployment_id,scope.instance_id,
                        next.generation,payload,previous_generation])?
            };
            ensure!(changed == 1, SOLUTION_INSTALLATION_CONFLICT);
            transaction.execute("INSERT INTO solution_installation_versions
                (org_id,workspace_id,deployment_id,instance_id,generation,record_json)
                VALUES (?1,?2,?3,?4,?5,?6)",
                params![scope.org_id,scope.workspace_id,scope.deployment_id,scope.instance_id,
                    next.generation,payload])?;
            transaction.commit()?;
            Ok(next)
        })
    }
}

fn apply_transition(
    record: &mut SolutionInstallation,
    transition: SolutionInstallationTransition<'_>,
) -> anyhow::Result<()> {
    let (id, attempt_id) = match &transition {
        SolutionInstallationTransition::Begin => return Ok(()),
        SolutionInstallationTransition::Claim {
            component_id,
            attempt_id,
        }
        | SolutionInstallationTransition::RecordStaged {
            component_id,
            attempt_id,
            ..
        } => (*component_id, *attempt_id),
    };
    ensure!(
        !attempt_id.is_empty()
            && attempt_id.len() <= 128
            && attempt_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte)),
        "invalid solution component attempt id"
    );
    let component = record
        .plan
        .components
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("component is not selected in this installation"))?;
    ensure!(
        component.depends_on.iter().all(|dependency| matches!(
            record.components.get(dependency),
            Some(SolutionComponentProgress::Staged { .. })
        )),
        "stage component dependencies first"
    );
    let progress = record
        .components
        .get_mut(id)
        .ok_or_else(|| anyhow::anyhow!("component progress is missing"))?;
    match transition {
        SolutionInstallationTransition::Begin => unreachable!(),
        SolutionInstallationTransition::Claim { .. } => {
            ensure!(
                matches!(progress, SolutionComponentProgress::Pending),
                "component is already claimed or staged; reconcile authoritative runtime state"
            );
            *progress = SolutionComponentProgress::Claimed {
                attempt_id: attempt_id.into(),
            };
        }
        SolutionInstallationTransition::RecordStaged {
            resource_sha256, ..
        } => {
            ensure!(
                resource_sha256.len() == 64
                    && resource_sha256
                        .bytes()
                        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
                "invalid staged resource fingerprint"
            );
            if !matches!(progress, SolutionComponentProgress::Claimed { attempt_id: claimed }
                if claimed == attempt_id)
            {
                bail!("staged component must match its durable claim");
            }
            *progress = SolutionComponentProgress::Staged {
                attempt_id: attempt_id.into(),
                resource_sha256: resource_sha256.into(),
            };
        }
    }
    Ok(())
}

pub(crate) const SCHEMA_V7: &str = "
CREATE TABLE solution_installations (
    org_id TEXT NOT NULL, workspace_id TEXT NOT NULL, deployment_id TEXT NOT NULL,
    instance_id TEXT NOT NULL, generation BIGINT NOT NULL CHECK (generation > 0), record_json TEXT NOT NULL,
    PRIMARY KEY (org_id,workspace_id,deployment_id,instance_id)
);
CREATE TABLE solution_installation_versions (
    org_id TEXT NOT NULL, workspace_id TEXT NOT NULL, deployment_id TEXT NOT NULL,
    instance_id TEXT NOT NULL, generation BIGINT NOT NULL CHECK (generation > 0), record_json TEXT NOT NULL,
    PRIMARY KEY (org_id,workspace_id,deployment_id,instance_id,generation)
);
UPDATE schema_metadata SET schema_version=7;
";

#[cfg(feature = "storage-sqlite")]
pub(super) fn migrate_sqlite(connection: &mut rusqlite::Connection) -> anyhow::Result<()> {
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    transaction.execute_batch(SCHEMA_V7)?;
    transaction.commit()?;
    Ok(())
}
