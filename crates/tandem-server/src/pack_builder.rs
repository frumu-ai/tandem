use std::collections::{BTreeSet, HashMap};
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::RwLock;
use uuid::Uuid;

use tandem_tools::Tool;
use tandem_types::{ToolResult, ToolSchema};

use crate::pack_manager::PackInstallRequest;
use crate::{
    mcp_catalog, AppState, RoutineMisfirePolicy, RoutineSchedule, RoutineSpec, RoutineStatus,
};

#[derive(Clone)]
pub struct PackBuilderTool {
    state: AppState,
    plans: Arc<RwLock<HashMap<String, PreparedPlan>>>,
    plans_path: PathBuf,
    last_plan_by_session: Arc<RwLock<HashMap<String, String>>>,
    workflows: Arc<RwLock<HashMap<String, WorkflowRecord>>>,
    workflows_path: PathBuf,
}

impl PackBuilderTool {
    pub fn new(state: AppState) -> Self {
        let workflows_path = resolve_pack_builder_workflows_path();
        let plans_path = resolve_pack_builder_plans_path();
        Self {
            state,
            plans: Arc::new(RwLock::new(load_plans(&plans_path))),
            plans_path,
            last_plan_by_session: Arc::new(RwLock::new(HashMap::new())),
            workflows: Arc::new(RwLock::new(load_workflows(&workflows_path))),
            workflows_path,
        }
    }

    async fn upsert_workflow(
        &self,
        event_type: &str,
        status: WorkflowStatus,
        plan_id: &str,
        session_id: Option<&str>,
        thread_key: Option<&str>,
        goal: &str,
        metadata: &Value,
    ) {
        let now = now_ms();
        let workflow_id = format!("wf-{}", plan_id);
        let mut workflows = self.workflows.write().await;
        let created_at_ms = workflows
            .get(plan_id)
            .map(|row| row.created_at_ms)
            .unwrap_or(now);
        workflows.insert(
            plan_id.to_string(),
            WorkflowRecord {
                workflow_id: workflow_id.clone(),
                plan_id: plan_id.to_string(),
                session_id: session_id.map(ToString::to_string),
                thread_key: thread_key.map(ToString::to_string),
                goal: goal.to_string(),
                status: status.clone(),
                metadata: metadata.clone(),
                created_at_ms,
                updated_at_ms: now,
            },
        );
        retain_recent_workflows(&mut workflows, 256);
        save_workflows(&self.workflows_path, &workflows);
        drop(workflows);

        self.state.event_bus.publish(tandem_types::EngineEvent::new(
            event_type,
            json!({
                "sessionID": session_id.unwrap_or_default(),
                "threadKey": thread_key.unwrap_or_default(),
                "planID": plan_id,
                "status": workflow_status_label(&status),
                "metadata": metadata,
            }),
        ));
    }

    async fn resolve_plan_id_from_session(
        &self,
        session_id: Option<&str>,
        thread_key: Option<&str>,
    ) -> Option<String> {
        if let Some(session) = session_id {
            if let Some(thread) = thread_key {
                let scoped_key = session_thread_scope_key(session, Some(thread));
                if let Some(found) = self
                    .last_plan_by_session
                    .read()
                    .await
                    .get(&scoped_key)
                    .cloned()
                {
                    return Some(found);
                }
            }
        }
        if let Some(session) = session_id {
            if let Some(found) = self.last_plan_by_session.read().await.get(session).cloned() {
                return Some(found);
            }
        }
        let workflows = self.workflows.read().await;
        let mut best: Option<(&String, u64)> = None;
        for (plan_id, wf) in workflows.iter() {
            if !matches!(wf.status, WorkflowStatus::PreviewPending) {
                continue;
            }
            if session_id.is_some() && wf.session_id.as_deref() != session_id {
                continue;
            }
            if let Some(thread) = thread_key {
                if wf.thread_key.as_deref() != Some(thread) {
                    continue;
                }
            }
            let ts = wf.updated_at_ms;
            if best.map(|(_, b)| ts > b).unwrap_or(true) {
                best = Some((plan_id, ts));
            }
        }
        best.map(|(plan_id, _)| plan_id.clone())
    }

