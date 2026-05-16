use std::collections::{BTreeSet, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    canonical_tool_name, tool_name_matches_profile, ToolCapabilityProfile, ToolEffectLedgerPhase,
    ToolEffectLedgerRecord, ToolEffectLedgerStatus,
};

pub const FINTECH_STRICT_PROFILE: &str = "fintech_strict";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FintechProtectedActionCategory {
    AccountAction,
    CustomerCommunication,
    RegulatoryFiling,
    SystemOfRecordUpdate,
    CreditDecision,
    MoneyMovement,
    EvidencePublication,
}

impl FintechProtectedActionCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AccountAction => "account_action",
            Self::CustomerCommunication => "customer_communication",
            Self::RegulatoryFiling => "regulatory_filing",
            Self::SystemOfRecordUpdate => "system_of_record_update",
            Self::CreditDecision => "credit_decision",
            Self::MoneyMovement => "money_movement",
            Self::EvidencePublication => "evidence_publication",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FintechToolPolicyClassification {
    Safe,
    RequiresApproval(FintechProtectedActionCategory),
    BlockedUnknownMutation,
}

impl FintechToolPolicyClassification {
    pub fn allowed_without_approval(&self) -> bool {
        matches!(self, Self::Safe)
    }

    pub fn category(&self) -> Option<FintechProtectedActionCategory> {
        match self {
            Self::RequiresApproval(category) => Some(*category),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FintechToolPolicyDecision {
    pub allowed: bool,
    pub classification: FintechToolPolicyClassification,
    pub reason: Option<String>,
}

pub fn metadata_enables_fintech_strict(metadata: Option<&Value>) -> bool {
    let Some(metadata) = metadata else {
        return false;
    };
    if metadata
        .get("fintech_strict")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return true;
    }
    if metadata
        .pointer("/fintech/strict")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return true;
    }
    [
        "runtime_profile",
        "domain_profile",
        "fintech_profile",
        "profile",
        "domain",
    ]
    .iter()
    .any(|key| metadata_string_matches(metadata.get(*key)))
}

fn metadata_string_matches(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(raw)) => {
            let normalized = normalize_marker(raw);
            matches!(
                normalized.as_str(),
                FINTECH_STRICT_PROFILE | "fintech" | "regulated_finance"
            )
        }
        Some(Value::Array(values)) => values
            .iter()
            .any(|value| metadata_string_matches(Some(value))),
        _ => false,
    }
}

pub fn classify_fintech_tool(tool_name: &str) -> FintechToolPolicyClassification {
    let action = fintech_action_name(tool_name);
    let tokens = tokens(&action);
    let compact = compact(&action);
    let has = |needle: &str| tokens.iter().any(|token| token == needle);
    let has_any = |needles: &[&str]| needles.iter().any(|needle| has(needle));

    if has_any(&["sar", "regulator", "regulatory", "filing", "attestation"])
        || (has("submit") && has_any(&["response", "report"]))
    {
        return FintechToolPolicyClassification::RequiresApproval(
            FintechProtectedActionCategory::RegulatoryFiling,
        );
    }

    if has_any(&[
        "payment",
        "payout",
        "funds",
        "fund",
        "transaction",
        "ledger",
        "ach",
        "wire",
    ]) || has_any(&["transfer", "reverse", "refund"])
    {
        return FintechToolPolicyClassification::RequiresApproval(
            FintechProtectedActionCategory::MoneyMovement,
        );
    }

    if has("credit") || has("underwriting") || compact.contains("creditlimit") {
        return FintechToolPolicyClassification::RequiresApproval(
            FintechProtectedActionCategory::CreditDecision,
        );
    }

    if has_any(&[
        "freeze", "unfreeze", "close", "restrict", "unlock", "disable",
    ]) && has_any(&["account", "card", "customer"])
    {
        return FintechToolPolicyClassification::RequiresApproval(
            FintechProtectedActionCategory::AccountAction,
        );
    }

    if (has_any(&["send", "deliver", "post", "publish"]) || compact.contains("sendemail"))
        && has_any(&[
            "customer",
            "notice",
            "adverse",
            "incident",
            "compliance",
            "email",
            "mail",
            "gmail",
            "outlook",
        ])
    {
        return FintechToolPolicyClassification::RequiresApproval(
            FintechProtectedActionCategory::CustomerCommunication,
        );
    }

    if has_any(&["publish", "finalize", "send", "mark"])
        && has_any(&["evidence", "audit", "packet", "attestation"])
    {
        return FintechToolPolicyClassification::RequiresApproval(
            FintechProtectedActionCategory::EvidencePublication,
        );
    }

    if (has_any(&["update", "close", "mark", "resolve", "set", "write"])
        && has_any(&[
            "record",
            "case",
            "control",
            "exception",
            "vendor",
            "risk",
            "rating",
            "status",
        ]))
        || compact.contains("riskrating")
    {
        return FintechToolPolicyClassification::RequiresApproval(
            FintechProtectedActionCategory::SystemOfRecordUpdate,
        );
    }

    if tool_name_matches_profile(tool_name, ToolCapabilityProfile::ExternalMutation) {
        return FintechToolPolicyClassification::BlockedUnknownMutation;
    }

    FintechToolPolicyClassification::Safe
}

