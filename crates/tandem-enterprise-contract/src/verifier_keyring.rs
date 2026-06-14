//! EAA-04 (TAN-29): scoped public verifier keyrings.
//!
//! Hosted Tandem signs tokens (context assertions, approval receipts,
//! delegation projections, cross-tenant grants) with private keys that live in
//! the hosted control plane / KMS. Runtime and ACA never see private material —
//! they receive only a **public** verifier keyring: a `kid -> public key` map
//! where every entry is scoped by purpose, organization, deployment, audience,
//! resource scope, status, and validity window.
//!
//! [`VerifierKeyring::resolve_verifying_key`] is the single fail-closed lookup
//! every verifier should route through: it returns an `ed25519` [`VerifyingKey`]
//! only when the presented `kid` maps to an active, in-window key whose declared
//! purpose and scope match the token being verified. A key minted for one
//! purpose (e.g. context assertion) can therefore never verify a token of
//! another lane (e.g. an approval receipt), and a key scoped to one
//! org/deployment/audience/resource-prefix cannot verify outside that scope.
//!
//! This formalizes the ad-hoc context-assertion keyring that previously lived
//! private to the server middleware, into one shared, purpose-aware contract
//! type that runtime and ACA can both load from distributed public keyrings.

use base64::Engine as _;
use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::SigningKeyPurpose;

/// Lifecycle status of a verifier key. Only [`KeyStatus::Active`] keys verify.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyStatus {
    /// In service: may verify tokens (within its window/scope).
    #[default]
    Active,
    /// Rotated out: kept for audit/lookup but must not verify new tokens.
    Retired,
    /// Compromised or explicitly revoked: must never verify.
    Revoked,
}

impl KeyStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Retired => "retired",
            Self::Revoked => "revoked",
        }
    }
}

/// Why a key did not authorize verification of a token. Every variant is a hard
/// block; there is no allow-on-ambiguity path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyringDenial {
    /// No key is registered for the presented `kid`.
    UnknownKid,
    /// The key exists but was minted for a different [`SigningKeyPurpose`].
    WrongPurpose,
    /// The key is retired (rotated out) and must not verify new tokens.
    KeyRetired,
    /// The key is revoked (compromised) and must never verify.
    KeyRevoked,
    /// `now` is before the key's `not_before_ms`.
    NotYetValid,
    /// `now` is at or after the key's `not_after_ms`.
    Expired,
    /// The token audience is not in the key's `allowed_audiences`.
    AudienceNotAllowed,
    /// The token's organization does not match the key's `organization_id`.
    OrganizationMismatch,
    /// The token's deployment does not match the key's `deployment_id`.
    DeploymentMismatch,
    /// The token's resource scope is outside the key's allowed prefixes.
    ResourceScopeNotAllowed,
    /// The stored public key is not a valid base64 Ed25519 verifying key.
    MalformedKey,
}

impl KeyringDenial {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnknownKid => "verifier_key_unknown_kid",
            Self::WrongPurpose => "verifier_key_wrong_purpose",
            Self::KeyRetired => "verifier_key_retired",
            Self::KeyRevoked => "verifier_key_revoked",
            Self::NotYetValid => "verifier_key_not_yet_valid",
            Self::Expired => "verifier_key_expired",
            Self::AudienceNotAllowed => "verifier_key_audience_not_allowed",
            Self::OrganizationMismatch => "verifier_key_organization_mismatch",
            Self::DeploymentMismatch => "verifier_key_deployment_mismatch",
            Self::ResourceScopeNotAllowed => "verifier_key_resource_scope_not_allowed",
            Self::MalformedKey => "verifier_key_malformed",
        }
    }
}

/// The scope a token is being verified under. A key authorizes verification
/// only when the token's scope is within the key's declared scope.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeyUsageContext {
    pub audience: Option<String>,
    pub organization_id: Option<String>,
    pub deployment_id: Option<String>,
    /// Resource scope string/path the token operates on (prefix-matched against
    /// the key's `allowed_resource_scope_prefixes`).
    pub resource_scope: Option<String>,
}

