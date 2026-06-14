//! EAA-14 (TAN-39): provider ACL sync classification + admin-labeled fallback.
//!
//! Connectors pull data from external providers whose native access-control
//! lists (ACLs) vary in fidelity. Some providers expose reliable per-object
//! ACLs Tandem can sync and enforce; others (e.g. Google Drive today) do not,
//! so relying on their ACLs would be unsafe. For the latter, access must be
//! governed by an **explicit admin-labeled source binding** plus the admin
//! access grants that the retrieval layer already enforces.
//!
//! This module provides the fail-closed policy core:
//!
//! - [`provider_acl_sync_mode`] classifies a provider as [`ProviderAclSyncMode::Synced`]
//!   (reliable ACLs), [`ProviderAclSyncMode::AdminLabeled`] (no reliable ACLs —
//!   admin label required), or [`ProviderAclSyncMode::Unsupported`] (unknown —
//!   deny).
//! - [`DataClass::requires_ingestion_review`] flags high-risk data classes that
//!   must be held for human review/quarantine before indexing.
//! - [`evaluate_ingestion_admission`] is the single decision a connector
//!   ingestion path routes through: it returns [`IngestionAdmission::Deny`],
//!   [`IngestionAdmission::Quarantine`], or [`IngestionAdmission::Admit`].

use crate::{ConnectorInstance, DataClass, SourceBinding};

/// How a connector provider's native ACLs are obtained and trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderAclSyncMode {
    /// Provider exposes reliable per-object ACLs that Tandem syncs and enforces.
    /// Indexing may proceed on provider ACLs alone (still subject to review
    /// policy and data-class gating).
    Synced,
    /// Provider ACLs are absent, incomplete, or unsafe to rely on. Access must
    /// be governed by an explicit admin-labeled source binding plus admin
    /// access grants (the "admin-labeled fallback"). A binding with no admin
    /// label is denied ingestion.
    AdminLabeled,
    /// Provider is unknown/unsupported. Fail closed: no ingestion.
    Unsupported,
}

impl ProviderAclSyncMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Synced => "synced",
            Self::AdminLabeled => "admin_labeled",
            Self::Unsupported => "unsupported",
        }
    }
}

/// Classify a connector provider's ACL sync mode.
///
/// Only providers with proven, reliable ACL fidelity are returned as
/// [`ProviderAclSyncMode::Synced`]. `google_drive` is [`ProviderAclSyncMode::AdminLabeled`]
/// (its ACLs are not synced today — see the `not_synced_v1` provider
/// descriptor), and any unknown provider is [`ProviderAclSyncMode::Unsupported`]
/// so new providers fail closed until explicitly classified here.
pub fn provider_acl_sync_mode(provider: &str) -> ProviderAclSyncMode {
    match provider.trim().to_ascii_lowercase().as_str() {
        // No provider currently has proven reliable ACL sync; reliable
        // providers are added here as their sync is implemented and verified.
        "google_drive" | "google-drive" | "googledrive" => ProviderAclSyncMode::AdminLabeled,
        _ => ProviderAclSyncMode::Unsupported,
    }
}

impl DataClass {
    /// Whether ingesting this data class should be held for human review /
    /// quarantine before it is indexed and made retrievable.
    ///
    /// High-risk classes are those whose accidental exposure is regulated or
    /// otherwise materially harmful: secrets ([`DataClass::Credential`]),
    /// regulated data ([`DataClass::Regulated`]), financial records
    /// ([`DataClass::FinancialRecord`]), and the most sensitive internal tier
    /// ([`DataClass::Restricted`]).
    pub fn requires_ingestion_review(self) -> bool {
        matches!(
            self,
            Self::Credential | Self::Regulated | Self::FinancialRecord | Self::Restricted
        )
    }
}

/// Why ingestion of a source binding is denied outright.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestionDenyReason {
    /// The binding and connector do not refer to the same connector/tenant.
    ConnectorMismatch,
    /// The connector is not in an ingestion-allowing lifecycle state.
    ConnectorNotActive,
    /// The source binding is disabled or quarantined.
    BindingNotEnabled,
    /// The binding's ingestion policy disables indexing.
    IndexingDisabled,
    /// The provider is unknown/unsupported, so ingestion fails closed.
    ProviderAclUnsupported,
    /// The provider's ACLs are not synced and the binding carries no admin
    /// label, so there is no trustworthy access basis for indexing.
    AdminLabelRequired,
}

