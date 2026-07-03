use std::collections::HashMap;

use anyhow::Context;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tandem_types::{
    DataClass, PrincipalRef, ResourceScope, SecretRef, TenantContext, ToolRiskTier,
};
use tokio::fs;
use uuid::Uuid;

use crate::automation_v2::types::*;
use crate::stateful_runtime::{
    append_stateful_run_event_once_with_next_seq, begin_claimed_stateful_wait_wake_completion,
    cancel_stateful_wait_after_phase_guard_denial, claim_matching_stateful_webhook_wait,
    finish_claimed_stateful_wait_completion, guarded_phase_state_from_status,
    list_stateful_run_snapshots, write_stateful_run_snapshot, StatefulRunEventRecord,
    StatefulRunSnapshotRecord, StatefulRuntimeScope, StatefulRuntimeStoragePaths, StatefulWaitKind,
    StatefulWaitRecord, StatefulWaitStatus, StatefulWebhookWaitEvent, StatefulWorkflowPhaseState,
    StatefulWorkflowRunKind, StatefulWorkflowRunStatus,
};
use crate::util::time::now_ms;

use super::{
    automation_webhook_accepted_delivery, automation_webhook_delivery_correlation,
    automation_webhook_delivery_matches_key, automation_webhook_rejection_delivery,
    automation_webhook_run_metadata, automation_webhook_scope_denial_reason,
    idempotency_outcome_ref, new_automation_webhook_delivery_id, AppState,
    AutomationWebhookDedupeDecision, AutomationWebhookFeedbackLoopCandidate,
    AutomationWebhookReservedClaim, AutomationWebhookVerificationDecision,
};

type HmacSha256 = Hmac<Sha256>;

