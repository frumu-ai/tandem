// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn governance_tenant_scope(
    tenant_context: &tandem_types::TenantContext,
) -> tandem_types::TenantContext {
    let mut scope = tenant_context.clone();
    scope.actor_id = None;
    scope
}

fn governance_tenant_matches(
    left: &tandem_types::TenantContext,
    right: &tandem_types::TenantContext,
) -> bool {
    left.org_id == right.org_id
        && left.workspace_id == right.workspace_id
        && left.deployment_id == right.deployment_id
}

fn quarantine_governance_record_for_tenant(
    record: &mut AutomationGovernanceRecord,
    tenant_context: &tandem_types::TenantContext,
    observed_at_ms: u64,
) {
    let foreign_tenant = record.tenant_context.as_ref().map(governance_tenant_scope);
    let foreign_grant_tenants = record
        .modify_grants
        .iter()
        .chain(record.capability_grants.iter())
        .filter_map(|grant| grant.tenant_context.as_ref().map(governance_tenant_scope))
        .collect::<Vec<_>>();
    let cleared_modify_grants = record.modify_grants.len();
    let cleared_capability_grants = record.capability_grants.len();

    record.tenant_context = Some(governance_tenant_scope(tenant_context));
    record.modify_grants.clear();
    record.capability_grants.clear();
    record.creation_paused = true;
    record.paused_for_lifecycle = true;
    record.review_required = true;
    record.review_kind = Some(AutomationLifecycleReviewKind::TenantOwnershipMismatch);
    record.review_requested_at_ms = Some(observed_at_ms);
    record.review_request_id = None;
    record.health_last_checked_at_ms = Some(observed_at_ms);
    record
        .health_findings
        .retain(|finding| finding.kind != AutomationLifecycleReviewKind::TenantOwnershipMismatch);
    record.health_findings.push(AutomationLifecycleFinding {
        finding_id: format!("tenant-ownership-mismatch:{}", record.automation_id),
        kind: AutomationLifecycleReviewKind::TenantOwnershipMismatch,
        severity: AutomationLifecycleFindingSeverity::Critical,
        summary: "Governance tenant ownership mismatch quarantined".to_string(),
        detail: Some("Foreign governance state was detached; grants were cleared and execution remains paused pending independent review.".to_string()),
        observed_at_ms,
        automation_run_id: None,
        approval_id: None,
        evidence: Some(json!({
            "foreignTenant": foreign_tenant,
            "foreignGrantTenants": foreign_grant_tenants,
            "clearedModifyGrants": cleared_modify_grants,
            "clearedCapabilityGrants": cleared_capability_grants,
        })),
    });
    record.updated_at_ms = observed_at_ms;
}

impl AppState {
    pub async fn ensure_automation_governance_run_allowed(
        &self,
        automation: &crate::AutomationV2Spec,
    ) -> anyhow::Result<()> {
        if !self.premium_governance_enabled() {
            return Ok(());
        }
        let tenant_context = automation.tenant_context();
        let record = self
            .get_or_bootstrap_automation_governance(automation)
            .await;
        if !governance_record_owned_by(&record, &tenant_context)
            || record.creation_paused
            || record.paused_for_lifecycle
        {
            anyhow::bail!(
                "automation governance is quarantined or paused pending independent review"
            );
        }
        Ok(())
    }

    pub async fn can_mutate_automation(
        &self,
        automation_id: &str,
        actor: &GovernanceActorRef,
        destructive: bool,
        tenant_context: &tandem_types::TenantContext,
    ) -> Result<AutomationGovernanceRecord, GovernanceError> {
        let guard = self.automation_governance.read().await;
        let Some(record) = guard.records.get(automation_id).cloned() else {
            return Err(GovernanceError::forbidden(
                "AUTOMATION_V2_GOVERNANCE_MISSING",
                "automation governance record not found",
            ));
        };
        if !governance_record_owned_by(&record, tenant_context) {
            return Err(GovernanceError::forbidden(
                "AUTOMATION_V2_TENANT_MISMATCH",
                "automation governance record not found",
            ));
        }
        if record.review_required
            && record.review_kind
                == Some(AutomationLifecycleReviewKind::TenantOwnershipMismatch)
        {
            return Err(GovernanceError::forbidden(
                "AUTOMATION_V2_TENANT_QUARANTINED",
                "automation governance is quarantined pending independent review",
            ));
        }
        self.governance_engine
            .authorize_mutation(&record, actor, destructive)?;
        Ok(record)
    }