impl IngestionDenyReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConnectorMismatch => "ingestion_connector_mismatch",
            Self::ConnectorNotActive => "ingestion_connector_not_active",
            Self::BindingNotEnabled => "ingestion_binding_not_enabled",
            Self::IndexingDisabled => "ingestion_indexing_disabled",
            Self::ProviderAclUnsupported => "ingestion_provider_acl_unsupported",
            Self::AdminLabelRequired => "ingestion_admin_label_required",
        }
    }
}

/// Why ingestion of a source binding must be held for review before indexing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestionReviewReason {
    /// The binding's ingestion policy explicitly requires review.
    PolicyRequiresReview,
    /// The binding's data class is high-risk and requires review.
    HighRiskDataClass,
}

impl IngestionReviewReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PolicyRequiresReview => "source binding requires ingestion review",
            Self::HighRiskDataClass => "high-risk data class requires ingestion review",
        }
    }
}

/// The admission decision for a source binding's ingestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestionAdmission {
    /// Index and keep.
    Admit,
    /// Index, then hold for admin review (quarantine).
    Quarantine { reason: IngestionReviewReason },
    /// Do not ingest at all.
    Deny { reason: IngestionDenyReason },
}

impl IngestionAdmission {
    /// The deny reason, if this admission is a denial.
    pub fn denied(&self) -> Option<IngestionDenyReason> {
        match self {
            Self::Deny { reason } => Some(*reason),
            _ => None,
        }
    }

    /// Whether the ingested content must be held for review (quarantine).
    pub fn requires_review(&self) -> bool {
        matches!(self, Self::Quarantine { .. })
    }

    /// The review reason, if this admission requires review.
    pub fn review_reason(&self) -> Option<IngestionReviewReason> {
        match self {
            Self::Quarantine { reason } => Some(*reason),
            _ => None,
        }
    }
}

/// Whether a source binding carries an explicit, non-empty admin label.
fn has_admin_label(binding: &SourceBinding) -> bool {
    binding
        .source_root_label
        .as_deref()
        .map(str::trim)
        .is_some_and(|label| !label.is_empty())
}

