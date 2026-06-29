use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SignalTriageDomain {
    ResearchEvidence,
    UseCaseDiscovery,
}

impl Default for SignalTriageDomain {
    fn default() -> Self {
        Self::ResearchEvidence
    }
}

impl SignalTriageDomain {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ResearchEvidence => "research_evidence",
            Self::UseCaseDiscovery => "use_case_discovery",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SignalTriageMemoryPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default)]
    pub review_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_profile: Option<String>,
    #[serde(default)]
    pub source_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SignalTriageSignal {
    #[serde(default)]
    pub domain: SignalTriageDomain,
    pub signal_id: String,
    pub source: String,
    pub title: String,
    pub summary: String,
    pub confidence: String,
    pub risk: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_of: Option<String>,
    #[serde(default)]
    pub review_required: bool,
    #[serde(default)]
    pub research_claims: Vec<String>,
    #[serde(default)]
    pub research_sources: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_problem: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub candidate_use_case: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_value: Option<String>,
    #[serde(default)]
    pub risks: Vec<String>,
    #[serde(default = "default_rollout_disabled")]
    pub rollout_disabled: bool,
    #[serde(default)]
    pub auto_enable_workflow: bool,
    #[serde(default)]
    pub memory_policy: SignalTriageMemoryPolicy,
}

