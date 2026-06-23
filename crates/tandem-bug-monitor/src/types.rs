use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_types::ModelSpec;

pub const BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID: &str = "legacy-github";

fn default_bug_monitor_log_format() -> BugMonitorLogFormat {
    BugMonitorLogFormat::Auto
}

fn default_bug_monitor_minimum_level() -> BugMonitorLogMinimumLevel {
    BugMonitorLogMinimumLevel::Error
}

fn default_bug_monitor_watch_interval_seconds() -> u64 {
    60
}

fn default_bug_monitor_log_start_position() -> BugMonitorLogStartPosition {
    BugMonitorLogStartPosition::End
}

fn default_bug_monitor_max_bytes_per_poll() -> u64 {
    262_144
}

fn default_bug_monitor_max_candidates_per_poll() -> usize {
    20
}

fn default_bug_monitor_fingerprint_cooldown_ms() -> u64 {
    3_600_000
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BugMonitorProviderPreference {
    #[default]
    Auto,
    OfficialGithub,
    Composio,
    Arcade,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BugMonitorLabelMode {
    #[default]
    ReporterOnly,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BugMonitorDestinationKind {
    #[default]
    GithubIssue,
    LinearIssue,
    Webhook,
    Telemetry,
    McpTool,
    InternalMemory,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BugMonitorApprovalPolicy {
    #[default]
    Inherit,
    Always,
    HighRisk,
    Never,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BugMonitorDestinationConfig {
    pub destination_id: String,
    pub name: String,
    #[serde(default)]
    pub kind: BugMonitorDestinationKind,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub require_approval: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_server: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linear_team: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linear_project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webhook_secret_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_category: Option<String>,
    #[serde(default)]
    pub route_tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<Value>,
}

impl Default for BugMonitorDestinationConfig {
    fn default() -> Self {
        Self {
            destination_id: String::new(),
            name: String::new(),
            kind: BugMonitorDestinationKind::GithubIssue,
            enabled: true,
            require_approval: false,
            repo: None,
            mcp_server: None,
            linear_team: None,
            linear_project: None,
            webhook_url: None,
            webhook_secret_ref: None,
            telemetry_path: None,
            mcp_tool: None,
            memory_category: None,
            route_tags: Vec::new(),
            config: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BugMonitorRouteConfig {
    pub route_id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub priority: i64,
    #[serde(default)]
    pub destination_ids: Vec<String>,
    #[serde(default)]
    pub approval_policy: BugMonitorApprovalPolicy,
    #[serde(default)]
    pub match_event_types: Vec<String>,
    #[serde(default)]
    pub match_sources: Vec<String>,
    #[serde(default)]
    pub match_components: Vec<String>,
    #[serde(default)]
    pub match_risk_levels: Vec<String>,
    #[serde(default)]
    pub match_confidence: Vec<String>,
    #[serde(default)]
    pub match_expected_destinations: Vec<String>,
    #[serde(default)]
    pub match_project_ids: Vec<String>,
    #[serde(default)]
    pub match_log_source_ids: Vec<String>,
    #[serde(default)]
    pub match_route_tags: Vec<String>,
}

impl Default for BugMonitorRouteConfig {
    fn default() -> Self {
        Self {
            route_id: String::new(),
            name: String::new(),
            enabled: true,
            priority: 0,
            destination_ids: Vec::new(),
            approval_policy: BugMonitorApprovalPolicy::Inherit,
            match_event_types: Vec::new(),
            match_sources: Vec::new(),
            match_components: Vec::new(),
            match_risk_levels: Vec::new(),
            match_confidence: Vec::new(),
            match_expected_destinations: Vec::new(),
            match_project_ids: Vec::new(),
            match_log_source_ids: Vec::new(),
            match_route_tags: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BugMonitorSafetyDefaults {
    #[serde(default = "default_true")]
    pub require_approval_for_high_risk: bool,
    #[serde(default = "default_true")]
    pub redact_secrets: bool,
    #[serde(default)]
    pub block_unready_destinations: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_days: Option<u64>,
}

impl Default for BugMonitorSafetyDefaults {
    fn default() -> Self {
        Self {
            require_approval_for_high_risk: true,
            redact_secrets: true,
            block_unready_destinations: false,
            retention_days: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorDestinationReadiness {
    pub destination_id: String,
    pub kind: BugMonitorDestinationKind,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub ready: bool,
    #[serde(default)]
    pub publish_ready: bool,
    #[serde(default)]
    pub requires_approval: bool,
    #[serde(default)]
    pub missing: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorRoutePreviewMatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_name: Option<String>,
    #[serde(default)]
    pub destination_ids: Vec<String>,
    #[serde(default)]
    pub approval_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorRoutePreviewResponse {
    #[serde(default)]
    pub matches: Vec<BugMonitorRoutePreviewMatch>,
    #[serde(default)]
    pub destinations: Vec<BugMonitorDestinationConfig>,
    #[serde(default)]
    pub readiness: Vec<BugMonitorDestinationReadiness>,
    #[serde(default)]
    pub default_destination_ids: Vec<String>,
    #[serde(default)]
    pub effective_destination_ids: Vec<String>,
    #[serde(default)]
    pub approval_required: bool,
    #[serde(default)]
    pub blocked: bool,
    #[serde(default)]
    pub blocked_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BugMonitorConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub paused: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_server: Option<String>,
    #[serde(default)]
    pub provider_preference: BugMonitorProviderPreference,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_policy: Option<Value>,
    #[serde(default = "default_true")]
    pub auto_create_new_issues: bool,
    #[serde(default)]
    pub require_approval_for_new_issues: bool,
    #[serde(default = "default_true")]
    pub auto_comment_on_matched_open_issues: bool,
    #[serde(default)]
    pub label_mode: BugMonitorLabelMode,
    /// How long to wait for a queued triage run to reach a terminal state
    /// before marking the draft as timed out and falling back to a basic
    /// (non-LLM) issue body. `None` disables the deadline; `Some(0)` is
    /// treated as "no wait — fall back immediately if no artifact yet".
    /// Always serialized (even when `None`) so an explicit `None` set by
    /// the operator survives a save/load cycle instead of being replaced
    /// by `default_triage_timeout_ms` on the next deserialize.
    #[serde(default = "default_triage_timeout_ms")]
    pub triage_timeout_ms: Option<u64>,
    #[serde(default)]
    pub monitored_projects: Vec<BugMonitorMonitoredProject>,
    #[serde(default)]
    pub destinations: Vec<BugMonitorDestinationConfig>,
    #[serde(default)]
    pub routes: Vec<BugMonitorRouteConfig>,
    #[serde(default)]
    pub default_destination_ids: Vec<String>,
    #[serde(default)]
    pub safety_defaults: BugMonitorSafetyDefaults,
    #[serde(default)]
    pub updated_at_ms: u64,
}

fn default_triage_timeout_ms() -> Option<u64> {
    // Aligned with `bug_monitor_triage_spec.execution.max_total_runtime_ms`
    // (1_800_000 ms / 30 minutes). The previous 5-minute default
    // guaranteed timeouts because individual nodes have per-node
    // timeout_ms of up to 600_000 ms (research) plus 240_000 ms
    // (inspect/validate) plus 360_000 ms (fix proposal). Even a
    // single slow node could exceed the external deadline. The new
    // value lets nodes use their full budget; the per-node and
    // per-run timeouts inside the spec remain the real ceiling.
    Some(1_800_000)
}

impl Default for BugMonitorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            paused: false,
            workspace_root: None,
            repo: None,
            mcp_server: None,
            provider_preference: BugMonitorProviderPreference::Auto,
            model_policy: None,
            auto_create_new_issues: true,
            require_approval_for_new_issues: false,
            auto_comment_on_matched_open_issues: true,
            label_mode: BugMonitorLabelMode::ReporterOnly,
            triage_timeout_ms: default_triage_timeout_ms(),
            monitored_projects: Vec::new(),
            destinations: Vec::new(),
            routes: Vec::new(),
            default_destination_ids: Vec::new(),
            safety_defaults: BugMonitorSafetyDefaults::default(),
            updated_at_ms: 0,
        }
    }
}

impl BugMonitorConfig {
    pub fn effective_destinations(&self) -> Vec<BugMonitorDestinationConfig> {
        if !self.destinations.is_empty() {
            return self.destinations.clone();
        }

        vec![BugMonitorDestinationConfig {
            destination_id: BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string(),
            name: "GitHub (legacy Bug Monitor)".to_string(),
            kind: BugMonitorDestinationKind::GithubIssue,
            enabled: self.enabled,
            require_approval: self.require_approval_for_new_issues,
            repo: self.repo.clone(),
            mcp_server: self.mcp_server.clone(),
            route_tags: vec!["legacy_github".to_string()],
            ..BugMonitorDestinationConfig::default()
        }]
    }

    pub fn effective_default_destination_ids(&self) -> Vec<String> {
        let explicit = self
            .default_destination_ids
            .iter()
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        if !explicit.is_empty() {
            return explicit;
        }

        if self.destinations.is_empty() {
            return vec![BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()];
        }

        Vec::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorMonitoredProject {
    pub project_id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub paused: bool,
    pub repo: String,
    pub workspace_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_server: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_policy: Option<Value>,
    #[serde(default = "default_true")]
    pub auto_create_new_issues: bool,
    #[serde(default)]
    pub require_approval_for_new_issues: bool,
    #[serde(default = "default_true")]
    pub auto_comment_on_matched_open_issues: bool,
    #[serde(default)]
    pub log_sources: Vec<BugMonitorLogSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BugMonitorLogFormat {
    Auto,
    Json,
    Plaintext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BugMonitorLogMinimumLevel {
    Error,
    Warn,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BugMonitorLogStartPosition {
    End,
    Beginning,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BugMonitorLogSource {
    pub source_id: String,
    pub path: String,
    #[serde(default = "default_bug_monitor_log_format")]
    pub format: BugMonitorLogFormat,
    #[serde(default = "default_bug_monitor_minimum_level")]
    pub minimum_level: BugMonitorLogMinimumLevel,
    #[serde(default = "default_bug_monitor_watch_interval_seconds")]
    pub watch_interval_seconds: u64,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub paused: bool,
    #[serde(default = "default_bug_monitor_log_start_position")]
    pub start_position: BugMonitorLogStartPosition,
    #[serde(default = "default_bug_monitor_max_bytes_per_poll")]
    pub max_bytes_per_poll: u64,
    #[serde(default = "default_bug_monitor_max_candidates_per_poll")]
    pub max_candidates_per_poll: usize,
    #[serde(default = "default_bug_monitor_fingerprint_cooldown_ms")]
    pub fingerprint_cooldown_ms: u64,
}

impl Default for BugMonitorLogSource {
    fn default() -> Self {
        Self {
            source_id: String::new(),
            path: String::new(),
            format: default_bug_monitor_log_format(),
            minimum_level: default_bug_monitor_minimum_level(),
            watch_interval_seconds: default_bug_monitor_watch_interval_seconds(),
            enabled: true,
            paused: false,
            start_position: default_bug_monitor_log_start_position(),
            max_bytes_per_poll: default_bug_monitor_max_bytes_per_poll(),
            max_candidates_per_poll: default_bug_monitor_max_candidates_per_poll(),
            fingerprint_cooldown_ms: default_bug_monitor_fingerprint_cooldown_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorLogSourceState {
    pub project_id: String,
    pub source_id: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inode: Option<String>,
    #[serde(default)]
    pub offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial_line: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial_line_offset_start: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_line_hash: Option<String>,
    #[serde(default)]
    pub recent_fingerprints: std::collections::BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub positioned_at_ms: Option<u64>,
    #[serde(default)]
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default)]
    pub consecutive_errors: u64,
    #[serde(default)]
    pub total_bytes_read: u64,
    #[serde(default)]
    pub total_candidates: u64,
    #[serde(default)]
    pub total_submitted: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorLogCandidate {
    pub project_id: String,
    pub source_id: String,
    pub repo: String,
    pub workspace_root: String,
    pub path: String,
    pub offset_start: u64,
    pub offset_end: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inode: Option<String>,
    pub title: String,
    pub detail: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    pub event: String,
    pub level: String,
    pub excerpt: Vec<String>,
    pub raw_excerpt_redacted: Vec<String>,
    pub fingerprint: String,
    pub confidence: String,
    pub risk_level: String,
    pub expected_destination: String,
    pub evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorLogWatcherStatus {
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub enabled_projects: usize,
    #[serde(default)]
    pub enabled_sources: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_poll_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default)]
    pub sources: Vec<BugMonitorLogSourceRuntimeStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorLogSourceRuntimeStatus {
    pub project_id: String,
    pub source_id: String,
    pub path: String,
    pub healthy: bool,
    #[serde(default)]
    pub offset: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_size: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_poll_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_candidate_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_submitted_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default)]
    pub consecutive_errors: u64,
    #[serde(default)]
    pub total_bytes_read: u64,
    #[serde(default)]
    pub total_candidates: u64,
    #[serde(default)]
    pub total_submitted: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorProjectIntakeKey {
    pub key_id: String,
    pub project_id: String,
    pub name: String,
    pub key_hash: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_used_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorDraftRecord {
    pub draft_id: String,
    pub fingerprint: String,
    pub repo: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_source_id: Option<String>,
    pub status: String,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_granted_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triage_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_number: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_issue_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_comment_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_posted_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_issue_number: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_issue_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_destination: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_gate: Option<BugMonitorQualityGateReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_post_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorPostRecord {
    pub post_id: String,
    pub draft_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incident_id: Option<String>,
    pub fingerprint: String,
    pub repo: String,
    pub operation: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_number: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination_kind: Option<BugMonitorDestinationKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_match_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_destination: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_gate: Option<BugMonitorQualityGateReport>,
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_excerpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorIncidentRecord {
    pub incident_id: String,
    pub fingerprint: String,
    pub event_type: String,
    pub status: String,
    pub repo: String,
    pub workspace_root: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default)]
    pub excerpt: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(default)]
    pub occurrence_count: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triage_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_destination: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_gate: Option<BugMonitorQualityGateReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_summary: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_matches: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_payload: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorQualityGateResult {
    pub key: String,
    pub label: String,
    pub passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorQualityGateReport {
    pub stage: String,
    pub status: String,
    pub passed: bool,
    pub passed_count: usize,
    pub total_count: usize,
    #[serde(default)]
    pub gates: Vec<BugMonitorQualityGateResult>,
    #[serde(default)]
    pub missing: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorRuntimeStatus {
    #[serde(default)]
    pub monitoring_active: bool,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub pending_incidents: usize,
    #[serde(default)]
    pub total_incidents: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_processed_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_incident_event_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_runtime_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_post_result: Option<String>,
    #[serde(default)]
    pub pending_posts: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorSubmission {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(default)]
    pub excerpt: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_destination: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorCapabilityReadiness {
    #[serde(default)]
    pub github_list_issues: bool,
    #[serde(default)]
    pub github_get_issue: bool,
    #[serde(default)]
    pub github_create_issue: bool,
    #[serde(default)]
    pub github_comment_on_issue: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorCapabilityMatch {
    pub capability_id: String,
    pub provider: String,
    pub tool_name: String,
    pub binding_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorBindingCandidate {
    pub capability_id: String,
    pub binding_tool_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub matched: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorReadiness {
    #[serde(default)]
    pub config_valid: bool,
    #[serde(default)]
    pub repo_valid: bool,
    #[serde(default)]
    pub mcp_server_present: bool,
    #[serde(default)]
    pub mcp_connected: bool,
    #[serde(default)]
    pub github_read_ready: bool,
    #[serde(default)]
    pub github_write_ready: bool,
    #[serde(default)]
    pub selected_model_ready: bool,
    #[serde(default)]
    pub ingest_ready: bool,
    #[serde(default)]
    pub publish_ready: bool,
    #[serde(default)]
    pub runtime_ready: bool,
    #[serde(default)]
    pub destination_ready: bool,
    #[serde(default)]
    pub route_preview_ready: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorStatus {
    pub config: BugMonitorConfig,
    pub readiness: BugMonitorReadiness,
    #[serde(default)]
    pub runtime: BugMonitorRuntimeStatus,
    #[serde(default)]
    pub log_watcher: BugMonitorLogWatcherStatus,
    pub required_capabilities: BugMonitorCapabilityReadiness,
    #[serde(default)]
    pub missing_required_capabilities: Vec<String>,
    #[serde(default)]
    pub resolved_capabilities: Vec<BugMonitorCapabilityMatch>,
    #[serde(default)]
    pub discovered_mcp_tools: Vec<String>,
    #[serde(default)]
    pub selected_server_binding_candidates: Vec<BugMonitorBindingCandidate>,
    #[serde(default)]
    pub destinations: Vec<BugMonitorDestinationConfig>,
    #[serde(default)]
    pub destination_readiness: Vec<BugMonitorDestinationReadiness>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_source_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bindings_last_merged_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_model: Option<ModelSpec>,
    #[serde(default)]
    pub pending_drafts: usize,
    #[serde(default)]
    pub pending_posts: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_activity_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn legacy_config_deserializes_with_effective_github_destination() {
        let config: BugMonitorConfig = serde_json::from_value(json!({
            "enabled": true,
            "repo": "acme/platform",
            "mcp_server": "github",
            "require_approval_for_new_issues": true
        }))
        .expect("legacy bug monitor config should deserialize");

        assert!(config.destinations.is_empty());
        assert!(config.routes.is_empty());
        assert_eq!(
            config.effective_default_destination_ids(),
            vec![BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()]
        );

        let destinations = config.effective_destinations();
        assert_eq!(destinations.len(), 1);
        assert_eq!(
            destinations[0].destination_id,
            BUG_MONITOR_LEGACY_GITHUB_DESTINATION_ID
        );
        assert_eq!(destinations[0].kind, BugMonitorDestinationKind::GithubIssue);
        assert_eq!(destinations[0].repo.as_deref(), Some("acme/platform"));
        assert_eq!(destinations[0].mcp_server.as_deref(), Some("github"));
        assert!(destinations[0].require_approval);
    }

    #[test]
    fn explicit_destinations_preserve_empty_default_destination_ids() {
        let config = BugMonitorConfig {
            destinations: vec![BugMonitorDestinationConfig {
                destination_id: "linear-prod".to_string(),
                name: "Linear".to_string(),
                kind: BugMonitorDestinationKind::LinearIssue,
                ..BugMonitorDestinationConfig::default()
            }],
            ..BugMonitorConfig::default()
        };

        assert_eq!(config.effective_destinations().len(), 1);
        assert!(config.effective_default_destination_ids().is_empty());
    }

    #[test]
    fn safety_defaults_are_fail_closed_for_high_risk_and_redaction() {
        let defaults = BugMonitorSafetyDefaults::default();

        assert!(defaults.require_approval_for_high_risk);
        assert!(defaults.redact_secrets);
        assert!(!defaults.block_unready_destinations);
        assert_eq!(defaults.retention_days, None);
    }

    #[test]
    fn legacy_post_records_deserialize_without_destination_receipts() {
        let post: BugMonitorPostRecord = serde_json::from_value(json!({
            "post_id": "post-1",
            "draft_id": "draft-1",
            "fingerprint": "fp",
            "repo": "acme/platform",
            "operation": "create_issue",
            "status": "posted",
            "issue_number": 42,
            "issue_url": "https://github.com/acme/platform/issues/42",
            "idempotency_key": "legacy-key",
            "created_at_ms": 10,
            "updated_at_ms": 20
        }))
        .expect("legacy bug monitor post should deserialize");

        assert_eq!(post.issue_number, Some(42));
        assert_eq!(post.destination_id, None);
        assert_eq!(post.destination_kind, None);
        assert_eq!(post.external_url, None);
        assert_eq!(post.receipt, None);
    }
}
