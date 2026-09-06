// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Native routine staging for an already authorized installation. Ownership
//! metadata is not authority: callers must revalidate the journal, current
//! policy, signed artifact and host bindings before materializing this resource.

use std::collections::HashMap;

use serde::Deserialize;
use tandem_solutions::{canonical_json, sha256, MAX_ARTIFACT_BYTES};

use super::{normalize_routine, routine_store_index, AppState};
use crate::routines::errors::RoutineStoreError;
use crate::routines::types::{
    solution_routine_id, RoutineIdentity, RoutineMisfirePolicy, RoutineSchedule, RoutineSpec,
    RoutineStatus, SolutionRoutineOwner,
};
use tandem_types::TenantContext;

fn managed(message: impl ToString) -> RoutineStoreError {
    RoutineStoreError::ManagedResource {
        message: message.to_string(),
    }
}

pub(super) fn require_unmanaged(routine: &RoutineSpec) -> Result<(), RoutineStoreError> {
    if solution_routine_id(&routine.routine_id) || routine.solution_owner.is_some() {
        return Err(managed(
            "solution routines require the installation lifecycle",
        ));
    }
    Ok(())
}

fn bytes(value: &impl serde::Serialize) -> Result<Vec<u8>, RoutineStoreError> {
    canonical_json(value).map_err(managed)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoutineArtifact {
    name: String,
    status: RoutineStatus,
    schedule: RoutineSchedule,
    timezone: String,
    misfire_policy: String,
    entrypoint: String,
    args: serde_json::Value,
    allowed_tools: Vec<String>,
    output_targets: Vec<String>,
    requires_approval: bool,
    external_integrations_allowed: bool,
}

/// Decode the current text solution's reusable routine artifact. It supplies
/// neither tenant/creator IDs nor installation ownership. Artifact verification
/// remains the caller's responsibility (use the signed PackManager snapshot).
pub fn solution_routine_from_artifact(
    artifact: &[u8],
    resource_id: &str,
    tenant: &TenantContext,
) -> Result<RoutineSpec, RoutineStoreError> {
    if artifact.len() > MAX_ARTIFACT_BYTES {
        return Err(managed("routine artifact is too large"));
    }
    let input: RoutineArtifact = serde_json::from_slice(artifact).map_err(managed)?;
    if input.status != RoutineStatus::Paused || input.misfire_policy != "skip" {
        return Err(managed(
            "text solution routine requires paused status and skip misfire policy",
        ));
    }
    Ok(RoutineSpec {
        solution_owner: None,
        routine_id: resource_id.to_string(),
        tenant_context: tenant.clone(),
        name: input.name,
        status: RoutineStatus::Paused,
        schedule: input.schedule,
        timezone: input.timezone,
        misfire_policy: RoutineMisfirePolicy::Skip,
        entrypoint: input.entrypoint,
        args: input.args,
        allowed_tools: input.allowed_tools,
        output_targets: input.output_targets,
        creator_type: "solution".into(),
        creator_id: String::new(),
        requires_approval: input.requires_approval,
        external_integrations_allowed: input.external_integrations_allowed,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
    })
}

impl AppState {
    /// Stage an actual tenant-scoped native routine, returning its observed
    /// fingerprint for the installation journal. This never activates it.
    pub async fn stage_solution_routine(
        &self,
        mut routine: RoutineSpec,
        mut owner: SolutionRoutineOwner,
    ) -> Result<String, RoutineStoreError> {
        if !routine.routine_id.starts_with("solution-")
            || routine.routine_id.trim() != routine.routine_id
            || routine.routine_id.len() > 256
            || routine.routine_id.chars().any(char::is_control)
            || routine.solution_owner.is_some()
        {
            return Err(managed(
                "routine artifact requires an unowned stable solution ID",
            ));
        }
        for reference in [
            &owner.instance_id,
            &owner.component_id,
            &routine.tenant_context.org_id,
            &routine.tenant_context.workspace_id,
        ] {
            if reference.is_empty()
                || reference.len() > 256
                || reference.trim() != reference
                || reference.chars().any(char::is_control)
            {
                return Err(managed("solution routine requires complete ownership"));
            }
        }
        if routine
            .tenant_context
            .deployment_id
            .as_deref()
            .is_none_or(str::is_empty)
            || owner.composition_sha256.len() != 64
            || !owner
                .composition_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
        {
            return Err(managed(
                "solution routine requires deployment and reviewed composition",
            ));
        }
        owner.enabled = false;
        routine.creator_type = "solution".into();
        routine.creator_id = owner.instance_id.clone();
        routine.solution_owner = Some(owner);
        routine.status = RoutineStatus::Paused;
        routine.last_fired_at_ms = None;
        routine = normalize_routine(routine)?;
        // Activation computes a schedule from the activation time. Staging
        // receipts must not change simply because a retry happens later.
        routine.next_fire_at_ms = None;
        let payload = bytes(&routine)?;
        if payload.len() > MAX_ARTIFACT_BYTES {
            return Err(managed("solution routine is too large"));
        }
        let fingerprint = sha256(&payload);
        let identity = RoutineIdentity::new(&routine.routine_id, &routine.tenant_context);
        let key = identity.storage_key();
        let _operation = self.routine_persistence.lock().await;
        // The existing native routine store has one AppState writer per host.
        // Detect external edits or stale recovery snapshots instead of silently
        // replacing them with this process's cached map.
        let persisted = match tokio::fs::read(&self.routines_path).await {
            Ok(raw) => {
                let rows: HashMap<String, RoutineSpec> =
                    serde_json::from_slice(&raw).map_err(managed)?;
                let count = rows.len();
                let indexed = routine_store_index(rows);
                if indexed.len() != count {
                    return Err(managed(
                        "routine store has duplicate identities; reconcile before staging",
                    ));
                }
                indexed
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(error) => return Err(managed(error)),
        };
        let cached = self.routines.read().await.clone();
        if bytes(&persisted)? != bytes(&cached)? {
            return Err(managed(
                "routine store changed outside this writer; reload and reconcile",
            ));
        }
        if let Some(existing) = cached.get(&key) {
            if bytes(existing)? != payload {
                return Err(managed("solution routine ownership or content conflict"));
            }
            return Ok(fingerprint);
        }
        self.routines.write().await.insert(key.clone(), routine);
        if let Err(error) = self.persist_routines_inner(false).await {
            self.routines.write().await.remove(&key);
            return Err(RoutineStoreError::PersistFailed {
                message: error.to_string(),
            });
        }
        Ok(fingerprint)
    }
}

#[cfg(test)]
mod tests;
