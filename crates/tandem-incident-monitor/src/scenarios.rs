//! Production-mirroring adversarial scenario packs (TAN-487).
//!
//! Scenario packs are versioned, data-driven descriptions of adversarial
//! governance cases (regulatory escalation, prompt injection, excessive agency,
//! cross-tenant leakage, unsafe/unready destinations, …). They are executed in
//! dry-run / sandbox mode against the destination router's route preview,
//! approval gate, and readiness gates — they never mutate external systems.
//!
//! The types here are pure data; the dry-run runner that evaluates a scenario
//! against live config lives in the server crate.

use serde::{Deserialize, Serialize};

/// A versioned collection of adversarial scenarios.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentMonitorScenarioPack {
    pub pack_id: String,
    /// Semver-style pack version so results can be tied to a pack revision.
    pub version: String,
    #[serde(default)]
    pub description: String,
    pub scenarios: Vec<IncidentMonitorScenario>,
}

/// A single adversarial case: a synthetic input plus the control behavior the
/// governance config is expected to produce.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentMonitorScenario {
    pub scenario_id: String,
    /// One of the adversarial categories (regulatory_escalation,
    /// prompt_injection, excessive_agency, cross_tenant, unsafe_destination, …).
    pub category: String,
    #[serde(default)]
    pub description: String,
    pub input: IncidentMonitorScenarioInput,
    pub expect: IncidentMonitorScenarioExpectation,
}

/// Synthetic incident/report signals fed into the router's route preview. All
/// fields are optional so a pack author only sets what a scenario exercises.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IncidentMonitorScenarioInput {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_source_id: Option<String>,
    #[serde(default)]
    pub route_tags: Vec<String>,
    #[serde(default)]
    pub requested_destination_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_destination: Option<String>,
    /// Marks the synthetic case as carrying a prompt-injection / instruction
    /// conflict payload (used for reporting; the router treats it as a
    /// high-risk, approval-worthy signal).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_injection: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// The control behavior a well-governed config is expected to produce for a
/// scenario. Only the assertions a scenario cares about are set; unset
/// assertions are not checked.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IncidentMonitorScenarioExpectation {
    /// The route preview must (not) block the publish.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked: Option<bool>,
    /// The publish must (not) require human approval.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_required: Option<bool>,
    /// A substring that must appear in one of the blocked reasons.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason_contains: Option<String>,
    /// The winning effective destination id, when a scenario asserts routing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_destination_id: Option<String>,
    /// Free-text note describing the control intent, surfaced in results.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The built-in, versioned default scenario pack (embedded JSON, TAN-487).
pub const DEFAULT_SCENARIO_PACK_JSON: &str = include_str!("default_scenario_pack.json");

/// Parse the built-in default scenario pack. Panics only if the embedded JSON
/// is malformed, which a unit test guards against.
pub fn default_scenario_pack() -> IncidentMonitorScenarioPack {
    serde_json::from_str(DEFAULT_SCENARIO_PACK_JSON)
        .expect("embedded default incident-monitor scenario pack must be valid JSON")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_pack_parses_and_is_versioned() {
        let pack = default_scenario_pack();
        assert!(!pack.pack_id.is_empty());
        assert!(!pack.version.is_empty());
        assert!(
            pack.scenarios.len() >= 8,
            "default pack should cover the adversarial categories"
        );
        // Every scenario carries an id, category, and at least one assertion.
        for scenario in &pack.scenarios {
            assert!(!scenario.scenario_id.is_empty());
            assert!(!scenario.category.is_empty());
            let expect = &scenario.expect;
            assert!(
                expect.blocked.is_some()
                    || expect.approval_required.is_some()
                    || expect.reason_contains.is_some()
                    || expect.effective_destination_id.is_some(),
                "scenario {} has no assertions",
                scenario.scenario_id
            );
        }
    }
}
