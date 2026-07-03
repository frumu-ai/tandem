//! Continuous reassessment scheduler data model (TAN-490).
//!
//! Governance reassessment is continuous: the Incident Monitor re-runs its
//! authority / data-readiness / routing / approval / destination / incident
//! posture on a cadence *and* when relevant configuration or runtime
//! assumptions change. Each run produces a versioned [`ReassessmentRecord`] that
//! compares the current findings against the previous run (new / recurring /
//! resolved), carries evidence refs, and is redacted (ids, counts, and stable
//! fingerprints only). The metric *values* are computed server-side from live
//! state; these types carry the operator-tunable cadence and the pure
//! comparison / scheduling / trigger-detection logic.
//!
//! Reassessment defaults to dry-run/reporting behavior and never mutates
//! external systems: escalation of a finding flows through the normal Incident
//! Monitor draft / approval / routing path, not from this module.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::types::IncidentMonitorConfig;

pub const REASSESSMENT_SCHEMA_VERSION: u32 = 1;

/// Default reassessment cadence: 7 days.
pub const DEFAULT_REASSESSMENT_CADENCE_MS: u64 = 7 * 24 * 60 * 60 * 1_000;
/// Default overdue grace period: 1 day past the due date before a scope is
/// flagged overdue (vs merely due).
pub const DEFAULT_REASSESSMENT_OVERDUE_GRACE_MS: u64 = 24 * 60 * 60 * 1_000;

/// The reason a reassessment ran: either the recurring cadence, an operator
/// request, or one of the change events called out in the operating model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReassessmentTrigger {
    Scheduled,
    Manual,
    ModelPolicyChange,
    WorkflowAuthorityChange,
    McpInventoryChange,
    DestinationOrRouteChange,
    MonitoredSourceChange,
    TenantBoundaryChange,
    ApprovalPolicyChange,
    RecurringIncidentThreshold,
    ComplianceMappingChange,
}

impl ReassessmentTrigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            ReassessmentTrigger::Scheduled => "scheduled",
            ReassessmentTrigger::Manual => "manual",
            ReassessmentTrigger::ModelPolicyChange => "model_policy_change",
            ReassessmentTrigger::WorkflowAuthorityChange => "workflow_authority_change",
            ReassessmentTrigger::McpInventoryChange => "mcp_inventory_change",
            ReassessmentTrigger::DestinationOrRouteChange => "destination_or_route_change",
            ReassessmentTrigger::MonitoredSourceChange => "monitored_source_change",
            ReassessmentTrigger::TenantBoundaryChange => "tenant_boundary_change",
            ReassessmentTrigger::ApprovalPolicyChange => "approval_policy_change",
            ReassessmentTrigger::RecurringIncidentThreshold => "recurring_incident_threshold",
            ReassessmentTrigger::ComplianceMappingChange => "compliance_mapping_change",
        }
    }

    /// True for triggers fired by a change event (everything except the periodic
    /// cadence and an explicit manual run).
    pub fn is_change_event(&self) -> bool {
        !matches!(
            self,
            ReassessmentTrigger::Scheduled | ReassessmentTrigger::Manual
        )
    }
}

fn default_reassessment_cadence_ms() -> u64 {
    DEFAULT_REASSESSMENT_CADENCE_MS
}
fn default_reassessment_overdue_grace_ms() -> u64 {
    DEFAULT_REASSESSMENT_OVERDUE_GRACE_MS
}
fn default_true() -> bool {
    true
}

/// Operator-tunable reassessment schedule configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentMonitorReassessmentConfig {
    /// How often a scope must be reassessed before it is considered due.
    #[serde(default = "default_reassessment_cadence_ms")]
    pub cadence_ms: u64,
    /// Grace period past the due date before a scope is flagged overdue.
    #[serde(default = "default_reassessment_overdue_grace_ms")]
    pub overdue_grace_ms: u64,
    /// Whether configuration-change events schedule an immediate reassessment.
    #[serde(default = "default_true")]
    pub change_triggers_enabled: bool,
}

