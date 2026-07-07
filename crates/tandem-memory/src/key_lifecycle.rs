use crate::envelope::{MemoryEnvelopeMetadata, MemoryKeyScope};
use crate::types::{MemoryError, MemoryResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKeyVersionState {
    Primary,
    Active,
    Disabled,
    Revoked,
    Destroyed,
}

impl MemoryKeyVersionState {
    pub fn allows_normal_decrypt(self) -> bool {
        matches!(self, Self::Primary | Self::Active)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryKeyVersionEvidence {
    pub kek_id: String,
    pub kek_version: String,
    pub state: MemoryKeyVersionState,
    pub rotation_epoch: u64,
    pub evidence_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryKeyScopeRevocation {
    pub key_scope: MemoryKeyScope,
    pub reason: String,
    pub revoked_by: String,
    pub revoked_at_ms: u64,
    pub evidence_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryBreakGlassGrant {
    pub actor_id: String,
    pub approval_id: String,
    pub reason: String,
    pub key_scope: MemoryKeyScope,
    pub expires_at_ms: u64,
    pub max_export_items: u32,
    pub evidence_id: String,
}

impl MemoryBreakGlassGrant {
    pub fn is_active_for(&self, now_ms: u64, actor_id: &str, key_scope: &MemoryKeyScope) -> bool {
        self.expires_at_ms > now_ms
            && self.actor_id == actor_id
            && key_scope_matches(&self.key_scope, key_scope)
            && !self.approval_id.trim().is_empty()
            && !self.reason.trim().is_empty()
            && self.max_export_items > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MemoryKeyLifecyclePolicy {
    #[serde(default)]
    pub key_versions: Vec<MemoryKeyVersionEvidence>,
    #[serde(default)]
    pub revoked_scopes: Vec<MemoryKeyScopeRevocation>,
    #[serde(default)]
    pub break_glass_grants: Vec<MemoryBreakGlassGrant>,
    pub minimum_rotation_epoch: u64,
    pub now_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKeyLifecycleOutcome {
    Allowed,
    BreakGlassAllowed,
    Denied,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryKeyLifecycleDecision {
    pub outcome: MemoryKeyLifecycleOutcome,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_id: Option<String>,
}

impl MemoryKeyLifecycleDecision {
    pub fn allowed(reason: impl Into<String>, evidence_id: Option<String>) -> Self {
        Self {
            outcome: MemoryKeyLifecycleOutcome::Allowed,
            reason: reason.into(),
            evidence_id,
        }
    }

    pub fn break_glass(reason: impl Into<String>, evidence_id: Option<String>) -> Self {
        Self {
            outcome: MemoryKeyLifecycleOutcome::BreakGlassAllowed,
            reason: reason.into(),
            evidence_id,
        }
    }

    pub fn denied(reason: impl Into<String>, evidence_id: Option<String>) -> Self {
        Self {
            outcome: MemoryKeyLifecycleOutcome::Denied,
            reason: reason.into(),
            evidence_id,
        }
    }

    pub fn into_result(self) -> MemoryResult<Self> {
        if self.outcome == MemoryKeyLifecycleOutcome::Denied {
            return Err(MemoryError::InvalidConfig(self.reason));
        }
        Ok(self)
    }
}

pub fn evaluate_memory_key_lifecycle(
    envelope: &MemoryEnvelopeMetadata,
    actor_id: &str,
    break_glass_requested: bool,
    policy: &MemoryKeyLifecyclePolicy,
) -> MemoryKeyLifecycleDecision {
    if envelope.rotation_epoch < policy.minimum_rotation_epoch {
        return MemoryKeyLifecycleDecision::denied("memory key rotation epoch is stale", None);
    }

    if let Some(revocation) = policy
        .revoked_scopes
        .iter()
        .find(|revocation| key_scope_matches(&revocation.key_scope, &envelope.key_scope))
    {
        if break_glass_requested {
            if let Some(grant) = policy
                .break_glass_grants
                .iter()
                .find(|grant| grant.is_active_for(policy.now_ms, actor_id, &envelope.key_scope))
            {
                return MemoryKeyLifecycleDecision::break_glass(
                    "memory key scope revocation bypassed by scoped break-glass grant",
                    Some(grant.evidence_id.clone()),
                );
            }
        }
        return MemoryKeyLifecycleDecision::denied(
            "memory key scope is revoked",
            Some(revocation.evidence_id.clone()),
        );
    }

    let Some(version) = policy.key_versions.iter().find(|version| {
        version.kek_id == envelope.kek_id && version.kek_version == envelope.kek_version
    }) else {
        return MemoryKeyLifecycleDecision::denied("memory key version evidence is missing", None);
    };

    if version.rotation_epoch < envelope.rotation_epoch {
        return MemoryKeyLifecycleDecision::denied(
            "memory key version evidence is older than envelope rotation epoch",
            Some(version.evidence_id.clone()),
        );
    }

    if version.state.allows_normal_decrypt() {
        return MemoryKeyLifecycleDecision::allowed(
            "memory key version is active",
            Some(version.evidence_id.clone()),
        );
    }

    if break_glass_requested {
        if let Some(grant) = policy
            .break_glass_grants
            .iter()
            .find(|grant| grant.is_active_for(policy.now_ms, actor_id, &envelope.key_scope))
        {
            return MemoryKeyLifecycleDecision::break_glass(
                "memory key version bypassed by scoped break-glass grant",
                Some(grant.evidence_id.clone()),
            );
        }
    }

    MemoryKeyLifecycleDecision::denied(
        "memory key version is not active for normal decrypt",
        Some(version.evidence_id.clone()),
    )
}

pub fn key_scope_matches(expected: &MemoryKeyScope, actual: &MemoryKeyScope) -> bool {
    expected.org_id == actual.org_id
        && expected.workspace_id == actual.workspace_id
        && expected.deployment_id.as_deref().unwrap_or("")
            == actual.deployment_id.as_deref().unwrap_or("")
        // Department is a key dimension (TAN-662): a revocation / break-glass
        // grant scoped to one department must not match another department's
        // key scope in the same tenant + data class.
        && expected.org_unit.as_deref().unwrap_or("") == actual.org_unit.as_deref().unwrap_or("")
        && expected.data_class == actual.data_class
        && expected.source_binding_id.as_deref().unwrap_or("")
            == actual.source_binding_id.as_deref().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::envelope::{MemoryEnvelopeMetadata, MemoryKeyScope};
    use crate::types::MemoryTenantScope;
    use tandem_enterprise_contract::DataClass;

    fn tenant_scope() -> MemoryTenantScope {
        MemoryTenantScope {
            org_id: "acme".to_string(),
            workspace_id: "finance".to_string(),
            deployment_id: Some("prod".to_string()),
        }
    }

    fn envelope() -> MemoryEnvelopeMetadata {
        MemoryEnvelopeMetadata {
            key_scope: MemoryKeyScope::new(
                &tenant_scope(),
                DataClass::FinancialRecord,
                Some("drive-finance".to_string()),
            ),
            kek_id: "kek-finance".to_string(),
            kek_version: "7".to_string(),
            wrapped_dek: "wrapped".to_string(),
            algorithm: "AES-256-GCM".to_string(),
            encryption_context_hash: "ctx-hash".to_string(),
            rotation_epoch: 4,
            policy_decision_id: "decision-1".to_string(),
            audit_id: "audit-1".to_string(),
        }
    }

    fn active_policy() -> MemoryKeyLifecyclePolicy {
        MemoryKeyLifecyclePolicy {
            key_versions: vec![MemoryKeyVersionEvidence {
                kek_id: "kek-finance".to_string(),
                kek_version: "7".to_string(),
                state: MemoryKeyVersionState::Primary,
                rotation_epoch: 4,
                evidence_id: "key-evidence-1".to_string(),
            }],
            revoked_scopes: vec![],
            break_glass_grants: vec![],
            minimum_rotation_epoch: 0,
            now_ms: 1_000,
        }
    }

    #[test]
    fn active_key_version_allows_normal_decrypt() {
        let decision =
            evaluate_memory_key_lifecycle(&envelope(), "actor-1", false, &active_policy());
        assert_eq!(decision.outcome, MemoryKeyLifecycleOutcome::Allowed);
        assert_eq!(decision.evidence_id.as_deref(), Some("key-evidence-1"));
    }

    #[test]
    fn disabled_key_version_denies_normal_decrypt() {
        let mut policy = active_policy();
        policy.key_versions[0].state = MemoryKeyVersionState::Disabled;
        let decision = evaluate_memory_key_lifecycle(&envelope(), "actor-1", false, &policy);
        assert_eq!(decision.outcome, MemoryKeyLifecycleOutcome::Denied);
        assert!(decision.reason.contains("not active"));
    }

    #[test]
    fn revoked_scope_denies_normal_decrypt() {
        let mut policy = active_policy();
        policy.revoked_scopes.push(MemoryKeyScopeRevocation {
            key_scope: envelope().key_scope,
            reason: "connector revoked".to_string(),
            revoked_by: "security-admin".to_string(),
            revoked_at_ms: 900,
            evidence_id: "revocation-1".to_string(),
        });
        let decision = evaluate_memory_key_lifecycle(&envelope(), "actor-1", false, &policy);
        assert_eq!(decision.outcome, MemoryKeyLifecycleOutcome::Denied);
        assert_eq!(decision.evidence_id.as_deref(), Some("revocation-1"));
    }

    #[test]
    fn revocation_is_scoped_per_department() {
        // TAN-662: a revocation scoped to one department must not deny another
        // department's key scope in the same tenant + data class + source.
        let mut sales_envelope = envelope();
        sales_envelope.key_scope = sales_envelope
            .key_scope
            .with_org_unit(Some("department/sales".to_string()));

        let mut policy = active_policy();
        policy.revoked_scopes.push(MemoryKeyScopeRevocation {
            key_scope: envelope()
                .key_scope
                .with_org_unit(Some("department/engineering".to_string())),
            reason: "engineering key compromised".to_string(),
            revoked_by: "security-admin".to_string(),
            revoked_at_ms: 900,
            evidence_id: "revocation-eng".to_string(),
        });

        // The sales scope is unaffected by the engineering revocation.
        let decision = evaluate_memory_key_lifecycle(&sales_envelope, "actor-1", false, &policy);
        assert_eq!(decision.outcome, MemoryKeyLifecycleOutcome::Allowed);

        // Revoking the sales scope specifically now denies it.
        policy.revoked_scopes.push(MemoryKeyScopeRevocation {
            key_scope: sales_envelope.key_scope.clone(),
            reason: "sales key compromised".to_string(),
            revoked_by: "security-admin".to_string(),
            revoked_at_ms: 900,
            evidence_id: "revocation-sales".to_string(),
        });
        let decision = evaluate_memory_key_lifecycle(&sales_envelope, "actor-1", false, &policy);
        assert_eq!(decision.outcome, MemoryKeyLifecycleOutcome::Denied);
        assert_eq!(decision.evidence_id.as_deref(), Some("revocation-sales"));
    }

    #[test]
    fn break_glass_requires_matching_active_grant() {
        let mut policy = active_policy();
        policy.key_versions[0].state = MemoryKeyVersionState::Disabled;
        policy.break_glass_grants.push(MemoryBreakGlassGrant {
            actor_id: "support-1".to_string(),
            approval_id: "approval-1".to_string(),
            reason: "customer incident".to_string(),
            key_scope: envelope().key_scope,
            expires_at_ms: 2_000,
            max_export_items: 5,
            evidence_id: "break-glass-1".to_string(),
        });

        let denied = evaluate_memory_key_lifecycle(&envelope(), "support-1", false, &policy);
        assert_eq!(denied.outcome, MemoryKeyLifecycleOutcome::Denied);

        let allowed = evaluate_memory_key_lifecycle(&envelope(), "support-1", true, &policy);
        assert_eq!(
            allowed.outcome,
            MemoryKeyLifecycleOutcome::BreakGlassAllowed
        );
        assert_eq!(allowed.evidence_id.as_deref(), Some("break-glass-1"));
    }
}
