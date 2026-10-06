// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use crate::AppState;
use tandem_memory::derived_lineage::{
    resolve_derived_lineage, CanonicalInputReference, DerivedMemoryLineage,
};
use tandem_memory::types::{GlobalMemoryRecord, MemoryAccessFilter};
use tandem_memory::{MemoryReadScope, MemoryStore};
use tandem_types::{canonical_message_digest, MessageRole, TenantContext};

pub fn lineage_resolver(
    state: AppState,
    tenant: TenantContext,
    verified: Option<tandem_types::VerifiedTenantContext>,
) -> tandem_memory::DerivedMemoryAccessResolver {
    std::sync::Arc::new(move |store, scope, lineage, filter| {
        let state = state.clone();
        let tenant = tenant.clone();
        let verified = verified.clone();
        Box::pin(async move {
            if state
                .enterprise
                .hosted_policy
                .authorize(verified.as_ref())
                .is_err()
            {
                return None;
            }
            let future = resolved_filter_for_lineage(
                &state,
                &tenant,
                store.as_ref(),
                &scope,
                &lineage,
                filter,
            );
            match verified.as_ref().and_then(|context| {
                super::decrypt_principal::memory_decrypt_principal_from_verified_context(
                    context,
                    crate::now_ms(),
                )
            }) {
                Some(principal) => {
                    tandem_memory::decrypt_context::with_decrypt_principal(principal, future).await
                }
                None => future.await,
            }
        })
    })
}

pub fn candidate_lineage(
    candidate: &crate::WorkflowLearningCandidate,
) -> tandem_memory::types::MemoryResult<Option<DerivedMemoryLineage>> {
    DerivedMemoryLineage::from_metadata(
        candidate
            .proposed_memory_payload
            .as_ref()
            .and_then(|payload| payload.get("metadata")),
    )
}

pub async fn candidate_lineage_readable(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&tandem_types::VerifiedTenantContext>,
    candidate: &crate::WorkflowLearningCandidate,
) -> bool {
    let lineage = match candidate_lineage(candidate) {
        Ok(Some(lineage)) => lineage,
        Ok(None) => return true,
        Err(_) => return false,
    };
    if state.enterprise.hosted_policy.authorize(verified).is_err() {
        return false;
    }
    let Ok(subject) = super::subject::request_memory_subject(tenant, verified, None) else {
        return false;
    };
    let Ok(store) = state.memory_store().await else {
        return false;
    };
    let mut scope = MemoryReadScope::tenant(tandem_memory::types::MemoryTenantScope {
        org_id: tenant.org_id.clone(),
        workspace_id: tenant.workspace_id.clone(),
        deployment_id: tenant.deployment_id.clone(),
    });
    scope.subject = Some(subject.subject.clone());
    scope.org_unit = super::subject::active_org_unit(verified);
    let filter = super::read_policy::governed_memory_read_filter(
        crate::config::env::resolve_runtime_auth_mode(),
        verified,
        false,
        crate::now_ms(),
    )
    .unwrap_or_else(|| MemoryAccessFilter::local_noop(crate::now_ms()))
    .with_caller_subject(subject.subject);
    let future =
        resolved_filter_for_lineage(state, tenant, store.as_ref(), &scope, &lineage, filter);
    match verified.and_then(|context| {
        super::decrypt_principal::memory_decrypt_principal_from_verified_context(
            context,
            crate::now_ms(),
        )
    }) {
        Some(principal) => {
            tandem_memory::decrypt_context::with_decrypt_principal(principal, future)
                .await
                .is_some()
        }
        None => future.await.is_some(),
    }
}

/// Native session evidence lives outside the memory store. Recheck it before
/// issuing an ephemeral proof, including evidence inherited through a memory
/// source. Persisted hashes alone cannot prove a source still exists.
pub async fn resolved_filter_for_lineage(
    state: &AppState,
    request_tenant: &TenantContext,
    store: &dyn MemoryStore,
    scope: &MemoryReadScope,
    lineage: &DerivedMemoryLineage,
    filter: MemoryAccessFilter,
) -> Option<MemoryAccessFilter> {
    let input_refs = lineage.all_input_refs().ok()?;
    for input in input_refs {
        let CanonicalInputReference::SessionMessage {
            session_id,
            message_id,
            body_digest,
        } = input
        else {
            continue;
        };
        let session = state.storage.get_session(&session_id).await?;
        let tenant = &session.tenant_context;
        if tenant.org_id != scope.tenant.org_id
            || tenant.workspace_id != scope.tenant.workspace_id
            || tenant.deployment_id != scope.tenant.deployment_id
            || (!tenant.is_local_implicit()
                && (tenant.actor_id.is_none() || tenant.actor_id != request_tenant.actor_id))
        {
            return None;
        }
        let message = session
            .messages
            .iter()
            .find(|message| message.id == message_id)?;
        if canonical_message_digest(message) != body_digest {
            return None;
        }
        match &message.role {
            MessageRole::User => {}
            MessageRole::Assistant => {
                let source = message.source_lineage.as_ref()?;
                if source.schema_version != 1
                    || !source.complete
                    || source.message_digest != body_digest
                    || source.tenant_context.org_id != tenant.org_id
                    || source.tenant_context.workspace_id != tenant.workspace_id
                    || source.tenant_context.deployment_id != tenant.deployment_id
                    || source.tenant_context.actor_id != tenant.actor_id
                    || scope.subject.as_deref() != Some(source.subject.as_str())
                {
                    return None;
                }
            }
            _ => return None,
        }
    }
    let proof = resolve_derived_lineage(store, scope, lineage).await.ok()?;
    let filter = filter.with_resolved_derived_lineage(proof);
    filter
        .decision_for_derived_lineage(lineage)
        .allowed
        .then_some(filter)
}

pub async fn resolved_filter_for_record(
    state: &AppState,
    request_tenant: &TenantContext,
    store: &dyn MemoryStore,
    scope: &MemoryReadScope,
    record: &GlobalMemoryRecord,
    filter: MemoryAccessFilter,
) -> Option<MemoryAccessFilter> {
    match DerivedMemoryLineage::from_metadata(record.metadata.as_ref()).ok()? {
        Some(lineage) => {
            resolved_filter_for_lineage(state, request_tenant, store, scope, &lineage, filter).await
        }
        None => Some(filter),
    }
}
