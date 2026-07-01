use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_types::ModelSpec;

pub const INCIDENT_MONITOR_LEGACY_GITHUB_DESTINATION_ID: &str = "legacy-github";

fn default_incident_monitor_log_format() -> IncidentMonitorLogFormat {
    IncidentMonitorLogFormat::Auto
}

fn default_incident_monitor_minimum_level() -> IncidentMonitorLogMinimumLevel {
    IncidentMonitorLogMinimumLevel::Error
}

fn default_incident_monitor_watch_interval_seconds() -> u64 {
    60
}

fn default_incident_monitor_log_start_position() -> IncidentMonitorLogStartPosition {
    IncidentMonitorLogStartPosition::End
}

fn default_incident_monitor_max_bytes_per_poll() -> u64 {
    262_144
}

fn default_incident_monitor_max_candidates_per_poll() -> usize {
    20
}

fn default_incident_monitor_fingerprint_cooldown_ms() -> u64 {
    3_600_000
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IncidentMonitorProviderPreference {
    #[default]
    Auto,
    OfficialGithub,
    Composio,
    Arcade,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IncidentMonitorLabelMode {
    #[default]
    ReporterOnly,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IncidentMonitorDestinationKind {
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
pub enum IncidentMonitorApprovalPolicy {
    #[default]
    Inherit,
    Always,
    HighRisk,
    Never,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IncidentMonitorSourceKind {
    TandemRuntime,
    TandemMonitor,
    ExternalApp,
    Ci,
    AgentRuntime,
    McpGateway,
    #[default]
    CustomerSystem,
}

impl IncidentMonitorSourceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TandemRuntime => "tandem_runtime",
            Self::TandemMonitor => "tandem_monitor",
            Self::ExternalApp => "external_app",
            Self::Ci => "ci",
            Self::AgentRuntime => "agent_runtime",
            Self::McpGateway => "mcp_gateway",
            Self::CustomerSystem => "customer_system",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentMonitorDestinationConfig {
    pub destination_id: String,
    pub name: String,
    #[serde(default)]
    pub kind: IncidentMonitorDestinationKind,
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

impl Default for IncidentMonitorDestinationConfig {
    fn default() -> Self {
        Self {
            destination_id: String::new(),
            name: String::new(),
            kind: IncidentMonitorDestinationKind::GithubIssue,
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
pub struct IncidentMonitorRouteConfig {
    pub route_id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub priority: i64,
    #[serde(default)]
    pub destination_ids: Vec<String>,
    #[serde(default)]
    pub approval_policy: IncidentMonitorApprovalPolicy,
    #[serde(default)]
    pub match_event_types: Vec<String>,
    #[serde(default)]
    pub match_sources: Vec<String>,
    #[serde(default)]
    pub match_components: Vec<String>,
    #[serde(default)]
    pub match_risk_levels: Vec<String>,
    #[serde(default)]
    pub match_risk_categories: Vec<String>,
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
    #[serde(default)]
    pub match_source_kinds: Vec<String>,
    #[serde(default)]
    pub match_tenant_ids: Vec<String>,
    #[serde(default)]
    pub match_workspace_ids: Vec<String>,
    #[serde(default)]
    pub match_event_schema_versions: Vec<String>,
}

impl Default for IncidentMonitorRouteConfig {
    fn default() -> Self {
        Self {
            route_id: String::new(),
            name: String::new(),
            enabled: true,
            priority: 0,
            destination_ids: Vec::new(),
            approval_policy: IncidentMonitorApprovalPolicy::Inherit,
            match_event_types: Vec::new(),
            match_sources: Vec::new(),
            match_components: Vec::new(),
            match_risk_levels: Vec::new(),
            match_risk_categories: Vec::new(),
            match_confidence: Vec::new(),
            match_expected_destinations: Vec::new(),
            match_project_ids: Vec::new(),
            match_log_source_ids: Vec::new(),
            match_route_tags: Vec::new(),
            match_source_kinds: Vec::new(),
            match_tenant_ids: Vec::new(),
            match_workspace_ids: Vec::new(),
            match_event_schema_versions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentMonitorSafetyDefaults {
    #[serde(default = "default_true")]
    pub require_approval_for_high_risk: bool,
    #[serde(default = "default_true")]
    pub redact_secrets: bool,
    #[serde(default)]
    pub block_unready_destinations: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_days: Option<u64>,
}

impl Default for IncidentMonitorSafetyDefaults {
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
pub struct IncidentMonitorDestinationReadiness {
    pub destination_id: String,
    pub kind: IncidentMonitorDestinationKind,
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
pub struct IncidentMonitorRoutePreviewMatch {
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
pub struct IncidentMonitorRoutePreviewResponse {
    #[serde(default)]
    pub matches: Vec<IncidentMonitorRoutePreviewMatch>,
    #[serde(default)]
    pub destinations: Vec<IncidentMonitorDestinationConfig>,
    #[serde(default)]
    pub readiness: Vec<IncidentMonitorDestinationReadiness>,
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
pub struct IncidentMonitorConfig {
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
    pub provider_preference: IncidentMonitorProviderPreference,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_policy: Option<Value>,
    #[serde(default = "default_true")]
    pub auto_create_new_issues: bool,
    #[serde(default)]
    pub require_approval_for_new_issues: bool,
    #[serde(default = "default_true")]
    pub auto_comment_on_matched_open_issues: bool,
    #[serde(default)]
    pub label_mode: IncidentMonitorLabelMode,
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
    pub monitored_projects: Vec<IncidentMonitorMonitoredProject>,
    #[serde(default)]
    pub destinations: Vec<IncidentMonitorDestinationConfig>,
    #[serde(default)]
    pub routes: Vec<IncidentMonitorRouteConfig>,
    #[serde(default)]
    pub default_destination_ids: Vec<String>,
    #[serde(default)]
    pub safety_defaults: IncidentMonitorSafetyDefaults,
    #[serde(default)]
    pub updated_at_ms: u64,
}

fn default_triage_timeout_ms() -> Option<u64> {
    // Aligned with the incident triage spec's execution.max_total_runtime_ms
    // (1_800_000 ms / 30 minutes). The previous 5-minute default
    // guaranteed timeouts because individual nodes have per-node
    // timeout_ms of up to 600_000 ms (research) plus 240_000 ms
    // (inspect/validate) plus 360_000 ms (fix proposal). Even a
    // single slow node could exceed the external deadline. The new
    // value lets nodes use their full budget; the per-node and
    // per-run timeouts inside the spec remain the real ceiling.
    Some(1_800_000)
}

impl Default for IncidentMonitorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            paused: false,
            workspace_root: None,
            repo: None,
            mcp_server: None,
            provider_preference: IncidentMonitorProviderPreference::Auto,
            model_policy: None,
            auto_create_new_issues: true,
            require_approval_for_new_issues: false,
            auto_comment_on_matched_open_issues: true,
            label_mode: IncidentMonitorLabelMode::ReporterOnly,
            triage_timeout_ms: default_triage_timeout_ms(),
            monitored_projects: Vec::new(),
            destinations: Vec::new(),
            routes: Vec::new(),
            default_destination_ids: Vec::new(),
            safety_defaults: IncidentMonitorSafetyDefaults::default(),
            updated_at_ms: 0,
        }
    }
}

impl IncidentMonitorConfig {
    pub fn effective_destinations(&self) -> Vec<IncidentMonitorDestinationConfig> {
        if !self.destinations.is_empty() {
            return self.destinations.clone();
        }

        vec![IncidentMonitorDestinationConfig {
            destination_id: INCIDENT_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string(),
            name: "GitHub (default Incident Monitor)".to_string(),
            kind: IncidentMonitorDestinationKind::GithubIssue,
            enabled: self.enabled,
            require_approval: self.require_approval_for_new_issues,
            repo: self.repo.clone(),
            mcp_server: self.mcp_server.clone(),
            route_tags: vec!["legacy_github".to_string()],
            ..IncidentMonitorDestinationConfig::default()
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
            return vec![INCIDENT_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()];
        }

        Vec::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IncidentMonitorMonitoredProject {
    pub project_id: String,
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub paused: bool,
    pub repo: String,
    pub workspace_root: String,
    #[serde(default)]
    pub source_kind: IncidentMonitorSourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_server: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_policy: Option<Value>,
    #[serde(default)]
    pub allowed_destination_ids: Vec<String>,
    #[serde(default)]
    pub default_destination_ids: Vec<String>,
    #[serde(default)]
    pub default_route_tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_schema_version: Option<String>,
    #[serde(default)]
    pub approval_policy: IncidentMonitorApprovalPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redaction_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_profile: Option<String>,
    #[serde(default = "default_true")]
    pub auto_create_new_issues: bool,
    #[serde(default)]
    pub require_approval_for_new_issues: bool,
    #[serde(default = "default_true")]
    pub auto_comment_on_matched_open_issues: bool,
    #[serde(default)]
    pub log_sources: Vec<IncidentMonitorLogSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IncidentMonitorLogFormat {
    Auto,
    Json,
    Plaintext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IncidentMonitorLogMinimumLevel {
    Error,
    Warn,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IncidentMonitorLogStartPosition {
    End,
    Beginning,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentMonitorLogSource {
    pub source_id: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<IncidentMonitorSourceKind>,
    #[serde(default = "default_incident_monitor_log_format")]
    pub format: IncidentMonitorLogFormat,
    #[serde(default = "default_incident_monitor_minimum_level")]
    pub minimum_level: IncidentMonitorLogMinimumLevel,
    #[serde(default = "default_incident_monitor_watch_interval_seconds")]
    pub watch_interval_seconds: u64,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub paused: bool,
    #[serde(default = "default_incident_monitor_log_start_position")]
    pub start_position: IncidentMonitorLogStartPosition,
    #[serde(default = "default_incident_monitor_max_bytes_per_poll")]
    pub max_bytes_per_poll: u64,
    #[serde(default = "default_incident_monitor_max_candidates_per_poll")]
    pub max_candidates_per_poll: usize,
    #[serde(default = "default_incident_monitor_fingerprint_cooldown_ms")]
    pub fingerprint_cooldown_ms: u64,
    #[serde(default)]
    pub allowed_destination_ids: Vec<String>,
    #[serde(default)]
    pub default_destination_ids: Vec<String>,
    #[serde(default)]
    pub default_route_tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_schema_version: Option<String>,
    #[serde(default)]
    pub approval_policy: IncidentMonitorApprovalPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redaction_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_profile: Option<String>,
}

impl Default for IncidentMonitorLogSource {
    fn default() -> Self {
        Self {
            source_id: String::new(),
            path: String::new(),
            source_kind: None,
            format: default_incident_monitor_log_format(),
            minimum_level: default_incident_monitor_minimum_level(),
            watch_interval_seconds: default_incident_monitor_watch_interval_seconds(),
            enabled: true,
            paused: false,
            start_position: default_incident_monitor_log_start_position(),
            max_bytes_per_poll: default_incident_monitor_max_bytes_per_poll(),
            max_candidates_per_poll: default_incident_monitor_max_candidates_per_poll(),
            fingerprint_cooldown_ms: default_incident_monitor_fingerprint_cooldown_ms(),
            allowed_destination_ids: Vec::new(),
            default_destination_ids: Vec::new(),
            default_route_tags: Vec::new(),
            tenant_id: None,
            workspace_id: None,
            event_schema_version: None,
            approval_policy: IncidentMonitorApprovalPolicy::Inherit,
            redaction_profile: None,
            retention_profile: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IncidentMonitorSourceBinding {
    pub project_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_id: Option<String>,
    pub repo: String,
    pub workspace_root: String,
    pub source_kind: IncidentMonitorSourceKind,
    #[serde(default)]
    pub allowed_destination_ids: Vec<String>,
    #[serde(default)]
    pub default_destination_ids: Vec<String>,
    #[serde(default)]
    pub default_route_tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_schema_version: Option<String>,
    #[serde(default)]
    pub approval_policy: IncidentMonitorApprovalPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redaction_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_profile: Option<String>,
}

impl IncidentMonitorMonitoredProject {
    pub fn source_binding(
        &self,
        source: Option<&IncidentMonitorLogSource>,
    ) -> IncidentMonitorSourceBinding {
        let source_allowed = source
            .map(|row| normalize_source_values(&row.allowed_destination_ids))
            .unwrap_or_default();
        let project_allowed = normalize_source_values(&self.allowed_destination_ids);
        let allowed_destination_ids = if source_allowed.is_empty() {
            project_allowed
        } else if project_allowed.is_empty() {
            source_allowed
        } else {
            source_allowed
                .into_iter()
                .filter(|value| project_allowed.iter().any(|allowed| allowed == value))
                .collect()
        };

        let source_default_destination_ids = source
            .map(|row| normalize_source_values(&row.default_destination_ids))
            .unwrap_or_default();
        let default_destination_ids = if source_default_destination_ids.is_empty() {
            normalize_source_values(&self.default_destination_ids)
        } else {
            source_default_destination_ids
        };

        let mut default_route_tags = normalize_source_values(&self.default_route_tags);
        if let Some(source) = source {
            push_normalized_source_values(&mut default_route_tags, &source.default_route_tags);
        }

        let approval_policy = source
            .map(|row| row.approval_policy.clone())
            .filter(|value| *value != IncidentMonitorApprovalPolicy::Inherit)
            .unwrap_or_else(|| self.approval_policy.clone());

        IncidentMonitorSourceBinding {
            project_id: self.project_id.clone(),
            source_id: source.map(|row| row.source_id.clone()),
            repo: self.repo.clone(),
            workspace_root: self.workspace_root.clone(),
            source_kind: source
                .and_then(|row| row.source_kind.clone())
                .unwrap_or_else(|| self.source_kind.clone()),
            allowed_destination_ids,
            default_destination_ids,
            default_route_tags,
            tenant_id: source
                .and_then(|row| row.tenant_id.clone())
                .or_else(|| self.tenant_id.clone()),
            workspace_id: source
                .and_then(|row| row.workspace_id.clone())
                .or_else(|| self.workspace_id.clone()),
            event_schema_version: source
                .and_then(|row| row.event_schema_version.clone())
                .or_else(|| self.event_schema_version.clone()),
            approval_policy,
            redaction_profile: source
                .and_then(|row| row.redaction_profile.clone())
                .or_else(|| self.redaction_profile.clone()),
            retention_profile: source
                .and_then(|row| row.retention_profile.clone())
                .or_else(|| self.retention_profile.clone()),
        }
    }
}

fn normalize_source_values(values: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    push_normalized_source_values(&mut out, values);
    out
}

fn push_normalized_source_values(out: &mut Vec<String>, values: &[String]) {
    for value in values {
        let value = value.trim();
        if value.is_empty() || out.iter().any(|existing| existing == value) {
            continue;
        }
        out.push(value.to_string());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IncidentMonitorLogSourceState {
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
pub struct IncidentMonitorLogCandidate {
    pub project_id: String,
    pub source_id: String,
    #[serde(default)]
    pub source_kind: IncidentMonitorSourceKind,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blast_radius: Option<String>,
    #[serde(default)]
    pub external_correlation_ids: Vec<String>,
    pub expected_destination: String,
    pub evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_ms: Option<u64>,
    #[serde(default)]
    pub route_tags: Vec<String>,
    #[serde(default)]
    pub allowed_destination_ids: Vec<String>,
    #[serde(default)]
    pub default_destination_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_schema_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_approval_policy: Option<IncidentMonitorApprovalPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redaction_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_profile: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IncidentMonitorLogWatcherStatus {
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
    pub sources: Vec<IncidentMonitorLogSourceRuntimeStatus>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IncidentMonitorLogSourceRuntimeStatus {
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
pub struct IncidentMonitorProjectIntakeKey {
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
pub struct IncidentMonitorDraftRecord {
    pub draft_id: String,
    pub fingerprint: String,
    pub repo: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<IncidentMonitorSourceKind>,
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
    pub actor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blast_radius: Option<String>,
    #[serde(default)]
    pub external_correlation_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_destination: Option<String>,
    #[serde(default)]
    pub route_tags: Vec<String>,
    #[serde(default)]
    pub allowed_destination_ids: Vec<String>,
    #[serde(default)]
    pub default_destination_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_schema_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_approval_policy: Option<IncidentMonitorApprovalPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redaction_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_profile: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_gate: Option<IncidentMonitorQualityGateReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_post_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IncidentMonitorPostRecord {
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
    pub destination_kind: Option<IncidentMonitorDestinationKind>,
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
    pub quality_gate: Option<IncidentMonitorQualityGateReport>,
    pub idempotency_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_excerpt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IncidentMonitorIncidentRecord {
    pub incident_id: String,
    pub fingerprint: String,
    pub event_type: String,
    pub status: String,
    pub repo: String,
    pub workspace_root: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<IncidentMonitorSourceKind>,
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
    pub actor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blast_radius: Option<String>,
    #[serde(default)]
    pub external_correlation_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_destination: Option<String>,
    #[serde(default)]
    pub route_tags: Vec<String>,
    #[serde(default)]
    pub allowed_destination_ids: Vec<String>,
    #[serde(default)]
    pub default_destination_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_schema_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_approval_policy: Option<IncidentMonitorApprovalPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redaction_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_profile: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality_gate: Option<IncidentMonitorQualityGateReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_summary: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate_matches: Option<Vec<Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_payload: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IncidentMonitorQualityGateResult {
    pub key: String,
    pub label: String,
    pub passed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IncidentMonitorQualityGateReport {
    pub stage: String,
    pub status: String,
    pub passed: bool,
    pub passed_count: usize,
    pub total_count: usize,
    #[serde(default)]
    pub gates: Vec<IncidentMonitorQualityGateResult>,
    #[serde(default)]
    pub missing: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IncidentMonitorRuntimeStatus {
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
pub struct IncidentMonitorSubmission {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_source_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_kind: Option<IncidentMonitorSourceKind>,
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
    pub actor: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub risk_category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blast_radius: Option<String>,
    #[serde(default)]
    pub external_correlation_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_destination: Option<String>,
    #[serde(default)]
    pub route_tags: Vec<String>,
    #[serde(default)]
    pub allowed_destination_ids: Vec<String>,
    #[serde(default)]
    pub default_destination_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_schema_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_approval_policy: Option<IncidentMonitorApprovalPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redaction_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retention_profile: Option<String>,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IncidentMonitorCapabilityReadiness {
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
pub struct IncidentMonitorCapabilityMatch {
    pub capability_id: String,
    pub provider: String,
    pub tool_name: String,
    pub binding_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IncidentMonitorBindingCandidate {
    pub capability_id: String,
    pub binding_tool_name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub matched: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct IncidentMonitorReadiness {
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
pub struct IncidentMonitorStatus {
    pub config: IncidentMonitorConfig,
    pub readiness: IncidentMonitorReadiness,
    #[serde(default)]
    pub runtime: IncidentMonitorRuntimeStatus,
    #[serde(default)]
    pub log_watcher: IncidentMonitorLogWatcherStatus,
    pub required_capabilities: IncidentMonitorCapabilityReadiness,
    #[serde(default)]
    pub missing_required_capabilities: Vec<String>,
    #[serde(default)]
    pub resolved_capabilities: Vec<IncidentMonitorCapabilityMatch>,
    #[serde(default)]
    pub discovered_mcp_tools: Vec<String>,
    #[serde(default)]
    pub selected_server_binding_candidates: Vec<IncidentMonitorBindingCandidate>,
    #[serde(default)]
    pub destinations: Vec<IncidentMonitorDestinationConfig>,
    #[serde(default)]
    pub destination_readiness: Vec<IncidentMonitorDestinationReadiness>,
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
        let config: IncidentMonitorConfig = serde_json::from_value(json!({
            "enabled": true,
            "repo": "acme/platform",
            "mcp_server": "github",
            "require_approval_for_new_issues": true
        }))
        .expect("legacy Incident Monitor config should deserialize");

        assert!(config.destinations.is_empty());
        assert!(config.routes.is_empty());
        assert_eq!(
            config.effective_default_destination_ids(),
            vec![INCIDENT_MONITOR_LEGACY_GITHUB_DESTINATION_ID.to_string()]
        );

        let destinations = config.effective_destinations();
        assert_eq!(destinations.len(), 1);
        assert_eq!(
            destinations[0].destination_id,
            INCIDENT_MONITOR_LEGACY_GITHUB_DESTINATION_ID
        );
        assert_eq!(
            destinations[0].kind,
            IncidentMonitorDestinationKind::GithubIssue
        );
        assert_eq!(destinations[0].repo.as_deref(), Some("acme/platform"));
        assert_eq!(destinations[0].mcp_server.as_deref(), Some("github"));
        assert!(destinations[0].require_approval);
    }

    #[test]
    fn explicit_destinations_preserve_empty_default_destination_ids() {
        let config = IncidentMonitorConfig {
            destinations: vec![IncidentMonitorDestinationConfig {
                destination_id: "linear-prod".to_string(),
                name: "Linear".to_string(),
                kind: IncidentMonitorDestinationKind::LinearIssue,
                ..IncidentMonitorDestinationConfig::default()
            }],
            ..IncidentMonitorConfig::default()
        };

        assert_eq!(config.effective_destinations().len(), 1);
        assert!(config.effective_default_destination_ids().is_empty());
    }

    #[test]
    fn safety_context_fields_are_additive_for_legacy_records() {
        let incident: IncidentMonitorIncidentRecord = serde_json::from_value(json!({
            "incident_id": "incident-1",
            "fingerprint": "fingerprint-1",
            "event_type": "automation.failed",
            "status": "queued",
            "repo": "acme/platform",
            "workspace_root": "/tmp/platform",
            "title": "Workflow failed",
            "occurrence_count": 1,
            "created_at_ms": 1,
            "updated_at_ms": 1
        }))
        .expect("legacy incident record should deserialize");
        assert!(incident.actor.is_none());
        assert!(incident.risk_category.is_none());
        assert!(incident.external_correlation_ids.is_empty());

        let submission: IncidentMonitorSubmission = serde_json::from_value(json!({
            "actor": "agent:release",
            "model": "gpt-5",
            "tool_name": "slack.post_message",
            "action": "send_message",
            "policy": "approval.high_risk",
            "approval_state": "denied",
            "risk_category": "data_exfiltration",
            "blast_radius": "customer-visible channel",
            "external_correlation_ids": ["case-123"]
        }))
        .expect("submission safety context should deserialize");
        assert_eq!(submission.actor.as_deref(), Some("agent:release"));
        assert_eq!(
            submission.risk_category.as_deref(),
            Some("data_exfiltration")
        );
        assert_eq!(submission.external_correlation_ids, vec!["case-123"]);

        let route: IncidentMonitorRouteConfig = serde_json::from_value(json!({
            "route_id": "route-security",
            "name": "Security",
            "match_risk_categories": ["data_exfiltration"]
        }))
        .expect("route risk category matcher should deserialize");
        assert_eq!(route.match_risk_categories, vec!["data_exfiltration"]);
    }

    #[test]
    fn legacy_monitored_project_config_deserializes_with_default_source_binding() {
        let config: IncidentMonitorConfig = serde_json::from_value(json!({
            "monitored_projects": [{
                "project_id": "customer-api",
                "name": "Customer API",
                "repo": "acme/customer-api",
                "workspace_root": "/tmp/customer-api",
                "log_sources": [{
                    "source_id": "app-log",
                    "path": "logs/app.log"
                }]
            }]
        }))
        .expect("legacy monitored project config should deserialize");

        let project = &config.monitored_projects[0];
        let source = &project.log_sources[0];
        let binding = project.source_binding(Some(source));

        assert_eq!(
            binding.source_kind,
            IncidentMonitorSourceKind::CustomerSystem
        );
        assert!(binding.allowed_destination_ids.is_empty());
        assert!(binding.default_destination_ids.is_empty());
        assert!(binding.default_route_tags.is_empty());
        assert_eq!(
            binding.approval_policy,
            IncidentMonitorApprovalPolicy::Inherit
        );
    }

    #[test]
    fn source_binding_applies_source_overrides_and_intersects_allowlists() {
        let project = IncidentMonitorMonitoredProject {
            project_id: "payments".to_string(),
            name: "Payments".to_string(),
            repo: "acme/payments".to_string(),
            workspace_root: "/tmp/payments".to_string(),
            source_kind: IncidentMonitorSourceKind::ExternalApp,
            allowed_destination_ids: vec!["legacy-github".to_string(), "linear-prod".to_string()],
            default_destination_ids: vec!["legacy-github".to_string()],
            default_route_tags: vec!["payments".to_string()],
            tenant_id: Some("tenant-a".to_string()),
            approval_policy: IncidentMonitorApprovalPolicy::HighRisk,
            log_sources: vec![IncidentMonitorLogSource {
                source_id: "ci".to_string(),
                path: "logs/ci.jsonl".to_string(),
                source_kind: Some(IncidentMonitorSourceKind::Ci),
                allowed_destination_ids: vec!["linear-prod".to_string()],
                default_destination_ids: vec!["linear-prod".to_string()],
                default_route_tags: vec!["ci".to_string(), "payments".to_string()],
                workspace_id: Some("workspace-a".to_string()),
                approval_policy: IncidentMonitorApprovalPolicy::Always,
                ..IncidentMonitorLogSource::default()
            }],
            ..IncidentMonitorMonitoredProject::default()
        };

        let binding = project.source_binding(project.log_sources.first());

        assert_eq!(binding.source_kind, IncidentMonitorSourceKind::Ci);
        assert_eq!(
            binding.allowed_destination_ids,
            vec!["linear-prod".to_string()]
        );
        assert_eq!(
            binding.default_destination_ids,
            vec!["linear-prod".to_string()]
        );
        assert_eq!(
            binding.default_route_tags,
            vec!["payments".to_string(), "ci".to_string()]
        );
        assert_eq!(binding.tenant_id.as_deref(), Some("tenant-a"));
        assert_eq!(binding.workspace_id.as_deref(), Some("workspace-a"));
        assert_eq!(
            binding.approval_policy,
            IncidentMonitorApprovalPolicy::Always
        );
    }

    #[test]
    fn safety_defaults_are_fail_closed_for_high_risk_and_redaction() {
        let defaults = IncidentMonitorSafetyDefaults::default();

        assert!(defaults.require_approval_for_high_risk);
        assert!(defaults.redact_secrets);
        assert!(!defaults.block_unready_destinations);
        assert_eq!(defaults.retention_days, None);
    }

    #[test]
    fn legacy_post_records_deserialize_without_destination_receipts() {
        let post: IncidentMonitorPostRecord = serde_json::from_value(json!({
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
        .expect("legacy Incident Monitor post should deserialize");

        assert_eq!(post.issue_number, Some(42));
        assert_eq!(post.destination_id, None);
        assert_eq!(post.destination_kind, None);
        assert_eq!(post.external_url, None);
        assert_eq!(post.receipt, None);
    }
}
