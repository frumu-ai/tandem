// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::AppState;
use crate::{ExternalActionProvenance, ExternalActionRecord};
use tandem_types::{PrincipalRef, TenantContext};

impl AppState {
    /// Resolve receipt authority from persisted source objects, never from
    /// receipt metadata or a caller-supplied tenant label. Unknown and legacy
    /// sources remain unattributed and are not readable in hosted mode.
    pub(super) async fn external_action_provenance(
        &self,
        action: &ExternalActionRecord,
    ) -> Option<ExternalActionProvenance> {
        let tenant_context = match action.source_kind.as_deref()? {
            "automation_v2" => {
                let run_id = action.source_id.as_deref()?.split(':').next()?;
                let run = self.get_automation_v2_run(run_id).await?;
                // A receipt belongs to the automation as it was dispatched.
                // Later edits or ownership transfers must not reattribute a
                // historical run. Only legacy snapshotless runs use the live
                // object as a fallback.
                let automation = if let Some(snapshot) = run.automation_snapshot.clone() {
                    snapshot
                } else {
                    self.get_automation_v2(&run.automation_id).await?
                };
                let automation_tenant = automation.tenant_context();
                if automation.automation_id != run.automation_id
                    || automation_tenant.org_id != run.tenant_context.org_id
                    || automation_tenant.workspace_id != run.tenant_context.workspace_id
                    || automation_tenant.deployment_id != run.tenant_context.deployment_id
                {
                    return None;
                }
                // A run may be triggered by a different human than the owner
                // of its automation. Bind receipts to the persisted object's
                // canonical owner, not to that trigger actor.
                return Some(ExternalActionProvenance {
                    tenant_context: run.tenant_context,
                    owner_principal: crate::http::routines_automations::automation_v2_object_owner(
                        &automation,
                    )
                    .map(PrincipalRef::human_user),
                });
            }
            "workflow" => {
                let run_id = action.source_id.as_deref()?.split(':').next()?;
                let run = self.get_workflow_run(run_id).await?;
                run.tenant_context
            }
            "coder" => {
                let coder_run_id = action.source_id.as_deref()?;
                let context_run_id = action.context_run_id.as_deref()?;
                crate::http::external_action_context_run_tenant(self, coder_run_id, context_run_id)
                    .await?
            }
            "incident_monitor" => {
                let draft_id = action.source_id.as_deref()?;
                let post = self.get_incident_monitor_post(&action.action_id).await?;
                if post.draft_id != draft_id {
                    return None;
                }
                let draft = self.get_incident_monitor_draft(draft_id).await?;
                if post.tenant_id != draft.tenant_id || post.workspace_id != draft.workspace_id {
                    return None;
                }
                let org_id = post
                    .tenant_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())?;
                let workspace_id = post
                    .workspace_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())?;
                let mut tenant = TenantContext::explicit(org_id, workspace_id, None);
                if let Ok(Some(policy)) = self.enterprise.hosted_policy.current() {
                    let bundle = policy.bundle();
                    if org_id == bundle.organization_id && workspace_id == bundle.deployment_id {
                        tenant.deployment_id = Some(bundle.deployment_id.clone());
                    }
                }
                // Incident monitor effects are system-owned; a draft's free-form
                // `actor` field is not an authenticated human owner.
                return Some(ExternalActionProvenance {
                    tenant_context: tenant,
                    owner_principal: None,
                });
            }
            _ => return None,
        };
        let owner_principal = tenant_context
            .actor_id
            .as_deref()
            .map(str::trim)
            .filter(|actor| !actor.is_empty())
            .map(PrincipalRef::human_user);
        Some(ExternalActionProvenance {
            tenant_context,
            owner_principal,
        })
    }
}
