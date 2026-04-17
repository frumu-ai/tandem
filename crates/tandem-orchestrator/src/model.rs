use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeScope {
    Run,
    #[default]
    Project,
    Global,
}

impl std::fmt::Display for KnowledgeScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Run => write!(f, "run"),
            Self::Project => write!(f, "project"),
            Self::Global => write!(f, "global"),
        }
    }
}

impl std::str::FromStr for KnowledgeScope {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "run" => Ok(Self::Run),
            "project" => Ok(Self::Project),
            "global" => Ok(Self::Global),
            other => Err(format!("unknown knowledge scope: {}", other)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeTrustLevel {
    Working,
    #[default]
    Promoted,
    ApprovedDefault,
}

impl std::fmt::Display for KnowledgeTrustLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Working => write!(f, "working"),
            Self::Promoted => write!(f, "promoted"),
            Self::ApprovedDefault => write!(f, "approved_default"),
        }
    }
}

impl std::str::FromStr for KnowledgeTrustLevel {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "working" => Ok(Self::Working),
            "promoted" => Ok(Self::Promoted),
            "approved_default" => Ok(Self::ApprovedDefault),
            other => Err(format!("unknown knowledge trust level: {}", other)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeReuseMode {
    Disabled,
    #[default]
    Preflight,
    OnDemand,
}

impl std::fmt::Display for KnowledgeReuseMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => write!(f, "disabled"),
            Self::Preflight => write!(f, "preflight"),
            Self::OnDemand => write!(f, "on_demand"),
        }
    }
}

impl std::str::FromStr for KnowledgeReuseMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "disabled" => Ok(Self::Disabled),
            "preflight" => Ok(Self::Preflight),
            "on_demand" => Ok(Self::OnDemand),
            other => Err(format!("unknown knowledge reuse mode: {}", other)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeSpaceRef {
    #[serde(default)]
    pub scope: KnowledgeScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub space_id: Option<String>,
}

impl Default for KnowledgeSpaceRef {
    fn default() -> Self {
        Self {
            scope: KnowledgeScope::Project,
            project_id: None,
            namespace: None,
            space_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeBinding {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub reuse_mode: KnowledgeReuseMode,
    #[serde(default)]
    pub trust_floor: KnowledgeTrustLevel,
    #[serde(default)]
    pub read_spaces: Vec<KnowledgeSpaceRef>,
    #[serde(default)]
    pub promote_spaces: Vec<KnowledgeSpaceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_ms: Option<u64>,
}

impl Default for KnowledgeBinding {
    fn default() -> Self {
        Self {
            enabled: true,
            reuse_mode: KnowledgeReuseMode::Preflight,
            trust_floor: KnowledgeTrustLevel::Promoted,
            read_spaces: Vec::new(),
            promote_spaces: Vec::new(),
            namespace: None,
            subject: None,
            freshness_ms: None,
        }
    }
}

impl KnowledgeTrustLevel {
    pub fn rank(self) -> u8 {
        match self {
            Self::Working => 0,
            Self::Promoted => 1,
            Self::ApprovedDefault => 2,
        }
    }

    pub fn meets_floor(self, floor: KnowledgeTrustLevel) -> bool {
        self.rank() >= floor.rank()
    }
}

fn truncate_segment(mut value: String, max_len: usize) -> String {
    if value.len() > max_len {
        value.truncate(max_len);
        value = value.trim_end_matches('-').to_string();
    }
    value
}

/// Normalize a knowledge segment (namespace or task family) into a stable, URL-ish key.
pub fn normalize_knowledge_segment(segment: &str) -> String {
    let trimmed = segment.trim().to_lowercase();
    let mut buf = String::with_capacity(trimmed.len());
    let mut prev_dash = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            buf.push(ch);
            prev_dash = false;
        } else if (ch == '_'
            || ch == '-'
            || ch == '/'
            || ch.is_whitespace()
            || ch.is_ascii_punctuation())
            && !prev_dash
        {
            buf.push('-');
            prev_dash = true;
        }
    }
    let normalized = buf.trim_matches('-').to_string();
    truncate_segment(normalized, 64)
}

/// Normalize a subject into a stable key, collapsing noise without losing intent.
pub fn normalize_knowledge_subject(subject: &str) -> String {
    let mut trimmed = subject.trim().to_lowercase();
    trimmed = trimmed
        .trim_matches('"')
        .trim_matches('\'')
        .trim_matches('`')
        .to_string();
    let mut buf = String::with_capacity(trimmed.len());
    let mut prev_sep = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            buf.push(ch);
            prev_sep = false;
        } else if (ch == '_' || ch == '-' || ch.is_whitespace() || ch.is_ascii_punctuation())
            && !prev_sep
        {
            buf.push('-');
            prev_sep = true;
        }
    }
    let normalized = buf.trim_matches('-').to_string();
    truncate_segment(normalized, 120)
}

