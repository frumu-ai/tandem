use std::collections::{BTreeSet, HashMap};
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;

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
}

impl PackBuilderTool {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            plans: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
struct PackBuilderInput {
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    goal: Option<String>,
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

#[derive(Debug, Clone, Serialize)]
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

#[derive(Debug, Clone)]
struct PreparedPlan {
    plan_id: String,
    goal: String,
    pack_id: String,
    pack_name: String,
    version: String,
    capabilities_required: Vec<String>,
    capabilities_optional: Vec<String>,
    recommended_connectors: Vec<ConnectorCandidate>,
    selected_mcp_tools: Vec<String>,
    fallback_warnings: Vec<String>,
    required_secrets: Vec<String>,
    generated_zip_path: PathBuf,
    routine_ids: Vec<String>,
    routine_template: RoutineTemplate,
}

#[derive(Debug, Clone)]
struct RoutineTemplate {
    routine_id: String,
    name: String,
    timezone: String,
    schedule: RoutineSchedule,
    entrypoint: String,
    allowed_tools: Vec<String>,
}

#[derive(Debug, Clone)]
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
                    "mode": {"type": "string", "enum": ["preview", "apply"]},
                    "goal": {"type": "string"},
                    "plan_id": {"type": "string"},
                    "selected_connectors": {"type": "array", "items": {"type": "string"}},
                    "approve_connector_registration": {"type": "boolean"},
                    "approve_pack_install": {"type": "boolean"},
                    "approve_enable_routines": {"type": "boolean"},
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
        let input: PackBuilderInput = serde_json::from_value(args).unwrap_or_default();
        let mode = input
            .mode
            .as_deref()
            .unwrap_or("preview")
            .trim()
            .to_ascii_lowercase();

        match mode.as_str() {
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
        let mut selected_mcp_tools = BTreeSet::<String>::new();
        let mut required = Vec::<String>::new();
        let mut optional = Vec::<String>::new();
        let mut fallback_warnings = Vec::<String>::new();

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
                continue;
            }
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

        let stage_root =
            std::env::temp_dir().join(format!("tandem-pack-builder-{}", Uuid::new_v4()));
        let pack_root = stage_root.join("pack");
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

        let zip_path = stage_root.join(format!("{}-{}.zip", pack_name, version));
        zip_dir(&pack_root, &zip_path)?;

        let plan_id = format!("plan-{}", Uuid::new_v4());
        let required_secrets = derive_required_secret_refs(&recommended_connectors);

        let prepared = PreparedPlan {
            plan_id: plan_id.clone(),
            goal: goal.clone(),
            pack_id: pack_id.clone(),
            pack_name: pack_name.clone(),
            version,
            capabilities_required: required.clone(),
            capabilities_optional: optional.clone(),
            recommended_connectors: recommended_connectors.clone(),
            selected_mcp_tools: tool_ids.clone(),
            fallback_warnings: fallback_warnings.clone(),
            required_secrets: required_secrets.clone(),
            generated_zip_path: zip_path.clone(),
            routine_ids: vec![routine_template.routine_id.clone()],
            routine_template,
        };
        self.plans.write().await.insert(plan_id.clone(), prepared);

        let output = json!({
            "mode": "preview",
            "plan_id": plan_id,
            "goal": goal,
            "pack": {
                "pack_id": pack_id,
                "name": pack_name,
                "version": "0.4.1"
            },
            "connector_candidates": recommended_connectors,
            "connector_selection_required": !needs.iter().filter(|n| n.external).all(|need| {
                recommended_connectors.iter().any(|c| need.query_terms.iter().any(|q| c.slug.contains(q) || c.name.to_ascii_lowercase().contains(q)))
            }),
            "mcp_mapping": tool_ids,
            "fallback_warnings": fallback_warnings,
            "required_secrets": required_secrets,
            "zip_path": zip_path.to_string_lossy(),
            "approval_required": {
                "register_connectors": true,
                "install_pack": true,
                "enable_routines": true
            }
        });

