use std::collections::HashMap;

use serde_json::json;
use serde_json::Value;
use tokio::fs;
use uuid::Uuid;

use crate::audit::append_protected_audit_event;
use crate::automation_v2::governance::*;
use crate::{now_ms, AppState};

const GOVERNANCE_AUDIT_EVENT_PREFIX: &str = "automation.governance";

fn default_human_provenance(
    creator_id: Option<String>,
    source: impl Into<String>,
) -> AutomationProvenanceRecord {
    AutomationProvenanceRecord::human(creator_id, source)
}

fn declared_capabilities_for_automation(
    automation: &crate::AutomationV2Spec,
) -> AutomationDeclaredCapabilities {
    AutomationDeclaredCapabilities::from_metadata(automation.metadata.as_ref())
}

impl AppState {
    pub async fn load_automation_governance(&self) -> anyhow::Result<()> {
        if !self.automation_governance_path.exists() {
            return Ok(());
        }
        let raw = fs::read_to_string(&self.automation_governance_path).await?;
        let parsed = serde_json::from_str::<GovernanceState>(&raw).unwrap_or_default();
        *self.automation_governance.write().await = parsed;
        Ok(())
    }

    pub async fn persist_automation_governance(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.automation_governance_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let payload = {
            let guard = self.automation_governance.read().await;
            serde_json::to_string_pretty(&*guard)?
        };
        fs::write(&self.automation_governance_path, payload).await?;
        Ok(())
    }

    async fn persist_automation_governance_locked(&self) -> anyhow::Result<()> {
        self.persist_automation_governance().await
    }

    pub async fn bootstrap_automation_governance(&self) -> anyhow::Result<usize> {
        let automations = self.list_automations_v2().await;
        let now = now_ms();
        let mut inserted = 0usize;
        {
            let mut guard = self.automation_governance.write().await;
            for automation in automations {
                if guard.records.contains_key(&automation.automation_id) {
                    continue;
                }
                guard.records.insert(
                    automation.automation_id.clone(),
                    AutomationGovernanceRecord {
                        automation_id: automation.automation_id.clone(),
                        provenance: default_human_provenance(
                            Some(automation.creator_id.clone()),
                            "migration_or_legacy_default",
                        ),
                        declared_capabilities: declared_capabilities_for_automation(&automation),
                        modify_grants: Vec::new(),
                        capability_grants: Vec::new(),
                        created_at_ms: automation.created_at_ms.max(now),
                        updated_at_ms: now,
                        deleted_at_ms: None,
                        delete_retention_until_ms: None,
                        published_externally: false,
                        creation_paused: false,
                        review_required: false,
                        review_kind: None,
                        review_requested_at_ms: None,
                        review_request_id: None,
                        last_reviewed_at_ms: None,
                        runs_since_review: 0,
                        expires_at_ms: None,
                        expired_at_ms: None,
                        retired_at_ms: None,
                        retire_reason: None,
                        paused_for_lifecycle: false,
                        health_last_checked_at_ms: None,
                        health_findings: Vec::new(),
                    },
                );
                inserted += 1;
            }
            guard.updated_at_ms = now;
        }
        if inserted > 0 {
            self.persist_automation_governance().await?;
        }
        Ok(inserted)
    }

    pub async fn get_automation_governance(
        &self,
        automation_id: &str,
    ) -> Option<AutomationGovernanceRecord> {
        self.automation_governance
            .read()
            .await
            .records
            .get(automation_id)
            .cloned()
    }

    pub async fn get_or_bootstrap_automation_governance(
        &self,
        automation: &crate::AutomationV2Spec,
    ) -> AutomationGovernanceRecord {
        if let Some(record) = self
            .get_automation_governance(&automation.automation_id)
            .await
        {
            return record;
        }
        let record = AutomationGovernanceRecord {
            automation_id: automation.automation_id.clone(),
            provenance: default_human_provenance(
                Some(automation.creator_id.clone()),
                "legacy_default",
            ),
            declared_capabilities: declared_capabilities_for_automation(automation),
            modify_grants: Vec::new(),
            capability_grants: Vec::new(),
            created_at_ms: automation.created_at_ms,
            updated_at_ms: now_ms(),
            deleted_at_ms: None,
            delete_retention_until_ms: None,
            published_externally: false,
            creation_paused: false,
            review_required: false,
            review_kind: None,
            review_requested_at_ms: None,
            review_request_id: None,
            last_reviewed_at_ms: None,
            runs_since_review: 0,
            expires_at_ms: None,
            expired_at_ms: None,
            retired_at_ms: None,
            retire_reason: None,
            paused_for_lifecycle: false,
            health_last_checked_at_ms: None,
            health_findings: Vec::new(),
        };
        let _ = self.upsert_automation_governance(record.clone()).await;
        record
    }

    pub async fn upsert_automation_governance(
        &self,
        mut record: AutomationGovernanceRecord,
    ) -> anyhow::Result<AutomationGovernanceRecord> {
        if record.automation_id.trim().is_empty() {
            anyhow::bail!("automation_id is required");
        }
        let now = now_ms();
        if record.created_at_ms == 0 {
            record.created_at_ms = now;
        }
        record.updated_at_ms = now;
        {
            let mut guard = self.automation_governance.write().await;
            guard
                .records
                .insert(record.automation_id.clone(), record.clone());
            guard.updated_at_ms = now;
        }
        self.persist_automation_governance().await?;
        let _ = append_protected_audit_event(
            self,
            format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.record.updated"),
            &tandem_types::TenantContext::local_implicit(),
            record
                .provenance
                .creator
                .actor_id
                .clone()
                .or_else(|| record.provenance.creator.source.clone()),
            json!({
                "automationID": record.automation_id,
                "provenance": record.provenance,
                "declaredCapabilities": record.declared_capabilities,
                "publishedExternally": record.published_externally,
                "creationPaused": record.creation_paused,
            }),
        )
        .await;
        Ok(record)
    }

    pub async fn set_automation_governance_provenance(
        &self,
        automation_id: &str,
        provenance: AutomationProvenanceRecord,
    ) -> anyhow::Result<AutomationGovernanceRecord> {
        let mut record = self
            .get_automation_governance(automation_id)
            .await
            .unwrap_or_else(|| AutomationGovernanceRecord {
                automation_id: automation_id.to_string(),
                provenance: provenance.clone(),
                declared_capabilities: AutomationDeclaredCapabilities::default(),
                modify_grants: Vec::new(),
                capability_grants: Vec::new(),
                created_at_ms: now_ms(),
                updated_at_ms: now_ms(),
                deleted_at_ms: None,
                delete_retention_until_ms: None,
                published_externally: false,
                creation_paused: false,
                review_required: false,
                review_kind: None,
                review_requested_at_ms: None,
                review_request_id: None,
                last_reviewed_at_ms: None,
                runs_since_review: 0,
                expires_at_ms: None,
                expired_at_ms: None,
                retired_at_ms: None,
                retire_reason: None,
                paused_for_lifecycle: false,
                health_last_checked_at_ms: None,
                health_findings: Vec::new(),
            });
        record.provenance = provenance;
        if record.expires_at_ms.is_none()
            && record.provenance.creator.kind == GovernanceActorKind::Agent
        {
            let default_expires_after_ms = self
                .automation_governance
                .read()
                .await
                .limits
                .default_expires_after_ms;
            if default_expires_after_ms > 0 {
                record.expires_at_ms = Some(now_ms().saturating_add(default_expires_after_ms));
            }
        }
        let stored = self.upsert_automation_governance(record).await?;
        if let Some(agent_id) = stored
            .provenance
            .creator
            .actor_id
            .as_deref()
            .filter(|_| stored.provenance.creator.kind == GovernanceActorKind::Agent)
        {
            let _ = self
                .record_agent_creation_review_progress(agent_id, &stored.automation_id)
                .await;
        }
        Ok(stored)
    }

