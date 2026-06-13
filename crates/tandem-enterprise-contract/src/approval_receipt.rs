//! EAA-07 (TAN-32): signed approval receipts for protected mutations.
//!
//! A protected mutation (money movement, regulatory filing, external customer
//! communication, …) may only execute against a signed [`ApprovalReceipt`]
//! that is bound — by a canonical SHA-256 action hash — to the *exact* action
//! about to run. The receipt is verified against the strict tenant context at
//! execution time: Ed25519 signature, audience, validity window, tenant,
//! actor, policy/approval ids, and the action hash all must match.
//!
//! Fail-closed contract: [`ApprovalReceipt::verify_for_action`] returns
//! `Err(ApprovalReceiptDenial)` for every failure mode (malformed, bad
//! signature, missing key, expired, wrong tenant/actor, mismatched action),
//! and a missing receipt means the caller never gets to `Ok`. Callers MUST
//! treat any non-`Ok` outcome — including verifier crash, timeout, or key
//! outage on their side — as a hard block on the protected mutation.

use base64::Engine as _;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{DataClass, PrincipalRef};

/// JWS `typ` for approval receipts. Distinct from the hosted-assertion,
/// cross-tenant-grant, and delegation-projection lanes, so a receipt can
/// never be replayed as any of those.
pub const APPROVAL_RECEIPT_TYP: &str = "tandem-approval-receipt+jws";

/// Canonical protected-action payload. The execution path constructs this
/// from the action it is *about* to perform; its SHA-256 ([`Self::action_hash`])
/// is what an approval receipt binds to. Includes the full tenant/actor/
/// location/tool/resource binding plus issuance metadata so a receipt cannot
/// be replayed for a different action, tenant, actor, or time window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectedActionPayload {
    pub version: String,
    pub org_id: String,
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment_id: Option<String>,
    /// Human/tenant actor that requested the action (the approval subject).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    /// The principal that will actually execute the mutation (agent/service).
    pub execution_principal: PrincipalRef,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    pub tool: String,
    /// SHA-256 of the normalized tool args (e.g. `fintech_protected_action_hash`).
    pub args_hash: String,
    /// SHA-256 of the resource target descriptor the mutation touches.
    pub resource_target_hash: String,
    pub data_class: DataClass,
    /// Delegation chain that authorized the executor, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegation_id: Option<String>,
    pub policy_id: String,
    pub approval_id: String,
    pub issued_at_ms: u64,
    pub expires_at_ms: u64,
    /// Replay-prevention nonce / bound assertion id.
    pub nonce: String,
}

