use crate::types::{MemoryError, MemoryResult, MemoryTenantScope};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tandem_enterprise_contract::DataClass;

pub const MEMORY_ENVELOPE_METADATA_KEY: &str = "memory_envelope";
const HOSTED_ENCRYPTION_REQUIRED_ENV: &str = "TANDEM_MEMORY_ENCRYPTION_REQUIRED";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryKeyScope {
    pub org_id: String,
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment_id: Option<String>,
    pub data_class: DataClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_binding_id: Option<String>,
}

impl MemoryKeyScope {
    pub fn new(
        tenant_scope: &MemoryTenantScope,
        data_class: DataClass,
        source_binding_id: Option<String>,
    ) -> Self {
        Self {
            org_id: tenant_scope.org_id.clone(),
            workspace_id: tenant_scope.workspace_id.clone(),
            deployment_id: tenant_scope.deployment_id.clone(),
            data_class,
            source_binding_id,
        }
    }

    pub fn canonical_id(&self) -> String {
        let deployment = self.deployment_id.as_deref().unwrap_or("default");
        let class = serde_json::to_value(self.data_class)
            .ok()
            .and_then(|value| value.as_str().map(ToOwned::to_owned))
            .unwrap_or_else(|| "unknown".to_string());
        match self.source_binding_id.as_deref() {
            Some(source_binding_id) if !source_binding_id.trim().is_empty() => format!(
                "tandem/memory/{}/{}/{}/{}/source/{}",
                self.org_id, self.workspace_id, deployment, class, source_binding_id
            ),
            _ => format!(
                "tandem/memory/{}/{}/{}/{}",
                self.org_id, self.workspace_id, deployment, class
            ),
        }
    }

    fn validates_against_tenant(&self, tenant_scope: &MemoryTenantScope) -> bool {
        self.org_id == tenant_scope.org_id
            && self.workspace_id == tenant_scope.workspace_id
            && self.deployment_id.as_deref().unwrap_or("")
                == tenant_scope.deployment_id.as_deref().unwrap_or("")
    }

