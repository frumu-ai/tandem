// Tandem Tauri Commands
// These are the IPC commands exposed to the frontend

use crate::error::{Result, TandemError};
use crate::keystore::{validate_api_key, validate_key_type, ApiKeyType, SecureKeyStore};
use crate::logs::{self, LogFileInfo};
use crate::memory::indexer::{index_workspace, IndexingStats};
use crate::memory::types::{
    ClearFileIndexResult, EmbeddingHealth, MemoryRetrievalMeta, MemoryStats, MemoryTenantScope,
    MemoryTier, ProjectMemoryStats, StoreMessageRequest,
};
use crate::modes::{ModeDefinition, ModeResolution, ModeScope, ResolvedMode};
use crate::orchestrator::{
    engine::OrchestratorEngine,
    policy::{PolicyConfig, PolicyEngine},
    store::OrchestratorStore,
    types::{
        AgentModelRouting, Blackboard, BlackboardPatchRecord, Budget, ModelSelection,
        OrchestratorConfig, OutputTarget, Run, RunSnapshot, RunSource, RunStatus, RunSummary, Task,
        TaskExecutionMode, TaskKind, TaskState,
    },
};
use crate::python_env;
use crate::sidecar::{
    ActiveRunStatusResponse, AgentTeamApprovals, AgentTeamCancelRequest, AgentTeamDecisionResult,
    AgentTeamInstance, AgentTeamInstancesQuery, AgentTeamMissionSummary, AgentTeamSpawnRequest,
    AgentTeamSpawnResult, AgentTeamTemplate, ChannelsConfigResponse, ChannelsStatusResponse,
    ContextBlackboardPatchRecord as ContextPatchRecord, ContextBlackboardState,
    ContextCheckpointRecord, ContextReplayResponse, ContextRunCreateRequest,
    ContextRunEventAppendRequest, ContextRunEventRecord, ContextRunState, ContextRunStatus,
    ContextStepStatus, CreateSessionRequest, FilePartInput, McpActionResponse, McpAddRequest,
    McpRemoteTool, McpServerRecord, MissionApplyEventResult, MissionCreateRequest, MissionState,
    ModelInfo, ModelSpec, Project, ProviderInfo, RoutineCreateRequest, RoutineHistoryEvent,
    RoutinePatchRequest, RoutineRunArtifact, RoutineRunArtifactAddRequest,
    RoutineRunDecisionRequest, RoutineRunNowRequest, RoutineRunNowResponse, RoutineRunRecord,
    RoutineSpec, SendMessageRequest, Session, SessionMessage, SidecarState, StreamEvent, TodoItem,
};
use crate::sidecar_manager::{self, SidecarStatus};
use crate::state::{AppState, AppStateInfo, ProvidersConfig};
use crate::stream_hub::{StreamEventEnvelopeV2, StreamEventSource};
use crate::tool_history::ToolExecutionRow;
use crate::tool_policy;
use crate::tool_proxy::{FileSnapshot, JournalEntry, OperationStatus, UndoAction};
use crate::vault::{self, EncryptedVaultKey, VaultStatus};
use crate::VaultState;
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tandem_core::{
    migrate_legacy_storage_if_needed, normalize_workspace_path, resolve_shared_paths,
    SessionRepairStats, Storage,
};
use tandem_observability::{emit_event, ObservabilityEvent, ProcessKind};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_store::StoreExt;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize)]
struct CoderWorkflowEventTelemetry {
    event: String,
    recorded_at_ms: Option<u64>,
    reason: Option<String>,
    workflow_class: Option<String>,
    phase: Option<String>,
    status: Option<String>,
    failure_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct CoderTaskTelemetry {
    task_id: String,
    label: String,
    status: String,
    owner: Option<String>,
    workflow_class: Option<String>,
    phase: Option<String>,
    failure_kind: Option<String>,
    changed_files: Vec<String>,
    diff_files: Vec<String>,
    verification_commands: Vec<String>,
    verification_outcome: Option<String>,
    verification_passed: Option<bool>,
    validations_attempted: Option<usize>,
    latest_failing_command: Option<String>,
    failure_detail: Option<String>,
    artifact_paths: Vec<String>,
    workflow_events: Vec<CoderWorkflowEventTelemetry>,
}

#[derive(Debug, Clone, Serialize)]
struct CoderRunTelemetrySummary {
    changed_files: Vec<String>,
    verification_commands: Vec<String>,
    patch_summaries: Vec<String>,
    validation_failures: Vec<String>,
    workflow_events: Vec<CoderWorkflowEventTelemetry>,
    task_summaries: Vec<CoderTaskTelemetry>,
}

fn shared_app_data_dir(_app: &AppHandle) -> Result<PathBuf> {
    match resolve_shared_paths() {
        Ok(paths) => Ok(paths.canonical_root),
        Err(e) => dirs::data_dir().map(|d| d.join("tandem")).ok_or_else(|| {
            TandemError::InvalidConfig(format!(
                "Failed to resolve canonical shared app data dir: {}",
                e
            ))
        }),
    }
}

fn json_record(value: &serde_json::Value) -> Option<&serde_json::Map<String, serde_json::Value>> {
    value.as_object()
}

fn json_text(value: Option<&serde_json::Value>) -> Option<String> {
    match value {
        Some(serde_json::Value::String(text)) if !text.trim().is_empty() => {
            Some(text.trim().to_string())
        }
        Some(serde_json::Value::Number(number)) => Some(number.to_string()),
        Some(serde_json::Value::Bool(flag)) => Some(flag.to_string()),
        _ => None,
    }
}

fn summarize_workflow_event(event: &serde_json::Value) -> Option<CoderWorkflowEventTelemetry> {
    let record = json_record(event)?;
    let metadata = record.get("metadata").and_then(json_record);
    Some(CoderWorkflowEventTelemetry {
        event: json_text(record.get("event")).unwrap_or_else(|| "event".to_string()),
        recorded_at_ms: record
            .get("recorded_at_ms")
            .and_then(|value| value.as_u64()),
        reason: json_text(record.get("reason")),
        workflow_class: metadata.and_then(|value| json_text(value.get("workflow_class"))),
        phase: metadata.and_then(|value| json_text(value.get("phase"))),
        status: metadata.and_then(|value| json_text(value.get("status"))),
        failure_kind: metadata.and_then(|value| json_text(value.get("failure_kind"))),
    })
}

fn workflow_event_matches_task(task: &serde_json::Value, event: &serde_json::Value) -> bool {
    let task_record = match json_record(task) {
        Some(value) => value,
        None => return false,
    };
    let event_record = match json_record(event) {
        Some(value) => value,
        None => return false,
    };
    let metadata = match event_record.get("metadata").and_then(json_record) {
        Some(value) => value,
        None => return false,
    };
    let event_node_id = json_text(metadata.get("node_id"))
        .map(|value| value.to_lowercase())
        .unwrap_or_default();
    if event_node_id.is_empty() {
        return false;
    }
    [
        json_text(task_record.get("id")),
        json_text(task_record.get("workflow_node_id")),
        json_text(task_record.get("task_type")),
    ]
    .into_iter()
    .flatten()
    .map(|value| value.to_lowercase())
    .any(|value| value == event_node_id)
}

fn collect_json_strings(value: &serde_json::Value, depth: usize, out: &mut Vec<String>) {
    if depth > 4 {
        return;
    }
    match value {
        serde_json::Value::String(text) => out.push(text.clone()),
        serde_json::Value::Number(number) => out.push(number.to_string()),
        serde_json::Value::Bool(flag) => out.push(flag.to_string()),
        serde_json::Value::Array(items) => {
            for item in items {
                collect_json_strings(item, depth + 1, out);
            }
        }
        serde_json::Value::Object(record) => {
            for child in record.values() {
                collect_json_strings(child, depth + 1, out);
            }
        }
        _ => {}
    }
}

fn collect_keyed_json_strings<F>(
    value: &serde_json::Value,
    key_matcher: &F,
    depth: usize,
    out: &mut Vec<String>,
) where
    F: Fn(&str) -> bool,
{
    if depth > 5 {
        return;
    }
    match value {
        serde_json::Value::Array(items) => {
            for item in items {
                collect_keyed_json_strings(item, key_matcher, depth + 1, out);
            }
        }
        serde_json::Value::Object(record) => {
            for (key, child) in record {
                if key_matcher(&key.to_lowercase()) {
                    collect_json_strings(child, depth + 1, out);
                }
                collect_keyed_json_strings(child, key_matcher, depth + 1, out);
            }
        }
        _ => {}
    }
}

fn unique_strings<I>(values: I, limit: usize) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut seen = std::collections::BTreeSet::new();
    let mut result = Vec::new();
    for value in values {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lowered = trimmed.to_lowercase();
        if seen.insert(lowered) {
            result.push(trimmed.to_string());
            if result.len() >= limit {
                break;
            }
        }
    }
    result
}

