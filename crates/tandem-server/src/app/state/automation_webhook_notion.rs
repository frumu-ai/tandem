// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Notion provider webhook support (TAN-562).
//!
//! Notion's model differs from Tandem's generated-secret model: Notion POSTs a
//! `verification_token` to the callback URL *after* the trigger exists, the
//! operator copies that token back into Notion to activate the subscription, and
//! subsequent events are signed with `X-Notion-Signature` keyed by that token.
//!
//! This module captures the verification token (storing it as the trigger's
//! signing secret material so the existing verifier path works unchanged),
//! tracks the verification lifecycle, and exposes a one-time operator reveal.
//! The public intake resolves the tenant only from the stored trigger — the
//! Notion payload never selects tenant/workspace/automation/authority.

use anyhow::Context;
use serde_json::{json, Value};
use tandem_types::TenantContext;

use super::automation_webhook_store::{secret_digest, secret_material_key};
use crate::automation_v2::types::{
    normalize_automation_webhook_provider, AutomationWebhookNotionVerification,
    AutomationWebhookNotionVerificationStatus,
};
use crate::util::time::now_ms;
use crate::{AppState, AutomationWebhookDeliveryStatus, AutomationWebhookTriggerRecord};

/// Outcome of inspecting an inbound public webhook for the Notion verification
/// handshake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutomationWebhookNotionIntake {
    /// Not a Notion verification handshake — proceed with normal signature
    /// verification and queueing.
    NotApplicable,
    /// Verification token captured and stored; respond opaque-200 without
    /// queueing a workflow run.
    Captured,
    /// A Notion verification-token payload that was ignored (a token was already
    /// received); respond opaque-200 without queueing a workflow run.
    Ignored,
}

impl AppState {
    /// If this inbound request is a Notion subscription verification handshake
    /// (Notion provider, unsigned, JSON body carrying `verification_token`),
    /// capture the token as the trigger's signing secret and record a sanitized
    /// status delivery. Returns [`AutomationWebhookNotionIntake::NotApplicable`]
    /// for everything else so the caller runs normal signature verification.
    pub(crate) async fn handle_automation_webhook_notion_verification(
        &self,
        public_path_token: &str,
        body: &[u8],
        has_notion_signature: bool,
        received_at_ms: u64,
    ) -> AutomationWebhookNotionIntake {
        // A signed request is a real event, not the verification handshake.
        if has_notion_signature {
            return AutomationWebhookNotionIntake::NotApplicable;
        }
        let Some(_token) = extract_notion_verification_token(body) else {
            return AutomationWebhookNotionIntake::NotApplicable;
        };
        let Some(trigger) = self
            .get_automation_webhook_trigger_by_public_token(public_path_token)
            .await
        else {
            return AutomationWebhookNotionIntake::NotApplicable;
        };
        if normalize_automation_webhook_provider(&trigger.provider).as_deref() != Some("notion") {
            return AutomationWebhookNotionIntake::NotApplicable;
        }

        let status = trigger
            .notion_verification
            .as_ref()
            .map(|verification| verification.status)
            .unwrap_or_default();
        let body_digest = super::automation_webhook_body_digest(body);

        // Only capture the first token while awaiting; never overwrite a token
        // that has already been received, so an unsigned request cannot reset a
        // subscription that is already being set up or is live.
        if status != AutomationWebhookNotionVerificationStatus::AwaitingToken {
            let _ = self
                .record_automation_webhook_rejection(
                    &trigger,
                    None,
                    body_digest,
                    AutomationWebhookDeliveryStatus::Suppressed,
                    "notion_verification_token_ignored",
                    received_at_ms,
                    json!({ "notion_verification": "ignored", "reason": "already_received" }),
                    None,
                )
                .await;
            return AutomationWebhookNotionIntake::Ignored;
        }

        // Re-extract and apply the token inside the storing call under the
        // persistence lock. `applied == false` means another concurrent
        // verification POST captured the token first (first-token-wins).
        let applied = match self
            .store_notion_verification_token(&trigger, body, received_at_ms)
            .await
        {
            Ok(applied) => applied,
            Err(error) => {
                tracing::warn!(
                    target: "tandem_server::state",
                    error = ?error,
                    trigger_id = %trigger.trigger_id,
                    "failed to store notion verification token"
                );
                // Fall through to the normal path, which will reject the unsigned
                // request rather than silently accepting it.
                return AutomationWebhookNotionIntake::NotApplicable;
            }
        };

        if !applied {
            let _ = self
                .record_automation_webhook_rejection(
                    &trigger,
                    None,
                    body_digest,
                    AutomationWebhookDeliveryStatus::Suppressed,
                    "notion_verification_token_ignored",
                    received_at_ms,
                    json!({ "notion_verification": "ignored", "reason": "already_received" }),
                    None,
                )
                .await;
            return AutomationWebhookNotionIntake::Ignored;
        }

        let _ = self
            .record_automation_webhook_rejection(
                &trigger,
                None,
                body_digest,
                AutomationWebhookDeliveryStatus::Received,
                "notion_verification_token_received",
                received_at_ms,
                json!({ "notion_verification": "token_received" }),
                None,
            )
            .await;
        AutomationWebhookNotionIntake::Captured
    }