impl ProtectedActionPayload {
    /// Canonical JSON (object keys sorted, no whitespace) SHA-256, lowercase
    /// hex. Stable across serde field order and map iteration order so an
    /// issuer and a verifier independently derive the same hash.
    pub fn action_hash(&self) -> String {
        let value = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        sha256_hex(canonical_json_string(&value).as_bytes())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalReceiptHeader {
    pub alg: String,
    pub typ: String,
    pub kid: String,
}

impl ApprovalReceiptHeader {
    pub fn ed25519(key_id: impl Into<String>) -> Self {
        Self {
            alg: "EdDSA".to_string(),
            typ: APPROVAL_RECEIPT_TYP.to_string(),
            kid: key_id.into(),
        }
    }

    pub fn is_well_formed(&self) -> bool {
        self.alg == "EdDSA" && self.typ == APPROVAL_RECEIPT_TYP && !self.kid.trim().is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalReceiptClaims {
    pub version: String,
    /// Verifier-service audience (mirrors assertion audience semantics).
    pub audience: String,
    pub org_id: String,
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment_id: Option<String>,
    /// Approval subject the receipt was issued for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    pub policy_id: String,
    pub approval_id: String,
    /// SHA-256 of the canonical [`ProtectedActionPayload`] this receipt
    /// authorizes. Binds the receipt to one exact action.
    pub action_hash: String,
    pub issued_at_ms: u64,
    pub not_before_ms: u64,
    pub expires_at_ms: u64,
    pub issued_by: PrincipalRef,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalReceipt {
    pub header: ApprovalReceiptHeader,
    pub claims: ApprovalReceiptClaims,
    /// Base64url (no pad) Ed25519 signature over `b64(header).b64(claims)`.
    pub signature: String,
}

/// Why an approval receipt does not authorize a protected action. Every
/// variant is a hard block; there is no "allow on error" path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalReceiptDenial {
    Malformed,
    MissingVerifyingKey,
    SignatureInvalid,
    AudienceMismatch,
    NotYetValid,
    Expired,
    TenantMismatch,
    ActorMismatch,
    PolicyMismatch,
    ApprovalMismatch,
    ActionHashMismatch,
    /// The receipt was already consumed — a replay of a single-use receipt.
    Replayed,
}

impl ApprovalReceiptDenial {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Malformed => "approval_receipt_malformed",
            Self::MissingVerifyingKey => "approval_receipt_missing_key",
            Self::SignatureInvalid => "approval_receipt_signature_invalid",
            Self::AudienceMismatch => "approval_receipt_audience_mismatch",
            Self::NotYetValid => "approval_receipt_not_yet_valid",
            Self::Expired => "approval_receipt_expired",
            Self::TenantMismatch => "approval_receipt_tenant_mismatch",
            Self::ActorMismatch => "approval_receipt_actor_mismatch",
            Self::PolicyMismatch => "approval_receipt_policy_mismatch",
            Self::ApprovalMismatch => "approval_receipt_approval_mismatch",
            Self::ActionHashMismatch => "approval_receipt_action_hash_mismatch",
            Self::Replayed => "approval_receipt_replayed",
        }
    }
}

/// One-time-use enforcement for approval receipts. A pure signature/validity
/// check cannot stop a captured receipt from being submitted again before it
/// expires, so non-idempotent protected actions (money movement, filings)
/// must gate execution on single-use consumption. Callers back this with a
/// durable store (e.g. a `(approval_id, nonce)` table with TTL); the
/// in-process [`InMemoryReplayGuard`] is a single-process default and a test
/// double.
pub trait ApprovalReceiptReplayGuard {
    /// Atomically record `(approval_id, nonce)` as consumed. Returns `true`
    /// on first consumption (the caller may proceed), `false` if it was
    /// already consumed (replay — the caller must block). `expires_at_ms`
    /// lets implementations prune entries once a receipt can no longer be
    /// valid. Implementations MUST be atomic against concurrent callers.
    fn consume(&mut self, approval_id: &str, nonce: &str, expires_at_ms: u64) -> bool;
}

/// In-process single-use guard. Suitable for a single-replica deployment or
/// tests; multi-replica deployments need a shared/durable guard.
#[derive(Debug, Default)]
pub struct InMemoryReplayGuard {
    consumed: std::collections::HashMap<(String, String), u64>,
}

impl InMemoryReplayGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop entries whose receipts have expired at or before `now_ms`.
    pub fn prune(&mut self, now_ms: u64) {
        self.consumed
            .retain(|_, expires_at_ms| *expires_at_ms > now_ms);
    }
}

impl ApprovalReceiptReplayGuard for InMemoryReplayGuard {
    fn consume(&mut self, approval_id: &str, nonce: &str, expires_at_ms: u64) -> bool {
        let key = (approval_id.to_string(), nonce.to_string());
        match self.consumed.entry(key) {
            std::collections::hash_map::Entry::Occupied(_) => false,
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(expires_at_ms);
                true
            }
        }
    }
}

impl ApprovalReceipt {
    pub fn new(
        header: ApprovalReceiptHeader,
        claims: ApprovalReceiptClaims,
        signature: impl Into<String>,
    ) -> Self {
        Self {
            header,
            claims,
            signature: signature.into(),
        }
    }

    pub fn is_well_formed(&self) -> bool {
        self.header.is_well_formed()
            && !self.signature.trim().is_empty()
            && self.claims.version == "v1"
            && !self.claims.audience.trim().is_empty()
            && !self.claims.policy_id.trim().is_empty()
            && !self.claims.approval_id.trim().is_empty()
            && !self.claims.action_hash.trim().is_empty()
            && self.claims.expires_at_ms > self.claims.not_before_ms
    }

    /// The Ed25519 signing input: `base64url(header).base64url(claims)`.
    pub fn signing_input(&self) -> Option<String> {
        let header = encode_json_base64url(&self.header)?;
        let claims = encode_json_base64url(&self.claims)?;
        Some(format!("{header}.{claims}"))
    }

    /// Verify the receipt's Ed25519 signature with `verifying_key`. A `None`
    /// key (missing/unresolvable signing key) fails closed.
    pub fn signature_verifies(&self, verifying_key: Option<&VerifyingKey>) -> bool {
        let Some(verifying_key) = verifying_key else {
            return false;
        };
        let Some(signing_input) = self.signing_input() else {
            return false;
        };
        let Some(signature_bytes) = decode_signature_bytes(&self.signature) else {
            return false;
        };
        let signature = Signature::from_bytes(&signature_bytes);
        verifying_key
            .verify(signing_input.as_bytes(), &signature)
            .is_ok()
    }

