// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::HashMap;

use serde_json::json;
use serde_json::Value;
use tokio::fs;
use uuid::Uuid;

use crate::audit::append_protected_audit_event;
use crate::automation_v2::governance::*;
use crate::{now_ms, AppState};

const GOVERNANCE_AUDIT_EVENT_PREFIX: &str = "automation.governance";

fn governance_agent_scope_key(
    tenant_context: &tandem_types::TenantContext,
    agent_id: &str,
) -> String {
    let agent_id = agent_id.trim();
    if tenant_context.is_local_implicit() {
        return agent_id.to_string();
    }
    let deployment_id = tenant_context.deployment_id.as_deref().unwrap_or("");
    format!(
        "tenant:{}",
        serde_json::to_string(&(
            tenant_context.org_id.as_str(),
            tenant_context.workspace_id.as_str(),
            deployment_id,
            agent_id
        ))
        .unwrap_or_else(|_| agent_id.to_string())
    )
}

fn scoped_agent_id_maps(
    tenant_context: &tandem_types::TenantContext,
    agent_ids: &[String],
) -> (
    Vec<String>,
    HashMap<String, String>,
    HashMap<String, String>,
) {
    let mut scoped_ids = Vec::with_capacity(agent_ids.len());
    let mut raw_to_scoped = HashMap::new();
    let mut scoped_to_raw = HashMap::new();
    for agent_id in agent_ids {
        let scoped_id = governance_agent_scope_key(tenant_context, agent_id);
        scoped_ids.push(scoped_id.clone());
        raw_to_scoped.insert(agent_id.clone(), scoped_id.clone());
        scoped_to_raw.insert(scoped_id, agent_id.clone());
    }
    (scoped_ids, raw_to_scoped, scoped_to_raw)
}

fn scoped_agent_id_for_storage(agent_id: &str, raw_to_scoped: &HashMap<String, String>) -> String {
    raw_to_scoped
        .get(agent_id)
        .cloned()
        .unwrap_or_else(|| agent_id.to_string())
}

fn display_agent_id(agent_id: &str, scoped_to_raw: &HashMap<String, String>) -> String {
    scoped_to_raw
        .get(agent_id)
        .cloned()
        .unwrap_or_else(|| agent_id.to_string())
}

/// CT-09: decide whether an approval receipt is actionable by / visible to a caller.
/// Tenant identity is `(org_id, workspace_id)` — the same isolation axis the audit read
/// path scopes on (CT-04). A receipt with no bound tenant was issued under the local
/// default tenant, so it is compared against the local-implicit org/workspace; this keeps
/// single-tenant deployments a no-op (every caller shares that org/workspace) while a real
/// explicit tenant can never consume a receipt issued by a different org/workspace.
pub(super) fn approval_receipt_matches_tenant(
    approval: &GovernanceApprovalRequest,
    caller: &tandem_types::TenantContext,
) -> bool {
    let local = tandem_types::TenantContext::local_implicit();
    let owner = approval.tenant_context.as_ref().unwrap_or(&local);
    owner.org_id == caller.org_id && owner.workspace_id == caller.workspace_id
}

#[derive(Default)]
pub struct UnavailableGovernanceEngine;

impl GovernancePolicyEngine for UnavailableGovernanceEngine {
    fn premium_enabled(&self) -> bool {
        false
    }

    fn authorize_create(
        &self,
        _snapshot: &GovernanceContextSnapshot,
        actor: &GovernanceActorRef,
        _provenance: &AutomationProvenanceRecord,
        _declared_capabilities: &AutomationDeclaredCapabilities,
        _now_ms: u64,
    ) -> Result<(), GovernanceError> {
        if actor.kind == GovernanceActorKind::Human {
            return Ok(());
        }
        Err(GovernanceError::feature_unavailable(
            "premium governance is required for agent-authored automation creation",
        ))
    }

    fn authorize_capability_escalation(
        &self,
        _snapshot: &GovernanceContextSnapshot,
        actor: &GovernanceActorRef,
        _previous: &AutomationDeclaredCapabilities,
        _next: &AutomationDeclaredCapabilities,
        _now_ms: u64,
    ) -> Result<(), GovernanceError> {
        if actor.kind == GovernanceActorKind::Human {
            return Ok(());
        }
        Err(GovernanceError::feature_unavailable(
            "premium governance is required for agent capability escalation",
        ))
    }