const AUTOMATION_WEBHOOK_SCHEMA_VERSION: u32 = 1;
const AUTOMATION_WEBHOOK_SECRET_PROVIDER: &str = "tandem_automation_webhooks";
const AUTOMATION_WEBHOOK_STATEFUL_WAIT_CLAIMANT: &str = "automation_webhook_router";
const AUTOMATION_WEBHOOK_STATEFUL_WAIT_LEASE_MS: u64 = 30_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AutomationWebhookTriggersFile {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    triggers: HashMap<String, AutomationWebhookTriggerRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AutomationWebhookDeliveriesFile {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    deliveries: HashMap<String, AutomationWebhookDeliveryRecord>,
}

#[derive(Clone, Serialize, Deserialize)]
struct AutomationWebhookSecretMaterialFile {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    secrets: HashMap<String, AutomationWebhookSecretMaterialRecord>,
}

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct AutomationWebhookSecretMaterialRecord {
    pub secret_ref: SecretRef,
    pub tenant_context: TenantContext,
    pub trigger_id: String,
    pub secret_version: u64,
    pub secret: String,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotated_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rotated_by: Option<String>,
}

#[derive(Clone)]
pub(crate) struct AutomationWebhookTriggerCreateInput {
    pub automation_id: String,
    pub tenant_context: TenantContext,
    pub owner_principal: Option<PrincipalRef>,
    pub created_by: Option<String>,
    pub owning_org_unit_id: Option<String>,
    pub resource_scope: Option<ResourceScope>,
    pub default_data_class: DataClass,
    pub default_risk_tier: Option<ToolRiskTier>,
    pub name: Option<String>,
    pub provider: String,
    pub provider_event_kind: Option<String>,
    pub signature_scheme: Option<AutomationWebhookSignatureScheme>,
    pub enabled: bool,
}

#[derive(Clone, Default)]
pub(crate) struct AutomationWebhookTriggerUpdateInput {
    pub name: Option<String>,
    pub provider: Option<String>,
    pub provider_event_kind: Option<Option<String>>,
    pub signature_scheme: Option<AutomationWebhookSignatureScheme>,
    pub default_data_class: Option<DataClass>,
    pub default_risk_tier: Option<Option<ToolRiskTier>>,
    pub enabled: Option<bool>,
}

#[derive(Clone)]
pub(crate) struct AutomationWebhookCreateResult {
    pub trigger: AutomationWebhookTriggerRecord,
    pub secret: String,
}

#[derive(Clone)]
pub(crate) struct AutomationWebhookRotationResult {
    pub trigger: AutomationWebhookTriggerRecord,
    pub secret: String,
}

#[derive(Debug, Clone)]
pub(crate) struct VerifiedAutomationWebhookRequest {
    pub trigger: AutomationWebhookTriggerRecord,
    pub provider_event_id: Option<String>,
    pub body_digest: String,
    pub received_at_ms: u64,
    pub verification: AutomationWebhookVerificationDecision,
}

#[derive(Debug, Clone)]
pub(crate) enum AutomationWebhookQueueResult {
    Accepted {
        delivery: AutomationWebhookDeliveryRecord,
        run: AutomationV2RunRecord,
    },
    Duplicate {
        delivery: AutomationWebhookDeliveryRecord,
    },
    Woken {
        delivery: AutomationWebhookDeliveryRecord,
        wait: StatefulWaitRecord,
    },
    Suppressed {
        delivery: AutomationWebhookDeliveryRecord,
    },
    Rejected {
        delivery: AutomationWebhookDeliveryRecord,
        reason_code: String,
    },
}

fn parse_automation_webhook_triggers_file(
    raw: &str,
) -> anyhow::Result<HashMap<String, AutomationWebhookTriggerRecord>> {
    if raw.trim().is_empty() || raw.trim() == "{}" {
        return Ok(HashMap::new());
    }
    let value: Value = serde_json::from_str(raw)
        .context("failed to parse automation webhook triggers state file")?;
    if value.get("schema_version").is_none() {
        return serde_json::from_value(value)
            .context("failed to parse legacy automation webhook trigger map");
    }
    let file = serde_json::from_value::<AutomationWebhookTriggersFile>(value)
        .context("failed to parse versioned automation webhook triggers state file")?;
    ensure_supported_schema(file.schema_version, "automation webhook triggers")?;
    Ok(file.triggers)
}

fn parse_automation_webhook_deliveries_file(
    raw: &str,
) -> anyhow::Result<HashMap<String, AutomationWebhookDeliveryRecord>> {
    if raw.trim().is_empty() || raw.trim() == "{}" {
        return Ok(HashMap::new());
    }
    let value: Value = serde_json::from_str(raw)
        .context("failed to parse automation webhook deliveries state file")?;
    if value.get("schema_version").is_none() {
        return serde_json::from_value(value)
            .context("failed to parse legacy automation webhook delivery map");
    }
    let file = serde_json::from_value::<AutomationWebhookDeliveriesFile>(value)
        .context("failed to parse versioned automation webhook deliveries state file")?;
    ensure_supported_schema(file.schema_version, "automation webhook deliveries")?;
    Ok(file.deliveries)
}

fn parse_automation_webhook_secret_material_file(
    raw: &str,
) -> anyhow::Result<HashMap<String, AutomationWebhookSecretMaterialRecord>> {
    if raw.trim().is_empty() || raw.trim() == "{}" {
        return Ok(HashMap::new());
    }
    let value: Value = serde_json::from_str(raw)
        .context("failed to parse automation webhook secret material state file")?;
    if value.get("schema_version").is_none() {
        return serde_json::from_value(value)
            .context("failed to parse legacy automation webhook secret material map");
    }
    let file = serde_json::from_value::<AutomationWebhookSecretMaterialFile>(value)
        .context("failed to parse versioned automation webhook secret material state file")?;
    ensure_supported_schema(file.schema_version, "automation webhook secret material")?;
    Ok(file.secrets)
}

fn ensure_supported_schema(schema_version: u32, label: &str) -> anyhow::Result<()> {
    if schema_version > AUTOMATION_WEBHOOK_SCHEMA_VERSION {
        anyhow::bail!(
            "{label} schema version {schema_version} is newer than supported version {AUTOMATION_WEBHOOK_SCHEMA_VERSION}"
        );
    }
    Ok(())
}

fn serialize_automation_webhook_triggers_file(
    triggers: HashMap<String, AutomationWebhookTriggerRecord>,
) -> anyhow::Result<String> {
    serde_json::to_string_pretty(&AutomationWebhookTriggersFile {
        schema_version: AUTOMATION_WEBHOOK_SCHEMA_VERSION,
        triggers,
    })
    .context("failed to serialize automation webhook triggers state file")
}

fn serialize_automation_webhook_deliveries_file(
    deliveries: HashMap<String, AutomationWebhookDeliveryRecord>,
) -> anyhow::Result<String> {
    serde_json::to_string_pretty(&AutomationWebhookDeliveriesFile {
        schema_version: AUTOMATION_WEBHOOK_SCHEMA_VERSION,
        deliveries,
    })
    .context("failed to serialize automation webhook deliveries state file")
}

fn serialize_automation_webhook_secret_material_file(
    secrets: HashMap<String, AutomationWebhookSecretMaterialRecord>,
) -> anyhow::Result<String> {
    serde_json::to_string_pretty(&AutomationWebhookSecretMaterialFile {
        schema_version: AUTOMATION_WEBHOOK_SCHEMA_VERSION,
        secrets,
    })
    .context("failed to serialize automation webhook secret material state file")
}

pub(crate) fn tenant_context_matches(left: &TenantContext, right: &TenantContext) -> bool {
    left.org_id == right.org_id
        && left.workspace_id == right.workspace_id
        && left.deployment_id == right.deployment_id
}

pub(crate) fn secret_material_key(secret_ref: &SecretRef) -> String {
    format!(
        "{}::{}::{}::{}",
        secret_ref.org_id, secret_ref.workspace_id, secret_ref.provider, secret_ref.secret_id
    )
}

fn new_public_path_token(existing: &HashMap<String, AutomationWebhookTriggerRecord>) -> String {
    loop {
        let token = format!("whpub_{}", Uuid::new_v4().simple());
        if existing
            .values()
            .all(|trigger| trigger.public_path_token != token)
        {
            return token;
        }
    }
}

fn new_secret() -> String {
    format!(
        "whsec_{}{}{}",
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple(),
        Uuid::new_v4().simple()
    )
}

fn secret_ref_for_trigger(
    tenant_context: &TenantContext,
    trigger_id: &str,
    secret_version: u64,
) -> SecretRef {
    SecretRef {
        org_id: tenant_context.org_id.clone(),
        workspace_id: tenant_context.workspace_id.clone(),
        provider: AUTOMATION_WEBHOOK_SECRET_PROVIDER.to_string(),
        secret_id: format!("automation_webhook/{trigger_id}/v{secret_version}"),
        name: format!("Automation webhook {trigger_id} v{secret_version}"),
    }
}

fn secret_digest(secret: &str, tenant_context: &TenantContext, trigger_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(tenant_context.org_id.as_bytes());
    hasher.update([0]);
    hasher.update(tenant_context.workspace_id.as_bytes());
    hasher.update([0]);
    hasher.update(
        tenant_context
            .deployment_id
            .as_deref()
            .unwrap_or("")
            .as_bytes(),
    );
    hasher.update([0]);
    hasher.update(trigger_id.as_bytes());
    hasher.update([0]);
    hasher.update(secret.as_bytes());
    format!("sha256:{}", hex_encode(&hasher.finalize()))
}

pub(crate) fn automation_webhook_body_digest(body: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(body);
    format!("sha256:{}", hex_encode(&hasher.finalize()))
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn sanitize_preview_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    normalized.contains("secret")
        || normalized.contains("token")
        || normalized.contains("api_key")
        || normalized.contains("apikey")
        || normalized.contains("authorization")
        || normalized.contains("cookie")
        || normalized.contains("signature")
        || normalized.contains("password")
}

pub(crate) fn sanitize_automation_webhook_preview(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sanitized = serde_json::Map::new();
            for (key, value) in map {
                if sanitize_preview_key(key) {
                    sanitized.insert(key.clone(), Value::String("[redacted]".to_string()));
                } else {
                    sanitized.insert(key.clone(), sanitize_automation_webhook_preview(value));
                }
            }
            Value::Object(sanitized)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(sanitize_automation_webhook_preview)
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn insert_automation_metadata_value(metadata: &mut Option<Value>, key: &str, value: Value) {
    match metadata {
        Some(Value::Object(map)) => {
            map.insert(key.to_string(), value);
        }
        _ => {
            let mut map = serde_json::Map::new();
            map.insert(key.to_string(), value);
            *metadata = Some(Value::Object(map));
        }
    }
}

#[derive(Debug, Clone)]
enum AutomationWebhookStatefulWaitResult {
    Woken {
        delivery: AutomationWebhookDeliveryRecord,
        wait: StatefulWaitRecord,
    },
    Rejected {
        delivery: AutomationWebhookDeliveryRecord,
        reason_code: String,
    },
}

fn automation_webhook_stateful_wait_event(
    trigger: &AutomationWebhookTriggerRecord,
    provider_event_id: Option<String>,
    body_digest: String,
) -> StatefulWebhookWaitEvent {
    let idempotency_key = provider_event_id
        .as_deref()
        .map(|event_id| format!("{}:{event_id}", trigger.provider))
        .unwrap_or_else(|| body_digest.clone());
    StatefulWebhookWaitEvent {
        trigger_id: trigger.trigger_id.clone(),
        provider: trigger.provider.clone(),
        provider_event_kind: trigger.provider_event_kind.clone(),
        provider_event_id,
        body_digest,
        idempotency_key,
    }
}

fn stateful_webhook_wake_key(
    wait: &StatefulWaitRecord,
    event: &StatefulWebhookWaitEvent,
) -> String {
    format!(
        "webhook:{}:{}:{}",
        event.idempotency_key, wait.run_id, wait.wait_id
    )
}

fn guarded_phase_state_for_webhook_wait(
    paths: &StatefulRuntimeStoragePaths,
    wait: &StatefulWaitRecord,
    received_at_ms: u64,
) -> anyhow::Result<StatefulWorkflowPhaseState> {
    let status = StatefulWorkflowRunStatus::Running;
    let previous_snapshot = list_stateful_run_snapshots(
        &paths.snapshots_root,
        &wait.scope.tenant_context,
        &wait.run_id,
        Some(1),
    )
    .pop();
    let previous_history = previous_snapshot
        .as_ref()
        .map(|snapshot| snapshot.phase_history.as_slice())
        .unwrap_or(&[]);
    guarded_phase_state_from_status(
        &wait.run_id,
        &status,
        received_at_ms,
        wait.phase_id.as_deref(),
        previous_snapshot.as_ref().map(|snapshot| snapshot.phase),
        previous_history,
        Some("automation_webhook:wake_wait".to_string()),
    )
    .map_err(anyhow::Error::from)
}

async fn cancel_webhook_wait_after_phase_guard_denial(
    paths: &StatefulRuntimeStoragePaths,
    wait: &StatefulWaitRecord,
    reason: &str,
    received_at_ms: u64,
) {
    match cancel_stateful_wait_after_phase_guard_denial(
        &paths.waits_path,
        &wait.scope.tenant_context,
        wait,
        reason,
        received_at_ms,
    )
    .await
    {
        Ok(Some(_)) => {}
        Ok(None) => {
            tracing::warn!(
                wait_id = %wait.wait_id,
                run_id = %wait.run_id,
                "phase-denied webhook wait was not cancelled because it no longer matched"
            );
        }
        Err(cancel_error) => {
            tracing::warn!(
                wait_id = %wait.wait_id,
                run_id = %wait.run_id,
                error = %cancel_error,
                "failed to cancel webhook wait after phase guard denial"
            );
        }
    }
}

fn automation_webhook_phase_denied_delivery(
    trigger: &AutomationWebhookTriggerRecord,
    provider_event_id: Option<String>,
    body_digest: String,
    received_at_ms: u64,
    sanitized_preview: Value,
    verification: &AutomationWebhookVerificationDecision,
    primary_idempotency: Option<&AutomationWebhookReservedClaim>,
) -> AutomationWebhookDeliveryRecord {
    let mut delivery = automation_webhook_rejection_delivery(
        trigger,
        provider_event_id,
        body_digest,
        AutomationWebhookDeliveryStatus::Rejected,
        "stateful_wait_phase_denied",
        received_at_ms,
        sanitized_preview,
        Some(verification.clone()),
    );
    if let Some(primary) = primary_idempotency {
        delivery.idempotency_key = Some(primary.claim.key.clone());
        delivery.idempotency_record_id = Some(primary.record.record_id.clone());
        delivery.dedupe_result = Some(AutomationWebhookDedupeResult::Accepted);
        delivery.dedupe_reason_code = Some(format!("rejected_{}", primary.claim.key_kind));
    }
    delivery
}

async fn ensure_parent_dir(path: &std::path::Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    Ok(())
}

async fn write_secret_material_file_atomically(
    path: &std::path::Path,
    payload: &str,
) -> anyhow::Result<()> {
    let tmp = path.with_extension("tmp");
    let _ = fs::remove_file(&tmp).await;

    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        use tokio::io::AsyncWriteExt;

        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&tmp)
            .await?;
        file.write_all(payload.as_bytes()).await?;
        file.sync_all().await?;
        drop(file);
        fs::rename(&tmp, path).await?;
        fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).await?;
        Ok(())
    }

    #[cfg(not(unix))]
    {
        fs::write(&tmp, payload).await?;
        fs::rename(&tmp, path).await?;
        Ok(())
    }
}

