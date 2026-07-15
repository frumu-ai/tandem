// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Linear provider webhook support (TAN-610/TAN-611).
//!
//! Linear's model is the inverse of Notion's handshake: Linear generates the
//! signing secret in its own webhook settings UI, and the operator pastes it
//! *into* Tandem via an authenticated import mutation. Until that import
//! happens the trigger's stored material is a Tandem-generated placeholder
//! Linear cannot sign with, and deliveries fail closed
//! (`provider_secret_not_imported`).
//!
//! The imported secret is stored as the trigger's signing secret material so
//! the existing verifier path works unchanged; re-import is allowed (Linear
//! secrets can be rotated in Linear's UI) and bumps the secret version.

use anyhow::Context;
use tandem_types::TenantContext;

use super::automation_webhook_store::{secret_digest, secret_material_key};
use crate::automation_v2::types::{
    AutomationWebhookLinearVerification, AutomationWebhookSecretMetadata,
    AutomationWebhookSignatureScheme, AutomationWebhookTriggerRecord,
};
use crate::util::time::now_ms;
use crate::AppState;

/// Bound pasted secrets to something sane: Linear signing secrets are long
/// hex strings; anything beyond this is an operator mistake (e.g. pasting a
/// whole config file).
const MAX_IMPORTED_SECRET_LEN: usize = 1024;

impl AppState {
    /// Import (or replace) the provider-owned Linear signing secret for a
    /// trigger. Tenant + automation + scheme scoped: the trigger must belong to
    /// the caller's tenant and automation and verify with `linear_hmac_sha256`.
    /// Each import writes new secret material under a bumped secret version and
    /// resets the verification lifecycle to `secret_imported` until the next
    /// signed event proves the secret out.
    pub(crate) async fn import_automation_webhook_linear_secret(
        &self,
        tenant_context: &TenantContext,
        automation_id: &str,
        trigger_id: &str,
        secret: &str,
        actor_id: Option<String>,
    ) -> anyhow::Result<AutomationWebhookTriggerRecord> {
        let secret = secret.trim();
        if secret.is_empty() {
            anyhow::bail!("linear signing secret is required");
        }
        if secret.len() > MAX_IMPORTED_SECRET_LEN {
            anyhow::bail!("linear signing secret is unreasonably long");
        }

        let _guard = self.automation_webhook_persistence.lock().await;
        let now = now_ms();
        let current_trigger = {
            let triggers = self.automation_webhook_triggers.read().await;
            let trigger = triggers
                .get(trigger_id)
                .with_context(|| format!("webhook trigger `{trigger_id}` not found"))?
                .clone();
            if !trigger.tenant_matches(tenant_context) || trigger.automation_id != automation_id {
                anyhow::bail!("webhook trigger tenant or automation mismatch");
            }
            trigger
        };
        if !matches!(
            current_trigger.signature_scheme,
            AutomationWebhookSignatureScheme::LinearHmacSha256
        ) {
            anyhow::bail!(
                "provider secret import is only supported for linear_hmac_sha256 webhook triggers"
            );
        }

        let old_secret_ref = current_trigger.secret.secret_ref.clone();
        let secret_version = current_trigger
            .secret
            .secret_version
            .saturating_add(1)
            .max(1);
        let secret_ref = super::automation_webhook_store::secret_ref_for_trigger(
            tenant_context,
            trigger_id,
            secret_version,
        );
        secret_ref
            .validate_for_tenant(tenant_context)
            .map_err(|error| anyhow::anyhow!("webhook secret ref tenant mismatch: {error:?}"))?;

        let mut trigger = current_trigger.clone();
        trigger.secret = AutomationWebhookSecretMetadata {
            secret_ref: secret_ref.clone(),
            secret_digest: secret_digest(secret, tenant_context, trigger_id),
            secret_version,
            created_at_ms: now,
            rotated_at_ms: Some(now),
            rotated_by: actor_id.clone(),
        };
        trigger
            .linear_verification
            .get_or_insert_with(AutomationWebhookLinearVerification::default)
            .mark_secret_imported(now);
        trigger.updated_at_ms = now;
        trigger.updated_by = actor_id.clone();

        let material = super::AutomationWebhookSecretMaterialRecord {
            secret_ref: secret_ref.clone(),
            tenant_context: tenant_context.clone(),
            trigger_id: trigger_id.to_string(),
            secret_version,
            secret: secret.to_string(),
            created_at_ms: now,
            rotated_at_ms: Some(now),
            rotated_by: actor_id,
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
            return Err(error.context("failed to persist imported webhook secret material"));
        }

        self.automation_webhook_triggers
            .write()
            .await
            .insert(trigger_id.to_string(), trigger.clone());
        if let Err(error) = self.persist_automation_webhook_triggers_locked().await {
            self.automation_webhook_triggers
                .write()
                .await
                .insert(trigger_id.to_string(), current_trigger);
            self.automation_webhook_secret_material
                .write()
                .await
                .remove(&new_secret_key);
            if let Err(cleanup_error) = self
                .persist_automation_webhook_secret_material_locked()
                .await
            {
                tracing::warn!(
                    target: "tandem_server::state",
                    error = ?cleanup_error,
                    trigger_id,
                    "failed to clean up imported webhook secret material after trigger persist failure"
                );
            }
            return Err(error.context("failed to persist webhook trigger metadata after import"));
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
                "failed to persist removal of superseded webhook secret material"
            );
        }

        Ok(trigger)
    }
}