    fn authorize_mutation(
        &self,
        record: &AutomationGovernanceRecord,
        actor: &GovernanceActorRef,
        _destructive: bool,
    ) -> Result<(), GovernanceError> {
        if actor.kind != GovernanceActorKind::Human {
            return Err(GovernanceError::feature_unavailable(
                "premium governance is required for agent-owned automation mutation",
            ));
        }
        // GOV-B10: in the OSS/local engine a human may mutate, but a record that has
        // a DISTINCT IDENTIFIED human owner may only be mutated by that owner. This
        // is intentionally a no-op for non-enterprise local operation: the local
        // control-panel user is anonymous (no actor_id) and locally-created records
        // carry no identified owner, so neither side has an id to compare and the
        // mutation is allowed. The check only engages when two distinct human
        // identities are present (e.g. a shared/hosted-style deployment running the
        // OSS engine), preventing one identified human from mutating another's
        // automation.
        let owner = &record.provenance.creator;
        if owner.kind == GovernanceActorKind::Human {
            if let (Some(owner_id), Some(actor_id)) =
                (owner.actor_id.as_deref(), actor.actor_id.as_deref())
            {
                let owner_id = owner_id.trim();
                let actor_id = actor_id.trim();
                if !owner_id.is_empty()
                    && !actor_id.is_empty()
                    && !owner_id.eq_ignore_ascii_case(actor_id)
                {
                    return Err(GovernanceError::forbidden(
                        "AUTOMATION_V2_NOT_OWNER",
                        "only the automation owner may mutate it in this build",
                    ));
                }
            }
        }
        Ok(())
    }

    fn create_approval_request(
        &self,
        _snapshot: &GovernanceContextSnapshot,
        _input: GovernanceApprovalDraftInput,
        _now_ms: u64,
    ) -> Result<GovernanceApprovalRequest, GovernanceError> {
        Err(GovernanceError::feature_unavailable(
            "premium governance approval flows are not available in this build",
        ))
    }

    fn decide_approval_request(
        &self,
        _existing: &GovernanceApprovalRequest,
        _reviewer: GovernanceActorRef,
        _approved: bool,
        _notes: Option<String>,
        _now_ms: u64,
    ) -> Result<GovernanceApprovalRequest, GovernanceError> {
        Err(GovernanceError::feature_unavailable(
            "premium governance approval flows are not available in this build",
        ))
    }

    fn evaluate_creation_review_progress(
        &self,
        _snapshot: &GovernanceContextSnapshot,
        _agent_id: &str,
        _automation_id: &str,
        _now_ms: u64,
    ) -> Result<GovernanceCreationReviewEvaluation, GovernanceError> {
        Err(GovernanceError::feature_unavailable(
            "premium governance review tracking is not available in this build",
        ))
    }

    fn evaluate_run_review_progress(
        &self,
        _snapshot: &GovernanceContextSnapshot,
        _automation_id: &str,
        _reason: AutomationLifecycleReviewKind,
        _run_id: Option<String>,
        _detail: Option<String>,
        _now_ms: u64,
    ) -> Result<Option<GovernanceAutomationReviewEvaluation>, GovernanceError> {
        Err(GovernanceError::feature_unavailable(
            "premium governance review tracking is not available in this build",
        ))
    }

    fn evaluate_dependency_revocation(
        &self,
        _snapshot: &GovernanceContextSnapshot,
        _input: GovernanceDependencyRevocationInput,
        _now_ms: u64,
    ) -> Result<GovernanceAutomationReviewEvaluation, GovernanceError> {
        Err(GovernanceError::feature_unavailable(
            "premium governance dependency revocation is not available in this build",
        ))
    }

    fn health_check_run_window(&self, _limits: &GovernanceLimits) -> usize {
        // The lifecycle health sweep is a premium feature; the host
        // short-circuits before gathering runs, so no window is needed.
        0
    }