    pub async fn sync_automation_governance_from_spec(
        &self,
        automation: &crate::AutomationV2Spec,
        provenance: Option<AutomationProvenanceRecord>,
    ) -> anyhow::Result<AutomationGovernanceRecord> {
        let now = now_ms();
        let mut record = self
            .get_automation_governance(&automation.automation_id)
            .await
            .unwrap_or_else(|| AutomationGovernanceRecord {
                automation_id: automation.automation_id.clone(),
                provenance: provenance.clone().unwrap_or_else(|| {
                    default_human_provenance(Some(automation.creator_id.clone()), "sync_default")
                }),
                declared_capabilities: declared_capabilities_for_automation(automation),
                modify_grants: Vec::new(),
                capability_grants: Vec::new(),
                created_at_ms: automation.created_at_ms,
                updated_at_ms: now,
                deleted_at_ms: None,
                delete_retention_until_ms: None,
                published_externally: false,
                creation_paused: false,
                review_required: false,
                review_kind: None,
                review_requested_at_ms: None,
                review_request_id: None,
                last_reviewed_at_ms: None,
                runs_since_review: 0,
                expires_at_ms: None,
                expired_at_ms: None,
                retired_at_ms: None,
                retire_reason: None,
                paused_for_lifecycle: false,
                health_last_checked_at_ms: None,
                health_findings: Vec::new(),
            });
        if let Some(provenance) = provenance {
            record.provenance = provenance;
        }
        record.declared_capabilities = declared_capabilities_for_automation(automation);
        if record.created_at_ms == 0 {
            record.created_at_ms = automation.created_at_ms;
        }
        record.updated_at_ms = now;
        {
            let mut guard = self.automation_governance.write().await;
            guard
                .records
                .insert(record.automation_id.clone(), record.clone());
            guard.updated_at_ms = now;
        }
        self.persist_automation_governance().await?;
        if let Some(agent_id) = record
            .provenance
            .creator
            .actor_id
            .as_deref()
            .filter(|_| record.provenance.creator.kind == GovernanceActorKind::Agent)
        {
            let _ = self
                .record_agent_creation_review_progress(agent_id, &record.automation_id)
                .await;
        }
        Ok(record)
    }

    pub async fn pause_automation_creation_for_agent(
        &self,
        agent_id: &str,
        paused: bool,
    ) -> anyhow::Result<()> {
        let mut guard = self.automation_governance.write().await;
        if paused {
            if !guard.paused_agents.iter().any(|value| value == agent_id) {
                guard.paused_agents.push(agent_id.to_string());
            }
        } else {
            guard.paused_agents.retain(|value| value != agent_id);
        }
        guard.updated_at_ms = now_ms();
        drop(guard);
        self.persist_automation_governance().await?;
        Ok(())
    }

    pub async fn can_create_automation_for_actor(
        &self,
        actor: &GovernanceActorRef,
        provenance: &AutomationProvenanceRecord,
        declared_capabilities: &AutomationDeclaredCapabilities,
    ) -> Result<(), GovernanceError> {
        let guard = self.automation_governance.read().await;
        let limits = &guard.limits;
        if !limits.creation_enabled {
            return Err(GovernanceError::forbidden(
                "AUTOMATION_V2_CREATION_DISABLED",
                "agent automation creation is disabled for this tenant",
            ));
        }
        if matches!(actor.kind, GovernanceActorKind::Agent) {
            let agent_id = actor.actor_id.as_deref().unwrap_or_default();
            if agent_id.is_empty() {
                return Err(GovernanceError::forbidden(
                    "AUTOMATION_V2_AGENT_ID_REQUIRED",
                    "agent automation creation requires an agent identifier",
                ));
            }
            if guard.is_agent_paused(agent_id) {
                return Err(GovernanceError::forbidden(
                    "AUTOMATION_V2_AGENT_CREATION_PAUSED",
                    "this agent is paused from creating automations",
                ));
            }
            if guard.is_agent_spend_paused(agent_id)
                && !guard.has_approved_agent_quota_override(agent_id)
            {
                return Err(GovernanceError::too_many_requests(
                    "AUTOMATION_V2_AGENT_SPEND_CAP_EXCEEDED",
                    "this agent is paused after reaching its spend cap",
                ));
            }
            if guard
                .agent_creation_reviews
                .get(agent_id)
                .is_some_and(|summary| summary.review_required)
            {
                return Err(GovernanceError::too_many_requests(
                    "AUTOMATION_V2_AGENT_REVIEW_REQUIRED",
                    format!(
                        "agent {} must be reviewed before creating additional automations",
                        agent_id
                    ),
                ));
            }
            self.validate_declared_capabilities_for_agent(
                &guard,
                agent_id,
                declared_capabilities,
                None,
            )?;
            if provenance.depth > limits.lineage_depth_limit {
                return Err(GovernanceError::forbidden(
                    "AUTOMATION_V2_LINEAGE_DEPTH_EXCEEDED",
                    format!(
                        "lineage depth {} exceeds configured limit {}",
                        provenance.depth, limits.lineage_depth_limit
                    ),
                ));
            }
            let window_start = now_ms().saturating_sub(24 * 60 * 60 * 1000);
            let created_today = guard
                .records
                .values()
                .filter(|record| {
                    record.deleted_at_ms.is_none()
                        && record.provenance.creator.kind == GovernanceActorKind::Agent
                        && record
                            .provenance
                            .creator
                            .actor_id
                            .as_deref()
                            .is_some_and(|value| value == agent_id)
                        && record.created_at_ms >= window_start
                })
                .count() as u64;
            if created_today >= limits.per_agent_daily_creation_limit {
                return Err(GovernanceError::too_many_requests(
                    "AUTOMATION_V2_AGENT_DAILY_QUOTA_EXCEEDED",
                    format!(
                        "agent {} has reached the daily automation creation quota",
                        agent_id
                    ),
                ));
            }
            let active_agent_created = guard
                .records
                .values()
                .filter(|record| {
                    record.deleted_at_ms.is_none()
                        && record.provenance.creator.kind == GovernanceActorKind::Agent
                })
                .count() as u64;
            if active_agent_created >= limits.active_agent_automation_cap {
                return Err(GovernanceError::too_many_requests(
                    "AUTOMATION_V2_AGENT_CAP_EXCEEDED",
                    "tenant has reached the active agent-authored automation cap",
                ));
            }
        }
        Ok(())
    }

    fn validate_declared_capabilities_for_agent(
        &self,
        guard: &GovernanceState,
        agent_id: &str,
        declared_capabilities: &AutomationDeclaredCapabilities,
        previous_capabilities: Option<&AutomationDeclaredCapabilities>,
    ) -> Result<(), GovernanceError> {
        let previous = previous_capabilities.cloned().unwrap_or_default();
        for capability in declared_capabilities.escalates_from(&previous) {
            if !guard.has_approved_agent_capability(agent_id, capability) {
                return Err(GovernanceError::forbidden(
                    "AUTOMATION_V2_CAPABILITY_ESCALATION_FORBIDDEN",
                    format!(
                        "agent {} lacks approval for capability {}",
                        agent_id, capability
                    ),
                ));
            }
        }
        Ok(())
    }

    pub async fn can_escalate_declared_capabilities(
        &self,
        actor: &GovernanceActorRef,
        previous: &AutomationDeclaredCapabilities,
        next: &AutomationDeclaredCapabilities,
    ) -> Result<(), GovernanceError> {
        if matches!(actor.kind, GovernanceActorKind::Human) {
            return Ok(());
        }
        let Some(agent_id) = actor.actor_id.as_deref() else {
            return Err(GovernanceError::forbidden(
                "AUTOMATION_V2_AGENT_ID_REQUIRED",
                "agent automation requests require an agent identifier",
            ));
        };
        let guard = self.automation_governance.read().await;
        self.validate_declared_capabilities_for_agent(&guard, agent_id, next, Some(previous))
    }

