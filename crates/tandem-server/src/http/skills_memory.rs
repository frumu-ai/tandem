// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::context_runs::context_run_engine;
use super::memory_audit_store::{append_memory_audit, load_memory_audit_events};
use super::*;
use crate::http::{SkillLocation, SkillsConflictPolicy};
use crate::{
    WorkflowLearningCandidate, WorkflowLearningCandidateKind,
    WorkflowLearningCandidateSourceBinding, WorkflowLearningCandidateStatus,
};
use tandem_memory::import_files;
use tandem_memory::types::{
    MemoryAccessFilter, MemoryImportFormat, MemoryImportProgress,
    MemoryImportRequest as TandemMemoryImportRequest, MemoryImportSourceBinding, MemoryImportStats,
    MemorySourceAccessTarget, MemoryTenantScope, MemoryTier, SourceObjectLifecycleRecord,
    SourceObjectLifecycleState,
};
use tandem_types::{
    AccessPermission, ConnectorLifecycleState, IngestionJob, IngestionJobState,
    IngestionQuarantine, RequestPrincipal, VerifiedTenantContext,
};

async fn workflow_learning_candidate_access(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    candidate: &WorkflowLearningCandidate,
    mutation: bool,
) -> bool {
    if tenant.is_local_implicit() {
        return true;
    }
    let Some(verified) = verified else {
        return false;
    };
    match candidate.source_binding.as_ref() {
        Some(binding @ WorkflowLearningCandidateSourceBinding::Workflow { created_at_ms, .. })
            if *created_at_ms > 0 =>
        {
            let Some(source) = state.get_automation_v2(&candidate.workflow_id).await else {
                return false;
            };
            if binding != &WorkflowLearningCandidateSourceBinding::workflow(&source) {
                return false;
            }
            if mutation {
                super::automation_object_authority::can_write(
                    state,
                    tenant,
                    Some(verified),
                    &source,
                )
            } else {
                super::automation_object_authority::can_read(state, tenant, Some(verified), &source)
            }
        }
        Some(WorkflowLearningCandidateSourceBinding::Session {
            tenant_context,
            actor_id,
            subject,
            session_id,
        }) => {
            if actor_id.is_empty()
                || subject.is_empty()
                || session_id.is_empty()
                || candidate.kind != WorkflowLearningCandidateKind::MemoryFact
                || candidate.workflow_id != format!("session:{session_id}")
                || !super::tenant_matches(tenant, tenant_context)
                || !super::tenant_matches(tenant, &verified.tenant_context)
                || tenant_context.actor_id.as_deref() != Some(actor_id)
                || tenant.actor_id.as_deref() != Some(actor_id)
                || verified.human_actor.actor_id.as_str() != actor_id.as_str()
                || verified.is_expired_at(crate::now_ms())
                || crate::memory::subject::request_memory_subject(tenant, Some(verified), None)
                    .map_or(true, |resolved| resolved.subject != subject.as_str())
            {
                return false;
            }
            state
                .enterprise
                .hosted_policy
                .authorize_permission(
                    Some(verified),
                    if mutation {
                        AccessPermission::HostedAutomationWrite
                    } else {
                        AccessPermission::HostedAutomationRead
                    },
                )
                .is_ok()
        }
        _ => false,
    }
}

pub(super) async fn workflow_learning_distillation_source_binding(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    workflow_id: Option<&str>,
    session_id: &str,
    subject: &str,
) -> Result<WorkflowLearningCandidateSourceBinding, StatusCode> {
    let session_id = session_id.trim();
    if session_id.is_empty() || subject.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if let Some(workflow_id) = workflow_id {
        let source = state.get_automation_v2(workflow_id).await;
        match source {
            Some(source)
                if source.created_at_ms > 0
                    && (tenant.is_local_implicit()
                        || super::automation_object_authority::can_write(
                            state, tenant, verified, &source,
                        )) =>
            {
                return Ok(WorkflowLearningCandidateSourceBinding::workflow(&source));
            }
            _ if !tenant.is_local_implicit() => return Err(StatusCode::NOT_FOUND),
            _ => {}
        }
    }
    let (tenant_context, actor_id) = if tenant.is_local_implicit() {
        (
            tenant.clone(),
            tenant
                .actor_id
                .clone()
                .unwrap_or_else(|| "local".to_string()),
        )
    } else {
        let verified = verified.ok_or(StatusCode::FORBIDDEN)?;
        if verified.is_expired_at(crate::now_ms())
            || !super::tenant_matches(tenant, &verified.tenant_context)
            || tenant.actor_id.as_deref() != Some(verified.human_actor.actor_id.as_str())
            || state
                .enterprise
                .hosted_policy
                .authorize(Some(verified))
                .is_err()
        {
            return Err(StatusCode::FORBIDDEN);
        }
        (
            verified.tenant_context.clone(),
            verified.human_actor.actor_id.clone(),
        )
    };
    Ok(WorkflowLearningCandidateSourceBinding::Session {
        tenant_context,
        actor_id,
        subject: subject.to_string(),
        session_id: session_id.to_string(),
    })
}

include!("skills_memory_parts/part01.rs");
include!("skills_memory_parts/part06.rs");
include!("skills_memory_parts/part02.rs");
include!("skills_memory_parts/part04.rs");
include!("skills_memory_parts/part03.rs");
include!("skills_memory_parts/part05.rs");
include!("skills_memory_parts/part07.rs");

impl GovernedDistillationWriter {
    async fn source_binding(
        &self,
        session_id: &str,
    ) -> tandem_memory::types::MemoryResult<WorkflowLearningCandidateSourceBinding> {
        workflow_learning_distillation_source_binding(
            &self.state,
            &self.tenant_context,
            self.verified_tenant_context.as_ref(),
            self.workflow_id.as_deref(),
            session_id,
            &self.subject,
        )
        .await
        .map_err(|status| {
            tandem_memory::types::MemoryError::InvalidConfig(format!(
                "workflow learning candidate source access denied: {status}"
            ))
        })
    }
}