impl Default for IncidentMonitorReassessmentConfig {
    fn default() -> Self {
        Self {
            cadence_ms: default_reassessment_cadence_ms(),
            overdue_grace_ms: default_reassessment_overdue_grace_ms(),
            change_triggers_enabled: default_true(),
        }
    }
}

impl IncidentMonitorReassessmentConfig {
    /// Effective cadence, guarding against a zero/negative operator value.
    pub fn effective_cadence_ms(&self) -> u64 {
        self.cadence_ms.max(1)
    }
}

/// A single reassessment finding, carried across runs by its stable
/// `fingerprint` so recurrences increment `occurrence_count` instead of adding
/// duplicate noise.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReassessmentFinding {
    pub fingerprint: String,
    pub rule_id: String,
    /// The subject the finding is about: `deployment`, `source:<id>`,
    /// `destination:<id>`, `route:<id>`, `workflow:<id>`, etc.
    pub scope: String,
    pub severity: String,
    pub category: String,
    /// Redacted, id-only summary (never free-text payload/message content).
    pub summary: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    pub first_seen_at_ms: u64,
    pub last_seen_at_ms: u64,
    pub occurrence_count: u64,
    /// `new` on first observation, `recurring` once carried across a run.
    pub status: String,
}

/// Comparison of a run against the immediately previous run for the same scope.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReassessmentComparison {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_record_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_version: Option<u64>,
    #[serde(default)]
    pub new_fingerprints: Vec<String>,
    #[serde(default)]
    pub recurring_fingerprints: Vec<String>,
    #[serde(default)]
    pub resolved_fingerprints: Vec<String>,
}

/// A versioned reassessment result for one scope of one tenant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReassessmentRecord {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    pub record_id: String,
    /// Stable key grouping a scope's run history: `<org>/<workspace>/<scope>`.
    pub scope_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    pub scope: String,
    pub version: u64,
    pub trigger: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_detail: Option<String>,
    pub generated_at_ms: u64,
    #[serde(default = "default_dry_run_mode")]
    pub mode: String,
    #[serde(default)]
    pub mutates_external_systems: bool,
    #[serde(default)]
    pub findings: Vec<ReassessmentFinding>,
    #[serde(default)]
    pub comparison: ReassessmentComparison,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

fn default_schema_version() -> u32 {
    REASSESSMENT_SCHEMA_VERSION
}
fn default_dry_run_mode() -> String {
    "dry_run".to_string()
}

/// Point-in-time schedule status for a scope, derived from its run history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReassessmentScheduleStatus {
    pub scope_key: String,
    pub scope: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_completed_at_ms: Option<u64>,
    pub next_due_at_ms: u64,
    pub overdue: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_version: Option<u64>,
}