impl KeyUsageContext {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_audience(mut self, audience: impl Into<String>) -> Self {
        self.audience = Some(audience.into());
        self
    }

    pub fn with_organization_id(mut self, organization_id: impl Into<String>) -> Self {
        self.organization_id = Some(organization_id.into());
        self
    }

    pub fn with_deployment_id(mut self, deployment_id: impl Into<String>) -> Self {
        self.deployment_id = Some(deployment_id.into());
        self
    }

    pub fn with_resource_scope(mut self, resource_scope: impl Into<String>) -> Self {
        self.resource_scope = Some(resource_scope.into());
        self
    }
}

/// One public verifier key plus its scoping metadata. This is the per-`kid`
/// record an operator records in the deployment keyring (the matching private
/// key/version lives in KMS, referenced by [`Self::kms_key_reference`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifierKeyEntry {
    /// Key id. Set from the keyring map key on load; not part of the map value.
    #[serde(default, skip)]
    pub kid: String,
    /// The lane this key may verify. A key verifies only tokens of this purpose.
    pub purpose: SigningKeyPurpose,
    /// Base64 (url-safe or standard) of the 32-byte Ed25519 public key.
    pub public_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub organization_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_audiences: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_resource_scope_prefixes: Vec<String>,
    #[serde(default)]
    pub status: KeyStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_before_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_after_ms: Option<u64>,
    /// Control-plane reference to the KMS key/version holding the PRIVATE key
    /// (e.g. a Google KMS resource name). Metadata only — runtime/ACA never use
    /// it; they only ever hold the public key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kms_key_reference: Option<String>,
}

impl VerifierKeyEntry {
    /// Build a minimal active entry for `kid`/`purpose`/base64 `public_key`.
    pub fn new(
        kid: impl Into<String>,
        purpose: SigningKeyPurpose,
        public_key: impl Into<String>,
    ) -> Self {
        Self {
            kid: kid.into(),
            purpose,
            public_key: public_key.into(),
            organization_id: None,
            deployment_id: None,
            allowed_audiences: Vec::new(),
            allowed_resource_scope_prefixes: Vec::new(),
            status: KeyStatus::Active,
            not_before_ms: None,
            not_after_ms: None,
            kms_key_reference: None,
        }
    }

    pub fn with_organization_id(mut self, organization_id: impl Into<String>) -> Self {
        self.organization_id = Some(organization_id.into());
        self
    }

    pub fn with_deployment_id(mut self, deployment_id: impl Into<String>) -> Self {
        self.deployment_id = Some(deployment_id.into());
        self
    }

    pub fn with_allowed_audiences(mut self, audiences: Vec<String>) -> Self {
        self.allowed_audiences = audiences;
        self
    }

    pub fn with_allowed_resource_scope_prefixes(mut self, prefixes: Vec<String>) -> Self {
        self.allowed_resource_scope_prefixes = prefixes;
        self
    }

    pub fn with_status(mut self, status: KeyStatus) -> Self {
        self.status = status;
        self
    }

    pub fn with_validity_window(mut self, not_before_ms: u64, not_after_ms: u64) -> Self {
        self.not_before_ms = Some(not_before_ms);
        self.not_after_ms = Some(not_after_ms);
        self
    }

    pub fn with_kms_key_reference(mut self, reference: impl Into<String>) -> Self {
        self.kms_key_reference = Some(reference.into());
        self
    }

    /// Decode the stored base64 public key into an Ed25519 [`VerifyingKey`].
    pub fn verifying_key(&self) -> Result<VerifyingKey, KeyringDenial> {
        let bytes = decode_public_key_bytes(&self.public_key).ok_or(KeyringDenial::MalformedKey)?;
        VerifyingKey::from_bytes(&bytes).map_err(|_| KeyringDenial::MalformedKey)
    }