    fn summarize_run_health(
        &self,
        _runs: &[GovernanceRunHealthObservation],
    ) -> GovernanceRunHealthSummary {
        GovernanceRunHealthSummary::default()
    }

    fn default_automation_expiry(
        &self,
        limits: &GovernanceLimits,
        provenance: &AutomationProvenanceRecord,
        now_ms: u64,
    ) -> Option<u64> {
        // Preserve the historical open behavior: agent-created records pick up
        // the configured default expiration (agent creation is rejected in
        // this build, so this only matters for pre-existing premium data).
        if provenance.creator.kind != GovernanceActorKind::Agent {
            return None;
        }
        if limits.default_expires_after_ms == 0 {
            return None;
        }
        Some(now_ms.saturating_add(limits.default_expires_after_ms))
    }

    fn acknowledge_creation_review(
        &self,
        existing: Option<AgentCreationReviewSummary>,
        agent_id: &str,
        notes: Option<String>,
        now_ms: u64,
    ) -> AgentCreationReviewSummary {
        // Preserve the historical open behavior: acknowledgment clears the
        // review state (the HTTP surface gates this behind premium anyway).
        let mut summary = existing
            .unwrap_or_else(|| AgentCreationReviewSummary::new(agent_id.to_string(), now_ms));
        summary.created_since_review = 0;
        summary.review_required = false;
        summary.review_kind = None;
        summary.review_requested_at_ms = None;
        summary.review_request_id = None;
        summary.last_reviewed_at_ms = Some(now_ms);
        summary.last_review_notes = notes;
        summary.updated_at_ms = now_ms;
        summary
    }

    fn acknowledge_automation_review(
        &self,
        record: &AutomationGovernanceRecord,
        now_ms: u64,
    ) -> AutomationGovernanceRecord {
        // Preserve the historical open behavior: acknowledgment clears the
        // review state (the HTTP surface gates this behind premium anyway).
        let mut record = record.clone();
        record.review_required = false;
        record.review_kind = None;
        record.review_requested_at_ms = None;
        record.review_request_id = None;
        record.last_reviewed_at_ms = Some(now_ms);
        record.runs_since_review = 0;
        record.health_findings.clear();
        record.health_last_checked_at_ms = Some(now_ms);
        record.updated_at_ms = now_ms;
        record
    }

    fn evaluate_health_check(
        &self,
        _snapshot: &GovernanceContextSnapshot,
        _input: GovernanceHealthCheckInput,
        _now_ms: u64,
    ) -> Result<Option<GovernanceHealthCheckEvaluation>, GovernanceError> {
        Ok(None)
    }

    fn evaluate_retirement(
        &self,
        _input: GovernanceRetirementInput,
        _now_ms: u64,
    ) -> Result<AutomationGovernanceRecord, GovernanceError> {
        Err(GovernanceError::feature_unavailable(
            "premium governance retirement logic is not available in this build",
        ))
    }

    fn evaluate_retirement_extension(
        &self,
        _input: GovernanceRetirementExtensionInput,
        _now_ms: u64,
    ) -> Result<AutomationGovernanceRecord, GovernanceError> {
        Err(GovernanceError::feature_unavailable(
            "premium governance retirement logic is not available in this build",
        ))
    }

    fn evaluate_spend_usage(
        &self,
        _snapshot: &GovernanceContextSnapshot,
        _input: &GovernanceSpendInput,
        _now_ms: u64,
    ) -> Result<GovernanceSpendEvaluation, GovernanceError> {
        Err(GovernanceError::feature_unavailable(
            "premium governance spend tracking is not available in this build",
        ))
    }
}

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

/// One-to-one translation from the host run status to the governance wire
/// status. Classification (terminal/failed/drift) happens in the policy
/// engine, not here.
fn governance_run_health_status(status: &crate::AutomationRunStatus) -> GovernanceRunHealthStatus {
    match status {
        crate::AutomationRunStatus::Queued => GovernanceRunHealthStatus::Queued,
        crate::AutomationRunStatus::Running => GovernanceRunHealthStatus::Running,
        crate::AutomationRunStatus::Pausing => GovernanceRunHealthStatus::Pausing,
        crate::AutomationRunStatus::Paused => GovernanceRunHealthStatus::Paused,
        crate::AutomationRunStatus::AwaitingApproval => GovernanceRunHealthStatus::AwaitingApproval,
        crate::AutomationRunStatus::Completed => GovernanceRunHealthStatus::Completed,
        crate::AutomationRunStatus::Blocked => GovernanceRunHealthStatus::Blocked,
        crate::AutomationRunStatus::Failed => GovernanceRunHealthStatus::Failed,
        crate::AutomationRunStatus::Cancelled => GovernanceRunHealthStatus::Cancelled,
    }
}