fn looks_like_file_path(value: &str) -> bool {
    let text = value.trim();
    if text.is_empty()
        || text.starts_with("http://")
        || text.starts_with("https://")
        || text.contains('\n')
        || text.len() > 260
    {
        return false;
    }
    text.contains('/')
        || text.contains('\\')
        || regex::Regex::new(r"\.[a-zA-Z0-9]{1,8}$")
            .map(|re| re.is_match(text))
            .unwrap_or(false)
}

fn looks_like_command(value: &str) -> bool {
    let text = value.trim();
    if text.is_empty() || text.len() > 240 || text.contains('\n') {
        return false;
    }
    regex::Regex::new(
        r"(?i)(^|[\s])(cargo|pnpm|npm|yarn|bun|node|python|pytest|ruff|uv|make|cmake|go|rustc|git|bash|sh|zsh|deno|turbo|nx|jest|vitest|playwright|mvn|gradle)([\s]|$)",
    )
    .map(|re| re.is_match(text))
    .unwrap_or(false)
}

fn extract_changed_files(value: &serde_json::Value, limit: usize) -> Vec<String> {
    let mut strings = Vec::new();
    collect_keyed_json_strings(
        value,
        &|key| {
            key.contains("changed_file")
                || key.contains("files_changed")
                || key.contains("touched_file")
                || key.contains("modified_file")
                || key.contains("updated_file")
                || key.contains("created_file")
                || key == "path"
                || key == "file"
                || key == "file_path"
                || key == "target_file"
                || key == "relative_path"
        },
        0,
        &mut strings,
    );
    unique_strings(
        strings
            .into_iter()
            .filter(|value| looks_like_file_path(value))
            .collect::<Vec<_>>(),
        limit,
    )
}