pub fn fintech_strict_tool_decision(tool_name: &str) -> FintechToolPolicyDecision {
    let classification = classify_fintech_tool(tool_name);
    match classification {
        FintechToolPolicyClassification::Safe => FintechToolPolicyDecision {
            allowed: true,
            classification,
            reason: None,
        },
        FintechToolPolicyClassification::RequiresApproval(category) => FintechToolPolicyDecision {
            allowed: false,
            classification: FintechToolPolicyClassification::RequiresApproval(category),
            reason: Some(format!(
                "fintech strict mode requires protected approval for `{}` before tool `{}` can execute",
                category.as_str(),
                canonical_tool_name(tool_name)
            )),
        },
        FintechToolPolicyClassification::BlockedUnknownMutation => FintechToolPolicyDecision {
            allowed: false,
            classification,
            reason: Some(format!(
                "fintech strict mode blocked unknown external mutation tool `{}` until it is classified",
                canonical_tool_name(tool_name)
            )),
        },
    }
}

fn fintech_action_name(tool_name: &str) -> String {
    let normalized = tool_name.trim().to_ascii_lowercase().replace('-', "_");
    normalized
        .strip_prefix("mcp.")
        .and_then(|rest| rest.rsplit('.').next())
        .unwrap_or(normalized.as_str())
        .to_string()
}

fn normalize_marker(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace('-', "_")
}