/// Build the stable fingerprint for a finding. Deterministic across processes
/// and releases (sha256 over a normalized composite key) so a recurring finding
/// keeps the same fingerprint and does not re-notify.
pub fn reassessment_finding_fingerprint(
    tenant_id: Option<&str>,
    workspace_id: Option<&str>,
    scope: &str,
    rule_id: &str,
    subject: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(tenant_id.unwrap_or("").as_bytes());
    hasher.update([0x1f]);
    hasher.update(workspace_id.unwrap_or("").as_bytes());
    hasher.update([0x1f]);
    hasher.update(scope.trim().to_ascii_lowercase().as_bytes());
    hasher.update([0x1f]);
    hasher.update(rule_id.trim().to_ascii_lowercase().as_bytes());
    hasher.update([0x1f]);
    hasher.update(subject.trim().to_ascii_lowercase().as_bytes());
    let digest = hasher.finalize();
    // 16 bytes / 32 hex chars is ample collision resistance for this use.
    digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Compare `current` findings against the `previous` record (if any), mutating
/// each current finding's `first_seen_at_ms` / `occurrence_count` / `status`
/// to carry recurrences forward, and returning the new/recurring/resolved
/// fingerprint partition. Stable fingerprints are what suppress duplicate noise.
pub fn apply_reassessment_comparison(
    previous: Option<&ReassessmentRecord>,
    current: &mut [ReassessmentFinding],
    now_ms: u64,
) -> ReassessmentComparison {
    let mut comparison = ReassessmentComparison::default();
    if let Some(previous) = previous {
        comparison.previous_record_id = Some(previous.record_id.clone());
        comparison.previous_version = Some(previous.version);
    }
    let previous_by_fingerprint: std::collections::HashMap<&str, &ReassessmentFinding> = previous
        .map(|record| {
            record
                .findings
                .iter()
                .map(|finding| (finding.fingerprint.as_str(), finding))
                .collect()
        })
        .unwrap_or_default();

    let mut current_fingerprints = std::collections::HashSet::new();
    for finding in current.iter_mut() {
        finding.last_seen_at_ms = now_ms;
        current_fingerprints.insert(finding.fingerprint.clone());
        match previous_by_fingerprint.get(finding.fingerprint.as_str()) {
            Some(prior) => {
                finding.first_seen_at_ms = prior.first_seen_at_ms;
                finding.occurrence_count = prior.occurrence_count.saturating_add(1);
                finding.status = "recurring".to_string();
                comparison
                    .recurring_fingerprints
                    .push(finding.fingerprint.clone());
            }
            None => {
                finding.first_seen_at_ms = now_ms;
                finding.occurrence_count = 1;
                finding.status = "new".to_string();
                comparison
                    .new_fingerprints
                    .push(finding.fingerprint.clone());
            }
        }
    }

    if let Some(previous) = previous {
        for finding in &previous.findings {
            if !current_fingerprints.contains(&finding.fingerprint) {
                comparison
                    .resolved_fingerprints
                    .push(finding.fingerprint.clone());
            }
        }
    }

    comparison
}

/// When a scope is next due, given its last completed run. A scope that has
/// never run is due immediately (`now_ms`).
pub fn reassessment_next_due_at_ms(
    last_completed_at_ms: Option<u64>,
    cadence_ms: u64,
    now_ms: u64,
) -> u64 {
    match last_completed_at_ms {
        Some(last) => last.saturating_add(cadence_ms.max(1)),
        None => now_ms,
    }
}

/// True once a scope is past its due date by more than `grace_ms`. A scope that
/// has never run is *due* but not yet *overdue* until the grace period lapses.
pub fn reassessment_is_overdue(
    last_completed_at_ms: Option<u64>,
    cadence_ms: u64,
    grace_ms: u64,
    now_ms: u64,
) -> bool {
    let next_due = reassessment_next_due_at_ms(last_completed_at_ms, cadence_ms, now_ms);
    now_ms > next_due.saturating_add(grace_ms)
}

/// Whether a scope should run now: it is due (or overdue) by cadence.
pub fn reassessment_is_due(
    last_completed_at_ms: Option<u64>,
    cadence_ms: u64,
    now_ms: u64,
) -> bool {
    now_ms >= reassessment_next_due_at_ms(last_completed_at_ms, cadence_ms, now_ms)
}

/// Detect which change-triggered reassessments a config edit should schedule, by
/// diffing the governance-relevant sections of the previous and next config.
/// Returns the distinct triggers (stable order) so a single edit can fan out to
/// several (e.g. a monitored-source edit that also re-binds a tenant).
pub fn config_change_reassessment_triggers(
    previous: &IncidentMonitorConfig,
    next: &IncidentMonitorConfig,
) -> Vec<ReassessmentTrigger> {
    let mut triggers = Vec::new();
    let mut push = |trigger: ReassessmentTrigger| {
        if !triggers.contains(&trigger) {
            triggers.push(trigger);
        }
    };

    if section_changed(&previous.routes, &next.routes)
        || section_changed(&previous.destinations, &next.destinations)
        || section_changed(
            &previous.default_destination_ids,
            &next.default_destination_ids,
        )
    {
        push(ReassessmentTrigger::DestinationOrRouteChange);
    }
    if section_changed(&previous.monitored_projects, &next.monitored_projects) {
        push(ReassessmentTrigger::MonitoredSourceChange);
    }
    if tenant_bindings(previous) != tenant_bindings(next) {
        push(ReassessmentTrigger::TenantBoundaryChange);
    }
    if section_changed(&previous.model_policy, &next.model_policy) {
        push(ReassessmentTrigger::ModelPolicyChange);
    }
    if previous.mcp_server != next.mcp_server {
        push(ReassessmentTrigger::McpInventoryChange);
    }
    if section_changed(&previous.safety_defaults, &next.safety_defaults)
        || previous.require_approval_for_new_issues != next.require_approval_for_new_issues
    {
        push(ReassessmentTrigger::ApprovalPolicyChange);
    }

    triggers
}

/// Structural equality via JSON, so we don't require `PartialEq` on the many
/// config sub-structs and stay robust to field reordering.
fn section_changed<T: Serialize>(previous: &T, next: &T) -> bool {
    serde_json::to_value(previous).ok() != serde_json::to_value(next).ok()
}

/// The set of (tenant, workspace) bindings a config attributes to its monitored
/// projects — a change here moves the tenant boundary.
fn tenant_bindings(config: &IncidentMonitorConfig) -> Vec<(Option<String>, Option<String>)> {
    let mut bindings = config
        .monitored_projects
        .iter()
        .map(|project| (project.tenant_id.clone(), project.workspace_id.clone()))
        .collect::<Vec<_>>();
    bindings.sort();
    bindings.dedup();
    bindings
}

/// Best-effort extraction of a finding's subject scope from a posture-finding
/// JSON value, checking the id-bearing keys in priority order. Falls back to the
/// deployment scope so every finding is always attributed somewhere.
pub fn reassessment_scope_from_finding(finding: &Value) -> String {
    const SUBJECT_KEYS: [(&str, &str); 6] = [
        ("source_id", "source"),
        ("destination_id", "destination"),
        ("route_id", "route"),
        ("workflow_id", "workflow"),
        ("automation_id", "workflow"),
        ("project_id", "source"),
    ];
    for (key, prefix) in SUBJECT_KEYS {
        if let Some(id) = finding
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return format!("{prefix}:{id}");
        }
    }
    "deployment".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record_with(findings: Vec<ReassessmentFinding>, version: u64) -> ReassessmentRecord {
        ReassessmentRecord {
            schema_version: REASSESSMENT_SCHEMA_VERSION,
            record_id: format!("rec-{version}"),
            scope_key: "org-a/ws-a/deployment".to_string(),
            tenant_id: Some("org-a".to_string()),
            workspace_id: Some("ws-a".to_string()),
            scope: "deployment".to_string(),
            version,
            trigger: ReassessmentTrigger::Scheduled.as_str().to_string(),
            trigger_detail: None,
            generated_at_ms: 1_000 * version,
            mode: "dry_run".to_string(),
            mutates_external_systems: false,
            findings,
            comparison: ReassessmentComparison::default(),
            evidence_refs: Vec::new(),
        }
    }

    fn finding(fingerprint: &str) -> ReassessmentFinding {
        ReassessmentFinding {
            fingerprint: fingerprint.to_string(),
            rule_id: "rule".to_string(),
            scope: "deployment".to_string(),
            severity: "high".to_string(),
            category: "governance_gap".to_string(),
            summary: "rule (governance_gap, high)".to_string(),
            evidence_refs: Vec::new(),
            first_seen_at_ms: 0,
            last_seen_at_ms: 0,
            occurrence_count: 0,
            status: String::new(),
        }
    }

    #[test]
    fn fingerprint_is_stable_and_scope_sensitive() {
        let a = reassessment_finding_fingerprint(
            Some("org-a"),
            Some("ws-a"),
            "deployment",
            "rule.x",
            "sub",
        );
        let b = reassessment_finding_fingerprint(
            Some("org-a"),
            Some("ws-a"),
            "deployment",
            "rule.x",
            "sub",
        );
        assert_eq!(a, b, "same inputs must hash identically");
        let other_scope = reassessment_finding_fingerprint(
            Some("org-a"),
            Some("ws-a"),
            "source:s1",
            "rule.x",
            "sub",
        );
        assert_ne!(a, other_scope, "scope must change the fingerprint");
        assert_eq!(a.len(), 32);
    }

    #[test]
    fn comparison_partitions_new_recurring_and_resolved() {
        let previous = record_with(
            vec![
                {
                    let mut f = finding("keep");
                    f.first_seen_at_ms = 500;
                    f.occurrence_count = 2;
                    f
                },
                finding("gone"),
            ],
            1,
        );
        let mut current = vec![finding("keep"), finding("fresh")];
        let comparison = apply_reassessment_comparison(Some(&previous), &mut current, 9_000);

        assert_eq!(comparison.previous_version, Some(1));
        assert_eq!(comparison.new_fingerprints, vec!["fresh".to_string()]);
        assert_eq!(comparison.recurring_fingerprints, vec!["keep".to_string()]);
        assert_eq!(comparison.resolved_fingerprints, vec!["gone".to_string()]);

        let keep = current.iter().find(|f| f.fingerprint == "keep").unwrap();
        assert_eq!(keep.status, "recurring");
        assert_eq!(keep.first_seen_at_ms, 500, "first_seen carries forward");
        assert_eq!(keep.occurrence_count, 3, "recurrence increments");
        let fresh = current.iter().find(|f| f.fingerprint == "fresh").unwrap();
        assert_eq!(fresh.status, "new");
        assert_eq!(fresh.first_seen_at_ms, 9_000);
        assert_eq!(fresh.occurrence_count, 1);
    }

    #[test]
    fn overdue_and_next_due_track_cadence() {
        let cadence = 1_000;
        let grace = 100;
        // Never run: due now, not yet overdue.
        assert!(reassessment_is_due(None, cadence, 5_000));
        assert!(!reassessment_is_overdue(None, cadence, grace, 5_000));
        // Just completed: not due, next due one cadence out.
        assert_eq!(
            reassessment_next_due_at_ms(Some(5_000), cadence, 5_000),
            6_000
        );
        assert!(!reassessment_is_due(Some(5_000), cadence, 5_500));
        // Past due + grace: overdue.
        assert!(reassessment_is_due(Some(5_000), cadence, 6_000));
        assert!(reassessment_is_overdue(Some(5_000), cadence, grace, 6_200));
    }

    #[test]
    fn config_diff_detects_route_source_and_approval_triggers() {
        let mut previous = IncidentMonitorConfig::default();
        previous.monitored_projects = vec![crate::types::IncidentMonitorMonitoredProject {
            project_id: "p1".to_string(),
            name: "P1".to_string(),
            enabled: true,
            tenant_id: Some("org-a".to_string()),
            workspace_id: Some("ws-a".to_string()),
            ..Default::default()
        }];

        // Route change.
        let mut next = previous.clone();
        next.default_destination_ids = vec!["dest-x".to_string()];
        assert_eq!(
            config_change_reassessment_triggers(&previous, &next),
            vec![ReassessmentTrigger::DestinationOrRouteChange]
        );

        // Monitored-source freshness/lineage edit (no tenant rebind).
        let mut next = previous.clone();
        next.monitored_projects[0].name = "P1 renamed".to_string();
        assert_eq!(
            config_change_reassessment_triggers(&previous, &next),
            vec![ReassessmentTrigger::MonitoredSourceChange]
        );

        // Approval policy change.
        let mut next = previous.clone();
        next.require_approval_for_new_issues = !previous.require_approval_for_new_issues;
        assert_eq!(
            config_change_reassessment_triggers(&previous, &next),
            vec![ReassessmentTrigger::ApprovalPolicyChange]
        );

        // Tenant re-bind fans out to source + boundary triggers.
        let mut next = previous.clone();
        next.monitored_projects[0].tenant_id = Some("org-b".to_string());
        let triggers = config_change_reassessment_triggers(&previous, &next);
        assert!(triggers.contains(&ReassessmentTrigger::MonitoredSourceChange));
        assert!(triggers.contains(&ReassessmentTrigger::TenantBoundaryChange));

        // No change: no triggers.
        assert!(config_change_reassessment_triggers(&previous, &previous).is_empty());
    }
}