    fn emit_metric(
        &self,
        metric: &str,
        plan_id: &str,
        status: &str,
        session_id: Option<&str>,
        thread_key: Option<&str>,
    ) {
        let surface = infer_surface(thread_key);
        self.state.event_bus.publish(tandem_types::EngineEvent::new(
            "pack_builder.metric",
            json!({
                "metric": metric,
                "value": 1,
                "surface": surface,
                "planID": plan_id,
                "status": status,
                "sessionID": session_id.unwrap_or_default(),
                "threadKey": thread_key.unwrap_or_default(),
            }),
        ));
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PackBuilderInput {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    goal: Option<String>,
    #[serde(default)]
    auto_apply: Option<bool>,
    #[serde(default)]
    selected_connectors: Vec<String>,
    #[serde(default)]
    plan_id: Option<String>,
    #[serde(default)]
    approve_connector_registration: Option<bool>,
    #[serde(default)]
    approve_pack_install: Option<bool>,
    #[serde(default)]
    approve_enable_routines: Option<bool>,
    #[serde(default)]
    schedule: Option<PreviewScheduleInput>,
    #[serde(default, rename = "__session_id")]
    session_id: Option<String>,
    #[serde(default)]
    thread_key: Option<String>,
    #[serde(default)]
    secret_refs_confirmed: Option<Value>,
    /// Execution architecture: "single" | "team" | "swarm"
    /// - single: one agent loop (current default fallback)
    /// - team: orchestrated agent team with planner + workers
    /// - swarm: context-run swarm (parallel sub-tasks)
    #[serde(default)]
    execution_mode: Option<String>,
    /// For swarm mode: max parallel sub-tasks
    #[serde(default)]
    max_agents: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PreviewScheduleInput {
    #[serde(default)]
    interval_seconds: Option<u64>,
    #[serde(default)]
    cron: Option<String>,
    #[serde(default)]
    timezone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConnectorCandidate {
    slug: String,
    name: String,
    description: String,
    documentation_url: String,
    transport_url: String,
    requires_auth: bool,
    requires_setup: bool,
    tool_count: usize,
    score: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PreparedPlan {
    plan_id: String,
    goal: String,
    pack_id: String,
    pack_name: String,
    version: String,
    capabilities_required: Vec<String>,
    capabilities_optional: Vec<String>,
    recommended_connectors: Vec<ConnectorCandidate>,
    selected_connector_slugs: Vec<String>,
    selected_mcp_tools: Vec<String>,
    fallback_warnings: Vec<String>,
    required_secrets: Vec<String>,
    generated_zip_path: PathBuf,
    routine_ids: Vec<String>,
    routine_template: RoutineTemplate,
    created_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum WorkflowStatus {
    PreviewPending,
    ApplyBlockedMissingSecrets,
    ApplyBlockedAuth,
    ApplyComplete,
    Cancelled,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkflowRecord {
    workflow_id: String,
    plan_id: String,
    session_id: Option<String>,
    thread_key: Option<String>,
    goal: String,
    status: WorkflowStatus,
    metadata: Value,
    created_at_ms: u64,
    updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RoutineTemplate {
    routine_id: String,
    name: String,
    timezone: String,
    schedule: RoutineSchedule,
    entrypoint: String,
    allowed_tools: Vec<String>,
}

fn automation_v2_schedule_from_routine(
    schedule: &RoutineSchedule,
    timezone: &str,
) -> crate::AutomationV2Schedule {
    match schedule {
        RoutineSchedule::IntervalSeconds { seconds } => crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Interval,
            cron_expression: None,
            interval_seconds: Some(*seconds),
            timezone: timezone.to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        RoutineSchedule::Cron { expression } => crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Cron,
            cron_expression: Some(expression.clone()),
            interval_seconds: None,
            timezone: timezone.to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
    }
}

fn build_pack_builder_automation(
    plan: &PreparedPlan,
    routine_id: &str,
    execution_mode: &str,
    max_agents: u32,
    registered_servers: &[String],
    routine_enabled: bool,
) -> crate::AutomationV2Spec {
    let now = now_ms();
    let automation_id = format!("automation.{}", routine_id);
    crate::AutomationV2Spec {
        automation_id: automation_id.clone(),
        name: format!("{} automation", plan.pack_name),
        description: Some(format!(
            "Pack Builder automation for `{}` generated from plan `{}`.",
            plan.pack_name, plan.plan_id
        )),
        // Pack Builder still uses the routine as the active trigger wrapper today.
        // Keep the mirrored automation paused so apply does not double-register
        // two active schedulable runtimes for the same pack.
        status: crate::AutomationV2Status::Paused,
        schedule: automation_v2_schedule_from_routine(
            &plan.routine_template.schedule,
            &plan.routine_template.timezone,
        ),
        agents: vec![crate::AutomationAgentProfile {
            agent_id: "pack_builder_agent".to_string(),
            template_id: None,
            display_name: plan.pack_name.clone(),
            avatar_url: None,
            model_policy: None,
            skills: vec![plan.pack_id.clone()],
            tool_policy: crate::AutomationAgentToolPolicy {
                allowlist: plan.routine_template.allowed_tools.clone(),
                denylist: Vec::new(),
            },
            mcp_policy: crate::AutomationAgentMcpPolicy {
                allowed_servers: registered_servers.to_vec(),
                allowed_tools: None,
            },
            approval_policy: None,
        }],
        flow: crate::AutomationFlowSpec {
            nodes: vec![crate::AutomationFlowNode {
                node_id: "pack_builder_execute".to_string(),
                agent_id: "pack_builder_agent".to_string(),
                objective: format!(
                    "Execute the installed pack `{}` for this goal: {}",
                    plan.pack_name, plan.goal
                ),
                depends_on: Vec::new(),
                input_refs: Vec::new(),
                output_contract: Some(crate::AutomationFlowOutputContract {
                    kind: "report_markdown".to_string(),
                    validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                    schema: None,
                    summary_guidance: None,
                }),
                retry_policy: Some(json!({ "max_attempts": 3 })),
                timeout_ms: None,
                stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                gate: None,
                metadata: Some(json!({
                    "builder": {
                        "origin": "pack_builder",
                        "task_kind": "pack_recipe",
                        "execution_mode": execution_mode,
                    },
                    "pack_builder": {
                        "pack_id": plan.pack_id,
                        "pack_name": plan.pack_name,
                        "plan_id": plan.plan_id,
                        "routine_id": routine_id,
                    }
                })),
            }],
        },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(max_agents.clamp(1, 16)),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: vec![format!("run/{routine_id}/report.md")],
        created_at_ms: now,
        updated_at_ms: now,
        creator_id: "pack_builder".to_string(),
        workspace_root: None,
        metadata: Some(json!({
            "origin": "pack_builder",
            "pack_builder_plan_id": plan.plan_id,
            "pack_id": plan.pack_id,
            "pack_name": plan.pack_name,
            "goal": plan.goal,
            "execution_mode": execution_mode,
            "routine_id": routine_id,
            "activation_mode": "routine_wrapper_mirror",
            "routine_enabled": routine_enabled,
            "registered_servers": registered_servers,
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CapabilityNeed {
    id: String,
    external: bool,
    query_terms: Vec<String>,
}

#[derive(Debug, Clone)]
struct CatalogServer {
    slug: String,
    name: String,
    description: String,
    documentation_url: String,
    transport_url: String,
    requires_auth: bool,
    requires_setup: bool,
    tool_names: Vec<String>,
}

#[derive(Clone)]
struct McpBridgeTool {
    schema: ToolSchema,
    mcp: tandem_runtime::McpRegistry,
    server_name: String,
    tool_name: String,
}

#[async_trait]
impl Tool for McpBridgeTool {
    fn schema(&self) -> ToolSchema {
        self.schema.clone()
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.mcp
            .call_tool(&self.server_name, &self.tool_name, args)
            .await
            .map_err(anyhow::Error::msg)
    }
}

#[async_trait]
impl Tool for PackBuilderTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "pack_builder".to_string(),
            description: "MCP-first Tandem pack builder with preview/apply phases".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "mode": {"type": "string", "enum": ["preview", "apply", "cancel", "pending"]},
                    "goal": {"type": "string"},
                    "auto_apply": {"type": "boolean"},
                    "plan_id": {"type": "string"},
                    "thread_key": {"type": "string"},
                    "secret_refs_confirmed": {"oneOf":[{"type":"boolean"},{"type":"array","items":{"type":"string"}}]},
                    "selected_connectors": {"type": "array", "items": {"type": "string"}},
                    "approve_connector_registration": {"type": "boolean"},
                    "approve_pack_install": {"type": "boolean"},
                    "approve_enable_routines": {"type": "boolean"},
                    "execution_mode": {
                        "type": "string",
                        "enum": ["single", "team", "swarm"],
                        "description": "Execution architecture: single agent, orchestrated team, or parallel swarm"
                    },
                    "max_agents": {"type": "integer", "minimum": 2, "maximum": 32},
                    "schedule": {
                        "type": "object",
                        "properties": {
                            "interval_seconds": {"type": "integer", "minimum": 30},
                            "cron": {"type": "string"},
                            "timezone": {"type": "string"}
                        }
                    }
                },
                "required": ["mode"]
            }),
        }
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let mut input: PackBuilderInput = serde_json::from_value(args).unwrap_or_default();
        let mut mode = input
            .mode
            .as_deref()
            .unwrap_or("preview")
            .trim()
            .to_ascii_lowercase();

        if mode == "apply" && input.plan_id.is_none() {
            input.plan_id = self
                .resolve_plan_id_from_session(
                    input.session_id.as_deref(),
                    input.thread_key.as_deref(),
                )
                .await;
        }

        if mode == "preview" {
            let goal_text = input.goal.as_deref().map(str::trim).unwrap_or("");
            if is_confirmation_goal_text(goal_text) {
                if let Some(last_plan_id) = self
                    .resolve_plan_id_from_session(
                        input.session_id.as_deref(),
                        input.thread_key.as_deref(),
                    )
                    .await
                {
                    input.mode = Some("apply".to_string());
                    input.plan_id = Some(last_plan_id);
                    input.approve_pack_install = Some(true);
                    input.approve_connector_registration = Some(true);
                    input.approve_enable_routines = Some(true);
                    mode = "apply".to_string();
                }
            }
        }

        match mode.as_str() {
            "cancel" => self.cancel(input).await,
            "pending" => self.pending(input).await,
            "apply" => self.apply(input).await,
            _ => self.preview(input).await,
        }
    }
}

impl PackBuilderTool {
    async fn preview(&self, input: PackBuilderInput) -> anyhow::Result<ToolResult> {
        let goal = input
            .goal
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("Create a useful automation pack")
            .to_string();

        let needs = infer_capabilities_from_goal(&goal);
        let all_catalog = catalog_servers();
        let builtin_tools = available_builtin_tools(&self.state).await;
        let mut recommended_connectors = Vec::<ConnectorCandidate>::new();
        let mut selected_connector_slugs = BTreeSet::<String>::new();
        let mut selected_mcp_tools = BTreeSet::<String>::new();
        let mut required = Vec::<String>::new();
        let mut optional = Vec::<String>::new();
        let mut fallback_warnings = Vec::<String>::new();
        let mut unresolved_external_needs = Vec::<String>::new();
        let mut resolved_needs = BTreeSet::<String>::new();

        for need in &needs {
            if need.external {
                required.push(need.id.clone());
            } else {
                optional.push(need.id.clone());
            }
            if !need.external {
                continue;
            }
            if need_satisfied_by_builtin(&builtin_tools, need) {
                resolved_needs.insert(need.id.clone());
                continue;
            }
            unresolved_external_needs.push(need.id.clone());
            let mut candidates = score_candidates_for_need(&all_catalog, need);
            if candidates.is_empty() {
                fallback_warnings.push(format!(
                    "No MCP connector found for capability `{}`. Falling back to built-in tools.",
                    need.id
                ));
                continue;
            }
            candidates.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.slug.cmp(&b.slug)));
            if let Some(best) = candidates.first() {
                if should_auto_select_connector(need, best) {
                    selected_connector_slugs.insert(best.slug.clone());
                    resolved_needs.insert(need.id.clone());
                    if let Some(server) = all_catalog.iter().find(|s| s.slug == best.slug) {
                        for tool in server.tool_names.iter().take(3) {
                            selected_mcp_tools.insert(format!(
                                "mcp.{}.{}",
                                namespace_segment(&server.slug),
                                namespace_segment(tool)
                            ));
                        }
                    }
                }
            }
            recommended_connectors.extend(candidates.into_iter().take(3));
        }

        recommended_connectors
            .sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.slug.cmp(&b.slug)));
        recommended_connectors.dedup_by(|a, b| a.slug == b.slug);

        let schedule = build_schedule(input.schedule.as_ref());
        let pack_slug = goal_to_slug(&goal);
        let pack_id = format!("tpk_pack_builder_{}", pack_slug);
        let pack_name = format!("pack-builder-{}", pack_slug);
        let version = "0.4.1".to_string();

        // Use the persistent state dir for staging – NOT temp_dir() which OSes
        // clean up arbitrarily. The zip must outlive the preview phase so that
        // apply() can still find it even if several minutes pass between the two.
        let zips_dir = resolve_pack_builder_zips_dir();
        fs::create_dir_all(&zips_dir)?;
        let stage_id = Uuid::new_v4();
        let pack_root = zips_dir.join(format!("stage-{}", stage_id)).join("pack");
        fs::create_dir_all(pack_root.join("agents"))?;
        fs::create_dir_all(pack_root.join("missions"))?;
        fs::create_dir_all(pack_root.join("routines"))?;

        let mission_id = "default".to_string();
        let routine_id = "default".to_string();
        let tool_ids = selected_mcp_tools.iter().cloned().collect::<Vec<_>>();
        let routine_template = RoutineTemplate {
            routine_id: format!("{}.{}", pack_id, routine_id),
            name: format!("{} routine", pack_name),
            timezone: schedule.2.clone(),
            schedule: schedule.0.clone(),
            entrypoint: "mission.default".to_string(),
            allowed_tools: build_allowed_tools(&tool_ids, &needs),
        };

        let mission_yaml = render_mission_yaml(&mission_id, &tool_ids, &needs);
        let agent_md = render_agent_md(&tool_ids, &goal);
        let routine_yaml = render_routine_yaml(
            &routine_id,
            &schedule.0,
            &schedule.1,
            &schedule.2,
            &routine_template.allowed_tools,
        );
        let manifest_yaml = render_manifest_yaml(
            &pack_id,
            &pack_name,
            &version,
            &required,
            &optional,
            &mission_id,
            &routine_id,
        );

        fs::write(pack_root.join("missions/default.yaml"), mission_yaml)?;
        fs::write(pack_root.join("agents/default.md"), agent_md)?;
        fs::write(pack_root.join("routines/default.yaml"), routine_yaml)?;
        fs::write(pack_root.join("tandempack.yaml"), manifest_yaml)?;
        fs::write(pack_root.join("README.md"), "# Generated by pack_builder\n")?;

        // Save the zip into the same persistent dir (parent of pack_root)
        let zip_path = pack_root
            .parent()
            .expect("pack_root always has a parent staging dir")
            .join(format!("{}-{}.zip", pack_name, version));
        zip_dir(&pack_root, &zip_path)?;

        let plan_id = format!("plan-{}", Uuid::new_v4());
        let selected_connector_slugs = selected_connector_slugs.into_iter().collect::<Vec<_>>();
        let required_secrets =
            derive_required_secret_refs_for_selected(&all_catalog, &selected_connector_slugs);
        let connector_selection_required = unresolved_external_needs
            .iter()
            .any(|need_id| !resolved_needs.contains(need_id));
        let auto_apply_requested = input.auto_apply.unwrap_or(true);
        let auto_apply_ready = auto_apply_requested
            && !connector_selection_required
            && required_secrets.is_empty()
            && fallback_warnings.is_empty();

        let prepared = PreparedPlan {
            plan_id: plan_id.clone(),
            goal: goal.clone(),
            pack_id: pack_id.clone(),
            pack_name: pack_name.clone(),
            version,
            capabilities_required: required.clone(),
            capabilities_optional: optional.clone(),
            recommended_connectors: recommended_connectors.clone(),
            selected_connector_slugs: selected_connector_slugs.clone(),
            selected_mcp_tools: tool_ids.clone(),
            fallback_warnings: fallback_warnings.clone(),
            required_secrets: required_secrets.clone(),
            generated_zip_path: zip_path.clone(),
            routine_ids: vec![routine_template.routine_id.clone()],
            routine_template,
            created_at_ms: now_ms(),
        };
        {
            let mut plans = self.plans.write().await;
            plans.insert(plan_id.clone(), prepared);
            retain_recent_plans(&mut plans, 256);
            save_plans(&self.plans_path, &plans);
        }
        if let Some(session_id) = input
            .session_id
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            let mut last = self.last_plan_by_session.write().await;
            last.insert(session_id.to_string(), plan_id.clone());
            if let Some(thread_key) = input
                .thread_key
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                last.insert(
                    session_thread_scope_key(session_id, Some(thread_key)),
                    plan_id.clone(),
                );
            }
        }

        let output = json!({
            "workflow_id": format!("wf-{}", plan_id),
            "mode": "preview",
            "plan_id": plan_id,
            "session_id": input.session_id,
            "thread_key": input.thread_key,
            "goal": goal,
            "pack": {
                "pack_id": pack_id,
                "name": pack_name,
                "version": "0.4.1"
            },
            "connector_candidates": recommended_connectors,
            "selected_connectors": selected_connector_slugs,
            "connector_selection_required": connector_selection_required,
            "mcp_mapping": tool_ids,
            "fallback_warnings": fallback_warnings,
            "required_secrets": required_secrets,
            "zip_path": zip_path.to_string_lossy(),
            "auto_apply_requested": auto_apply_requested,
            "auto_apply_ready": auto_apply_ready,
            "status": "preview_pending",
            "next_actions": build_preview_next_actions(
                connector_selection_required,
                &required_secrets,
                !selected_connector_slugs.is_empty(),
            ),
            "approval_required": {
                "register_connectors": false,
                "install_pack": false,
                "enable_routines": false
            }
        });

        self.emit_metric(
            "pack_builder.preview.count",
            plan_id.as_str(),
            "preview_pending",
            input.session_id.as_deref(),
            input.thread_key.as_deref(),
        );

        if auto_apply_ready {
            let applied = self
                .apply(PackBuilderInput {
                    mode: Some("apply".to_string()),
                    goal: None,
                    auto_apply: Some(false),
                    selected_connectors: selected_connector_slugs.clone(),
                    plan_id: Some(plan_id.clone()),
                    approve_connector_registration: Some(true),
                    approve_pack_install: Some(true),
                    approve_enable_routines: Some(true),
                    schedule: None,
                    session_id: input.session_id.clone(),
                    thread_key: input.thread_key.clone(),
                    secret_refs_confirmed: Some(json!(true)),
                    // Forward the execution mode from the preview input
                    execution_mode: input.execution_mode.clone(),
                    max_agents: input.max_agents,
                })
                .await?;
            let mut metadata = applied.metadata.clone();
            if let Some(obj) = metadata.as_object_mut() {
                obj.insert("auto_applied_from_preview".to_string(), json!(true));
                obj.insert("preview_plan_id".to_string(), json!(plan_id));
            }
            self.upsert_workflow(
                "pack_builder.apply_completed",
                WorkflowStatus::ApplyComplete,
                plan_id.as_str(),
                input.session_id.as_deref(),
                input.thread_key.as_deref(),
                goal.as_str(),
                &metadata,
            )
            .await;
            return Ok(ToolResult {
                output: render_pack_builder_apply_output(&metadata),
                metadata,
            });
        }

        self.upsert_workflow(
            "pack_builder.preview_ready",
            WorkflowStatus::PreviewPending,
            plan_id.as_str(),
            input.session_id.as_deref(),
            input.thread_key.as_deref(),
            goal.as_str(),
            &output,
        )
        .await;

        Ok(ToolResult {
            output: render_pack_builder_preview_output(&output),
            metadata: output,
        })
    }

    async fn apply(&self, input: PackBuilderInput) -> anyhow::Result<ToolResult> {
        let resolved_plan_id = if input.plan_id.is_none() {
            self.resolve_plan_id_from_session(
                input.session_id.as_deref(),
                input.thread_key.as_deref(),
            )
            .await
        } else {
            input.plan_id.clone()
        };
        let Some(plan_id) = resolved_plan_id.as_deref() else {
            self.emit_metric(
                "pack_builder.apply.wrong_plan_prevented",
                "unknown",
                "error",
                input.session_id.as_deref(),
                input.thread_key.as_deref(),
            );
            let output = json!({"error":"plan_id is required for apply"});
            self.upsert_workflow(
                "pack_builder.error",
                WorkflowStatus::Error,
                "unknown",
                input.session_id.as_deref(),
                input.thread_key.as_deref(),
                input.goal.as_deref().unwrap_or_default(),
                &output,
            )
            .await;
            return Ok(ToolResult {
                output: render_pack_builder_apply_output(&output),
                metadata: output,
            });
        };

        let plan = {
            let guard = self.plans.read().await;
            guard.get(plan_id).cloned()
        };
        let Some(plan) = plan else {
            self.emit_metric(
                "pack_builder.apply.wrong_plan_prevented",
                plan_id,
                "error",
                input.session_id.as_deref(),
                input.thread_key.as_deref(),
            );
            let output = json!({"error":"unknown plan_id", "plan_id": plan_id});
            self.upsert_workflow(
                "pack_builder.error",
                WorkflowStatus::Error,
                plan_id,
                input.session_id.as_deref(),
                input.thread_key.as_deref(),
                input.goal.as_deref().unwrap_or_default(),
                &output,
            )
            .await;
            return Ok(ToolResult {
                output: render_pack_builder_apply_output(&output),
                metadata: output,
            });
        };

        let session_id = input.session_id.as_deref();
        let thread_key = input.thread_key.as_deref();
        if self
            .workflows
            .read()
            .await
            .get(plan_id)
            .map(|wf| matches!(wf.status, WorkflowStatus::Cancelled))
            .unwrap_or(false)
        {
            let output = json!({
                "error":"plan_cancelled",
                "plan_id": plan_id,
                "status":"cancelled",
                "next_actions": ["Create a new preview to continue."]
            });
            return Ok(ToolResult {
                output: render_pack_builder_apply_output(&output),
                metadata: output,
            });
        }

        self.emit_metric(
            "pack_builder.apply.count",
            plan_id,
            "apply_started",
            session_id,
            thread_key,
        );

        if input.approve_pack_install != Some(true) {
            let output = json!({
                "error": "approval_required",
                "required": {
                    "approve_pack_install": true
                },
                "status": "error"
            });
            self.upsert_workflow(
                "pack_builder.error",
                WorkflowStatus::Error,
                plan_id,
                session_id,
                thread_key,
                &plan.goal,
                &output,
            )
            .await;
            return Ok(ToolResult {
                output: render_pack_builder_apply_output(&output),
                metadata: output,
            });
        }

        let all_catalog = catalog_servers();
        let selected = if input.selected_connectors.is_empty() {
            plan.selected_connector_slugs.clone()
        } else {
            input.selected_connectors.clone()
        };
        if !selected.is_empty() && input.approve_connector_registration != Some(true) {
            let output = json!({
                "error": "approval_required",
                "required": {
                    "approve_connector_registration": true,
                    "approve_pack_install": true
                },
                "status": "error"
            });
            self.upsert_workflow(
                "pack_builder.error",
                WorkflowStatus::Error,
                plan_id,
                session_id,
                thread_key,
                &plan.goal,
                &output,
            )
            .await;
            return Ok(ToolResult {
                output: render_pack_builder_apply_output(&output),
                metadata: output,
            });
        }

        if !plan.required_secrets.is_empty()
            && !secret_refs_confirmed(&input.secret_refs_confirmed, &plan.required_secrets)
        {
            let output = json!({
                "workflow_id": format!("wf-{}", plan.plan_id),
                "mode": "apply",
                "plan_id": plan.plan_id,
                "session_id": input.session_id,
                "thread_key": input.thread_key,
                "goal": plan.goal,
                "status": "apply_blocked_missing_secrets",
                "required_secrets": plan.required_secrets,
                "next_actions": [
                    "Set required secrets in engine settings/environment.",
                    "Re-run apply with `secret_refs_confirmed` after secrets are set."
                ],
            });
            self.upsert_workflow(
                "pack_builder.apply_blocked",
                WorkflowStatus::ApplyBlockedMissingSecrets,
                plan_id,
                session_id,
                thread_key,
                &plan.goal,
                &output,
            )
            .await;
            self.emit_metric(
                "pack_builder.apply.blocked_missing_secrets",
                plan_id,
                "apply_blocked_missing_secrets",
                session_id,
                thread_key,
            );
            return Ok(ToolResult {
                output: render_pack_builder_apply_output(&output),
                metadata: output,
            });
        }

        let auth_blocked = selected.iter().any(|slug| {
            plan.recommended_connectors
                .iter()
                .any(|c| &c.slug == slug && (c.requires_setup || c.transport_url.contains('{')))
        });
        if auth_blocked {
            let output = json!({
                "workflow_id": format!("wf-{}", plan.plan_id),
                "mode": "apply",
                "plan_id": plan.plan_id,
                "session_id": input.session_id,
                "thread_key": input.thread_key,
                "goal": plan.goal,
                "status": "apply_blocked_auth",
                "selected_connectors": selected,
                "next_actions": [
                    "Complete connector setup/auth from the connector documentation.",
                    "Re-run apply after connector auth is completed."
                ],
            });
            self.upsert_workflow(
                "pack_builder.apply_blocked",
                WorkflowStatus::ApplyBlockedAuth,
                plan_id,
                session_id,
                thread_key,
                &plan.goal,
                &output,
            )
            .await;
            self.emit_metric(
                "pack_builder.apply.blocked_auth",
                plan_id,
                "apply_blocked_auth",
                session_id,
                thread_key,
            );
            return Ok(ToolResult {
                output: render_pack_builder_apply_output(&output),
                metadata: output,
            });
        }

        self.state.event_bus.publish(tandem_types::EngineEvent::new(
            "pack_builder.apply_started",
            json!({
                "sessionID": session_id.unwrap_or_default(),
                "threadKey": thread_key.unwrap_or_default(),
                "planID": plan_id,
                "status": "apply_started",
            }),
        ));

        if !plan.generated_zip_path.exists() {
            let output = json!({
                "workflow_id": format!("wf-{}", plan.plan_id),
                "mode": "apply",
                "plan_id": plan.plan_id,
                "session_id": input.session_id,
                "thread_key": input.thread_key,
                "goal": plan.goal,
                "status": "apply_blocked_missing_preview_artifacts",
                "error": "preview_artifacts_missing",
                "next_actions": [
                    "Run a new Pack Builder preview for this goal.",
                    "Confirm apply from the new preview."
                ]
            });
            self.upsert_workflow(
                "pack_builder.apply_blocked",
                WorkflowStatus::Error,
                plan_id,
                session_id,
                thread_key,
                &plan.goal,
                &output,
            )
            .await;
            return Ok(ToolResult {
                output: render_pack_builder_apply_output(&output),
                metadata: output,
            });
        }

        let mut connector_results = Vec::<Value>::new();
        let mut registered_servers = Vec::<String>::new();

        for slug in &selected {
            let Some(server) = all_catalog.iter().find(|s| &s.slug == slug) else {
                connector_results
                    .push(json!({"slug": slug, "ok": false, "error": "not_in_catalog"}));
                continue;
            };
            let transport = if server.transport_url.contains('{') || server.transport_url.is_empty()
            {
                connector_results.push(json!({
                    "slug": server.slug,
                    "ok": false,
                    "error": "transport_requires_manual_setup",
                    "documentation_url": server.documentation_url
                }));
                continue;
            } else {
                server.transport_url.clone()
            };

            let name = server.slug.clone();
            self.state
                .mcp
                .add_or_update(name.clone(), transport, HashMap::new(), true)
                .await;
            let connected = self.state.mcp.connect(&name).await;
            let tool_count = if connected {
                sync_mcp_tools_for_server(&self.state, &name).await
            } else {
                0
            };
            if connected {
                registered_servers.push(name.clone());
            }
            connector_results.push(json!({
                "slug": server.slug,
                "ok": connected,
                "registered_name": name,
                "tool_count": tool_count,
                "documentation_url": server.documentation_url,
                "requires_auth": server.requires_auth
            }));
        }

        let installed = self
            .state
            .pack_manager
            .install(PackInstallRequest {
                path: Some(plan.generated_zip_path.to_string_lossy().to_string()),
                url: None,
                source: json!({"kind":"pack_builder", "plan_id": plan.plan_id, "goal": plan.goal}),
            })
            .await?;

        let mut routines_registered = Vec::<String>::new();
        let mut automations_registered = Vec::<String>::new();
        for routine_id in &plan.routine_ids {
            let exec_mode = input
                .execution_mode
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .unwrap_or("team");
            let max_agents = input.max_agents.unwrap_or(4);
            let mut routine = RoutineSpec {
                routine_id: routine_id.clone(),
                name: plan.routine_template.name.clone(),
                status: RoutineStatus::Active,
                schedule: plan.routine_template.schedule.clone(),
                timezone: plan.routine_template.timezone.clone(),
                misfire_policy: RoutineMisfirePolicy::RunOnce,
                entrypoint: plan.routine_template.entrypoint.clone(),
                args: json!({
                    "prompt": plan.goal,
                    // execution_mode controls how the orchestrator handles this routine:
                    // "single"  → one agent loop (simple tasks)
                    // "team"    → orchestrated agent team with planner + specialist workers
                    // "swarm"   → context-run based swarm with parallel sub-tasks
                    "mode": exec_mode,
                    "uses_external_integrations": true,
                    "pack_id": plan.pack_id,
                    "pack_name": plan.pack_name,
                    "pack_builder_plan_id": plan.plan_id,
                    // team/swarm configuration hints for the orchestrator
                    "orchestration": {
                        "execution_mode": exec_mode,
                        "max_agents": max_agents,
                        "objective": plan.goal,
                    },
                }),
                allowed_tools: plan.routine_template.allowed_tools.clone(),
                output_targets: vec![format!("run/{}/report.md", routine_id)],
                creator_type: "agent".to_string(),
                creator_id: "pack_builder".to_string(),
                requires_approval: false,
                external_integrations_allowed: true,
                next_fire_at_ms: None,
                last_fired_at_ms: None,
            };
            if input.approve_enable_routines == Some(false) {
                routine.status = RoutineStatus::Paused;
            }
            let automation = build_pack_builder_automation(
                &plan,
                routine_id,
                exec_mode,
                max_agents,
                &registered_servers,
                input.approve_enable_routines != Some(false),
            );
            let stored_automation = self.state.put_automation_v2(automation).await?;
            automations_registered.push(stored_automation.automation_id.clone());
            let stored = self
                .state
                .put_routine(routine)
                .await
                .map_err(|err| anyhow::anyhow!("failed to register routine: {:?}", err))?;
            routines_registered.push(stored.routine_id);
        }

        let preset_path = save_pack_preset(&plan, &registered_servers)?;

        let output = json!({
            "workflow_id": format!("wf-{}", plan.plan_id),
            "mode": "apply",
            "plan_id": plan.plan_id,
            "session_id": input.session_id,
            "thread_key": input.thread_key,
            "capabilities": {
                "required": plan.capabilities_required,
                "optional": plan.capabilities_optional
            },
            "pack_installed": {
                "pack_id": installed.pack_id,
                "name": installed.name,
                "version": installed.version,
                "install_path": installed.install_path,
            },
            "connectors": connector_results,
            "registered_servers": registered_servers,
            "automations_registered": automations_registered,
            "routines_registered": routines_registered,
            "routines_enabled": input.approve_enable_routines != Some(false),
            "fallback_warnings": plan.fallback_warnings,
            "status": "apply_complete",
            "next_actions": [
                "Review the installed pack in Packs view.",
                "Routine is enabled by default and will run on schedule."
            ],
            "pack_preset": {
                "path": preset_path.to_string_lossy().to_string(),
                "required_secrets": plan.required_secrets,
                "selected_tools": plan.selected_mcp_tools,
            }
        });

        self.upsert_workflow(
            "pack_builder.apply_completed",
            WorkflowStatus::ApplyComplete,
            plan_id,
            session_id,
            thread_key,
            &plan.goal,
            &output,
        )
        .await;
        self.emit_metric(
            "pack_builder.apply.success",
            plan_id,
            "apply_complete",
            session_id,
            thread_key,
        );

        Ok(ToolResult {
            output: render_pack_builder_apply_output(&output),
            metadata: output,
        })
    }

    async fn cancel(&self, input: PackBuilderInput) -> anyhow::Result<ToolResult> {
        let plan_id = if let Some(plan_id) = input.plan_id.as_deref().map(str::trim) {
            if !plan_id.is_empty() {
                Some(plan_id.to_string())
            } else {
                None
            }
        } else {
            self.resolve_plan_id_from_session(
                input.session_id.as_deref(),
                input.thread_key.as_deref(),
            )
            .await
        };
        let Some(plan_id) = plan_id else {
            let output = json!({"error":"plan_id is required for cancel"});
            return Ok(ToolResult {
                output: render_pack_builder_apply_output(&output),
                metadata: output,
            });
        };
        let goal = self
            .plans
            .read()
            .await
            .get(&plan_id)
            .map(|p| p.goal.clone())
            .unwrap_or_default();
        let output = json!({
            "workflow_id": format!("wf-{}", plan_id),
            "mode": "cancel",
            "plan_id": plan_id,
            "session_id": input.session_id,
            "thread_key": input.thread_key,
            "goal": goal,
            "status": "cancelled",
            "next_actions": ["Create a new preview when ready."]
        });
        self.upsert_workflow(
            "pack_builder.cancelled",
            WorkflowStatus::Cancelled,
            output
                .get("plan_id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            input.session_id.as_deref(),
            input.thread_key.as_deref(),
            output
                .get("goal")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            &output,
        )
        .await;
        self.emit_metric(
            "pack_builder.apply.cancelled",
            output
                .get("plan_id")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            "cancelled",
            input.session_id.as_deref(),
            input.thread_key.as_deref(),
        );
        Ok(ToolResult {
            output: "Pack Builder Apply Cancelled\n- Pending plan cancelled.".to_string(),
            metadata: output,
        })
    }

    async fn pending(&self, input: PackBuilderInput) -> anyhow::Result<ToolResult> {
        let plan_id = if let Some(plan_id) = input.plan_id.as_deref().map(str::trim) {
            if !plan_id.is_empty() {
                Some(plan_id.to_string())
            } else {
                None
            }
        } else {
            self.resolve_plan_id_from_session(
                input.session_id.as_deref(),
                input.thread_key.as_deref(),
            )
            .await
        };
        let Some(plan_id) = plan_id else {
            let output = json!({"status":"none","pending":null});
            return Ok(ToolResult {
                output: "No pending pack-builder plan for this session.".to_string(),
                metadata: output,
            });
        };
        let workflows = self.workflows.read().await;
        let Some(record) = workflows.get(&plan_id) else {
            let output = json!({"status":"none","plan_id":plan_id});
            return Ok(ToolResult {
                output: "No pending pack-builder plan found.".to_string(),
                metadata: output,
            });
        };
        let output = json!({
            "status":"ok",
            "pending": record,
            "plan_id": plan_id
        });
        Ok(ToolResult {
            output: serde_json::to_string_pretty(&output).unwrap_or_else(|_| output.to_string()),
            metadata: output,
        })
    }
}

fn render_pack_builder_preview_output(meta: &Value) -> String {
    let goal = meta
        .get("goal")
        .and_then(Value::as_str)
        .unwrap_or("automation goal");
    let plan_id = meta.get("plan_id").and_then(Value::as_str).unwrap_or("-");
    let pack_name = meta
        .get("pack")
        .and_then(|v| v.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("generated-pack");
    let pack_id = meta
        .get("pack")
        .and_then(|v| v.get("pack_id"))
        .and_then(Value::as_str)
        .unwrap_or("-");
    let auto_apply_ready = meta
        .get("auto_apply_ready")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let connector_selection_required = meta
        .get("connector_selection_required")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let selected_connectors = meta
        .get("selected_connectors")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(|v| format!("- {}", v))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let required_secrets = meta
        .get("required_secrets")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(|v| format!("- {}", v))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let fallback_warnings = meta
        .get("fallback_warnings")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(|v| format!("- {}", v))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut lines = vec![
        "Pack Builder Preview".to_string(),
        format!("- Goal: {}", goal),
        format!("- Plan ID: {}", plan_id),
        format!("- Pack: {} ({})", pack_name, pack_id),
    ];

    if selected_connectors.is_empty() {
        lines.push("- Selected connectors: none".to_string());
    } else {
        lines.push("- Selected connectors:".to_string());
        lines.extend(selected_connectors);
    }
    if required_secrets.is_empty() {
        lines.push("- Required secrets: none".to_string());
    } else {
        lines.push("- Required secrets:".to_string());
        lines.extend(required_secrets);
    }
    if !fallback_warnings.is_empty() {
        lines.push("- Warnings:".to_string());
        lines.extend(fallback_warnings);
    }

    if auto_apply_ready {
        lines.push("- Status: ready for automatic apply".to_string());
    } else {
        lines.push("- Status: waiting for apply confirmation".to_string());
        if connector_selection_required {
            lines.push("- Action needed: choose connectors before apply.".to_string());
        }
    }
    lines.join("\n")
}

fn render_pack_builder_apply_output(meta: &Value) -> String {
    if let Some(status) = meta.get("status").and_then(Value::as_str) {
        match status {
            "apply_blocked_missing_secrets" => {
                let required = meta
                    .get("required_secrets")
                    .and_then(Value::as_array)
                    .map(|rows| {
                        rows.iter()
                            .filter_map(Value::as_str)
                            .map(|v| format!("- {}", v))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let mut lines = vec![
                    "Pack Builder Apply Blocked".to_string(),
                    "- Reason: missing required secrets.".to_string(),
                ];
                if !required.is_empty() {
                    lines.push("- Required secrets:".to_string());
                    lines.extend(required);
                }
                lines.push("- Action: set secrets, then apply again.".to_string());
                return lines.join("\n");
            }
            "apply_blocked_auth" => {
                let connectors = meta
                    .get("selected_connectors")
                    .and_then(Value::as_array)
                    .map(|rows| {
                        rows.iter()
                            .filter_map(Value::as_str)
                            .map(|v| format!("- {}", v))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let mut lines = vec![
                    "Pack Builder Apply Blocked".to_string(),
                    "- Reason: connector authentication/setup required.".to_string(),
                ];
                if !connectors.is_empty() {
                    lines.push("- Connectors awaiting setup:".to_string());
                    lines.extend(connectors);
                }
                lines.push("- Action: complete connector auth, then apply again.".to_string());
                return lines.join("\n");
            }
            "cancelled" => {
                return "Pack Builder Apply Cancelled\n- Pending plan cancelled.".to_string();
            }
            "apply_blocked_missing_preview_artifacts" => {
                return "Pack Builder Apply Blocked\n- Preview artifacts expired. Run preview again, then confirm.".to_string();
            }
            _ => {}
        }
    }

    if let Some(error) = meta.get("error").and_then(Value::as_str) {
        return match error {
            "approval_required" => {
                "Pack Builder Apply Blocked\n- Approval required for this apply step.".to_string()
            }
            "unknown plan_id" => "Pack Builder Apply Failed\n- Plan not found.".to_string(),
            "plan_cancelled" => {
                "Pack Builder Apply Failed\n- Plan was already cancelled.".to_string()
            }
            _ => format!("Pack Builder Apply Failed\n- {}", error),
        };
    }

    let pack_id = meta
        .get("pack_installed")
        .and_then(|v| v.get("pack_id"))
        .and_then(Value::as_str)
        .unwrap_or("-");
    let pack_name = meta
        .get("pack_installed")
        .and_then(|v| v.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("-");
    let install_path = meta
        .get("pack_installed")
        .and_then(|v| v.get("install_path"))
        .and_then(Value::as_str)
        .unwrap_or("-");
    let routines_enabled = meta
        .get("routines_enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let registered_servers = meta
        .get("registered_servers")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(|v| format!("- {}", v))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let routines = meta
        .get("routines_registered")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(|v| format!("- {}", v))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut lines = vec![
        "Pack Builder Apply Complete".to_string(),
        format!("- Installed pack: {} ({})", pack_name, pack_id),
        format!("- Install path: {}", install_path),
        format!(
            "- Routines: {}",
            if routines_enabled {
                "enabled"
            } else {
                "paused"
            }
        ),
    ];

    if registered_servers.is_empty() {
        lines.push("- Registered connectors: none".to_string());
    } else {
        lines.push("- Registered connectors:".to_string());
        lines.extend(registered_servers);
    }
    if !routines.is_empty() {
        lines.push("- Registered routines:".to_string());
        lines.extend(routines);
    }

    lines.join("\n")
}

fn resolve_pack_builder_workflows_path() -> PathBuf {
    if let Ok(dir) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("pack_builder_workflows.json");
        }
    }
    if let Some(data_dir) = dirs::data_dir() {
        return data_dir
            .join("tandem")
            .join("data")
            .join("pack_builder_workflows.json");
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".tandem")
        .join("data")
        .join("pack_builder_workflows.json")
}

fn resolve_pack_builder_plans_path() -> PathBuf {
    if let Ok(dir) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("pack_builder_plans.json");
        }
    }
    if let Some(data_dir) = dirs::data_dir() {
        return data_dir
            .join("tandem")
            .join("data")
            .join("pack_builder_plans.json");
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".tandem")
        .join("data")
        .join("pack_builder_plans.json")
}

/// Returns the directory for persistent pack zip staging.
/// Zips are stored here (not in temp_dir) so they survive until apply() runs.
fn resolve_pack_builder_zips_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("pack_builder_zips");
        }
    }
    if let Some(data_dir) = dirs::data_dir() {
        return data_dir
            .join("tandem")
            .join("data")
            .join("pack_builder_zips");
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".tandem")
        .join("data")
        .join("pack_builder_zips")
}

fn load_workflows(path: &PathBuf) -> HashMap<String, WorkflowRecord> {
    let Ok(bytes) = fs::read(path) else {
        return HashMap::new();
    };
    serde_json::from_slice::<HashMap<String, WorkflowRecord>>(&bytes).unwrap_or_default()
}

fn save_workflows(path: &PathBuf, workflows: &HashMap<String, WorkflowRecord>) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(workflows) {
        let _ = fs::write(path, bytes);
    }
}

fn load_plans(path: &PathBuf) -> HashMap<String, PreparedPlan> {
    let Ok(bytes) = fs::read(path) else {
        return HashMap::new();
    };
    serde_json::from_slice::<HashMap<String, PreparedPlan>>(&bytes).unwrap_or_default()
}

fn save_plans(path: &PathBuf, plans: &HashMap<String, PreparedPlan>) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(plans) {
        let _ = fs::write(path, bytes);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn retain_recent_workflows(workflows: &mut HashMap<String, WorkflowRecord>, keep: usize) {
    if workflows.len() <= keep {
        return;
    }
    let mut rows = workflows
        .iter()
        .map(|(key, value)| (key.clone(), value.updated_at_ms))
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    let keep_keys = rows
        .into_iter()
        .take(keep)
        .map(|(key, _)| key)
        .collect::<BTreeSet<_>>();
    workflows.retain(|key, _| keep_keys.contains(key));
}

fn retain_recent_plans(plans: &mut HashMap<String, PreparedPlan>, keep: usize) {
    if plans.len() <= keep {
        return;
    }
    let mut rows = plans
        .iter()
        .map(|(key, value)| {
            (
                key.clone(),
                value.created_at_ms,
                value.generated_zip_path.clone(),
            )
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    let mut keep_keys = BTreeSet::<String>::new();
    let mut evict_zips = Vec::<PathBuf>::new();
    for (i, (key, _, zip_path)) in rows.iter().enumerate() {
        if i < keep {
            keep_keys.insert(key.clone());
        } else {
            evict_zips.push(zip_path.clone());
        }
    }
    plans.retain(|key, _| keep_keys.contains(key));
    // Best-effort removal of the staging directories for evicted plans
    for zip in evict_zips {
        if let Some(stage_dir) = zip.parent() {
            let _ = fs::remove_dir_all(stage_dir);
        }
    }
}

fn session_thread_scope_key(session_id: &str, thread_key: Option<&str>) -> String {
    let thread = thread_key.unwrap_or_default().trim();
    if thread.is_empty() {
        return session_id.trim().to_string();
    }
    format!("{}::{}", session_id.trim(), thread)
}

fn workflow_status_label(status: &WorkflowStatus) -> &'static str {
    match status {
        WorkflowStatus::PreviewPending => "preview_pending",
        WorkflowStatus::ApplyBlockedMissingSecrets => "apply_blocked_missing_secrets",
        WorkflowStatus::ApplyBlockedAuth => "apply_blocked_auth",
        WorkflowStatus::ApplyComplete => "apply_complete",
        WorkflowStatus::Cancelled => "cancelled",
        WorkflowStatus::Error => "error",
    }
}

fn infer_surface(thread_key: Option<&str>) -> &'static str {
    let key = thread_key.unwrap_or_default().to_lowercase();
    if key.starts_with("telegram:") {
        "telegram"
    } else if key.starts_with("discord:") {
        "discord"
    } else if key.starts_with("slack:") {
        "slack"
    } else if key.starts_with("desktop:") || key.starts_with("tauri:") {
        "tauri"
    } else if key.starts_with("web:") || key.starts_with("control-panel:") {
        "web"
    } else {
        "unknown"
    }
}

fn build_preview_next_actions(
    connector_selection_required: bool,
    required_secrets: &[String],
    has_connector_registration: bool,
) -> Vec<String> {
    let mut actions = Vec::new();
    if connector_selection_required {
        actions.push("Select connector(s) before applying.".to_string());
    }
    if !required_secrets.is_empty() {
        actions.push("Set required secrets in engine settings/environment.".to_string());
    }
    if has_connector_registration {
        actions.push("Confirm connector registration and pack install.".to_string());
    } else {
        actions.push("Apply to install the generated pack.".to_string());
    }
    actions
}

fn secret_refs_confirmed(confirmed: &Option<Value>, required: &[String]) -> bool {
    if required.is_empty() {
        return true;
    }
    if env_has_all_required_secrets(required) {
        return true;
    }
    let Some(value) = confirmed else {
        return false;
    };
    if value.as_bool() == Some(true) {
        return true;
    }
    let Some(rows) = value.as_array() else {
        return false;
    };
    let confirmed = rows
        .iter()
        .filter_map(Value::as_str)
        .map(|v| v.trim().to_ascii_uppercase())
        .collect::<BTreeSet<_>>();
    required
        .iter()
        .all(|item| confirmed.contains(&item.to_ascii_uppercase()))
}

fn env_has_all_required_secrets(required: &[String]) -> bool {
    required.iter().all(|key| {
        std::env::var(key)
            .ok()
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    })
}

fn build_schedule(input: Option<&PreviewScheduleInput>) -> (RoutineSchedule, String, String) {
    let timezone = input
        .and_then(|v| v.timezone.as_deref())
        .filter(|v| !v.trim().is_empty())
        .unwrap_or("UTC")
        .to_string();

    if let Some(cron) = input
        .and_then(|v| v.cron.as_deref())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return (
            RoutineSchedule::Cron {
                expression: cron.to_string(),
            },
            "cron".to_string(),
            timezone,
        );
    }

    let seconds = input
        .and_then(|v| v.interval_seconds)
        .unwrap_or(86_400)
        .clamp(30, 31_536_000);

    (
        RoutineSchedule::IntervalSeconds { seconds },
        format!("every_{}_seconds", seconds),
        timezone,
    )
}

fn build_allowed_tools(mcp_tools: &[String], needs: &[CapabilityNeed]) -> Vec<String> {
    let mut out = BTreeSet::<String>::new();
    for tool in mcp_tools {
        out.insert(tool.clone());
    }
    out.insert("question".to_string());
    if needs.iter().any(|n| !n.external) {
        out.insert("read".to_string());
        out.insert("write".to_string());
    }
    if needs
        .iter()
        .any(|n| n.id.contains("news") || n.id.contains("headline"))
    {
        out.insert("websearch".to_string());
        out.insert("webfetch".to_string());
    }
    out.into_iter().collect()
}

fn render_mission_yaml(mission_id: &str, mcp_tools: &[String], needs: &[CapabilityNeed]) -> String {
    let mut lines = vec![
        format!("id: {}", mission_id),
        "title: Generated Pack Builder Mission".to_string(),
        "steps:".to_string(),
    ];

    let mut step_idx = 1usize;
    for tool in mcp_tools {
        lines.push(format!("  - id: step_{}", step_idx));
        lines.push(format!("    action: {}", tool));
        step_idx += 1;
    }

    if mcp_tools.is_empty() {
        lines.push("  - id: step_1".to_string());
        lines.push("    action: websearch".to_string());
    }

    for need in needs {
        lines.push(format!("  - id: verify_{}", namespace_segment(&need.id)));
        lines.push("    action: question".to_string());
        lines.push("    optional: true".to_string());
    }

    lines.join("\n") + "\n"
}

fn render_agent_md(mcp_tools: &[String], goal: &str) -> String {
    let mut lines = vec![
        "---".to_string(),
        "name: default".to_string(),
        "description: Generated MCP-first pack agent".to_string(),
        "---".to_string(),
        "".to_string(),
        "You are the Pack Builder runtime agent for this routine.".to_string(),
        format!("Mission goal: {}", goal),
        "Use the mission steps exactly and invoke the discovered MCP tools explicitly.".to_string(),
        "".to_string(),
        "Discovered MCP tool IDs: ".to_string(),
    ];

    if mcp_tools.is_empty() {
        lines
            .push("- (none discovered; fallback to built-ins is allowed for this run)".to_string());
    } else {
        for tool in mcp_tools {
            lines.push(format!("- {}", tool));
        }
    }

    lines.push("".to_string());
    lines.push("If a required connector is missing or unauthorized, report it and stop before side effects.".to_string());
    lines.join("\n") + "\n"
}

fn render_routine_yaml(
    routine_id: &str,
    schedule: &RoutineSchedule,
    schedule_label: &str,
    timezone: &str,
    allowed_tools: &[String],
) -> String {
    let mut lines = vec![format!("id: {}", routine_id), "trigger:".to_string()];

    match schedule {
        RoutineSchedule::Cron { expression } => {
            lines.push("  type: cron".to_string());
            lines.push(format!("  expression: \"{}\"", expression));
        }
        RoutineSchedule::IntervalSeconds { seconds } => {
            lines.push("  type: interval_seconds".to_string());
            lines.push(format!("  seconds: {}", seconds));
        }
    }
    lines.push("mission_id: default".to_string());
    lines.push("enabled_by_default: false".to_string());
    lines.push("".to_string());

    lines.push(format!("routine_id: {}", routine_id));
    lines.push(format!("name: {}", schedule_label));
    lines.push(format!("timezone: {}", timezone));
    match schedule {
        RoutineSchedule::Cron { expression } => {
            lines.push("schedule:".to_string());
            lines.push(format!("  cron: \"{}\"", expression));
        }
        RoutineSchedule::IntervalSeconds { seconds } => {
            lines.push("schedule:".to_string());
            lines.push(format!("  interval_seconds: {}", seconds));
        }
    }
    lines.push("entrypoint: mission.default".to_string());
    lines.push("allowed_tools:".to_string());
    for tool in allowed_tools {
        lines.push(format!("  - {}", tool));
    }
    lines.push("output_targets:".to_string());
    lines.push(format!("  - run/{}/report.md", routine_id));
    lines.push("requires_approval: false".to_string());
    lines.push("external_integrations_allowed: true".to_string());
    lines.join("\n") + "\n"
}

fn render_manifest_yaml(
    pack_id: &str,
    pack_name: &str,
    version: &str,
    required: &[String],
    optional: &[String],
    mission_id: &str,
    routine_id: &str,
) -> String {
    let mut lines = vec![
        "manifest_schema_version: 1".to_string(),
        format!("pack_id: \"{}\"", pack_id),
        format!("name: {}", pack_name),
        format!("version: {}", version),
        "type: workflow".to_string(),
        "entrypoints:".to_string(),
        format!("  missions: [\"{}\"]", mission_id),
        format!("  routines: [\"{}\"]", routine_id),
        "capabilities:".to_string(),
        "  required:".to_string(),
    ];

    if required.is_empty() {
        lines.push("    - websearch".to_string());
    } else {
        for cap in required {
            lines.push(format!("    - {}", cap));
        }
    }

    lines.push("  optional:".to_string());
    for cap in optional {
        lines.push(format!("    - {}", cap));
    }
    if optional.is_empty() {
        lines.push("    - question".to_string());
    }

    lines.push("contents:".to_string());
    lines.push("  agents:".to_string());
    lines.push("    - id: default".to_string());
    lines.push("      path: agents/default.md".to_string());
    lines.push("  missions:".to_string());
    lines.push(format!("    - id: {}", mission_id));
    lines.push("      path: missions/default.yaml".to_string());
    lines.push("  routines:".to_string());
    lines.push(format!("    - id: {}", routine_id));
    lines.push("      path: routines/default.yaml".to_string());
    lines.join("\n") + "\n"
}

fn infer_capabilities_from_goal(goal: &str) -> Vec<CapabilityNeed> {
    let g = goal.to_ascii_lowercase();
    let mut out = Vec::<CapabilityNeed>::new();
    let push_need = |id: &str, external: bool, terms: &[&str], out: &mut Vec<CapabilityNeed>| {
        if out.iter().any(|n| n.id == id) {
            return;
        }
        out.push(CapabilityNeed {
            id: id.to_string(),
            external,
            query_terms: terms.iter().map(|v| v.to_string()).collect(),
        });
    };

    if g.contains("notion") {
        push_need("notion.read_write", true, &["notion"], &mut out);
    }
    if g.contains("slack") {
        push_need("slack.post_message", true, &["slack"], &mut out);
    }
    if g.contains("stripe") || g.contains("payment") {
        push_need("stripe.read_write", true, &["stripe"], &mut out);
    }
    if g.contains("github") || g.contains("pr") {
        push_need("github.read_write", true, &["github"], &mut out);
    }
    if g.contains("headline") || g.contains("news") {
        push_need("news.latest", true, &["news", "zapier"], &mut out);
    }
    if g.contains("email") || contains_email_address(goal) {
        push_need("email.send", true, &["gmail", "email", "zapier"], &mut out);
    }

    push_need("question.ask", false, &["question"], &mut out);
    if out.len() == 1 {
        push_need("web.research", false, &["websearch"], &mut out);
    }
    out
}

fn contains_email_address(text: &str) -> bool {
    text.split_whitespace().any(|token| {
        let token = token.trim_matches(|ch: char| {
            ch.is_ascii_punctuation() && ch != '@' && ch != '.' && ch != '_' && ch != '-'
        });
        let mut parts = token.split('@');
        let local = parts.next().unwrap_or_default();
        let domain = parts.next().unwrap_or_default();
        let no_extra = parts.next().is_none();
        no_extra
            && !local.is_empty()
            && domain.contains('.')
            && domain
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')
    })
}

fn is_confirmation_goal_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "ok" | "okay"
            | "yes"
            | "y"
            | "confirm"
            | "confirmed"
            | "approve"
            | "approved"
            | "go"
            | "go ahead"
            | "proceed"
            | "do it"
            | "ship it"
            | "run it"
            | "apply"
    )
}

