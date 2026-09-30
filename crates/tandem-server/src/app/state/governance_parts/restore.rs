// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

// Included by governance.rs. A pending restore is deliberately still a
// tombstone: a staged definition is not live until its protected audit and the
// final governance snapshot are durable.

fn pending_restore_actor_matches(
    pending: &PendingAutomationRestore,
    actor: &GovernanceActorRef,
) -> bool {
    pending.restored_by.kind == actor.kind
        && match (
            pending.restored_by.actor_id.as_deref(),
            actor.actor_id.as_deref(),
        ) {
            (Some(left), Some(right)) => left.eq_ignore_ascii_case(right),
            (None, None) => pending.restored_by.source == actor.source,
            _ => false,
        }
}

fn pending_restore_matches_request(
    pending: &PendingAutomationRestore,
    actor: &GovernanceActorRef,
    approval_id: Option<&str>,
    reservation_id: Option<&str>,
    tenant_context: &tandem_types::TenantContext,
) -> bool {
    pending.tenant_context == *tenant_context
        && pending_restore_actor_matches(pending, actor)
        && pending.approval_id.as_deref() == approval_id
        && pending.reservation_id.as_deref() == reservation_id
}

fn restore_reservation_matches(
    governance: &GovernanceState,
    automation_id: &str,
    pending: &PendingAutomationRestore,
    require_fresh_approval: bool,
) -> bool {
    let (Some(approval_id), Some(reservation_id)) = (
        pending.approval_id.as_deref(),
        pending.reservation_id.as_deref(),
    ) else {
        return pending.approval_id.is_none() && pending.reservation_id.is_none();
    };
    let Some(approval) = governance.approvals.get(approval_id) else {
        return false;
    };
    let actor_id = pending.restored_by.actor_id.as_deref();
    let requester_id = approval.requested_by.actor_id.as_deref();
    let reviewer_id = approval
        .reviewed_by
        .as_ref()
        .and_then(|reviewer| reviewer.actor_id.as_deref());
    let reservation = approval.context.get(MUTATION_APPROVAL_RESERVATION_KEY);
    approval_receipt_matches_tenant(approval, &pending.tenant_context)
        && approval.status == GovernanceApprovalStatus::Approved
        && (!require_fresh_approval || now_ms() < approval.expires_at_ms)
        && matches!(
            approval.request_type,
            GovernanceApprovalRequestType::RetirementAction
                | GovernanceApprovalRequestType::LifecycleReview
        )
        && matches!(
            approval.target_resource.resource_type.as_str(),
            "automation" | "automation_v2"
        )
        && approval.target_resource.id == automation_id
        && approval.context.get("action").and_then(Value::as_str) == Some("restore_automation")
        && approval.context.get("parameters") == Some(&json!({}))
        && approval
            .context
            .get(MUTATION_APPROVAL_CONSUMPTION_KEY)
            .is_none()
        && actor_id.is_some_and(|actor_id| {
            requester_id.is_some_and(|requester| requester.eq_ignore_ascii_case(actor_id))
                && reviewer_id.is_some_and(|reviewer| !reviewer.eq_ignore_ascii_case(actor_id))
        })
        && reservation
            .and_then(|value| value.get("reservationID"))
            .and_then(Value::as_str)
            == Some(reservation_id)
        && reservation
            .and_then(|value| value.get("action"))
            .and_then(Value::as_str)
            == Some("restore_automation")
        && reservation
            .and_then(|value| value.get("actorID"))
            .and_then(Value::as_str)
            .zip(actor_id)
            .is_some_and(|(reserved, actor)| reserved.eq_ignore_ascii_case(actor))
}

impl AppState {
    /// Only an exact continuation may reuse a hosted approval reservation.
    /// The caller still has to pass the ordinary route admin and mutation gates.
    pub(crate) async fn pending_restore_matches_approval(
        &self,
        automation_id: &str,
        actor: &GovernanceActorRef,
        approval_id: &str,
        reservation_id: &str,
        tenant_context: &tandem_types::TenantContext,
    ) -> bool {
        let governance = self.automation_governance.read().await;
        let Some(pending) = governance
            .deleted_automations
            .get(automation_id)
            .and_then(|deleted| deleted.pending_restore.as_ref())
        else {
            return false;
        };
        pending_restore_matches_request(
            pending,
            actor,
            Some(approval_id),
            Some(reservation_id),
            tenant_context,
        ) && restore_reservation_matches(&governance, automation_id, pending, false)
    }