fn extract_verification_commands(value: &serde_json::Value, limit: usize) -> Vec<String> {
    let mut strings = Vec::new();
    collect_keyed_json_strings(
        value,
        &|key| {
            key == "command"
                || key == "cmd"
                || key == "commands"
                || key.contains("command_run")
                || key.contains("commands_run")
                || key.contains("verification_command")
                || key.contains("validation_command")
                || key.contains("test_command")
                || key.contains("lint_command")
                || key.contains("build_command")
        },
        0,
        &mut strings,
    );
    unique_strings(
        strings
            .into_iter()
            .filter(|value| looks_like_command(value))
            .collect::<Vec<_>>(),
        limit,
    )
}

fn extract_patch_summaries(value: &serde_json::Value, limit: usize) -> Vec<String> {
    let mut strings = Vec::new();
    collect_keyed_json_strings(
        value,
        &|key| {
            key == "summary"
                || key.contains("patch_summary")
                || key.contains("diff_summary")
                || key.contains("change_summary")
                || key.contains("result_summary")
        },
        0,
        &mut strings,
    );
    unique_strings(
        strings
            .into_iter()
            .filter(|value| value.trim().len() >= 12 && !looks_like_file_path(value))
            .collect::<Vec<_>>(),
        limit,
    )
}

fn extract_failure_details(value: &serde_json::Value, limit: usize) -> Vec<String> {
    let mut strings = Vec::new();
    collect_keyed_json_strings(
        value,
        &|key| {
            key == "error"
                || key == "error_message"
                || key == "last_error"
                || key == "failure_reason"
                || key == "reason"
                || key == "stderr"
                || key == "validation_error"
                || key == "failure_detail"
        },
        0,
        &mut strings,
    );
    unique_strings(
        strings
            .into_iter()
            .filter(|value| !value.trim().is_empty() && value.trim().len() >= 8)
            .collect::<Vec<_>>(),
        limit,
    )
}