fn catalog_servers() -> Vec<CatalogServer> {
    let mut out = Vec::<CatalogServer>::new();
    let Some(index) = mcp_catalog::index() else {
        return out;
    };
    let rows = index
        .get("servers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for row in rows {
        let slug = row.get("slug").and_then(Value::as_str).unwrap_or("").trim();
        if slug.is_empty() {
            continue;
        }
        let transport = row
            .get("transport_url")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let tool_names = row
            .get("tool_names")
            .and_then(Value::as_array)
            .map(|vals| {
                vals.iter()
                    .filter_map(Value::as_str)
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        out.push(CatalogServer {
            slug: slug.to_string(),
            name: row
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(slug)
                .to_string(),
            description: row
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            documentation_url: row
                .get("documentation_url")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            transport_url: transport,
            requires_auth: row
                .get("requires_auth")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            requires_setup: row
                .get("requires_setup")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            tool_names,
        });
    }
    out
}

fn score_candidates_for_need(
    catalog: &[CatalogServer],
    need: &CapabilityNeed,
) -> Vec<ConnectorCandidate> {
    let mut out = Vec::<ConnectorCandidate>::new();
    for server in catalog {
        let mut score = 0usize;
        let hay = format!(
            "{} {} {} {}",
            server.slug,
            server.name.to_ascii_lowercase(),
            server.description.to_ascii_lowercase(),
            server.tool_names.join(" ").to_ascii_lowercase()
        );
        for term in &need.query_terms {
            if hay.contains(&term.to_ascii_lowercase()) {
                score += 3;
            }
        }
        if need.id.contains("news") && hay.contains("news") {
            score += 4;
        }
        if score == 0 {
            continue;
        }
        out.push(ConnectorCandidate {
            slug: server.slug.clone(),
            name: server.name.clone(),
            description: server.description.clone(),
            documentation_url: server.documentation_url.clone(),
            transport_url: server.transport_url.clone(),
            requires_auth: server.requires_auth,
            requires_setup: server.requires_setup,
            tool_count: server.tool_names.len(),
            score,
        });
    }
    out
}

fn should_auto_select_connector(need: &CapabilityNeed, candidate: &ConnectorCandidate) -> bool {
    match need.id.as_str() {
        "email.send" => {
            if candidate.score < 6 {
                return false;
            }
            let hay = format!(
                "{} {} {}",
                candidate.slug.to_ascii_lowercase(),
                candidate.name.to_ascii_lowercase(),
                candidate.description.to_ascii_lowercase()
            );
            let looks_like_marketing = ["crm", "campaign", "marketing", "sales"]
                .iter()
                .any(|term| hay.contains(term));
            let looks_like_mail_delivery = [
                "email",
                "mail",
                "gmail",
                "smtp",
                "sendgrid",
                "mailgun",
                "outlook",
                "office365",
            ]
            .iter()
            .any(|term| hay.contains(term));
            if looks_like_marketing && !looks_like_mail_delivery {
                return false;
            }
            true
        }
        _ => true,
    }
}

async fn available_builtin_tools(state: &AppState) -> BTreeSet<String> {
    state
        .tools
        .list()
        .await
        .into_iter()
        .map(|schema| schema.name)
        .filter(|name| !name.starts_with("mcp."))
        .collect()
}

fn need_satisfied_by_builtin(builtin_tools: &BTreeSet<String>, need: &CapabilityNeed) -> bool {
    let has = |name: &str| builtin_tools.contains(name);
    match need.id.as_str() {
        "news.latest" | "web.research" => has("websearch") && has("webfetch"),
        "question.ask" => has("question"),
        _ => false,
    }
}

fn derive_required_secret_refs_for_selected(
    catalog: &[CatalogServer],
    selected_connectors: &[String],
) -> Vec<String> {
    let mut refs = BTreeSet::<String>::new();
    for slug in selected_connectors {
        if let Some(connector) = catalog.iter().find(|row| &row.slug == slug) {
            if !connector.requires_auth {
                continue;
            }
            refs.insert(format!(
                "{}_TOKEN",
                connector.slug.to_ascii_uppercase().replace('-', "_")
            ));
        }
    }
    refs.into_iter().collect()
}

fn goal_to_slug(goal: &str) -> String {
    let mut out = String::new();
    for ch in goal.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= 42 {
            break;
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "automation".to_string()
    } else {
        trimmed.to_string()
    }
}

fn namespace_segment(raw: &str) -> String {
    let mut out = String::new();
    let mut prev_sep = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_sep = false;
        } else if !prev_sep {
            out.push('_');
            prev_sep = true;
        }
    }
    let trimmed = out.trim_matches('_');
    if trimmed.is_empty() {
        "tool".to_string()
    } else {
        trimmed.to_string()
    }
}