fn tokens(value: &str) -> Vec<String> {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn compact(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FintechConnectorProofRecord {
    pub tool: String,
    pub source_ids: Vec<String>,
}

impl FintechConnectorProofRecord {
    pub fn new(tool: impl Into<String>, source_ids: Vec<String>) -> Self {
        let mut source_ids = source_ids
            .into_iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>();
        source_ids.sort();
        source_ids.dedup();
        Self {
            tool: tool.into(),
            source_ids,
        }
    }
}

pub fn connector_proof_from_tool_record(
    record: &ToolEffectLedgerRecord,
) -> Option<FintechConnectorProofRecord> {
    if record.phase != ToolEffectLedgerPhase::Outcome
        || record.status != ToolEffectLedgerStatus::Succeeded
    {
        return None;
    }
    let normalized = canonical_tool_name(&record.tool);
    if !normalized.starts_with("mcp.")
        && !matches!(
            normalized.as_str(),
            "webfetch" | "webfetch_html" | "websearch"
        )
    {
        return None;
    }
    let action = fintech_action_name(&normalized);
    let action_tokens = tokens(&action);
    if action_tokens
        .iter()
        .any(|token| matches!(token.as_str(), "discover" | "list" | "catalog" | "tools"))
    {
        return None;
    }

    let mut source_ids = BTreeSet::new();
    for key in [
        "url",
        "path",
        "source_id",
        "document_id",
        "ticket_id",
        "record_id",
    ] {
        if let Some(value) = record.args_summary.get(key).and_then(Value::as_str) {
            let value = value.trim();
            if !value.is_empty() {
                source_ids.insert(value.to_string());
            }
        }
    }
    if source_ids.is_empty() {
        return None;
    }
    Some(FintechConnectorProofRecord::new(
        normalized,
        source_ids.into_iter().collect(),
    ))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FintechArtifactValidationReport {
    pub passed: bool,
    pub issues: Vec<String>,
}

pub fn validate_fintech_compliance_brief_artifact(
    artifact: &Value,
    connector_proof: &[FintechConnectorProofRecord],
) -> FintechArtifactValidationReport {
    let mut issues = Vec::new();
    for field in [
        "run_id",
        "tenant",
        "source_scope",
        "sources_reviewed",
        "material_claims",
        "citations",
        "limitations",
        "reviewer_status",
        "approval_state",
        "audit_event_ids",
    ] {
        if value_is_empty(artifact.get(field)) {
            issues.push(format!("missing_required_field:{field}"));
        }
    }

    let proof_ids = connector_proof
        .iter()
        .flat_map(|record| record.source_ids.iter().cloned())
        .collect::<HashSet<_>>();

    if let Some(claims) = artifact.get("material_claims").and_then(Value::as_array) {
        for (idx, claim) in claims.iter().enumerate() {
            let citation = first_string(claim, &["citation_id", "source_id", "url"]);
            let limitation = first_string(claim, &["limitation"]);
            if citation.is_none() && limitation.is_none() {
                issues.push(format!(
                    "material_claim_without_citation_or_limitation:{idx}"
                ));
            }
        }
    }

    if let Some(citations) = artifact.get("citations").and_then(Value::as_array) {
        for (idx, citation) in citations.iter().enumerate() {
            let source_id = first_string(citation, &["source_id", "url", "id"]);
            match source_id {
                Some(source_id) if proof_ids.contains(&source_id) => {}
                Some(source_id) => issues.push(format!(
                    "citation_without_connector_proof:{idx}:{source_id}"
                )),
                None => issues.push(format!("citation_missing_source_id:{idx}")),
            }
        }
    }

    FintechArtifactValidationReport {
        passed: issues.is_empty(),
        issues,
    }
}

fn value_is_empty(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::String(value)) => value.trim().is_empty(),
        Some(Value::Array(values)) => values.is_empty(),
        Some(Value::Object(values)) => values.is_empty(),
        Some(_) => false,
    }
}

fn first_string(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| value.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .next()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FintechAuditPackage {
    pub run_id: String,
    pub tenant: Value,
    pub actor: Option<String>,
    pub tool_calls: Vec<ToolEffectLedgerRecord>,
    pub connector_proof: Vec<FintechConnectorProofRecord>,
    pub artifacts: Vec<Value>,
    pub approvals: Vec<Value>,
    pub policy_decisions: Vec<Value>,
    pub limitations: Vec<String>,
}

pub fn build_fintech_audit_package(
    run_id: impl Into<String>,
    tenant: Value,
    actor: Option<String>,
    tool_calls: Vec<ToolEffectLedgerRecord>,
    artifacts: Vec<Value>,
    approvals: Vec<Value>,
    policy_decisions: Vec<Value>,
    limitations: Vec<String>,
) -> FintechAuditPackage {
    let connector_proof = tool_calls
        .iter()
        .filter_map(connector_proof_from_tool_record)
        .collect();
    FintechAuditPackage {
        run_id: run_id.into(),
        tenant,
        actor,
        tool_calls,
        connector_proof,
        artifacts,
        approvals,
        policy_decisions,
        limitations,
    }
}

pub fn fintech_policy_decision_payload(
    tool: &str,
    classification: &FintechToolPolicyClassification,
    reason: &str,
) -> Value {
    json!({
        "runtime_profile": FINTECH_STRICT_PROFILE,
        "tool": canonical_tool_name(tool),
        "classification": match classification {
            FintechToolPolicyClassification::Safe => "safe",
            FintechToolPolicyClassification::RequiresApproval(_) => "requires_approval",
            FintechToolPolicyClassification::BlockedUnknownMutation => "blocked_unknown_mutation",
        },
        "category": classification.category().map(FintechProtectedActionCategory::as_str),
        "reason": reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_tool_effect_ledger_record, ToolEffectLedgerPhase, ToolEffectLedgerStatus};

    #[test]
    fn classifier_categorizes_protected_fintech_actions() {
        assert_eq!(
            classify_fintech_tool("mcp.bank.freeze_account"),
            FintechToolPolicyClassification::RequiresApproval(
                FintechProtectedActionCategory::AccountAction
            )
        );
        assert_eq!(
            classify_fintech_tool("mcp.gmail.gmail_send_email"),
            FintechToolPolicyClassification::RequiresApproval(
                FintechProtectedActionCategory::CustomerCommunication
            )
        );
        assert_eq!(
            classify_fintech_tool("file_sar_report"),
            FintechToolPolicyClassification::RequiresApproval(
                FintechProtectedActionCategory::RegulatoryFiling
            )
        );
        assert_eq!(
            classify_fintech_tool("update_customer_risk_rating"),
            FintechToolPolicyClassification::RequiresApproval(
                FintechProtectedActionCategory::SystemOfRecordUpdate
            )
        );
        assert_eq!(
            classify_fintech_tool("approve_credit_limit"),
            FintechToolPolicyClassification::RequiresApproval(
                FintechProtectedActionCategory::CreditDecision
            )
        );
        assert_eq!(
            classify_fintech_tool("release_funds"),
            FintechToolPolicyClassification::RequiresApproval(
                FintechProtectedActionCategory::MoneyMovement
            )
        );
        assert_eq!(
            classify_fintech_tool("publish_audit_packet"),
            FintechToolPolicyClassification::RequiresApproval(
                FintechProtectedActionCategory::EvidencePublication
            )
        );
    }

    #[test]
    fn classifier_allows_research_and_blocks_unknown_mutation() {
        assert_eq!(
            classify_fintech_tool("mcp.regulator.fetch_bulletin"),
            FintechToolPolicyClassification::Safe
        );
        assert_eq!(
            classify_fintech_tool("mcp.vendor.update_widget"),
            FintechToolPolicyClassification::BlockedUnknownMutation
        );
    }

    #[test]
    fn metadata_marker_enables_fintech_strict() {
        assert!(metadata_enables_fintech_strict(Some(&json!({
            "runtime_profile": "fintech_strict"
        }))));
        assert!(metadata_enables_fintech_strict(Some(&json!({
            "fintech": { "strict": true }
        }))));
        assert!(!metadata_enables_fintech_strict(Some(&json!({
            "runtime_profile": "default"
        }))));
    }

    #[test]
    fn connector_discovery_without_source_retrieval_is_not_proof() {
        let discovery = build_tool_effect_ledger_record(
            "session-1",
            "message-1",
            Some("call-1"),
            "mcp.regulator.list_tools",
            ToolEffectLedgerPhase::Outcome,
            ToolEffectLedgerStatus::Succeeded,
            &json!({"query": "rules"}),
            None,
            Some("available tools"),
            None,
        );
        assert!(connector_proof_from_tool_record(&discovery).is_none());

        let fetch = build_tool_effect_ledger_record(
            "session-1",
            "message-1",
            Some("call-2"),
            "mcp.regulator.fetch_bulletin",
            ToolEffectLedgerPhase::Outcome,
            ToolEffectLedgerStatus::Succeeded,
            &json!({"url": "https://regulator.example/rule-1"}),
            None,
            Some("rule text"),
            None,
        );
        let proof = connector_proof_from_tool_record(&fetch).expect("proof");
        assert_eq!(proof.source_ids, vec!["https://regulator.example/rule-1"]);
    }

    #[test]
    fn compliance_artifact_requires_citations_to_map_to_proof() {
        let artifact = json!({
            "run_id": "run-1",
            "tenant": {"org_id": "local"},
            "source_scope": ["regulator"],
            "sources_reviewed": ["https://regulator.example/rule-1"],
            "material_claims": [
                {"claim": "Rule changed", "source_id": "https://regulator.example/rule-1"}
            ],
            "citations": [
                {"source_id": "https://regulator.example/rule-1"}
            ],
            "limitations": ["Not legal advice"],
            "reviewer_status": "needs_review",
            "approval_state": {"required": false},
            "audit_event_ids": ["audit-1"]
        });
        let report = validate_fintech_compliance_brief_artifact(
            &artifact,
            &[FintechConnectorProofRecord::new(
                "mcp.regulator.fetch_bulletin",
                vec!["https://regulator.example/rule-1".to_string()],
            )],
        );
        assert!(report.passed, "{:?}", report.issues);

        let failed = validate_fintech_compliance_brief_artifact(&artifact, &[]);
        assert!(!failed.passed);
        assert!(failed
            .issues
            .iter()
            .any(|issue| issue.starts_with("citation_without_connector_proof")));
    }
}
