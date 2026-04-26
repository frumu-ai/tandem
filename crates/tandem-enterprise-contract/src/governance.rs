use std::collections::HashMap;

use chrono::{DateTime, Datelike, TimeZone, Utc};
use http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn resolve_bool_env(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else if trimmed.eq_ignore_ascii_case("true")
                || trimmed.eq_ignore_ascii_case("1")
                || trimmed.eq_ignore_ascii_case("yes")
                || trimmed.eq_ignore_ascii_case("on")
            {
                Some(true)
            } else if trimmed.eq_ignore_ascii_case("false")
                || trimmed.eq_ignore_ascii_case("0")
                || trimmed.eq_ignore_ascii_case("no")
                || trimmed.eq_ignore_ascii_case("off")
            {
                Some(false)
            } else {
                None
            }
        })
        .unwrap_or(default)
}

fn resolve_u64_env(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(default)
}

fn resolve_f64_env(name: &str, default: Option<f64>) -> Option<f64> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.trim().parse::<f64>().ok())
        .or(default)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceActorKind {
    Human,
    Agent,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GovernanceActorRef {
    pub kind: GovernanceActorKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

impl GovernanceActorRef {
    pub fn human(actor_id: Option<String>, source: impl Into<String>) -> Self {
        Self {
            kind: GovernanceActorKind::Human,
            actor_id,
            source: Some(source.into()),
        }
    }

    pub fn agent(actor_id: Option<String>, source: impl Into<String>) -> Self {
        Self {
            kind: GovernanceActorKind::Agent,
            actor_id,
            source: Some(source.into()),
        }
    }

    pub fn system(source: impl Into<String>) -> Self {
        Self {
            kind: GovernanceActorKind::System,
            actor_id: None,
            source: Some(source.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GovernanceLineageEntry {
    pub depth: u64,
    pub actor: GovernanceActorRef,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutomationDeclaredCapabilities {
    #[serde(default)]
    pub creates_agents: bool,
    #[serde(default)]
    pub modifies_grants: bool,
}

impl AutomationDeclaredCapabilities {
    pub fn from_metadata(metadata: Option<&Value>) -> Self {
        metadata
            .and_then(|metadata| metadata.get("capabilities"))
            .and_then(|value| serde_json::from_value::<Self>(value.clone()).ok())
            .unwrap_or_default()
    }

    pub fn escalates_from(&self, previous: &Self) -> Vec<&'static str> {
        let mut escalations = Vec::new();
        if self.creates_agents && !previous.creates_agents {
            escalations.push("creates_agents");
        }
        if self.modifies_grants && !previous.modifies_grants {
            escalations.push("modifies_grants");
        }
        escalations
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutomationProvenanceRecord {
    pub creator: GovernanceActorRef,
    pub root_actor: GovernanceActorRef,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ancestor_chain: Vec<GovernanceLineageEntry>,
    pub depth: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_source: Option<String>,
}

impl AutomationProvenanceRecord {
    pub fn human(actor_id: Option<String>, source: impl Into<String>) -> Self {
        let creator = GovernanceActorRef::human(actor_id.clone(), source.into());
        Self {
            root_actor: creator.clone(),
            creator,
            ancestor_chain: Vec::new(),
            depth: 0,
            request_source: None,
        }
    }

    pub fn agent(
        agent_id: Option<String>,
        root_actor: GovernanceActorRef,
        ancestor_chain: Vec<GovernanceLineageEntry>,
        request_source: impl Into<String>,
    ) -> Self {
        let depth = ancestor_chain
            .last()
            .map(|entry| entry.depth.saturating_add(1))
            .unwrap_or(1);
        Self {
            creator: GovernanceActorRef::agent(agent_id, request_source.into()),
            root_actor,
            ancestor_chain,
            depth,
            request_source: None,
        }
    }

    pub fn agent_lineage_ids(&self) -> Vec<String> {
        let mut ids = Vec::new();
        if self.creator.kind == GovernanceActorKind::Agent {
            if let Some(agent_id) = self.creator.actor_id.as_deref() {
                let agent_id = agent_id.trim();
                if !agent_id.is_empty() {
                    ids.push(agent_id.to_string());
                }
            }
        }
        for entry in &self.ancestor_chain {
            if entry.actor.kind != GovernanceActorKind::Agent {
                continue;
            }
            if let Some(agent_id) = entry.actor.actor_id.as_deref() {
                let agent_id = agent_id.trim();
                if !agent_id.is_empty() && !ids.iter().any(|value| value == agent_id) {
                    ids.push(agent_id.to_string());
                }
            }
        }
        ids
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationLifecycleReviewKind {
    CreationQuota,
    RunDrift,
    HealthDrift,
    ExpirationSoon,
    Expired,
    DependencyRevoked,
    Retired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationLifecycleFindingSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationLifecycleFinding {
    pub finding_id: String,
    pub kind: AutomationLifecycleReviewKind,
    pub severity: AutomationLifecycleFindingSeverity,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub observed_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentCreationReviewSummary {
    pub agent_id: String,
    #[serde(default)]
    pub created_since_review: u64,
    #[serde(default)]
    pub review_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_kind: Option<AutomationLifecycleReviewKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_requested_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reviewed_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_review_notes: Option<String>,
    #[serde(default)]
    pub updated_at_ms: u64,
}

impl AgentCreationReviewSummary {
    pub fn new(agent_id: impl Into<String>, now: u64) -> Self {
        Self {
            agent_id: agent_id.into(),
            created_since_review: 0,
            review_required: false,
            review_kind: None,
            review_requested_at_ms: None,
            review_request_id: None,
            last_reviewed_at_ms: None,
            last_review_notes: None,
            updated_at_ms: now,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpendWindowKind {
    Daily,
    Weekly,
    Monthly,
    Lifetime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpendWindowRecord {
    pub kind: SpendWindowKind,
    pub window_start_ms: u64,
    pub window_end_ms: u64,
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub cost_usd: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_automation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_run_id: Option<String>,
    #[serde(default)]
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soft_warning_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hard_stop_at_ms: Option<u64>,
}

impl Default for AgentSpendWindowRecord {
    fn default() -> Self {
        Self::new(SpendWindowKind::Lifetime, 0)
    }
}

impl AgentSpendWindowRecord {
    pub fn new(kind: SpendWindowKind, now_ms: u64) -> Self {
        let (window_start_ms, window_end_ms) = spend_window_bounds(kind, now_ms);
        Self {
            kind,
            window_start_ms,
            window_end_ms,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            cost_usd: 0.0,
            last_automation_id: None,
            last_run_id: None,
            updated_at_ms: now_ms,
            soft_warning_at_ms: None,
            hard_stop_at_ms: None,
        }
    }

    fn refresh(&mut self, now_ms: u64) {
        let (window_start_ms, window_end_ms) = spend_window_bounds(self.kind, now_ms);
        if self.window_start_ms != window_start_ms || self.window_end_ms != window_end_ms {
            *self = Self::new(self.kind, now_ms);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn apply_usage(
        &mut self,
        now_ms: u64,
        automation_id: Option<&str>,
        run_id: Option<&str>,
        prompt_tokens: u64,
        completion_tokens: u64,
        total_tokens: u64,
        cost_usd: f64,
    ) {
        self.refresh(now_ms);
        self.prompt_tokens = self.prompt_tokens.saturating_add(prompt_tokens);
        self.completion_tokens = self.completion_tokens.saturating_add(completion_tokens);
        self.total_tokens = self.total_tokens.saturating_add(total_tokens);
        self.cost_usd += cost_usd.max(0.0);
        self.last_automation_id = automation_id.map(|value| value.to_string());
        self.last_run_id = run_id.map(|value| value.to_string());
        self.updated_at_ms = now_ms;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpendSummary {
    pub agent_id: String,
    #[serde(default)]
    pub daily: AgentSpendWindowRecord,
    #[serde(default)]
    pub weekly: AgentSpendWindowRecord,
    #[serde(default)]
    pub monthly: AgentSpendWindowRecord,
    #[serde(default)]
    pub lifetime: AgentSpendWindowRecord,
    #[serde(default)]
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paused_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pause_reason: Option<String>,
}

impl Default for AgentSpendSummary {
    fn default() -> Self {
        Self::new("unknown".to_string(), 0)
    }
}

impl AgentSpendSummary {
    pub fn new(agent_id: impl Into<String>, now_ms: u64) -> Self {
        let agent_id = agent_id.into();
        Self {
            agent_id,
            daily: AgentSpendWindowRecord::new(SpendWindowKind::Daily, now_ms),
            weekly: AgentSpendWindowRecord::new(SpendWindowKind::Weekly, now_ms),
            monthly: AgentSpendWindowRecord::new(SpendWindowKind::Monthly, now_ms),
            lifetime: AgentSpendWindowRecord::new(SpendWindowKind::Lifetime, now_ms),
            updated_at_ms: now_ms,
            paused_at_ms: None,
            pause_reason: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn apply_usage(
        &mut self,
        now_ms: u64,
        automation_id: Option<&str>,
        run_id: Option<&str>,
        prompt_tokens: u64,
        completion_tokens: u64,
        total_tokens: u64,
        cost_usd: f64,
    ) {
        self.daily.apply_usage(
            now_ms,
            automation_id,
            run_id,
            prompt_tokens,
            completion_tokens,
            total_tokens,
            cost_usd,
        );
        self.weekly.apply_usage(
            now_ms,
            automation_id,
            run_id,
            prompt_tokens,
            completion_tokens,
            total_tokens,
            cost_usd,
        );
        self.monthly.apply_usage(
            now_ms,
            automation_id,
            run_id,
            prompt_tokens,
            completion_tokens,
            total_tokens,
            cost_usd,
        );
        self.lifetime.apply_usage(
            now_ms,
            automation_id,
            run_id,
            prompt_tokens,
            completion_tokens,
            total_tokens,
            cost_usd,
        );
        self.updated_at_ms = now_ms;
    }

    pub fn weekly_warning_threshold_reached(&self, limit_usd: f64, threshold_ratio: f64) -> bool {
        limit_usd > 0.0
            && threshold_ratio > 0.0
            && self.weekly.cost_usd >= (limit_usd * threshold_ratio)
    }

    pub fn weekly_limit_reached(&self, limit_usd: f64) -> bool {
        limit_usd > 0.0 && self.weekly.cost_usd >= limit_usd
    }
}

fn spend_window_bounds(kind: SpendWindowKind, now_ms: u64) -> (u64, u64) {
    let dt = Utc
        .timestamp_millis_opt(now_ms as i64)
        .single()
        .unwrap_or_else(|| DateTime::<Utc>::from_timestamp_millis(0).expect("unix epoch"));
    match kind {
        SpendWindowKind::Lifetime => (0, u64::MAX),
        SpendWindowKind::Daily => {
            let start = Utc
                .with_ymd_and_hms(dt.year(), dt.month(), dt.day(), 0, 0, 0)
                .single()
                .unwrap_or(dt);
            let end = start + chrono::Duration::days(1);
            (
                start.timestamp_millis().max(0) as u64,
                end.timestamp_millis().max(0) as u64,
            )
        }
        SpendWindowKind::Weekly => {
            let start_of_day = Utc
                .with_ymd_and_hms(dt.year(), dt.month(), dt.day(), 0, 0, 0)
                .single()
                .unwrap_or(dt);
            let offset = i64::from(dt.weekday().num_days_from_monday());
            let start = start_of_day - chrono::Duration::days(offset);
            let end = start + chrono::Duration::days(7);
            (
                start.timestamp_millis().max(0) as u64,
                end.timestamp_millis().max(0) as u64,
            )
        }
        SpendWindowKind::Monthly => {
            let start = Utc
                .with_ymd_and_hms(dt.year(), dt.month(), 1, 0, 0, 0)
                .single()
                .unwrap_or(dt);
            let end = if dt.month() == 12 {
                Utc.with_ymd_and_hms(dt.year() + 1, 1, 1, 0, 0, 0)
                    .single()
                    .unwrap_or(start)
            } else {
                Utc.with_ymd_and_hms(dt.year(), dt.month() + 1, 1, 0, 0, 0)
                    .single()
                    .unwrap_or(start + chrono::Duration::days(31))
            };
            (
                start.timestamp_millis().max(0) as u64,
                end.timestamp_millis().max(0) as u64,
            )
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationGrantKind {
    Modify,
    Capability,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationGrantRecord {
    pub grant_id: String,
    pub automation_id: String,
    pub grant_kind: AutomationGrantKind,
    pub granted_to: GovernanceActorRef,
    pub granted_by: GovernanceActorRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_key: Option<String>,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoked_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revoke_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationGovernanceRecord {
    pub automation_id: String,
    pub provenance: AutomationProvenanceRecord,
    #[serde(default)]
    pub declared_capabilities: AutomationDeclaredCapabilities,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modify_grants: Vec<AutomationGrantRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capability_grants: Vec<AutomationGrantRecord>,
    #[serde(default)]
    pub created_at_ms: u64,
    #[serde(default)]
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deleted_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delete_retention_until_ms: Option<u64>,
    #[serde(default)]
    pub published_externally: bool,
    #[serde(default)]
    pub creation_paused: bool,
    #[serde(default)]
    pub review_required: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_kind: Option<AutomationLifecycleReviewKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_requested_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_reviewed_at_ms: Option<u64>,
    #[serde(default)]
    pub runs_since_review: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expired_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retire_reason: Option<String>,
    #[serde(default)]
    pub paused_for_lifecycle: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_last_checked_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub health_findings: Vec<AutomationLifecycleFinding>,
}

impl AutomationGovernanceRecord {
    pub fn created_by_agent(&self) -> Option<String> {
        if self.provenance.creator.kind != GovernanceActorKind::Agent {
            return None;
        }
        self.provenance.creator.actor_id.clone()
    }

    pub fn has_modify_grant(&self, actor_id: &str) -> bool {
        self.modify_grants.iter().any(|grant| {
            grant.revoked_at_ms.is_none()
                && grant
                    .granted_to
                    .actor_id
                    .as_deref()
                    .is_some_and(|value| value == actor_id)
        })
    }

    pub fn has_capability_grant(&self, actor_id: &str) -> bool {
        self.capability_grants.iter().any(|grant| {
            grant.revoked_at_ms.is_none()
                && grant
                    .granted_to
                    .actor_id
                    .as_deref()
                    .is_some_and(|value| value == actor_id)
        })
    }

    pub fn agent_lineage_ids(&self) -> Vec<String> {
        self.provenance.agent_lineage_ids()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceApprovalRequestType {
    CapabilityRequest,
    ExternalPost,
    QuotaOverride,
    LifecycleReview,
    ElevatedCapability,
    DepthOverride,
    RetirementAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceApprovalStatus {
    Pending,
    Approved,
    Denied,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceResourceRef {
    #[serde(rename = "type")]
    pub resource_type: String,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceApprovalRequest {
    pub approval_id: String,
    pub request_type: GovernanceApprovalRequestType,
    pub requested_by: GovernanceActorRef,
    pub target_resource: GovernanceResourceRef,
    pub rationale: String,
    #[serde(default)]
    pub context: Value,
    pub status: GovernanceApprovalStatus,
    pub expires_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_by: Option<GovernanceActorRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_notes: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceLimits {
    pub creation_enabled: bool,
    pub per_agent_daily_creation_limit: u64,
    pub active_agent_automation_cap: u64,
    pub lineage_depth_limit: u64,
    #[serde(default)]
    pub per_agent_creation_review_threshold: u64,
    #[serde(default)]
    pub run_review_threshold: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weekly_spend_cap_usd: Option<f64>,
    #[serde(default)]
    pub spend_warning_threshold_ratio: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_spike_multiplier: Option<f64>,
    #[serde(default)]
    pub default_expires_after_ms: u64,
    #[serde(default)]
    pub expiration_warning_window_ms: u64,
    #[serde(default)]
    pub health_check_interval_ms: u64,
    #[serde(default)]
    pub health_window_run_limit: u64,
    #[serde(default)]
    pub health_failure_rate_threshold: f64,
    #[serde(default)]
    pub health_guardrail_stop_threshold: u64,
    pub approval_ttl_ms: u64,
    pub per_agent_pause_enabled: bool,
}

impl Default for GovernanceLimits {
    fn default() -> Self {
        Self {
            creation_enabled: resolve_bool_env("TANDEM_AGENT_AUTOMATION_CREATION_ENABLED", true),
            per_agent_daily_creation_limit: resolve_u64_env(
                "TANDEM_AGENT_AUTOMATION_CREATION_DAILY_LIMIT",
                10,
            ),
            active_agent_automation_cap: resolve_u64_env("TANDEM_AGENT_AUTOMATION_ACTIVE_CAP", 50),
            lineage_depth_limit: resolve_u64_env("TANDEM_AGENT_AUTOMATION_DEPTH_LIMIT", 3),
            per_agent_creation_review_threshold: resolve_u64_env(
                "TANDEM_AGENT_AUTOMATION_CREATION_REVIEW_THRESHOLD",
                5,
            ),
            run_review_threshold: resolve_u64_env(
                "TANDEM_AGENT_AUTOMATION_RUN_REVIEW_THRESHOLD",
                20,
            ),
            weekly_spend_cap_usd: resolve_f64_env(
                "TANDEM_AGENT_AUTOMATION_WEEKLY_SPEND_CAP_USD",
                None,
            ),
            spend_warning_threshold_ratio: resolve_f64_env(
                "TANDEM_AGENT_AUTOMATION_SPEND_WARNING_THRESHOLD_RATIO",
                Some(0.8),
            )
            .unwrap_or(0.8),
            cost_spike_multiplier: resolve_f64_env(
                "TANDEM_AGENT_AUTOMATION_COST_SPIKE_MULTIPLIER",
                Some(10.0),
            ),
            default_expires_after_ms: resolve_u64_env(
                "TANDEM_AGENT_AUTOMATION_DEFAULT_EXPIRES_AFTER_MS",
                90 * 24 * 60 * 60 * 1000,
            ),
            expiration_warning_window_ms: resolve_u64_env(
                "TANDEM_AGENT_AUTOMATION_EXPIRATION_WARNING_WINDOW_MS",
                7 * 24 * 60 * 60 * 1000,
            ),
            health_check_interval_ms: resolve_u64_env(
                "TANDEM_AGENT_AUTOMATION_HEALTH_CHECK_INTERVAL_MS",
                6 * 60 * 60 * 1000,
            ),
            health_window_run_limit: resolve_u64_env(
                "TANDEM_AGENT_AUTOMATION_HEALTH_WINDOW_RUN_LIMIT",
                20,
            ),
            health_failure_rate_threshold: resolve_f64_env(
                "TANDEM_AGENT_AUTOMATION_HEALTH_FAILURE_RATE_THRESHOLD",
                Some(0.5),
            )
            .unwrap_or(0.5),
            health_guardrail_stop_threshold: resolve_u64_env(
                "TANDEM_AGENT_AUTOMATION_HEALTH_GUARDRAIL_STOP_THRESHOLD",
                2,
            ),
            approval_ttl_ms: resolve_u64_env(
                "TANDEM_AGENT_AUTOMATION_APPROVAL_TTL_MS",
                72 * 60 * 60 * 1000,
            ),
            per_agent_pause_enabled: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GovernanceContextSnapshot {
    #[serde(default)]
    pub records: HashMap<String, AutomationGovernanceRecord>,
    #[serde(default)]
    pub approvals: HashMap<String, GovernanceApprovalRequest>,
    #[serde(default)]
    pub paused_agents: Vec<String>,
    #[serde(default)]
    pub spend_paused_agents: Vec<String>,
    #[serde(default)]
    pub agent_spend: HashMap<String, AgentSpendSummary>,
    #[serde(default)]
    pub agent_creation_reviews: HashMap<String, AgentCreationReviewSummary>,
    #[serde(default)]
    pub limits: GovernanceLimits,
}

impl GovernanceContextSnapshot {
    pub fn is_agent_paused(&self, actor_id: &str) -> bool {
        self.paused_agents.iter().any(|value| value == actor_id)
    }

    pub fn is_agent_spend_paused(&self, actor_id: &str) -> bool {
        self.spend_paused_agents
            .iter()
            .any(|value| value == actor_id)
    }

    pub fn has_approved_agent_capability(
        &self,
        agent_id: &str,
        capability_key: &str,
        now_ms: u64,
    ) -> bool {
        self.approvals.values().any(|request| {
            request.request_type == GovernanceApprovalRequestType::CapabilityRequest
                && request.status == GovernanceApprovalStatus::Approved
                && request.expires_at_ms > now_ms
                && request.target_resource.resource_type == "agent"
                && request.target_resource.id == agent_id
                && request
                    .context
                    .get("capability_key")
                    .or_else(|| request.context.get("capability"))
                    .and_then(|value| value.as_str())
                    .is_some_and(|value| value == capability_key)
        })
    }

    pub fn has_approved_agent_quota_override(&self, agent_id: &str, now_ms: u64) -> bool {
        self.approvals.values().any(|request| {
            request.request_type == GovernanceApprovalRequestType::QuotaOverride
                && request.status == GovernanceApprovalStatus::Approved
                && request.expires_at_ms > now_ms
                && request.target_resource.resource_type == "agent"
                && request.target_resource.id == agent_id
        })
    }

    pub fn has_pending_agent_quota_override(&self, agent_id: &str, now_ms: u64) -> bool {
        self.approvals.values().any(|request| {
            request.request_type == GovernanceApprovalRequestType::QuotaOverride
                && request.status == GovernanceApprovalStatus::Pending
                && request.expires_at_ms > now_ms
                && request.target_resource.resource_type == "agent"
                && request.target_resource.id == agent_id
        })
    }

    pub fn has_pending_approval_request(
        &self,
        request_type: GovernanceApprovalRequestType,
        resource_type: &str,
        resource_id: &str,
        now_ms: u64,
    ) -> bool {
        self.approvals.values().any(|request| {
            request.request_type == request_type
                && request.status == GovernanceApprovalStatus::Pending
                && request.expires_at_ms > now_ms
                && request.target_resource.resource_type == resource_type
                && request.target_resource.id == resource_id
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GovernanceAction {
    Create,
    Modify,
    Delete,
    Run,
    Pause,
    Resume,
    GrantModify,
    GrantCapability,
    RevokeGrant,
    Approve,
    Deny,
}

#[derive(Debug, Clone)]
pub struct GovernanceError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
}

impl GovernanceError {
    pub fn forbidden(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            code,
            message: message.into(),
        }
    }

    pub fn too_many_requests(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            code,
            message: message.into(),
        }
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            code,
            message: message.into(),
        }
    }

    pub fn feature_unavailable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_IMPLEMENTED,
            code: "PREMIUM_FEATURE_REQUIRED",
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GovernanceApprovalDraftInput {
    pub request_type: GovernanceApprovalRequestType,
    pub requested_by: GovernanceActorRef,
    pub target_resource: GovernanceResourceRef,
    pub rationale: String,
    pub context: Value,
    pub expires_at_ms: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct GovernanceCreationReviewEvaluation {
    pub summary: AgentCreationReviewSummary,
    pub approval_request: Option<GovernanceApprovalRequest>,
}

#[derive(Debug, Clone)]
pub struct GovernanceAutomationReviewEvaluation {
    pub record: AutomationGovernanceRecord,
    pub approval_request: Option<GovernanceApprovalRequest>,
}

#[derive(Debug, Clone)]
pub struct GovernanceHealthCheckInput {
    pub automation_id: String,
    pub current_record: Option<AutomationGovernanceRecord>,
    pub default_provenance: AutomationProvenanceRecord,
    pub declared_capabilities: AutomationDeclaredCapabilities,
    pub terminal_run_count: u64,
    pub failure_count: u64,
    pub empty_output_count: u64,
    pub guardrail_stop_count: u64,
    pub last_terminal_run_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GovernanceHealthCheckEvaluation {
    pub record: AutomationGovernanceRecord,
    pub approval_requests: Vec<GovernanceApprovalRequest>,
    pub pause_automation: bool,
}

#[derive(Debug, Clone)]
pub struct GovernanceDependencyRevocationInput {
    pub automation_id: String,
    pub current_record: Option<AutomationGovernanceRecord>,
    pub default_provenance: AutomationProvenanceRecord,
    pub declared_capabilities: AutomationDeclaredCapabilities,
    pub reason: String,
    pub evidence: Value,
}

#[derive(Debug, Clone)]
pub struct GovernanceRetirementInput {
    pub automation_id: String,
    pub current_record: Option<AutomationGovernanceRecord>,
    pub default_provenance: AutomationProvenanceRecord,
    pub declared_capabilities: AutomationDeclaredCapabilities,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct GovernanceRetirementExtensionInput {
    pub automation_id: String,
    pub current_record: Option<AutomationGovernanceRecord>,
    pub default_provenance: AutomationProvenanceRecord,
    pub declared_capabilities: AutomationDeclaredCapabilities,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone)]
pub struct GovernanceSpendInput {
    pub automation_id: String,
    pub run_id: String,
    pub agent_ids: Vec<String>,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub delta_cost_usd: f64,
}

#[derive(Debug, Clone)]
pub struct GovernanceSpendWarningRecord {
    pub agent_id: String,
    pub weekly_cost_usd: f64,
    pub weekly_spend_cap_usd: f64,
}

#[derive(Debug, Clone)]
pub struct GovernanceSpendHardStopRecord {
    pub agent_id: String,
    pub weekly_cost_usd: f64,
    pub weekly_spend_cap_usd: f64,
}

#[derive(Debug, Clone, Default)]
pub struct GovernanceSpendEvaluation {
    pub updated_summaries: Vec<AgentSpendSummary>,
    pub warnings: Vec<GovernanceSpendWarningRecord>,
    pub hard_stops: Vec<GovernanceSpendHardStopRecord>,
    pub approvals: Vec<GovernanceApprovalRequest>,
    pub spend_paused_agents: Vec<String>,
}

pub trait GovernancePolicyEngine: Send + Sync {
    fn premium_enabled(&self) -> bool;

    fn authorize_create(
        &self,
        snapshot: &GovernanceContextSnapshot,
        actor: &GovernanceActorRef,
        provenance: &AutomationProvenanceRecord,
        declared_capabilities: &AutomationDeclaredCapabilities,
        now_ms: u64,
    ) -> Result<(), GovernanceError>;

    fn authorize_capability_escalation(
        &self,
        snapshot: &GovernanceContextSnapshot,
        actor: &GovernanceActorRef,
        previous: &AutomationDeclaredCapabilities,
        next: &AutomationDeclaredCapabilities,
        now_ms: u64,
    ) -> Result<(), GovernanceError>;

    fn authorize_mutation(
        &self,
        record: &AutomationGovernanceRecord,
        actor: &GovernanceActorRef,
        destructive: bool,
    ) -> Result<(), GovernanceError>;

    fn create_approval_request(
        &self,
        snapshot: &GovernanceContextSnapshot,
        input: GovernanceApprovalDraftInput,
        now_ms: u64,
    ) -> Result<GovernanceApprovalRequest, GovernanceError>;

    fn decide_approval_request(
        &self,
        existing: &GovernanceApprovalRequest,
        reviewer: GovernanceActorRef,
        approved: bool,
        notes: Option<String>,
        now_ms: u64,
    ) -> Result<GovernanceApprovalRequest, GovernanceError>;

    fn evaluate_creation_review_progress(
        &self,
        snapshot: &GovernanceContextSnapshot,
        agent_id: &str,
        automation_id: &str,
        now_ms: u64,
    ) -> Result<GovernanceCreationReviewEvaluation, GovernanceError>;

    fn evaluate_run_review_progress(
        &self,
        snapshot: &GovernanceContextSnapshot,
        automation_id: &str,
        reason: AutomationLifecycleReviewKind,
        run_id: Option<String>,
        detail: Option<String>,
        now_ms: u64,
    ) -> Result<Option<GovernanceAutomationReviewEvaluation>, GovernanceError>;

    fn evaluate_health_check(
        &self,
        snapshot: &GovernanceContextSnapshot,
        input: GovernanceHealthCheckInput,
        now_ms: u64,
    ) -> Result<Option<GovernanceHealthCheckEvaluation>, GovernanceError>;

    fn evaluate_dependency_revocation(
        &self,
        snapshot: &GovernanceContextSnapshot,
        input: GovernanceDependencyRevocationInput,
        now_ms: u64,
    ) -> Result<GovernanceAutomationReviewEvaluation, GovernanceError>;

    fn evaluate_retirement(
        &self,
        input: GovernanceRetirementInput,
        now_ms: u64,
    ) -> Result<AutomationGovernanceRecord, GovernanceError>;

    fn evaluate_retirement_extension(
        &self,
        input: GovernanceRetirementExtensionInput,
        now_ms: u64,
    ) -> Result<AutomationGovernanceRecord, GovernanceError>;

    fn evaluate_spend_usage(
        &self,
        snapshot: &GovernanceContextSnapshot,
        input: &GovernanceSpendInput,
        now_ms: u64,
    ) -> Result<GovernanceSpendEvaluation, GovernanceError>;
}