async fn sync_mcp_tools_for_server(state: &AppState, name: &str) -> usize {
    let prefix = format!("mcp.{}.", namespace_segment(name));
    state.tools.unregister_by_prefix(&prefix).await;
    let tools = state.mcp.server_tools(name).await;
    for tool in &tools {
        let schema = ToolSchema {
            name: tool.namespaced_name.clone(),
            description: if tool.description.trim().is_empty() {
                format!("MCP tool {} from {}", tool.tool_name, tool.server_name)
            } else {
                tool.description.clone()
            },
            input_schema: tool.input_schema.clone(),
        };
        state
            .tools
            .register_tool(
                schema.name.clone(),
                Arc::new(McpBridgeTool {
                    schema,
                    mcp: state.mcp.clone(),
                    server_name: tool.server_name.clone(),
                    tool_name: tool.tool_name.clone(),
                }),
            )
            .await;
    }
    tools.len()
}

fn save_pack_preset(plan: &PreparedPlan, registered_servers: &[String]) -> anyhow::Result<PathBuf> {
    let paths = tandem_core::resolve_shared_paths().context("resolve shared paths")?;
    let dir = paths
        .canonical_root
        .join("presets")
        .join("overrides")
        .join("pack_presets");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.yaml", plan.pack_id));

    let mut content = String::new();
    content.push_str(&format!("id: {}\n", plan.pack_id));
    content.push_str(&format!("version: {}\n", plan.version));
    content.push_str("kind: pack_preset\n");
    content.push_str("pack:\n");
    content.push_str(&format!("  pack_id: {}\n", plan.pack_id));
    content.push_str(&format!("  name: {}\n", plan.pack_name));
    content.push_str(&format!(
        "  goal: |\n    {}\n",
        plan.goal.replace('\n', "\n    ")
    ));
    content.push_str("connectors:\n");
    for row in &plan.recommended_connectors {
        let selected = registered_servers.iter().any(|v| v == &row.slug);
        content.push_str(&format!("  - slug: {}\n", row.slug));
        content.push_str(&format!("    name: {}\n", row.name));
        content.push_str(&format!(
            "    documentation_url: {}\n",
            row.documentation_url
        ));
        content.push_str(&format!("    transport_url: {}\n", row.transport_url));
        content.push_str(&format!("    requires_auth: {}\n", row.requires_auth));
        content.push_str(&format!("    selected: {}\n", selected));
    }
    content.push_str("registered_servers:\n");
    for srv in registered_servers {
        content.push_str(&format!("  - {}\n", srv));
    }
    content.push_str("required_credentials:\n");
    for sec in &plan.required_secrets {
        content.push_str(&format!("  - {}\n", sec));
    }
    content.push_str("selected_mcp_tools:\n");
    for tool in &plan.selected_mcp_tools {
        content.push_str(&format!("  - {}\n", tool));
    }

    fs::write(&path, content)?;
    Ok(path)
}

