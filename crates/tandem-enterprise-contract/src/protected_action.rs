//! EAA-06 (TAN-31): action-based protected-mutation classification.
//!
//! The legacy classifier (`tandem-core::classify_fintech_tool`) decides whether
//! a mutation is "protected" from the *tool name alone*. That cannot express
//! "the same tool is safe in one resource scope and protected in another" — a
//! `update_record` call against a `Public` draft is routine, the same call
//! against a `FinancialRecord` system-of-record is a regulated mutation.
//!
//! This module classifies the *normalized action*: tool id, normalized args,
//! the [`ResourceRef`] it touches, its [`ActionEffect`], the resource
//! [`DataClass`], the executing [`PrincipalRef`], and — when the request runs
//! under enterprise authority — the [`StrictTenantContext`] that governs it.
//! Classification composes directly with the approval-receipt machinery
//! (EAA-07): a protected action is only authorized by a signed
//! [`ApprovalReceipt`] bound — by canonical action hash — to the exact
//! [`ProtectedActionPayload`] about to run.
//!
//! Fail-closed contract: a protected action with no governing strict context,
//! no covering grant, an explicit deny, or a missing/invalid approval receipt
//! is denied. There is no allow-on-missing-data path.

use ed25519_dalek::VerifyingKey;
use serde::{Deserialize, Serialize};

use crate::approval_receipt::{canonical_json_string, sha256_hex};
use crate::{
    AccessDecision, AccessPermission, ApprovalReceipt, DataClass, PrincipalRef,
    ProtectedActionPayload, ResourceRef, StrictTenantContext,
};

/// The concrete effect an action has on its target resource. Distinct from the
/// name-based `FintechProtectedActionCategory` in tandem-core: callers map the
/// name heuristic (or a connector-declared capability) onto an effect, and the
/// effect — not the tool name — drives whether the action is protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionEffect {
    /// Reads only; never approval-gated (governed by ordinary access control).
    ReadOnly,
    /// Mutates internal state. Protected only when the data class is sensitive.
    InternalMutation,
    /// Sends a communication outside the organization (email, message, filing
    /// to a counterparty). Always protected.
    ExternalSend,
    /// Moves funds. Always protected.
    MoneyMovement,
    /// Submits a regulatory filing / attestation. Always protected.
    RegulatoryFiling,
    /// Updates a system-of-record / record of truth. Always protected.
    RecordOfTruthUpdate,
    /// Publishes governance evidence / audit packets. Always protected.
    EvidencePublication,
    /// Reads or mints credentials / secrets. Always protected.
    CredentialAccess,
}

impl ActionEffect {
    /// The access permission a principal must hold to perform this effect.
    pub fn required_permission(self) -> AccessPermission {
        match self {
            Self::ReadOnly => AccessPermission::Read,
            Self::InternalMutation => AccessPermission::Edit,
            Self::ExternalSend
            | Self::MoneyMovement
            | Self::RegulatoryFiling
            | Self::RecordOfTruthUpdate
            | Self::EvidencePublication
            | Self::CredentialAccess => AccessPermission::Execute,
        }
    }

    /// Effects that are protected by their nature regardless of data class.
    pub fn is_inherently_protected(self) -> bool {
        !matches!(self, Self::ReadOnly | Self::InternalMutation)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::InternalMutation => "internal_mutation",
            Self::ExternalSend => "external_send",
            Self::MoneyMovement => "money_movement",
            Self::RegulatoryFiling => "regulatory_filing",
            Self::RecordOfTruthUpdate => "record_of_truth_update",
            Self::EvidencePublication => "evidence_publication",
            Self::CredentialAccess => "credential_access",
        }
    }
}

/// A data class is protected when a *mutation* touching it must be approval-gated
/// even if the effect alone (an internal mutation) would otherwise be routine.
pub fn data_class_is_protected(data_class: DataClass) -> bool {
    matches!(
        data_class,
        DataClass::FinancialRecord
            | DataClass::Regulated
            | DataClass::CustomerData
            | DataClass::Credential
            | DataClass::Executive
            | DataClass::Restricted
    )
}

