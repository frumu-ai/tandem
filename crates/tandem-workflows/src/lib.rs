use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use tandem_orchestrator::KnowledgeBinding;
use tandem_types::TenantContext;

mod mission_builder;
pub mod plan_package;

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
    Completed,
    Failed,
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
    pub status: WorkflowRunStatus,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    #[serde(default)]
    pub actions: Vec<WorkflowActionRunRecord>,
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

pub fn validate_registry(registry: &WorkflowRegistry) -> Vec<WorkflowValidationMessage> {
    let mut messages = Vec::new();
    for workflow in registry.workflows.values() {
        if workflow.steps.is_empty()
            && registry
                .hooks
                .iter()
                .all(|hook| hook.workflow_id != workflow.workflow_id)
        {
            messages.push(WorkflowValidationMessage {
                severity: WorkflowValidationSeverity::Warning,
                message: format!(
                    "workflow `{}` has no steps and no hook bindings",
                    workflow.workflow_id
                ),
            });
        }
        for step in &workflow.steps {
            if step.action.trim().is_empty() {
                messages.push(WorkflowValidationMessage {
                    severity: WorkflowValidationSeverity::Error,
                    message: format!(
                        "workflow `{}` has step `{}` with empty action",
                        workflow.workflow_id, step.step_id
                    ),
                });
            }
        }
    }
    for hook in &registry.hooks {
        if !registry.workflows.contains_key(&hook.workflow_id) {
            messages.push(WorkflowValidationMessage {
                severity: WorkflowValidationSeverity::Error,
                message: format!(
                    "hook `{}` references unknown workflow `{}`",
                    hook.binding_id, hook.workflow_id
                ),
            });
        }
        if hook.actions.is_empty() {
            messages.push(WorkflowValidationMessage {
                severity: WorkflowValidationSeverity::Warning,
                message: format!("hook `{}` has no actions", hook.binding_id),
            });
        }
    }
    messages
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
}
