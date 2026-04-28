use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tandem_types::TenantContext;

#[derive(Debug, Deserialize, Default, Clone, Copy)]
pub(super) struct ContextRunReplayQuery {
    pub(super) upto_seq: Option<u64>,
    pub(super) from_checkpoint: Option<bool>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub(super) struct ContextRunListQuery {
    pub(super) workspace: Option<String>,
    pub(super) run_type: Option<String>,
    pub(super) limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ContextRunStatus {
    Queued,
    Planning,
    Running,
    AwaitingApproval,
    Paused,
    Blocked,
    Failed,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum ContextStepStatus {
    Pending,
    Runnable,
    InProgress,
    Blocked,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct ContextWorkspaceLease {
    pub(super) workspace_id: String,
    pub(super) canonical_path: String,
    pub(super) lease_epoch: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ContextRunStep {
    pub(super) step_id: String,
    pub(super) title: String,
    pub(super) status: ContextStepStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ContextRunState {
    pub(super) run_id: String,
    pub(super) run_type: String,
    #[serde(default = "default_tenant_context")]
    pub(super) tenant_context: TenantContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) source_client: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) model_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) model_id: Option<String>,
    #[serde(default)]
    pub(super) mcp_servers: Vec<String>,
    pub(super) status: ContextRunStatus,
    pub(super) objective: String,
    pub(super) workspace: ContextWorkspaceLease,
    #[serde(default)]
    pub(super) steps: Vec<ContextRunStep>,
    #[serde(default)]
    pub(super) tasks: Vec<ContextBlackboardTask>,
    #[serde(default)]
    pub(super) why_next_step: Option<String>,
    pub(super) revision: u64,
    #[serde(default)]
    pub(super) last_event_seq: u64,
    pub(super) created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) ended_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) last_error: Option<String>,
    pub(super) updated_at_ms: u64,
}

fn default_tenant_context() -> TenantContext {
    TenantContext::local_implicit()
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ContextRunCreateInput {
    pub(super) run_id: Option<String>,
    pub(super) objective: String,
    pub(super) run_type: Option<String>,
    pub(super) workspace: Option<ContextWorkspaceLease>,
    pub(super) source_client: Option<String>,
    #[serde(alias = "modelProvider")]
    pub(super) model_provider: Option<String>,
    #[serde(alias = "modelId")]
    pub(super) model_id: Option<String>,
    #[serde(alias = "mcpServers")]
    pub(super) mcp_servers: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ContextRunEventRecord {
    pub(super) event_id: String,
    pub(super) run_id: String,
    #[serde(alias = "event_seq")]
    pub(super) seq: u64,
    pub(super) ts_ms: u64,
    #[serde(rename = "type")]
    pub(super) event_type: String,
    pub(super) status: ContextRunStatus,
    #[serde(default)]
    pub(super) revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) step_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) command_id: Option<String>,
    pub(super) payload: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ContextRunEventAppendInput {
    #[serde(rename = "type")]
    pub(crate) event_type: String,
    pub(crate) status: ContextRunStatus,
    pub(crate) step_id: Option<String>,
    #[serde(default)]
    pub(crate) payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct ContextBlackboardItem {
    pub(super) id: String,
    pub(super) ts_ms: u64,
    pub(super) text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) step_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) source_event_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct ContextBlackboardArtifact {
    pub(super) id: String,
    pub(super) ts_ms: u64,
    pub(super) path: String,
    pub(super) artifact_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) step_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) source_event_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct ContextBlackboardSummaries {
    pub(super) rolling: String,
    pub(super) latest_context_pack: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct ContextBlackboardState {
    #[serde(default)]
    pub(super) facts: Vec<ContextBlackboardItem>,
    #[serde(default)]
    pub(super) decisions: Vec<ContextBlackboardItem>,
    #[serde(default)]
    pub(super) open_questions: Vec<ContextBlackboardItem>,
    #[serde(default)]
    pub(super) artifacts: Vec<ContextBlackboardArtifact>,
    #[serde(default)]
    pub(super) tasks: Vec<ContextBlackboardTask>,
    #[serde(default)]
    pub(super) summaries: ContextBlackboardSummaries,
    #[serde(default)]
    pub(super) revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ContextBlackboardTaskStatus {
    Pending,
    Runnable,
    InProgress,
    Blocked,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ContextTaskKind {
    Implementation,
    Inspection,
    Research,
    Validation,
    FixProposal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ContextTaskExecutionMode {
    StrictWrite,
    StrictNonwriting,
    BestEffort,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct ContextTaskOutputTarget {
    pub(super) path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) operation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ContextBlackboardTask {
    pub(super) id: String,
    pub(super) task_type: String,
    #[serde(default)]
    pub(super) payload: Value,
    pub(super) status: ContextBlackboardTaskStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) workflow_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) workflow_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) parent_task_id: Option<String>,
    #[serde(default)]
    pub(super) depends_on_task_ids: Vec<String>,
    #[serde(default)]
    pub(super) decision_ids: Vec<String>,
    #[serde(default)]
    pub(super) artifact_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) assigned_agent: Option<String>,
    #[serde(default)]
    pub(super) priority: i32,
    #[serde(default)]
    pub(super) attempt: u32,
    #[serde(default)]
    pub(super) max_attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) next_retry_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) lease_owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) lease_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) lease_expires_at_ms: Option<u64>,
    #[serde(default)]
    pub(super) task_rev: u64,
    pub(super) created_ts: u64,
    pub(super) updated_ts: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ContextBlackboardPatchOp {
    AddFact,
    AddDecision,
    AddOpenQuestion,
    AddArtifact,
    SetRollingSummary,
    SetLatestContextPack,
    AddTask,
    UpdateTaskLease,
    UpdateTaskState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ContextBlackboardPatchRecord {
    pub(super) patch_id: String,
    pub(super) run_id: String,
    pub(super) seq: u64,
    pub(super) ts_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) source_event_seq: Option<u64>,
    pub(super) op: ContextBlackboardPatchOp,
    pub(super) payload: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ContextBlackboardPatchInput {
    pub(super) op: ContextBlackboardPatchOp,
    pub(super) payload: Value,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct ContextBlackboardPatchesQuery {
    pub(super) since_seq: Option<u64>,
    pub(super) tail: Option<usize>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct ContextRunsEventsStreamQuery {
    pub(super) workspace: Option<String>,
    pub(super) run_ids: Option<String>,
    pub(super) cursor: Option<String>,
    pub(super) tail: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(super) struct ContextRunsStreamCursor {
    #[serde(default)]
    pub(super) events: HashMap<String, u64>,
    #[serde(default)]
    pub(super) patches: HashMap<String, u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ContextRunsStreamEnvelope {
    pub(super) kind: String,
    pub(super) run_id: String,
    pub(super) workspace: String,
    pub(super) seq: u64,
    pub(super) ts_ms: u64,
    pub(super) payload: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ContextTaskCreateInput {
    #[serde(default)]
    pub(super) command_id: Option<String>,
    pub(super) id: Option<String>,
    pub(super) task_type: String,
    #[serde(default)]
    pub(super) payload: Value,
    pub(super) status: Option<ContextBlackboardTaskStatus>,
    #[serde(default)]
    pub(super) workflow_id: Option<String>,
    #[serde(default)]
    pub(super) workflow_node_id: Option<String>,
    #[serde(default)]
    pub(super) parent_task_id: Option<String>,
    #[serde(default)]
    pub(super) depends_on_task_ids: Vec<String>,
    #[serde(default)]
    pub(super) decision_ids: Vec<String>,
    #[serde(default)]
    pub(super) artifact_ids: Vec<String>,
    #[serde(default)]
    pub(super) priority: Option<i32>,
    #[serde(default)]
    pub(super) max_attempts: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ContextTaskCreateBatchInput {
    pub(super) tasks: Vec<ContextTaskCreateInput>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ContextTaskClaimInput {
    pub(super) agent_id: String,
    #[serde(default)]
    pub(super) command_id: Option<String>,
    #[serde(default)]
    pub(super) task_type: Option<String>,
    #[serde(default)]
    pub(super) workflow_id: Option<String>,
    #[serde(default)]
    pub(super) lease_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ContextTaskTransitionInput {
    pub(super) action: String,
    #[serde(default)]
    pub(super) command_id: Option<String>,
    #[serde(default)]
    pub(super) expected_task_rev: Option<u64>,
    #[serde(default)]
    pub(super) lease_token: Option<String>,
    #[serde(default)]
    pub(super) agent_id: Option<String>,
    #[serde(default)]
    pub(super) status: Option<ContextBlackboardTaskStatus>,
    #[serde(default)]
    pub(super) error: Option<String>,
    #[serde(default)]
    pub(super) lease_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ContextCheckpointRecord {
    pub(super) checkpoint_id: String,
    pub(super) run_id: String,
    pub(super) seq: u64,
    pub(super) ts_ms: u64,
    pub(super) reason: String,
    pub(super) run_state: ContextRunState,
    pub(super) blackboard: ContextBlackboardState,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ContextCheckpointCreateInput {
    pub(super) reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ContextReplayDrift {
    pub(super) mismatch: bool,
    pub(super) status_mismatch: bool,
    pub(super) why_next_step_mismatch: bool,
    pub(super) step_count_mismatch: bool,
    #[serde(default)]
    pub(super) blackboard_revision_mismatch: bool,
    #[serde(default)]
    pub(super) blackboard_task_count_mismatch: bool,
    #[serde(default)]
    pub(super) blackboard_task_status_mismatch: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub(super) struct ContextDriverNextInput {
    pub(super) dry_run: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ContextTodoSyncItemInput {
    pub(super) id: Option<String>,
    pub(super) content: String,
    pub(super) status: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ContextTodoSyncInput {
    pub(super) todos: Vec<ContextTodoSyncItemInput>,
    pub(super) source_session_id: Option<String>,
    pub(super) source_run_id: Option<String>,
    pub(super) replace: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct ContextLeaseValidateInput {
    pub(super) phase: String,
    pub(super) current_path: String,
    pub(super) step_id: Option<String>,
}
