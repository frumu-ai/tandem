//! Draft lifecycle and version queries for orchestration definitions.
//!
//! Drafts occupy the mutable `version = 0` slot of `orchestration_specs`;
//! publishing snapshots the draft into the next immutable version (`1..N`)
//! through `put_orchestration`, which enforces published immutability. Draft
//! writes deliberately skip full graph validation — an authoring canvas must
//! be able to save incomplete graphs — but goals can only ever start from a
//! `Published` row, so invalid drafts never execute.

use anyhow::{bail, Context};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use tandem_automation::{OrchestrationSpec, OrchestrationStatus};
use tandem_types::TenantContext;

use super::OrchestrationStateStore;

/// The mutable draft slot; published versions start at 1.
pub const ORCHESTRATION_DRAFT_VERSION: u64 = 0;

/// Marker embedded in optimistic-concurrency failures so the HTTP layer can
/// map them to 409 instead of 500.
pub const DRAFT_CONCURRENCY_CONFLICT: &str = "orchestration draft was modified concurrently";
const ORCHESTRATION_TOOL_REQUEST_LEASE_MS: u64 = 30_000;

impl OrchestrationStateStore {
    pub fn begin_orchestration_action_request(
        &self,
        tenant: &TenantContext,
        operation: &str,
        idempotency_key: &str,
        request_digest: &str,
        now_ms: u64,
    ) -> anyhow::Result<Option<serde_json::Value>> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let deployment = tenant.deployment_id.as_deref().unwrap_or("");
            let existing = transaction
                .query_row(
                    "SELECT request_digest, response_json FROM orchestration_tool_requests
                     WHERE org_id = ?1 AND workspace_id = ?2 AND deployment_key = ?3
                       AND operation = ?4 AND idempotency_key = ?5",
                    params![
                        tenant.org_id,
                        tenant.workspace_id,
                        deployment,
                        operation,
                        idempotency_key,
                    ],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .optional()?;
            if let Some((stored_digest, response)) = existing {
                if stored_digest != request_digest {
                    bail!("idempotency key is already bound to a different {operation} request");
                }
                let Some(response) = response else {
                    bail!(
                        "the prior {operation} outcome is unknown; inspect authoritative state before issuing a new action"
                    );
                };
                transaction.commit()?;
                return Ok(Some(serde_json::from_str(&response)?));
            }
            transaction.execute(
                "INSERT INTO orchestration_tool_requests (
                    org_id, workspace_id, deployment_key, operation, idempotency_key,
                    request_digest, response_json, created_at_ms, completed_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, NULL)",
                params![
                    tenant.org_id,
                    tenant.workspace_id,
                    deployment,
                    operation,
                    idempotency_key,
                    request_digest,
                    now_ms,
                ],
            )?;
            transaction.commit()?;
            Ok(None)
        })
    }

    pub fn completed_orchestration_tool_request(
        &self,
        tenant: &TenantContext,
        operation: &str,
        idempotency_key: &str,
        request_digest: &str,
    ) -> anyhow::Result<Option<serde_json::Value>> {
        self.with_connection(|connection| {
            let existing = connection
                .query_row(
                    "SELECT request_digest, response_json FROM orchestration_tool_requests
                     WHERE org_id = ?1 AND workspace_id = ?2 AND deployment_key = ?3
                       AND operation = ?4 AND idempotency_key = ?5",
                    params![
                        tenant.org_id,
                        tenant.workspace_id,
                        tenant.deployment_id.as_deref().unwrap_or(""),
                        operation,
                        idempotency_key,
                    ],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
                )
                .optional()?;
            let Some((stored_digest, response)) = existing else {
                return Ok(None);
            };
            if stored_digest != request_digest {
                bail!("idempotency key is already bound to a different {operation} request");
            }
            response
                .map(|payload| serde_json::from_str(&payload).map_err(Into::into))
                .transpose()
        })
    }

    pub fn begin_orchestration_tool_request(
        &self,
        tenant: &TenantContext,
        operation: &str,
        idempotency_key: &str,
        request_digest: &str,
        now_ms: u64,
    ) -> anyhow::Result<Option<serde_json::Value>> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let deployment = tenant.deployment_id.as_deref().unwrap_or("");
            let existing = transaction
                .query_row(
                    "SELECT request_digest, response_json, created_at_ms
                     FROM orchestration_tool_requests
                     WHERE org_id = ?1 AND workspace_id = ?2 AND deployment_key = ?3
                       AND operation = ?4 AND idempotency_key = ?5",
                    params![
                        tenant.org_id,
                        tenant.workspace_id,
                        deployment,
                        operation,
                        idempotency_key,
                    ],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, u64>(2)?,
                        ))
                    },
                )
                .optional()?;
            if let Some((stored_digest, response, created_at_ms)) = existing {
                if stored_digest != request_digest {
                    bail!("idempotency key is already bound to a different {operation} request");
                }
                if response.is_none()
                    && now_ms.saturating_sub(created_at_ms) < ORCHESTRATION_TOOL_REQUEST_LEASE_MS
                {
                    bail!("{operation} request with this idempotency key is still in flight");
                }
                if response.is_none() {
                    transaction.execute(
                        "UPDATE orchestration_tool_requests SET created_at_ms = ?6
                         WHERE org_id = ?1 AND workspace_id = ?2 AND deployment_key = ?3
                           AND operation = ?4 AND idempotency_key = ?5",
                        params![
                            tenant.org_id,
                            tenant.workspace_id,
                            deployment,
                            operation,
                            idempotency_key,
                            now_ms,
                        ],
                    )?;
                }
                transaction.commit()?;
                return response
                    .map(|payload| serde_json::from_str(&payload).map_err(Into::into))
                    .transpose();
            }
            transaction.execute(
                "INSERT INTO orchestration_tool_requests (
                    org_id, workspace_id, deployment_key, operation, idempotency_key,
                    request_digest, response_json, created_at_ms, completed_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, NULL)",
                params![
                    tenant.org_id,
                    tenant.workspace_id,
                    deployment,
                    operation,
                    idempotency_key,
                    request_digest,
                    now_ms,
                ],
            )?;
            transaction.commit()?;
            Ok(None)
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn complete_orchestration_tool_request(
        &self,
        tenant: &TenantContext,
        operation: &str,
        idempotency_key: &str,
        request_digest: &str,
        response: &serde_json::Value,
        now_ms: u64,
    ) -> anyhow::Result<()> {
        self.with_connection(|connection| {
            let updated = connection.execute(
                "UPDATE orchestration_tool_requests
                 SET response_json = ?7, completed_at_ms = ?8
                 WHERE org_id = ?1 AND workspace_id = ?2 AND deployment_key = ?3
                   AND operation = ?4 AND idempotency_key = ?5 AND request_digest = ?6",
                params![
                    tenant.org_id,
                    tenant.workspace_id,
                    tenant.deployment_id.as_deref().unwrap_or(""),
                    operation,
                    idempotency_key,
                    request_digest,
                    serde_json::to_string(response)?,
                    now_ms,
                ],
            )?;
            if updated != 1 {
                bail!("orchestration tool request reservation was not found");
            }
            Ok(())
        })
    }

    /// Upsert the draft slot. `expected_updated_at_ms` is the optimistic
    /// concurrency token: when provided it must equal the stored draft's
    /// `updated_at_ms`, otherwise the write is rejected so a stale editor
    /// cannot silently overwrite newer work. `None` is only valid for the
    /// first write (creation).
    pub fn put_orchestration_draft(
        &self,
        spec: &OrchestrationSpec,
        expected_updated_at_ms: Option<u64>,
    ) -> anyhow::Result<()> {
        if spec.version != ORCHESTRATION_DRAFT_VERSION {
            bail!("orchestration drafts must use version {ORCHESTRATION_DRAFT_VERSION}");
        }
        if spec.status == OrchestrationStatus::Published {
            bail!("drafts cannot carry published status; publish creates a new version");
        }
        let payload = serde_json::to_string(spec)?;
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let existing = transaction
                .query_row(
                    "SELECT updated_at_ms FROM orchestration_specs
                     WHERE orchestration_id = ?1 AND version = ?2
                       AND org_id = ?3 AND workspace_id = ?4 AND deployment_key = ?5",
                    params![
                        spec.orchestration_id,
                        ORCHESTRATION_DRAFT_VERSION,
                        spec.tenant_context.org_id,
                        spec.tenant_context.workspace_id,
                        spec.tenant_context.deployment_id.as_deref().unwrap_or(""),
                    ],
                    |row| row.get::<_, u64>(0),
                )
                .optional()?;
            match (existing, expected_updated_at_ms) {
                (Some(stored), Some(expected)) if stored != expected => {
                    bail!(
                        "{DRAFT_CONCURRENCY_CONFLICT}: stored updated_at_ms {stored}, expected {expected}"
                    );
                }
                (Some(stored), None) => {
                    bail!(
                        "{DRAFT_CONCURRENCY_CONFLICT}: draft already exists (updated_at_ms {stored}); \
                         send expected_updated_at_ms to update it"
                    );
                }
                _ => {}
            }
            transaction.execute(
                "INSERT INTO orchestration_specs (
                    orchestration_id, version, org_id, workspace_id, deployment_id, deployment_key,
                    status, definition_json, created_at_ms, updated_at_ms, published_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL)
                 ON CONFLICT(org_id, workspace_id, deployment_key, orchestration_id, version)
                 DO UPDATE SET
                    status = excluded.status,
                    definition_json = excluded.definition_json,
                    updated_at_ms = excluded.updated_at_ms",
                params![
                    spec.orchestration_id,
                    ORCHESTRATION_DRAFT_VERSION,
                    spec.tenant_context.org_id,
                    spec.tenant_context.workspace_id,
                    spec.tenant_context.deployment_id,
                    spec.tenant_context.deployment_id.as_deref().unwrap_or(""),
                    serde_json::to_value(&spec.status)?
                        .as_str()
                        .unwrap_or("draft"),
                    payload,
                    spec.created_at_ms,
                    spec.updated_at_ms,
                ],
            )?;
            transaction.commit()?;
            Ok(())
        })
    }

    pub fn get_orchestration_draft(
        &self,
        tenant: &TenantContext,
        orchestration_id: &str,
    ) -> anyhow::Result<Option<OrchestrationSpec>> {
        self.get_orchestration_for_tenant(tenant, orchestration_id, ORCHESTRATION_DRAFT_VERSION)
    }

    /// Every stored row (drafts and published versions) visible to the tenant.
    pub fn list_orchestration_specs(
        &self,
        tenant: &TenantContext,
    ) -> anyhow::Result<Vec<OrchestrationSpec>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT definition_json FROM orchestration_specs
                 WHERE org_id = ?1 AND workspace_id = ?2
                   AND (deployment_id IS ?3 OR deployment_id = ?3)
                 ORDER BY orchestration_id, version",
            )?;
            let rows = statement.query_map(
                params![tenant.org_id, tenant.workspace_id, tenant.deployment_id],
                |row| row.get::<_, String>(0),
            )?;
            let mut specs = Vec::new();
            for row in rows {
                specs.push(serde_json::from_str(&row?)?);
            }
            Ok(specs)
        })
    }

    pub fn list_orchestration_versions(
        &self,
        tenant: &TenantContext,
        orchestration_id: &str,
    ) -> anyhow::Result<Vec<OrchestrationSpec>> {
        Ok(self
            .list_orchestration_specs(tenant)?
            .into_iter()
            .filter(|spec| {
                spec.orchestration_id == orchestration_id
                    && spec.version != ORCHESTRATION_DRAFT_VERSION
            })
            .collect())
    }

    pub fn latest_published_orchestration_version(
        &self,
        tenant: &TenantContext,
        orchestration_id: &str,
    ) -> anyhow::Result<Option<u64>> {
        self.with_connection(|connection| {
            let version = connection
                .query_row(
                    "SELECT MAX(version) FROM orchestration_specs
                     WHERE orchestration_id = ?1 AND status = 'published'
                       AND org_id = ?2 AND workspace_id = ?3 AND deployment_key = ?4",
                    params![
                        orchestration_id,
                        tenant.org_id,
                        tenant.workspace_id,
                        tenant.deployment_id.as_deref().unwrap_or(""),
                    ],
                    |row| row.get::<_, Option<u64>>(0),
                )
                .optional()?
                .flatten();
            Ok(version)
        })
    }

    /// Publish the draft as the next immutable version. The caller has already
    /// validated the graph and refreshed referenced definition hashes; this
    /// method only guards the version sequence inside one transaction so two
    /// concurrent publishes cannot both claim the same version number.
    pub fn publish_orchestration_draft(
        &self,
        published: &OrchestrationSpec,
        expected_draft_updated_at_ms: Option<u64>,
    ) -> anyhow::Result<()> {
        if published.status != OrchestrationStatus::Published {
            bail!("publishing requires a spec with published status");
        }
        if published.version == ORCHESTRATION_DRAFT_VERSION {
            bail!("published versions must be greater than the draft slot");
        }
        let payload = serde_json::to_string(published)?;
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if let Some(expected) = expected_draft_updated_at_ms {
                let stored: Option<u64> = transaction
                    .query_row(
                        "SELECT updated_at_ms FROM orchestration_specs
                         WHERE orchestration_id = ?1 AND version = ?2
                           AND org_id = ?3 AND workspace_id = ?4 AND deployment_key = ?5",
                        params![
                            published.orchestration_id,
                            ORCHESTRATION_DRAFT_VERSION,
                            published.tenant_context.org_id,
                            published.tenant_context.workspace_id,
                            published
                                .tenant_context
                                .deployment_id
                                .as_deref()
                                .unwrap_or(""),
                        ],
                        |row| row.get(0),
                    )
                    .optional()?;
                if stored != Some(expected) {
                    bail!(
                        "{DRAFT_CONCURRENCY_CONFLICT}: stored updated_at_ms {:?}, expected {expected}",
                        stored
                    );
                }
            }
            let latest: Option<u64> = transaction
                .query_row(
                    "SELECT MAX(version) FROM orchestration_specs
                     WHERE orchestration_id = ?1 AND status = 'published'
                       AND org_id = ?2 AND workspace_id = ?3 AND deployment_key = ?4",
                    params![
                        published.orchestration_id,
                        published.tenant_context.org_id,
                        published.tenant_context.workspace_id,
                        published
                            .tenant_context
                            .deployment_id
                            .as_deref()
                            .unwrap_or(""),
                    ],
                    |row| row.get::<_, Option<u64>>(0),
                )
                .optional()?
                .flatten();
            let expected = latest.unwrap_or(0).saturating_add(1);
            if published.version != expected {
                bail!(
                    "orchestration {} publish raced: expected version {expected}, got {}",
                    published.orchestration_id,
                    published.version
                );
            }
            transaction.execute(
                "INSERT INTO orchestration_specs (
                    orchestration_id, version, org_id, workspace_id, deployment_id, deployment_key,
                    status, definition_json, created_at_ms, updated_at_ms, published_at_ms
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'published', ?7, ?8, ?9, ?10)",
                params![
                    published.orchestration_id,
                    published.version,
                    published.tenant_context.org_id,
                    published.tenant_context.workspace_id,
                    published.tenant_context.deployment_id,
                    published
                        .tenant_context
                        .deployment_id
                        .as_deref()
                        .unwrap_or(""),
                    payload,
                    published.created_at_ms,
                    published.updated_at_ms,
                    published.published_at_ms,
                ],
            )?;
            transaction.commit()?;
            Ok(())
        })
    }

    /// Archive the draft slot. Published versions are never archived here —
    /// they stay immutable so active goals keep their original snapshot.
    pub fn archive_orchestration_draft(
        &self,
        tenant: &TenantContext,
        orchestration_id: &str,
        expected_updated_at_ms: Option<u64>,
        now_ms: u64,
    ) -> anyhow::Result<OrchestrationSpec> {
        let mut draft = self
            .get_orchestration_draft(tenant, orchestration_id)?
            .context("orchestration draft not found")?;
        let same_scope = draft.tenant_context.org_id == tenant.org_id
            && draft.tenant_context.workspace_id == tenant.workspace_id
            && draft.tenant_context.deployment_id == tenant.deployment_id;
        if !same_scope {
            bail!("orchestration is outside the caller tenant scope");
        }
        if expected_updated_at_ms.is_some_and(|expected| expected != draft.updated_at_ms) {
            bail!(
                "{DRAFT_CONCURRENCY_CONFLICT}: stored updated_at_ms {}, expected {}",
                draft.updated_at_ms,
                expected_updated_at_ms.unwrap_or_default()
            );
        }
        draft.status = OrchestrationStatus::Archived;
        let expected = draft.updated_at_ms;
        draft.updated_at_ms = now_ms;
        self.put_orchestration_draft(&draft, Some(expected))?;
        Ok(draft)
    }
}
