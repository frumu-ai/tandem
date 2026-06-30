use serde_json::Value;

use crate::automation_v2::types::{
    AutomationWebhookFeedbackLoopDecision, AutomationWebhookTriggerRecord,
};
use crate::ExternalActionRecord;

use super::{
    automation_webhook_feedback_decision_from_action, AppState,
    AutomationWebhookFeedbackLoopCandidate,
};

impl AppState {
    pub(crate) async fn classify_automation_webhook_feedback_loop(
        &self,
        trigger: &AutomationWebhookTriggerRecord,
        candidate: Option<&AutomationWebhookFeedbackLoopCandidate>,
    ) -> Option<AutomationWebhookFeedbackLoopDecision> {
        let candidate = candidate.filter(|candidate| !candidate.is_empty())?;
        let action = self
            .feedback_loop_candidate_action(candidate)
            .await
            .filter(|action| self.feedback_loop_candidate_matches_action(candidate, action))?;
        if !self
            .external_action_matches_webhook_tenant(trigger, &action, candidate)
            .await
        {
            return None;
        }
        Some(automation_webhook_feedback_decision_from_action(
            &action, candidate,
        ))
    }

    async fn feedback_loop_candidate_action(
        &self,
        candidate: &AutomationWebhookFeedbackLoopCandidate,
    ) -> Option<ExternalActionRecord> {
        if let Some(key) = candidate
            .source_idempotency_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if let Some(action) = self.get_external_action_by_idempotency_key(key).await {
                return Some(action);
            }
        }
        let action_id = candidate
            .source_action_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())?;
        self.get_external_action(action_id).await
    }

    fn feedback_loop_candidate_matches_action(
        &self,
        candidate: &AutomationWebhookFeedbackLoopCandidate,
        action: &ExternalActionRecord,
    ) -> bool {
        if let Some(candidate_action_id) = candidate.source_action_id.as_deref() {
            if candidate_action_id != action.action_id {
                return false;
            }
        }
        if let (Some(candidate_key), Some(action_key)) = (
            candidate.source_idempotency_key.as_deref(),
            action.idempotency_key.as_deref(),
        ) {
            if candidate_key.trim() != action_key.trim() {
                return false;
            }
        }
        if let Some(candidate_run_id) = candidate.source_run_id.as_deref() {
            if let Some(action_run_id) = external_action_run_id(action) {
                if candidate_run_id != action_run_id {
                    return false;
                }
            }
        }
        if let Some(candidate_node_id) = candidate.source_node_id.as_deref() {
            if let Some(action_node_id) = external_action_node_id(action) {
                if candidate_node_id != action_node_id {
                    return false;
                }
            }
        }
        if let (Some(resource_id), Some(target)) = (
            candidate.provider_resource_id.as_deref(),
            action.target.as_deref(),
        ) {
            if !target.contains(resource_id) {
                return false;
            }
        }
        true
    }

    async fn external_action_matches_webhook_tenant(
        &self,
        trigger: &AutomationWebhookTriggerRecord,
        action: &ExternalActionRecord,
        candidate: &AutomationWebhookFeedbackLoopCandidate,
    ) -> bool {
        if let Some(tenant_context) = external_action_tenant_context(action) {
            return tenant_context.org_id == trigger.tenant_context.org_id
                && tenant_context.workspace_id == trigger.tenant_context.workspace_id
                && tenant_context.deployment_id == trigger.tenant_context.deployment_id;
        }

        let run_id = candidate
            .source_run_id
            .as_deref()
            .map(ToOwned::to_owned)
            .or_else(|| external_action_run_id(action));
        let Some(run_id) = run_id else {
            return false;
        };
        self.get_automation_v2_run(&run_id)
            .await
            .is_some_and(|run| {
                run.tenant_context.org_id == trigger.tenant_context.org_id
                    && run.tenant_context.workspace_id == trigger.tenant_context.workspace_id
                    && run.tenant_context.deployment_id == trigger.tenant_context.deployment_id
            })
    }
}

fn external_action_metadata_value<'a>(
    action: &'a ExternalActionRecord,
    key: &str,
) -> Option<&'a Value> {
    action.metadata.as_ref()?.get(key)
}

fn external_action_metadata_string(action: &ExternalActionRecord, key: &str) -> Option<String> {
    external_action_metadata_value(action, key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn external_action_run_id(action: &ExternalActionRecord) -> Option<String> {
    external_action_metadata_string(action, "automationRunID")
        .or_else(|| external_action_metadata_string(action, "automation_run_id"))
        .or_else(|| {
            action
                .source_id
                .as_deref()
                .and_then(|source_id| source_id.split(':').next())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
}

fn external_action_node_id(action: &ExternalActionRecord) -> Option<String> {
    external_action_metadata_string(action, "nodeID")
        .or_else(|| external_action_metadata_string(action, "node_id"))
}

fn external_action_tenant_context(
    action: &ExternalActionRecord,
) -> Option<tandem_types::TenantContext> {
    action
        .metadata
        .as_ref()
        .and_then(|metadata| {
            metadata
                .get("tenantContext")
                .or_else(|| metadata.get("tenant_context"))
        })
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}
