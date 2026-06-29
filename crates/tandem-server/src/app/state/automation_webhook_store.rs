use std::collections::HashMap;

use anyhow::Context;
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
    append_stateful_run_event_once, claim_matching_stateful_webhook_wait, mark_stateful_wait_woken,
    query_stateful_run_events, write_stateful_run_snapshot, StatefulRunEventQuery,
    StatefulRunEventRecord, StatefulRunSnapshotRecord, StatefulRuntimeScope,
    StatefulRuntimeStoragePaths, StatefulWaitKind, StatefulWaitRecord, StatefulWebhookWaitEvent,
    StatefulWorkflowRunKind, StatefulWorkflowRunStatus,
};
use crate::util::time::now_ms;

use super::{
    verify_automation_webhook_signature, AppState, AutomationWebhookSignatureHeaders,
    AutomationWebhookSignatureVerificationContext, AutomationWebhookVerificationDecision,
    AutomationWebhookVerificationError,
};

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

fn tenant_context_matches(left: &TenantContext, right: &TenantContext) -> bool {
    left.org_id == right.org_id
        && left.workspace_id == right.workspace_id
        && left.deployment_id == right.deployment_id
}

fn secret_material_key(secret_ref: &SecretRef) -> String {
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

fn new_automation_webhook_delivery_id() -> String {
    format!("automation-webhook-delivery-{}", Uuid::new_v4())
}

fn automation_webhook_delivery_matches_key(
    delivery: &AutomationWebhookDeliveryRecord,
    trigger: &AutomationWebhookTriggerRecord,
    provider_event_id: Option<&String>,
    body_digest: &str,
) -> bool {
    if delivery.trigger_id != trigger.trigger_id
        || !delivery.tenant_matches(&trigger.tenant_context)
    {
        return false;
    }
    if !matches!(
        delivery.status,
        AutomationWebhookDeliveryStatus::Accepted | AutomationWebhookDeliveryStatus::Duplicate
    ) {
        return false;
    }
    delivery.body_digest == body_digest
        || provider_event_id
            .is_some_and(|event_id| delivery.provider_event_id.as_ref() == Some(event_id))
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

fn automation_webhook_run_metadata(
    trigger: &AutomationWebhookTriggerRecord,
    delivery: &AutomationWebhookDeliveryRecord,
) -> Value {
    json!({
        "trigger_id": trigger.trigger_id,
        "delivery_id": delivery.delivery_id,
        "provider": trigger.provider,
        "provider_event_kind": trigger.provider_event_kind,
        "provider_event_id": delivery.provider_event_id,
        "body_digest": delivery.body_digest,
        "preview": delivery.sanitized_preview,
        "data_class": trigger.default_data_class,
        "risk_tier": trigger.default_risk_tier,
        "owning_org_unit_id": trigger.owning_org_unit_id,
        "resource_scope": trigger.resource_scope,
        "trust": "untrusted_external_webhook",
    })
}

fn automation_webhook_rejection_delivery(
    trigger: &AutomationWebhookTriggerRecord,
    provider_event_id: Option<String>,
    body_digest: String,
    status: AutomationWebhookDeliveryStatus,
    reason_code: impl Into<String>,
    received_at_ms: u64,
    sanitized_preview: Value,
    verification: Option<AutomationWebhookVerificationDecision>,
) -> AutomationWebhookDeliveryRecord {
    let verification_scheme = verification
        .as_ref()
        .map(|decision| decision.scheme.clone());
    let verification_provider = verification
        .as_ref()
        .map(|decision| decision.provider.clone());
    let verification_reason_code = verification.map(|decision| decision.reason_code);
    AutomationWebhookDeliveryRecord {
        delivery_id: new_automation_webhook_delivery_id(),
        trigger_id: trigger.trigger_id.clone(),
        automation_id: trigger.automation_id.clone(),
        tenant_context: trigger.tenant_context.clone(),
        provider_event_id,
        body_digest,
        status,
        rejection_reason_code: Some(reason_code.into()),
        verification_scheme,
        verification_provider,
        verification_reason_code,
        queued_run_id: None,
        woken_run_id: None,
        woken_wait_id: None,
        received_at_ms,
        accepted_at_ms: None,
        rejected_at_ms: Some(received_at_ms),
        sanitized_preview,
        audit_event_id: None,
    }
}

#[derive(Debug, Clone)]
struct AutomationWebhookStatefulWakeResult {
    delivery: AutomationWebhookDeliveryRecord,
    wait: StatefulWaitRecord,
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

fn next_stateful_run_event_seq(
    path: &std::path::Path,
    tenant_context: &TenantContext,
    run_id: &str,
) -> u64 {
    query_stateful_run_events(
        path,
        tenant_context,
        StatefulRunEventQuery {
            run_id,
            after_seq: None,
            limit: None,
        },
    )
    .last()
    .map(|event| event.seq.saturating_add(1))
    .unwrap_or(1)
}

fn stateful_run_event_seq_by_id(
    path: &std::path::Path,
    tenant_context: &TenantContext,
    run_id: &str,
    event_id: &str,
) -> Option<u64> {
    query_stateful_run_events(
        path,
        tenant_context,
        StatefulRunEventQuery {
            run_id,
            after_seq: None,
            limit: None,
        },
    )
    .into_iter()
    .find(|event| event.event_id == event_id)
    .map(|event| event.seq)
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
        let deliveries = if self.automation_webhook_deliveries_path.exists() {
            let raw = fs::read_to_string(&self.automation_webhook_deliveries_path).await?;
            parse_automation_webhook_deliveries_file(&raw)?
        } else {
            HashMap::new()
        };
        *self.automation_webhook_deliveries.write().await = deliveries;
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

    async fn persist_automation_webhook_deliveries_locked(&self) -> anyhow::Result<()> {
        let deliveries = self.automation_webhook_deliveries.read().await.clone();
        let payload = serialize_automation_webhook_deliveries_file(deliveries)?;
        ensure_parent_dir(&self.automation_webhook_deliveries_path).await?;
        super::write_state_file_atomically(&self.automation_webhook_deliveries_path, payload).await
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
            signature_scheme: input.signature_scheme.unwrap_or_default(),
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
            if let Some(name) = input.name {
                let name = name.trim();
                if name.is_empty() {
                    anyhow::bail!("webhook trigger name is required");
                }
                trigger.name = name.to_string();
            }
            if let Some(provider) = input.provider {
                let provider = normalize_automation_webhook_provider(&provider)
                    .ok_or_else(|| anyhow::anyhow!("webhook provider is required"))?;
                trigger.provider = provider.clone();
                if trigger.name.trim().is_empty() {
                    trigger.name = provider;
                }
            }
            if let Some(provider_event_kind) = input.provider_event_kind {
                trigger.provider_event_kind = provider_event_kind
                    .as_deref()
                    .and_then(normalize_automation_webhook_provider_event_kind);
            }
            if let Some(signature_scheme) = input.signature_scheme {
                trigger.signature_scheme = signature_scheme;
            }
            if let Some(default_data_class) = input.default_data_class {
                trigger.default_data_class = default_data_class;
            }
            if let Some(default_risk_tier) = input.default_risk_tier {
                trigger.default_risk_tier = default_risk_tier;
            }
            if let Some(enabled) = input.enabled {
                trigger.enabled = enabled;
            }
            trigger.updated_at_ms = now_ms();
            trigger.updated_by = actor_id;
            trigger.clone()
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
    ) -> anyhow::Result<Option<AutomationWebhookStatefulWakeResult>> {
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
        let mut seq = next_stateful_run_event_seq(
            &paths.run_events_path,
            &trigger.tenant_context,
            &claimed_wait.run_id,
        );
        let scope = claimed_wait.scope.clone();
        let event = StatefulRunEventRecord {
            schema_version: 1,
            event_id: event_id.clone(),
            run_id: claimed_wait.run_id.clone(),
            seq,
            event_type: "stateful_runtime.wait.webhook_woken".to_string(),
            occurred_at_ms: received_at_ms,
            scope: scope.clone(),
            actor: trigger.owner_principal.clone(),
            phase_id: claimed_wait.phase_id.clone(),
            wait_kind: Some(StatefulWaitKind::Webhook),
            causation_id: Some(delivery_id.clone()),
            correlation_id: provider_event_id
                .clone()
                .or_else(|| Some(body_digest.clone())),
            payload: json!({
                "delivery_id": delivery_id,
                "trigger_id": trigger.trigger_id,
                "automation_id": trigger.automation_id,
                "provider": trigger.provider,
                "provider_event_kind": trigger.provider_event_kind,
                "provider_event_id": provider_event_id,
                "body_digest": body_digest,
                "wait_id": claimed_wait.wait_id,
                "wake_idempotency_key": wake_key,
            }),
        };
        if !append_stateful_run_event_once(&paths.run_events_path, &event).await? {
            if let Some(existing_seq) = stateful_run_event_seq_by_id(
                &paths.run_events_path,
                &trigger.tenant_context,
                &claimed_wait.run_id,
                &event_id,
            ) {
                seq = existing_seq;
            }
        }
        let woken_wait = mark_stateful_wait_woken(
            &paths.waits_path,
            &trigger.tenant_context,
            &claimed_wait.run_id,
            &claimed_wait.wait_id,
            &wake_key,
            seq,
            received_at_ms,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("stateful webhook wait wake conflict"))?;
        let snapshot = StatefulRunSnapshotRecord {
            schema_version: 1,
            snapshot_id: format!("stateful-webhook-wake-{delivery_id}"),
            run_id: woken_wait.run_id.clone(),
            seq,
            created_at_ms: received_at_ms,
            scope,
            status: StatefulWorkflowRunStatus::Running,
            phase_id: woken_wait.phase_id.clone(),
            source_record_kind: Some(StatefulWorkflowRunKind::AutomationV2),
            checkpoint: None,
            payload_digest: Some(body_digest.clone()),
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            metadata: Some(json!({
                "source": "automation_webhook",
                "delivery_id": delivery_id,
                "trigger_id": trigger.trigger_id,
                "provider": trigger.provider,
                "provider_event_id": provider_event_id,
                "body_digest": body_digest,
                "wait_id": woken_wait.wait_id,
            })),
        };
        write_stateful_run_snapshot(&paths.snapshots_root, &snapshot).await?;

        let delivery = AutomationWebhookDeliveryRecord {
            delivery_id: delivery_id.clone(),
            trigger_id: trigger.trigger_id.clone(),
            automation_id: trigger.automation_id.clone(),
            tenant_context: trigger.tenant_context.clone(),
            provider_event_id,
            body_digest,
            status: AutomationWebhookDeliveryStatus::Accepted,
            rejection_reason_code: None,
            verification_scheme: Some(verification.scheme.clone()),
            verification_provider: Some(verification.provider.clone()),
            verification_reason_code: Some(verification.reason_code.clone()),
            queued_run_id: None,
            woken_run_id: Some(woken_wait.run_id.clone()),
            woken_wait_id: Some(woken_wait.wait_id.clone()),
            received_at_ms,
            accepted_at_ms: Some(received_at_ms),
            rejected_at_ms: None,
            sanitized_preview,
            audit_event_id: None,
        };
        let delivery = self
            .record_automation_webhook_delivery_locked(delivery)
            .await?;
        self.event_bus.publish(crate::EngineEvent::new(
            "stateful_runtime.wait.webhook_woken",
            json!({
                "runID": woken_wait.run_id,
                "waitID": woken_wait.wait_id,
                "deliveryID": delivery.delivery_id,
                "triggerID": trigger.trigger_id,
                "provider": trigger.provider,
                "tenantContext": trigger.tenant_context,
            }),
        ));
        Ok(Some(AutomationWebhookStatefulWakeResult {
            delivery,
            wait: woken_wait,
        }))
    }

    pub(crate) async fn queue_automation_v2_run_from_webhook_delivery(
        &self,
        verified: VerifiedAutomationWebhookRequest,
        sanitized_preview: Value,
    ) -> anyhow::Result<AutomationWebhookQueueResult> {
        let trigger = verified.trigger;
        let verification = verified.verification.clone();
        let sanitized_preview = sanitize_automation_webhook_preview(&sanitized_preview);
        let automation = match self.get_automation_v2(&trigger.automation_id).await {
            Some(automation) => automation,
            None => {
                let delivery = self
                    .record_automation_webhook_rejection(
                        &trigger,
                        verified.provider_event_id,
                        verified.body_digest,
                        AutomationWebhookDeliveryStatus::Failed,
                        "automation_missing",
                        verified.received_at_ms,
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
                    verified.provider_event_id,
                    verified.body_digest,
                    AutomationWebhookDeliveryStatus::Rejected,
                    "automation_tenant_mismatch",
                    verified.received_at_ms,
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
                    verified.provider_event_id,
                    verified.body_digest,
                    AutomationWebhookDeliveryStatus::Rejected,
                    "automation_inactive",
                    verified.received_at_ms,
                    sanitized_preview,
                    Some(verification.clone()),
                )
                .await?;
            return Ok(AutomationWebhookQueueResult::Rejected {
                delivery,
                reason_code: "automation_inactive".to_string(),
            });
        }

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
                    verified.provider_event_id,
                    verified.body_digest,
                    AutomationWebhookDeliveryStatus::Disabled,
                    "trigger_disabled",
                    verified.received_at_ms,
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
            let duplicate = self
                .automation_webhook_deliveries
                .read()
                .await
                .values()
                .any(|delivery| {
                    automation_webhook_delivery_matches_key(
                        delivery,
                        &trigger,
                        verified.provider_event_id.as_ref(),
                        &verified.body_digest,
                    )
                });
            if duplicate {
                let delivery = automation_webhook_rejection_delivery(
                    &trigger,
                    verified.provider_event_id,
                    verified.body_digest,
                    AutomationWebhookDeliveryStatus::Duplicate,
                    "duplicate_delivery",
                    verified.received_at_ms,
                    sanitized_preview,
                    Some(verification.clone()),
                );
                let delivery = self
                    .record_automation_webhook_delivery_locked(delivery)
                    .await?;
                return Ok(AutomationWebhookQueueResult::Duplicate { delivery });
            }
            if let Some(woken) = self
                .wake_matching_stateful_webhook_wait_locked(
                    &trigger,
                    verified.provider_event_id.clone(),
                    verified.body_digest.clone(),
                    verified.received_at_ms,
                    sanitized_preview.clone(),
                    verification.clone(),
                )
                .await?
            {
                return Ok(AutomationWebhookQueueResult::Woken {
                    delivery: woken.delivery,
                    wait: woken.wait,
                });
            }

            let delivery = AutomationWebhookDeliveryRecord {
                delivery_id: new_automation_webhook_delivery_id(),
                trigger_id: trigger.trigger_id.clone(),
                automation_id: trigger.automation_id.clone(),
                tenant_context: trigger.tenant_context.clone(),
                provider_event_id: verified.provider_event_id,
                body_digest: verified.body_digest,
                status: AutomationWebhookDeliveryStatus::Accepted,
                rejection_reason_code: None,
                verification_scheme: Some(verification.scheme.clone()),
                verification_provider: Some(verification.provider.clone()),
                verification_reason_code: Some(verification.reason_code.clone()),
                queued_run_id: None,
                woken_run_id: None,
                woken_wait_id: None,
                received_at_ms: verified.received_at_ms,
                accepted_at_ms: Some(verified.received_at_ms),
                rejected_at_ms: None,
                sanitized_preview,
                audit_event_id: None,
            };
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

    pub(crate) async fn verify_automation_webhook_request(
        &self,
        public_path_token: &str,
        signature_header: Option<&str>,
        body: &[u8],
        provider_event_id: Option<String>,
        request_now_ms: u64,
        signature_tolerance_ms: u64,
    ) -> Result<VerifiedAutomationWebhookRequest, AutomationWebhookVerificationError> {
        self.verify_automation_webhook_request_with_headers(
            public_path_token,
            AutomationWebhookSignatureHeaders::tandem(signature_header),
            body,
            provider_event_id,
            request_now_ms,
            signature_tolerance_ms,
        )
        .await
    }

    pub(crate) async fn verify_automation_webhook_request_with_headers(
        &self,
        public_path_token: &str,
        signature_headers: AutomationWebhookSignatureHeaders,
        body: &[u8],
        provider_event_id: Option<String>,
        request_now_ms: u64,
        signature_tolerance_ms: u64,
    ) -> Result<VerifiedAutomationWebhookRequest, AutomationWebhookVerificationError> {
        let trigger = self
            .automation_webhook_triggers
            .read()
            .await
            .values()
            .find(|trigger| trigger.public_path_token == public_path_token)
            .cloned()
            .ok_or(AutomationWebhookVerificationError::UnknownTrigger)?;
        if !trigger.enabled {
            return Err(AutomationWebhookVerificationError::DisabledTrigger);
        }
        let material = self
            .automation_webhook_secret_material
            .read()
            .await
            .get(&secret_material_key(&trigger.secret.secret_ref))
            .cloned()
            .ok_or(AutomationWebhookVerificationError::MissingSecretMaterial)?;
        if !tenant_context_matches(&material.tenant_context, &trigger.tenant_context)
            || material.trigger_id != trigger.trigger_id
        {
            return Err(AutomationWebhookVerificationError::MissingSecretMaterial);
        }

        let verification =
            verify_automation_webhook_signature(AutomationWebhookSignatureVerificationContext {
                provider: &trigger.provider,
                scheme: &trigger.signature_scheme,
                headers: &signature_headers,
                secret: Some(&material.secret),
                body,
                request_now_ms,
                signature_tolerance_ms,
            })?;

        let body_digest = automation_webhook_body_digest(body);
        let replay = self
            .automation_webhook_deliveries
            .read()
            .await
            .values()
            .any(|delivery| {
                if delivery.trigger_id != trigger.trigger_id
                    || !delivery.tenant_matches(&trigger.tenant_context)
                    || !matches!(
                        delivery.status,
                        AutomationWebhookDeliveryStatus::Accepted
                            | AutomationWebhookDeliveryStatus::Duplicate
                    )
                {
                    return false;
                }
                delivery.body_digest == body_digest
                    || provider_event_id.as_ref().is_some_and(|event_id| {
                        delivery.provider_event_id.as_ref() == Some(event_id)
                    })
            });
        if replay {
            return Err(AutomationWebhookVerificationError::ReplayDetected);
        }

        Ok(VerifiedAutomationWebhookRequest {
            trigger,
            provider_event_id,
            body_digest,
            received_at_ms: request_now_ms,
            verification,
        })
    }
}