    pub async fn restore_deleted_automation_v2_with_reservation<F>(
        &self,
        automation_id: &str,
        restored_by: GovernanceActorRef,
        approval_id: Option<String>,
        reservation_id: Option<String>,
        tenant_context: &tandem_types::TenantContext,
        authorize: F,
    ) -> anyhow::Result<Option<crate::AutomationV2Spec>>
    where
        F: Fn() -> anyhow::Result<()> + Send + Sync,
    {
        let _persistence_guard = self.automations_v2_persistence.lock().await;
        let visible = {
            let governance = self.automation_governance.read().await;
            governance
                .deleted_automations
                .get(automation_id)
                .is_some_and(|deleted| {
                    governance_tenant_matches(&deleted.automation.tenant_context(), tenant_context)
                })
                && governance
                    .records
                    .get(automation_id)
                    .is_some_and(|record| governance_record_owned_by(record, tenant_context))
        };
        if !visible {
            return Ok(None);
        }
        if self.automations_v2.read().await.contains_key(automation_id) {
            anyhow::bail!("automation id already exists");
        }
        let (restored, pending) = {
            let mut governance = self.automation_governance.write().await;
            let Some(deleted) = governance.deleted_automations.get(automation_id) else {
                return Ok(None);
            };
            let restored = deleted.automation.clone();
            if !governance_tenant_matches(&restored.tenant_context(), tenant_context)
                || !governance
                    .records
                    .get(automation_id)
                    .is_some_and(|record| governance_record_owned_by(record, tenant_context))
            {
                return Ok(None);
            }
            authorize()?;
            let pending = if let Some(pending) = deleted.pending_restore.clone() {
                anyhow::ensure!(
                    pending_restore_matches_request(
                        &pending,
                        &restored_by,
                        approval_id.as_deref(),
                        reservation_id.as_deref(),
                        tenant_context,
                    ),
                    "pending automation restore belongs to a different request"
                );
                anyhow::ensure!(
                    restore_reservation_matches(&governance, automation_id, &pending, false),
                    "pending automation restore approval reservation is no longer valid"
                );
                pending
            } else {
                anyhow::ensure!(
                    approval_id.is_some() == reservation_id.is_some(),
                    "restore approval and reservation must be provided together"
                );
                let pending = PendingAutomationRestore {
                    operation_id: Uuid::new_v4().to_string(),
                    restored_by: restored_by.clone(),
                    tenant_context: tenant_context.clone(),
                    approval_id: approval_id.clone(),
                    reservation_id: reservation_id.clone(),
                };
                anyhow::ensure!(
                    restore_reservation_matches(&governance, automation_id, &pending, true),
                    "restore approval reservation is not valid for this request"
                );
                let previous = governance.clone();
                governance
                    .deleted_automations
                    .get_mut(automation_id)
                    .expect("validated tombstone remains present")
                    .pending_restore = Some(pending.clone());
                governance.updated_at_ms = now_ms();
                if let Err(error) = self
                    .persist_automation_governance_snapshot(&governance)
                    .await
                {
                    *governance = previous;
                    return Err(error);
                }
                pending
            };
            (restored, pending)
        };

        // The tombstone remains authoritative both in memory and on disk. A
        // crash here may leave a shard, but startup removes it before Ready.
        authorize()?;
        let mut staged = self.automations_v2.read().await.clone();
        anyhow::ensure!(
            staged
                .insert(automation_id.to_string(), restored.clone())
                .is_none(),
            "automation id already exists"
        );
        super::persist_automation_v2_definition_shards(&self.automations_v2_path, &staged).await?;
        super::archive_automation_v2_aggregate_file(&self.automations_v2_path).await?;
        let _ = super::cleanup_stale_legacy_automations_v2_file(&self.automations_v2_path).await;
        self.verify_automation_v2_persisted_locked(automation_id, true)
            .await?;
        authorize()?;

        crate::audit::append_protected_audit_event_once(
            self,
            &pending.operation_id,
            format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.restored"),
            &pending.tenant_context,
            pending
                .restored_by
                .actor_id
                .clone()
                .or_else(|| pending.restored_by.source.clone()),
            json!({
                "automationID": automation_id,
                "restoredBy": pending.restored_by,
                "approvalID": pending.approval_id,
                "operationID": pending.operation_id,
            }),
        )
        .await?;
        authorize()?;

        if let (Some(approval_id), Some(reservation_id)) = (
            pending.approval_id.as_deref(),
            pending.reservation_id.as_deref(),
        ) {
            crate::audit::append_protected_audit_event_once(
                self,
                &format!("{}:approval-consumed", pending.operation_id),
                format!("{GOVERNANCE_AUDIT_EVENT_PREFIX}.approval.mutation_consumed"),
                &pending.tenant_context,
                pending
                    .restored_by
                    .actor_id
                    .clone()
                    .or_else(|| pending.restored_by.source.clone()),
                json!({
                    "approvalID": approval_id,
                    "reservationID": reservation_id,
                    "consumedBy": pending.restored_by,
                    "operationID": pending.operation_id,
                }),
            )
            .await?;
        }

        // The persistence mutex excludes checked writers. Publish governance
        // first, then the live map: readers may briefly see no definition,
        // but can never see an unaudited active definition. Do not nest the two
        // RwLocks here; governance readers may also inspect automations.
        anyhow::ensure!(
            !self.automations_v2.read().await.contains_key(automation_id),
            "automation id already exists"
        );
        let mut governance = self.automation_governance.write().await;
        authorize()?;
        let current = governance
            .deleted_automations
            .get(automation_id)
            .and_then(|deleted| deleted.pending_restore.as_ref());
        anyhow::ensure!(
            current.is_some_and(|current| current.operation_id == pending.operation_id),
            "pending automation restore changed before commit"
        );
        anyhow::ensure!(
            restore_reservation_matches(&governance, automation_id, &pending, false),
            "restore approval reservation changed before commit"
        );
        let mut next = governance.clone();
        next.deleted_automations.remove(automation_id);
        let record = next
            .records
            .get_mut(automation_id)
            .ok_or_else(|| anyhow::anyhow!("missing automation governance record"))?;
        record.deleted_at_ms = None;
        record.delete_retention_until_ms = None;
        record.updated_at_ms = now_ms();
        if let (Some(approval_id), Some(reservation_id)) = (
            pending.approval_id.as_deref(),
            pending.reservation_id.as_deref(),
        ) {
            let approval = next
                .approvals
                .get_mut(approval_id)
                .expect("validated approval remains present");
            let context = approval
                .context
                .as_object_mut()
                .expect("validated approval context remains an object");
            context.remove(MUTATION_APPROVAL_RESERVATION_KEY);
            context.insert(
                MUTATION_APPROVAL_CONSUMPTION_KEY.to_string(),
                json!({
                    "reservationID": reservation_id,
                    "actorID": pending.restored_by.actor_id,
                    "consumedAtMs": now_ms(),
                }),
            );
            approval.updated_at_ms = now_ms();
        }
        next.updated_at_ms = now_ms();
        self.persist_automation_governance_snapshot(&next).await?;
        *governance = next;
        drop(governance);
        let mut automations = self.automations_v2.write().await;
        anyhow::ensure!(
            !automations.contains_key(automation_id),
            "automation id already exists"
        );
        automations.insert(automation_id.to_string(), restored.clone());
        Ok(Some(restored))
    }

    /// Startup must never publish a staged hosted shard without a fresh
    /// request and current admin authority. Unreviewed local/direct intents can
    /// be replayed after tombstone reconciliation using their original id.
    pub(crate) async fn reconcile_pending_automation_restores(&self) -> anyhow::Result<()> {
        let pending = self
            .automation_governance
            .read()
            .await
            .deleted_automations
            .iter()
            .filter_map(|(id, deleted)| {
                deleted
                    .pending_restore
                    .as_ref()
                    .filter(|pending| pending.approval_id.is_none())
                    .map(|pending| (id.clone(), pending.clone()))
            })
            .collect::<Vec<_>>();
        for (automation_id, pending) in pending {
            self.restore_deleted_automation_v2_with_reservation(
                &automation_id,
                pending.restored_by,
                None,
                None,
                &pending.tenant_context,
                || Ok(()),
            )
            .await?;
        }
        Ok(())
    }
}
