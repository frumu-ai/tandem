//! Approval gate matrix for risk-tiered actions (CT-20 / TAN-91).
//!
//! Approval gates already exist in the runtime, but deciding *whether* an
//! action needs approval — and *who* may approve it — was implicit and spread
//! across call sites. This module makes that policy declarative: a single
//! [`ApprovalGateMatrix`] maps an action's risk tier and data class to a gate
//! outcome (allow / deny / approval-required), the reviewer eligibility the
//! approval demands, and the TTL to apply to the resulting approval request.
//!
//! The matrix **fails closed**: when an action cannot be classified, it never
//! silently allows — it requires an elevated-reviewer approval instead.

use serde::{Deserialize, Serialize};
use tandem_enterprise_contract::DataClass;

use crate::policy_decision::PolicyDecisionEffect;
use crate::tool::ToolRiskTier;

/// Default approval window for standard gated actions (72 hours).
pub const DEFAULT_APPROVAL_TTL_MS: u64 = 72 * 60 * 60 * 1000;
/// Tighter approval window for elevated/high-risk actions (1 hour).
pub const ELEVATED_APPROVAL_TTL_MS: u64 = 60 * 60 * 1000;

/// Reviewer eligibility a gate demands before an action may proceed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewerEligibility {
    /// No human reviewer required (auto-allowed or hard-denied outcomes).
    None,
    /// Any authorized human reviewer within the tenant may decide.
    AnyHumanReviewer,
    /// A reviewer with elevated authority for the data class / domain
    /// (restricted, credential, financial, executive, regulated).
    ElevatedReviewer,
}

impl ReviewerEligibility {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::AnyHumanReviewer => "any_human_reviewer",
            Self::ElevatedReviewer => "elevated_reviewer",
        }
    }

    pub fn requires_elevated(self) -> bool {
        matches!(self, Self::ElevatedReviewer)
    }
}

/// The action being evaluated against the gate matrix.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_tier: Option<ToolRiskTier>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_class: Option<DataClass>,
    /// True when the action is an external, customer-facing send (it pauses for
    /// approval by default even if no risk tier was classified).
    #[serde(default)]
    pub external_customer_facing: bool,
}

impl GateRequest {
    pub fn new(risk_tier: Option<ToolRiskTier>, data_class: Option<DataClass>) -> Self {
        Self {
            risk_tier,
            data_class,
            external_customer_facing: false,
        }
    }

    pub fn external_customer_send() -> Self {
        Self {
            risk_tier: Some(ToolRiskTier::ExternalSend),
            data_class: None,
            external_customer_facing: true,
        }
    }

    pub fn with_external_customer_facing(mut self, value: bool) -> Self {
        self.external_customer_facing = value;
        self
    }
}

/// The resolved gate outcome for an action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateOutcome {
    pub effect: PolicyDecisionEffect,
    pub reviewer_eligibility: ReviewerEligibility,
    /// TTL to apply to an approval request when `effect` is `ApprovalRequired`.
    pub approval_ttl_ms: u64,
    pub reason_code: String,
    pub reason: String,
}

impl GateOutcome {
    pub fn requires_approval(&self) -> bool {
        matches!(self.effect, PolicyDecisionEffect::ApprovalRequired)
    }

    pub fn is_denied(&self) -> bool {
        matches!(self.effect, PolicyDecisionEffect::Deny)
    }

    pub fn is_allowed(&self) -> bool {
        matches!(self.effect, PolicyDecisionEffect::Allow)
    }
}

/// A declarative matrix that resolves an action's gate outcome from its risk
/// tier and data class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalGateMatrix {
    pub default_approval_ttl_ms: u64,
    pub elevated_approval_ttl_ms: u64,
    /// Risk tiers that are categorically blocked (hard deny, no approval path).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub denied_risk_tiers: Vec<ToolRiskTier>,
    /// Data classes that are categorically blocked (hard deny).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub denied_data_classes: Vec<DataClass>,
}

impl Default for ApprovalGateMatrix {
    fn default() -> Self {
        Self::strict_default()
    }
}

impl ApprovalGateMatrix {
    /// The default strict profile: external sends and high-risk tiers require
    /// approval, sensitive data classes require an elevated reviewer, and
    /// unclassified actions fail closed.
    pub fn strict_default() -> Self {
        Self {
            default_approval_ttl_ms: DEFAULT_APPROVAL_TTL_MS,
            elevated_approval_ttl_ms: ELEVATED_APPROVAL_TTL_MS,
            denied_risk_tiers: Vec::new(),
            denied_data_classes: Vec::new(),
        }
    }

    pub fn with_denied_risk_tiers(mut self, tiers: Vec<ToolRiskTier>) -> Self {
        self.denied_risk_tiers = tiers;
        self
    }

    pub fn with_denied_data_classes(mut self, classes: Vec<DataClass>) -> Self {
        self.denied_data_classes = classes;
        self
    }

