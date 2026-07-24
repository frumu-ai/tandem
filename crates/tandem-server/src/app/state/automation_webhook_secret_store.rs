// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Ciphertext-at-rest format for automation webhook signing material.

use std::collections::HashMap;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_enterprise_contract::DataClass;
use tandem_memory::envelope::MemoryKeyScope;
use tandem_memory::types::MemoryTenantScope;
use tandem_types::{SecretRef, TenantContext};

use super::automation_webhook_store::{secret_material_key, AutomationWebhookSecretMaterialRecord};

const SECRET_MATERIAL_SCHEMA_VERSION: u32 = 2;
const SECRET_PURPOSE: &str = "automation_webhook_signing";

#[derive(Clone, Serialize, Deserialize)]
struct LegacySecretMaterialFile {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    secrets: HashMap<String, AutomationWebhookSecretMaterialRecord>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ProtectedSecretMaterialFile {
    schema_version: u32,
    #[serde(default)]
    secrets: HashMap<String, ProtectedSecretMaterialEntry>,
}

#[derive(Clone, Serialize, Deserialize)]
struct ProtectedSecretMaterialEntry {
    secret_ref: SecretRef,
    tenant_context: TenantContext,
    trigger_id: String,
    secret_version: u64,
    protected_secret: String,
    created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rotated_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rotated_by: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct BoundWebhookSecret {
    tenant_context: TenantContext,
    trigger_id: String,
    purpose: String,
    secret_version: u64,
    secret: String,
}

pub(super) struct ParsedSecretMaterial {
    pub(super) secrets: HashMap<String, AutomationWebhookSecretMaterialRecord>,
    pub(super) migrated_from_plaintext: bool,
}

fn same_tenant(left: &TenantContext, right: &TenantContext) -> bool {
    left.org_id == right.org_id
        && left.workspace_id == right.workspace_id
        && left.deployment_id == right.deployment_id
}

fn secret_context(
    tenant: &TenantContext,
    trigger_id: &str,
    secret_version: u64,
) -> crate::encrypted_file_store::ProtectedRecordContext {
    let tenant_scope = MemoryTenantScope {
        org_id: tenant.org_id.clone(),
        workspace_id: tenant.workspace_id.clone(),
        deployment_id: tenant.deployment_id.clone(),
    };
    let key_scope = MemoryKeyScope::new(
        &tenant_scope,
        DataClass::Restricted,
        Some("automation-webhook-secret-material".to_string()),
    );
    crate::encrypted_file_store::ProtectedRecordContext::new(
        key_scope,
        format!("automation-webhook-secret:{SECRET_PURPOSE}:v1"),
        format!("automation-webhook-secret:{trigger_id}:v{secret_version}"),
    )
}

fn protect_secret(record: &AutomationWebhookSecretMaterialRecord) -> anyhow::Result<String> {
    let bound = BoundWebhookSecret {
        tenant_context: record.tenant_context.clone(),
        trigger_id: record.trigger_id.clone(),
        purpose: SECRET_PURPOSE.to_string(),
        secret_version: record.secret_version,
        secret: record.secret.clone(),
    };
    let plaintext = serde_json::to_string(&bound)?;
    crate::encrypted_file_store::encrypt_text_required(
        &plaintext,
        &secret_context(
            &record.tenant_context,
            &record.trigger_id,
            record.secret_version,
        ),
    )
    .context("protect automation webhook secret material")
}

fn unprotect_secret(entry: &ProtectedSecretMaterialEntry) -> anyhow::Result<String> {
    anyhow::ensure!(
        crate::encrypted_file_store::is_encrypted_payload(&entry.protected_secret),
        "plaintext automation webhook secret material is not accepted in schema v2"
    );
    let plaintext = crate::encrypted_file_store::decrypt_text_required(
        &entry.protected_secret,
        &secret_context(
            &entry.tenant_context,
            &entry.trigger_id,
            entry.secret_version,
        ),
    )
    .context("unprotect automation webhook secret material")?;
    let bound = serde_json::from_str::<BoundWebhookSecret>(&plaintext)
        .context("parse bound automation webhook secret material")?;
    anyhow::ensure!(
        same_tenant(&bound.tenant_context, &entry.tenant_context),
        "automation webhook secret tenant binding does not match"
    );
    anyhow::ensure!(
        bound.trigger_id == entry.trigger_id
            && bound.purpose == SECRET_PURPOSE
            && bound.secret_version == entry.secret_version,
        "automation webhook secret trigger/purpose/version binding does not match"
    );
    Ok(bound.secret)
}

fn legacy_secrets(
    value: Value,
) -> anyhow::Result<HashMap<String, AutomationWebhookSecretMaterialRecord>> {
    if value.get("schema_version").is_none() {
        return serde_json::from_value(value)
            .context("failed to parse legacy automation webhook secret material map");
    }
    let file = serde_json::from_value::<LegacySecretMaterialFile>(value)
        .context("failed to parse plaintext automation webhook secret material file")?;
    anyhow::ensure!(
        file.schema_version <= 1,
        "unsupported plaintext automation webhook secret material schema {}",
        file.schema_version
    );
    Ok(file.secrets)
}

pub(super) fn parse_secret_material_file(raw: &str) -> anyhow::Result<ParsedSecretMaterial> {
    if raw.trim().is_empty() || raw.trim() == "{}" {
        return Ok(ParsedSecretMaterial {
            secrets: HashMap::new(),
            migrated_from_plaintext: false,
        });
    }
    let value: Value = serde_json::from_str(raw)
        .context("failed to parse automation webhook secret material state file")?;
    let schema_version = value
        .get("schema_version")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if schema_version < SECRET_MATERIAL_SCHEMA_VERSION as u64 {
        return Ok(ParsedSecretMaterial {
            secrets: legacy_secrets(value)?,
            migrated_from_plaintext: true,
        });
    }
    anyhow::ensure!(
        schema_version == SECRET_MATERIAL_SCHEMA_VERSION as u64,
        "automation webhook secret material schema {schema_version} is newer than supported {SECRET_MATERIAL_SCHEMA_VERSION}"
    );
    let file = serde_json::from_value::<ProtectedSecretMaterialFile>(value)
        .context("failed to parse protected automation webhook secret material file")?;
    let mut secrets = HashMap::with_capacity(file.secrets.len());
    for (key, entry) in file.secrets {
        entry
            .secret_ref
            .validate_for_tenant(&entry.tenant_context)
            .map_err(|error| anyhow::anyhow!("webhook secret ref tenant mismatch: {error:?}"))?;
        anyhow::ensure!(
            key == secret_material_key(&entry.secret_ref),
            "automation webhook protected secret key does not match metadata"
        );
        let secret = unprotect_secret(&entry)?;
        let record = AutomationWebhookSecretMaterialRecord {
            secret_ref: entry.secret_ref,
            tenant_context: entry.tenant_context,
            trigger_id: entry.trigger_id,
            secret_version: entry.secret_version,
            secret,
            created_at_ms: entry.created_at_ms,
            rotated_at_ms: entry.rotated_at_ms,
            rotated_by: entry.rotated_by,
        };
        anyhow::ensure!(
            secrets.insert(key, record).is_none(),
            "duplicate webhook secret key"
        );
    }
    Ok(ParsedSecretMaterial {
        secrets,
        migrated_from_plaintext: false,
    })
}

pub(super) fn serialize_secret_material_file(
    secrets: HashMap<String, AutomationWebhookSecretMaterialRecord>,
) -> anyhow::Result<String> {
    let mut protected = HashMap::with_capacity(secrets.len());
    for (key, record) in secrets {
        anyhow::ensure!(
            key == secret_material_key(&record.secret_ref),
            "automation webhook secret key does not match metadata"
        );
        let entry = ProtectedSecretMaterialEntry {
            protected_secret: protect_secret(&record)?,
            secret_ref: record.secret_ref,
            tenant_context: record.tenant_context,
            trigger_id: record.trigger_id,
            secret_version: record.secret_version,
            created_at_ms: record.created_at_ms,
            rotated_at_ms: record.rotated_at_ms,
            rotated_by: record.rotated_by,
        };
        protected.insert(key, entry);
    }
    serde_json::to_string_pretty(&ProtectedSecretMaterialFile {
        schema_version: SECRET_MATERIAL_SCHEMA_VERSION,
        secrets: protected,
    })
    .context("failed to serialize protected automation webhook secret material file")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_memory::MemoryCryptoProvider;

    fn fixture(secret: &str) -> (String, AutomationWebhookSecretMaterialRecord) {
        let tenant =
            TenantContext::explicit_user_workspace("org-secret", "workspace-secret", None, "test");
        let trigger_id = "trigger-secret";
        let secret_ref =
            super::super::automation_webhook_store::secret_ref_for_trigger(&tenant, trigger_id, 1);
        let key = secret_material_key(&secret_ref);
        (
            key,
            AutomationWebhookSecretMaterialRecord {
                secret_ref,
                tenant_context: tenant,
                trigger_id: trigger_id.to_string(),
                secret_version: 1,
                secret: secret.to_string(),
                created_at_ms: 1,
                rotated_at_ms: None,
                rotated_by: None,
            },
        )
    }

    #[tokio::test]
    async fn schema_v2_is_ciphertext_only_and_authority_bound() {
        crate::encrypted_file_store::with_test_crypto_provider(
            MemoryCryptoProvider::local_key([0x45; 32]),
            None,
            async {
                let marker = "webhook-secret-plaintext-marker";
                let (key, record) = fixture(marker);
                let serialized =
                    serialize_secret_material_file(HashMap::from([(key.clone(), record.clone())]))
                        .expect("serialize protected secrets");
                assert!(serialized.contains("\"schema_version\": 2"));
                assert!(serialized.contains(crate::encrypted_file_store::SCOPED_RECORD_PREFIX));
                assert!(!serialized.contains(marker));

                let parsed = parse_secret_material_file(&serialized).expect("parse protected");
                assert!(!parsed.migrated_from_plaintext);
                assert_eq!(parsed.secrets[&key].secret, marker);

                let mut tampered: Value =
                    serde_json::from_str(&serialized).expect("protected json");
                tampered["secrets"][&key]["trigger_id"] =
                    Value::String("other-trigger".to_string());
                let error = parse_secret_material_file(
                    &serde_json::to_string(&tampered).expect("tampered json"),
                )
                .err()
                .expect("AAD-bound trigger tampering must fail");
                assert!(error.to_string().contains("unprotect"));

                let plaintext_v2 = serde_json::json!({
                    "schema_version": 2,
                    "secrets": {
                        key: {
                            "secret_ref": record.secret_ref,
                            "tenant_context": record.tenant_context,
                            "trigger_id": record.trigger_id,
                            "secret_version": record.secret_version,
                            "protected_secret": marker,
                            "created_at_ms": record.created_at_ms,
                        }
                    }
                });
                assert!(parse_secret_material_file(&plaintext_v2.to_string()).is_err());
            },
        )
        .await;
    }

    #[test]
    fn legacy_plaintext_is_identified_for_immediate_migration() {
        let marker = "legacy-webhook-secret-marker";
        let (key, record) = fixture(marker);
        let raw = serde_json::json!({
            "schema_version": 1,
            "secrets": { key.clone(): record }
        })
        .to_string();
        let parsed = parse_secret_material_file(&raw).expect("parse legacy secrets");
        assert!(parsed.migrated_from_plaintext);
        assert_eq!(parsed.secrets[&key].secret, marker);
    }
}
