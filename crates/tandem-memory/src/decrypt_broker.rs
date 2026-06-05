use crate::envelope::{hosted_memory_encryption_required, MemoryEnvelopeMetadata};
use crate::key_lifecycle::{
    evaluate_memory_key_lifecycle, MemoryKeyLifecycleDecision, MemoryKeyLifecycleOutcome,
    MemoryKeyLifecyclePolicy,
};
use crate::types::{MemoryError, MemoryResult, MemoryTenantScope};
use serde::{Deserialize, Serialize};
use tandem_enterprise_contract::DataClass;

const MEMORY_DECRYPT_PROVIDER_ENV: &str = "TANDEM_MEMORY_DECRYPT_PROVIDER";
const MEMORY_DECRYPT_PRINCIPAL_ID_ENV: &str = "TANDEM_MEMORY_DECRYPT_PRINCIPAL_ID";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryDecryptPurpose {
    RetrievalGateway,
    IngestionWorker,
    RuntimeWorker,
    Migration,
    BreakGlass,
    KeyAdministration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemorySecretFamily {
    MemoryEnvelope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryDecryptPrincipal {
    pub principal_id: String,
    pub purpose: MemoryDecryptPurpose,
    pub tenant_scope: MemoryTenantScope,
    pub allowed_data_classes: Vec<DataClass>,
    #[serde(default)]
    pub allowed_source_binding_ids: Vec<String>,
}

impl MemoryDecryptPrincipal {
    pub fn retrieval_gateway(
        principal_id: impl Into<String>,
        tenant_scope: MemoryTenantScope,
        allowed_data_classes: Vec<DataClass>,
        allowed_source_binding_ids: Vec<String>,
    ) -> Self {
        Self {
            principal_id: principal_id.into(),
            purpose: MemoryDecryptPurpose::RetrievalGateway,
            tenant_scope,
            allowed_data_classes,
            allowed_source_binding_ids,
        }
    }

    fn validate(&self) -> MemoryResult<()> {
        if is_wildcard_or_blank(&self.principal_id) {
            return Err(MemoryError::InvalidConfig(
                "memory decrypt principal id must be explicit".to_string(),
            ));
        }
        validate_tenant_scope(&self.tenant_scope)?;
        if self.allowed_data_classes.is_empty() {
            return Err(MemoryError::InvalidConfig(
                "memory decrypt principal requires explicit data classes".to_string(),
            ));
        }
        if self.purpose == MemoryDecryptPurpose::KeyAdministration {
            return Err(MemoryError::InvalidConfig(
                "key administration principal cannot unwrap memory DEKs".to_string(),
            ));
        }
        if self
            .allowed_source_binding_ids
            .iter()
            .any(|value| is_wildcard_or_blank(value))
        {
            return Err(MemoryError::InvalidConfig(
                "memory decrypt source grants must not use wildcard values".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryDecryptBrokerConfig {
    pub provider: String,
    pub runtime_principal_id: String,
    pub secret_family: MemorySecretFamily,
    pub hosted_required: bool,
}

impl MemoryDecryptBrokerConfig {
    pub fn local_disabled() -> Self {
        Self {
            provider: "disabled".to_string(),
            runtime_principal_id: "local".to_string(),
            secret_family: MemorySecretFamily::MemoryEnvelope,
            hosted_required: false,
        }
    }

    pub fn hosted(
        provider: impl Into<String>,
        runtime_principal_id: impl Into<String>,
    ) -> MemoryResult<Self> {
        let config = Self {
            provider: provider.into(),
            runtime_principal_id: runtime_principal_id.into(),
            secret_family: MemorySecretFamily::MemoryEnvelope,
            hosted_required: true,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn from_env() -> MemoryResult<Self> {
        let hosted_required = hosted_memory_encryption_required();
        let provider = std::env::var(MEMORY_DECRYPT_PROVIDER_ENV).unwrap_or_default();
        let principal_id = std::env::var(MEMORY_DECRYPT_PRINCIPAL_ID_ENV).unwrap_or_default();
        if !hosted_required && provider.trim().is_empty() && principal_id.trim().is_empty() {
            return Ok(Self::local_disabled());
        }
        let config = Self {
            provider,
            runtime_principal_id: principal_id,
            secret_family: MemorySecretFamily::MemoryEnvelope,
            hosted_required,
        };
        Ok(config)
    }

    pub fn validate(&self) -> MemoryResult<()> {
        if self.secret_family != MemorySecretFamily::MemoryEnvelope {
            return Err(MemoryError::InvalidConfig(
                "memory decrypt broker must use the memory envelope secret family".to_string(),
            ));
        }
        if self.hosted_required {
            if is_wildcard_or_blank(&self.provider) || self.provider == "disabled" {
                return Err(MemoryError::InvalidConfig(
                    "hosted memory decrypt requires an explicit KMS provider".to_string(),
                ));
            }
            if is_wildcard_or_blank(&self.runtime_principal_id) {
                return Err(MemoryError::InvalidConfig(
                    "hosted memory decrypt requires a scoped runtime principal".to_string(),
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryDecryptRequest {
    pub envelope: MemoryEnvelopeMetadata,
    pub tenant_scope: MemoryTenantScope,
    pub principal: MemoryDecryptPrincipal,
    pub policy_decision_id: String,
    pub audit_id: String,
    #[serde(default)]
    pub break_glass_requested: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_lifecycle_policy: Option<MemoryKeyLifecyclePolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryDekUnwrapTicket {
    pub provider: String,
    pub runtime_principal_id: String,
    pub principal_id: String,
    pub purpose: MemoryDecryptPurpose,
    pub key_scope_id: String,
    pub kek_id: String,
    pub kek_version: String,
    pub wrapped_dek: String,
    pub algorithm: String,
    pub encryption_context_hash: String,
    pub policy_decision_id: String,
    pub audit_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_lifecycle_decision: Option<MemoryKeyLifecycleDecision>,
}

pub trait MemoryDekUnwrapProvider {
    fn provider_id(&self) -> &str;
    fn secret_family(&self) -> MemorySecretFamily;
    fn unwrap_dek(&self, ticket: &MemoryDekUnwrapTicket) -> MemoryResult<Vec<u8>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryDecryptAuditOutcome {
    Allowed,
    Denied,
    Noop,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryDecryptAuditEvent {
    pub outcome: MemoryDecryptAuditOutcome,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_principal_id: Option<String>,
    pub principal_id: String,
    pub purpose: MemoryDecryptPurpose,
    pub org_id: String,
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment_id: Option<String>,
    pub data_class: DataClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_binding_id: Option<String>,
    pub policy_decision_id: String,
    pub audit_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryDecryptAuthorization {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ticket: Option<MemoryDekUnwrapTicket>,
    pub audit_event: MemoryDecryptAuditEvent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryDecryptBroker {
    config: MemoryDecryptBrokerConfig,
}

impl MemoryDecryptBroker {
    pub fn new(config: MemoryDecryptBrokerConfig) -> MemoryResult<Self> {
        Ok(Self { config })
    }

    pub fn from_env() -> MemoryResult<Self> {
        Self::new(MemoryDecryptBrokerConfig::from_env()?)
    }

    pub fn authorize_unwrap(
        &self,
        request: MemoryDecryptRequest,
    ) -> MemoryResult<Option<MemoryDekUnwrapTicket>> {
        let authorization = self.authorize_unwrap_with_audit(request)?;
        match authorization.audit_event.outcome {
            MemoryDecryptAuditOutcome::Allowed | MemoryDecryptAuditOutcome::Noop => {
                Ok(authorization.ticket)
            }
            MemoryDecryptAuditOutcome::Denied => {
                Err(MemoryError::InvalidConfig(authorization.audit_event.reason))
            }
        }
    }

    pub fn authorize_unwrap_with_audit(
        &self,
        request: MemoryDecryptRequest,
    ) -> MemoryResult<MemoryDecryptAuthorization> {
        if !self.config.hosted_required && self.config.provider == "disabled" {
            return Ok(MemoryDecryptAuthorization {
                ticket: None,
                audit_event: self.audit_event(
                    &request,
                    MemoryDecryptAuditOutcome::Noop,
                    "local memory decrypt broker disabled",
                ),
            });
        }

        if let Err(error) = self.config.validate() {
            return Ok(MemoryDecryptAuthorization {
                ticket: None,
                audit_event: self.audit_event(
                    &request,
                    MemoryDecryptAuditOutcome::Denied,
                    &error.to_string(),
                ),
            });
        }
        if let Err(error) = request.principal.validate() {
            return Ok(MemoryDecryptAuthorization {
                ticket: None,
                audit_event: self.audit_event(
                    &request,
                    MemoryDecryptAuditOutcome::Denied,
                    &error.to_string(),
                ),
            });
        }
        if let Err(error) = validate_decrypt_request(&request) {
            return Ok(MemoryDecryptAuthorization {
                ticket: None,
                audit_event: self.audit_event(
                    &request,
                    MemoryDecryptAuditOutcome::Denied,
                    &error.to_string(),
                ),
            });
        }

        let lifecycle_decision = match request.key_lifecycle_policy.as_ref() {
            Some(policy) => {
                let decision = evaluate_memory_key_lifecycle(
                    &request.envelope,
                    &request.principal.principal_id,
                    request.break_glass_requested,
                    policy,
                );
                if decision.outcome == MemoryKeyLifecycleOutcome::Denied {
                    return Ok(MemoryDecryptAuthorization {
                        ticket: None,
                        audit_event: self.audit_event(
                            &request,
                            MemoryDecryptAuditOutcome::Denied,
                            &decision.reason,
                        ),
                    });
                }
                Some(decision)
            }
            None => None,
        };

        let audit_event = self.audit_event(
            &request,
            MemoryDecryptAuditOutcome::Allowed,
            "memory decrypt unwrap authorized",
        );
        Ok(MemoryDecryptAuthorization {
            ticket: Some(MemoryDekUnwrapTicket {
                provider: self.config.provider.clone(),
                runtime_principal_id: self.config.runtime_principal_id.clone(),
                principal_id: request.principal.principal_id,
                purpose: request.principal.purpose,
                key_scope_id: request.envelope.key_scope.canonical_id(),
                kek_id: request.envelope.kek_id,
                kek_version: request.envelope.kek_version,
                wrapped_dek: request.envelope.wrapped_dek,
                algorithm: request.envelope.algorithm,
                encryption_context_hash: request.envelope.encryption_context_hash,
                policy_decision_id: request.policy_decision_id,
                audit_id: request.audit_id,
                key_lifecycle_decision: lifecycle_decision,
            }),
            audit_event,
        })
    }

    fn audit_event(
        &self,
        request: &MemoryDecryptRequest,
        outcome: MemoryDecryptAuditOutcome,
        reason: &str,
    ) -> MemoryDecryptAuditEvent {
        MemoryDecryptAuditEvent {
            outcome,
            reason: reason.to_string(),
            provider: non_empty_string(&self.config.provider),
            runtime_principal_id: non_empty_string(&self.config.runtime_principal_id),
            principal_id: request.principal.principal_id.clone(),
            purpose: request.principal.purpose,
            org_id: request.tenant_scope.org_id.clone(),
            workspace_id: request.tenant_scope.workspace_id.clone(),
            deployment_id: request.tenant_scope.deployment_id.clone(),
            data_class: request.envelope.key_scope.data_class,
            source_binding_id: request.envelope.key_scope.source_binding_id.clone(),
            policy_decision_id: request.policy_decision_id.clone(),
            audit_id: request.audit_id.clone(),
        }
    }
}

fn validate_decrypt_request(request: &MemoryDecryptRequest) -> MemoryResult<()> {
    if is_wildcard_or_blank(&request.policy_decision_id) {
        return Err(MemoryError::InvalidConfig(
            "memory decrypt requires a policy decision id".to_string(),
        ));
    }
    if is_wildcard_or_blank(&request.audit_id) {
        return Err(MemoryError::InvalidConfig(
            "memory decrypt requires an audit id".to_string(),
        ));
    }
    if request.policy_decision_id != request.envelope.policy_decision_id {
        return Err(MemoryError::InvalidConfig(
            "memory decrypt policy decision does not match envelope".to_string(),
        ));
    }
    if request.audit_id != request.envelope.audit_id {
        return Err(MemoryError::InvalidConfig(
            "memory decrypt audit id does not match envelope".to_string(),
        ));
    }
    if !tenant_scopes_match(&request.tenant_scope, &request.envelope.key_scope) {
        return Err(MemoryError::InvalidConfig(
            "memory decrypt tenant scope does not match envelope".to_string(),
        ));
    }
    if request.principal.tenant_scope != request.tenant_scope {
        return Err(MemoryError::InvalidConfig(
            "memory decrypt principal tenant scope does not match request".to_string(),
        ));
    }
    if !request
        .principal
        .allowed_data_classes
        .contains(&request.envelope.key_scope.data_class)
    {
        return Err(MemoryError::InvalidConfig(
            "memory decrypt principal lacks data-class grant".to_string(),
        ));
    }
    if let Some(source_binding_id) = request.envelope.key_scope.source_binding_id.as_deref() {
        if !request
            .principal
            .allowed_source_binding_ids
            .iter()
            .any(|allowed| allowed == source_binding_id)
        {
            return Err(MemoryError::InvalidConfig(
                "memory decrypt principal lacks source-binding grant".to_string(),
            ));
        }
    }
    Ok(())
}

fn tenant_scopes_match(
    tenant_scope: &MemoryTenantScope,
    key_scope: &crate::envelope::MemoryKeyScope,
) -> bool {
    tenant_scope.org_id == key_scope.org_id
        && tenant_scope.workspace_id == key_scope.workspace_id
        && tenant_scope.deployment_id.as_deref().unwrap_or("")
            == key_scope.deployment_id.as_deref().unwrap_or("")
}

fn validate_tenant_scope(tenant_scope: &MemoryTenantScope) -> MemoryResult<()> {
    for (field, value) in [
        ("org_id", tenant_scope.org_id.as_str()),
        ("workspace_id", tenant_scope.workspace_id.as_str()),
    ] {
        if is_wildcard_or_blank(value) {
            return Err(MemoryError::InvalidConfig(format!(
                "memory decrypt principal must not use wildcard `{field}`"
            )));
        }
    }
    if tenant_scope
        .deployment_id
        .as_deref()
        .map(is_wildcard_or_blank)
        .unwrap_or(false)
    {
        return Err(MemoryError::InvalidConfig(
            "memory decrypt principal must not use wildcard `deployment_id`".to_string(),
        ));
    }
    Ok(())
}

fn is_wildcard_or_blank(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "*" | "all" | "global" | "default"
    )
}

fn non_empty_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{MemoryEnvelopeMetadata, MemoryKeyScope};
    use crate::key_lifecycle::{
        MemoryBreakGlassGrant, MemoryKeyLifecycleOutcome, MemoryKeyLifecyclePolicy,
        MemoryKeyScopeRevocation, MemoryKeyVersionEvidence, MemoryKeyVersionState,
    };

    fn tenant_scope() -> MemoryTenantScope {
        MemoryTenantScope {
            org_id: "acme".to_string(),
            workspace_id: "finance".to_string(),
            deployment_id: Some("prod".to_string()),
        }
    }

    fn envelope(data_class: DataClass, source_binding_id: Option<&str>) -> MemoryEnvelopeMetadata {
        MemoryEnvelopeMetadata {
            key_scope: MemoryKeyScope::new(
                &tenant_scope(),
                data_class,
                source_binding_id.map(ToOwned::to_owned),
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

    fn broker() -> MemoryDecryptBroker {
        MemoryDecryptBroker::new(
            MemoryDecryptBrokerConfig::hosted("google_cloud_kms", "runtime-memory-decryptor")
                .expect("hosted config"),
        )
        .expect("broker")
    }

    fn principal(data_classes: Vec<DataClass>, sources: Vec<&str>) -> MemoryDecryptPrincipal {
        MemoryDecryptPrincipal::retrieval_gateway(
            "kb-mcp-retrieval-gateway",
            tenant_scope(),
            data_classes,
            sources.into_iter().map(ToOwned::to_owned).collect(),
        )
    }

    fn request(
        envelope: MemoryEnvelopeMetadata,
        principal: MemoryDecryptPrincipal,
    ) -> MemoryDecryptRequest {
        MemoryDecryptRequest {
            envelope,
            tenant_scope: tenant_scope(),
            principal,
            policy_decision_id: "decision-1".to_string(),
            audit_id: "audit-1".to_string(),
            break_glass_requested: false,
            key_lifecycle_policy: None,
        }
    }

    fn active_lifecycle_policy() -> MemoryKeyLifecyclePolicy {
        MemoryKeyLifecyclePolicy {
            key_versions: vec![MemoryKeyVersionEvidence {
                kek_id: "projects/acme/locations/global/keyRings/memory/cryptoKeys/finance"
                    .to_string(),
                kek_version: "1".to_string(),
                state: MemoryKeyVersionState::Primary,
                rotation_epoch: 0,
                evidence_id: "key-evidence-1".to_string(),
            }],
            revoked_scopes: vec![],
            break_glass_grants: vec![],
            minimum_rotation_epoch: 0,
            now_ms: 1_000,
        }
    }

    #[test]
    fn local_disabled_broker_is_noop() {
        let broker = MemoryDecryptBroker::new(MemoryDecryptBrokerConfig::local_disabled())
            .expect("local broker");
        let ticket = broker
            .authorize_unwrap(request(
                envelope(DataClass::Internal, None),
                principal(vec![DataClass::Internal], vec![]),
            ))
            .expect("local noop");
        assert!(ticket.is_none());
    }

    #[test]
    fn hosted_config_fails_closed_without_provider_or_principal() {
        let missing_provider = MemoryDecryptBrokerConfig {
            provider: String::new(),
            runtime_principal_id: "runtime-memory-decryptor".to_string(),
            secret_family: MemorySecretFamily::MemoryEnvelope,
            hosted_required: true,
        };
        assert!(missing_provider.validate().is_err());

        let missing_principal = MemoryDecryptBrokerConfig {
            provider: "google_cloud_kms".to_string(),
            runtime_principal_id: String::new(),
            secret_family: MemorySecretFamily::MemoryEnvelope,
            hosted_required: true,
        };
        assert!(missing_principal.validate().is_err());
    }

    #[test]
    fn hosted_missing_provider_returns_denied_audit_event() {
        let broker = MemoryDecryptBroker::new(MemoryDecryptBrokerConfig {
            provider: String::new(),
            runtime_principal_id: "runtime-memory-decryptor".to_string(),
            secret_family: MemorySecretFamily::MemoryEnvelope,
            hosted_required: true,
        })
        .expect("broker");
        let authorization = broker
            .authorize_unwrap_with_audit(request(
                envelope(DataClass::Internal, None),
                principal(vec![DataClass::Internal], vec![]),
            ))
            .expect("authorization");
        assert!(authorization.ticket.is_none());
        assert_eq!(
            authorization.audit_event.outcome,
            MemoryDecryptAuditOutcome::Denied
        );
        assert_eq!(
            authorization.audit_event.principal_id,
            "kb-mcp-retrieval-gateway"
        );
        assert_eq!(authorization.audit_event.audit_id, "audit-1");
        assert!(authorization
            .audit_event
            .reason
            .contains("explicit KMS provider"));
    }

    #[test]
    fn hosted_unwrap_requires_matching_tenant_scope() {
        let mut other_tenant = tenant_scope();
        other_tenant.workspace_id = "hr".to_string();
        let mut principal = principal(vec![DataClass::Internal], vec![]);
        principal.tenant_scope = other_tenant;
        let err = broker()
            .authorize_unwrap(request(envelope(DataClass::Internal, None), principal))
            .expect_err("tenant mismatch rejected");
        assert!(err
            .to_string()
            .contains("principal tenant scope does not match request"));
    }

    #[test]
    fn hosted_unwrap_requires_data_class_grant() {
        let err = broker()
            .authorize_unwrap(request(
                envelope(DataClass::FinancialRecord, None),
                principal(vec![DataClass::Internal], vec![]),
            ))
            .expect_err("data-class mismatch rejected");
        assert!(err.to_string().contains("lacks data-class grant"));
    }

    #[test]
    fn low_risk_principal_cannot_decrypt_sensitive_classes() {
        for sensitive_class in [
            DataClass::Restricted,
            DataClass::Credential,
            DataClass::FinancialRecord,
            DataClass::Executive,
        ] {
            let err = broker()
                .authorize_unwrap(request(
                    envelope(sensitive_class, None),
                    principal(vec![DataClass::Public, DataClass::Internal], vec![]),
                ))
                .expect_err("sensitive data class rejected");
            assert!(err.to_string().contains("lacks data-class grant"));
        }
    }

    #[test]
    fn key_administration_principal_cannot_unwrap_data_keys() {
        let mut admin = principal(vec![DataClass::Internal], vec![]);
        admin.purpose = MemoryDecryptPurpose::KeyAdministration;
        let err = broker()
            .authorize_unwrap(request(envelope(DataClass::Internal, None), admin))
            .expect_err("key admin decrypt rejected");
        assert!(err.to_string().contains("cannot unwrap memory DEKs"));
    }

    #[test]
    fn hosted_retrieval_requires_source_binding_grant() {
        let err = broker()
            .authorize_unwrap(request(
                envelope(DataClass::Confidential, Some("drive-finance")),
                principal(vec![DataClass::Confidential], vec!["drive-hr"]),
            ))
            .expect_err("source-binding mismatch rejected");
        assert!(err.to_string().contains("lacks source-binding grant"));
    }

    #[test]
    fn hosted_unwrap_ticket_is_bound_to_scope_policy_and_audit() {
        let ticket = broker()
            .authorize_unwrap(request(
                envelope(DataClass::Confidential, Some("drive-finance")),
                principal(vec![DataClass::Confidential], vec!["drive-finance"]),
            ))
            .expect("authorized")
            .expect("ticket");
        assert_eq!(ticket.provider, "google_cloud_kms");
        assert_eq!(ticket.runtime_principal_id, "runtime-memory-decryptor");
        assert_eq!(ticket.principal_id, "kb-mcp-retrieval-gateway");
        assert_eq!(ticket.policy_decision_id, "decision-1");
        assert_eq!(ticket.audit_id, "audit-1");
        assert_eq!(ticket.wrapped_dek, "wrapped");
        assert_eq!(ticket.algorithm, "AES-256-GCM");
        assert!(ticket
            .key_scope_id
            .contains("/confidential/source/drive-finance"));
    }

    #[test]
    fn hosted_unwrap_denies_revoked_key_scope() {
        let envelope = envelope(DataClass::Confidential, Some("drive-finance"));
        let mut lifecycle = active_lifecycle_policy();
        lifecycle.revoked_scopes.push(MemoryKeyScopeRevocation {
            key_scope: envelope.key_scope.clone(),
            reason: "source revoked".to_string(),
            revoked_by: "security-admin".to_string(),
            revoked_at_ms: 900,
            evidence_id: "revocation-1".to_string(),
        });
        let mut request = request(
            envelope,
            principal(vec![DataClass::Confidential], vec!["drive-finance"]),
        );
        request.key_lifecycle_policy = Some(lifecycle);

        let authorization = broker()
            .authorize_unwrap_with_audit(request)
            .expect("authorization");
        assert!(authorization.ticket.is_none());
        assert_eq!(
            authorization.audit_event.outcome,
            MemoryDecryptAuditOutcome::Denied
        );
        assert!(authorization
            .audit_event
            .reason
            .contains("scope is revoked"));
    }

    #[test]
    fn hosted_unwrap_denies_disabled_key_without_break_glass() {
        let mut lifecycle = active_lifecycle_policy();
        lifecycle.key_versions[0].state = MemoryKeyVersionState::Disabled;
        let mut request = request(
            envelope(DataClass::Confidential, Some("drive-finance")),
            principal(vec![DataClass::Confidential], vec!["drive-finance"]),
        );
        request.key_lifecycle_policy = Some(lifecycle);

        let err = broker()
            .authorize_unwrap(request)
            .expect_err("disabled key denied");
        assert!(err.to_string().contains("not active"));
    }

    #[test]
    fn hosted_unwrap_allows_scoped_break_glass_for_disabled_key() {
        let envelope = envelope(DataClass::Confidential, Some("drive-finance"));
        let mut lifecycle = active_lifecycle_policy();
        lifecycle.key_versions[0].state = MemoryKeyVersionState::Disabled;
        lifecycle.break_glass_grants.push(MemoryBreakGlassGrant {
            actor_id: "kb-mcp-retrieval-gateway".to_string(),
            approval_id: "approval-1".to_string(),
            reason: "customer incident".to_string(),
            key_scope: envelope.key_scope.clone(),
            expires_at_ms: 2_000,
            max_export_items: 5,
            evidence_id: "break-glass-1".to_string(),
        });
        let mut request = request(
            envelope,
            principal(vec![DataClass::Confidential], vec!["drive-finance"]),
        );
        request.break_glass_requested = true;
        request.key_lifecycle_policy = Some(lifecycle);

        let ticket = broker()
            .authorize_unwrap(request)
            .expect("break-glass authorized")
            .expect("ticket");
        assert_eq!(
            ticket
                .key_lifecycle_decision
                .as_ref()
                .map(|decision| decision.outcome),
            Some(MemoryKeyLifecycleOutcome::BreakGlassAllowed)
        );
    }

    #[test]
    fn hosted_unwrap_rejects_policy_or_audit_substitution() {
        let mut request = request(
            envelope(DataClass::Internal, None),
            principal(vec![DataClass::Internal], vec![]),
        );
        request.policy_decision_id = "decision-2".to_string();
        let err = broker()
            .authorize_unwrap(request)
            .expect_err("policy substitution rejected");
        assert!(err
            .to_string()
            .contains("policy decision does not match envelope"));
    }
}
