use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use tandem_orchestrator::KnowledgeBinding;
use tandem_types::TenantContext;

mod action_schema;
mod mission_builder;
pub mod plan_package;

pub use action_schema::{
    WorkflowActionDefinition, WorkflowActionKind, WorkflowActionRegistry,
    WorkflowActionValidationIssue, WorkflowActionValidationMode, WorkflowResolvedAction,
};
pub use mission_builder::{
    validate_mission_blueprint, ApprovalDecision, HumanApprovalGate, InputRefBlueprint,
    MissionBlueprint, MissionMilestoneBlueprint, MissionPhaseBlueprint, MissionPhaseExecutionMode,
    MissionTeamBlueprint, OutputContractBlueprint, ReviewStage, ReviewStageKind, ValidationMessage,
    ValidationSeverity, WorkstreamBlueprint,
};
pub use plan_package::{
    AutomationV2Schedule, AutomationV2ScheduleType, WorkflowPlan, WorkflowPlanChatMessage,
    WorkflowPlanConversation, WorkflowPlanDraftRecord, WorkflowPlanStep,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowSourceKind {
    BuiltIn,
    Pack,
    Workspace,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowSourceRef {
    pub kind: WorkflowSourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowActionSpec {
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub with: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowStepSpec {
    pub step_id: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub with: Option<Value>,
    #[serde(default)]
    pub knowledge: KnowledgeBinding,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowHookBinding {
    pub binding_id: String,
    pub workflow_id: String,
    pub event: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub actions: Vec<WorkflowActionSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<WorkflowSourceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowSpec {
    pub workflow_id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub knowledge: KnowledgeBinding,
    #[serde(default)]
    pub steps: Vec<WorkflowStepSpec>,
    #[serde(default)]
    pub hooks: Vec<WorkflowHookBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<WorkflowSourceRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct WorkflowRegistry {
    #[serde(default)]
    pub workflows: HashMap<String, WorkflowSpec>,
    #[serde(default)]
    pub hooks: Vec<WorkflowHookBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowRunStatus {
    Queued,
    Running,
    /// Paused on a `HumanApprovalGate` action; resumes via the gate decision
    /// endpoint (`POST /workflows/runs/{id}/gate`).
    AwaitingApproval,
    Completed,
    Failed,
    Cancelled,
    DryRun,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowActionRunStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Skipped,
}

/// A pending human approval gate blocking a workflow run. Mirrors
/// `AutomationPendingGate` semantics: the gate is durable run state, not an
/// in-process wait, so it survives restarts and stays visible in the
/// approvals inbox until decided.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowPendingGate {
    /// The gate action's id within the run.
    pub action_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    /// Allowed decisions; defaults to approve/rework/cancel.
    #[serde(default)]
    pub decisions: Vec<String>,
    /// Action ids re-queued on a `rework` decision.
    #[serde(default)]
    pub rework_targets: Vec<String>,
    pub requested_at_ms: u64,
}

/// Audit record of a decided workflow gate.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowGateDecisionRecord {
    pub action_id: String,
    pub decision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_wait: Option<tandem_types::ApprovalWaitRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub decided_at_ms: u64,
    /// Serialized governance actor (kind/actor_id/source).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided_by: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowActionRunRecord {
    pub action_id: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    pub status: WorkflowActionRunStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowRunRecord {
    pub run_id: String,
    pub workflow_id: String,
    #[serde(default = "default_tenant_context")]
    pub tenant_context: TenantContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_event: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_event_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enterprise_scope: Option<Value>,
    pub status: WorkflowRunStatus,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    #[serde(default)]
    pub actions: Vec<WorkflowActionRunRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub awaiting_gate: Option<WorkflowPendingGate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gate_history: Vec<WorkflowGateDecisionRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<WorkflowSourceRef>,
}

fn default_tenant_context() -> TenantContext {
    TenantContext::local_implicit()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowSimulationResult {
    #[serde(default)]
    pub matched_bindings: Vec<WorkflowHookBinding>,
    #[serde(default)]
    pub planned_actions: Vec<WorkflowActionSpec>,
    #[serde(default)]
    pub canonical_events: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowValidationSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowValidationMessage {
    pub severity: WorkflowValidationSeverity,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

impl WorkflowValidationMessage {
    fn new(severity: WorkflowValidationSeverity, message: impl Into<String>) -> Self {
        Self {
            severity,
            message: message.into(),
            source_path: None,
            workflow_id: None,
            step_id: None,
            field: None,
        }
    }

    fn with_workflow(mut self, workflow: &WorkflowSpec, field: impl Into<String>) -> Self {
        self.source_path = workflow
            .source
            .as_ref()
            .and_then(|source| source.path.clone());
        self.workflow_id = Some(workflow.workflow_id.clone());
        self.field = Some(field.into());
        self
    }

    fn with_step(
        mut self,
        workflow: &WorkflowSpec,
        step: &WorkflowStepSpec,
        field: impl Into<String>,
    ) -> Self {
        self.source_path = workflow
            .source
            .as_ref()
            .and_then(|source| source.path.clone());
        self.workflow_id = Some(workflow.workflow_id.clone());
        self.step_id = Some(step.step_id.clone());
        self.field = Some(field.into());
        self
    }

    fn with_hook(mut self, hook: &WorkflowHookBinding, field: impl Into<String>) -> Self {
        self.source_path = hook.source.as_ref().and_then(|source| source.path.clone());
        self.workflow_id = Some(hook.workflow_id.clone());
        self.field = Some(field.into());
        self
    }
}

#[derive(Debug, Clone)]
pub struct WorkflowRegistryValidationOptions {
    pub action_validation_mode: WorkflowActionValidationMode,
    pub action_registry: WorkflowActionRegistry,
}

impl Default for WorkflowRegistryValidationOptions {
    fn default() -> Self {
        Self {
            action_validation_mode: WorkflowActionValidationMode::Local,
            action_registry: WorkflowActionRegistry::default(),
        }
    }
}

impl WorkflowRegistryValidationOptions {
    pub fn strict(action_registry: WorkflowActionRegistry) -> Self {
        Self {
            action_validation_mode: WorkflowActionValidationMode::Strict,
            action_registry,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct WorkflowRegistryLoadOptions {
    pub validation: WorkflowRegistryValidationOptions,
    pub reject_on_error: bool,
}

impl WorkflowRegistryLoadOptions {
    pub fn strict(action_registry: WorkflowActionRegistry) -> Self {
        Self {
            validation: WorkflowRegistryValidationOptions::strict(action_registry),
            reject_on_error: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorkflowLoadSource {
    pub root: PathBuf,
    pub kind: WorkflowSourceKind,
    pub pack_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WorkflowFileEnvelope {
    #[serde(default)]
    workflow: Option<WorkflowFileShape>,
    #[serde(default)]
    hooks: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct WorkflowFileShape {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    workflow_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    steps: Vec<WorkflowStepInput>,
    #[serde(default)]
    hooks: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum WorkflowStepInput {
    String(String),
    Object(WorkflowStepObjectInput),
}

#[derive(Debug, Deserialize)]
struct WorkflowStepObjectInput {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    step_id: Option<String>,
    action: String,
    #[serde(default)]
    with: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum HookFileShape {
    Map(HashMap<String, Vec<HookActionInput>>),
    List(Vec<HookBindingInput>),
}

#[derive(Debug, Deserialize)]
struct HookBindingInput {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    binding_id: Option<String>,
    #[serde(default)]
    workflow: Option<String>,
    #[serde(default)]
    workflow_id: Option<String>,
    event: String,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    actions: Vec<HookActionInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum HookActionInput {
    String(String),
    Object(WorkflowActionSpec),
}

pub fn load_registry(sources: &[WorkflowLoadSource]) -> anyhow::Result<WorkflowRegistry> {
    let mut registry = WorkflowRegistry::default();
    for source in sources {
        load_source_into(&mut registry, source)?;
    }
    Ok(registry)
}

pub fn load_registry_with_options(
    sources: &[WorkflowLoadSource],
    options: &WorkflowRegistryLoadOptions,
) -> anyhow::Result<WorkflowRegistry> {
    let registry = load_registry(sources)?;
    if options.reject_on_error {
        let messages = validate_registry_with_options(&registry, &options.validation);
        let errors = messages
            .iter()
            .filter(|message| message.severity == WorkflowValidationSeverity::Error)
            .collect::<Vec<_>>();
        if !errors.is_empty() {
            anyhow::bail!(
                "workflow registry validation failed: {}",
                format_validation_messages(&errors)
            );
        }
    }
    Ok(registry)
}

pub fn validate_registry(registry: &WorkflowRegistry) -> Vec<WorkflowValidationMessage> {
    validate_registry_with_options(registry, &WorkflowRegistryValidationOptions::default())
}

pub fn validate_registry_with_options(
    registry: &WorkflowRegistry,
    options: &WorkflowRegistryValidationOptions,
) -> Vec<WorkflowValidationMessage> {
    let mut messages = Vec::new();
    for workflow in registry.workflows.values() {
        if workflow.steps.is_empty()
            && registry
                .hooks
                .iter()
                .all(|hook| hook.workflow_id != workflow.workflow_id)
        {
            messages.push(
                WorkflowValidationMessage::new(
                    WorkflowValidationSeverity::Warning,
                    format!(
                        "workflow `{}` has no steps and no hook bindings",
                        workflow.workflow_id
                    ),
                )
                .with_workflow(workflow, "steps"),
            );
        }
        for step in &workflow.steps {
            if step.step_id.trim().is_empty() {
                messages.push(
                    WorkflowValidationMessage::new(
                        WorkflowValidationSeverity::Error,
                        format!(
                            "workflow `{}` has step with empty step_id",
                            workflow.workflow_id
                        ),
                    )
                    .with_step(workflow, step, "step_id"),
                );
            }
            if step.action.trim().is_empty() {
                messages.push(
                    WorkflowValidationMessage::new(
                        WorkflowValidationSeverity::Error,
                        format!(
                            "workflow `{}` has step `{}` with empty action",
                            workflow.workflow_id, step.step_id
                        ),
                    )
                    .with_step(workflow, step, "action"),
                );
            }
            messages.extend(
                options
                    .action_registry
                    .validate_action(
                        &step.action,
                        step.with.as_ref(),
                        options.action_validation_mode,
                    )
                    .into_iter()
                    .map(|issue| {
                        WorkflowValidationMessage::new(
                            issue.severity,
                            format!(
                                "workflow action `{}` validation failed: {}",
                                step.action, issue.message
                            ),
                        )
                        .with_step(workflow, step, issue.field)
                    }),
            );
        }
    }
    for hook in &registry.hooks {
        if !registry.workflows.contains_key(&hook.workflow_id) {
            messages.push(
                WorkflowValidationMessage::new(
                    WorkflowValidationSeverity::Error,
                    format!(
                        "hook `{}` references unknown workflow `{}`",
                        hook.binding_id, hook.workflow_id
                    ),
                )
                .with_hook(hook, "workflow_id"),
            );
        }
        if hook.actions.is_empty() {
            messages.push(
                WorkflowValidationMessage::new(
                    WorkflowValidationSeverity::Warning,
                    format!("hook `{}` has no actions", hook.binding_id),
                )
                .with_hook(hook, "actions"),
            );
        }
        for (idx, action) in hook.actions.iter().enumerate() {
            messages.extend(
                options
                    .action_registry
                    .validate_action(
                        &action.action,
                        action.with.as_ref(),
                        options.action_validation_mode,
                    )
                    .into_iter()
                    .map(|issue| {
                        WorkflowValidationMessage::new(
                            issue.severity,
                            format!(
                                "workflow hook action `{}` validation failed: {}",
                                action.action, issue.message
                            ),
                        )
                        .with_hook(hook, format!("actions[{idx}].{}", issue.field))
                    }),
            );
        }
    }
    messages
}

fn format_validation_messages(messages: &[&WorkflowValidationMessage]) -> String {
    messages
        .iter()
        .map(|message| {
            let mut parts = Vec::new();
            if let Some(path) = message.source_path.as_deref() {
                parts.push(path.to_string());
            }
            if let Some(workflow_id) = message.workflow_id.as_deref() {
                parts.push(format!("workflow={workflow_id}"));
            }
            if let Some(step_id) = message.step_id.as_deref() {
                parts.push(format!("step_id={step_id}"));
            }
            if let Some(field) = message.field.as_deref() {
                parts.push(format!("field={field}"));
            }
            if parts.is_empty() {
                message.message.clone()
            } else {
                format!("{} ({})", message.message, parts.join(", "))
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn load_source_into(
    registry: &mut WorkflowRegistry,
    source: &WorkflowLoadSource,
) -> anyhow::Result<()> {
    for entry in collect_yaml_files(&source.root.join("workflows"))? {
        let workflow = load_workflow_file(&entry, source)?;
        registry
            .workflows
            .insert(workflow.workflow_id.clone(), workflow.clone());
        registry.hooks.retain(|hook| {
            hook.workflow_id != workflow.workflow_id
                || !matches!(
                    hook.source.as_ref(),
                    Some(src) if src.path.as_deref() == Some(entry.to_string_lossy().as_ref())
                )
        });
        registry.hooks.extend(workflow.hooks.clone());
    }
    for entry in collect_yaml_files(&source.root.join("hooks"))? {
        registry.hooks.extend(load_hook_file(&entry, source)?);
    }
    Ok(())
}

fn collect_yaml_files(dir: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(files),
        Err(err) => return Err(err.into()),
    };
    for entry in entries {
        let path = entry?.path();
        if path.is_dir() {
            files.extend(collect_yaml_files(&path)?);
            continue;
        }
        let ext = path
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or_default();
        if matches!(ext, "yaml" | "yml") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn load_workflow_file(path: &Path, source: &WorkflowLoadSource) -> anyhow::Result<WorkflowSpec> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let parsed = serde_yaml::from_str::<WorkflowFileEnvelope>(&raw)
        .with_context(|| format!("parse workflow yaml {}", path.display()))?;
    let workflow = parsed
        .workflow
        .ok_or_else(|| anyhow::anyhow!("missing `workflow` key"))?;
    let workflow_id = workflow
        .workflow_id
        .or(workflow.id)
        .or_else(|| {
            path.file_stem()
                .and_then(|v| v.to_str())
                .map(ToString::to_string)
        })
        .ok_or_else(|| anyhow::anyhow!("workflow id missing"))?;
    let name = workflow.name.clone().unwrap_or_else(|| workflow_id.clone());
    let source_ref = source_ref(source, path);
    let steps = workflow
        .steps
        .into_iter()
        .enumerate()
        .map(|(idx, step)| match step {
            WorkflowStepInput::String(action) => WorkflowStepSpec {
                step_id: format!("step_{}", idx + 1),
                action,
                with: None,
                knowledge: KnowledgeBinding::default(),
            },
            WorkflowStepInput::Object(step) => WorkflowStepSpec {
                step_id: step
                    .step_id
                    .or(step.id)
                    .unwrap_or_else(|| format!("step_{}", idx + 1)),
                action: step.action,
                with: step.with,
                knowledge: KnowledgeBinding::default(),
            },
        })
        .collect::<Vec<_>>();
    let mut hooks = parse_hooks_value(
        workflow.hooks.as_ref().or(parsed.hooks.as_ref()),
        &workflow_id,
        &source_ref,
    )?;
    for hook in &mut hooks {
        if hook.workflow_id.is_empty() {
            hook.workflow_id = workflow_id.clone();
        }
    }
    Ok(WorkflowSpec {
        workflow_id,
        name,
        description: workflow.description,
        enabled: workflow.enabled.unwrap_or(true),
        knowledge: KnowledgeBinding::default(),
        steps,
        hooks,
        source: Some(source_ref),
    })
}

fn load_hook_file(
    path: &Path,
    source: &WorkflowLoadSource,
) -> anyhow::Result<Vec<WorkflowHookBinding>> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    let env = serde_yaml::from_str::<WorkflowFileEnvelope>(&raw)
        .with_context(|| format!("parse hook yaml {}", path.display()))?;
    parse_hooks_value(env.hooks.as_ref(), "", &source_ref(source, path))
}

fn parse_hooks_value(
    hooks_value: Option<&Value>,
    default_workflow_id: &str,
    source_ref: &WorkflowSourceRef,
) -> anyhow::Result<Vec<WorkflowHookBinding>> {
    let Some(hooks_value) = hooks_value else {
        return Ok(Vec::new());
    };
    let shape = serde_json::from_value::<HookFileShape>(hooks_value.clone())
        .or_else(|_| serde_yaml::from_value::<HookFileShape>(serde_yaml::to_value(hooks_value)?))
        .context("parse hooks")?;
    let mut out = Vec::new();
    match shape {
        HookFileShape::Map(map) => {
            for (event, actions) in map {
                out.push(WorkflowHookBinding {
                    binding_id: format!(
                        "{}.{}",
                        default_workflow_id_or_default(default_workflow_id),
                        normalize_ident(&event)
                    ),
                    workflow_id: default_workflow_id.to_string(),
                    event,
                    enabled: true,
                    actions: actions.into_iter().map(to_action_spec).collect(),
                    source: Some(source_ref.clone()),
                });
            }
        }
        HookFileShape::List(items) => {
            for item in items {
                out.push(WorkflowHookBinding {
                    binding_id: item.binding_id.or(item.id).unwrap_or_else(|| {
                        format!(
                            "{}.{}",
                            item.workflow_id
                                .clone()
                                .or(item.workflow.clone())
                                .unwrap_or_else(|| default_workflow_id_or_default(
                                    default_workflow_id
                                )),
                            normalize_ident(&item.event)
                        )
                    }),
                    workflow_id: item
                        .workflow_id
                        .or(item.workflow)
                        .unwrap_or_else(|| default_workflow_id.to_string()),
                    event: item.event,
                    enabled: item.enabled.unwrap_or(true),
                    actions: item.actions.into_iter().map(to_action_spec).collect(),
                    source: Some(source_ref.clone()),
                });
            }
        }
    }
    Ok(out)
}

fn default_workflow_id_or_default(workflow_id: &str) -> String {
    if workflow_id.trim().is_empty() {
        "workflow".to_string()
    } else {
        workflow_id.to_string()
    }
}

fn to_action_spec(input: HookActionInput) -> WorkflowActionSpec {
    match input {
        HookActionInput::String(action) => WorkflowActionSpec { action, with: None },
        HookActionInput::Object(spec) => spec,
    }
}

fn normalize_ident(input: &str) -> String {
    input
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '/', '.'], "_")
}

fn source_ref(source: &WorkflowLoadSource, path: &Path) -> WorkflowSourceRef {
    WorkflowSourceRef {
        kind: source.kind.clone(),
        pack_id: source.pack_id.clone(),
        path: Some(path.to_string_lossy().to_string()),
    }
}

fn default_true() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn workspace_source(root: &Path) -> WorkflowLoadSource {
        WorkflowLoadSource {
            root: root.to_path_buf(),
            kind: WorkflowSourceKind::Workspace,
            pack_id: None,
        }
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(path, contents).expect("write");
    }

    #[test]
    fn loads_workflow_with_embedded_hooks() {
        let dir = tempdir().expect("dir");
        let workflows_dir = dir.path().join("workflows");
        fs::create_dir_all(&workflows_dir).expect("mkdir");
        fs::write(
            workflows_dir.join("demo.yaml"),
            r#"
workflow:
  id: build_feature
  name: Build Feature
  steps:
    - planner
    - action: verifier.run
      with:
        strict: true
  hooks:
    task_created:
      - kanban.update
      - action: slack.notify
        with:
          channel: engineering
"#,
        )
        .expect("write");
        let registry = load_registry(&[WorkflowLoadSource {
            root: dir.path().to_path_buf(),
            kind: WorkflowSourceKind::Workspace,
            pack_id: None,
        }])
        .expect("registry");
        let workflow = registry.workflows.get("build_feature").expect("workflow");
        assert_eq!(workflow.steps.len(), 2);
        assert_eq!(registry.hooks.len(), 1);
        assert_eq!(registry.hooks[0].actions.len(), 2);
    }

    #[test]
    fn workflow_spec_yaml_round_trips() {
        let spec = WorkflowSpec {
            workflow_id: "triage".to_string(),
            name: "Triage".to_string(),
            description: Some("Route incoming work".to_string()),
            enabled: true,
            knowledge: KnowledgeBinding::default(),
            steps: vec![WorkflowStepSpec {
                step_id: "classify".to_string(),
                action: "classifier.run".to_string(),
                with: Some(serde_json::json!({"mode": "strict"})),
                knowledge: KnowledgeBinding::default(),
            }],
            hooks: vec![WorkflowHookBinding {
                binding_id: "triage.task_created".to_string(),
                workflow_id: "triage".to_string(),
                event: "task_created".to_string(),
                enabled: true,
                actions: vec![WorkflowActionSpec {
                    action: "kanban.update".to_string(),
                    with: None,
                }],
                source: None,
            }],
            source: Some(WorkflowSourceRef {
                kind: WorkflowSourceKind::Pack,
                pack_id: Some("ops".to_string()),
                path: Some("packs/ops/workflows/triage.yaml".to_string()),
            }),
        };

        let encoded = serde_yaml::to_string(&spec).expect("serialize");
        let decoded: WorkflowSpec = serde_yaml::from_str(&encoded).expect("deserialize");
        assert_eq!(decoded, spec);
    }

    #[test]
    fn string_steps_get_stable_generated_step_ids() {
        let dir = tempdir().expect("dir");
        write_file(
            &dir.path().join("workflows/generated.yaml"),
            r#"
workflow:
  id: generated
  name: Generated Steps
  steps:
    - planner
    - verifier.run
"#,
        );

        let registry = load_registry(&[workspace_source(dir.path())]).expect("registry");
        let workflow = registry.workflows.get("generated").expect("workflow");
        let step_ids = workflow
            .steps
            .iter()
            .map(|step| step.step_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(step_ids, vec!["step_1", "step_2"]);
    }

    #[test]
    fn validate_registry_reports_empty_step_id() {
        let registry = WorkflowRegistry {
            workflows: HashMap::from([(
                "bad".to_string(),
                WorkflowSpec {
                    workflow_id: "bad".to_string(),
                    name: "Bad".to_string(),
                    description: None,
                    enabled: true,
                    knowledge: KnowledgeBinding::default(),
                    steps: vec![WorkflowStepSpec {
                        step_id: " ".to_string(),
                        action: "planner".to_string(),
                        with: None,
                        knowledge: KnowledgeBinding::default(),
                    }],
                    hooks: Vec::new(),
                    source: None,
                },
            )]),
            hooks: Vec::new(),
        };

        let messages = validate_registry(&registry);
        assert!(messages.iter().any(|message| {
            message.severity == WorkflowValidationSeverity::Error
                && message.message.contains("empty step_id")
        }));
    }

    #[test]
    fn validate_registry_reports_empty_step_action() {
        let registry = WorkflowRegistry {
            workflows: HashMap::from([(
                "bad".to_string(),
                WorkflowSpec {
                    workflow_id: "bad".to_string(),
                    name: "Bad".to_string(),
                    description: None,
                    enabled: true,
                    knowledge: KnowledgeBinding::default(),
                    steps: vec![WorkflowStepSpec {
                        step_id: "empty_action".to_string(),
                        action: " ".to_string(),
                        with: None,
                        knowledge: KnowledgeBinding::default(),
                    }],
                    hooks: Vec::new(),
                    source: None,
                },
            )]),
            hooks: Vec::new(),
        };

        let messages = validate_registry(&registry);
        assert!(messages.iter().any(|message| {
            message.severity == WorkflowValidationSeverity::Error
                && message.message.contains("empty action")
        }));
    }

    #[test]
    fn validate_registry_reports_dangling_hook_reference() {
        let registry = WorkflowRegistry {
            workflows: HashMap::new(),
            hooks: vec![WorkflowHookBinding {
                binding_id: "missing.task_created".to_string(),
                workflow_id: "missing".to_string(),
                event: "task_created".to_string(),
                enabled: true,
                actions: vec![WorkflowActionSpec {
                    action: "planner".to_string(),
                    with: None,
                }],
                source: None,
            }],
        };

        let messages = validate_registry(&registry);
        assert!(messages.iter().any(|message| {
            message.severity == WorkflowValidationSeverity::Error
                && message.message.contains("unknown workflow `missing`")
        }));
    }

    #[test]
    fn validate_registry_warns_for_empty_hook_actions() {
        let registry = WorkflowRegistry {
            workflows: HashMap::from([(
                "demo".to_string(),
                WorkflowSpec {
                    workflow_id: "demo".to_string(),
                    name: "Demo".to_string(),
                    description: None,
                    enabled: true,
                    knowledge: KnowledgeBinding::default(),
                    steps: vec![WorkflowStepSpec {
                        step_id: "plan".to_string(),
                        action: "planner".to_string(),
                        with: None,
                        knowledge: KnowledgeBinding::default(),
                    }],
                    hooks: Vec::new(),
                    source: None,
                },
            )]),
            hooks: vec![WorkflowHookBinding {
                binding_id: "demo.task_created".to_string(),
                workflow_id: "demo".to_string(),
                event: "task_created".to_string(),
                enabled: true,
                actions: Vec::new(),
                source: None,
            }],
        };

        let messages = validate_registry(&registry);
        assert!(messages.iter().any(|message| {
            message.severity == WorkflowValidationSeverity::Warning
                && message.message.contains("has no actions")
        }));
    }

    #[test]
    fn strict_load_accepts_valid_registered_tool_action() {
        let dir = tempdir().expect("dir");
        let workflow_path = dir.path().join("workflows/registered-tool.yaml");
        write_file(
            &workflow_path,
            r#"
workflow:
  id: registered_tool
  name: Registered Tool
  steps:
    - id: notify
      action: tool:workflow_test.notify
      with:
        channel: engineering
"#,
        );

        let action_registry = WorkflowActionRegistry::new().with_tool_schema(
            "workflow_test.notify",
            serde_json::json!({
                "type": "object",
                "required": ["channel"],
                "properties": {
                    "channel": { "type": "string" }
                },
                "additionalProperties": false
            }),
        );
        let registry = load_registry_with_options(
            &[workspace_source(dir.path())],
            &WorkflowRegistryLoadOptions::strict(action_registry),
        )
        .expect("strict registry");

        assert!(registry.workflows.contains_key("registered_tool"));
    }

    #[test]
    fn strict_load_rejects_unknown_action_with_span_context() {
        let dir = tempdir().expect("dir");
        let workflow_path = dir.path().join("workflows/unknown.yaml");
        write_file(
            &workflow_path,
            r#"
workflow:
  id: unknown_action
  name: Unknown Action
  steps:
    - id: mystery
      action: totally.unknown
"#,
        );

        let error = load_registry_with_options(
            &[workspace_source(dir.path())],
            &WorkflowRegistryLoadOptions::strict(WorkflowActionRegistry::new()),
        )
        .expect_err("unknown action should fail in strict mode");
        let message = error.to_string();
        assert!(
            message.contains("capability `totally.unknown`"),
            "{message}"
        );
        let normalized_message = message.replace('\\', "/");
        assert!(
            normalized_message.contains("workflows/unknown.yaml"),
            "{message}"
        );
        assert!(message.contains("workflow=unknown_action"), "{message}");
        assert!(message.contains("step_id=mystery"), "{message}");
        assert!(message.contains("field=action"), "{message}");
    }

    #[test]
    fn strict_load_rejects_case_variant_prefix_runtime_would_not_execute() {
        let dir = tempdir().expect("dir");
        write_file(
            &dir.path().join("workflows/case-variant.yaml"),
            r#"
workflow:
  id: case_variant
  name: Case Variant
  steps:
    - id: notify
      action: Tool:workflow_test.notify
      with:
        channel: engineering
"#,
        );

        let action_registry = WorkflowActionRegistry::new().with_tool_schema(
            "workflow_test.notify",
            serde_json::json!({
                "type": "object",
                "required": ["channel"],
                "properties": {
                    "channel": { "type": "string" }
                }
            }),
        );
        let error = load_registry_with_options(
            &[workspace_source(dir.path())],
            &WorkflowRegistryLoadOptions::strict(action_registry),
        )
        .expect_err("case-varied prefix should not validate stricter than runtime");
        let message = error.to_string();
        assert!(
            message.contains("capability `Tool:workflow_test.notify`"),
            "{message}"
        );
        assert!(message.contains("field=action"), "{message}");
    }

    #[test]
    fn strict_load_rejects_wrong_builtin_action_param_type() {
        let dir = tempdir().expect("dir");
        write_file(
            &dir.path().join("workflows/bad-approval.yaml"),
            r#"
workflow:
  id: bad_approval
  name: Bad Approval
  steps:
    - id: review_gate
      action: approval:gate
      with:
        title: 42
"#,
        );

        let error = load_registry_with_options(
            &[workspace_source(dir.path())],
            &WorkflowRegistryLoadOptions::strict(WorkflowActionRegistry::default()),
        )
        .expect_err("wrong param type should fail in strict mode");
        let message = error.to_string();
        assert!(message.contains("with.title"), "{message}");
        assert!(message.contains("must be string"), "{message}");
        assert!(message.contains("step_id=review_gate"), "{message}");
    }

    #[test]
    fn strict_load_rejects_closed_schema_without_properties() {
        let dir = tempdir().expect("dir");
        write_file(
            &dir.path().join("workflows/closed-schema.yaml"),
            r#"
workflow:
  id: closed_schema
  name: Closed Schema
  steps:
    - id: noop
      action: tool:workflow_test.noop
      with:
        unexpected: 1
"#,
        );

        let action_registry = WorkflowActionRegistry::new().with_tool_schema(
            "workflow_test.noop",
            serde_json::json!({
                "type": "object",
                "additionalProperties": false
            }),
        );
        let error = load_registry_with_options(
            &[workspace_source(dir.path())],
            &WorkflowRegistryLoadOptions::strict(action_registry),
        )
        .expect_err("closed schema should reject every undeclared key");
        let message = error.to_string();
        assert!(message.contains("with.unexpected"), "{message}");
        assert!(message.contains("not allowed"), "{message}");
    }

    #[test]
    fn strict_load_validates_mcp_tool_against_catalog_schema() {
        let dir = tempdir().expect("dir");
        write_file(
            &dir.path().join("workflows/mcp.yaml"),
            r#"
workflow:
  id: mcp_issue
  name: MCP Issue
  steps:
    - id: create_issue
      action: tool:mcp.github.create_issue
      with:
        title: File bug
"#,
        );

        let schema = serde_json::json!({
            "type": "object",
            "required": ["title"],
            "properties": {
                "title": { "type": "string" }
            },
            "additionalProperties": false
        });
        load_registry_with_options(
            &[workspace_source(dir.path())],
            &WorkflowRegistryLoadOptions::strict(
                WorkflowActionRegistry::new().with_tool_schema("mcp.github.create_issue", schema),
            ),
        )
        .expect("catalog-backed MCP action should load");

        write_file(
            &dir.path().join("workflows/mcp.yaml"),
            r#"
workflow:
  id: mcp_issue
  name: MCP Issue
  steps:
    - id: create_issue
      action: tool:mcp.github.create_issue
      with:
        title: 7
"#,
        );
        let schema = serde_json::json!({
            "type": "object",
            "required": ["title"],
            "properties": {
                "title": { "type": "string" }
            },
            "additionalProperties": false
        });
        let error = load_registry_with_options(
            &[workspace_source(dir.path())],
            &WorkflowRegistryLoadOptions::strict(
                WorkflowActionRegistry::new().with_tool_schema("mcp.github.create_issue", schema),
            ),
        )
        .expect_err("MCP tool schema should reject bad payload");
        let message = error.to_string();
        assert!(
            message.contains("tool:mcp.github.create_issue"),
            "{message}"
        );
        assert!(message.contains("with.title"), "{message}");
    }

    #[test]
    fn flat_and_explicit_embedded_hook_formats_parse_equivalently() {
        let dir = tempdir().expect("dir");
        write_file(
            &dir.path().join("workflows/flat.yaml"),
            r#"
workflow:
  id: flat
  name: Flat
  steps:
    - planner
  hooks:
    task_created:
      - kanban.update
      - action: slack.notify
        with:
          channel: engineering
"#,
        );
        write_file(
            &dir.path().join("workflows/list.yaml"),
            r#"
workflow:
  id: list
  name: List
  steps:
    - planner
  hooks:
    - event: task_created
      actions:
        - kanban.update
        - action: slack.notify
          with:
            channel: engineering
"#,
        );

        let registry = load_registry(&[workspace_source(dir.path())]).expect("registry");
        let flat = registry
            .hooks
            .iter()
            .find(|hook| hook.workflow_id == "flat")
            .expect("flat hook");
        let list = registry
            .hooks
            .iter()
            .find(|hook| hook.workflow_id == "list")
            .expect("list hook");
        assert_eq!(flat.event, list.event);
        assert_eq!(flat.enabled, list.enabled);
        assert_eq!(flat.actions, list.actions);
    }

    #[test]
    fn recursive_directory_loading_discovers_nested_workflows_and_hooks() {
        let dir = tempdir().expect("dir");
        write_file(
            &dir.path().join("workflows/nested/demo.yaml"),
            r#"
workflow:
  id: nested_demo
  name: Nested Demo
  steps:
    - planner
"#,
        );
        write_file(
            &dir.path().join("hooks/deep/events.yaml"),
            r#"
hooks:
  - workflow: nested_demo
    event: task_created
    actions:
      - verifier.run
"#,
        );

        let registry = load_registry(&[workspace_source(dir.path())]).expect("registry");
        assert!(registry.workflows.contains_key("nested_demo"));
        assert!(registry.hooks.iter().any(|hook| {
            hook.workflow_id == "nested_demo"
                && hook.event == "task_created"
                && hook.actions[0].action == "verifier.run"
        }));
    }

    #[test]
    fn malformed_nested_yaml_fails_loudly() {
        let dir = tempdir().expect("dir");
        write_file(
            &dir.path().join("workflows/nested/broken.yaml"),
            "workflow: [not: valid: yaml",
        );

        let error = load_registry(&[workspace_source(dir.path())]).expect_err("parse error");
        let message = format!("{error:#}");
        assert!(message.contains("parse workflow yaml"));
        assert!(message.contains("broken.yaml"));
    }

    #[test]
    fn workflow_file_missing_workflow_key_fails_loudly() {
        let dir = tempdir().expect("dir");
        write_file(
            &dir.path().join("workflows/missing.yaml"),
            r#"
hooks:
  task_created:
    - planner
"#,
        );

        let error = load_registry(&[workspace_source(dir.path())]).expect_err("missing workflow");
        assert!(format!("{error:#}").contains("missing `workflow` key"));
    }

    #[test]
    fn source_ref_preserves_pack_id_and_path() {
        let dir = tempdir().expect("dir");
        write_file(
            &dir.path().join("workflows/pack/demo.yaml"),
            r#"
workflow:
  id: packed
  name: Packed
  steps:
    - planner
"#,
        );

        let registry = load_registry(&[WorkflowLoadSource {
            root: dir.path().to_path_buf(),
            kind: WorkflowSourceKind::Pack,
            pack_id: Some("starter-pack".to_string()),
        }])
        .expect("registry");
        let source = registry
            .workflows
            .get("packed")
            .and_then(|workflow| workflow.source.as_ref())
            .expect("source");
        assert_eq!(source.kind, WorkflowSourceKind::Pack);
        assert_eq!(source.pack_id.as_deref(), Some("starter-pack"));
        assert!(source
            .path
            .as_deref()
            .is_some_and(|value| value.ends_with("demo.yaml")));
    }
}