/// The normalized action under evaluation. Everything that distinguishes one
/// protected decision from another — beyond the tool name — lives here, so two
/// invocations of the same tool that differ in args, target resource, tenant,
/// or actor produce a different [`Self::action_fingerprint`] and can classify
/// differently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtectedActionDescriptor {
    pub tool: String,
    pub effect: ActionEffect,
    pub resource: ResourceRef,
    pub data_class: DataClass,
    /// The principal that will execute the mutation.
    pub principal: PrincipalRef,
    /// Normalized tool args. Canonicalized (sorted keys) for hashing.
    pub args: serde_json::Value,
}

impl ProtectedActionDescriptor {
    pub fn new(
        tool: impl Into<String>,
        effect: ActionEffect,
        resource: ResourceRef,
        data_class: DataClass,
        principal: PrincipalRef,
        args: serde_json::Value,
    ) -> Self {
        Self {
            tool: tool.into(),
            effect,
            resource,
            data_class,
            principal,
            args,
        }
    }

    /// Canonical SHA-256 over the *whole* action identity (tool, effect, data
    /// class, resource target, principal, args). Stable across arg/field order;
    /// changes whenever the args, resource (incl. org/workspace tenant), actor,
    /// tool, effect, or data class change. This is the action-level identity
    /// that an approval decision is bound to.
    pub fn action_fingerprint(&self) -> String {
        let value = serde_json::json!({
            "tool": self.tool,
            "effect": self.effect,
            "data_class": self.data_class,
            "resource": self.resource,
            "principal": self.principal,
            "args": self.args,
        });
        sha256_hex(canonical_json_string(&value).as_bytes())
    }

    /// SHA-256 of the normalized args, for the `args_hash` field of a
    /// [`ProtectedActionPayload`].
    pub fn args_hash(&self) -> String {
        sha256_hex(canonical_json_string(&self.args).as_bytes())
    }

    /// SHA-256 of the canonical resource target, for the `resource_target_hash`
    /// field of a [`ProtectedActionPayload`].
    pub fn resource_target_hash(&self) -> String {
        let value = serde_json::to_value(&self.resource).unwrap_or(serde_json::Value::Null);
        sha256_hex(canonical_json_string(&value).as_bytes())
    }
}

/// How a normalized action is classified under enterprise authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionClassification {
    /// Not protected — may proceed without an approval receipt (still subject
    /// to ordinary access control).
    Safe { reason: String },
    /// Protected — execution requires a valid approval receipt bound to the
    /// exact action. `grant_id` is the scope grant that authorized the access.
    RequiresApproval {
        effect: ActionEffect,
        required_permission: AccessPermission,
        grant_id: Option<String>,
        reason: String,
    },
    /// Denied outright (fail-closed): missing governing context, no covering
    /// grant, or an explicit deny.
    Denied { reason: String },
}

impl ActionClassification {
    pub fn requires_approval(&self) -> bool {
        matches!(self, Self::RequiresApproval { .. })
    }

    pub fn is_safe(&self) -> bool {
        matches!(self, Self::Safe { .. })
    }

    pub fn is_denied(&self) -> bool {
        matches!(self, Self::Denied { .. })
    }
}