impl AppState {
    pub async fn load_automation_webhook_records(&self) -> anyhow::Result<()> {
        let _guard = self.automation_webhook_persistence.lock().await;
        self.load_automation_webhook_triggers_locked().await?;
        self.load_automation_webhook_deliveries_locked().await?;
        self.load_automation_webhook_secret_material_locked()
            .await?;
        Ok(())
    }

    async fn load_automation_webhook_triggers_locked(&self) -> anyhow::Result<()> {
        let triggers = if self.automation_webhook_triggers_path.exists() {
            let raw = fs::read_to_string(&self.automation_webhook_triggers_path).await?;
            parse_automation_webhook_triggers_file(&raw)?
        } else {
            HashMap::new()
        };
        *self.automation_webhook_triggers.write().await = triggers;
        Ok(())
    }

    async fn load_automation_webhook_deliveries_locked(&self) -> anyhow::Result<()> {
        let mut deliveries = if self.automation_webhook_deliveries_path.exists() {
            let raw = fs::read_to_string(&self.automation_webhook_deliveries_path).await?;
            parse_automation_webhook_deliveries_file(&raw)?
        } else {
            HashMap::new()
        };
        let triggers = self.automation_webhook_triggers.read().await.clone();
        let mut upgraded = false;
        for delivery in deliveries.values_mut() {
            if delivery.enterprise_scope.is_some() {
                continue;
            }
            let Some(trigger) = triggers.get(&delivery.trigger_id).filter(|trigger| {
                trigger.automation_id == delivery.automation_id
                    && trigger.tenant_matches(&delivery.tenant_context)
            }) else {
                continue;
            };
            delivery.enterprise_scope = trigger.enterprise_scope();
            upgraded |= delivery.enterprise_scope.is_some();
        }
        *self.automation_webhook_deliveries.write().await = deliveries;
        if upgraded {
            self.persist_automation_webhook_deliveries_locked().await?;
        }
        Ok(())
    }

    async fn load_automation_webhook_secret_material_locked(&self) -> anyhow::Result<()> {
        let secrets = if self.automation_webhook_secret_material_path.exists() {
            super::check_file_permissions(&self.automation_webhook_secret_material_path);
            let raw = fs::read_to_string(&self.automation_webhook_secret_material_path).await?;
            parse_automation_webhook_secret_material_file(&raw)?
        } else {
            HashMap::new()
        };
        *self.automation_webhook_secret_material.write().await = secrets;
        Ok(())
    }

    async fn persist_automation_webhook_triggers_locked(&self) -> anyhow::Result<()> {
        let triggers = self.automation_webhook_triggers.read().await.clone();
        let payload = serialize_automation_webhook_triggers_file(triggers)?;
        ensure_parent_dir(&self.automation_webhook_triggers_path).await?;
        super::write_state_file_atomically(&self.automation_webhook_triggers_path, payload).await
    }

    pub(crate) async fn persist_automation_webhook_deliveries_locked(&self) -> anyhow::Result<()> {
        let deliveries = self.automation_webhook_deliveries.read().await.clone();
        let payload = serialize_automation_webhook_deliveries_file(deliveries)?;
        ensure_parent_dir(&self.automation_webhook_deliveries_path).await?;
        super::write_state_file_atomically(&self.automation_webhook_deliveries_path, payload).await
    }