    /// Fail-closed verification that this receipt authorizes `action` for the
    /// given audience at `now_ms`, signed by `verifying_key`. Returns
    /// `Err(..)` on every failure mode; `Ok(())` only when the signature is
    /// valid and the receipt is bound to this exact action, tenant, actor,
    /// policy, and approval within its validity window.
    pub fn verify_for_action(
        &self,
        action: &ProtectedActionPayload,
        expected_audience: &str,
        verifying_key: Option<&VerifyingKey>,
        now_ms: u64,
    ) -> Result<(), ApprovalReceiptDenial> {
        if !self.is_well_formed() {
            return Err(ApprovalReceiptDenial::Malformed);
        }
        if verifying_key.is_none() {
            return Err(ApprovalReceiptDenial::MissingVerifyingKey);
        }
        if !self.signature_verifies(verifying_key) {
            return Err(ApprovalReceiptDenial::SignatureInvalid);
        }
        if self.claims.audience != expected_audience {
            return Err(ApprovalReceiptDenial::AudienceMismatch);
        }
        if now_ms < self.claims.not_before_ms {
            return Err(ApprovalReceiptDenial::NotYetValid);
        }
        if now_ms >= self.claims.expires_at_ms {
            return Err(ApprovalReceiptDenial::Expired);
        }
        if self.claims.org_id != action.org_id
            || self.claims.workspace_id != action.workspace_id
            || self.claims.deployment_id != action.deployment_id
        {
            return Err(ApprovalReceiptDenial::TenantMismatch);
        }
        if self.claims.actor_id != action.actor_id {
            return Err(ApprovalReceiptDenial::ActorMismatch);
        }
        if self.claims.policy_id != action.policy_id {
            return Err(ApprovalReceiptDenial::PolicyMismatch);
        }
        if self.claims.approval_id != action.approval_id {
            return Err(ApprovalReceiptDenial::ApprovalMismatch);
        }
        // Recompute the canonical hash so the receipt is bound to THIS action,
        // not merely a well-formed one, and not the (possibly stale) hash the
        // issuer happened to embed.
        if self.claims.action_hash != action.action_hash() {
            return Err(ApprovalReceiptDenial::ActionHashMismatch);
        }
        Ok(())
    }

    /// Verify *and* atomically consume the receipt for single use. Runs the
    /// full [`Self::verify_for_action`] check, then consumes
    /// `(approval_id, nonce)` through `guard`; a second call with the same
    /// receipt returns `Err(Replayed)`. Use this — not bare
    /// `verify_for_action` — to gate non-idempotent protected mutations so a
    /// captured receipt cannot authorize duplicate execution before it
    /// expires. The guard is only touched after every other check passes, so
    /// a rejected receipt never burns a nonce.
    pub fn verify_and_consume<G: ApprovalReceiptReplayGuard>(
        &self,
        action: &ProtectedActionPayload,
        expected_audience: &str,
        verifying_key: Option<&VerifyingKey>,
        now_ms: u64,
        guard: &mut G,
    ) -> Result<(), ApprovalReceiptDenial> {
        self.verify_for_action(action, expected_audience, verifying_key, now_ms)?;
        if !guard.consume(
            &self.claims.approval_id,
            &action.nonce,
            self.claims.expires_at_ms,
        ) {
            return Err(ApprovalReceiptDenial::Replayed);
        }
        Ok(())
    }
}