fn zip_dir(src_dir: &PathBuf, output_zip: &PathBuf) -> anyhow::Result<()> {
    let file =
        File::create(output_zip).with_context(|| format!("create {}", output_zip.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    let mut stack = vec![src_dir.clone()];
    while let Some(current) = stack.pop() {
        let mut entries = fs::read_dir(&current)?
            .filter_map(|e| e.ok())
            .collect::<Vec<_>>();
        entries.sort_by_key(|e| e.path());
        for entry in entries {
            let path = entry.path();
            let rel = path
                .strip_prefix(src_dir)
                .context("strip prefix")?
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                if !rel.is_empty() {
                    zip.add_directory(format!("{}/", rel), opts)?;
                }
                stack.push(path);
                continue;
            }
            zip.start_file(rel, opts)?;
            let bytes = fs::read(&path)?;
            zip.write_all(&bytes)?;
        }
    }
    zip.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn email_send_does_not_auto_select_low_confidence_connector() {
        let need = CapabilityNeed {
            id: "email.send".to_string(),
            external: true,
            query_terms: vec!["email".to_string()],
        };
        let candidate = ConnectorCandidate {
            slug: "activecampaign".to_string(),
            name: "ActiveCampaign".to_string(),
            description: "Marketing automation and CRM workflows".to_string(),
            documentation_url: String::new(),
            transport_url: String::new(),
            requires_auth: true,
            requires_setup: false,
            tool_count: 5,
            score: 3,
        };
        assert!(!should_auto_select_connector(&need, &candidate));
    }

    #[test]
    fn email_send_allows_high_confidence_mail_connector() {
        let need = CapabilityNeed {
            id: "email.send".to_string(),
            external: true,
            query_terms: vec!["email".to_string()],
        };
        let candidate = ConnectorCandidate {
            slug: "gmail".to_string(),
            name: "Gmail".to_string(),
            description: "Send and manage email messages".to_string(),
            documentation_url: String::new(),
            transport_url: String::new(),
            requires_auth: true,
            requires_setup: false,
            tool_count: 8,
            score: 9,
        };
        assert!(should_auto_select_connector(&need, &candidate));
    }

    #[test]
    fn build_pack_builder_automation_mirrors_routine_template() {
        let plan = PreparedPlan {
            plan_id: "plan-pack-builder-test".to_string(),
            goal: "Create a daily digest pack".to_string(),
            pack_id: "daily_digest_pack".to_string(),
            pack_name: "Daily Digest Pack".to_string(),
            version: "0.1.0".to_string(),
            capabilities_required: vec!["web.search".to_string()],
            capabilities_optional: Vec::new(),
            recommended_connectors: Vec::new(),
            selected_connector_slugs: Vec::new(),
            selected_mcp_tools: Vec::new(),
            fallback_warnings: Vec::new(),
            required_secrets: Vec::new(),
            generated_zip_path: PathBuf::from("/tmp/daily-digest-pack.zip"),
            routine_ids: vec!["routine.daily_digest_pack".to_string()],
            routine_template: RoutineTemplate {
                routine_id: "routine.daily_digest_pack".to_string(),
                name: "Daily Digest Pack".to_string(),
                timezone: "UTC".to_string(),
                schedule: RoutineSchedule::Cron {
                    expression: "0 8 * * *".to_string(),
                },
                entrypoint: "packs/daily_digest_pack/run".to_string(),
                allowed_tools: vec!["web_search".to_string(), "write_file".to_string()],
            },
            created_at_ms: 0,
        };

        let automation = build_pack_builder_automation(
            &plan,
            "routine.daily_digest_pack",
            "team",
            6,
            &["slack".to_string(), "github".to_string()],
            true,
        );

        assert_eq!(
            automation.automation_id,
            "automation.routine.daily_digest_pack"
        );
        assert_eq!(automation.status, crate::AutomationV2Status::Paused);
        assert_eq!(
            automation.schedule.schedule_type,
            crate::AutomationV2ScheduleType::Cron
        );
        assert_eq!(
            automation.schedule.cron_expression.as_deref(),
            Some("0 8 * * *")
        );
        assert_eq!(automation.agents.len(), 1);
        assert_eq!(automation.flow.nodes.len(), 1);
        assert_eq!(automation.flow.nodes[0].node_id, "pack_builder_execute");
        assert_eq!(
            automation.flow.nodes[0]
                .output_contract
                .as_ref()
                .map(|contract| contract.validator.clone()),
            Some(Some(crate::AutomationOutputValidatorKind::GenericArtifact))
        );
        assert_eq!(
            automation
                .metadata
                .as_ref()
                .and_then(|v| v.get("origin"))
                .and_then(|v| v.as_str()),
            Some("pack_builder")
        );
        assert_eq!(
            automation
                .metadata
                .as_ref()
                .and_then(|v| v.get("activation_mode"))
                .and_then(|v| v.as_str()),
            Some("routine_wrapper_mirror")
        );
        assert_eq!(
            automation
                .metadata
                .as_ref()
                .and_then(|v| v.get("routine_enabled"))
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            automation
                .metadata
                .as_ref()
                .and_then(|v| v.get("pack_builder_plan_id"))
                .and_then(|v| v.as_str()),
            Some("plan-pack-builder-test")
        );
        assert_eq!(
            automation
                .metadata
                .as_ref()
                .and_then(|v| v.get("routine_id"))
                .and_then(|v| v.as_str()),
            Some("routine.daily_digest_pack")
        );
    }
}