/// Classify a normalized action.
///
/// `strict_context` is the enterprise authority governing the request, or
/// `None` when the request runs under enterprise authority but no governing
/// context could be resolved. A protected action with `None` context fails
/// closed — callers operating in purely local mode must not route through this
/// classifier (they have no protected-action policy to enforce).
///
/// * Read-only actions and unprotected internal mutations (routine effect on a
///   non-sensitive data class) are [`ActionClassification::Safe`].
/// * Protected actions (inherently protected effect, or any mutation on a
///   protected data class) are evaluated against the strict context. An
///   `Allow` grant yields [`ActionClassification::RequiresApproval`]; a `Deny`
///   or `NotApplicable` (no covering grant) fails closed to
///   [`ActionClassification::Denied`].
pub fn classify_action(
    descriptor: &ProtectedActionDescriptor,
    strict_context: Option<&StrictTenantContext>,
    now_ms: u64,
) -> ActionClassification {
    let effect = descriptor.effect;
    let permission = effect.required_permission();

    if effect == ActionEffect::ReadOnly {
        return ActionClassification::Safe {
            reason: "read_only_action".to_string(),
        };
    }

    let protected =
        effect.is_inherently_protected() || data_class_is_protected(descriptor.data_class);
    if !protected {
        return ActionClassification::Safe {
            reason: "unprotected_mutation".to_string(),
        };
    }

    // Protected action: a governing strict context is mandatory.
    let Some(context) = strict_context else {
        return ActionClassification::Denied {
            reason: "missing_strict_context_for_protected_action".to_string(),
        };
    };

    let evaluation =
        context.evaluate_access(&descriptor.resource, permission, descriptor.data_class, now_ms);
    match evaluation.decision {
        AccessDecision::Deny => ActionClassification::Denied {
            reason: format!("access_denied:{}", evaluation.reason),
        },
        AccessDecision::NotApplicable => ActionClassification::Denied {
            reason: format!("no_grant_for_protected_action:{}", evaluation.reason),
        },
        AccessDecision::Allow => ActionClassification::RequiresApproval {
            effect,
            required_permission: permission,
            grant_id: evaluation.grant_id,
            reason: "protected_action_requires_approval".to_string(),
        },
    }
}

/// The final execution decision for a normalized action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionDecision {
    Allow { reason: String },
    Deny { reason: String },
}

impl ActionDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allow { .. })
    }
}