    pub(crate) fn set_allow_unsigned_dev_webhooks(&self, allowed: bool) {
        self.allow_unsigned_dev_webhooks
            .store(allowed, std::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn unsigned_dev_webhooks_allowed(&self) -> bool {
        self.allow_unsigned_dev_webhooks
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    fn validate_webhook_signature_scheme_allowed(
        &self,
        scheme: AutomationWebhookSignatureScheme,
    ) -> anyhow::Result<AutomationWebhookSignatureScheme> {
        if matches!(scheme, AutomationWebhookSignatureScheme::UnsignedDevMode)
            && !self.unsigned_dev_webhooks_allowed()
        {
            anyhow::bail!(
                "unsigned_dev_mode webhook signature scheme requires an explicit dev/test server flag"
            );
        }
        Ok(scheme)
    }

    async fn persist_automation_webhook_secret_material_locked(&self) -> anyhow::Result<()> {
        let secrets = self.automation_webhook_secret_material.read().await.clone();
        let payload = serialize_automation_webhook_secret_material_file(secrets)?;
        ensure_parent_dir(&self.automation_webhook_secret_material_path).await?;
        write_secret_material_file_atomically(
            &self.automation_webhook_secret_material_path,
            &payload,
        )
        .await
    }

    pub(crate) async fn create_automation_webhook_trigger(
        &self,
        input: AutomationWebhookTriggerCreateInput,
    ) -> anyhow::Result<AutomationWebhookCreateResult> {
        let provider = normalize_automation_webhook_provider(&input.provider)
            .ok_or_else(|| anyhow::anyhow!("webhook provider is required"))?;
        let name = input
            .name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(provider.as_str())
            .to_string();

        {
            let automations = self.automations_v2.read().await;
            let automation = automations
                .get(&input.automation_id)
                .with_context(|| format!("automation `{}` not found", input.automation_id))?;
            let automation_tenant = automation.tenant_context();
            if !tenant_context_matches(&automation_tenant, &input.tenant_context) {
                anyhow::bail!("automation webhook trigger tenant does not match automation tenant");
            }
        }

        let requested_scheme = self.validate_webhook_signature_scheme_allowed(
            input.signature_scheme.clone().unwrap_or_default(),
        )?;
        let _guard = self.automation_webhook_persistence.lock().await;
        let now = now_ms();
        let trigger_id = format!("whtr_{}", Uuid::new_v4().simple());
        let secret_version = 1;
        let secret = new_secret();
        let secret_ref = secret_ref_for_trigger(&input.tenant_context, &trigger_id, secret_version);
        secret_ref
            .validate_for_tenant(&input.tenant_context)
            .map_err(|error| anyhow::anyhow!("webhook secret ref tenant mismatch: {error:?}"))?;
        let secret_digest = secret_digest(&secret, &input.tenant_context, &trigger_id);
        let public_path_token = {
            let triggers = self.automation_webhook_triggers.read().await;
            new_public_path_token(&triggers)
        };
        let trigger = AutomationWebhookTriggerRecord {
            trigger_id: trigger_id.clone(),
            automation_id: input.automation_id,
            tenant_context: input.tenant_context.clone(),
            owner_principal: input.owner_principal,
            created_by: input.created_by.clone(),
            updated_by: input.created_by,
            owning_org_unit_id: input.owning_org_unit_id,
            resource_scope: input.resource_scope,
            default_data_class: input.default_data_class,
            default_risk_tier: input.default_risk_tier,
            name,
            provider,
            provider_event_kind: input
                .provider_event_kind
                .as_deref()
                .and_then(normalize_automation_webhook_provider_event_kind),
            enabled: input.enabled,
            public_path_token,
            signature_scheme: requested_scheme,
            secret: AutomationWebhookSecretMetadata {
                secret_ref: secret_ref.clone(),
                secret_digest,
                secret_version,
                created_at_ms: now,
                rotated_at_ms: None,
                rotated_by: None,
            },
            created_at_ms: now,
            updated_at_ms: now,
            last_received_at_ms: None,
            last_accepted_at_ms: None,
            last_rejected_at_ms: None,
        };
        let material = AutomationWebhookSecretMaterialRecord {
            secret_ref: secret_ref.clone(),
            tenant_context: input.tenant_context,
            trigger_id: trigger_id.clone(),
            secret_version,
            secret: secret.clone(),
            created_at_ms: now,
            rotated_at_ms: None,
            rotated_by: None,
        };

        let secret_key = secret_material_key(&secret_ref);
        self.automation_webhook_secret_material
            .write()
            .await
            .insert(secret_key.clone(), material);
        if let Err(error) = self
            .persist_automation_webhook_secret_material_locked()
            .await
        {
            self.automation_webhook_secret_material
                .write()
                .await
                .remove(&secret_key);
            return Err(error.context("failed to persist webhook secret material"));
        }

        self.automation_webhook_triggers
            .write()
            .await
            .insert(trigger_id.clone(), trigger.clone());
        if let Err(error) = self.persist_automation_webhook_triggers_locked().await {
            self.automation_webhook_triggers
                .write()
                .await
                .remove(&trigger_id);
            self.automation_webhook_secret_material
                .write()
                .await
                .remove(&secret_key);
            if let Err(cleanup_error) = self
                .persist_automation_webhook_secret_material_locked()
                .await
            {
                tracing::warn!(
                    target: "tandem_server::state",
                    error = ?cleanup_error,
                    trigger_id,
                    "failed to clean up webhook secret material after trigger persist failure"
                );
            }
            return Err(error.context("failed to persist webhook trigger metadata"));
        }

        Ok(AutomationWebhookCreateResult { trigger, secret })
    }

    pub(crate) async fn rotate_automation_webhook_secret(
        &self,
        tenant_context: &TenantContext,
        trigger_id: &str,
        actor_id: Option<String>,
    ) -> anyhow::Result<AutomationWebhookRotationResult> {
        let _guard = self.automation_webhook_persistence.lock().await;
        let now = now_ms();
        let secret = new_secret();
        let current_trigger = {
            let triggers = self.automation_webhook_triggers.read().await;
            let trigger = triggers
                .get(trigger_id)
                .with_context(|| format!("webhook trigger `{trigger_id}` not found"))?
                .clone();
            if !trigger.tenant_matches(tenant_context) {
                anyhow::bail!("webhook trigger tenant mismatch");
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

        let mut trigger = current_trigger.clone();
        trigger.secret = AutomationWebhookSecretMetadata {
            secret_ref: secret_ref.clone(),
            secret_digest: secret_digest(&secret, tenant_context, trigger_id),
            secret_version,
            created_at_ms: now,
            rotated_at_ms: Some(now),
            rotated_by: actor_id.clone(),
        };
        trigger.updated_at_ms = now;
        trigger.updated_by = actor_id.clone();

        let material = AutomationWebhookSecretMaterialRecord {
            secret_ref: secret_ref.clone(),
            tenant_context: tenant_context.clone(),
            trigger_id: trigger_id.to_string(),
            secret_version,
            secret: secret.clone(),
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
            return Err(error.context("failed to persist rotated webhook secret material"));
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
                    "failed to clean up rotated webhook secret material after trigger persist failure"
                );
            }
            return Err(error.context("failed to persist rotated webhook trigger metadata"));
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
                "failed to remove old webhook secret material after successful rotation"
            );
        }

        Ok(AutomationWebhookRotationResult { trigger, secret })
    }

    pub(crate) async fn list_automation_webhook_triggers_for_automation(
        &self,
        tenant_context: &TenantContext,
        automation_id: &str,
    ) -> Vec<AutomationWebhookTriggerRecord> {
        self.automation_webhook_triggers
            .read()
            .await
            .values()
            .filter(|trigger| {
                trigger.automation_id == automation_id && trigger.tenant_matches(tenant_context)
            })
            .cloned()
            .collect()
    }

    pub(crate) async fn get_automation_webhook_trigger(
        &self,
        tenant_context: &TenantContext,
        trigger_id: &str,
    ) -> Option<AutomationWebhookTriggerRecord> {
        self.automation_webhook_triggers
            .read()
            .await
            .get(trigger_id)
            .filter(|trigger| trigger.tenant_matches(tenant_context))
            .cloned()
    }

    pub(crate) async fn update_automation_webhook_trigger(
        &self,
        tenant_context: &TenantContext,
        automation_id: &str,
        trigger_id: &str,
        input: AutomationWebhookTriggerUpdateInput,
        actor_id: Option<String>,
    ) -> anyhow::Result<AutomationWebhookTriggerRecord> {
        let _guard = self.automation_webhook_persistence.lock().await;
        let updated = {
            let mut triggers = self.automation_webhook_triggers.write().await;
            let trigger = triggers
                .get_mut(trigger_id)
                .with_context(|| format!("webhook trigger `{trigger_id}` not found"))?;
            if !trigger.tenant_matches(tenant_context) || trigger.automation_id != automation_id {
                anyhow::bail!("webhook trigger tenant or automation mismatch");
            }
            let mut updated_trigger = trigger.clone();
            if let Some(name) = input.name {
                let name = name.trim();
                if name.is_empty() {
                    anyhow::bail!("webhook trigger name is required");
                }
                updated_trigger.name = name.to_string();
            }
            if let Some(provider) = input.provider {
                let provider = normalize_automation_webhook_provider(&provider)
                    .ok_or_else(|| anyhow::anyhow!("webhook provider is required"))?;
                updated_trigger.provider = provider.clone();
                if updated_trigger.name.trim().is_empty() {
                    updated_trigger.name = provider;
                }
            }
            if let Some(provider_event_kind) = input.provider_event_kind {
                updated_trigger.provider_event_kind = provider_event_kind
                    .as_deref()
                    .and_then(normalize_automation_webhook_provider_event_kind);
            }
            if let Some(signature_scheme) = input.signature_scheme {
                updated_trigger.signature_scheme =
                    self.validate_webhook_signature_scheme_allowed(signature_scheme)?;
            }
            if let Some(default_data_class) = input.default_data_class {
                updated_trigger.default_data_class = default_data_class;
            }
            if let Some(default_risk_tier) = input.default_risk_tier {
                updated_trigger.default_risk_tier = default_risk_tier;
            }
            if let Some(enabled) = input.enabled {
                updated_trigger.enabled = enabled;
            }
            updated_trigger.updated_at_ms = now_ms();
            updated_trigger.updated_by = actor_id;
            *trigger = updated_trigger.clone();
            updated_trigger
        };
        self.persist_automation_webhook_triggers_locked().await?;
        Ok(updated)
    }

    pub(crate) async fn get_automation_webhook_trigger_by_public_token(
        &self,
        public_path_token: &str,
    ) -> Option<AutomationWebhookTriggerRecord> {
        self.automation_webhook_triggers
            .read()
            .await
            .values()
            .find(|trigger| trigger.public_path_token == public_path_token)
            .cloned()
    }

    pub(crate) async fn disable_automation_webhook_trigger(
        &self,
        tenant_context: &TenantContext,
        trigger_id: &str,
        actor_id: Option<String>,
    ) -> anyhow::Result<AutomationWebhookTriggerRecord> {
        let _guard = self.automation_webhook_persistence.lock().await;
        let updated = {
            let mut triggers = self.automation_webhook_triggers.write().await;
            let trigger = triggers
                .get_mut(trigger_id)
                .with_context(|| format!("webhook trigger `{trigger_id}` not found"))?;
            if !trigger.tenant_matches(tenant_context) {
                anyhow::bail!("webhook trigger tenant mismatch");
            }
            trigger.enabled = false;
            trigger.updated_at_ms = now_ms();
            trigger.updated_by = actor_id;
            trigger.clone()
        };
        self.persist_automation_webhook_triggers_locked().await?;
        Ok(updated)
    }

    pub(crate) async fn delete_automation_webhook_trigger(
        &self,
        tenant_context: &TenantContext,
        trigger_id: &str,
    ) -> anyhow::Result<bool> {
        let _guard = self.automation_webhook_persistence.lock().await;
        let secret_key = {
            let mut triggers = self.automation_webhook_triggers.write().await;
            let Some(trigger) = triggers.get(trigger_id) else {
                return Ok(false);
            };
            if !trigger.tenant_matches(tenant_context) {
                anyhow::bail!("webhook trigger tenant mismatch");
            }
            let secret_key = secret_material_key(&trigger.secret.secret_ref);
            triggers.remove(trigger_id);
            secret_key
        };
        self.automation_webhook_secret_material
            .write()
            .await
            .remove(&secret_key);
        self.persist_automation_webhook_triggers_locked().await?;
        self.persist_automation_webhook_secret_material_locked()
            .await?;
        Ok(true)
    }

    async fn record_automation_webhook_delivery_locked(
        &self,
        mut delivery: AutomationWebhookDeliveryRecord,
    ) -> anyhow::Result<AutomationWebhookDeliveryRecord> {
        let now = now_ms();
        let updated_trigger = {
            let mut triggers = self.automation_webhook_triggers.write().await;
            let trigger = triggers
                .get_mut(&delivery.trigger_id)
                .with_context(|| format!("webhook trigger `{}` not found", delivery.trigger_id))?;
            if !trigger.tenant_matches(&delivery.tenant_context)
                || trigger.automation_id != delivery.automation_id
            {
                anyhow::bail!("webhook delivery tenant or automation mismatch");
            }
            trigger.last_received_at_ms = Some(delivery.received_at_ms);
            match delivery.status {
                AutomationWebhookDeliveryStatus::Accepted => {
                    let accepted_at_ms = delivery.accepted_at_ms.unwrap_or(now);
                    delivery.accepted_at_ms = Some(accepted_at_ms);
                    trigger.last_accepted_at_ms = Some(accepted_at_ms);
                }
                AutomationWebhookDeliveryStatus::Rejected
                | AutomationWebhookDeliveryStatus::Duplicate
                | AutomationWebhookDeliveryStatus::Suppressed
                | AutomationWebhookDeliveryStatus::Disabled
                | AutomationWebhookDeliveryStatus::Failed => {
                    let rejected_at_ms = delivery.rejected_at_ms.unwrap_or(now);
                    delivery.rejected_at_ms = Some(rejected_at_ms);
                    trigger.last_rejected_at_ms = Some(rejected_at_ms);
                }
                AutomationWebhookDeliveryStatus::Received => {}
            }
            trigger.updated_at_ms = now;
            trigger.clone()
        };
        delivery.sanitized_preview =
            sanitize_automation_webhook_preview(&delivery.sanitized_preview);
        if delivery.correlation.is_none() {
            delivery.correlation = Some(automation_webhook_delivery_correlation(&delivery, None));
        }
        self.automation_webhook_deliveries
            .write()
            .await
            .insert(delivery.delivery_id.clone(), delivery.clone());
        self.persist_automation_webhook_triggers_locked().await?;
        self.persist_automation_webhook_deliveries_locked().await?;
        tracing::debug!(
            trigger_id = %updated_trigger.trigger_id,
            delivery_id = %delivery.delivery_id,
            status = ?delivery.status,
            "recorded automation webhook delivery"
        );
        Ok(delivery)
    }

    async fn attach_automation_webhook_delivery_run_locked(
        &self,
        tenant_context: &TenantContext,
        delivery_id: &str,
        run_id: &str,
    ) -> anyhow::Result<AutomationWebhookDeliveryRecord> {
        let delivery = {
            let mut deliveries = self.automation_webhook_deliveries.write().await;
            let delivery = deliveries
                .get_mut(delivery_id)
                .with_context(|| format!("webhook delivery `{delivery_id}` not found"))?;
            if !delivery.tenant_matches(tenant_context) {
                anyhow::bail!("webhook delivery tenant mismatch");
            }
            if !matches!(delivery.status, AutomationWebhookDeliveryStatus::Accepted) {
                anyhow::bail!("webhook delivery is not accepted");
            }
            if let Some(existing_run_id) = delivery.queued_run_id.as_ref() {
                if existing_run_id != run_id {
                    anyhow::bail!("webhook delivery already linked to another run");
                }
            }
            delivery.queued_run_id = Some(run_id.to_string());
            delivery.correlation = Some(automation_webhook_delivery_correlation(delivery, None));
            delivery.clone()
        };
        self.persist_automation_webhook_deliveries_locked().await?;
        Ok(delivery)
    }

    pub(crate) async fn record_automation_webhook_delivery(
        &self,
        delivery: AutomationWebhookDeliveryRecord,
    ) -> anyhow::Result<AutomationWebhookDeliveryRecord> {
        let _guard = self.automation_webhook_persistence.lock().await;
        self.record_automation_webhook_delivery_locked(delivery)
            .await
    }

    pub(crate) async fn record_automation_webhook_rejection(
        &self,
        trigger: &AutomationWebhookTriggerRecord,
        provider_event_id: Option<String>,
        body_digest: String,
        status: AutomationWebhookDeliveryStatus,
        reason_code: impl Into<String>,
        received_at_ms: u64,
        sanitized_preview: Value,
        verification: Option<AutomationWebhookVerificationDecision>,
    ) -> anyhow::Result<AutomationWebhookDeliveryRecord> {
        let delivery = automation_webhook_rejection_delivery(
            trigger,
            provider_event_id,
            body_digest,
            status,
            reason_code,
            received_at_ms,
            sanitized_preview,
            verification,
        );
        self.record_automation_webhook_delivery(delivery).await
    }

    async fn wake_matching_stateful_webhook_wait_locked(
        &self,
        trigger: &AutomationWebhookTriggerRecord,
        provider_event_id: Option<String>,
        body_digest: String,
        received_at_ms: u64,
        sanitized_preview: Value,
        verification: AutomationWebhookVerificationDecision,
        primary_idempotency: Option<AutomationWebhookReservedClaim>,
        feedback_loop: Option<AutomationWebhookFeedbackLoopDecision>,
    ) -> anyhow::Result<Option<AutomationWebhookStatefulWaitResult>> {
        let paths =
            StatefulRuntimeStoragePaths::from_runtime_events_path(&self.runtime_events_path);
        let wait_event = automation_webhook_stateful_wait_event(
            trigger,
            provider_event_id.clone(),
            body_digest.clone(),
        );
        let Some(claimed_wait) = claim_matching_stateful_webhook_wait(
            &paths.waits_path,
            &trigger.tenant_context,
            &wait_event,
            AUTOMATION_WEBHOOK_STATEFUL_WAIT_CLAIMANT,
            received_at_ms,
            AUTOMATION_WEBHOOK_STATEFUL_WAIT_LEASE_MS,
        )
        .await?
        else {
            return Ok(None);
        };

        let delivery_id = new_automation_webhook_delivery_id();
        let wake_key = stateful_webhook_wake_key(&claimed_wait, &wait_event);
        let event_id = format!("stateful-webhook-wake-{wake_key}");
        let status = StatefulWorkflowRunStatus::Running;
        if let Err(error) =
            guarded_phase_state_for_webhook_wait(&paths, &claimed_wait, received_at_ms)
        {
            let reason = error.to_string();
            cancel_webhook_wait_after_phase_guard_denial(
                &paths,
                &claimed_wait,
                &reason,
                received_at_ms,
            )
            .await;
            let delivery = automation_webhook_phase_denied_delivery(
                trigger,
                provider_event_id,
                body_digest,
                received_at_ms,
                sanitized_preview,
                &verification,
                primary_idempotency.as_ref(),
            );
            let delivery = self
                .record_automation_webhook_delivery_locked(delivery)
                .await?;
            return Ok(Some(AutomationWebhookStatefulWaitResult::Rejected {
                delivery,
                reason_code: "stateful_wait_phase_denied".to_string(),
            }));
        }
        let reserved_wait = begin_claimed_stateful_wait_wake_completion(
            &paths.waits_path,
            &trigger.tenant_context,
            &claimed_wait,
            &wake_key,
            received_at_ms,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("stateful webhook wait wake conflict"))?;
        let phase_state =
            match guarded_phase_state_for_webhook_wait(&paths, &reserved_wait, received_at_ms) {
                Ok(phase_state) => phase_state,
                Err(error) => {
                    let reason = error.to_string();
                    cancel_webhook_wait_after_phase_guard_denial(
                        &paths,
                        &reserved_wait,
                        &reason,
                        received_at_ms,
                    )
                    .await;
                    let delivery = automation_webhook_phase_denied_delivery(
                        trigger,
                        provider_event_id,
                        body_digest,
                        received_at_ms,
                        sanitized_preview,
                        &verification,
                        primary_idempotency.as_ref(),
                    );
                    let delivery = self
                        .record_automation_webhook_delivery_locked(delivery)
                        .await?;
                    return Ok(Some(AutomationWebhookStatefulWaitResult::Rejected {
                        delivery,
                        reason_code: "stateful_wait_phase_denied".to_string(),
                    }));
                }
            };
        let scope = claimed_wait.scope.clone();
        let event = StatefulRunEventRecord {
            schema_version: 1,
            event_id: event_id.clone(),
            run_id: claimed_wait.run_id.clone(),
            seq: 0,
            event_type: "stateful_runtime.wait.webhook_woken".to_string(),
            occurred_at_ms: received_at_ms,
            scope: scope.clone(),
            actor: trigger.owner_principal.clone(),
            phase_id: claimed_wait.phase_id.clone(),
            phase_transition: None,
            wait_kind: Some(StatefulWaitKind::Webhook),
            causation_id: Some(delivery_id.clone()),
            correlation_id: provider_event_id
                .clone()
                .or_else(|| Some(body_digest.clone())),
            payload: json!({
                "delivery_id": &delivery_id,
                "trigger_id": &trigger.trigger_id,
                "automation_id": &trigger.automation_id,
                "provider": &trigger.provider,
                "provider_event_kind": &trigger.provider_event_kind,
                "provider_event_id": &provider_event_id,
                "body_digest": &body_digest,
                "wait_id": &claimed_wait.wait_id,
                "wake_idempotency_key": &wake_key,
            }),
        };
        let (_appended, seq) = append_stateful_run_event_once_with_next_seq(
            &paths.run_events_path,
            &trigger.tenant_context,
            &event,
        )
        .await?;
        let _ = self
            .requeue_automation_v2_run_from_stateful_wait_wake(
                &reserved_wait.run_id,
                &reserved_wait.wait_id,
                "stateful_runtime.wait.webhook_woken",
                seq,
                format!(
                    "stateful webhook wait `{}` woke from delivery `{delivery_id}`",
                    reserved_wait.wait_id
                ),
                json!({
                    "delivery_id": &delivery_id,
                    "trigger_id": &trigger.trigger_id,
                    "provider": &trigger.provider,
                    "provider_event_kind": &trigger.provider_event_kind,
                    "provider_event_id": &provider_event_id,
                    "body_digest": &body_digest,
                }),
            )
            .await;
        let snapshot = StatefulRunSnapshotRecord {
            schema_version: 1,
            snapshot_id: format!("stateful-webhook-wake-{delivery_id}"),
            run_id: reserved_wait.run_id.clone(),
            seq,
            created_at_ms: received_at_ms,
            scope,
            status,
            phase: phase_state.phase,
            phase_history: phase_state.phase_history,
            allowed_next_phases: phase_state.allowed_next_phases,
            phase_id: reserved_wait.phase_id.clone(),
            source_record_kind: Some(StatefulWorkflowRunKind::AutomationV2),
            checkpoint: None,
            payload_digest: Some(body_digest.clone()),
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            metadata: Some(json!({
                "source": "automation_webhook",
                "delivery_id": &delivery_id,
                "trigger_id": &trigger.trigger_id,
                "provider": &trigger.provider,
                "provider_event_id": &provider_event_id,
                "body_digest": &body_digest,
                "wait_id": &reserved_wait.wait_id,
            })),
        };
        write_stateful_run_snapshot(&paths.snapshots_root, &snapshot).await?;

        let delivery = automation_webhook_accepted_delivery(
            Some(delivery_id.clone()),
            trigger,
            provider_event_id,
            body_digest,
            received_at_ms,
            sanitized_preview,
            &verification,
            primary_idempotency.as_ref(),
            Some(reserved_wait.run_id.clone()),
            Some(reserved_wait.wait_id.clone()),
            feedback_loop,
        );
        let delivery = self
            .record_automation_webhook_delivery_locked(delivery)
            .await?;
        let woken_wait = finish_claimed_stateful_wait_completion(
            &paths.waits_path,
            &trigger.tenant_context,
            &reserved_wait,
            &wake_key,
            seq,
            StatefulWaitStatus::Woken,
            received_at_ms,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("stateful webhook wait wake conflict"))?;
        self.event_bus.publish(crate::EngineEvent::new(
            "stateful_runtime.wait.webhook_woken",
            json!({
                "runID": &woken_wait.run_id,
                "waitID": &woken_wait.wait_id,
                "deliveryID": &delivery.delivery_id,
                "triggerID": &trigger.trigger_id,
                "provider": &trigger.provider,
                "tenantContext": &trigger.tenant_context,
            }),
        ));
        Ok(Some(AutomationWebhookStatefulWaitResult::Woken {
            delivery,
            wait: woken_wait,
        }))
    }

    pub(crate) async fn queue_automation_v2_run_from_webhook_delivery(
        &self,
        verified: VerifiedAutomationWebhookRequest,
        sanitized_preview: Value,
    ) -> anyhow::Result<AutomationWebhookQueueResult> {
        // Box the large inner future so this entry point's frame stays small.
        // The delivery-queueing future is big and is often driven concurrently
        // (e.g. two deliveries under tokio::join!); keeping it on the heap gives
        // real headroom against the default 2 MiB worker/test stack instead of
        // sitting one unrelated codegen change away from a stack overflow.
        Box::pin(
            self.queue_automation_v2_run_from_webhook_delivery_with_feedback_loop(
                verified,
                sanitized_preview,
                None,
            ),
        )
        .await
    }

    pub(crate) async fn queue_automation_v2_run_from_webhook_delivery_with_feedback_loop(
        &self,
        verified: VerifiedAutomationWebhookRequest,
        sanitized_preview: Value,
        feedback_loop_candidate: Option<AutomationWebhookFeedbackLoopCandidate>,
    ) -> anyhow::Result<AutomationWebhookQueueResult> {
        let trigger = verified.trigger;
        let verification = verified.verification.clone();
        let sanitized_preview = sanitize_automation_webhook_preview(&sanitized_preview);
        let provider_event_id = verified.provider_event_id.clone();
        let body_digest = verified.body_digest.clone();
        let received_at_ms = verified.received_at_ms;
        let automation = match self.get_automation_v2(&trigger.automation_id).await {
            Some(automation) => automation,
            None => {
                let delivery = self
                    .record_automation_webhook_rejection(
                        &trigger,
                        provider_event_id,
                        body_digest,
                        AutomationWebhookDeliveryStatus::Failed,
                        "automation_missing",
                        received_at_ms,
                        sanitized_preview,
                        Some(verification.clone()),
                    )
                    .await?;
                return Ok(AutomationWebhookQueueResult::Rejected {
                    delivery,
                    reason_code: "automation_missing".to_string(),
                });
            }
        };
        if !tenant_context_matches(&automation.tenant_context(), &trigger.tenant_context) {
            let delivery = self
                .record_automation_webhook_rejection(
                    &trigger,
                    provider_event_id,
                    body_digest,
                    AutomationWebhookDeliveryStatus::Rejected,
                    "automation_tenant_mismatch",
                    received_at_ms,
                    sanitized_preview,
                    Some(verification.clone()),
                )
                .await?;
            return Ok(AutomationWebhookQueueResult::Rejected {
                delivery,
                reason_code: "automation_tenant_mismatch".to_string(),
            });
        }
        if !matches!(automation.status, AutomationV2Status::Active) {
            let delivery = self
                .record_automation_webhook_rejection(
                    &trigger,
                    provider_event_id,
                    body_digest,
                    AutomationWebhookDeliveryStatus::Rejected,
                    "automation_inactive",
                    received_at_ms,
                    sanitized_preview,
                    Some(verification.clone()),
                )
                .await?;
            return Ok(AutomationWebhookQueueResult::Rejected {
                delivery,
                reason_code: "automation_inactive".to_string(),
            });
        }
        if let Some(reason_code) = automation_webhook_scope_denial_reason(&trigger, &automation) {
            let delivery = self
                .record_automation_webhook_rejection(
                    &trigger,
                    provider_event_id,
                    body_digest,
                    AutomationWebhookDeliveryStatus::Rejected,
                    reason_code,
                    received_at_ms,
                    sanitized_preview,
                    Some(verification.clone()),
                )
                .await?;
            return Ok(AutomationWebhookQueueResult::Rejected {
                delivery,
                reason_code: reason_code.to_string(),
            });
        }
        let feedback_loop = self
            .classify_automation_webhook_feedback_loop(&trigger, feedback_loop_candidate.as_ref())
            .await;

        let accepted_idempotency_records: Vec<AutomationWebhookReservedClaim>;
        let delivery = {
            let _guard = self.automation_webhook_persistence.lock().await;
            let current_trigger = self
                .automation_webhook_triggers
                .read()
                .await
                .get(&trigger.trigger_id)
                .cloned()
                .filter(|current| {
                    current.tenant_matches(&trigger.tenant_context)
                        && current.automation_id == trigger.automation_id
                })
                .ok_or_else(|| {
                    anyhow::anyhow!("webhook trigger changed before delivery queueing")
                })?;
            if !current_trigger.enabled {
                let delivery = automation_webhook_rejection_delivery(
                    &trigger,
                    provider_event_id,
                    body_digest,
                    AutomationWebhookDeliveryStatus::Disabled,
                    "trigger_disabled",
                    received_at_ms,
                    sanitized_preview,
                    Some(verification.clone()),
                );
                let delivery = self
                    .record_automation_webhook_delivery_locked(delivery)
                    .await?;
                return Ok(AutomationWebhookQueueResult::Rejected {
                    delivery,
                    reason_code: "trigger_disabled".to_string(),
                });
            }
            let dedupe = self
                .reserve_automation_webhook_dedupe(
                    &trigger,
                    provider_event_id.as_ref(),
                    &body_digest,
                    received_at_ms,
                )
                .await?;
            let reserved_records = dedupe.accepted_records();
            match dedupe {
                AutomationWebhookDedupeDecision::Duplicate {
                    primary_claim,
                    primary_record,
                    reserved_records,
                } => {
                    let (mut duplicate_of_delivery_id, mut duplicate_of_run_id) =
                        idempotency_outcome_ref(&primary_record);
                    if duplicate_of_delivery_id.is_none() {
                        let original_delivery = {
                            let deliveries = self.automation_webhook_deliveries.read().await;
                            deliveries
                                .values()
                                .find(|delivery| {
                                    automation_webhook_delivery_matches_key(
                                        delivery,
                                        &trigger,
                                        provider_event_id.as_ref(),
                                        &body_digest,
                                    )
                                })
                                .cloned()
                        };
                        if let Some(original) = original_delivery {
                            duplicate_of_delivery_id = Some(original.delivery_id);
                            duplicate_of_run_id = original.queued_run_id;
                        }
                    }
                    let duplicate_suppressed_feedback =
                        feedback_loop.as_ref().is_some_and(|decision| {
                            matches!(
                                decision.outcome,
                                AutomationWebhookFeedbackLoopOutcome::Suppressed
                            )
                        });
                    if !duplicate_suppressed_feedback {
                        if let Some(stateful_wait_result) = self
                            .wake_matching_stateful_webhook_wait_locked(
                                &trigger,
                                verified.provider_event_id.clone(),
                                verified.body_digest.clone(),
                                verified.received_at_ms,
                                sanitized_preview.clone(),
                                verification.clone(),
                                Some(AutomationWebhookReservedClaim {
                                    claim: primary_claim.clone(),
                                    record: primary_record.clone(),
                                }),
                                feedback_loop.clone(),
                            )
                            .await?
                        {
                            match stateful_wait_result {
                                AutomationWebhookStatefulWaitResult::Woken { delivery, wait } => {
                                    self.complete_automation_webhook_idempotency_records(
                                        &reserved_records,
                                        &delivery,
                                        "woken",
                                        received_at_ms,
                                    )
                                    .await?;
                                    return Ok(AutomationWebhookQueueResult::Woken {
                                        delivery,
                                        wait,
                                    });
                                }
                                AutomationWebhookStatefulWaitResult::Rejected {
                                    delivery,
                                    reason_code,
                                } => {
                                    self.complete_automation_webhook_idempotency_records(
                                        &reserved_records,
                                        &delivery,
                                        "rejected",
                                        received_at_ms,
                                    )
                                    .await?;
                                    return Ok(AutomationWebhookQueueResult::Rejected {
                                        delivery,
                                        reason_code,
                                    });
                                }
                            }
                        }
                    }
                    let mut delivery = automation_webhook_rejection_delivery(
                        &trigger,
                        provider_event_id,
                        body_digest,
                        AutomationWebhookDeliveryStatus::Duplicate,
                        "duplicate_delivery",
                        received_at_ms,
                        sanitized_preview,
                        Some(verification.clone()),
                    );
                    delivery.idempotency_key = Some(primary_claim.key);
                    delivery.idempotency_record_id = Some(primary_record.record_id);
                    delivery.dedupe_result = Some(AutomationWebhookDedupeResult::Duplicate);
                    delivery.dedupe_reason_code =
                        Some(format!("duplicate_{}", primary_claim.key_kind));
                    delivery.duplicate_of_delivery_id = duplicate_of_delivery_id;
                    delivery.duplicate_of_run_id = duplicate_of_run_id;
                    let delivery = self
                        .record_automation_webhook_delivery_locked(delivery)
                        .await?;
                    self.complete_automation_webhook_idempotency_records(
                        &reserved_records,
                        &delivery,
                        "duplicate",
                        received_at_ms,
                    )
                    .await?;
                    return Ok(AutomationWebhookQueueResult::Duplicate { delivery });
                }
                AutomationWebhookDedupeDecision::Conflict {
                    primary_claim,
                    primary_record,
                    reserved_records,
                } => {
                    let (duplicate_of_delivery_id, duplicate_of_run_id) =
                        idempotency_outcome_ref(&primary_record);
                    let mut delivery = automation_webhook_rejection_delivery(
                        &trigger,
                        provider_event_id,
                        body_digest,
                        AutomationWebhookDeliveryStatus::Rejected,
                        "idempotency_conflict",
                        received_at_ms,
                        sanitized_preview,
                        Some(verification.clone()),
                    );
                    delivery.idempotency_key = Some(primary_claim.key);
                    delivery.idempotency_record_id = Some(primary_record.record_id);
                    delivery.dedupe_result = Some(AutomationWebhookDedupeResult::Conflict);
                    delivery.dedupe_reason_code =
                        Some(format!("conflicting_{}", primary_claim.key_kind));
                    delivery.duplicate_of_delivery_id = duplicate_of_delivery_id;
                    delivery.duplicate_of_run_id = duplicate_of_run_id;
                    let delivery = self
                        .record_automation_webhook_delivery_locked(delivery)
                        .await?;
                    self.complete_automation_webhook_idempotency_records(
                        &reserved_records,
                        &delivery,
                        "conflict",
                        received_at_ms,
                    )
                    .await?;
                    return Ok(AutomationWebhookQueueResult::Rejected {
                        delivery,
                        reason_code: "idempotency_conflict".to_string(),
                    });
                }
                AutomationWebhookDedupeDecision::New { .. } => {}
            }
            let original_delivery = {
                let deliveries = self.automation_webhook_deliveries.read().await;
                deliveries
                    .values()
                    .find(|delivery| {
                        automation_webhook_delivery_matches_key(
                            delivery,
                            &trigger,
                            provider_event_id.as_ref(),
                            &body_digest,
                        )
                    })
                    .cloned()
            };
            if let Some(original) = original_delivery {
                let primary = reserved_records.first();
                let mut delivery = automation_webhook_rejection_delivery(
                    &trigger,
                    provider_event_id,
                    body_digest,
                    AutomationWebhookDeliveryStatus::Duplicate,
                    "duplicate_delivery",
                    received_at_ms,
                    sanitized_preview,
                    Some(verification.clone()),
                );
                if let Some(primary) = primary {
                    delivery.idempotency_key = Some(primary.claim.key.clone());
                    delivery.idempotency_record_id = Some(primary.record.record_id.clone());
                    delivery.dedupe_reason_code =
                        Some(format!("duplicate_{}", primary.claim.key_kind));
                } else {
                    delivery.dedupe_reason_code = Some("duplicate_legacy_delivery".to_string());
                }
                delivery.dedupe_result = Some(AutomationWebhookDedupeResult::Duplicate);
                delivery.duplicate_of_delivery_id = Some(original.delivery_id);
                delivery.duplicate_of_run_id = original.queued_run_id;
                let delivery = self
                    .record_automation_webhook_delivery_locked(delivery)
                    .await?;
                self.complete_automation_webhook_idempotency_records(
                    &reserved_records,
                    &delivery,
                    "duplicate",
                    received_at_ms,
                )
                .await?;
                return Ok(AutomationWebhookQueueResult::Duplicate { delivery });
            }
            let primary = reserved_records.first();
            accepted_idempotency_records = reserved_records.clone();
            if let Some(feedback_loop) = feedback_loop.as_ref().filter(|decision| {
                matches!(
                    decision.outcome,
                    AutomationWebhookFeedbackLoopOutcome::Suppressed
                )
            }) {
                let mut delivery = automation_webhook_rejection_delivery(
                    &trigger,
                    provider_event_id,
                    body_digest,
                    AutomationWebhookDeliveryStatus::Suppressed,
                    feedback_loop.reason_code.clone(),
                    received_at_ms,
                    sanitized_preview,
                    Some(verification.clone()),
                );
                if let Some(primary) = primary {
                    delivery.idempotency_key = Some(primary.claim.key.clone());
                    delivery.idempotency_record_id = Some(primary.record.record_id.clone());
                    delivery.dedupe_reason_code =
                        Some(format!("suppressed_{}", primary.claim.key_kind));
                }
                delivery.dedupe_result = Some(AutomationWebhookDedupeResult::IgnoredFeedbackLoop);
                delivery.feedback_loop = Some(feedback_loop.clone());
                let delivery = self
                    .record_automation_webhook_delivery_locked(delivery)
                    .await?;
                self.complete_automation_webhook_idempotency_records(
                    &accepted_idempotency_records,
                    &delivery,
                    "suppressed",
                    received_at_ms,
                )
                .await?;
                return Ok(AutomationWebhookQueueResult::Suppressed { delivery });
            }
            if let Some(stateful_wait_result) = self
                .wake_matching_stateful_webhook_wait_locked(
                    &trigger,
                    verified.provider_event_id.clone(),
                    verified.body_digest.clone(),
                    verified.received_at_ms,
                    sanitized_preview.clone(),
                    verification.clone(),
                    primary.cloned(),
                    feedback_loop.clone(),
                )
                .await?
            {
                match stateful_wait_result {
                    AutomationWebhookStatefulWaitResult::Woken { delivery, wait } => {
                        self.complete_automation_webhook_idempotency_records(
                            &accepted_idempotency_records,
                            &delivery,
                            "woken",
                            received_at_ms,
                        )
                        .await?;
                        return Ok(AutomationWebhookQueueResult::Woken { delivery, wait });
                    }
                    AutomationWebhookStatefulWaitResult::Rejected {
                        delivery,
                        reason_code,
                    } => {
                        self.complete_automation_webhook_idempotency_records(
                            &accepted_idempotency_records,
                            &delivery,
                            "rejected",
                            received_at_ms,
                        )
                        .await?;
                        return Ok(AutomationWebhookQueueResult::Rejected {
                            delivery,
                            reason_code,
                        });
                    }
                }
            }

            let delivery = automation_webhook_accepted_delivery(
                None,
                &trigger,
                provider_event_id,
                body_digest,
                received_at_ms,
                sanitized_preview,
                &verification,
                primary,
                None,
                None,
                feedback_loop.clone(),
            );
            self.record_automation_webhook_delivery_locked(delivery)
                .await?
        };
        let run = self
            .create_automation_v2_run(&automation, "webhook")
            .await?;
        let delivery = {
            let _guard = self.automation_webhook_persistence.lock().await;
            self.attach_automation_webhook_delivery_run_locked(
                &trigger.tenant_context,
                &delivery.delivery_id,
                &run.run_id,
            )
            .await?
        };
        self.complete_automation_webhook_idempotency_records(
            &accepted_idempotency_records,
            &delivery,
            "accepted",
            now_ms(),
        )
        .await?;
        let webhook_metadata = automation_webhook_run_metadata(&trigger, &delivery);
        let trigger_reason = format!(
            "{} webhook delivery {}",
            trigger.provider, delivery.delivery_id
        );
        let run = self
            .update_automation_v2_run(&run.run_id, |row| {
                row.trigger_reason = Some(trigger_reason.clone());
                row.detail = Some(format!(
                    "Queued from {} webhook delivery {}",
                    trigger.provider, delivery.delivery_id
                ));
                if let Some(snapshot) = row.automation_snapshot.as_mut() {
                    insert_automation_metadata_value(
                        &mut snapshot.metadata,
                        "automation_webhook",
                        webhook_metadata.clone(),
                    );
                    snapshot.stamp_enterprise_scope_metadata();
                }
            })
            .await
            .unwrap_or(run);
        let _ =
            crate::http::context_runs::sync_automation_v2_run_blackboard(self, &automation, &run)
                .await;
        self.event_bus.publish(crate::EngineEvent::new(
            "automation.v2.run.created",
            json!({
                "automationID": run.automation_id,
                "runID": run.run_id,
                "run": run.clone(),
                "tenantContext": run.tenant_context,
                "triggerType": "webhook",
                "deliveryID": delivery.delivery_id,
                "triggerID": trigger.trigger_id,
                "provider": trigger.provider,
            }),
        ));
        Ok(AutomationWebhookQueueResult::Accepted { delivery, run })
    }

    pub(crate) async fn list_automation_webhook_deliveries_for_trigger(
        &self,
        tenant_context: &TenantContext,
        trigger_id: &str,
    ) -> Vec<AutomationWebhookDeliveryRecord> {
        self.automation_webhook_deliveries
            .read()
            .await
            .values()
            .filter(|delivery| {
                delivery.trigger_id == trigger_id && delivery.tenant_matches(tenant_context)
            })
            .cloned()
            .collect()
    }

    pub(crate) async fn get_automation_webhook_delivery(
        &self,
        tenant_context: &TenantContext,
        delivery_id: &str,
    ) -> Option<AutomationWebhookDeliveryRecord> {
        self.automation_webhook_deliveries
            .read()
            .await
            .get(delivery_id)
            .filter(|delivery| delivery.tenant_matches(tenant_context))
            .cloned()
    }
}