fn extract_validation_outcome(value: &serde_json::Value) -> Option<String> {
    let record = json_record(value)?;
    json_text(record.get("outcome"))
        .or_else(|| json_text(record.get("result")))
        .or_else(|| {
            record
                .get("validation")
                .and_then(json_record)
                .and_then(|validation| {
                    json_text(validation.get("outcome"))
                        .or_else(|| json_text(validation.get("result")))
                })
        })
}

fn extract_validation_passed(value: &serde_json::Value) -> Option<bool> {
    let record = json_record(value)?;
    record
        .get("passed")
        .and_then(|value| value.as_bool())
        .or_else(|| {
            record
                .get("validation")
                .and_then(json_record)
                .and_then(|validation| validation.get("passed"))
                .and_then(|value| value.as_bool())
        })
}

fn extract_validations_attempted(value: &serde_json::Value) -> Option<usize> {
    let record = json_record(value)?;
    record
        .get("validations_attempted")
        .and_then(|value| value.as_array())
        .map(|items| items.len())
}

fn first_failing_command(value: &serde_json::Value) -> Option<String> {
    let commands = extract_verification_commands(value, 4);
    commands.into_iter().next()
}

fn read_json_file(path: &str) -> Option<serde_json::Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn summarize_coder_run_telemetry(
    payload: &serde_json::Value,
    artifacts_payload: Option<&serde_json::Value>,
) -> Option<CoderRunTelemetrySummary> {
    let run = json_record(payload)?.get("run")?;
    let run_record = json_record(run)?;
    let tasks = run_record
        .get("tasks")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let events = run_record
        .get("events")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let artifacts = artifacts_payload
        .and_then(json_record)
        .and_then(|record| record.get("artifacts"))
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let lifecycle_history = run_record
        .get("checkpoint")
        .and_then(json_record)
        .and_then(|checkpoint| {
            checkpoint
                .get("lifecycle_history")
                .or_else(|| checkpoint.get("lifecycleHistory"))
        })
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let mut artifacts_by_step: std::collections::BTreeMap<String, Vec<serde_json::Value>> =
        std::collections::BTreeMap::new();
    let mut artifact_json_by_path: std::collections::BTreeMap<String, serde_json::Value> =
        std::collections::BTreeMap::new();
    for artifact in &artifacts {
        let Some(record) = json_record(artifact) else {
            continue;
        };
        let path = json_text(record.get("path")).unwrap_or_default();
        if let Some(step_id) = json_text(record.get("step_id")) {
            artifacts_by_step
                .entry(step_id)
                .or_default()
                .push(artifact.clone());
        }
        if !path.is_empty() {
            if let Some(parsed) = read_json_file(&path) {
                artifact_json_by_path.insert(path, parsed);
            }
        }
    }

    let changed_files = unique_strings(
        tasks
            .iter()
            .flat_map(|task| extract_changed_files(task, 8))
            .chain(
                artifact_json_by_path
                    .values()
                    .flat_map(|artifact| extract_changed_files(artifact, 8)),
            )
            .chain(events.iter().flat_map(|event| {
                json_record(event)
                    .and_then(|record| record.get("payload"))
                    .map(|payload| extract_changed_files(payload, 8))
                    .unwrap_or_default()
            }))
            .collect::<Vec<_>>(),
        24,
    );

    let verification_commands = unique_strings(
        tasks
            .iter()
            .flat_map(|task| extract_verification_commands(task, 4))
            .chain(
                artifact_json_by_path
                    .values()
                    .flat_map(|artifact| extract_verification_commands(artifact, 4)),
            )
            .chain(events.iter().flat_map(|event| {
                json_record(event)
                    .and_then(|record| record.get("payload"))
                    .map(|payload| extract_verification_commands(payload, 4))
                    .unwrap_or_default()
            }))
            .collect::<Vec<_>>(),
        16,
    );

    let patch_summaries = unique_strings(
        tasks
            .iter()
            .flat_map(|task| extract_patch_summaries(task, 2))
            .chain(
                artifact_json_by_path
                    .values()
                    .flat_map(|artifact| extract_patch_summaries(artifact, 2)),
            )
            .chain(events.iter().flat_map(|event| {
                json_record(event)
                    .and_then(|record| record.get("payload"))
                    .map(|payload| extract_patch_summaries(payload, 2))
                    .unwrap_or_default()
            }))
            .collect::<Vec<_>>(),
        8,
    );

    let validation_failures = unique_strings(
        artifact_json_by_path
            .values()
            .flat_map(|artifact| extract_failure_details(artifact, 2))
            .chain(
                tasks
                    .iter()
                    .flat_map(|task| extract_failure_details(task, 2)),
            )
            .collect::<Vec<_>>(),
        8,
    );
    let workflow_events = lifecycle_history
        .iter()
        .filter_map(summarize_workflow_event)
        .collect::<Vec<_>>();

    let task_summaries = tasks
        .iter()
        .filter_map(|task| {
            let record = json_record(task)?;
            let payload = record.get("payload");
            let task_id = json_text(record.get("id"))
                .or_else(|| json_text(record.get("workflow_node_id")))
                .or_else(|| json_text(record.get("task_type")))
                .unwrap_or_else(|| "task".to_string());
            let task_workflow_events = lifecycle_history
                .iter()
                .filter(|event| workflow_event_matches_task(task, event))
                .filter_map(summarize_workflow_event)
                .collect::<Vec<_>>();
            let related_artifacts = artifacts_by_step.get(&task_id).cloned().unwrap_or_default();
            let artifact_paths = unique_strings(
                related_artifacts
                    .iter()
                    .filter_map(|artifact| json_record(artifact))
                    .filter_map(|artifact| json_text(artifact.get("path")))
                    .collect::<Vec<_>>(),
                6,
            );
            let related_artifact_json = artifact_paths
                .iter()
                .filter_map(|path| artifact_json_by_path.get(path))
                .cloned()
                .collect::<Vec<_>>();
            let owner = json_text(record.get("owner"))
                .or_else(|| json_text(record.get("claimed_by")))
                .or_else(|| json_text(record.get("assignee")))
                .or_else(|| {
                    payload
                        .and_then(|p| json_record(p))
                        .and_then(|p| json_text(p.get("owner")))
                })
                .or_else(|| {
                    payload
                        .and_then(|p| json_record(p))
                        .and_then(|p| json_text(p.get("claimed_by")))
                })
                .or_else(|| {
                    payload
                        .and_then(|p| json_record(p))
                        .and_then(|p| json_text(p.get("assignee")))
                });
            let failure_detail = json_text(record.get("error_message"))
                .or_else(|| json_text(record.get("error")))
                .or_else(|| json_text(record.get("last_error")))
                .or_else(|| {
                    payload
                        .and_then(|p| json_record(p))
                        .and_then(|p| json_text(p.get("error_message")))
                })
                .or_else(|| {
                    payload
                        .and_then(|p| json_record(p))
                        .and_then(|p| json_text(p.get("error")))
                })
                .or_else(|| {
                    payload
                        .and_then(|p| json_record(p))
                        .and_then(|p| json_text(p.get("last_error")))
                })
                .or_else(|| {
                    payload
                        .and_then(|p| json_record(p))
                        .and_then(|p| json_text(p.get("failure_reason")))
                })
                .or_else(|| {
                    payload
                        .and_then(|p| json_record(p))
                        .and_then(|p| json_text(p.get("reason")))
                })
                .or_else(|| {
                    payload
                        .and_then(|p| json_record(p))
                        .and_then(|p| json_text(p.get("stderr")))
                })
                .or_else(|| {
                    related_artifact_json
                        .iter()
                        .flat_map(|artifact| extract_failure_details(artifact, 1))
                        .next()
                });
            let verification_outcome = extract_validation_outcome(task).or_else(|| {
                related_artifact_json
                    .iter()
                    .find_map(extract_validation_outcome)
            });
            let verification_passed = extract_validation_passed(task).or_else(|| {
                related_artifact_json
                    .iter()
                    .find_map(extract_validation_passed)
            });
            let validations_attempted = extract_validations_attempted(task).or_else(|| {
                related_artifact_json
                    .iter()
                    .find_map(extract_validations_attempted)
            });
            let latest_failing_command = if failure_detail.is_some()
                || verification_passed == Some(false)
                || verification_outcome
                    .as_ref()
                    .map(|value| value.to_lowercase().contains("fail"))
                    .unwrap_or(false)
            {
                first_failing_command(task)
                    .or_else(|| related_artifact_json.iter().find_map(first_failing_command))
            } else {
                None
            };
            Some(CoderTaskTelemetry {
                task_id: task_id.clone(),
                label: json_text(record.get("title"))
                    .or_else(|| json_text(record.get("workflow_node_id")))
                    .or_else(|| json_text(record.get("task_type")))
                    .or_else(|| json_text(record.get("id")))
                    .unwrap_or_else(|| "task".to_string()),
                status: json_text(record.get("status")).unwrap_or_else(|| "unknown".to_string()),
                owner,
                workflow_class: task_workflow_events
                    .iter()
                    .find_map(|event| event.workflow_class.clone()),
                phase: task_workflow_events
                    .iter()
                    .find_map(|event| event.phase.clone()),
                failure_kind: task_workflow_events
                    .iter()
                    .find_map(|event| event.failure_kind.clone()),
                changed_files: unique_strings(
                    extract_changed_files(task, 8)
                        .into_iter()
                        .chain(
                            related_artifact_json
                                .iter()
                                .flat_map(|artifact| extract_changed_files(artifact, 8)),
                        )
                        .collect::<Vec<_>>(),
                    8,
                ),
                diff_files: unique_strings(
                    related_artifact_json
                        .iter()
                        .flat_map(|artifact| extract_changed_files(artifact, 8))
                        .collect::<Vec<_>>(),
                    8,
                ),
                verification_commands: unique_strings(
                    extract_verification_commands(task, 4)
                        .into_iter()
                        .chain(
                            related_artifact_json
                                .iter()
                                .flat_map(|artifact| extract_verification_commands(artifact, 4)),
                        )
                        .collect::<Vec<_>>(),
                    4,
                ),
                verification_outcome,
                verification_passed,
                validations_attempted,
                latest_failing_command,
                failure_detail,
                artifact_paths,
                workflow_events: task_workflow_events,
            })
        })
        .collect::<Vec<_>>();

    Some(CoderRunTelemetrySummary {
        changed_files,
        verification_commands,
        patch_summaries,
        validation_failures,
        workflow_events,
        task_summaries,
    })
}