    pub async fn retire_automation_v2(
        &self,
        automation_id: &str,
        actor: GovernanceActorRef,
        reason: Option<String>,
        approval_id: Option<String>,
        tenant_context: &tandem_types::TenantContext,
    ) -> anyhow::Result<Option<crate::AutomationV2Spec>> {
        let mut automation = self
            .require_active_automation_governance_tenant(automation_id, tenant_context)
            .await?;
        let now = now_ms();
        let reason = reason.unwrap_or_else(|| "retired by operator".to_string());
        append_protected_audit_event(
            self,
            format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.retired"),
            tenant_context,
            actor.actor_id.clone().or_else(|| actor.source.clone()),
            json!({
                "automationID": automation_id,
                "reason": reason,
                "actor": actor,
                "approvalID": approval_id,
            }),
        )
        .await?;
        automation = self
            .require_active_automation_governance_tenant(automation_id, tenant_context)
            .await?;
        automation.status = crate::AutomationV2Status::Paused;
        let stored = self.put_automation_v2(automation).await?;
        let _ = self
            .pause_running_automation_v2_runs(
                automation_id,
                reason.clone(),
                crate::AutomationStopKind::OperatorStopped,
            )
            .await;
        let current_record = self.get_automation_governance(automation_id).await;
        let mut record = self
            .governance_engine
            .evaluate_retirement(
                GovernanceRetirementInput {
                    automation_id: automation_id.to_string(),
                    current_record,
                    default_provenance: default_human_provenance(
                        Some(stored.creator_id.clone()),
                        "retire_default",
                    ),
                    declared_capabilities: declared_capabilities_for_automation(&stored),
                    reason,
                },
                now,
            )
            .map_err(|error| anyhow::anyhow!(error.message))?;
        bind_governance_record_to_tenant(&mut record, tenant_context)?;
        {
            let mut guard = self.automation_governance.write().await;
            guard.records.insert(automation_id.to_string(), record);
            guard.updated_at_ms = now;
        }
        self.persist_automation_governance().await?;
        Ok(Some(stored))
    }