fn default_rollout_disabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SignalTriageGateResult {
    pub key: String,
    pub label: String,
    pub passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SignalTriageGateReport {
    pub stage: String,
    pub status: String,
    pub passed: bool,
    pub passed_count: usize,
    pub total_count: usize,
    #[serde(default)]
    pub gates: Vec<SignalTriageGateResult>,
    #[serde(default)]
    pub missing: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SignalTriageMemoryDisposition {
    pub allowed: bool,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_profile: Option<String>,
    #[serde(default)]
    pub source_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SignalTriageReviewedPayload {
    ResearchEvidence {
        claims: Vec<String>,
        sources: Vec<String>,
        recommendation: String,
        uncertainty: String,
    },
    UseCaseDiscovery {
        problem: String,
        candidate_use_case: String,
        expected_value: String,
        risks: Vec<String>,
        rollout_state: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SignalTriageReviewedArtifact {
    pub artifact_type: String,
    pub status: String,
    pub domain: SignalTriageDomain,
    pub title: String,
    pub confidence: String,
    pub risk: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub memory_write: SignalTriageMemoryDisposition,
    pub payload: SignalTriageReviewedPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SignalTriageOutcome {
    pub status: String,
    pub domain: SignalTriageDomain,
    pub gate: SignalTriageGateReport,
    #[serde(default)]
    pub blocked_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<SignalTriageReviewedArtifact>,
}

pub fn evaluate_signal_triage_gate(signal: &SignalTriageSignal) -> SignalTriageGateReport {
    let mut gates = vec![
        gate(
            "source_known",
            "Source known",
            non_empty(&signal.source),
            Some(signal.source.clone()),
        ),
        gate(
            "signal_described",
            "Signal title and summary present",
            non_empty(&signal.title) && non_empty(&signal.summary),
            first_non_empty([&signal.title, &signal.summary]),
        ),
        gate(
            "evidence_present",
            "Evidence refs present",
            !signal.evidence_refs.is_empty(),
            signal.evidence_refs.first().cloned(),
        ),
        gate(
            "confidence_sufficient",
            "Confidence is not low or speculative",
            confidence_is_sufficient(&signal.confidence),
            Some(signal.confidence.clone()),
        ),
        gate(
            "not_duplicate",
            "Signal is not a duplicate",
            signal
                .duplicate_of
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty(),
            signal.duplicate_of.clone(),
        ),
        gate(
            "review_required",
            "Human review is required before promotion",
            signal.review_required,
            Some(format!("review_required={}", signal.review_required)),
        ),
        gate(
            "memory_governed",
            "Memory learning is opt-in, scoped, reviewable, and retained",
            memory_policy_is_governed(&signal.memory_policy),
            memory_policy_detail(&signal.memory_policy),
        ),
    ];

    match signal.domain {
        SignalTriageDomain::ResearchEvidence => {
            gates.extend([
                gate(
                    "research_claims_present",
                    "Research claims present",
                    !signal.research_claims.is_empty(),
                    signal.research_claims.first().cloned(),
                ),
                gate(
                    "research_sources_present",
                    "Research sources present",
                    !signal.research_sources.is_empty(),
                    signal.research_sources.first().cloned(),
                ),
                gate(
                    "recommendation_present",
                    "Recommendation present",
                    option_non_empty(&signal.recommendation),
                    signal.recommendation.clone(),
                ),
            ]);
        }
        SignalTriageDomain::UseCaseDiscovery => {
            gates.extend([
                gate(
                    "problem_present",
                    "Observed problem present",
                    option_non_empty(&signal.observed_problem),
                    signal.observed_problem.clone(),
                ),
                gate(
                    "candidate_use_case_present",
                    "Candidate use case present",
                    option_non_empty(&signal.candidate_use_case),
                    signal.candidate_use_case.clone(),
                ),
                gate(
                    "expected_value_present",
                    "Expected value present",
                    option_non_empty(&signal.expected_value),
                    signal.expected_value.clone(),
                ),
                gate(
                    "risks_reviewed",
                    "Risks reviewed",
                    !signal.risks.is_empty(),
                    signal.risks.first().cloned(),
                ),
                gate(
                    "workflow_disabled",
                    "Candidate workflow remains disabled by default",
                    signal.rollout_disabled && !signal.auto_enable_workflow,
                    Some(format!(
                        "rollout_disabled={}, auto_enable_workflow={}",
                        signal.rollout_disabled, signal.auto_enable_workflow
                    )),
                ),
            ]);
        }
    }

    let passed_count = gates.iter().filter(|gate| gate.passed).count();
    let missing = gates
        .iter()
        .filter(|gate| !gate.passed)
        .map(|gate| gate.key.clone())
        .collect::<Vec<_>>();
    let passed = passed_count == gates.len();
    SignalTriageGateReport {
        stage: "signal_intake_to_reviewed_proposal".to_string(),
        status: if passed { "passed" } else { "blocked" }.to_string(),
        passed,
        passed_count,
        total_count: gates.len(),
        blocked_reason: if passed {
            None
        } else {
            Some(format!("missing quality gates: {}", missing.join(", ")))
        },
        gates,
        missing,
    }
}

pub fn triage_signal(signal: &SignalTriageSignal) -> SignalTriageOutcome {
    let gate = evaluate_signal_triage_gate(signal);
    if !gate.passed {
        return SignalTriageOutcome {
            status: "blocked".to_string(),
            domain: signal.domain,
            blocked_reasons: gate
                .gates
                .iter()
                .filter(|row| !row.passed)
                .map(|row| row.label.clone())
                .collect(),
            gate,
            artifact: None,
        };
    }

    let artifact = match signal.domain {
        SignalTriageDomain::ResearchEvidence => SignalTriageReviewedArtifact {
            artifact_type: "research_evidence_brief".to_string(),
            status: "review_required".to_string(),
            domain: signal.domain,
            title: signal.title.clone(),
            confidence: signal.confidence.clone(),
            risk: signal.risk.clone(),
            evidence_refs: signal.evidence_refs.clone(),
            memory_write: memory_disposition(&signal.memory_policy),
            payload: SignalTriageReviewedPayload::ResearchEvidence {
                claims: signal.research_claims.clone(),
                sources: signal.research_sources.clone(),
                recommendation: signal.recommendation.clone().unwrap_or_default(),
                uncertainty: format!(
                    "confidence={}, risk={}; human review required before action",
                    signal.confidence, signal.risk
                ),
            },
        },
        SignalTriageDomain::UseCaseDiscovery => SignalTriageReviewedArtifact {
            artifact_type: "use_case_discovery_proposal".to_string(),
            status: "review_required".to_string(),
            domain: signal.domain,
            title: signal.title.clone(),
            confidence: signal.confidence.clone(),
            risk: signal.risk.clone(),
            evidence_refs: signal.evidence_refs.clone(),
            memory_write: memory_disposition(&signal.memory_policy),
            payload: SignalTriageReviewedPayload::UseCaseDiscovery {
                problem: signal.observed_problem.clone().unwrap_or_default(),
                candidate_use_case: signal.candidate_use_case.clone().unwrap_or_default(),
                expected_value: signal.expected_value.clone().unwrap_or_default(),
                risks: signal.risks.clone(),
                rollout_state: "disabled_pending_review".to_string(),
            },
        },
    };

    SignalTriageOutcome {
        status: "review_required".to_string(),
        domain: signal.domain,
        gate,
        blocked_reasons: Vec::new(),
        artifact: Some(artifact),
    }
}

fn gate(key: &str, label: &str, passed: bool, detail: Option<String>) -> SignalTriageGateResult {
    SignalTriageGateResult {
        key: key.to_string(),
        label: label.to_string(),
        passed,
        detail: detail.filter(|value| !value.trim().is_empty()),
    }
}

fn non_empty(value: &str) -> bool {
    !value.trim().is_empty()
}

fn option_non_empty(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(non_empty)
}

fn first_non_empty(values: [&str; 2]) -> Option<String> {
    values
        .into_iter()
        .find(|value| non_empty(value))
        .map(ToString::to_string)
}

fn confidence_is_sufficient(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "medium" | "high" | "verified" | "confirmed"
    )
}

fn memory_policy_is_governed(policy: &SignalTriageMemoryPolicy) -> bool {
    if !policy.enabled {
        return true;
    }
    option_non_empty(&policy.scope)
        && policy.review_required
        && option_non_empty(&policy.retention_profile)
        && !policy.source_refs.is_empty()
}

fn memory_policy_detail(policy: &SignalTriageMemoryPolicy) -> Option<String> {
    if !policy.enabled {
        return Some("memory learning disabled".to_string());
    }
    Some(format!(
        "enabled=true, scope={}, review_required={}, retention_profile={}, source_refs={}",
        policy.scope.as_deref().unwrap_or(""),
        policy.review_required,
        policy.retention_profile.as_deref().unwrap_or(""),
        policy.source_refs.len()
    ))
}

fn memory_disposition(policy: &SignalTriageMemoryPolicy) -> SignalTriageMemoryDisposition {
    if !policy.enabled {
        return SignalTriageMemoryDisposition {
            allowed: false,
            reason: "memory learning disabled".to_string(),
            scope: None,
            retention_profile: None,
            source_refs: Vec::new(),
        };
    }
    SignalTriageMemoryDisposition {
        allowed: true,
        reason: "memory learning explicitly opted in and passed governance".to_string(),
        scope: policy.scope.clone(),
        retention_profile: policy.retention_profile.clone(),
        source_refs: policy.source_refs.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn research_fixture() -> SignalTriageSignal {
        SignalTriageSignal {
            domain: SignalTriageDomain::ResearchEvidence,
            signal_id: "research-signal-1".to_string(),
            source: "research-synthesis-pack".to_string(),
            title: "Refresh privacy-first positioning evidence".to_string(),
            summary: "New source material changes the confidence of a positioning claim."
                .to_string(),
            confidence: "high".to_string(),
            risk: "medium".to_string(),
            evidence_refs: vec![
                "agent-templates/pack-docs/research-synthesis-pack/START_HERE.md".to_string(),
                "agent-templates/packs/research-synthesis-pack/inputs/references.md".to_string(),
            ],
            review_required: true,
            research_claims: vec![
                "Privacy-first local agent workflows reduce regulated data egress.".to_string(),
            ],
            research_sources: vec![
                "research-synthesis-pack references.md".to_string(),
                "research-synthesis-pack methodology.md".to_string(),
            ],
            recommendation: Some(
                "Keep the claim, but attach source confidence and uncertainty in the brief."
                    .to_string(),
            ),
            ..SignalTriageSignal::default()
        }
    }

    fn use_case_fixture() -> SignalTriageSignal {
        SignalTriageSignal {
            domain: SignalTriageDomain::UseCaseDiscovery,
            signal_id: "use-case-signal-1".to_string(),
            source: "competitor-research-pipeline".to_string(),
            title: "Repeated competitor-watch summaries need review before automation".to_string(),
            summary: "Operators repeatedly ask for the same competitor-change digest.".to_string(),
            confidence: "medium".to_string(),
            risk: "medium".to_string(),
            evidence_refs: vec![
                "packages/tandem-control-panel/src/features/studio/templates/competitor-research-pipeline.yaml".to_string(),
            ],
            review_required: true,
            observed_problem: Some(
                "Competitor signal scans are repeated manually before roadmap reviews.".to_string(),
            ),
            candidate_use_case: Some(
                "Create a reviewed competitor-watch proposal for an operator to approve."
                    .to_string(),
            ),
            expected_value: Some(
                "Reduce repeated setup while keeping final workflow launch manual.".to_string(),
            ),
            risks: vec![
                "Source freshness can decay.".to_string(),
                "Market signal correlation can be mistaken for causality.".to_string(),
            ],
            rollout_disabled: true,
            auto_enable_workflow: false,
            memory_policy: SignalTriageMemoryPolicy {
                enabled: true,
                scope: Some("project:signal-triage".to_string()),
                review_required: true,
                retention_profile: Some("proposal-evidence".to_string()),
                source_refs: vec!["competitor-research-pipeline.yaml".to_string()],
            },
            ..SignalTriageSignal::default()
        }
    }

    #[test]
    fn research_evidence_fixture_produces_reviewed_brief() {
        let outcome = triage_signal(&research_fixture());

        assert_eq!(outcome.status, "review_required");
        assert!(outcome.gate.passed);
        let artifact = outcome.artifact.expect("reviewed artifact");
        assert_eq!(artifact.artifact_type, "research_evidence_brief");
        assert_eq!(artifact.status, "review_required");
        assert!(!artifact.memory_write.allowed);
        match artifact.payload {
            SignalTriageReviewedPayload::ResearchEvidence {
                claims,
                sources,
                recommendation,
                uncertainty,
            } => {
                assert_eq!(claims.len(), 1);
                assert_eq!(sources.len(), 2);
                assert!(recommendation.contains("source confidence"));
                assert!(uncertainty.contains("human review required"));
            }
            _ => panic!("expected research evidence payload"),
        }
    }

    #[test]
    fn use_case_fixture_produces_disabled_reviewed_proposal() {
        let outcome = triage_signal(&use_case_fixture());

        assert_eq!(outcome.status, "review_required");
        assert!(outcome.gate.passed);
        let artifact = outcome.artifact.expect("reviewed artifact");
        assert_eq!(artifact.artifact_type, "use_case_discovery_proposal");
        assert!(artifact.memory_write.allowed);
        assert_eq!(
            artifact.memory_write.scope.as_deref(),
            Some("project:signal-triage")
        );
        match artifact.payload {
            SignalTriageReviewedPayload::UseCaseDiscovery {
                candidate_use_case,
                rollout_state,
                ..
            } => {
                assert!(candidate_use_case.contains("operator to approve"));
                assert_eq!(rollout_state, "disabled_pending_review");
            }
            _ => panic!("expected use case discovery payload"),
        }
    }

    #[test]
    fn research_blocks_missing_evidence_and_speculative_confidence() {
        let mut signal = research_fixture();
        signal.evidence_refs.clear();
        signal.confidence = "speculative".to_string();

        let outcome = triage_signal(&signal);

        assert_eq!(outcome.status, "blocked");
        assert!(outcome.artifact.is_none());
        assert_missing(&outcome.gate, "evidence_present");
        assert_missing(&outcome.gate, "confidence_sufficient");
    }

    #[test]
    fn duplicate_signal_blocks_reviewed_proposal() {
        let mut signal = research_fixture();
        signal.duplicate_of = Some("research-signal-seed".to_string());

        let outcome = triage_signal(&signal);

        assert_eq!(outcome.status, "blocked");
        assert_missing(&outcome.gate, "not_duplicate");
    }

    #[test]
    fn use_case_blocks_auto_enabled_workflow_and_ungoverned_memory() {
        let mut signal = use_case_fixture();
        signal.rollout_disabled = false;
        signal.auto_enable_workflow = true;
        signal.memory_policy = SignalTriageMemoryPolicy {
            enabled: true,
            review_required: false,
            ..SignalTriageMemoryPolicy::default()
        };

        let outcome = triage_signal(&signal);

        assert_eq!(outcome.status, "blocked");
        assert!(outcome.artifact.is_none());
        assert_missing(&outcome.gate, "workflow_disabled");
        assert_missing(&outcome.gate, "memory_governed");
    }

    fn assert_missing(gate: &SignalTriageGateReport, key: &str) {
        assert!(
            gate.missing.iter().any(|value| value == key),
            "expected missing gate `{key}`, got {:?}",
            gate.missing
        );
    }
}
