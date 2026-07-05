//! On-disk state-file formats for Automation V2 webhook records.
//!
//! Split from `automation_webhook_store.rs` purely for file-size hygiene: the
//! versioned-file envelopes and their parse/serialize helpers are self-contained
//! and change far less often than the store logic itself.

use std::collections::HashMap;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::automation_webhook_store::AutomationWebhookSecretMaterialRecord;
use crate::automation_v2::types::{
    AutomationWebhookDeliveryRecord, AutomationWebhookTriggerRecord,
};

const AUTOMATION_WEBHOOK_SCHEMA_VERSION: u32 = 1;

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

pub(super) fn parse_automation_webhook_triggers_file(
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

pub(super) fn parse_automation_webhook_deliveries_file(
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

pub(super) fn parse_automation_webhook_secret_material_file(
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

pub(super) fn serialize_automation_webhook_triggers_file(
    triggers: HashMap<String, AutomationWebhookTriggerRecord>,
) -> anyhow::Result<String> {
    serde_json::to_string_pretty(&AutomationWebhookTriggersFile {
        schema_version: AUTOMATION_WEBHOOK_SCHEMA_VERSION,
        triggers,
    })
    .context("failed to serialize automation webhook triggers state file")
}

pub(super) fn serialize_automation_webhook_deliveries_file(
    deliveries: HashMap<String, AutomationWebhookDeliveryRecord>,
) -> anyhow::Result<String> {
    serde_json::to_string_pretty(&AutomationWebhookDeliveriesFile {
        schema_version: AUTOMATION_WEBHOOK_SCHEMA_VERSION,
        deliveries,
    })
    .context("failed to serialize automation webhook deliveries state file")
}

pub(super) fn serialize_automation_webhook_secret_material_file(
    secrets: HashMap<String, AutomationWebhookSecretMaterialRecord>,
) -> anyhow::Result<String> {
    serde_json::to_string_pretty(&AutomationWebhookSecretMaterialFile {
        schema_version: AUTOMATION_WEBHOOK_SCHEMA_VERSION,
        secrets,
    })
    .context("failed to serialize automation webhook secret material state file")
}