    /// Fail-closed check that this key authorizes verifying a token of `purpose`
    /// under `usage` at `now_ms`. Returns the decoded key on success.
    pub fn authorize(
        &self,
        purpose: SigningKeyPurpose,
        usage: &KeyUsageContext,
        now_ms: u64,
    ) -> Result<VerifyingKey, KeyringDenial> {
        if self.purpose != purpose {
            return Err(KeyringDenial::WrongPurpose);
        }
        match self.status {
            KeyStatus::Active => {}
            KeyStatus::Retired => return Err(KeyringDenial::KeyRetired),
            KeyStatus::Revoked => return Err(KeyringDenial::KeyRevoked),
        }
        if let Some(not_before_ms) = self.not_before_ms {
            if now_ms < not_before_ms {
                return Err(KeyringDenial::NotYetValid);
            }
        }
        if let Some(not_after_ms) = self.not_after_ms {
            if now_ms >= not_after_ms {
                return Err(KeyringDenial::Expired);
            }
        }
        if !self.allowed_audiences.is_empty() {
            let allowed = usage
                .audience
                .as_ref()
                .is_some_and(|audience| self.allowed_audiences.iter().any(|a| a == audience));
            if !allowed {
                return Err(KeyringDenial::AudienceNotAllowed);
            }
        }
        if let Some(key_org) = self.organization_id.as_ref() {
            if usage.organization_id.as_ref() != Some(key_org) {
                return Err(KeyringDenial::OrganizationMismatch);
            }
        }
        if let Some(key_deployment) = self.deployment_id.as_ref() {
            if usage.deployment_id.as_ref() != Some(key_deployment) {
                return Err(KeyringDenial::DeploymentMismatch);
            }
        }
        if !self.allowed_resource_scope_prefixes.is_empty() {
            let allowed = usage.resource_scope.as_ref().is_some_and(|scope| {
                self.allowed_resource_scope_prefixes
                    .iter()
                    .any(|prefix| scope_within_prefix(scope, prefix))
            });
            if !allowed {
                return Err(KeyringDenial::ResourceScopeNotAllowed);
            }
        }
        self.verifying_key()
    }
}

/// A public verifier keyring: `kid -> entry`. Distributed to runtime/ACA; holds
/// only public keys.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VerifierKeyring {
    keys: BTreeMap<String, VerifierKeyEntry>,
}

impl VerifierKeyring {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_entries(entries: impl IntoIterator<Item = VerifierKeyEntry>) -> Self {
        let mut keyring = Self::new();
        for entry in entries {
            keyring.insert(entry);
        }
        keyring
    }

    /// Insert (or replace) an entry, keyed by its `kid`.
    pub fn insert(&mut self, entry: VerifierKeyEntry) {
        self.keys.insert(entry.kid.clone(), entry);
    }

    pub fn get(&self, kid: &str) -> Option<&VerifierKeyEntry> {
        self.keys.get(kid)
    }

    pub fn len(&self) -> usize {
        self.keys.len()
    }

    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// The single fail-closed lookup verifiers route through. Resolves `kid` to
    /// an Ed25519 [`VerifyingKey`] only if the key is registered, active,
    /// in-window, and scoped to this `purpose` and `usage`.
    pub fn resolve_verifying_key(
        &self,
        kid: &str,
        purpose: SigningKeyPurpose,
        usage: &KeyUsageContext,
        now_ms: u64,
    ) -> Result<VerifyingKey, KeyringDenial> {
        let entry = self.keys.get(kid).ok_or(KeyringDenial::UnknownKid)?;
        entry.authorize(purpose, usage, now_ms)
    }

    /// Load a public keyring from its JSON distribution form: a map of
    /// `kid -> { purpose, public_key, ... }`. The `kid` is taken from the map
    /// key; a `kid` field inside the value is ignored.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let raw: BTreeMap<String, VerifierKeyEntry> = serde_json::from_str(json)?;
        let mut keyring = Self::new();
        for (kid, mut entry) in raw {
            entry.kid = kid;
            keyring.insert(entry);
        }
        Ok(keyring)
    }

    /// Serialize to the JSON distribution form (`kid -> value`).
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&self.keys)
    }
}