include!("commands/packs_updater.rs");
include!("commands/vault.rs");
include!("commands/memory.rs");
include!("commands/basic.rs");
include!("commands/project.rs");
include!("commands/api_keys.rs");
include!("commands/theme.rs");
include!("commands/providers.rs");
include!("commands/channels.rs");
include!("commands/sidecar.rs");
include!("commands/file_tree_sessions.rs");
include!("commands/messages.rs");
include!("commands/models_and_logs.rs");
include!("commands/undo_and_approvals.rs");
include!("commands/mcp.rs");
include!("commands/routines_and_planning.rs");
include!("commands/sidecar_tools.rs");
include!("commands/presentation_and_files.rs");
include!("commands/python_and_skills.rs");
include!("commands/plugins_and_plan.rs");
include!("commands/ralph.rs");
include!("commands/orchestrator_core.rs");
include!("commands/orchestrator_runtime.rs");

#[tauri::command]
pub async fn get_language_setting(app: AppHandle) -> Result<String> {
    let store = app
        .store("store.json")
        .map_err(|e| TandemError::InvalidConfig(format!("Failed to access store: {}", e)))?;

    let language = store
        .get("language")
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "en".to_string());

    Ok(language)
}

#[tauri::command]
pub async fn set_language_setting(app: AppHandle, language: String) -> Result<()> {
    let store = app
        .store("store.json")
        .map_err(|e| TandemError::InvalidConfig(format!("Failed to access store: {}", e)))?;

    store.set("language".to_string(), serde_json::json!(language));
    store.save().map_err(|e| {
        TandemError::InvalidConfig(format!("Failed to save language setting: {}", e))
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_skip_memory_retrieval_for_commands_and_empty() {
        assert!(should_skip_memory_retrieval("/undo"));
        assert!(should_skip_memory_retrieval("   /status"));
        assert!(should_skip_memory_retrieval(""));
        assert!(should_skip_memory_retrieval("   "));
        assert!(!should_skip_memory_retrieval("How does indexing work?"));
    }

    #[test]
    fn resolves_stale_desktop_codex_selection_to_supported_default() {
        let mut config = ProvidersConfig::default();
        config.selected_model = Some(crate::state::SelectedModel {
            provider_id: "openai-codex".to_string(),
            model_id: "gpt-5.6".to_string(),
        });

        let spec = resolve_default_model_spec(&config).expect("model spec");

        assert_eq!(spec.provider_id, "openai-codex");
        assert_eq!(spec.model_id, "gpt-5.5");
    }

    #[test]
    fn resolves_stale_explicit_codex_dispatch_to_supported_default() {
        let config = ProvidersConfig::default();

        let spec = resolve_required_model_spec(
            &config,
            Some("gpt-5.6".to_string()),
            Some("openai-codex".to_string()),
            "test dispatch",
        )
        .expect("model spec");

        assert_eq!(spec.provider_id, "openai-codex");
        assert_eq!(spec.model_id, "gpt-5.5");
    }

    #[test]
    fn preserves_explicit_supported_codex_preview_variant() {
        let config = ProvidersConfig::default();

        let spec = resolve_required_model_spec(
            &config,
            Some("gpt-5.6-sol".to_string()),
            Some("openai_codex".to_string()),
            "test dispatch",
        )
        .expect("model spec");

        assert_eq!(spec.provider_id, "openai-codex");
        assert_eq!(spec.model_id, "gpt-5.6-sol");
    }

    #[test]
    fn test_build_message_content_with_memory_context() {
        let merged = build_message_content_with_memory_context(
            "User question",
            "<memory_context>\n- fact\n</memory_context>",
        );
        assert!(merged.starts_with("<memory_context>"));
        assert!(merged.ends_with("User question"));

        let unchanged = build_message_content_with_memory_context("User question", "");
        assert_eq!(unchanged, "User question");
    }

    #[test]
    fn test_memory_retrieval_event_shape() {
        let meta = MemoryRetrievalMeta {
            used: true,
            chunks_total: 7,
            session_chunks: 2,
            history_chunks: 1,
            project_fact_chunks: 4,
            score_min: Some(0.2),
            score_max: Some(0.9),
        };
        let event = memory_retrieval_event(
            "session-1",
            "retrieved_used",
            &meta,
            42,
            "abcdef123456".to_string(),
            Some("ok".to_string()),
            None,
        );

        match event {
            StreamEvent::MemoryRetrieval {
                session_id,
                status,
                used,
                chunks_total,
                session_chunks,
                history_chunks,
                project_fact_chunks,
                latency_ms,
                query_hash,
                score_min,
                score_max,
                embedding_status,
                embedding_reason,
            } => {
                assert_eq!(session_id, "session-1");
                assert_eq!(status.as_deref(), Some("retrieved_used"));
                assert!(used);
                assert_eq!(chunks_total, 7);
                assert_eq!(session_chunks, 2);
                assert_eq!(history_chunks, 1);
                assert_eq!(project_fact_chunks, 4);
                assert_eq!(latency_ms, 42);
                assert_eq!(query_hash, "abcdef123456");
                assert_eq!(score_min, Some(0.2));
                assert_eq!(score_max, Some(0.9));
                assert_eq!(embedding_status.as_deref(), Some("ok"));
                assert_eq!(embedding_reason, None);
            }
            other => panic!("Unexpected event variant: {:?}", other),
        }
    }
}