    pub async fn extend_automation_v2_retirement(
        &self,
        automation_id: &str,
        actor: GovernanceActorRef,
        expires_at_ms: Option<u64>,
        reason: Option<String>,
        approval_id: Option<String>,
        tenant_context: &tandem_types::TenantContext,
    ) -> anyhow::Result<Option<crate::AutomationV2Spec>> {
        let mut automation = self
            .require_active_automation_governance_tenant(automation_id, tenant_context)
            .await?;
        let now = now_ms();
        let default_expires_after_ms = self
            .automation_governance
            .read()
            .await
            .limits
            .default_expires_after_ms;
        let next_expires_at_ms =
            expires_at_ms.unwrap_or_else(|| now.saturating_add(default_expires_after_ms.max(1)));
        append_protected_audit_event(
            self,
            format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.retirement.extended"),
            tenant_context,
            actor.actor_id.clone().or_else(|| actor.source.clone()),
            json!({
                "automationID": automation_id,
                "expiresAtMs": next_expires_at_ms,
                "reason": reason,
                "actor": actor,
                "approvalID": approval_id,
            }),
        )
        .await?;
        automation = self
            .require_active_automation_governance_tenant(automation_id, tenant_context)
            .await?;
        automation.status = crate::AutomationV2Status::Active;
        let stored = self.put_automation_v2(automation).await?;
        let current_record = self.get_automation_governance(automation_id).await;
        let mut record = self
            .governance_engine
            .evaluate_retirement_extension(
                GovernanceRetirementExtensionInput {
                    automation_id: automation_id.to_string(),
                    current_record,
                    default_provenance: default_human_provenance(
                        Some(stored.creator_id.clone()),
                        "extend_default",
                    ),
                    declared_capabilities: declared_capabilities_for_automation(&stored),
                    expires_at_ms: next_expires_at_ms,
                },
                now,
            )
            .map_err(|error| anyhow::anyhow!(error.message))?;
        bind_governance_record_to_tenant(&mut record, tenant_context)?;
        {
            let mut guard = self.automation_governance.write().await;
            guard.records.insert(automation_id.to_string(), record);
            guard.updated_at_ms = now;
        }
        self.persist_automation_governance().await?;
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
        let tenant_context = automation.tenant_context();
        let (scoped_agent_ids, raw_to_scoped, scoped_to_raw) =
            scoped_agent_id_maps(&tenant_context, &agent_ids);

        let now = now_ms();
        let snapshot = {
            let guard = self.automation_governance.read().await;
            self.governance_snapshot(&guard)
        };
        let evaluation = self
            .governance_engine
            .evaluate_spend_usage(
                &snapshot,
                &GovernanceSpendInput {
                    automation_id: automation.automation_id.clone(),
                    run_id: run_id.to_string(),
                    agent_ids: scoped_agent_ids.clone(),
                    prompt_tokens,
                    completion_tokens,
                    total_tokens,
                    delta_cost_usd,
                },
                now,
            )
            .map_err(|error| anyhow::anyhow!(error.message))?;
        {
            let mut guard = self.automation_governance.write().await;
            for summary in &evaluation.updated_summaries {
                let storage_agent_id =
                    scoped_agent_id_for_storage(&summary.agent_id, &raw_to_scoped);
                let mut stored_summary = summary.clone();
                stored_summary.agent_id = display_agent_id(&summary.agent_id, &scoped_to_raw);
                guard.agent_spend.insert(storage_agent_id, stored_summary);
            }
            for agent_id in &evaluation.spend_paused_agents {
                if !guard
                    .spend_paused_agents
                    .iter()
                    .any(|value| value == agent_id)
                {
                    guard.spend_paused_agents.push(agent_id.clone());
                }
            }
            for approval in &evaluation.approvals {
                guard
                    .approvals
                    .insert(approval.approval_id.clone(), approval.clone());
            }
            guard.updated_at_ms = now;
        }
        self.persist_automation_governance().await?;

        for warning in &evaluation.warnings {
            append_protected_audit_event(
                self,
                format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.spend.warning"),
                &tenant_context,
                governance
                    .provenance
                    .creator
                    .actor_id
                    .clone()
                    .or_else(|| Some(automation.creator_id.clone())),
                json!({
                    "automationID": automation.automation_id,
                    "runID": run_id,
                    "agentID": display_agent_id(&warning.agent_id, &scoped_to_raw),
                    "weeklyCostUsd": warning.weekly_cost_usd,
                    "weeklySpendCapUsd": warning.weekly_spend_cap_usd,
                }),
            )
            .await?;
        }

        let requested_approvals = evaluation
            .approvals
            .iter()
            .map(|approval| approval.approval_id.clone())
            .collect::<Vec<_>>();
        for approval in &evaluation.approvals {
            append_protected_audit_event(
                self,
                format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.approval.requested"),
                &tenant_context,
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

        if !evaluation.hard_stops.is_empty() {
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
            let paused_agent_labels = evaluation
                .hard_stops
                .iter()
                .map(|entry| {
                    let agent_id = display_agent_id(&entry.agent_id, &scoped_to_raw);
                    format!(
                        "{} ({:.4}/{:.4} USD)",
                        agent_id, entry.weekly_cost_usd, entry.weekly_spend_cap_usd
                    )
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
            append_protected_audit_event(
                self,
                format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.spend.paused"),
                &tenant_context,
                governance
                    .provenance
                    .creator
                    .actor_id
                    .clone()
                    .or_else(|| Some(automation.creator_id.clone())),
                json!({
                    "automationID": automation.automation_id,
                    "runID": run_id,
                    "pausedAgents": evaluation
                        .hard_stops
                        .iter()
                        .map(|entry| display_agent_id(&entry.agent_id, &scoped_to_raw))
                        .collect::<Vec<_>>(),
                    "requestedApprovals": requested_approvals,
                    "detail": detail,
                }),
            )
            .await?;
        }

        Ok(())
    }
}