/// Whether `scope` falls within `prefix`, respecting `/` segment boundaries so
/// a prefix like `org-a/workspace-a` matches `org-a/workspace-a` and
/// `org-a/workspace-a/doc-1` but NOT a sibling such as `org-a/workspace-a2/...`.
fn scope_within_prefix(scope: &str, prefix: &str) -> bool {
    if prefix.is_empty() || scope == prefix {
        return true;
    }
    let boundary = if prefix.ends_with('/') {
        prefix.to_string()
    } else {
        format!("{prefix}/")
    };
    scope.starts_with(&boundary)
}

/// Decode a base64 (url-safe no-pad, then standard) 32-byte Ed25519 public key.
fn decode_public_key_bytes(value: &str) -> Option<[u8; 32]> {
    let trimmed = value.trim();
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(trimmed)
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(trimmed))
        .ok()?;
    bytes.try_into().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn signing_key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn public_b64(key: &SigningKey) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(key.verifying_key().to_bytes())
    }

    fn usage() -> KeyUsageContext {
        KeyUsageContext::new()
            .with_audience("tandem-runtime")
            .with_organization_id("org-a")
            .with_deployment_id("deploy-1")
            .with_resource_scope("org-a/workspace-a/doc-1")
    }

    fn scoped_entry() -> VerifierKeyEntry {
        VerifierKeyEntry::new("key-1", SigningKeyPurpose::ApprovalReceipt, public_b64(&signing_key(7)))
            .with_organization_id("org-a")
            .with_deployment_id("deploy-1")
            .with_allowed_audiences(vec!["tandem-runtime".to_string()])
            .with_allowed_resource_scope_prefixes(vec!["org-a/workspace-a".to_string()])
            .with_validity_window(1_000, 9_000)
    }

    #[test]
    fn resolves_active_in_scope_key() {
        let keyring = VerifierKeyring::from_entries([scoped_entry()]);
        let key = keyring
            .resolve_verifying_key("key-1", SigningKeyPurpose::ApprovalReceipt, &usage(), 5_000)
            .expect("resolves");
        assert_eq!(key.to_bytes(), signing_key(7).verifying_key().to_bytes());
    }

    #[test]
    fn unknown_kid_is_denied() {
        let keyring = VerifierKeyring::from_entries([scoped_entry()]);
        assert_eq!(
            keyring.resolve_verifying_key("nope", SigningKeyPurpose::ApprovalReceipt, &usage(), 5_000),
            Err(KeyringDenial::UnknownKid)
        );
    }

    #[test]
    fn wrong_purpose_key_cannot_cross_lanes() {
        // Same key bytes, but registered for context assertions: it must not
        // verify an approval-receipt token.
        let entry = VerifierKeyEntry::new(
            "key-1",
            SigningKeyPurpose::ContextAssertion,
            public_b64(&signing_key(7)),
        );
        let keyring = VerifierKeyring::from_entries([entry]);
        assert_eq!(
            keyring.resolve_verifying_key("key-1", SigningKeyPurpose::ApprovalReceipt, &usage(), 5_000),
            Err(KeyringDenial::WrongPurpose)
        );
    }

    #[test]
    fn retired_and_revoked_keys_never_verify() {
        let retired = scoped_entry().with_status(KeyStatus::Retired);
        assert_eq!(
            VerifierKeyring::from_entries([retired]).resolve_verifying_key(
                "key-1",
                SigningKeyPurpose::ApprovalReceipt,
                &usage(),
                5_000
            ),
            Err(KeyringDenial::KeyRetired)
        );
        let revoked = scoped_entry().with_status(KeyStatus::Revoked);
        assert_eq!(
            VerifierKeyring::from_entries([revoked]).resolve_verifying_key(
                "key-1",
                SigningKeyPurpose::ApprovalReceipt,
                &usage(),
                5_000
            ),
            Err(KeyringDenial::KeyRevoked)
        );
    }

    #[test]
    fn validity_window_is_enforced() {
        let keyring = VerifierKeyring::from_entries([scoped_entry()]);
        assert_eq!(
            keyring.resolve_verifying_key("key-1", SigningKeyPurpose::ApprovalReceipt, &usage(), 500),
            Err(KeyringDenial::NotYetValid)
        );
        assert_eq!(
            keyring.resolve_verifying_key("key-1", SigningKeyPurpose::ApprovalReceipt, &usage(), 9_000),
            Err(KeyringDenial::Expired)
        );
    }

    #[test]
    fn out_of_scope_org_deployment_audience_resource_are_denied() {
        let keyring = VerifierKeyring::from_entries([scoped_entry()]);
        let base = usage();

        let mut wrong_org = base.clone();
        wrong_org.organization_id = Some("org-b".to_string());
        assert_eq!(
            keyring.resolve_verifying_key("key-1", SigningKeyPurpose::ApprovalReceipt, &wrong_org, 5_000),
            Err(KeyringDenial::OrganizationMismatch)
        );

        let mut wrong_deploy = base.clone();
        wrong_deploy.deployment_id = Some("deploy-2".to_string());
        assert_eq!(
            keyring.resolve_verifying_key("key-1", SigningKeyPurpose::ApprovalReceipt, &wrong_deploy, 5_000),
            Err(KeyringDenial::DeploymentMismatch)
        );

        let mut wrong_aud = base.clone();
        wrong_aud.audience = Some("other-service".to_string());
        assert_eq!(
            keyring.resolve_verifying_key("key-1", SigningKeyPurpose::ApprovalReceipt, &wrong_aud, 5_000),
            Err(KeyringDenial::AudienceNotAllowed)
        );

        let mut wrong_scope = base.clone();
        wrong_scope.resource_scope = Some("org-a/workspace-b/doc-9".to_string());
        assert_eq!(
            keyring.resolve_verifying_key("key-1", SigningKeyPurpose::ApprovalReceipt, &wrong_scope, 5_000),
            Err(KeyringDenial::ResourceScopeNotAllowed)
        );
    }

    #[test]
    fn resource_scope_prefix_respects_segment_boundaries() {
        // Key scoped to `org-a/workspace-a`.
        let keyring = VerifierKeyring::from_entries([scoped_entry()]);
        let allowed_purpose = SigningKeyPurpose::ApprovalReceipt;

        // Exact match and a true child are allowed.
        for scope in ["org-a/workspace-a", "org-a/workspace-a/doc-1"] {
            let usage = usage().with_resource_scope(scope);
            assert!(
                keyring
                    .resolve_verifying_key("key-1", allowed_purpose, &usage, 5_000)
                    .is_ok(),
                "expected {scope} to be in scope"
            );
        }

        // A sibling that merely shares the textual prefix must be rejected.
        for scope in ["org-a/workspace-a2", "org-a/workspace-a2/doc-9", "org-a/workspace-abc"] {
            let usage = usage().with_resource_scope(scope);
            assert_eq!(
                keyring.resolve_verifying_key("key-1", allowed_purpose, &usage, 5_000),
                Err(KeyringDenial::ResourceScopeNotAllowed),
                "expected {scope} to be rejected at the segment boundary"
            );
        }
    }

    #[test]
    fn unscoped_key_allows_any_org_and_audience() {
        // A key that declares no org/deployment/audience/scope restriction is a
        // global key (mirrors the existing context-assertion key semantics).
        let entry = VerifierKeyEntry::new(
            "global",
            SigningKeyPurpose::ContextAssertion,
            public_b64(&signing_key(3)),
        );
        let keyring = VerifierKeyring::from_entries([entry]);
        let any = KeyUsageContext::new().with_organization_id("whatever");
        assert!(keyring
            .resolve_verifying_key("global", SigningKeyPurpose::ContextAssertion, &any, 5_000)
            .is_ok());
    }

    #[test]
    fn malformed_public_key_fails_closed() {
        let entry = VerifierKeyEntry::new("bad", SigningKeyPurpose::ContextAssertion, "not-base64!!");
        let keyring = VerifierKeyring::from_entries([entry]);
        assert_eq!(
            keyring.resolve_verifying_key(
                "bad",
                SigningKeyPurpose::ContextAssertion,
                &KeyUsageContext::new(),
                5_000
            ),
            Err(KeyringDenial::MalformedKey)
        );
    }

    #[test]
    fn json_roundtrip_takes_kid_from_map_key() {
        let json = format!(
            r#"{{ "key-1": {{ "purpose": "approval_receipt", "public_key": "{}", "organization_id": "org-a", "status": "active" }} }}"#,
            public_b64(&signing_key(7))
        );
        let keyring = VerifierKeyring::from_json(&json).expect("parse");
        assert_eq!(keyring.len(), 1);
        let entry = keyring.get("key-1").expect("entry");
        assert_eq!(entry.kid, "key-1");
        assert_eq!(entry.purpose, SigningKeyPurpose::ApprovalReceipt);
        // Reserializes to a kid-keyed map that round-trips.
        let reparsed = VerifierKeyring::from_json(&keyring.to_json().unwrap()).unwrap();
        assert_eq!(reparsed.get("key-1").unwrap().kid, "key-1");
    }

    #[test]
    fn keyring_resolved_key_verifies_a_real_approval_receipt() {
        use crate::approval_receipt::{
            ApprovalReceipt, ApprovalReceiptClaims, ApprovalReceiptHeader, ProtectedActionPayload,
        };
        use crate::{DataClass, PrincipalKind, PrincipalRef};

        let signer = signing_key(7);
        let action = ProtectedActionPayload {
            version: "v1".to_string(),
            org_id: "org-a".to_string(),
            workspace_id: "workspace-a".to_string(),
            deployment_id: Some("deploy-1".to_string()),
            actor_id: Some("approver-1".to_string()),
            execution_principal: PrincipalRef::new(PrincipalKind::AgentWorker, "agent-1"),
            session_id: "ses-1".to_string(),
            run_id: None,
            node_id: None,
            tool: "mcp.bank.release_funds".to_string(),
            args_hash: "args".to_string(),
            resource_target_hash: "res".to_string(),
            data_class: DataClass::FinancialRecord,
            delegation_id: None,
            policy_id: "policy-1".to_string(),
            approval_id: "approval-1".to_string(),
            issued_at_ms: 1_000,
            expires_at_ms: 9_000,
            nonce: "nonce-1".to_string(),
        };
        let claims = ApprovalReceiptClaims {
            version: "v1".to_string(),
            audience: "tandem-runtime".to_string(),
            org_id: action.org_id.clone(),
            workspace_id: action.workspace_id.clone(),
            deployment_id: action.deployment_id.clone(),
            actor_id: action.actor_id.clone(),
            policy_id: action.policy_id.clone(),
            approval_id: action.approval_id.clone(),
            action_hash: action.action_hash(),
            issued_at_ms: 1_000,
            not_before_ms: 1_000,
            expires_at_ms: 9_000,
            issued_by: PrincipalRef::human_user("approver-1"),
        };
        let mut receipt =
            ApprovalReceipt::new(ApprovalReceiptHeader::ed25519("key-1"), claims, String::new());
        let signing_input = receipt.signing_input().expect("signing input");
        receipt.signature = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(signer.sign(signing_input.as_bytes()).to_bytes());

        let keyring = VerifierKeyring::from_entries([scoped_entry()]);
        let usage = KeyUsageContext::new()
            .with_audience("tandem-runtime")
            .with_organization_id("org-a")
            .with_deployment_id("deploy-1")
            .with_resource_scope("org-a/workspace-a/doc-1");

        // The keyring resolves the right key by the receipt's kid + purpose...
        let key = keyring
            .resolve_verifying_key(&receipt.header.kid, SigningKeyPurpose::ApprovalReceipt, &usage, 5_000)
            .expect("resolve");
        // ...and that key verifies the receipt end-to-end.
        assert_eq!(
            receipt.verify_for_action(&action, "tandem-runtime", Some(&key), 5_000),
            Ok(())
        );
    }
}