/// Build a deterministic coverage key: project_id + namespace + task_family + normalized_subject.
pub fn build_knowledge_coverage_key(
    project_id: &str,
    namespace: Option<&str>,
    task_family: &str,
    subject: &str,
) -> String {
    let project = normalize_knowledge_segment(project_id);
    let project = if project.is_empty() {
        "unknown-project".to_string()
    } else {
        project
    };
    let namespace = namespace
        .map(normalize_knowledge_segment)
        .filter(|value| !value.is_empty());
    let task_family = normalize_knowledge_segment(task_family);
    let task_family = if task_family.is_empty() {
        "general".to_string()
    } else {
        task_family
    };
    let subject = normalize_knowledge_subject(subject);
    let subject = if subject.is_empty() {
        "unspecified".to_string()
    } else {
        subject
    };
    match namespace {
        Some(namespace) => format!(
            "{project}::{namespace}::{task_family}::{subject}",
            project = project,
            namespace = namespace,
            task_family = task_family,
            subject = subject
        ),
        None => format!(
            "{project}::{task_family}::{subject}",
            project = project,
            task_family = task_family,
            subject = subject
        ),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeReuseDecision {
    Disabled,
    NoPriorKnowledge,
    ReusePromoted,
    ReuseApprovedDefault,
    RefreshRequired,
}

impl std::fmt::Display for KnowledgeReuseDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => write!(f, "disabled"),
            Self::NoPriorKnowledge => write!(f, "no_prior_knowledge"),
            Self::ReusePromoted => write!(f, "reuse_promoted"),
            Self::ReuseApprovedDefault => write!(f, "reuse_approved_default"),
            Self::RefreshRequired => write!(f, "refresh_required"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgePreflightRequest {
    pub project_id: String,
    pub task_family: String,
    pub subject: String,
    #[serde(default)]
    pub binding: KnowledgeBinding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgePackItem {
    pub item_id: String,
    pub space_id: String,
    pub coverage_key: String,
    pub title: String,
    pub summary: Option<String>,
    pub trust_level: KnowledgeTrustLevel,
    pub status: String,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default)]
    pub source_memory_ids: Vec<String>,
    pub freshness_expires_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgePreflightResult {
    pub project_id: String,
    pub namespace: Option<String>,
    pub task_family: String,
    pub subject: String,
    pub coverage_key: String,
    pub decision: KnowledgeReuseDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reuse_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_reason: Option<String>,
    #[serde(default)]
    pub items: Vec<KnowledgePackItem>,
}

impl KnowledgePreflightResult {
    pub fn is_reusable(&self) -> bool {
        matches!(
            self.decision,
            KnowledgeReuseDecision::ReusePromoted | KnowledgeReuseDecision::ReuseApprovedDefault
        ) && !self.items.is_empty()
    }

    pub fn format_for_injection(&self) -> String {
        if self.items.is_empty() {
            return String::new();
        }

        let mut lines = vec![format!(
            "Knowledge preflight: decision={} coverage_key={} reuse_reason={} skip_reason={} freshness_reason={}",
            self.decision,
            self.coverage_key,
            self.reuse_reason.as_deref().unwrap_or("none"),
            self.skip_reason.as_deref().unwrap_or("none"),
            self.freshness_reason.as_deref().unwrap_or("none")
        )];

        for item in &self.items {
            let summary = item.summary.as_deref().unwrap_or("no summary");
            lines.push(format!(
                "- [{} / {}] {} :: {}",
                item.trust_level, item.status, item.title, summary
            ));
        }

        format!(
            "<knowledge_context>\n{}\n</knowledge_context>",
            lines.join("\n")
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    Draft,
    Running,
    Paused,
    Succeeded,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MissionBudget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_steps: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tool_calls: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MissionCapabilities {
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub allowed_agents: Vec<String>,
    #[serde(default)]
    pub allowed_memory_tiers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionSpec {
    pub mission_id: String,
    pub title: String,
    pub goal: String,
    #[serde(default)]
    pub knowledge: KnowledgeBinding,
    #[serde(default)]
    pub success_criteria: Vec<String>,
    #[serde(default)]
    pub entrypoint: Option<String>,
    #[serde(default)]
    pub budgets: MissionBudget,
    #[serde(default)]
    pub capabilities: MissionCapabilities,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

impl MissionSpec {
    pub fn new(title: impl Into<String>, goal: impl Into<String>) -> Self {
        Self {
            mission_id: uuid::Uuid::new_v4().to_string(),
            title: title.into(),
            goal: goal.into(),
            knowledge: KnowledgeBinding::default(),
            success_criteria: Vec::new(),
            entrypoint: None,
            budgets: MissionBudget::default(),
            capabilities: MissionCapabilities::default(),
            metadata: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemStatus {
    Todo,
    InProgress,
    Blocked,
    Review,
    Test,
    Rework,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkItem {
    pub work_item_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub status: WorkItemStatus,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionState {
    pub mission_id: String,
    pub status: MissionStatus,
    pub spec: MissionSpec,
    #[serde(default)]
    pub work_items: Vec<WorkItem>,
    pub revision: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MissionEvent {
    MissionStarted {
        mission_id: String,
    },
    MissionPaused {
        mission_id: String,
        reason: String,
    },
    MissionResumed {
        mission_id: String,
    },
    MissionCanceled {
        mission_id: String,
        reason: String,
    },
    RunStarted {
        mission_id: String,
        work_item_id: String,
        run_id: String,
    },
    RunFinished {
        mission_id: String,
        work_item_id: String,
        run_id: String,
        status: String,
    },
    ToolObserved {
        mission_id: String,
        run_id: String,
        tool: String,
        phase: String,
    },
    ApprovalGranted {
        mission_id: String,
        work_item_id: String,
        approval_id: String,
    },
    ApprovalDenied {
        mission_id: String,
        work_item_id: String,
        approval_id: String,
        reason: String,
    },
    TimerFired {
        mission_id: String,
        timer_id: String,
    },
    ResourceChanged {
        mission_id: String,
        key: String,
        rev: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MissionCommand {
    StartRun {
        mission_id: String,
        work_item_id: String,
        agent: Option<String>,
        prompt: String,
    },
    RequestApproval {
        mission_id: String,
        work_item_id: String,
        kind: String,
        summary: String,
    },
    PersistArtifact {
        mission_id: String,
        work_item_id: String,
        artifact_ref: String,
        metadata: Option<Value>,
    },
    ScheduleTimer {
        mission_id: String,
        timer_id: String,
        due_at_ms: u64,
    },
    EmitNotice {
        mission_id: String,
        event_type: String,
        properties: Value,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mission_spec_defaults_to_project_promoted_preflight_knowledge() {
        let spec = MissionSpec::new("Title", "Goal");
        assert!(spec.knowledge.enabled);
        assert_eq!(spec.knowledge.reuse_mode, KnowledgeReuseMode::Preflight);
        assert_eq!(spec.knowledge.trust_floor, KnowledgeTrustLevel::Promoted);
        assert_eq!(spec.knowledge.read_spaces.len(), 0);
        assert_eq!(spec.knowledge.promote_spaces.len(), 0);
    }

    #[test]
    fn normalize_knowledge_subject_collapses_whitespace_and_case() {
        let subject = "  Shipping   Delay  Analysis  ";
        assert_eq!(
            normalize_knowledge_subject(subject),
            "shipping-delay-analysis"
        );
    }

    #[test]
    fn normalize_knowledge_subject_strips_quotes_and_punct() {
        let subject = "\"Deploy: plan (v2)\"";
        assert_eq!(normalize_knowledge_subject(subject), "deploy-plan-v2");
    }

    #[test]
    fn normalize_knowledge_segment_is_stable() {
        let segment = "Marketing/Positioning v2";
        assert_eq!(
            normalize_knowledge_segment(segment),
            "marketing-positioning-v2"
        );
    }

    #[test]
    fn build_coverage_key_is_stable_for_equivalent_inputs() {
        let first = build_knowledge_coverage_key(
            "Project-1",
            Some("Marketing / Positioning"),
            "Strategy",
            "Pricing Plan",
        );
        let second = build_knowledge_coverage_key(
            "project 1",
            Some("marketing/positioning"),
            "strategy",
            "pricing  plan",
        );
        assert_eq!(first, second);
    }

    #[test]
    fn build_coverage_key_differs_for_subjects() {
        let first =
            build_knowledge_coverage_key("project-1", Some("support"), "triage", "timeout error");
        let second =
            build_knowledge_coverage_key("project-1", Some("support"), "triage", "latency spike");
        assert_ne!(first, second);
    }

    #[test]
    fn build_coverage_key_differs_for_projects() {
        let first =
            build_knowledge_coverage_key("project-1", Some("support"), "triage", "timeout error");
        let second =
            build_knowledge_coverage_key("project-2", Some("support"), "triage", "timeout error");
        assert_ne!(first, second);
    }

    #[test]
    fn build_coverage_key_handles_empty_namespace() {
        let key = build_knowledge_coverage_key("project-1", None, "triage", "timeout error");
        assert_eq!(key, "project-1::triage::timeout-error");
    }
}