    pub async fn can_mutate_automation(
        &self,
        automation_id: &str,
        actor: &GovernanceActorRef,
        destructive: bool,
    ) -> Result<AutomationGovernanceRecord, GovernanceError> {
        let guard = self.automation_governance.read().await;
        let Some(record) = guard.records.get(automation_id).cloned() else {
            return Err(GovernanceError::forbidden(
                "AUTOMATION_V2_GOVERNANCE_MISSING",
                "automation governance record not found",
            ));
        };
        if matches!(actor.kind, GovernanceActorKind::Human) {
            return Ok(record);
        }
        let Some(actor_id) = actor.actor_id.as_deref() else {
            return Err(GovernanceError::forbidden(
                "AUTOMATION_V2_AGENT_ID_REQUIRED",
                "agent automation requests require an agent identifier",
            ));
        };
        if record.retired_at_ms.is_some() {
            return Err(GovernanceError::forbidden(
                "AUTOMATION_V2_RETIRED",
                "retired automations are not mutable by agents",
            ));
        }
        if record.expired_at_ms.is_some() && record.paused_for_lifecycle {
            return Err(GovernanceError::forbidden(
                "AUTOMATION_V2_EXPIRED",
                "expired automations are paused pending human review",
            ));
        }
        if record.paused_for_lifecycle {
            return Err(GovernanceError::forbidden(
                "AUTOMATION_V2_LIFECYCLE_PAUSED",
                "paused automations are not mutable by agents",
            ));
        }
        if destructive {
            if record.provenance.creator.kind != GovernanceActorKind::Agent {
                return Err(GovernanceError::forbidden(
                    "AUTOMATION_V2_DELETE_HUMAN_CREATED_DENIED",
                    "agents cannot delete human-created automations",
                ));
            }
            if record.provenance.creator.actor_id.as_deref() != Some(actor_id) {
                return Err(GovernanceError::forbidden(
                    "AUTOMATION_V2_DELETE_NOT_OWNER",
                    "agents can only delete automations they created",
                ));
            }
            return Ok(record);
        }
        if record.provenance.creator.kind == GovernanceActorKind::Agent
            && record.provenance.creator.actor_id.as_deref() == Some(actor_id)
        {
            return Ok(record);
        }
        if record.has_modify_grant(actor_id) {
            return Ok(record);
        }
        Err(GovernanceError::forbidden(
            "AUTOMATION_V2_MODIFY_FORBIDDEN",
            "agent lacks modify rights for this automation",
        ))
    }

    pub async fn record_automation_creation(
        &self,
        automation: &crate::AutomationV2Spec,
        provenance: AutomationProvenanceRecord,
    ) -> anyhow::Result<AutomationGovernanceRecord> {
        let mut record = AutomationGovernanceRecord {
            automation_id: automation.automation_id.clone(),
            provenance,
            declared_capabilities: declared_capabilities_for_automation(automation),
            modify_grants: Vec::new(),
            capability_grants: Vec::new(),
            created_at_ms: automation.created_at_ms,
            updated_at_ms: now_ms(),
            deleted_at_ms: None,
            delete_retention_until_ms: None,
            published_externally: false,
            creation_paused: false,
            review_required: false,
            review_kind: None,
            review_requested_at_ms: None,
            review_request_id: None,
            last_reviewed_at_ms: None,
            runs_since_review: 0,
            expires_at_ms: None,
            expired_at_ms: None,
            retired_at_ms: None,
            retire_reason: None,
            paused_for_lifecycle: false,
            health_last_checked_at_ms: None,
            health_findings: Vec::new(),
        };
        if record.expires_at_ms.is_none()
            && record.provenance.creator.kind == GovernanceActorKind::Agent
        {
            let default_expires_after_ms = self
                .automation_governance
                .read()
                .await
                .limits
                .default_expires_after_ms;
            if default_expires_after_ms > 0 {
                record.expires_at_ms = Some(now_ms().saturating_add(default_expires_after_ms));
            }
        }
        let stored = self.upsert_automation_governance(record).await?;
        if let Some(agent_id) = stored
            .provenance
            .creator
            .actor_id
            .as_deref()
            .filter(|_| stored.provenance.creator.kind == GovernanceActorKind::Agent)
        {
            let _ = self
                .record_agent_creation_review_progress(agent_id, &stored.automation_id)
                .await;
        }
        Ok(stored)
    }

    pub async fn grant_automation_modify_access(
        &self,
        automation_id: &str,
        granted_to: GovernanceActorRef,
        granted_by: GovernanceActorRef,
        reason: Option<String>,
    ) -> anyhow::Result<AutomationGrantRecord> {
        let grant = {
            let mut guard = self.automation_governance.write().await;
            let grant = {
                let Some(record) = guard.records.get_mut(automation_id) else {
                    anyhow::bail!("automation governance record not found");
                };
                let grant = AutomationGrantRecord {
                    grant_id: format!("grant-{}", Uuid::new_v4()),
                    automation_id: automation_id.to_string(),
                    grant_kind: AutomationGrantKind::Modify,
                    granted_to,
                    granted_by,
                    capability_key: None,
                    created_at_ms: now_ms(),
                    revoked_at_ms: None,
                    revoke_reason: reason,
                };
                record.modify_grants.push(grant.clone());
                record.updated_at_ms = now_ms();
                grant
            };
            guard.updated_at_ms = now_ms();
            grant
        };
        self.persist_automation_governance().await?;
        let _ = append_protected_audit_event(
            self,
            format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.grant.created"),
            &tandem_types::TenantContext::local_implicit(),
            grant
                .granted_by
                .actor_id
                .clone()
                .or_else(|| grant.granted_by.source.clone()),
            json!({
                "automationID": automation_id,
                "grant": grant,
            }),
        )
        .await;
        Ok(grant)
    }

    pub async fn revoke_automation_modify_access(
        &self,
        automation_id: &str,
        grant_id: &str,
        revoked_by: GovernanceActorRef,
        reason: Option<String>,
    ) -> anyhow::Result<Option<AutomationGrantRecord>> {
        let stored = {
            let mut guard = self.automation_governance.write().await;
            let stored = {
                let Some(record) = guard.records.get_mut(automation_id) else {
                    anyhow::bail!("automation governance record not found");
                };
                let Some(grant) = record
                    .modify_grants
                    .iter_mut()
                    .find(|grant| grant.grant_id == grant_id && grant.revoked_at_ms.is_none())
                else {
                    return Ok(None);
                };
                grant.revoked_at_ms = Some(now_ms());
                grant.revoke_reason = reason.clone();
                record.updated_at_ms = now_ms();
                grant.clone()
            };
            guard.updated_at_ms = now_ms();
            stored
        };
        self.persist_automation_governance().await?;
        let _ = append_protected_audit_event(
            self,
            format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.grant.revoked"),
            &tandem_types::TenantContext::local_implicit(),
            revoked_by
                .actor_id
                .clone()
                .or_else(|| revoked_by.source.clone()),
            json!({
                "automationID": automation_id,
                "grantID": grant_id,
                "reason": reason,
            }),
        )
        .await;
        Ok(Some(stored))
    }

