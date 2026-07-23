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
use serde_json::Value;
use tandem_types::TenantContext;

use super::automation_webhook_store::{
    new_notion_setup_nonce, new_secret, secret_digest, secret_material_key, secret_ref_for_trigger,
    AutomationWebhookSecretMaterialRecord, AUTOMATION_WEBHOOK_NOTION_SETUP_TTL_MS,
};
use crate::automation_v2::types::{
    normalize_automation_webhook_provider, AutomationWebhookNotionVerification,
    AutomationWebhookNotionVerificationStatus, AutomationWebhookSecretMetadata,
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

#[derive(Debug, Clone)]
pub(crate) struct AutomationWebhookNotionSetupReset {
    pub(crate) trigger: AutomationWebhookTriggerRecord,
    pub(crate) setup_nonce: String,
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
        setup_nonce: Option<&str>,
        body: &[u8],
        has_notion_signature: bool,
        received_at_ms: u64,
    ) -> AutomationWebhookNotionIntake {
        // A signed request is a real event, not the verification handshake.
        if has_notion_signature {
            return AutomationWebhookNotionIntake::NotApplicable;
        }
        let Some(_) = extract_notion_verification_token(body) else {
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

        let setup_is_current = trigger
            .notion_verification
            .as_ref()
            .is_some_and(|verification| {
                let Some(expected_digest) = verification.setup_challenge_digest.as_deref() else {
                    return false;
                };
                let Some(provided_nonce) = setup_nonce else {
                    return false;
                };
                status == AutomationWebhookNotionVerificationStatus::AwaitingToken
                    && verification.setup_challenge_consumed_at_ms.is_none()
                    && verification
                        .setup_challenge_expires_at_ms
                        .is_some_and(|expires_at_ms| received_at_ms <= expires_at_ms)
                    && crate::constant_time_str_eq(
                        expected_digest,
                        &secret_digest(
                            provided_nonce,
                            &trigger.tenant_context,
                            &trigger.trigger_id,
                        ),
                    )
            });
        if !setup_is_current {
            return AutomationWebhookNotionIntake::Ignored;
        }

        // Only capture the first token while awaiting; never overwrite a token
        // that has already been received, so an unsigned request cannot reset a
        // subscription that is already being set up or is live.
        if status != AutomationWebhookNotionVerificationStatus::AwaitingToken {
            let _ = self
                .record_automation_webhook_pre_auth_rejection(
                    &trigger,
                    None,
                    body_digest,
                    AutomationWebhookDeliveryStatus::Suppressed,
                    "notion_verification_token_ignored",
                    received_at_ms,
                    None,
                )
                .await;
            return AutomationWebhookNotionIntake::Ignored;
        }

        // Re-extract and apply the token inside the storing call under the
        // persistence lock. `applied == false` means another concurrent
        // verification POST captured the token first (first-token-wins).
        let applied = match self
            .store_notion_verification_token(
                &trigger,
                setup_nonce.unwrap_or_default(),
                body,
                received_at_ms,
            )
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
                .record_automation_webhook_pre_auth_rejection(
                    &trigger,
                    None,
                    body_digest,
                    AutomationWebhookDeliveryStatus::Suppressed,
                    "notion_verification_token_ignored",
                    received_at_ms,
                    None,
                )
                .await;
            return AutomationWebhookNotionIntake::Ignored;
        }

        let _ = self
            .record_automation_webhook_pre_auth_rejection(
                &trigger,
                None,
                body_digest,
                AutomationWebhookDeliveryStatus::Received,
                "notion_verification_token_received",
                received_at_ms,
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
        setup_nonce: &str,
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
            let verification = stored
                .notion_verification
                .as_ref()
                .context("notion trigger is missing verification state")?;
            let expected_digest = verification
                .setup_challenge_digest
                .as_deref()
                .context("notion setup challenge is not configured")?;
            if verification.setup_challenge_consumed_at_ms.is_some()
                || !verification
                    .setup_challenge_expires_at_ms
                    .is_some_and(|expires_at_ms| received_at_ms <= expires_at_ms)
                || !crate::constant_time_str_eq(
                    expected_digest,
                    &secret_digest(setup_nonce, &stored.tenant_context, &stored.trigger_id),
                )
            {
                return Ok(false);
            }
            (
                stored.secret.secret_ref.clone(),
                stored.tenant_context.clone(),
            )
        };

        let key = secret_material_key(&secret_ref);
        let previous_material = {
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
            let previous = material.clone();
            material.secret = token.clone();
            previous
        };
        if let Err(error) = self
            .persist_automation_webhook_secret_material_locked()
            .await
        {
            self.automation_webhook_secret_material
                .write()
                .await
                .insert(key.clone(), previous_material.clone());
            return Err(error.context("persist Notion verification secret"));
        }

        let digest = secret_digest(&token, &tenant_context, &trigger.trigger_id);
        let previous_trigger = {
            let mut triggers = self.automation_webhook_triggers.write().await;
            let stored = triggers
                .get_mut(&trigger.trigger_id)
                .context("notion trigger not found")?;
            let previous = stored.clone();
            stored.secret.secret_digest = digest;
            let verification = stored
                .notion_verification
                .get_or_insert_with(AutomationWebhookNotionVerification::default);
            verification.status = AutomationWebhookNotionVerificationStatus::TokenReceived;
            verification.token_received_at_ms = Some(received_at_ms);
            verification.token_revealed_at_ms = None;
            verification.verified_at_ms = None;
            verification.setup_challenge_digest = None;
            verification.setup_challenge_expires_at_ms = None;
            verification.setup_challenge_consumed_at_ms = Some(received_at_ms);
            stored.updated_at_ms = received_at_ms;
            previous
        };
        if let Err(error) = self.persist_automation_webhook_triggers_locked().await {
            self.automation_webhook_triggers
                .write()
                .await
                .insert(trigger.trigger_id.clone(), previous_trigger);
            self.automation_webhook_secret_material
                .write()
                .await
                .insert(key, previous_material);
            if let Err(rollback_error) = self
                .persist_automation_webhook_secret_material_locked()
                .await
            {
                tracing::error!(
                    target: "tandem_server::state",
                    error = ?rollback_error,
                    trigger_id = %trigger.trigger_id,
                    "failed to roll back Notion verification secret after trigger persistence failure"
                );
            }
            return Err(error.context("persist consumed Notion setup challenge"));
        }
        Ok(true)
    }

    pub(crate) async fn reset_automation_webhook_notion_setup(
        &self,
        tenant_context: &TenantContext,
        automation_id: &str,
        trigger_id: &str,
        actor_id: Option<String>,
    ) -> anyhow::Result<AutomationWebhookNotionSetupReset> {
        let setup_nonce = new_notion_setup_nonce();
        let placeholder_secret = new_secret();
        let now = now_ms();
        let _guard = self.automation_webhook_persistence.lock().await;
        let current_trigger = {
            let triggers = self.automation_webhook_triggers.read().await;
            let trigger = triggers
                .get(trigger_id)
                .with_context(|| format!("webhook trigger `{trigger_id}` not found"))?
                .clone();
            if !trigger.tenant_matches(tenant_context)
                || trigger.automation_id != automation_id
                || normalize_automation_webhook_provider(&trigger.provider).as_deref()
                    != Some("notion")
                || trigger.notion_verification.is_none()
            {
                anyhow::bail!("notion webhook trigger tenant, automation, or provider mismatch");
            }
            trigger
        };
        let old_secret_ref = current_trigger.secret.secret_ref.clone();
        let secret_version = current_trigger
            .secret
            .secret_version
            .saturating_add(1)
            .max(1);
        let secret_ref = secret_ref_for_trigger(tenant_context, trigger_id, secret_version);
        secret_ref
            .validate_for_tenant(tenant_context)
            .map_err(|error| anyhow::anyhow!("webhook secret ref tenant mismatch: {error:?}"))?;

        let material = AutomationWebhookSecretMaterialRecord {
            secret_ref: secret_ref.clone(),
            tenant_context: tenant_context.clone(),
            trigger_id: trigger_id.to_string(),
            secret_version,
            secret: placeholder_secret.clone(),
            created_at_ms: now,
            rotated_at_ms: Some(now),
            rotated_by: actor_id.clone(),
        };
        let new_secret_key = secret_material_key(&secret_ref);
        self.automation_webhook_secret_material
            .write()
            .await
            .insert(new_secret_key.clone(), material);
        if let Err(error) = self
            .persist_automation_webhook_secret_material_locked()
            .await
        {
            self.automation_webhook_secret_material
                .write()
                .await
                .remove(&new_secret_key);
            return Err(error.context("persist reset Notion placeholder secret"));
        }

        let mut updated = current_trigger.clone();
        updated.secret = AutomationWebhookSecretMetadata {
            secret_ref: secret_ref.clone(),
            secret_digest: secret_digest(&placeholder_secret, tenant_context, trigger_id),
            secret_version,
            created_at_ms: now,
            rotated_at_ms: Some(now),
            rotated_by: actor_id.clone(),
        };
        updated.updated_at_ms = now;
        updated.updated_by = actor_id;
        let verification = updated
            .notion_verification
            .as_mut()
            .context("notion trigger is missing verification state")?;
        verification.status = AutomationWebhookNotionVerificationStatus::AwaitingToken;
        verification.token_received_at_ms = None;
        verification.token_revealed_at_ms = None;
        verification.verified_at_ms = None;
        verification.setup_challenge_digest =
            Some(secret_digest(&setup_nonce, tenant_context, trigger_id));
        verification.setup_challenge_expires_at_ms =
            now.checked_add(AUTOMATION_WEBHOOK_NOTION_SETUP_TTL_MS);
        verification.setup_challenge_consumed_at_ms = None;
        verification.setup_generation = verification.setup_generation.saturating_add(1).max(1);
        self.automation_webhook_triggers
            .write()
            .await
            .insert(trigger_id.to_string(), updated.clone());
        if let Err(error) = self.persist_automation_webhook_triggers_locked().await {
            self.automation_webhook_triggers
                .write()
                .await
                .insert(trigger_id.to_string(), current_trigger);
            self.automation_webhook_secret_material
                .write()
                .await
                .remove(&new_secret_key);
            let _ = self
                .persist_automation_webhook_secret_material_locked()
                .await;
            return Err(error.context("persist reset Notion setup challenge"));
        }

        let old_secret_key = secret_material_key(&old_secret_ref);
        self.automation_webhook_secret_material
            .write()
            .await
            .remove(&old_secret_key);
        if let Err(error) = self
            .persist_automation_webhook_secret_material_locked()
            .await
        {
            tracing::warn!(
                target: "tandem_server::state",
                error = ?error,
                trigger_id,
                "failed to remove prior Notion secret after setup reset"
            );
        }
        Ok(AutomationWebhookNotionSetupReset {
            trigger: updated,
            setup_nonce,
        })
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