    fn validate_partitioned(&self) -> MemoryResult<()> {
        for (field, value) in [
            ("org_id", self.org_id.as_str()),
            ("workspace_id", self.workspace_id.as_str()),
        ] {
            if is_wildcard_scope(value) {
                return Err(MemoryError::InvalidConfig(format!(
                    "memory envelope key scope must not use wildcard `{field}`"
                )));
            }
        }
        if self
            .deployment_id
            .as_deref()
            .map(is_wildcard_scope)
            .unwrap_or(false)
        {
            return Err(MemoryError::InvalidConfig(
                "memory envelope key scope must not use wildcard `deployment_id`".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryEnvelopeMetadata {
    pub key_scope: MemoryKeyScope,
    pub kek_id: String,
    pub kek_version: String,
    pub wrapped_dek: String,
    pub algorithm: String,
    pub encryption_context_hash: String,
    pub rotation_epoch: u64,
    pub policy_decision_id: String,
    pub audit_id: String,
}

impl MemoryEnvelopeMetadata {
    pub fn from_metadata(metadata: Option<&Value>) -> MemoryResult<Option<Self>> {
        let Some(value) = metadata.and_then(|value| value.get(MEMORY_ENVELOPE_METADATA_KEY)) else {
            return Ok(None);
        };
        serde_json::from_value(value.clone())
            .map(Some)
            .map_err(MemoryError::from)
    }

    pub fn attach_to_metadata(&self, metadata: Option<Value>) -> MemoryResult<Value> {
        let mut object = match metadata {
            Some(Value::Object(object)) => object,
            Some(_) => {
                return Err(MemoryError::InvalidConfig(
                    "memory envelope metadata requires object metadata".to_string(),
                ));
            }
            None => Map::new(),
        };
        object.insert(
            MEMORY_ENVELOPE_METADATA_KEY.to_string(),
            serde_json::to_value(self)?,
        );
        Ok(Value::Object(object))
    }

    fn validate_required_fields(&self) -> MemoryResult<()> {
        let required = [
            ("kek_id", self.kek_id.as_str()),
            ("kek_version", self.kek_version.as_str()),
            ("wrapped_dek", self.wrapped_dek.as_str()),
            ("algorithm", self.algorithm.as_str()),
            (
                "encryption_context_hash",
                self.encryption_context_hash.as_str(),
            ),
            ("policy_decision_id", self.policy_decision_id.as_str()),
            ("audit_id", self.audit_id.as_str()),
        ];
        for (field, value) in required {
            if value.trim().is_empty() {
                return Err(MemoryError::InvalidConfig(format!(
                    "hosted memory encryption metadata missing `{field}`"
                )));
            }
        }
        Ok(())
    }
}

pub fn hosted_memory_encryption_required() -> bool {
    std::env::var(HOSTED_ENCRYPTION_REQUIRED_ENV)
        .map(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

pub fn validate_memory_envelope_for_write(
    tenant_scope: &MemoryTenantScope,
    metadata: Option<&Value>,
) -> MemoryResult<()> {
    validate_memory_envelope_for_required_write(
        tenant_scope,
        metadata,
        hosted_memory_encryption_required(),
    )
}

pub fn validate_memory_envelope_for_required_write(
    tenant_scope: &MemoryTenantScope,
    metadata: Option<&Value>,
    encryption_required: bool,
) -> MemoryResult<()> {
    let envelope = MemoryEnvelopeMetadata::from_metadata(metadata)?;
    let Some(envelope) = envelope else {
        if encryption_required {
            return Err(MemoryError::InvalidConfig(
                "hosted memory encryption requires memory_envelope metadata".to_string(),
            ));
        }
        return Ok(());
    };

    envelope.validate_required_fields()?;
    envelope.key_scope.validate_partitioned()?;
    if !envelope.key_scope.validates_against_tenant(tenant_scope) {
        return Err(MemoryError::InvalidConfig(
            "memory envelope key scope does not match tenant scope".to_string(),
        ));
    }
    validate_enterprise_source_binding(metadata, &envelope)
}

fn is_wildcard_scope(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "*" | "all" | "global" | "default"
    )
}

fn validate_enterprise_source_binding(
    metadata: Option<&Value>,
    envelope: &MemoryEnvelopeMetadata,
) -> MemoryResult<()> {
    let Some(binding) = metadata.and_then(|value| value.get("enterprise_source_binding")) else {
        return Ok(());
    };
    if let Some(binding_data_class) = binding.get("data_class").and_then(Value::as_str) {
        let expected = serde_json::to_value(envelope.key_scope.data_class)?
            .as_str()
            .unwrap_or_default()
            .to_string();
        if binding_data_class != expected {
            return Err(MemoryError::InvalidConfig(
                "memory envelope data class does not match enterprise source binding".to_string(),
            ));
        }
    }
    if let Some(binding_id) = binding.get("binding_id").and_then(Value::as_str) {
        if envelope.key_scope.source_binding_id.as_deref() != Some(binding_id) {
            return Err(MemoryError::InvalidConfig(
                "memory envelope source binding does not match enterprise source binding"
                    .to_string(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tenant_scope() -> MemoryTenantScope {
        MemoryTenantScope {
            org_id: "acme".to_string(),
            workspace_id: "finance".to_string(),
            deployment_id: Some("prod".to_string()),
        }
    }

    fn envelope(data_class: DataClass) -> MemoryEnvelopeMetadata {
        MemoryEnvelopeMetadata {
            key_scope: MemoryKeyScope::new(
                &tenant_scope(),
                data_class,
                Some("drive-1".to_string()),
            ),
            kek_id: "projects/acme/locations/global/keyRings/memory/cryptoKeys/finance".to_string(),
            kek_version: "1".to_string(),
            wrapped_dek: "wrapped".to_string(),
            algorithm: "AES-256-GCM".to_string(),
            encryption_context_hash: "ctx-hash".to_string(),
            rotation_epoch: 0,
            policy_decision_id: "decision-1".to_string(),
            audit_id: "audit-1".to_string(),
        }
    }

    #[test]
    fn key_scope_canonical_id_includes_tenant_class_and_source() {
        let scope = MemoryKeyScope::new(
            &tenant_scope(),
            DataClass::FinancialRecord,
            Some("drive-1".to_string()),
        );
        assert_eq!(
            scope.canonical_id(),
            "tandem/memory/acme/finance/prod/financial_record/source/drive-1"
        );
    }

    #[test]
    fn envelope_round_trips_through_metadata() {
        let envelope = envelope(DataClass::FinancialRecord);
        let metadata = envelope
            .attach_to_metadata(Some(serde_json::json!({"kind": "test"})))
            .expect("attach metadata");
        assert_eq!(
            MemoryEnvelopeMetadata::from_metadata(Some(&metadata))
                .expect("parse metadata")
                .as_ref(),
            Some(&envelope)
        );
    }

    #[test]
    fn validation_rejects_tenant_mismatch() {
        let mut envelope = envelope(DataClass::FinancialRecord);
        envelope.key_scope.workspace_id = "hr".to_string();
        let metadata = envelope.attach_to_metadata(None).expect("metadata");

        let err = validate_memory_envelope_for_write(&tenant_scope(), Some(&metadata))
            .expect_err("tenant mismatch should fail");
        assert!(err
            .to_string()
            .contains("key scope does not match tenant scope"));
    }

    #[test]
    fn validation_rejects_wildcard_key_scope() {
        let mut envelope = envelope(DataClass::FinancialRecord);
        envelope.key_scope.org_id = "*".to_string();
        let metadata = envelope.attach_to_metadata(None).expect("metadata");

        let err = validate_memory_envelope_for_write(&tenant_scope(), Some(&metadata))
            .expect_err("wildcard key scope should fail");
        assert!(err.to_string().contains("wildcard `org_id`"));
    }

    #[test]
    fn validation_rejects_source_binding_mismatch() {
        let metadata = envelope(DataClass::FinancialRecord)
            .attach_to_metadata(Some(serde_json::json!({
                "enterprise_source_binding": {
                    "binding_id": "other-drive",
                    "data_class": "financial_record"
                }
            })))
            .expect("metadata");

        let err = validate_memory_envelope_for_write(&tenant_scope(), Some(&metadata))
            .expect_err("source binding mismatch should fail");
        assert!(err.to_string().contains("source binding does not match"));
    }

    #[test]
    fn hosted_required_mode_rejects_missing_envelope() {
        let err = validate_memory_envelope_for_required_write(&tenant_scope(), None, true)
            .expect_err("hosted required mode should fail without metadata");

        assert!(err
            .to_string()
            .contains("requires memory_envelope metadata"));
    }

    #[test]
    fn local_mode_allows_missing_envelope() {
        validate_memory_envelope_for_required_write(&tenant_scope(), None, false)
            .expect("local mode should allow missing envelope metadata");
    }
}
