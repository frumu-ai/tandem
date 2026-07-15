// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde_json::{json, Value};

use crate::audit::append_protected_audit_event;
use crate::automation_v2::governance::GovernanceApprovalStatus;
use crate::{now_ms, AppState};

use super::governance::approval_receipt_matches_tenant;

fn action_gate_field<'a>(context: &'a Value, field: &str) -> Option<&'a Value> {
    context.get("action_gate")?.get(field)
}

pub(super) fn is_action_gate_context(context: &Value) -> bool {
    action_gate_field(context, "action_hash")
        .and_then(Value::as_str)
        .is_some()
}

pub(super) fn same_action_gate_scope(left: &Value, right: &Value) -> bool {
    [
        "action_hash",
        "session_id",
        "message_id",
        "run_id",
        "policy_id",
        "policy_version_id",
    ]
    .into_iter()
    .all(|field| action_gate_field(left, field) == action_gate_field(right, field))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActionGateApprovalState {
    Pending,
    ApprovedAndConsumed,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EgressDlpApprovalReceipt {
    pub(crate) approval_id: String,
    pub(crate) expires_at_ms: u64,
    pub(crate) state: ActionGateApprovalState,
}

impl AppState {
    pub(crate) async fn consume_egress_dlp_approval(
        &self,
        action_hash: &str,
        tool: &str,
        tenant_context: &tandem_types::TenantContext,
    ) -> anyhow::Result<Option<EgressDlpApprovalReceipt>> {
        let now = now_ms();
        let (receipt, consumed) = {
            let mut guard = self.automation_governance.write().await;
            let approval_id = guard
                .approvals
                .values()
                .filter(|request| {
                    request.context.get("policy_id").and_then(Value::as_str)
                        == Some("egress_dlp_preflight")
                        && request.context.get("action_hash").and_then(Value::as_str)
                            == Some(action_hash)
                        && request.context.get("tool").and_then(Value::as_str) == Some(tool)
                        && approval_receipt_matches_tenant(request, tenant_context)
                })
                .max_by_key(|request| request.created_at_ms)
                .map(|request| request.approval_id.clone());
            let Some(approval_id) = approval_id else {
                return Ok(None);
            };
            let (state, expires_at_ms) = {
                let request = guard
                    .approvals
                    .get_mut(&approval_id)
                    .ok_or_else(|| anyhow::anyhow!("egress DLP approval request disappeared"))?;
                let state = match request.status {
                    GovernanceApprovalStatus::Pending => ActionGateApprovalState::Pending,
                    GovernanceApprovalStatus::Denied | GovernanceApprovalStatus::Expired => {
                        ActionGateApprovalState::Denied
                    }
                    GovernanceApprovalStatus::Approved => {
                        if now >= request.expires_at_ms
                            || request
                                .context
                                .pointer("/egress_dlp/consumed_at_ms")
                                .is_some()
                        {
                            ActionGateApprovalState::Denied
                        } else {
                            request.context["egress_dlp"] = json!({ "consumed_at_ms": now });
                            request.updated_at_ms = now;
                            ActionGateApprovalState::ApprovedAndConsumed
                        }
                    }
                };
                (state, request.expires_at_ms)
            };
            if state == ActionGateApprovalState::ApprovedAndConsumed {
                guard.updated_at_ms = now;
            }
            (
                EgressDlpApprovalReceipt {
                    approval_id,
                    expires_at_ms,
                    state,
                },
                state == ActionGateApprovalState::ApprovedAndConsumed,
            )
        };

        if consumed {
            self.persist_automation_governance().await?;
            append_protected_audit_event(
                self,
                "automation.governance.egress_approval.consumed",
                tenant_context,
                tenant_context.actor_id.clone(),
                json!({
                    "approvalID": receipt.approval_id,
                    "actionHash": action_hash,
                    "tool": tool,
                }),
            )
            .await?;
        }
        Ok(Some(receipt))
    }

    pub(crate) async fn consume_action_gate_approval(
        &self,
        approval_id: &str,
        tenant_context: &tandem_types::TenantContext,
    ) -> anyhow::Result<ActionGateApprovalState> {
        let now = now_ms();
        let resolution = {
            let mut guard = self.automation_governance.write().await;
            let request = guard
                .approvals
                .get_mut(approval_id)
                .filter(|request| approval_receipt_matches_tenant(request, tenant_context))
                .ok_or_else(|| anyhow::anyhow!("action-gate approval request not found"))?;

            match request.status {
                GovernanceApprovalStatus::Pending => ActionGateApprovalState::Pending,
                GovernanceApprovalStatus::Denied | GovernanceApprovalStatus::Expired => {
                    ActionGateApprovalState::Denied
                }
                GovernanceApprovalStatus::Approved => {
                    if now >= request.expires_at_ms
                        || request
                            .context
                            .pointer("/action_gate/consumed_at_ms")
                            .is_some()
                    {
                        ActionGateApprovalState::Denied
                    } else {
                        request.context["action_gate"]["consumed_at_ms"] = json!(now);
                        request.updated_at_ms = now;
                        guard.updated_at_ms = now;
                        ActionGateApprovalState::ApprovedAndConsumed
                    }
                }
            }
        };

        if resolution == ActionGateApprovalState::ApprovedAndConsumed {
            self.persist_automation_governance().await?;
            append_protected_audit_event(
                self,
                "automation.governance.approval.consumed",
                tenant_context,
                tenant_context.actor_id.clone(),
                json!({ "approvalID": approval_id }),
            )
            .await?;
        }
        Ok(resolution)
    }
}