    /// Overwrite the trigger's placeholder secret material with Notion's
    /// verification token and advance the trigger to `token_received`. Returns
    /// `false` without mutating anything when the trigger is no longer awaiting a
    /// token (another verification POST won the race), enforcing first-token-wins.
    async fn store_notion_verification_token(
        &self,
        trigger: &AutomationWebhookTriggerRecord,
        body: &[u8],
        received_at_ms: u64,
    ) -> anyhow::Result<bool> {
        let token = extract_notion_verification_token(body)
            .context("missing verification_token in notion verification body")?;
        let _guard = self.automation_webhook_persistence.lock().await;

        // Re-read the current status while holding the lock — the pre-lock
        // `AwaitingToken` check was made on a stale clone.
        let (secret_ref, tenant_context) = {
            let triggers = self.automation_webhook_triggers.read().await;
            let stored = triggers
                .get(&trigger.trigger_id)
                .context("notion trigger not found")?;
            let status = stored
                .notion_verification
                .as_ref()
                .map(|verification| verification.status)
                .unwrap_or_default();
            if status != AutomationWebhookNotionVerificationStatus::AwaitingToken {
                return Ok(false);
            }
            (
                stored.secret.secret_ref.clone(),
                stored.tenant_context.clone(),
            )
        };

        let key = secret_material_key(&secret_ref);
        {
            let mut materials = self.automation_webhook_secret_material.write().await;
            let material = materials
                .get_mut(&key)
                .context("notion trigger secret material not found")?;
            if material.trigger_id != trigger.trigger_id
                || material.tenant_context.org_id != tenant_context.org_id
                || material.tenant_context.workspace_id != tenant_context.workspace_id
            {
                anyhow::bail!("notion verification token tenant/trigger binding mismatch");
            }
            material.secret = token.clone();
        }
        self.persist_automation_webhook_secret_material_locked()
            .await?;

        let digest = secret_digest(&token, &tenant_context, &trigger.trigger_id);
        {
            let mut triggers = self.automation_webhook_triggers.write().await;
            let stored = triggers
                .get_mut(&trigger.trigger_id)
                .context("notion trigger not found")?;
            stored.secret.secret_digest = digest;
            let verification = stored
                .notion_verification
                .get_or_insert_with(AutomationWebhookNotionVerification::default);
            verification.status = AutomationWebhookNotionVerificationStatus::TokenReceived;
            verification.token_received_at_ms = Some(received_at_ms);
            verification.token_revealed_at_ms = None;
            verification.verified_at_ms = None;
            stored.updated_at_ms = received_at_ms;
        }
        self.persist_automation_webhook_triggers_locked().await?;
        Ok(true)
    }

    /// One-time reveal of the stored Notion verification token to an authorized
    /// operator (tenant + automation + trigger scoped) so it can be pasted back
    /// into Notion. Returns the token exactly once; subsequent calls return
    /// `None` and the token is never exposed again.
    pub(crate) async fn reveal_automation_webhook_notion_verification_token(
        &self,
        tenant_context: &TenantContext,
        automation_id: &str,
        trigger_id: &str,
    ) -> anyhow::Result<Option<String>> {
        let _guard = self.automation_webhook_persistence.lock().await;
        let secret_ref = {
            let triggers = self.automation_webhook_triggers.read().await;
            let Some(trigger) = triggers.get(trigger_id).filter(|trigger| {
                trigger.tenant_matches(tenant_context) && trigger.automation_id == automation_id
            }) else {
                return Ok(None);
            };
            let available = trigger
                .notion_verification
                .as_ref()
                .map(AutomationWebhookNotionVerification::token_available_for_reveal)
                .unwrap_or(false);
            if !available {
                return Ok(None);
            }
            trigger.secret.secret_ref.clone()
        };

        let token = {
            let materials = self.automation_webhook_secret_material.read().await;
            materials
                .get(&secret_material_key(&secret_ref))
                .filter(|material| {
                    material.trigger_id == trigger_id
                        && material.tenant_context.org_id == tenant_context.org_id
                        && material.tenant_context.workspace_id == tenant_context.workspace_id
                })
                .map(|material| material.secret.clone())
        };
        let Some(token) = token else {
            return Ok(None);
        };

        {
            let mut triggers = self.automation_webhook_triggers.write().await;
            if let Some(trigger) = triggers.get_mut(trigger_id) {
                if let Some(verification) = trigger.notion_verification.as_mut() {
                    verification.token_revealed_at_ms = Some(now_ms());
                }
                trigger.updated_at_ms = now_ms();
            }
        }
        self.persist_automation_webhook_triggers_locked().await?;
        Ok(Some(token))
    }
}

fn extract_notion_verification_token(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    value
        .get("verification_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(ToString::to_string)
}