/// The single fail-closed admission decision a connector ingestion path routes
/// through. Checks, in order: connector/binding identity and lifecycle, the
/// binding's indexing policy, provider ACL trust (admin-label fallback for
/// providers without reliable ACL sync), and finally review gating for
/// policy-flagged or high-risk-data-class bindings.
pub fn evaluate_ingestion_admission(
    binding: &SourceBinding,
    connector: &ConnectorInstance,
    acl_mode: ProviderAclSyncMode,
) -> IngestionAdmission {
    if binding.connector_id != connector.connector_id
        || !connector.tenant_matches(&binding.tenant_context)
    {
        return IngestionAdmission::Deny {
            reason: IngestionDenyReason::ConnectorMismatch,
        };
    }
    if !connector.state.allows_ingestion() {
        return IngestionAdmission::Deny {
            reason: IngestionDenyReason::ConnectorNotActive,
        };
    }
    if !binding.state.allows_ingestion() {
        return IngestionAdmission::Deny {
            reason: IngestionDenyReason::BindingNotEnabled,
        };
    }
    if !binding.ingestion_policy.allow_indexing {
        return IngestionAdmission::Deny {
            reason: IngestionDenyReason::IndexingDisabled,
        };
    }

    match acl_mode {
        ProviderAclSyncMode::Unsupported => {
            return IngestionAdmission::Deny {
                reason: IngestionDenyReason::ProviderAclUnsupported,
            };
        }
        ProviderAclSyncMode::AdminLabeled => {
            if !has_admin_label(binding) {
                return IngestionAdmission::Deny {
                    reason: IngestionDenyReason::AdminLabelRequired,
                };
            }
        }
        ProviderAclSyncMode::Synced => {}
    }

    if binding.ingestion_policy.require_review {
        return IngestionAdmission::Quarantine {
            reason: IngestionReviewReason::PolicyRequiresReview,
        };
    }
    if binding.data_class.requires_ingestion_review() {
        return IngestionAdmission::Quarantine {
            reason: IngestionReviewReason::HighRiskDataClass,
        };
    }

    IngestionAdmission::Admit
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ConnectorLifecycleState, IngestionPolicy, PrincipalRef, ResourceKind, ResourceRef,
        SourceBindingState, TenantContext,
    };

    fn tenant() -> TenantContext {
        TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("dep-a".to_string()),
            "admin-a",
        )
    }

    fn connector() -> ConnectorInstance {
        ConnectorInstance::active(
            "connector-gd",
            tenant(),
            "google_drive",
            PrincipalRef::human_user("admin-a"),
            1_000,
        )
    }

    fn binding(data_class: DataClass) -> SourceBinding {
        let resource = ResourceRef::new(
            "org-a",
            "workspace-a",
            ResourceKind::SharedDrive,
            "drive-folder-1",
        );
        SourceBinding::enabled(
            "binding-1",
            tenant(),
            "connector-gd",
            "google_drive",
            "folder-123",
            resource,
            data_class,
            PrincipalRef::human_user("admin-a"),
            1_000,
        )
    }

    fn labeled_binding(data_class: DataClass) -> SourceBinding {
        let mut b = binding(data_class);
        b.source_root_label = Some("Engineering Shared Drive".to_string());
        b
    }

    // ── provider classification ───────────────────────────────────────────────

    #[test]
    fn google_drive_is_admin_labeled() {
        assert_eq!(
            provider_acl_sync_mode("google_drive"),
            ProviderAclSyncMode::AdminLabeled
        );
        assert_eq!(
            provider_acl_sync_mode("Google-Drive"),
            ProviderAclSyncMode::AdminLabeled
        );
    }

    #[test]
    fn unknown_provider_is_unsupported() {
        assert_eq!(
            provider_acl_sync_mode("notion"),
            ProviderAclSyncMode::Unsupported
        );
        assert_eq!(provider_acl_sync_mode(""), ProviderAclSyncMode::Unsupported);
    }

    // ── high-risk data classes ────────────────────────────────────────────────

    #[test]
    fn high_risk_data_classes_require_review() {
        for dc in [
            DataClass::Credential,
            DataClass::Regulated,
            DataClass::FinancialRecord,
            DataClass::Restricted,
        ] {
            assert!(dc.requires_ingestion_review(), "{dc:?} should be high-risk");
        }
    }

    #[test]
    fn low_risk_data_classes_do_not_require_review() {
        for dc in [
            DataClass::Public,
            DataClass::Internal,
            DataClass::Confidential,
            DataClass::CustomerData,
            DataClass::SourceCode,
        ] {
            assert!(
                !dc.requires_ingestion_review(),
                "{dc:?} should not be high-risk"
            );
        }
    }

    // ── admission: deny paths ─────────────────────────────────────────────────

    #[test]
    fn unsupported_provider_is_denied() {
        let mut connector = connector();
        connector.provider = "dropbox".to_string();
        let admission = evaluate_ingestion_admission(
            &labeled_binding(DataClass::Internal),
            &connector,
            provider_acl_sync_mode(&connector.provider),
        );
        assert_eq!(
            admission.denied(),
            Some(IngestionDenyReason::ProviderAclUnsupported)
        );
    }

    #[test]
    fn admin_labeled_provider_without_label_is_denied() {
        // google_drive (admin-labeled) binding with no source_root_label.
        let admission = evaluate_ingestion_admission(
            &binding(DataClass::Internal),
            &connector(),
            ProviderAclSyncMode::AdminLabeled,
        );
        assert_eq!(
            admission.denied(),
            Some(IngestionDenyReason::AdminLabelRequired)
        );
    }

    #[test]
    fn blank_admin_label_does_not_satisfy_requirement() {
        let mut b = binding(DataClass::Internal);
        b.source_root_label = Some("   ".to_string());
        let admission =
            evaluate_ingestion_admission(&b, &connector(), ProviderAclSyncMode::AdminLabeled);
        assert_eq!(
            admission.denied(),
            Some(IngestionDenyReason::AdminLabelRequired)
        );
    }

    #[test]
    fn disabled_binding_is_denied() {
        let b =
            labeled_binding(DataClass::Internal).with_state(SourceBindingState::Disabled, 2_000);
        let admission =
            evaluate_ingestion_admission(&b, &connector(), ProviderAclSyncMode::AdminLabeled);
        assert_eq!(
            admission.denied(),
            Some(IngestionDenyReason::BindingNotEnabled)
        );
    }

    #[test]
    fn paused_connector_is_denied() {
        let connector = connector().with_state(ConnectorLifecycleState::Paused, 2_000);
        let admission = evaluate_ingestion_admission(
            &labeled_binding(DataClass::Internal),
            &connector,
            ProviderAclSyncMode::AdminLabeled,
        );
        assert_eq!(
            admission.denied(),
            Some(IngestionDenyReason::ConnectorNotActive)
        );
    }

    #[test]
    fn indexing_disabled_policy_is_denied() {
        let b = labeled_binding(DataClass::Internal).with_ingestion_policy(IngestionPolicy {
            allow_indexing: false,
            ..IngestionPolicy::default()
        });
        let admission =
            evaluate_ingestion_admission(&b, &connector(), ProviderAclSyncMode::AdminLabeled);
        assert_eq!(
            admission.denied(),
            Some(IngestionDenyReason::IndexingDisabled)
        );
    }

    #[test]
    fn connector_mismatch_is_denied() {
        let mut b = labeled_binding(DataClass::Internal);
        b.connector_id = "other-connector".to_string();
        let admission =
            evaluate_ingestion_admission(&b, &connector(), ProviderAclSyncMode::AdminLabeled);
        assert_eq!(
            admission.denied(),
            Some(IngestionDenyReason::ConnectorMismatch)
        );
    }

    // ── admission: quarantine paths ───────────────────────────────────────────

    #[test]
    fn labeled_high_risk_binding_is_quarantined() {
        let admission = evaluate_ingestion_admission(
            &labeled_binding(DataClass::Regulated),
            &connector(),
            ProviderAclSyncMode::AdminLabeled,
        );
        assert!(admission.requires_review());
        assert_eq!(
            admission.review_reason(),
            Some(IngestionReviewReason::HighRiskDataClass)
        );
    }

    #[test]
    fn require_review_policy_quarantines_even_for_low_risk() {
        let b = labeled_binding(DataClass::Internal).with_ingestion_policy(IngestionPolicy {
            require_review: true,
            ..IngestionPolicy::default()
        });
        let admission =
            evaluate_ingestion_admission(&b, &connector(), ProviderAclSyncMode::AdminLabeled);
        assert_eq!(
            admission.review_reason(),
            Some(IngestionReviewReason::PolicyRequiresReview)
        );
    }

    // ── admission: admit path ─────────────────────────────────────────────────

    #[test]
    fn labeled_low_risk_binding_is_admitted() {
        let admission = evaluate_ingestion_admission(
            &labeled_binding(DataClass::Internal),
            &connector(),
            ProviderAclSyncMode::AdminLabeled,
        );
        assert_eq!(admission, IngestionAdmission::Admit);
    }

    #[test]
    fn synced_provider_does_not_require_admin_label() {
        // A hypothetical reliable-ACL provider: no admin label needed.
        let admission = evaluate_ingestion_admission(
            &binding(DataClass::Internal),
            &connector(),
            ProviderAclSyncMode::Synced,
        );
        assert_eq!(admission, IngestionAdmission::Admit);
    }

    #[test]
    fn synced_provider_still_quarantines_high_risk() {
        let admission = evaluate_ingestion_admission(
            &binding(DataClass::Credential),
            &connector(),
            ProviderAclSyncMode::Synced,
        );
        assert_eq!(
            admission.review_reason(),
            Some(IngestionReviewReason::HighRiskDataClass)
        );
    }
}