/// Fresh governance record with every lifecycle field at its default.
fn new_governance_record(
    automation_id: String,
    provenance: AutomationProvenanceRecord,
    declared_capabilities: AutomationDeclaredCapabilities,
    created_at_ms: u64,
    updated_at_ms: u64,
) -> AutomationGovernanceRecord {
    AutomationGovernanceRecord {
        automation_id,
        provenance,
        declared_capabilities,
        modify_grants: Vec::new(),
        capability_grants: Vec::new(),
        created_at_ms,
        updated_at_ms,
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
    }
}

impl AppState {
    pub fn premium_governance_enabled(&self) -> bool {
        self.governance_engine.premium_enabled()
    }

    fn governance_snapshot(&self, state: &GovernanceState) -> GovernanceContextSnapshot {
        state.snapshot()
    }

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
                    new_governance_record(
                        automation.automation_id.clone(),
                        default_human_provenance(
                            Some(automation.creator_id.clone()),
                            "migration_or_legacy_default",
                        ),
                        declared_capabilities_for_automation(&automation),
                        automation.created_at_ms.max(now),
                        now,
                    ),
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
        let record = new_governance_record(
            automation.automation_id.clone(),
            default_human_provenance(Some(automation.creator_id.clone()), "legacy_default"),
            declared_capabilities_for_automation(automation),
            automation.created_at_ms,
            now_ms(),
        );
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
        append_protected_audit_event(
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
        .await?;
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
            .unwrap_or_else(|| {
                new_governance_record(
                    automation_id.to_string(),
                    provenance.clone(),
                    AutomationDeclaredCapabilities::default(),
                    now_ms(),
                    now_ms(),
                )
            });
        record.provenance = provenance;
        if record.expires_at_ms.is_none() {
            let limits = self.automation_governance.read().await.limits.clone();
            record.expires_at_ms = self.governance_engine.default_automation_expiry(
                &limits,
                &record.provenance,
                now_ms(),
            );
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
            .unwrap_or_else(|| {
                new_governance_record(
                    automation.automation_id.clone(),
                    provenance.clone().unwrap_or_else(|| {
                        default_human_provenance(
                            Some(automation.creator_id.clone()),
                            "sync_default",
                        )
                    }),
                    declared_capabilities_for_automation(automation),
                    automation.created_at_ms,
                    now,
                )
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
        tenant_context: &tandem_types::TenantContext,
        actor: &GovernanceActorRef,
        provenance: &AutomationProvenanceRecord,
        declared_capabilities: &AutomationDeclaredCapabilities,
    ) -> Result<(), GovernanceError> {
        let snapshot = {
            let guard = self.automation_governance.read().await;
            let mut snapshot = self.governance_snapshot(&guard);
            if !tenant_context.is_local_implicit() && actor.kind == GovernanceActorKind::Agent {
                if let Some(agent_id) = actor
                    .actor_id
                    .as_deref()
                    .filter(|value| !value.trim().is_empty())
                {
                    let scoped_agent_id = governance_agent_scope_key(tenant_context, agent_id);
                    if guard.is_agent_spend_paused(&scoped_agent_id)
                        && !guard.has_approved_agent_quota_override(&scoped_agent_id)
                    {
                        return Err(GovernanceError::too_many_requests(
                            "AUTOMATION_V2_AGENT_SPEND_CAP_EXCEEDED",
                            "this agent is paused after reaching its spend cap",
                        ));
                    }
                    snapshot
                        .spend_paused_agents
                        .retain(|paused_agent_id| paused_agent_id != agent_id);
                }
            }
            snapshot
        };
        self.governance_engine.authorize_create(
            &snapshot,
            actor,
            provenance,
            declared_capabilities,
            now_ms(),
        )
    }

    pub async fn can_escalate_declared_capabilities(
        &self,
        actor: &GovernanceActorRef,
        previous: &AutomationDeclaredCapabilities,
        next: &AutomationDeclaredCapabilities,
    ) -> Result<(), GovernanceError> {
        let snapshot = {
            let guard = self.automation_governance.read().await;
            self.governance_snapshot(&guard)
        };
        self.governance_engine.authorize_capability_escalation(
            &snapshot,
            actor,
            previous,
            next,
            now_ms(),
        )
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
        self.governance_engine
            .authorize_mutation(&record, actor, destructive)?;
        Ok(record)
    }

    pub async fn record_automation_creation(
        &self,
        automation: &crate::AutomationV2Spec,
        provenance: AutomationProvenanceRecord,
    ) -> anyhow::Result<AutomationGovernanceRecord> {
        let mut record = new_governance_record(
            automation.automation_id.clone(),
            provenance,
            declared_capabilities_for_automation(automation),
            automation.created_at_ms,
            now_ms(),
        );
        if record.expires_at_ms.is_none() {
            let limits = self.automation_governance.read().await.limits.clone();
            record.expires_at_ms = self.governance_engine.default_automation_expiry(
                &limits,
                &record.provenance,
                now_ms(),
            );
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
        append_protected_audit_event(
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
        .await?;
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
        append_protected_audit_event(
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
        .await?;
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
        tenant_context: &tandem_types::TenantContext,
    ) -> anyhow::Result<GovernanceApprovalRequest> {
        let now = now_ms();
        let request = {
            let mut guard = self.automation_governance.write().await;
            if super::governance_action_gate::is_action_gate_context(&context) {
                if let Some(existing) = guard.approvals.values().find(|request| {
                    request.request_type == request_type
                        && super::governance_action_gate::same_action_gate_scope(
                            &request.context,
                            &context,
                        )
                        && approval_receipt_matches_tenant(request, tenant_context)
                }) {
                    return Ok(existing.clone());
                }
            }

            let snapshot = self.governance_snapshot(&guard);
            let mut request = self
                .governance_engine
                .create_approval_request(
                    &snapshot,
                    GovernanceApprovalDraftInput {
                        request_type,
                        requested_by,
                        target_resource,
                        rationale,
                        context,
                        expires_at_ms,
                    },
                    now,
                )
                .map_err(|error| anyhow::anyhow!(error.message))?;
            // CT-09: bind the issuing tenant to the receipt so it cannot later be
            // replayed (approved or revoked) from a different tenant.
            if !tenant_context.is_local_implicit() {
                request.tenant_context = Some(tenant_context.clone());
            }
            guard
                .approvals
                .insert(request.approval_id.clone(), request.clone());
            guard.updated_at_ms = now;
            request
        };
        self.persist_automation_governance().await?;
        append_protected_audit_event(
            self,
            format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.approval.requested"),
            tenant_context,
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
        .await?;
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

    /// GOV-B4: fetch an approval request so callers can enforce a self-review
    /// guard before deciding.
    pub async fn get_governance_approval_request(
        &self,
        approval_id: &str,
    ) -> Option<GovernanceApprovalRequest> {
        self.automation_governance
            .read()
            .await
            .approvals
            .get(approval_id)
            .cloned()
    }

    /// CT-09: tenant-scoped lookup. An explicit (multi-tenant) caller only sees an
    /// approval that is bound to its own tenant; local/single-tenant is a no-op.
    pub async fn get_governance_approval_request_for_tenant(
        &self,
        approval_id: &str,
        tenant_context: &tandem_types::TenantContext,
    ) -> Option<GovernanceApprovalRequest> {
        self.get_governance_approval_request(approval_id)
            .await
            .filter(|request| approval_receipt_matches_tenant(request, tenant_context))
    }

    /// CT-09: tenant-scoped listing so one tenant's approvals are never enumerated
    /// by another. Local/single-tenant returns the full list unchanged.
    pub async fn list_approval_requests_for_tenant(
        &self,
        request_type: Option<GovernanceApprovalRequestType>,
        status: Option<GovernanceApprovalStatus>,
        tenant_context: &tandem_types::TenantContext,
    ) -> Vec<GovernanceApprovalRequest> {
        self.list_approval_requests(request_type, status)
            .await
            .into_iter()
            .filter(|request| approval_receipt_matches_tenant(request, tenant_context))
            .collect()
    }

    pub async fn decide_approval_request(
        &self,
        approval_id: &str,
        reviewer: GovernanceActorRef,
        approved: bool,
        notes: Option<String>,
        tenant_context: &tandem_types::TenantContext,
    ) -> anyhow::Result<Option<GovernanceApprovalRequest>> {
        let existing = {
            let guard = self.automation_governance.read().await;
            let Some(request) = guard.approvals.get(approval_id).cloned() else {
                return Ok(None);
            };
            request
        };
        // CT-09: reject cross-tenant receipt replay. A receipt issued in tenant A
        // must not be approved or revoked from tenant B. The denial is audited under
        // the requesting tenant, and the caller sees the same `None` (HTTP 404) as a
        // missing receipt so existence is not leaked across the tenant boundary.
        if !approval_receipt_matches_tenant(&existing, tenant_context) {
            append_protected_audit_event(
                self,
                format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.approval.cross_tenant_denied"),
                tenant_context,
                reviewer
                    .actor_id
                    .clone()
                    .or_else(|| reviewer.source.clone()),
                json!({
                    "approvalID": approval_id,
                    "decision": if approved { "approve" } else { "deny" },
                    "reason": "cross_tenant_receipt_replay",
                }),
            )
            .await?;
            return Ok(None);
        }
        let stored = self
            .governance_engine
            .decide_approval_request(
                &existing,
                reviewer.clone(),
                approved,
                notes.clone(),
                now_ms(),
            )
            .map_err(|error| anyhow::anyhow!(error.message))?;
        {
            let mut guard = self.automation_governance.write().await;
            guard
                .approvals
                .insert(approval_id.to_string(), stored.clone());
            guard.updated_at_ms = now_ms();
        }
        self.persist_automation_governance().await?;
        append_protected_audit_event(
            self,
            format!(
                "{GOVERNANCE_AUDIT_EVENT_PREFIX}.approval.{}",
                if approved { "approved" } else { "denied" }
            ),
            tenant_context,
            reviewer
                .actor_id
                .clone()
                .or_else(|| reviewer.source.clone()),
            json!({
                "approvalID": approval_id,
                "approval": stored,
            }),
        )
        .await?;
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
                    .or_insert_with(|| {
                        new_governance_record(
                            automation_id.to_string(),
                            default_human_provenance(
                                Some(automation.creator_id.clone()),
                                "delete_default",
                            ),
                            declared_capabilities_for_automation(&automation),
                            automation.created_at_ms,
                            now,
                        )
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
            append_protected_audit_event(
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
            .await?;
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

    pub async fn tenant_agent_spend_summary(
        &self,
        tenant_context: &tandem_types::TenantContext,
        agent_id: &str,
    ) -> Option<AgentSpendSummary> {
        let scoped_agent_id = governance_agent_scope_key(tenant_context, agent_id);
        self.automation_governance
            .read()
            .await
            .agent_spend_summary(&scoped_agent_id)
    }

    pub async fn tenant_agent_has_quota_override(
        &self,
        tenant_context: &tandem_types::TenantContext,
        agent_id: &str,
    ) -> bool {
        let scoped_agent_id = governance_agent_scope_key(tenant_context, agent_id);
        self.automation_governance
            .read()
            .await
            .has_approved_agent_quota_override(&scoped_agent_id)
    }

    pub async fn tenant_agent_spend_paused_without_quota_override(
        &self,
        tenant_context: &tandem_types::TenantContext,
        agent_id: &str,
    ) -> bool {
        let scoped_agent_id = governance_agent_scope_key(tenant_context, agent_id);
        let governance = self.automation_governance.read().await;
        governance.is_agent_spend_paused(&scoped_agent_id)
            && !governance.has_approved_agent_quota_override(&scoped_agent_id)
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
        let snapshot = {
            let guard = self.automation_governance.read().await;
            self.governance_snapshot(&guard)
        };
        let evaluation = self
            .governance_engine
            .evaluate_creation_review_progress(&snapshot, agent_id, automation_id, now)
            .map_err(|error| anyhow::anyhow!(error.message))?;
        let approval = evaluation.approval_request.clone();
        {
            let mut guard = self.automation_governance.write().await;
            guard
                .agent_creation_reviews
                .insert(agent_id.to_string(), evaluation.summary);
            if let Some(approval) = approval.clone() {
                guard
                    .approvals
                    .insert(approval.approval_id.clone(), approval);
            }
            guard.updated_at_ms = now;
        }
        self.persist_automation_governance().await?;
        if let Some(approval) = approval {
            append_protected_audit_event(
                self,
                format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.approval.requested"),
                &tandem_types::TenantContext::local_implicit(),
                approval
                    .requested_by
                    .actor_id
                    .clone()
                    .or_else(|| approval.requested_by.source.clone()),
                json!({
                    "approvalID": approval.approval_id,
                    "request": approval,
                }),
            )
            .await?;
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
            let existing = guard.agent_creation_reviews.get(agent_id).cloned();
            let summary = self.governance_engine.acknowledge_creation_review(
                existing,
                agent_id,
                notes.clone(),
                now,
            );
            guard
                .agent_creation_reviews
                .insert(agent_id.to_string(), summary);
            guard.updated_at_ms = now;
        }
        self.persist_automation_governance().await?;
        append_protected_audit_event(
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
        .await?;
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
                let Some(record) = guard.records.get(automation_id) else {
                    return Ok(None);
                };
                let updated = self
                    .governance_engine
                    .acknowledge_automation_review(record, now_ms());
                guard
                    .records
                    .insert(automation_id.to_string(), updated.clone());
                updated
            };
            guard.updated_at_ms = now_ms();
            stored
        };
        self.persist_automation_governance().await?;
        append_protected_audit_event(
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
        .await?;
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
        let (evaluation, created_review_id) = {
            let guard = self.automation_governance.read().await;
            let snapshot = self.governance_snapshot(&guard);
            let current_record = guard.records.get(automation_id).cloned();
            let evaluation = self
                .governance_engine
                .evaluate_dependency_revocation(
                    &snapshot,
                    GovernanceDependencyRevocationInput {
                        automation_id: automation_id.to_string(),
                        current_record,
                        default_provenance: default_human_provenance(
                            Some(automation.creator_id.clone()),
                            "dependency_revocation_default",
                        ),
                        declared_capabilities: declared_capabilities_for_automation(&automation),
                        reason: reason.clone(),
                        evidence: dependency_context.clone(),
                    },
                    now,
                )
                .map_err(|error| anyhow::anyhow!(error.message))?;
            let created_review_id = evaluation
                .approval_request
                .as_ref()
                .map(|approval| approval.approval_id.clone())
                .or_else(|| evaluation.record.review_request_id.clone());
            (evaluation, created_review_id)
        };
        {
            let mut guard = self.automation_governance.write().await;
            guard
                .records
                .insert(automation_id.to_string(), evaluation.record.clone());
            if let Some(approval) = evaluation.approval_request.clone() {
                guard
                    .approvals
                    .insert(approval.approval_id.clone(), approval);
            }
            guard.updated_at_ms = now;
        }
        self.persist_automation_governance().await?;
        if let Some(approval) = evaluation.approval_request {
            append_protected_audit_event(
                self,
                format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.approval.requested"),
                &tandem_types::TenantContext::local_implicit(),
                approval
                    .requested_by
                    .actor_id
                    .clone()
                    .or_else(|| approval.requested_by.source.clone()),
                json!({
                    "approvalID": approval.approval_id,
                    "request": approval,
                }),
            )
            .await?;
        }

        append_protected_audit_event(
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
        .await?;

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
        let evaluation = {
            let guard = self.automation_governance.read().await;
            let snapshot = self.governance_snapshot(&guard);
            self.governance_engine
                .evaluate_run_review_progress(
                    &snapshot,
                    automation_id,
                    reason,
                    run_id.clone(),
                    detail.clone(),
                    now,
                )
                .map_err(|error| anyhow::anyhow!(error.message))?
        };
        let Some(evaluation) = evaluation else {
            return Ok(());
        };
        let approval = evaluation.approval_request.clone();
        {
            let mut guard = self.automation_governance.write().await;
            guard
                .records
                .insert(automation_id.to_string(), evaluation.record);
            if let Some(approval) = approval.clone() {
                guard
                    .approvals
                    .insert(approval.approval_id.clone(), approval);
            }
            guard.updated_at_ms = now;
        }
        self.persist_automation_governance().await?;
        if let Some(approval) = approval {
            append_protected_audit_event(
                self,
                format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.approval.requested"),
                &tandem_types::TenantContext::local_implicit(),
                approval
                    .requested_by
                    .actor_id
                    .clone()
                    .or_else(|| approval.requested_by.source.clone()),
                json!({
                    "approvalID": approval.approval_id,
                    "request": approval,
                }),
            )
            .await?;
        }
        Ok(())
    }

    pub async fn run_automation_governance_health_check(&self) -> anyhow::Result<usize> {
        if !self.premium_governance_enabled() {
            return Ok(0);
        }
        let now = now_ms();
        let limits = self.automation_governance.read().await.limits.clone();
        let automations = self.list_automations_v2().await;
        let mut finding_count = 0usize;

        let run_window = self.governance_engine.health_check_run_window(&limits);
        for automation in automations {
            let runs = self
                .list_automation_v2_runs(Some(&automation.automation_id), run_window)
                .await;
            let observations = runs
                .iter()
                .map(|run| GovernanceRunHealthObservation {
                    run_id: run.run_id.clone(),
                    status: governance_run_health_status(&run.status),
                    produced_node_outputs: !run.checkpoint.node_outputs.is_empty(),
                    guardrail_stopped: run.stop_kind
                        == Some(crate::AutomationStopKind::GuardrailStopped),
                })
                .collect::<Vec<_>>();
            let run_health = self.governance_engine.summarize_run_health(&observations);
            let evaluation = {
                let guard = self.automation_governance.read().await;
                let snapshot = self.governance_snapshot(&guard);
                self.governance_engine
                    .evaluate_health_check(
                        &snapshot,
                        GovernanceHealthCheckInput {
                            automation_id: automation.automation_id.clone(),
                            current_record: guard.records.get(&automation.automation_id).cloned(),
                            default_provenance: default_human_provenance(
                                Some(automation.creator_id.clone()),
                                "health_check_default",
                            ),
                            declared_capabilities: declared_capabilities_for_automation(
                                &automation,
                            ),
                            terminal_run_count: run_health.terminal_run_count,
                            failure_count: run_health.failure_count,
                            empty_output_count: run_health.empty_output_count,
                            guardrail_stop_count: run_health.guardrail_stop_count,
                            last_terminal_run_id: run_health.last_terminal_run_id.clone(),
                        },
                        now,
                    )
                    .map_err(|error| anyhow::anyhow!(error.message))?
            };
            let Some(evaluation) = evaluation else {
                continue;
            };
            {
                let mut guard = self.automation_governance.write().await;
                guard
                    .records
                    .insert(automation.automation_id.clone(), evaluation.record.clone());
                for approval in &evaluation.approval_requests {
                    guard
                        .approvals
                        .insert(approval.approval_id.clone(), approval.clone());
                }
                guard.updated_at_ms = now;
            }
            self.persist_automation_governance().await?;

            if evaluation.pause_automation && automation.status != crate::AutomationV2Status::Paused
            {
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

            for approval in &evaluation.approval_requests {
                append_protected_audit_event(
                    self,
                    format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.approval.requested"),
                    &tandem_types::TenantContext::local_implicit(),
                    approval
                        .requested_by
                        .actor_id
                        .clone()
                        .or_else(|| approval.requested_by.source.clone()),
                    json!({
                        "approvalID": approval.approval_id,
                        "request": approval,
                    }),
                )
                .await?;
            }

            finding_count += evaluation.record.health_findings.len();
        }

        Ok(finding_count)
    }

}

include!("governance_parts/part01.rs");