        Ok(ToolResult {
            output: serde_json::to_string_pretty(&output).unwrap_or_else(|_| output.to_string()),
            metadata: output,
        })
    }

    async fn apply(&self, input: PackBuilderInput) -> anyhow::Result<ToolResult> {
        let Some(plan_id) = input.plan_id.as_deref() else {
            let output = json!({"error":"plan_id is required for apply"});
            return Ok(ToolResult {
                output: output.to_string(),
                metadata: output,
            });
        };

        let plan = {
            let guard = self.plans.read().await;
            guard.get(plan_id).cloned()
        };
        let Some(plan) = plan else {
            let output = json!({"error":"unknown plan_id", "plan_id": plan_id});
            return Ok(ToolResult {
                output: output.to_string(),
                metadata: output,
            });
        };

        if input.approve_connector_registration != Some(true)
            || input.approve_pack_install != Some(true)
        {
            let output = json!({
                "error": "approval_required",
                "required": {
                    "approve_connector_registration": true,
                    "approve_pack_install": true
                }
            });
            return Ok(ToolResult {
                output: output.to_string(),
                metadata: output,
            });
        }

        let all_catalog = catalog_servers();
        let selected = if input.selected_connectors.is_empty() {
            plan.recommended_connectors
                .iter()
                .take(1)
                .map(|c| c.slug.clone())
                .collect::<Vec<_>>()
        } else {
            input.selected_connectors.clone()
        };

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
        for routine_id in &plan.routine_ids {
            let mut routine = RoutineSpec {
                routine_id: routine_id.clone(),
                name: plan.routine_template.name.clone(),
                status: RoutineStatus::Paused,
                schedule: plan.routine_template.schedule.clone(),
                timezone: plan.routine_template.timezone.clone(),
                misfire_policy: RoutineMisfirePolicy::RunOnce,
                entrypoint: plan.routine_template.entrypoint.clone(),
                args: json!({
                    "prompt": plan.goal,
                    "mode": "standalone",
                    "uses_external_integrations": true,
                    "pack_id": plan.pack_id,
                    "pack_name": plan.pack_name,
                    "pack_builder_plan_id": plan.plan_id,
                }),
                allowed_tools: plan.routine_template.allowed_tools.clone(),
                output_targets: vec![format!("run/{}/report.md", routine_id)],
                creator_type: "agent".to_string(),
                creator_id: "pack_builder".to_string(),
                requires_approval: true,
                external_integrations_allowed: true,
                next_fire_at_ms: None,
                last_fired_at_ms: None,
            };
            if input.approve_enable_routines == Some(true) {
                routine.status = RoutineStatus::Active;
            }
            let stored = self
                .state
                .put_routine(routine)
                .await
                .map_err(|err| anyhow::anyhow!("failed to register routine: {:?}", err))?;
            routines_registered.push(stored.routine_id);
        }

        let preset_path = save_pack_preset(&plan, &registered_servers)?;

        let output = json!({
            "mode": "apply",
            "plan_id": plan.plan_id,
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
            "routines_registered": routines_registered,
            "routines_enabled": input.approve_enable_routines == Some(true),
            "fallback_warnings": plan.fallback_warnings,
            "pack_preset": {
                "path": preset_path.to_string_lossy().to_string(),
                "required_secrets": plan.required_secrets,
                "selected_tools": plan.selected_mcp_tools,
            }
        });

        Ok(ToolResult {
            output: serde_json::to_string_pretty(&output).unwrap_or_else(|_| output.to_string()),
            metadata: output,
        })
    }
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
    lines.push("requires_approval: true".to_string());
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
    if g.contains("email") {
        push_need("email.send", true, &["gmail", "email", "zapier"], &mut out);
    }

    push_need("question.ask", false, &["question"], &mut out);
    if out.len() == 1 {
        push_need("web.research", false, &["websearch"], &mut out);
    }
    out
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

fn derive_required_secret_refs(connectors: &[ConnectorCandidate]) -> Vec<String> {
    let mut refs = BTreeSet::<String>::new();
    for connector in connectors {
        if connector.requires_auth {
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