    /// A data class that demands an elevated reviewer when gated.
    pub fn data_class_requires_elevated(data_class: DataClass) -> bool {
        matches!(
            data_class,
            DataClass::Restricted
                | DataClass::Credential
                | DataClass::FinancialRecord
                | DataClass::Executive
                | DataClass::Regulated
        )
    }

    /// A risk tier that demands an elevated reviewer when gated.
    pub fn risk_tier_requires_elevated(risk_tier: ToolRiskTier) -> bool {
        matches!(
            risk_tier,
            ToolRiskTier::FinancialRecordAccess
                | ToolRiskTier::CredentialAdmin
                | ToolRiskTier::DestructiveDelete
                | ToolRiskTier::MoneyMovementContract
        )
    }

    fn requires_elevated_reviewer(&self, request: &GateRequest) -> bool {
        request
            .data_class
            .map(Self::data_class_requires_elevated)
            .unwrap_or(false)
            || request
                .risk_tier
                .map(Self::risk_tier_requires_elevated)
                .unwrap_or(false)
    }

    fn ttl_for(&self, elevated: bool) -> u64 {
        if elevated {
            self.elevated_approval_ttl_ms
        } else {
            self.default_approval_ttl_ms
        }
    }

    /// Resolve the gate outcome for `request`.
    ///
    /// Precedence:
    /// 1. categorically denied tier/class → deny;
    /// 2. unclassified action (no tier, no class, not external) → fail closed
    ///    to an elevated-reviewer approval;
    /// 3. external customer-facing send, an approval-by-default risk tier, or a
    ///    sensitive data class → approval required;
    /// 4. otherwise → allow.
    pub fn resolve(&self, request: &GateRequest) -> GateOutcome {
        if let Some(tier) = request.risk_tier {
            if self.denied_risk_tiers.contains(&tier) {
                return GateOutcome {
                    effect: PolicyDecisionEffect::Deny,
                    reviewer_eligibility: ReviewerEligibility::None,
                    approval_ttl_ms: 0,
                    reason_code: "risk_tier_blocked".to_string(),
                    reason: "risk tier is categorically blocked by policy".to_string(),
                };
            }
        }
        if let Some(class) = request.data_class {
            if self.denied_data_classes.contains(&class) {
                return GateOutcome {
                    effect: PolicyDecisionEffect::Deny,
                    reviewer_eligibility: ReviewerEligibility::None,
                    approval_ttl_ms: 0,
                    reason_code: "data_class_blocked".to_string(),
                    reason: "data class is categorically blocked by policy".to_string(),
                };
            }
        }

        let elevated = self.requires_elevated_reviewer(request);

        // Fail closed: a completely unclassified action never auto-allows.
        if request.risk_tier.is_none()
            && request.data_class.is_none()
            && !request.external_customer_facing
        {
            return GateOutcome {
                effect: PolicyDecisionEffect::ApprovalRequired,
                reviewer_eligibility: ReviewerEligibility::ElevatedReviewer,
                approval_ttl_ms: self.elevated_approval_ttl_ms,
                reason_code: "unresolved_policy_fail_closed".to_string(),
                reason: "action could not be classified; requires elevated approval (fail closed)"
                    .to_string(),
            };
        }

        let approval_by_tier = request
            .risk_tier
            .map(ToolRiskTier::approval_required_by_default)
            .unwrap_or(false);

        if request.external_customer_facing || approval_by_tier || elevated {
            let reviewer_eligibility = if elevated {
                ReviewerEligibility::ElevatedReviewer
            } else {
                ReviewerEligibility::AnyHumanReviewer
            };
            let (reason_code, reason) = if request.external_customer_facing {
                (
                    "external_customer_send_requires_approval",
                    "external customer-facing send pauses for approval by default",
                )
            } else if elevated {
                (
                    "sensitive_class_requires_elevated_approval",
                    "sensitive data class or high-risk tier requires elevated reviewer approval",
                )
            } else {
                (
                    "risk_tier_requires_approval",
                    "risk tier requires approval by default",
                )
            };
            return GateOutcome {
                effect: PolicyDecisionEffect::ApprovalRequired,
                reviewer_eligibility,
                approval_ttl_ms: self.ttl_for(elevated),
                reason_code: reason_code.to_string(),
                reason: reason.to_string(),
            };
        }

        GateOutcome {
            effect: PolicyDecisionEffect::Allow,
            reviewer_eligibility: ReviewerEligibility::None,
            approval_ttl_ms: 0,
            reason_code: "low_risk_auto_allow".to_string(),
            reason: "action is low risk and auto-allowed by policy".to_string(),
        }
    }
}

/// Whether an approval may authorize execution right now.
///
/// Enforces that an expired (or undecided / denied) approval can never
/// authorize an action, regardless of its recorded status.
pub fn approval_authorizes_execution(approved: bool, expires_at_ms: u64, now_ms: u64) -> bool {
    approved && now_ms < expires_at_ms
}

#[cfg(test)]
mod tests;