fn encode_json_base64url<T: Serialize>(value: &T) -> Option<String> {
    let bytes = serde_json::to_vec(value).ok()?;
    Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

fn decode_signature_bytes(signature: &str) -> Option<[u8; 64]> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(signature.trim())
        .ok()?;
    bytes.try_into().ok()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

/// Canonical JSON string: object keys sorted ascending, arrays in order, no
/// whitespace. Mirrors the canonicalizer used by `fintech_protected_action_hash`
/// in tandem-core so action hashes agree across crates.
fn canonical_json_string(value: &serde_json::Value) -> String {
    use serde_json::Value;
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string()),
        Value::Array(items) => {
            let body = items
                .iter()
                .map(canonical_json_string)
                .collect::<Vec<_>>()
                .join(",");
            format!("[{body}]")
        }
        Value::Object(map) => {
            let mut entries = map.iter().collect::<Vec<_>>();
            entries.sort_by_key(|(key, _)| *key);
            let body = entries
                .into_iter()
                .map(|(key, value)| {
                    format!(
                        "{}:{}",
                        serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_string()),
                        canonical_json_string(value)
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PrincipalKind;
    use ed25519_dalek::{Signer, SigningKey};

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn action() -> ProtectedActionPayload {
        ProtectedActionPayload {
            version: "v1".to_string(),
            org_id: "org-a".to_string(),
            workspace_id: "workspace-a".to_string(),
            deployment_id: Some("deploy-1".to_string()),
            actor_id: Some("approver-1".to_string()),
            execution_principal: PrincipalRef::new(PrincipalKind::AgentWorker, "agent-1"),
            session_id: "ses-1".to_string(),
            run_id: Some("run-1".to_string()),
            node_id: Some("node-send".to_string()),
            tool: "mcp.bank.release_funds".to_string(),
            args_hash: "args-sha".to_string(),
            resource_target_hash: "resource-sha".to_string(),
            data_class: DataClass::FinancialRecord,
            delegation_id: None,
            policy_id: "policy-money-movement".to_string(),
            approval_id: "approval-1".to_string(),
            issued_at_ms: 1_000,
            expires_at_ms: 9_000,
            nonce: "nonce-1".to_string(),
        }
    }

    fn signed_receipt(action: &ProtectedActionPayload) -> ApprovalReceipt {
        let header = ApprovalReceiptHeader::ed25519("approval-key-1");
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
        let mut receipt = ApprovalReceipt::new(header, claims, String::new());
        let signing_input = receipt.signing_input().expect("signing input");
        let signature = signing_key().sign(signing_input.as_bytes());
        receipt.signature =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.to_bytes());
        receipt
    }

    fn verifying_key() -> VerifyingKey {
        signing_key().verifying_key()
    }

    #[test]
    fn receipt_typ_lane_is_disjoint() {
        assert_ne!(APPROVAL_RECEIPT_TYP, "tandem-tenant-context+jws");
        assert_ne!(APPROVAL_RECEIPT_TYP, "tandem-cross-tenant-grant+jws");
        assert_ne!(APPROVAL_RECEIPT_TYP, "tandem-delegation-projection+jws");
        let mut header = ApprovalReceiptHeader::ed25519("k");
        header.typ = "tandem-tenant-context+jws".to_string();
        assert!(!header.is_well_formed());
    }

    #[test]
    fn action_hash_is_stable_and_field_order_independent() {
        let action = action();
        let hash = action.action_hash();
        assert_eq!(hash.len(), 64);
        // Re-deriving from a reserialized value yields the same hash.
        let roundtrip: ProtectedActionPayload =
            serde_json::from_str(&serde_json::to_string(&action).unwrap()).unwrap();
        assert_eq!(roundtrip.action_hash(), hash);
        // A different action produces a different hash.
        let mut other = action.clone();
        other.args_hash = "different".to_string();
        assert_ne!(other.action_hash(), hash);
    }

    #[test]
    fn valid_receipt_authorizes_the_exact_action() {
        let action = action();
        let receipt = signed_receipt(&action);
        assert_eq!(
            receipt.verify_for_action(&action, "tandem-runtime", Some(&verifying_key()), 5_000),
            Ok(())
        );
    }

    #[test]
    fn missing_key_fails_closed() {
        let action = action();
        let receipt = signed_receipt(&action);
        assert_eq!(
            receipt.verify_for_action(&action, "tandem-runtime", None, 5_000),
            Err(ApprovalReceiptDenial::MissingVerifyingKey)
        );
    }

    #[test]
    fn tampered_signature_or_claims_fail_closed() {
        let action = action();

        // Tampered signature bytes.
        let mut bad_sig = signed_receipt(&action);
        bad_sig.signature = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([0u8; 64]);
        assert_eq!(
            bad_sig.verify_for_action(&action, "tandem-runtime", Some(&verifying_key()), 5_000),
            Err(ApprovalReceiptDenial::SignatureInvalid)
        );

        // Claims mutated after signing (approval_id swapped) invalidates the
        // signature over the canonical claims bytes.
        let mut mutated = signed_receipt(&action);
        mutated.claims.approval_id = "approval-evil".to_string();
        assert_eq!(
            mutated.verify_for_action(&action, "tandem-runtime", Some(&verifying_key()), 5_000),
            Err(ApprovalReceiptDenial::SignatureInvalid)
        );
    }

    #[test]
    fn wrong_audience_expiry_and_window_fail_closed() {
        let action = action();
        let receipt = signed_receipt(&action);
        assert_eq!(
            receipt.verify_for_action(&action, "other-service", Some(&verifying_key()), 5_000),
            Err(ApprovalReceiptDenial::AudienceMismatch)
        );
        assert_eq!(
            receipt.verify_for_action(&action, "tandem-runtime", Some(&verifying_key()), 500),
            Err(ApprovalReceiptDenial::NotYetValid)
        );
        assert_eq!(
            receipt.verify_for_action(&action, "tandem-runtime", Some(&verifying_key()), 9_000),
            Err(ApprovalReceiptDenial::Expired)
        );
    }

    #[test]
    fn receipt_for_a_different_action_is_rejected() {
        let action = action();
        let receipt = signed_receipt(&action);

        // Same receipt, but the action about to run differs (different args):
        // the recomputed action hash no longer matches.
        let mut other_action = action.clone();
        other_action.args_hash = "tampered-args".to_string();
        assert_eq!(
            receipt.verify_for_action(
                &other_action,
                "tandem-runtime",
                Some(&verifying_key()),
                5_000
            ),
            Err(ApprovalReceiptDenial::ActionHashMismatch)
        );
    }

    #[test]
    fn receipt_cannot_cross_tenant_actor_policy_or_approval() {
        let action = action();
        let receipt = signed_receipt(&action);

        let mut other_tenant = action.clone();
        other_tenant.workspace_id = "workspace-b".to_string();
        assert_eq!(
            receipt.verify_for_action(
                &other_tenant,
                "tandem-runtime",
                Some(&verifying_key()),
                5_000
            ),
            Err(ApprovalReceiptDenial::TenantMismatch)
        );

        let mut other_actor = action.clone();
        other_actor.actor_id = Some("approver-2".to_string());
        assert_eq!(
            receipt.verify_for_action(
                &other_actor,
                "tandem-runtime",
                Some(&verifying_key()),
                5_000
            ),
            Err(ApprovalReceiptDenial::ActorMismatch)
        );

        let mut other_policy = action.clone();
        other_policy.policy_id = "policy-other".to_string();
        assert_eq!(
            receipt.verify_for_action(
                &other_policy,
                "tandem-runtime",
                Some(&verifying_key()),
                5_000
            ),
            Err(ApprovalReceiptDenial::PolicyMismatch)
        );

        let mut other_approval = action.clone();
        other_approval.approval_id = "approval-2".to_string();
        assert_eq!(
            receipt.verify_for_action(
                &other_approval,
                "tandem-runtime",
                Some(&verifying_key()),
                5_000
            ),
            Err(ApprovalReceiptDenial::ApprovalMismatch)
        );
    }

    #[test]
    fn malformed_receipt_fails_closed_before_signature_check() {
        let action = action();
        let mut receipt = signed_receipt(&action);
        receipt.claims.action_hash = String::new();
        assert_eq!(
            receipt.verify_for_action(&action, "tandem-runtime", Some(&verifying_key()), 5_000),
            Err(ApprovalReceiptDenial::Malformed)
        );
    }

    #[test]
    fn single_use_consumption_blocks_replay() {
        let action = action();
        let receipt = signed_receipt(&action);
        let mut guard = InMemoryReplayGuard::new();

        // First use succeeds and consumes the nonce.
        assert_eq!(
            receipt.verify_and_consume(
                &action,
                "tandem-runtime",
                Some(&verifying_key()),
                5_000,
                &mut guard
            ),
            Ok(())
        );
        // Replaying the identical valid receipt within its window is blocked.
        assert_eq!(
            receipt.verify_and_consume(
                &action,
                "tandem-runtime",
                Some(&verifying_key()),
                5_000,
                &mut guard
            ),
            Err(ApprovalReceiptDenial::Replayed)
        );
    }

    #[test]
    fn rejected_receipt_does_not_burn_the_nonce() {
        let action = action();
        let receipt = signed_receipt(&action);
        let mut guard = InMemoryReplayGuard::new();

        // A failed verification (wrong audience) must not consume the nonce,
        // so a later legitimate use still succeeds.
        assert_eq!(
            receipt.verify_and_consume(
                &action,
                "wrong-audience",
                Some(&verifying_key()),
                5_000,
                &mut guard
            ),
            Err(ApprovalReceiptDenial::AudienceMismatch)
        );
        assert_eq!(
            receipt.verify_and_consume(
                &action,
                "tandem-runtime",
                Some(&verifying_key()),
                5_000,
                &mut guard
            ),
            Ok(())
        );
    }

    #[test]
    fn replay_guard_prunes_expired_entries() {
        let mut guard = InMemoryReplayGuard::new();
        assert!(guard.consume("approval-1", "nonce-1", 9_000));
        assert!(!guard.consume("approval-1", "nonce-1", 9_000));
        // After the receipt can no longer be valid, pruning frees the slot;
        // a fresh receipt reusing the id (new nonce in practice) is unaffected.
        guard.prune(9_000);
        assert!(guard.consume("approval-1", "nonce-1", 12_000));
    }
}
