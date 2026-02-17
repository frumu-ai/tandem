use serde::{Deserialize, Serialize};
use serde_json::Value;

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
