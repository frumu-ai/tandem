use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_types::ModelSpec;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BugMonitorProviderPreference {
    Auto,
    OfficialGithub,
    Composio,
    Arcade,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BugMonitorLabelMode {
    ReporterOnly,
}

impl Default for BugMonitorLabelMode {
    fn default() -> Self {
        Self::ReporterOnly
    }
}

impl Default for BugMonitorProviderPreference {
    fn default() -> Self {
        Self::Auto
    }
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
    #[serde(default)]
    pub updated_at_ms: u64,
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
            updated_at_ms: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorDraftRecord {
    pub draft_id: String,
    pub fingerprint: String,
    pub repo: String,
    pub status: String,
    pub created_at_ms: u64,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BugMonitorStatus {
    pub config: BugMonitorConfig,
    pub readiness: BugMonitorReadiness,
    #[serde(default)]
    pub runtime: BugMonitorRuntimeStatus,
    pub required_capabilities: BugMonitorCapabilityReadiness,
    #[serde(default)]
    pub missing_required_capabilities: Vec<String>,
    #[serde(default)]
    pub resolved_capabilities: Vec<BugMonitorCapabilityMatch>,
    #[serde(default)]
    pub discovered_mcp_tools: Vec<String>,
    #[serde(default)]
    pub selected_server_binding_candidates: Vec<BugMonitorBindingCandidate>,
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
