//! Protected budget records share the orchestration writer transaction and
//! transfer machinery. Version history detects deleting/rolling back only a
//! current row; off-host rollback detection still requires recovery anchors.

use anyhow::ensure;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tandem_solutions::{canonical_json, sha256, CustomerScope};
use tandem_types::TenantContext;

use super::protected_records;
use crate::stateful_runtime::backend::{params, Executor, OptionalExtension};

#[derive(Serialize, Deserialize)]
struct Record<T> {
    instance_id: String,
    key: String,
    generation: u64,
    value: T,
}

fn id(scope: &CustomerScope, key: &str, generation: u64) -> anyhow::Result<String> {
    Ok(format!(
        "{}:{generation}",
        sha256(&canonical_json(&(&scope.instance_id, key))?)
    ))
}

pub(super) fn load<T: DeserializeOwned>(
    executor: &impl Executor,
    tenant: &TenantContext,
    scope: &CustomerScope,
    key: &str,
) -> anyhow::Result<Option<(u64, T)>> {
    let row = executor.query_row(
        "SELECT generation,record_json FROM solution_budget_records
         WHERE org_id=?1 AND workspace_id=?2 AND deployment_id=?3 AND instance_id=?4 AND record_key=?5",
        params![scope.org_id,scope.workspace_id,scope.deployment_id,scope.instance_id,key],
        |row| Ok((row.get::<_, u64>(0)?, row.get::<_, String>(1)?)),
    ).optional()?;
    let maximum: Option<u64> = executor.query_row(
        "SELECT MAX(generation) FROM solution_budget_versions
         WHERE org_id=?1 AND workspace_id=?2 AND deployment_id=?3 AND instance_id=?4 AND record_key=?5",
        params![scope.org_id,scope.workspace_id,scope.deployment_id,scope.instance_id,key],
        |row| row.get(0),
    )?;
    ensure!(
        row.as_ref().map(|(generation, _)| *generation) == maximum,
        "solution budget current record missing or rolled back"
    );
    let Some((generation, raw)) = row else {
        return Ok(None);
    };
    let record: Record<T> = protected_records::decode(
        tenant,
        "solution-budget",
        &id(scope, key, generation)?,
        &raw,
    )?;
    ensure!(
        record.instance_id == scope.instance_id
            && record.key == key
            && record.generation == generation,
        "solution budget persisted binding mismatch"
    );
    Ok(Some((generation, record.value)))
}

pub(super) fn save<T: Serialize>(
    executor: &impl Executor,
    tenant: &TenantContext,
    scope: &CustomerScope,
    key: &str,
    previous: u64,
    value: T,
) -> anyhow::Result<()> {
    let generation = previous
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or_else(|| anyhow::anyhow!("solution budget generation exhausted"))?;
    let raw = protected_records::encode(
        tenant,
        "solution-budget",
        &id(scope, key, generation)?,
        &Record {
            instance_id: scope.instance_id.clone(),
            key: key.into(),
            generation,
            value,
        },
    )?;
    let changed = if previous == 0 {
        executor.execute(
            "INSERT INTO solution_budget_records
            (org_id,workspace_id,deployment_id,instance_id,record_key,generation,record_json)
            VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT DO NOTHING",
            params![
                scope.org_id,
                scope.workspace_id,
                scope.deployment_id,
                scope.instance_id,
                key,
                generation,
                raw
            ],
        )?
    } else {
        executor.execute("UPDATE solution_budget_records SET generation=?6,record_json=?7
            WHERE org_id=?1 AND workspace_id=?2 AND deployment_id=?3 AND instance_id=?4 AND record_key=?5 AND generation=?8",
            params![scope.org_id,scope.workspace_id,scope.deployment_id,scope.instance_id,key,generation,raw,previous])?
    };
    ensure!(changed == 1, "solution budget changed concurrently");
    executor.execute(
        "INSERT INTO solution_budget_versions
        (org_id,workspace_id,deployment_id,instance_id,record_key,generation,record_json)
        VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            scope.org_id,
            scope.workspace_id,
            scope.deployment_id,
            scope.instance_id,
            key,
            generation,
            raw
        ],
    )?;
    Ok(())
}

pub(crate) const SCHEMA_V8: &str = "
CREATE TABLE solution_budget_records (
 org_id TEXT NOT NULL,workspace_id TEXT NOT NULL,deployment_id TEXT NOT NULL,instance_id TEXT NOT NULL,
 record_key TEXT NOT NULL,generation BIGINT NOT NULL CHECK (generation>0),record_json TEXT NOT NULL,
 PRIMARY KEY (org_id,workspace_id,deployment_id,instance_id,record_key)
);
CREATE TABLE solution_budget_versions (
 org_id TEXT NOT NULL,workspace_id TEXT NOT NULL,deployment_id TEXT NOT NULL,instance_id TEXT NOT NULL,
 record_key TEXT NOT NULL,generation BIGINT NOT NULL CHECK (generation>0),record_json TEXT NOT NULL,
 PRIMARY KEY (org_id,workspace_id,deployment_id,instance_id,record_key,generation)
);
UPDATE schema_metadata SET schema_version=8;
";

#[cfg(feature = "storage-sqlite")]
pub(super) fn migrate_sqlite(connection: &mut rusqlite::Connection) -> anyhow::Result<()> {
    let transaction =
        connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    transaction.execute_batch(SCHEMA_V8)?;
    transaction.commit()?;
    Ok(())
}