/// Classify *and* authorize an action in one fail-closed step.
///
/// Composes [`classify_action`] with approval-receipt verification:
/// * `Safe` → [`ActionDecision::Allow`].
/// * `Denied` → [`ActionDecision::Deny`] carrying the classification reason.
/// * `RequiresApproval` → a `receipt` is mandatory; a missing receipt is denied,
///   and a present receipt is verified against `payload` via
///   [`ApprovalReceipt::verify_for_action`]. Any verification failure is denied.
///
/// `payload` is the canonical action the receipt must be bound to — its
/// `action_hash` is recomputed during verification, so a receipt issued for a
/// different action/tenant/actor cannot authorize this one. Callers gating a
/// *non-idempotent* protected mutation should additionally consume the receipt
/// through an [`crate::ApprovalReceiptReplayGuard`].
#[allow(clippy::too_many_arguments)]
pub fn authorize_action(
    descriptor: &ProtectedActionDescriptor,
    strict_context: Option<&StrictTenantContext>,
    payload: &ProtectedActionPayload,
    receipt: Option<&ApprovalReceipt>,
    expected_audience: &str,
    verifying_key: Option<&VerifyingKey>,
    now_ms: u64,
) -> ActionDecision {
    match classify_action(descriptor, strict_context, now_ms) {
        ActionClassification::Safe { reason } => ActionDecision::Allow { reason },
        ActionClassification::Denied { reason } => ActionDecision::Deny { reason },
        ActionClassification::RequiresApproval { .. } => {
            let Some(receipt) = receipt else {
                return ActionDecision::Deny {
                    reason: "protected_action_missing_approval_receipt".to_string(),
                };
            };
            match receipt.verify_for_action(payload, expected_audience, verifying_key, now_ms) {
                Ok(()) => ActionDecision::Allow {
                    reason: "approval_receipt_verified".to_string(),
                },
                Err(denial) => ActionDecision::Deny {
                    reason: denial.as_str().to_string(),
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::approval_receipt::{ApprovalReceiptClaims, ApprovalReceiptHeader};
    use crate::{
        AssertionMetadata, AuthorityChain, GrantSource, PrincipalKind, RequestPrincipal,
        ResourceKind, ResourceScope, ScopedGrant, TenantContext,
    };
    use base64::Engine as _;
    use ed25519_dalek::{Signer, SigningKey};

    fn resource(kind: ResourceKind, id: &str) -> ResourceRef {
        ResourceRef::new("org-a", "workspace-a", kind, id)
    }

    /// A strict context for `user-1` granting Edit+Execute on `record-1`
    /// (a system-of-record) for FinancialRecord data.
    fn strict_context() -> StrictTenantContext {
        strict_context_for(resource(ResourceKind::DataStore, "record-1"))
    }

    fn strict_context_for(allowed: ResourceRef) -> StrictTenantContext {
        let principal = PrincipalRef::human_user("user-1");
        let grant = ScopedGrant::new(
            "grant-1",
            principal.clone(),
            allowed.clone(),
            GrantSource::Direct,
        )
        .with_permissions(vec![
            AccessPermission::Read,
            AccessPermission::Edit,
            AccessPermission::Execute,
        ])
        .with_data_classes(vec![
            DataClass::Public,
            DataClass::Internal,
            DataClass::FinancialRecord,
        ]);
        StrictTenantContext::new(
            TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-1"),
            principal.clone(),
            AuthorityChain::from_request(RequestPrincipal::authenticated_user(
                principal.id, "tandem-web",
            )),
            ResourceScope::root(allowed),
            AssertionMetadata::new("tandem-web", "tandem-runtime", 1_000, 9_999_999_999, "assert-1"),
        )
        .with_grants(vec![grant])
    }

    fn descriptor(
        tool: &str,
        effect: ActionEffect,
        res: ResourceRef,
        data_class: DataClass,
    ) -> ProtectedActionDescriptor {
        ProtectedActionDescriptor::new(
            tool,
            effect,
            res,
            data_class,
            PrincipalRef::agent_worker("agent-1"),
            serde_json::json!({ "amount": 100, "currency": "usd" }),
        )
    }

    #[test]
    fn read_only_action_is_always_safe() {
        let action = descriptor(
            "mcp.bank.read_balance",
            ActionEffect::ReadOnly,
            resource(ResourceKind::DataStore, "record-1"),
            DataClass::FinancialRecord,
        );
        assert!(classify_action(&action, Some(&strict_context()), 5_000).is_safe());
    }

    #[test]
    fn unprotected_mutation_on_low_risk_data_is_safe() {
        // Internal mutation on Public data: routine.
        let action = descriptor(
            "mcp.docs.update_draft",
            ActionEffect::InternalMutation,
            resource(ResourceKind::Document, "doc-1"),
            DataClass::Public,
        );
        assert!(classify_action(&action, Some(&strict_context()), 5_000).is_safe());
    }

    #[test]
    fn same_tool_is_safe_in_one_scope_and_protected_in_another() {
        // The SAME tool + effect, differing only by the resource/data class it
        // targets, classifies differently.
        let public_target = descriptor(
            "mcp.records.update",
            ActionEffect::InternalMutation,
            resource(ResourceKind::Document, "draft-1"),
            DataClass::Public,
        );
        let financial_target = descriptor(
            "mcp.records.update",
            ActionEffect::InternalMutation,
            resource(ResourceKind::DataStore, "record-1"),
            DataClass::FinancialRecord,
        );
        let context = strict_context();
        assert!(classify_action(&public_target, Some(&context), 5_000).is_safe());
        assert!(classify_action(&financial_target, Some(&context), 5_000).requires_approval());
    }

    #[test]
    fn protected_action_without_strict_context_fails_closed() {
        let action = descriptor(
            "mcp.bank.release_funds",
            ActionEffect::MoneyMovement,
            resource(ResourceKind::DataStore, "record-1"),
            DataClass::FinancialRecord,
        );
        assert_eq!(
            classify_action(&action, None, 5_000),
            ActionClassification::Denied {
                reason: "missing_strict_context_for_protected_action".to_string()
            }
        );
    }

    #[test]
    fn protected_action_without_covering_grant_fails_closed() {
        // Money movement against a resource OUTSIDE the granted scope: the
        // strict context yields NotApplicable, which fails closed.
        let action = descriptor(
            "mcp.bank.release_funds",
            ActionEffect::MoneyMovement,
            resource(ResourceKind::DataStore, "unknown-account"),
            DataClass::FinancialRecord,
        );
        let classification = classify_action(&action, Some(&strict_context()), 5_000);
        assert!(classification.is_denied(), "got {classification:?}");
    }

    #[test]
    fn protected_action_denied_when_data_boundary_rejects_class() {
        // The grant covers Execute on record-1 but not Credential data, so a
        // credential-class money movement is denied by the boundary.
        let action = descriptor(
            "mcp.vault.rotate_key",
            ActionEffect::CredentialAccess,
            resource(ResourceKind::DataStore, "record-1"),
            DataClass::Credential,
        );
        let classification = classify_action(&action, Some(&strict_context()), 5_000);
        assert!(classification.is_denied(), "got {classification:?}");
    }

    #[test]
    fn granted_protected_action_requires_approval() {
        let action = descriptor(
            "mcp.bank.release_funds",
            ActionEffect::MoneyMovement,
            resource(ResourceKind::DataStore, "record-1"),
            DataClass::FinancialRecord,
        );
        match classify_action(&action, Some(&strict_context()), 5_000) {
            ActionClassification::RequiresApproval {
                effect,
                required_permission,
                grant_id,
                ..
            } => {
                assert_eq!(effect, ActionEffect::MoneyMovement);
                assert_eq!(required_permission, AccessPermission::Execute);
                assert_eq!(grant_id.as_deref(), Some("grant-1"));
            }
            other => panic!("expected RequiresApproval, got {other:?}"),
        }
    }

    #[test]
    fn expired_strict_context_fails_closed() {
        let action = descriptor(
            "mcp.bank.release_funds",
            ActionEffect::MoneyMovement,
            resource(ResourceKind::DataStore, "record-1"),
            DataClass::FinancialRecord,
        );
        // now past the assertion expiry → evaluate_access denies.
        assert!(classify_action(&action, Some(&strict_context()), 99_999_999_999).is_denied());
    }

    // --- action fingerprint: altered args/resource/tenant/actor change it ---

    #[test]
    fn fingerprint_is_stable_across_arg_order() {
        let mut a = descriptor(
            "mcp.bank.release_funds",
            ActionEffect::MoneyMovement,
            resource(ResourceKind::DataStore, "record-1"),
            DataClass::FinancialRecord,
        );
        a.args = serde_json::json!({ "currency": "usd", "amount": 100 });
        let mut b = a.clone();
        b.args = serde_json::json!({ "amount": 100, "currency": "usd" });
        assert_eq!(a.action_fingerprint(), b.action_fingerprint());
        assert_eq!(a.action_fingerprint().len(), 64);
    }

    #[test]
    fn fingerprint_changes_with_args_resource_tenant_and_actor() {
        let base = descriptor(
            "mcp.bank.release_funds",
            ActionEffect::MoneyMovement,
            resource(ResourceKind::DataStore, "record-1"),
            DataClass::FinancialRecord,
        );
        let base_fp = base.action_fingerprint();

        // Altered args.
        let mut altered_args = base.clone();
        altered_args.args = serde_json::json!({ "amount": 999, "currency": "usd" });
        assert_ne!(altered_args.action_fingerprint(), base_fp);

        // Altered resource target (different resource id).
        let mut altered_resource = base.clone();
        altered_resource.resource = resource(ResourceKind::DataStore, "record-2");
        assert_ne!(altered_resource.action_fingerprint(), base_fp);

        // Altered tenant (different workspace inside the resource).
        let mut altered_tenant = base.clone();
        altered_tenant.resource.workspace_id = "workspace-b".to_string();
        assert_ne!(altered_tenant.action_fingerprint(), base_fp);

        // Altered actor (different executing principal).
        let mut altered_actor = base.clone();
        altered_actor.principal = PrincipalRef::new(PrincipalKind::AgentWorker, "agent-2");
        assert_ne!(altered_actor.action_fingerprint(), base_fp);
    }

    // --- authorize_action: composition with approval receipts ---

    fn signing_key() -> SigningKey {
        SigningKey::from_bytes(&[9u8; 32])
    }

    fn payload_for(action: &ProtectedActionDescriptor) -> ProtectedActionPayload {
        ProtectedActionPayload {
            version: "v1".to_string(),
            org_id: action.resource.organization_id.clone(),
            workspace_id: action.resource.workspace_id.clone(),
            deployment_id: None,
            actor_id: Some("approver-1".to_string()),
            execution_principal: action.principal.clone(),
            session_id: "ses-1".to_string(),
            run_id: Some("run-1".to_string()),
            node_id: Some("node-1".to_string()),
            tool: action.tool.clone(),
            args_hash: action.args_hash(),
            resource_target_hash: action.resource_target_hash(),
            data_class: action.data_class,
            delegation_id: None,
            policy_id: "policy-money-movement".to_string(),
            approval_id: "approval-1".to_string(),
            issued_at_ms: 1_000,
            expires_at_ms: 9_000,
            nonce: "nonce-1".to_string(),
        }
    }

    fn signed_receipt(payload: &ProtectedActionPayload) -> ApprovalReceipt {
        let claims = ApprovalReceiptClaims {
            version: "v1".to_string(),
            audience: "tandem-runtime".to_string(),
            org_id: payload.org_id.clone(),
            workspace_id: payload.workspace_id.clone(),
            deployment_id: payload.deployment_id.clone(),
            actor_id: payload.actor_id.clone(),
            policy_id: payload.policy_id.clone(),
            approval_id: payload.approval_id.clone(),
            action_hash: payload.action_hash(),
            issued_at_ms: 1_000,
            not_before_ms: 1_000,
            expires_at_ms: 9_000,
            issued_by: PrincipalRef::human_user("approver-1"),
        };
        let mut receipt =
            ApprovalReceipt::new(ApprovalReceiptHeader::ed25519("k-1"), claims, String::new());
        let signing_input = receipt.signing_input().expect("signing input");
        receipt.signature = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(signing_key().sign(signing_input.as_bytes()).to_bytes());
        receipt
    }

    #[test]
    fn safe_action_is_allowed_without_a_receipt() {
        let action = descriptor(
            "mcp.docs.update_draft",
            ActionEffect::InternalMutation,
            resource(ResourceKind::Document, "doc-1"),
            DataClass::Public,
        );
        let payload = payload_for(&action);
        let decision = authorize_action(
            &action,
            Some(&strict_context()),
            &payload,
            None,
            "tandem-runtime",
            Some(&signing_key().verifying_key()),
            5_000,
        );
        assert!(decision.is_allowed(), "got {decision:?}");
    }

    #[test]
    fn protected_action_without_receipt_is_denied() {
        let action = descriptor(
            "mcp.bank.release_funds",
            ActionEffect::MoneyMovement,
            resource(ResourceKind::DataStore, "record-1"),
            DataClass::FinancialRecord,
        );
        let payload = payload_for(&action);
        let decision = authorize_action(
            &action,
            Some(&strict_context()),
            &payload,
            None,
            "tandem-runtime",
            Some(&signing_key().verifying_key()),
            5_000,
        );
        assert_eq!(
            decision,
            ActionDecision::Deny {
                reason: "protected_action_missing_approval_receipt".to_string()
            }
        );
    }

    #[test]
    fn protected_action_with_valid_receipt_is_allowed() {
        let action = descriptor(
            "mcp.bank.release_funds",
            ActionEffect::MoneyMovement,
            resource(ResourceKind::DataStore, "record-1"),
            DataClass::FinancialRecord,
        );
        let payload = payload_for(&action);
        let receipt = signed_receipt(&payload);
        let decision = authorize_action(
            &action,
            Some(&strict_context()),
            &payload,
            Some(&receipt),
            "tandem-runtime",
            Some(&signing_key().verifying_key()),
            5_000,
        );
        assert!(decision.is_allowed(), "got {decision:?}");
    }

    #[test]
    fn receipt_for_a_different_action_does_not_authorize() {
        let action = descriptor(
            "mcp.bank.release_funds",
            ActionEffect::MoneyMovement,
            resource(ResourceKind::DataStore, "record-1"),
            DataClass::FinancialRecord,
        );
        let payload = payload_for(&action);
        let receipt = signed_receipt(&payload);

        // The action about to run has different args → different action hash.
        let mut tampered = action.clone();
        tampered.args = serde_json::json!({ "amount": 999_999, "currency": "usd" });
        let tampered_payload = payload_for(&tampered);

        let decision = authorize_action(
            &tampered,
            Some(&strict_context()),
            &tampered_payload,
            Some(&receipt),
            "tandem-runtime",
            Some(&signing_key().verifying_key()),
            5_000,
        );
        assert_eq!(
            decision,
            ActionDecision::Deny {
                reason: "approval_receipt_action_hash_mismatch".to_string()
            }
        );
    }
}