    pub async fn request_approval(
        &self,
        request_type: GovernanceApprovalRequestType,
        requested_by: GovernanceActorRef,
        target_resource: GovernanceResourceRef,
        rationale: String,
        context: Value,
        expires_at_ms: Option<u64>,
    ) -> anyhow::Result<GovernanceApprovalRequest> {
        let now = now_ms();
        let approval_ttl_ms = self
            .automation_governance
            .read()
            .await
            .limits
            .approval_ttl_ms;
        let expires_at_ms = expires_at_ms.unwrap_or_else(|| now.saturating_add(approval_ttl_ms));
        let request = GovernanceApprovalRequest {
            approval_id: format!("apr_{}", Uuid::new_v4().simple()),
            request_type,
            requested_by,
            target_resource,
            rationale,
            context,
            status: GovernanceApprovalStatus::Pending,
            expires_at_ms,
            reviewed_by: None,
            reviewed_at_ms: None,
            review_notes: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        {
            let mut guard = self.automation_governance.write().await;
            guard
                .approvals
                .insert(request.approval_id.clone(), request.clone());
            guard.updated_at_ms = now;
        }
        self.persist_automation_governance().await?;
        let _ = append_protected_audit_event(
            self,
            format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.approval.requested"),
            &tandem_types::TenantContext::local_implicit(),
            request
                .requested_by
                .actor_id
                .clone()
                .or_else(|| request.requested_by.source.clone()),
            json!({
                "approvalID": request.approval_id,
                "request": request,
            }),
        )
        .await;
        Ok(request)
    }

    pub async fn list_approval_requests(
        &self,
        request_type: Option<GovernanceApprovalRequestType>,
        status: Option<GovernanceApprovalStatus>,
    ) -> Vec<GovernanceApprovalRequest> {
        let mut rows = self
            .automation_governance
            .read()
            .await
            .approvals
            .values()
            .filter(|request| {
                request_type
                    .map(|value| request.request_type == value)
                    .unwrap_or(true)
                    && status.map(|value| request.status == value).unwrap_or(true)
            })
            .cloned()
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
        rows
    }

    pub async fn decide_approval_request(
        &self,
        approval_id: &str,
        reviewer: GovernanceActorRef,
        approved: bool,
        notes: Option<String>,
    ) -> anyhow::Result<Option<GovernanceApprovalRequest>> {
        let stored = {
            let mut guard = self.automation_governance.write().await;
            let stored = {
                let Some(request) = guard.approvals.get_mut(approval_id) else {
                    return Ok(None);
                };
                if request.status != GovernanceApprovalStatus::Pending {
                    return Ok(Some(request.clone()));
                }
                let now = now_ms();
                request.status = if approved {
                    GovernanceApprovalStatus::Approved
                } else {
                    GovernanceApprovalStatus::Denied
                };
                request.reviewed_by = Some(reviewer.clone());
                request.reviewed_at_ms = Some(now);
                request.review_notes = notes.clone();
                request.updated_at_ms = now;
                request.clone()
            };
            guard.updated_at_ms = now_ms();
            stored
        };
        self.persist_automation_governance().await?;
        let _ = append_protected_audit_event(
            self,
            format!(
                "{GOVERNANCE_AUDIT_EVENT_PREFIX}.approval.{}",
                if approved { "approved" } else { "denied" }
            ),
            &tandem_types::TenantContext::local_implicit(),
            reviewer
                .actor_id
                .clone()
                .or_else(|| reviewer.source.clone()),
            json!({
                "approvalID": approval_id,
                "approval": stored,
            }),
        )
        .await;
        Ok(Some(stored))
    }

    pub async fn delete_automation_v2_with_governance(
        &self,
        automation_id: &str,
        deleted_by: GovernanceActorRef,
    ) -> anyhow::Result<Option<crate::AutomationV2Spec>> {
        let _guard = self.automations_v2_persistence.lock().await;
        let removed = self.automations_v2.write().await.remove(automation_id);
        if let Some(automation) = removed.clone() {
            let now = now_ms();
            {
                let mut governance = self.automation_governance.write().await;
                let record = governance
                    .records
                    .entry(automation_id.to_string())
                    .or_insert_with(|| AutomationGovernanceRecord {
                        automation_id: automation_id.to_string(),
                        provenance: default_human_provenance(
                            Some(automation.creator_id.clone()),
                            "delete_default",
                        ),
                        declared_capabilities: declared_capabilities_for_automation(&automation),
                        modify_grants: Vec::new(),
                        capability_grants: Vec::new(),
                        created_at_ms: automation.created_at_ms,
                        updated_at_ms: now,
                        deleted_at_ms: None,
                        delete_retention_until_ms: None,
                        published_externally: false,
                        creation_paused: false,
                        review_required: false,
                        review_kind: None,
                        review_requested_at_ms: None,
                        review_request_id: None,
                        last_reviewed_at_ms: None,
                        runs_since_review: 0,
                        expires_at_ms: None,
                        expired_at_ms: None,
                        retired_at_ms: None,
                        retire_reason: None,
                        paused_for_lifecycle: false,
                        health_last_checked_at_ms: None,
                        health_findings: Vec::new(),
                    });
                record.deleted_at_ms = Some(now);
                record.delete_retention_until_ms =
                    Some(now.saturating_add(7 * 24 * 60 * 60 * 1000));
                record.updated_at_ms = now;
                governance.deleted_automations.insert(
                    automation_id.to_string(),
                    DeletedAutomationRecord {
                        automation: automation.clone(),
                        deleted_at_ms: now,
                        deleted_by: deleted_by.clone(),
                        restore_until_ms: now.saturating_add(7 * 24 * 60 * 60 * 1000),
                    },
                );
                governance.updated_at_ms = now;
            }
            self.persist_automation_governance().await?;
            self.persist_automations_v2_locked().await?;
            let _ = append_protected_audit_event(
                self,
                format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.deleted"),
                &tandem_types::TenantContext::local_implicit(),
                deleted_by
                    .actor_id
                    .clone()
                    .or_else(|| deleted_by.source.clone()),
                json!({
                    "automationID": automation_id,
                    "deletedBy": deleted_by,
                    "deletedAtMs": now,
                }),
            )
            .await;
        }
        Ok(removed)
    }

    pub async fn restore_deleted_automation_v2(
        &self,
        automation_id: &str,
    ) -> anyhow::Result<Option<crate::AutomationV2Spec>> {
        let restored = {
            let mut governance = self.automation_governance.write().await;
            let Some(deleted) = governance.deleted_automations.remove(automation_id) else {
                return Ok(None);
            };
            let automation = deleted.automation.clone();
            self.automations_v2
                .write()
                .await
                .insert(automation_id.to_string(), automation.clone());
            if let Some(record) = governance.records.get_mut(automation_id) {
                record.deleted_at_ms = None;
                record.delete_retention_until_ms = None;
                record.updated_at_ms = now_ms();
            }
            governance.updated_at_ms = now_ms();
            automation
        };
        self.persist_automation_governance().await?;
        self.persist_automations_v2().await?;
        Ok(Some(restored))
    }

    pub async fn agent_spend_summary(&self, agent_id: &str) -> Option<AgentSpendSummary> {
        self.automation_governance
            .read()
            .await
            .agent_spend_summary(agent_id)
    }

    pub async fn list_agent_spend_summaries(&self) -> Vec<AgentSpendSummary> {
        self.automation_governance
            .read()
            .await
            .agent_spend_summaries()
    }

    pub async fn agent_creation_review_summary(
        &self,
        agent_id: &str,
    ) -> Option<AgentCreationReviewSummary> {
        self.automation_governance
            .read()
            .await
            .agent_creation_review_summary(agent_id)
    }

    pub async fn list_agent_creation_review_summaries(&self) -> Vec<AgentCreationReviewSummary> {
        self.automation_governance
            .read()
            .await
            .agent_creation_review_summaries()
    }

    pub async fn record_agent_creation_review_progress(
        &self,
        agent_id: &str,
        automation_id: &str,
    ) -> anyhow::Result<()> {
        let now = now_ms();
        let (created_since_review, threshold, should_request) = {
            let mut guard = self.automation_governance.write().await;
            let threshold = guard.limits.per_agent_creation_review_threshold;
            let (created_since_review, should_request) = {
                let summary = guard
                    .agent_creation_reviews
                    .entry(agent_id.to_string())
                    .or_insert_with(|| AgentCreationReviewSummary::new(agent_id.to_string(), now));
                summary.created_since_review = summary.created_since_review.saturating_add(1);
                summary.updated_at_ms = now;
                let should_request = threshold > 0
                    && summary.created_since_review >= threshold
                    && !summary.review_required;
                if should_request {
                    summary.review_required = true;
                    summary.review_kind = Some(AutomationLifecycleReviewKind::CreationQuota);
                    summary.review_requested_at_ms = Some(now);
                }
                (summary.created_since_review, should_request)
            };
            guard.updated_at_ms = now;
            (created_since_review, threshold, should_request)
        };
        self.persist_automation_governance().await?;
        if should_request {
            let _ = self
                .request_approval(
                    GovernanceApprovalRequestType::LifecycleReview,
                    GovernanceActorRef::system("automation_creation_review"),
                    GovernanceResourceRef {
                        resource_type: "agent".to_string(),
                        id: agent_id.to_string(),
                    },
                    format!(
                        "Human acknowledgment required after agent {agent_id} created {created_since_review} automations"
                    ),
                    json!({
                        "trigger": "creation_quota",
                        "agentID": agent_id,
                        "automationID": automation_id,
                        "createdSinceReview": created_since_review,
                        "creationReviewThreshold": threshold,
                    }),
                    None,
                )
                .await;
        }
        Ok(())
    }

    pub async fn acknowledge_agent_creation_review(
        &self,
        agent_id: &str,
        reviewer: GovernanceActorRef,
        notes: Option<String>,
    ) -> anyhow::Result<()> {
        let now = now_ms();
        {
            let mut guard = self.automation_governance.write().await;
            let summary = guard
                .agent_creation_reviews
                .entry(agent_id.to_string())
                .or_insert_with(|| AgentCreationReviewSummary::new(agent_id.to_string(), now));
            summary.created_since_review = 0;
            summary.review_required = false;
            summary.review_kind = None;
            summary.review_requested_at_ms = None;
            summary.review_request_id = None;
            summary.last_reviewed_at_ms = Some(now);
            summary.last_review_notes = notes.clone();
            summary.updated_at_ms = now;
            guard.updated_at_ms = now;
        }
        self.persist_automation_governance().await?;
        let _ = append_protected_audit_event(
            self,
            format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.review.agent_acknowledged"),
            &tandem_types::TenantContext::local_implicit(),
            reviewer
                .actor_id
                .clone()
                .or_else(|| reviewer.source.clone()),
            json!({
                "agentID": agent_id,
                "reviewer": reviewer,
                "notes": notes,
            }),
        )
        .await;
        Ok(())
    }

    pub async fn acknowledge_automation_review(
        &self,
        automation_id: &str,
        reviewer: GovernanceActorRef,
        notes: Option<String>,
    ) -> anyhow::Result<Option<AutomationGovernanceRecord>> {
        let stored = {
            let mut guard = self.automation_governance.write().await;
            let stored = {
                let Some(record) = guard.records.get_mut(automation_id) else {
                    return Ok(None);
                };
                let now = now_ms();
                record.review_required = false;
                record.review_kind = None;
                record.review_requested_at_ms = None;
                record.review_request_id = None;
                record.last_reviewed_at_ms = Some(now);
                record.runs_since_review = 0;
                record.health_findings.clear();
                record.health_last_checked_at_ms = Some(now);
                record.updated_at_ms = now;
                record.clone()
            };
            guard.updated_at_ms = now_ms();
            stored
        };
        self.persist_automation_governance().await?;
        let _ = append_protected_audit_event(
            self,
            format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.review.automation_acknowledged"),
            &tandem_types::TenantContext::local_implicit(),
            reviewer
                .actor_id
                .clone()
                .or_else(|| reviewer.source.clone()),
            json!({
                "automationID": automation_id,
                "reviewer": reviewer,
                "notes": notes,
            }),
        )
        .await;
        Ok(Some(stored))
    }

    pub async fn pause_automation_for_dependency_revocation(
        &self,
        automation_id: &str,
        reason: String,
        evidence: Value,
    ) -> anyhow::Result<()> {
        let Some(automation) = self.get_automation_v2(automation_id).await else {
            anyhow::bail!("automation not found");
        };
        let now = now_ms();
        let paused_runs = self
            .pause_running_automation_v2_runs(
                automation_id,
                reason.clone(),
                crate::AutomationStopKind::GuardrailStopped,
            )
            .await;

        let dependency_context = json!({
            "trigger": "dependency_revoked",
            "reason": reason.clone(),
            "evidence": evidence,
            "pausedRunIDs": paused_runs.clone(),
        });
        let finding = AutomationLifecycleFinding {
            finding_id: format!("finding-{}", uuid::Uuid::new_v4().simple()),
            kind: AutomationLifecycleReviewKind::DependencyRevoked,
            severity: AutomationLifecycleFindingSeverity::Critical,
            summary: "automation paused after dependency revocation".to_string(),
            detail: Some(
                "an owned grant or connected MCP capability was removed and the automation was paused pending review"
                    .to_string(),
            ),
            observed_at_ms: now,
            automation_run_id: None,
            approval_id: None,
            evidence: Some(dependency_context.clone()),
        };

        let pending_review_id = {
            let guard = self.automation_governance.read().await;
            guard
                .approvals
                .values()
                .filter(|request| {
                    request.request_type == GovernanceApprovalRequestType::LifecycleReview
                        && request.status == GovernanceApprovalStatus::Pending
                        && request.target_resource.resource_type == "automation"
                        && request.target_resource.id == automation_id
                })
                .max_by_key(|request| request.updated_at_ms)
                .map(|request| request.approval_id.clone())
        };

        {
            let mut guard = self.automation_governance.write().await;
            let record = guard
                .records
                .entry(automation_id.to_string())
                .or_insert_with(|| AutomationGovernanceRecord {
                    automation_id: automation_id.to_string(),
                    provenance: default_human_provenance(
                        Some(automation.creator_id.clone()),
                        "dependency_revocation_default",
                    ),
                    declared_capabilities: declared_capabilities_for_automation(&automation),
                    modify_grants: Vec::new(),
                    capability_grants: Vec::new(),
                    created_at_ms: automation.created_at_ms,
                    updated_at_ms: now,
                    deleted_at_ms: None,
                    delete_retention_until_ms: None,
                    published_externally: false,
                    creation_paused: false,
                    review_required: false,
                    review_kind: None,
                    review_requested_at_ms: None,
                    review_request_id: None,
                    last_reviewed_at_ms: None,
                    runs_since_review: 0,
                    expires_at_ms: None,
                    expired_at_ms: None,
                    retired_at_ms: None,
                    retire_reason: None,
                    paused_for_lifecycle: false,
                    health_last_checked_at_ms: None,
                    health_findings: Vec::new(),
                });
            record.declared_capabilities = declared_capabilities_for_automation(&automation);
            record.paused_for_lifecycle = true;
            record.review_required = true;
            record.review_kind = Some(AutomationLifecycleReviewKind::DependencyRevoked);
            record.review_requested_at_ms = Some(now);
            record.review_request_id = pending_review_id.clone();
            record.health_last_checked_at_ms = Some(now);
            record.health_findings.push(finding.clone());
            record.updated_at_ms = now;
            guard.updated_at_ms = now;
        }
        self.persist_automation_governance().await?;

        let mut created_review_id = pending_review_id;
        if created_review_id.is_none() {
            if let Ok(approval) = self
                .request_approval(
                    GovernanceApprovalRequestType::LifecycleReview,
                    GovernanceActorRef::system("automation_dependency_revocation"),
                    GovernanceResourceRef {
                        resource_type: "automation".to_string(),
                        id: automation_id.to_string(),
                    },
                    format!(
                        "Human review required after dependency revocation paused automation {automation_id}"
                    ),
                    dependency_context.clone(),
                    None,
                )
                .await
            {
                created_review_id = Some(approval.approval_id.clone());
                {
                    let mut guard = self.automation_governance.write().await;
                    if let Some(record) = guard.records.get_mut(automation_id) {
                        record.review_request_id = created_review_id.clone();
                        record.updated_at_ms = now_ms();
                    }
                    guard.updated_at_ms = now_ms();
                }
                self.persist_automation_governance().await?;
            }
        }

        let _ = append_protected_audit_event(
            self,
            format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.dependency_revoked"),
            &tandem_types::TenantContext::local_implicit(),
            Some("automation_dependency_revocation".to_string()),
            json!({
                "automationID": automation_id,
                "reason": reason,
                "pausedRunIDs": paused_runs,
                "evidence": dependency_context.clone(),
                "reviewRequestID": created_review_id,
            }),
        )
        .await;

        Ok(())
    }

    async fn pause_running_automation_v2_runs(
        &self,
        automation_id: &str,
        reason: String,
        stop_kind: crate::AutomationStopKind,
    ) -> Vec<String> {
        let runs = self.list_automation_v2_runs(Some(automation_id), 100).await;
        let mut paused_runs = Vec::new();
        for run in runs {
            if run.status != crate::AutomationRunStatus::Running {
                continue;
            }
            let session_ids = run.active_session_ids.clone();
            let instance_ids = run.active_instance_ids.clone();
            let _ = self
                .update_automation_v2_run(&run.run_id, |row| {
                    row.status = crate::AutomationRunStatus::Pausing;
                    row.pause_reason = Some(reason.clone());
                })
                .await;
            for session_id in &session_ids {
                let _ = self.cancellations.cancel(session_id).await;
            }
            for instance_id in instance_ids {
                let _ = self
                    .agent_teams
                    .cancel_instance(self, &instance_id, &reason)
                    .await;
            }
            self.forget_automation_v2_sessions(&session_ids).await;
            let _ = self
                .update_automation_v2_run(&run.run_id, |row| {
                    row.status = crate::AutomationRunStatus::Paused;
                    row.active_session_ids.clear();
                    row.active_instance_ids.clear();
                    row.pause_reason = Some(reason.clone());
                    row.stop_kind = Some(stop_kind.clone());
                    row.stop_reason = Some(reason.clone());
                    crate::app::state::automation::lifecycle::record_automation_lifecycle_event(
                        row,
                        "run_paused_governance",
                        Some(reason.clone()),
                        Some(stop_kind.clone()),
                    );
                })
                .await;
            paused_runs.push(run.run_id);
        }
        paused_runs
    }

    pub async fn record_automation_review_progress(
        &self,
        automation_id: &str,
        reason: AutomationLifecycleReviewKind,
        run_id: Option<String>,
        detail: Option<String>,
    ) -> anyhow::Result<()> {
        let now = now_ms();
        let (should_request, review_count) = {
            let mut guard = self.automation_governance.write().await;
            let threshold = guard.limits.run_review_threshold;
            let (should_request, review_count) = {
                let Some(record) = guard.records.get_mut(automation_id) else {
                    return Ok(());
                };
                record.runs_since_review = record.runs_since_review.saturating_add(1);
                record.health_last_checked_at_ms = Some(now);
                record.updated_at_ms = now;
                let should_request = threshold > 0
                    && record.runs_since_review >= threshold
                    && !record.review_required;
                if should_request {
                    record.review_required = true;
                    record.review_kind = Some(reason);
                    record.review_requested_at_ms = Some(now);
                }
                (should_request, record.runs_since_review)
            };
            guard.updated_at_ms = now;
            (should_request, review_count)
        };
        self.persist_automation_governance().await?;
        if should_request {
            let _ = self
                .request_approval(
                    GovernanceApprovalRequestType::LifecycleReview,
                    GovernanceActorRef::system("automation_lifecycle_review"),
                    GovernanceResourceRef {
                        resource_type: "automation".to_string(),
                        id: automation_id.to_string(),
                    },
                    format!(
                        "Human review required after automation {automation_id} completed {review_count} runs without acknowledgment"
                    ),
                    json!({
                        "trigger": "run_drift",
                        "automationID": automation_id,
                        "runID": run_id,
                        "detail": detail,
                        "runCountSinceReview": review_count,
                        "reviewKind": "run_drift",
                    }),
                    None,
                )
                .await;
        }
        Ok(())
    }

    pub async fn run_automation_governance_health_check(&self) -> anyhow::Result<usize> {
        let now = now_ms();
        let limits = self.automation_governance.read().await.limits.clone();
        let automations = self.list_automations_v2().await;
        let mut finding_count = 0usize;

        for automation in automations {
            let runs = self
                .list_automation_v2_runs(
                    Some(&automation.automation_id),
                    limits.health_window_run_limit.max(5) as usize,
                )
                .await;
            let terminal_runs = runs
                .iter()
                .filter(|run| {
                    matches!(
                        run.status,
                        crate::AutomationRunStatus::Completed
                            | crate::AutomationRunStatus::Blocked
                            | crate::AutomationRunStatus::Failed
                            | crate::AutomationRunStatus::Cancelled
                    )
                })
                .collect::<Vec<_>>();
            let failure_count = terminal_runs
                .iter()
                .filter(|run| {
                    matches!(
                        run.status,
                        crate::AutomationRunStatus::Failed | crate::AutomationRunStatus::Blocked
                    )
                })
                .count();
            let empty_output_count = terminal_runs
                .iter()
                .filter(|run| {
                    run.status == crate::AutomationRunStatus::Completed
                        && run.checkpoint.node_outputs.is_empty()
                })
                .count();
            let guardrail_stop_count = terminal_runs
                .iter()
                .filter(|run| run.stop_kind == Some(crate::AutomationStopKind::GuardrailStopped))
                .count();

            let mut findings = Vec::new();
            let mut automation_expires_at_ms = None;
            if !terminal_runs.is_empty() {
                let failure_rate = failure_count as f64 / terminal_runs.len() as f64;
                if failure_rate >= limits.health_failure_rate_threshold && terminal_runs.len() >= 5
                {
                    findings.push(AutomationLifecycleFinding {
                        finding_id: format!("finding-{}", uuid::Uuid::new_v4().simple()),
                        kind: AutomationLifecycleReviewKind::HealthDrift,
                        severity: if failure_rate >= 0.75 {
                            AutomationLifecycleFindingSeverity::Critical
                        } else {
                            AutomationLifecycleFindingSeverity::Warning
                        },
                        summary: "high failure rate across recent runs".to_string(),
                        detail: Some(format!(
                            "{} of {} recent terminal runs failed or were blocked ({:.0}% failure rate)",
                            failure_count,
                            terminal_runs.len(),
                            failure_rate * 100.0
                        )),
                        observed_at_ms: now,
                        automation_run_id: terminal_runs.last().map(|run| run.run_id.clone()),
                        approval_id: None,
                        evidence: Some(json!({
                            "failureCount": failure_count,
                            "terminalRunCount": terminal_runs.len(),
                            "failureRate": failure_rate,
                        })),
                    });
                }
            }
            if empty_output_count > 0 {
                findings.push(AutomationLifecycleFinding {
                    finding_id: format!("finding-{}", uuid::Uuid::new_v4().simple()),
                    kind: AutomationLifecycleReviewKind::HealthDrift,
                    severity: AutomationLifecycleFindingSeverity::Warning,
                    summary: "completed runs emitted empty outputs".to_string(),
                    detail: Some(format!(
                        "{} recent completed runs produced no node outputs",
                        empty_output_count
                    )),
                    observed_at_ms: now,
                    automation_run_id: terminal_runs.last().map(|run| run.run_id.clone()),
                    approval_id: None,
                    evidence: Some(json!({
                        "emptyOutputCount": empty_output_count,
                    })),
                });
            }
            if guardrail_stop_count >= limits.health_guardrail_stop_threshold as usize
                && limits.health_guardrail_stop_threshold > 0
            {
                findings.push(AutomationLifecycleFinding {
                    finding_id: format!("finding-{}", uuid::Uuid::new_v4().simple()),
                    kind: AutomationLifecycleReviewKind::HealthDrift,
                    severity: AutomationLifecycleFindingSeverity::Warning,
                    summary: "repeated guardrail stops detected".to_string(),
                    detail: Some(format!(
                        "{} recent terminal runs stopped on guardrails",
                        guardrail_stop_count
                    )),
                    observed_at_ms: now,
                    automation_run_id: terminal_runs.last().map(|run| run.run_id.clone()),
                    approval_id: None,
                    evidence: Some(json!({
                        "guardrailStopCount": guardrail_stop_count,
                    })),
                });
            }

            let mut should_create_review_request = false;
            let mut should_create_retirement_request = false;
            let mut should_pause_expired = false;
            {
                let mut guard = self.automation_governance.write().await;
                let has_pending_lifecycle_review = guard.has_pending_approval_request(
                    GovernanceApprovalRequestType::LifecycleReview,
                    "automation",
                    &automation.automation_id,
                );
                let has_pending_retirement_request = guard.has_pending_approval_request(
                    GovernanceApprovalRequestType::RetirementAction,
                    "automation",
                    &automation.automation_id,
                );
                let Some(record) = guard.records.get_mut(&automation.automation_id) else {
                    continue;
                };
                automation_expires_at_ms = record.expires_at_ms;
                record.health_last_checked_at_ms = Some(now);
                record.health_findings = findings.clone();
                if !findings.is_empty() {
                    record.review_required = true;
                    record.review_kind = Some(AutomationLifecycleReviewKind::HealthDrift);
                    if record.review_requested_at_ms.is_none() {
                        record.review_requested_at_ms = Some(now);
                    }
                    should_create_review_request = !has_pending_lifecycle_review;
                }
                if let Some(expires_at_ms) = record.expires_at_ms {
                    if now >= expires_at_ms && record.expired_at_ms.is_none() {
                        record.expired_at_ms = Some(now);
                        record.review_required = true;
                        record.review_kind = Some(AutomationLifecycleReviewKind::Expired);
                        record.review_requested_at_ms = Some(now);
                        record.paused_for_lifecycle = true;
                        should_pause_expired = true;
                        should_create_retirement_request = !has_pending_retirement_request;
                        findings.push(AutomationLifecycleFinding {
                            finding_id: format!("finding-{}", uuid::Uuid::new_v4().simple()),
                            kind: AutomationLifecycleReviewKind::Expired,
                            severity: AutomationLifecycleFindingSeverity::Critical,
                            summary: "automation has expired and was paused".to_string(),
                            detail: Some(format!(
                                "automation expired at {} and has been paused for human review",
                                expires_at_ms
                            )),
                            observed_at_ms: now,
                            automation_run_id: terminal_runs.last().map(|run| run.run_id.clone()),
                            approval_id: None,
                            evidence: Some(json!({
                                "expiresAtMs": expires_at_ms,
                                "expiredAtMs": now,
                            })),
                        });
                    } else if expires_at_ms > now
                        && expires_at_ms.saturating_sub(now) <= limits.expiration_warning_window_ms
                    {
                        record.review_required = true;
                        record.review_kind = Some(AutomationLifecycleReviewKind::ExpirationSoon);
                        if record.review_requested_at_ms.is_none() {
                            record.review_requested_at_ms = Some(now);
                        }
                        should_create_retirement_request = !has_pending_retirement_request;
                        findings.push(AutomationLifecycleFinding {
                            finding_id: format!("finding-{}", uuid::Uuid::new_v4().simple()),
                            kind: AutomationLifecycleReviewKind::ExpirationSoon,
                            severity: AutomationLifecycleFindingSeverity::Info,
                            summary: "automation is approaching its expiration date".to_string(),
                            detail: Some(format!(
                                "automation expires in {}ms",
                                expires_at_ms.saturating_sub(now)
                            )),
                            observed_at_ms: now,
                            automation_run_id: None,
                            approval_id: None,
                            evidence: Some(json!({
                                "expiresAtMs": expires_at_ms,
                                "warningWindowMs": limits.expiration_warning_window_ms,
                            })),
                        });
                    }
                }
                record.health_findings = findings.clone();
                record.updated_at_ms = now;
                guard.updated_at_ms = now;
            }
            self.persist_automation_governance().await?;

            if should_pause_expired && automation.status != crate::AutomationV2Status::Paused {
                let mut paused = automation.clone();
                paused.status = crate::AutomationV2Status::Paused;
                let _ = self.put_automation_v2(paused).await;
                let _ = self
                    .pause_running_automation_v2_runs(
                        &automation.automation_id,
                        format!(
                            "automation expired after reaching {}ms retention",
                            limits.default_expires_after_ms
                        ),
                        crate::AutomationStopKind::GuardrailStopped,
                    )
                    .await;
            }

            if should_create_review_request {
                let _ = self
                    .request_approval(
                        GovernanceApprovalRequestType::LifecycleReview,
                        GovernanceActorRef::system("automation_health_check"),
                        GovernanceResourceRef {
                            resource_type: "automation".to_string(),
                            id: automation.automation_id.clone(),
                        },
                        format!(
                            "Human review required after health check detected drift in automation {}",
                            automation.automation_id
                        ),
                        json!({
                            "trigger": "health_drift",
                            "automationID": automation.automation_id,
                            "findingCount": findings.len(),
                        }),
                        None,
                    )
                    .await;
            }

            if should_create_retirement_request {
                let _ = self
                    .request_approval(
                        GovernanceApprovalRequestType::RetirementAction,
                        GovernanceActorRef::system("automation_expiration"),
                        GovernanceResourceRef {
                            resource_type: "automation".to_string(),
                            id: automation.automation_id.clone(),
                        },
                        format!(
                            "Automation {} is expiring or has expired and needs operator action",
                            automation.automation_id
                        ),
                        json!({
                            "trigger": if should_pause_expired {
                                "expired"
                            } else {
                                "expiration_soon"
                            },
                            "automationID": automation.automation_id,
                            "expiresAtMs": automation_expires_at_ms,
                        }),
                        None,
                    )
                    .await;
            }

            finding_count += findings.len();
        }

        Ok(finding_count)
    }

    pub async fn retire_automation_v2(
        &self,
        automation_id: &str,
        actor: GovernanceActorRef,
        reason: Option<String>,
    ) -> anyhow::Result<Option<crate::AutomationV2Spec>> {
        let Some(mut automation) = self.get_automation_v2(automation_id).await else {
            return Ok(None);
        };
        let now = now_ms();
        let reason = reason.unwrap_or_else(|| "retired by operator".to_string());
        automation.status = crate::AutomationV2Status::Paused;
        let stored = self.put_automation_v2(automation).await?;
        let _ = self
            .pause_running_automation_v2_runs(
                automation_id,
                reason.clone(),
                crate::AutomationStopKind::OperatorStopped,
            )
            .await;
        {
            let mut guard = self.automation_governance.write().await;
            let record = guard
                .records
                .entry(automation_id.to_string())
                .or_insert_with(|| AutomationGovernanceRecord {
                    automation_id: automation_id.to_string(),
                    provenance: default_human_provenance(
                        Some(stored.creator_id.clone()),
                        "retire_default",
                    ),
                    declared_capabilities: declared_capabilities_for_automation(&stored),
                    modify_grants: Vec::new(),
                    capability_grants: Vec::new(),
                    created_at_ms: stored.created_at_ms,
                    updated_at_ms: now,
                    deleted_at_ms: None,
                    delete_retention_until_ms: None,
                    published_externally: false,
                    creation_paused: false,
                    review_required: false,
                    review_kind: None,
                    review_requested_at_ms: None,
                    review_request_id: None,
                    last_reviewed_at_ms: None,
                    runs_since_review: 0,
                    expires_at_ms: None,
                    expired_at_ms: None,
                    retired_at_ms: None,
                    retire_reason: None,
                    paused_for_lifecycle: false,
                    health_last_checked_at_ms: None,
                    health_findings: Vec::new(),
                });
            record.retired_at_ms = Some(now);
            record.retire_reason = Some(reason.clone());
            record.paused_for_lifecycle = true;
            record.review_required = false;
            record.review_kind = Some(AutomationLifecycleReviewKind::Retired);
            record.review_requested_at_ms = Some(now);
            record.updated_at_ms = now;
            guard.updated_at_ms = now;
        }
        self.persist_automation_governance().await?;
        let _ = append_protected_audit_event(
            self,
            format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.retired"),
            &tandem_types::TenantContext::local_implicit(),
            actor.actor_id.clone().or_else(|| actor.source.clone()),
            json!({
                "automationID": automation_id,
                "reason": reason,
                "actor": actor,
            }),
        )
        .await;
        Ok(Some(stored))
    }

    pub async fn extend_automation_v2_retirement(
        &self,
        automation_id: &str,
        actor: GovernanceActorRef,
        expires_at_ms: Option<u64>,
        reason: Option<String>,
    ) -> anyhow::Result<Option<crate::AutomationV2Spec>> {
        let Some(mut automation) = self.get_automation_v2(automation_id).await else {
            return Ok(None);
        };
        let now = now_ms();
        let default_expires_after_ms = self
            .automation_governance
            .read()
            .await
            .limits
            .default_expires_after_ms;
        let next_expires_at_ms =
            expires_at_ms.unwrap_or_else(|| now.saturating_add(default_expires_after_ms.max(1)));
        automation.status = crate::AutomationV2Status::Active;
        let stored = self.put_automation_v2(automation).await?;
        {
            let mut guard = self.automation_governance.write().await;
            let record = guard
                .records
                .entry(automation_id.to_string())
                .or_insert_with(|| AutomationGovernanceRecord {
                    automation_id: automation_id.to_string(),
                    provenance: default_human_provenance(
                        Some(stored.creator_id.clone()),
                        "extend_default",
                    ),
                    declared_capabilities: declared_capabilities_for_automation(&stored),
                    modify_grants: Vec::new(),
                    capability_grants: Vec::new(),
                    created_at_ms: stored.created_at_ms,
                    updated_at_ms: now,
                    deleted_at_ms: None,
                    delete_retention_until_ms: None,
                    published_externally: false,
                    creation_paused: false,
                    review_required: false,
                    review_kind: None,
                    review_requested_at_ms: None,
                    review_request_id: None,
                    last_reviewed_at_ms: None,
                    runs_since_review: 0,
                    expires_at_ms: None,
                    expired_at_ms: None,
                    retired_at_ms: None,
                    retire_reason: None,
                    paused_for_lifecycle: false,
                    health_last_checked_at_ms: None,
                    health_findings: Vec::new(),
                });
            record.expires_at_ms = Some(next_expires_at_ms);
            record.expired_at_ms = None;
            record.retired_at_ms = None;
            record.retire_reason = None;
            record.paused_for_lifecycle = false;
            record.review_required = false;
            record.review_kind = None;
            record.review_requested_at_ms = None;
            record.review_request_id = None;
            record.last_reviewed_at_ms = Some(now);
            record.health_last_checked_at_ms = Some(now);
            record.updated_at_ms = now;
            guard.updated_at_ms = now;
        }
        self.persist_automation_governance().await?;
        let _ = append_protected_audit_event(
            self,
            format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.retirement.extended"),
            &tandem_types::TenantContext::local_implicit(),
            actor.actor_id.clone().or_else(|| actor.source.clone()),
            json!({
                "automationID": automation_id,
                "expiresAtMs": next_expires_at_ms,
                "reason": reason,
                "actor": actor,
            }),
        )
        .await;
        Ok(Some(stored))
    }

    pub async fn record_automation_v2_spend(
        &self,
        run_id: &str,
        prompt_tokens: u64,
        completion_tokens: u64,
        total_tokens: u64,
        delta_cost_usd: f64,
    ) -> anyhow::Result<()> {
        let Some(run_snapshot) = self.get_automation_v2_run(run_id).await else {
            return Ok(());
        };
        let automation = if let Some(snapshot) = run_snapshot.automation_snapshot.clone() {
            snapshot
        } else {
            let Some(automation) = self.get_automation_v2(&run_snapshot.automation_id).await else {
                return Ok(());
            };
            automation
        };
        let governance = self
            .get_or_bootstrap_automation_governance(&automation)
            .await;
        let agent_ids = governance.agent_lineage_ids();
        if agent_ids.is_empty() {
            return Ok(());
        }

        let now = now_ms();
        let (weekly_cap, warning_threshold_ratio) = {
            let guard = self.automation_governance.read().await;
            (
                guard.limits.weekly_spend_cap_usd,
                guard.limits.spend_warning_threshold_ratio,
            )
        };

        let mut warning_events: Vec<(String, f64, f64)> = Vec::new();
        let mut hard_stop_agents: Vec<(String, f64, f64)> = Vec::new();
        {
            let mut guard = self.automation_governance.write().await;
            for agent_id in &agent_ids {
                let has_override = guard.has_approved_agent_quota_override(agent_id);
                let mut hard_stop_entry: Option<(String, f64, f64)> = None;
                let summary = guard
                    .agent_spend
                    .entry(agent_id.clone())
                    .or_insert_with(|| AgentSpendSummary::new(agent_id.clone(), now));
                summary.apply_usage(
                    now,
                    Some(&automation.automation_id),
                    Some(run_id),
                    prompt_tokens,
                    completion_tokens,
                    total_tokens,
                    delta_cost_usd,
                );
                if let Some(limit) = weekly_cap {
                    if summary.weekly_warning_threshold_reached(limit, warning_threshold_ratio)
                        && summary.weekly.soft_warning_at_ms.is_none()
                    {
                        summary.weekly.soft_warning_at_ms = Some(now);
                        warning_events.push((agent_id.clone(), summary.weekly.cost_usd, limit));
                    }
                    if summary.weekly_limit_reached(limit)
                        && summary.weekly.hard_stop_at_ms.is_none()
                        && !has_override
                    {
                        summary.weekly.hard_stop_at_ms = Some(now);
                        summary.paused_at_ms = Some(now);
                        summary.pause_reason =
                            Some(format!("weekly spend cap {:.2} USD reached", limit));
                        hard_stop_entry = Some((agent_id.clone(), summary.weekly.cost_usd, limit));
                    }
                }
                if let Some((agent_id, cost_usd, limit_usd)) = hard_stop_entry {
                    if !guard
                        .spend_paused_agents
                        .iter()
                        .any(|value| value == &agent_id)
                    {
                        guard.spend_paused_agents.push(agent_id.clone());
                    }
                    hard_stop_agents.push((agent_id, cost_usd, limit_usd));
                }
            }
            guard.updated_at_ms = now;
        }
        self.persist_automation_governance().await?;

        for (agent_id, cost_usd, limit_usd) in warning_events {
            let _ = append_protected_audit_event(
                self,
                format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.spend.warning"),
                &tandem_types::TenantContext::local_implicit(),
                governance
                    .provenance
                    .creator
                    .actor_id
                    .clone()
                    .or_else(|| Some(automation.creator_id.clone())),
                json!({
                    "automationID": automation.automation_id,
                    "runID": run_id,
                    "agentID": agent_id,
                    "weeklyCostUsd": cost_usd,
                    "weeklySpendCapUsd": limit_usd,
                }),
            )
            .await;
        }

        let mut requested_approvals = Vec::new();
        for (agent_id, cost_usd, limit_usd) in &hard_stop_agents {
            let guard = self.automation_governance.read().await;
            let has_override = guard.has_pending_agent_quota_override(agent_id)
                || guard.has_approved_agent_quota_override(agent_id);
            drop(guard);
            if has_override {
                continue;
            }
            if let Ok(approval) = self
                .request_approval(
                    GovernanceApprovalRequestType::QuotaOverride,
                    GovernanceActorRef::system("automation_spend_cap"),
                    GovernanceResourceRef {
                        resource_type: "agent".to_string(),
                        id: agent_id.clone(),
                    },
                    format!(
                        "Approve temporary quota override after agent {agent_id} reached weekly spend cap"
                    ),
                    json!({
                        "automationID": automation.automation_id,
                        "runID": run_id,
                        "agentID": agent_id,
                        "weeklyCostUsd": cost_usd,
                        "weeklySpendCapUsd": limit_usd,
                        "reason": "agent weekly spend cap exceeded",
                    }),
                    None,
                )
                .await
            {
                requested_approvals.push(approval.approval_id);
            }
        }

        if !hard_stop_agents.is_empty() {
            let session_ids = run_snapshot.active_session_ids.clone();
            for session_id in &session_ids {
                let _ = self.cancellations.cancel(session_id).await;
            }
            self.forget_automation_v2_sessions(&session_ids).await;
            let instance_ids = run_snapshot.active_instance_ids.clone();
            for instance_id in instance_ids {
                let _ = self
                    .agent_teams
                    .cancel_instance(self, &instance_id, "paused by spend guardrail")
                    .await;
            }
            let paused_agent_labels = hard_stop_agents
                .iter()
                .map(|(agent_id, cost_usd, limit_usd)| {
                    format!("{agent_id} ({cost_usd:.4}/{limit_usd:.4} USD)")
                })
                .collect::<Vec<_>>()
                .join(", ");
            let detail = format!("weekly spend cap exceeded for {paused_agent_labels}");
            let _ = self
                .update_automation_v2_run(run_id, |row| {
                    row.status = crate::AutomationRunStatus::Paused;
                    row.detail = Some(detail.clone());
                    row.pause_reason = Some(detail.clone());
                    row.stop_kind = Some(crate::AutomationStopKind::GuardrailStopped);
                    row.stop_reason = Some(detail.clone());
                    row.active_session_ids.clear();
                    row.latest_session_id = None;
                    row.active_instance_ids.clear();
                    crate::app::state::automation::lifecycle::record_automation_lifecycle_event(
                        row,
                        "run_paused_spend_cap_exceeded",
                        Some(detail.clone()),
                        Some(crate::AutomationStopKind::GuardrailStopped),
                    );
                })
                .await;
            let _ = append_protected_audit_event(
                self,
                format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.spend.paused"),
                &tandem_types::TenantContext::local_implicit(),
                governance
                    .provenance
                    .creator
                    .actor_id
                    .clone()
                    .or_else(|| Some(automation.creator_id.clone())),
                json!({
                    "automationID": automation.automation_id,
                    "runID": run_id,
                    "pausedAgents": hard_stop_agents
                        .iter()
                        .map(|(agent_id, _, _)| agent_id)
                        .cloned()
                        .collect::<Vec<_>>(),
                    "requestedApprovals": requested_approvals,
                    "detail": detail,
                }),
            )
            .await;
        }

        Ok(())
    }
}
