fn automations_help_text() -> String {
    "⚙️ *Automation Commands*\n\
/automations — list saved automations\n\
/automations show <id> — inspect one automation\n\
/automations runs <id> [limit] — list recent runs for one automation\n\
/automations run <id> — trigger an automation now\n\
/automations pause <id> — pause an automation\n\
/automations resume <id> — resume a paused automation\n\
/automations delete <id> --yes — delete an automation"
        .to_string()
}

fn runs_help_text() -> String {
    "🏃 *Run Commands*\n\
/runs — list recent automation runs\n\
/runs show <run_id> — inspect a run\n\
/runs pause <run_id> — pause a run\n\
/runs resume <run_id> — resume a paused run\n\
/runs cancel <run_id> — cancel a run\n\
/runs artifacts <run_id> — list run artifacts"
        .to_string()
}

fn memory_help_text() -> String {
    "🧠 *Memory Commands*\n\
/memory — list recent memory entries\n\
/memory search <query> — search across available memory\n\
/memory recent [limit] — list recent entries\n\
/memory save <text> — store a global note\n\
/memory scopes — show the current session/project/global scope binding\n\
/memory delete <id> --yes — delete a memory entry"
        .to_string()
}

fn public_demo_memory_help_text() -> String {
    "🧠 *Public Channel Memory Commands*\n\
/memory — list recent memory entries for this channel scope\n\
/memory search <query> — search channel-scoped public memory\n\
/memory recent [limit] — list recent channel-scoped entries\n\
/memory save <text> — store a note in this channel's public memory namespace\n\
/memory scopes — show the quarantined public memory scope for this channel\n\
/memory delete <id> --yes — delete a memory entry from this channel scope\n\
\n\
This memory is quarantined to the current public channel scope and does not read from Tandem's normal trusted project/global memory."
        .to_string()
}

fn workspace_help_text() -> String {
    "📁 *Workspace Commands*\n\
/workspace — show the current workspace binding\n\
/workspace status — show session/project/workspace status\n\
/workspace files <query> — find files by name in the workspace\n\
/workspace branch — show the current git branch"
        .to_string()
}

fn tools_help_text(security_profile: ChannelSecurityProfile) -> String {
    if security_profile == ChannelSecurityProfile::PublicDemo {
        return disabled_help_text(
            "tools",
            "Tool-scope override commands are disabled because this channel uses an enforced public security profile.",
        );
    }
    "🛠 *Tool Scope Commands*\n\
/tools — show this help\n\
/tools list — list available tools and their current state\n\
/tools enable <tool1,tool2> — enable tools for this channel\n\
/tools disable <tool1,tool2> — disable tools for this channel\n\
/tools reset — reset to default tool scope\n\
\n\
Available built-in tools: read, glob, ls, list, grep, codesearch, websearch,\nwebfetch, webfetch_html, bash, write, edit, apply_patch, todowrite, memory_search,\nmemory_store, memory_list, skill, task, question\n\n\
Use `/mcp` commands to manage MCP server access."
        .to_string()
}

fn mcp_help_text(security_profile: ChannelSecurityProfile) -> String {
    if security_profile == ChannelSecurityProfile::PublicDemo {
        return disabled_help_text(
            "mcp",
            "MCP connector commands are disabled in this public channel to avoid exposing external integrations.",
        );
    }
    "🔌 *MCP Commands*\n\
/mcp — list MCP servers\n\
/mcp tools [server] — list discovered tools\n\
/mcp resources — list discovered resources\n\
/mcp status — summarize connected servers\n\
/mcp connect <name> — connect a server\n\
/mcp disconnect <name> — disconnect a server\n\
/mcp refresh <name> — refresh a server\n\
/mcp enable <name> — enable an MCP server for this channel\n\
/mcp disable <name> — disable an MCP server for this channel"
        .to_string()
}

fn packs_help_text() -> String {
    "📦 *Pack Commands*\n\
/packs — list installed packs\n\
/packs show <selector> — inspect a pack\n\
/packs updates <selector> — check for updates\n\
/packs install <path-or-url> — install a pack\n\
/packs uninstall <selector> --yes — uninstall a pack"
        .to_string()
}

fn config_help_text(security_profile: ChannelSecurityProfile) -> String {
    if security_profile == ChannelSecurityProfile::PublicDemo {
        return disabled_help_text(
            "config",
            "Runtime configuration and model-management commands are disabled in this public channel for security.",
        );
    }
    "🛠️ *Config Commands*\n\
/config — show a runtime config summary\n\
/config providers — show provider summary\n\
/config channels — show channel status/config summary\n\
/config model — show the active default model\n\
/config set-model <model_id> — update the default model"
        .to_string()
}

async fn schedule_command_text(
    action: ScheduleCommand,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    match action {
        ScheduleCommand::Help => schedule_help_text(),
        ScheduleCommand::Plan { prompt } => {
            schedule_plan_text(prompt, msg, base_url, api_token, session_map).await
        }
        ScheduleCommand::Show { plan_id } => schedule_show_text(plan_id, base_url, api_token).await,
        ScheduleCommand::Edit { plan_id, message } => {
            schedule_edit_text(plan_id, message, base_url, api_token).await
        }
        ScheduleCommand::Reset { plan_id } => {
            schedule_reset_text(plan_id, base_url, api_token).await
        }
        ScheduleCommand::Apply { plan_id } => {
            schedule_apply_text(plan_id, base_url, api_token).await
        }
    }
}

async fn workflow_planner_workspace_root(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> Option<String> {
    let sid = active_session_id(msg, session_map).await?;
    let client = reqwest::Client::new();
    let resp = add_auth(client.get(format!("{base_url}/session/{sid}")), api_token)
        .send()
        .await
        .ok()?;
    let json = resp.json::<serde_json::Value>().await.ok()?;
    json.get("workspace_root")
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            json.get("directory")
                .and_then(|value| value.as_str())
                .filter(|value| value.starts_with('/'))
                .map(ToOwned::to_owned)
        })
}

fn workflow_plan_summary(plan: &serde_json::Value) -> String {
    let plan_id = plan
        .get("plan_id")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown");
    let title = plan
        .get("title")
        .and_then(|value| value.as_str())
        .unwrap_or("Untitled workflow");
    let workspace_root = plan
        .get("workspace_root")
        .and_then(|value| value.as_str())
        .unwrap_or("-");
    let confidence = plan
        .get("confidence")
        .and_then(|value| value.as_str())
        .unwrap_or("-");
    let step_count = plan
        .get("steps")
        .and_then(|value| value.as_array())
        .map(|items| items.len())
        .unwrap_or(0);
    let schedule = plan
        .get("schedule")
        .map(compact_json)
        .unwrap_or_else(|| "null".to_string());
    format!(
        "Plan `{}`\nTitle: {}\nSteps: {}\nConfidence: {}\nWorkspace: {}\nSchedule: {}",
        plan_id, title, step_count, confidence, workspace_root, schedule
    )
}

fn workflow_plan_change_summary(value: &serde_json::Value) -> Option<String> {
    let items = value
        .get("change_summary")
        .and_then(|entry| entry.as_array())
        .filter(|entries| !entries.is_empty())?;
    let lines = items
        .iter()
        .take(6)
        .filter_map(|item| item.as_str())
        .map(|item| format!("• {item}"))
        .collect::<Vec<_>>();
    if lines.is_empty() {
        None
    } else {
        Some(format!("Changes:\n{}", lines.join("\n")))
    }
}

fn assistant_message_text(value: &serde_json::Value) -> Option<String> {
    value
        .get("assistant_message")
        .and_then(|entry| entry.get("text"))
        .and_then(|entry| entry.as_str())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

fn compact_json(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

fn short_id(value: &str) -> String {
    value.chars().take(8).collect()
}

fn value_string<'a>(value: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(|entry| entry.as_str()))
}

fn value_u64(value: &serde_json::Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(|entry| entry.as_u64()))
}

fn yes_required_text(noun: &str, id: &str, example: &str) -> String {
    format!("⚠️ Refusing to {noun} `{id}` without confirmation.\nRun `{example} --yes` if you really want to continue.")
}

fn extract_tool_output(json: &serde_json::Value) -> String {
    json.get("output")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .to_string()
}

async fn json_request(
    method: reqwest::Method,
    path: &str,
    body: Option<serde_json::Value>,
    base_url: &str,
    api_token: &str,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("could not build HTTP client: {error}"))?;
    let mut request = add_auth(
        client.request(method, format!("{base_url}{path}")),
        api_token,
    );
    if let Some(body) = body {
        request = request.json(&body);
    }
    let resp = request
        .send()
        .await
        .map_err(|error| format!("request failed: {error}"))?;
    let status = resp.status();
    let json = resp
        .json::<serde_json::Value>()
        .await
        .unwrap_or_else(|_| serde_json::json!({}));
    if status.is_success() {
        Ok(json)
    } else {
        let detail = json
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or("request failed");
        Err(format!("{detail} (HTTP {status})"))
    }
}

async fn tool_execute(
    tool: &str,
    args: serde_json::Value,
    base_url: &str,
    api_token: &str,
) -> Result<serde_json::Value, String> {
    json_request(
        reqwest::Method::POST,
        "/tool/execute",
        Some(serde_json::json!({ "tool": tool, "args": args })),
        base_url,
        api_token,
    )
    .await
}

async fn active_session_details(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> Option<serde_json::Value> {
    let sid = active_session_id(msg, session_map).await?;
    json_request(
        reqwest::Method::GET,
        &format!("/session/{sid}"),
        None,
        base_url,
        api_token,
    )
    .await
    .ok()
}

async fn workflow_plan_post(
    path: &str,
    body: serde_json::Value,
    base_url: &str,
    api_token: &str,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("could not build HTTP client: {error}"))?;
    let resp = add_auth(client.post(format!("{base_url}{path}")), api_token)
        .json(&body)
        .send()
        .await
        .map_err(|error| format!("request failed: {error}"))?;
    let status = resp.status();
    let json = resp
        .json::<serde_json::Value>()
        .await
        .map_err(|error| format!("could not parse server response: {error}"))?;
    if status.is_success() {
        Ok(json)
    } else {
        let detail = json
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or("workflow planner request failed");
        Err(format!("{detail} (HTTP {status})"))
    }
}

async fn automations_command_text(
    action: AutomationsCommand,
    base_url: &str,
    api_token: &str,
) -> String {
    match action {
        AutomationsCommand::Help => automations_help_text(),
        AutomationsCommand::List => automations_list_text(base_url, api_token).await,
        AutomationsCommand::Show { automation_id } => {
            automation_show_text(automation_id, base_url, api_token).await
        }
        AutomationsCommand::Runs {
            automation_id,
            limit,
        } => automation_runs_text(automation_id, limit, base_url, api_token).await,
        AutomationsCommand::Run { automation_id } => {
            automation_run_now_text(automation_id, base_url, api_token).await
        }
        AutomationsCommand::Pause { automation_id } => {
            automation_pause_text(automation_id, base_url, api_token).await
        }
        AutomationsCommand::Resume { automation_id } => {
            automation_resume_text(automation_id, base_url, api_token).await
        }
        AutomationsCommand::Delete {
            automation_id,
            confirmed,
        } => automation_delete_text(automation_id, confirmed, base_url, api_token).await,
    }
}

async fn runs_command_text(action: RunsCommand, base_url: &str, api_token: &str) -> String {
    match action {
        RunsCommand::Help => runs_help_text(),
        RunsCommand::Automations { limit } => runs_list_text(limit, base_url, api_token).await,
        RunsCommand::Show { run_id } => run_show_text(run_id, base_url, api_token).await,
        RunsCommand::Pause { run_id } => run_pause_text(run_id, base_url, api_token).await,
        RunsCommand::Resume { run_id } => run_resume_text(run_id, base_url, api_token).await,
        RunsCommand::Cancel { run_id } => run_cancel_text(run_id, base_url, api_token).await,
        RunsCommand::Artifacts { run_id } => run_artifacts_text(run_id, base_url, api_token).await,
    }
}

async fn memory_command_text(
    action: MemoryCommand,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
    security_profile: ChannelSecurityProfile,
) -> String {
    match action {
        MemoryCommand::Help => {
            if security_profile == ChannelSecurityProfile::PublicDemo {
                public_demo_memory_help_text()
            } else {
                memory_help_text()
            }
        }
        MemoryCommand::Search { query } => {
            memory_search_text(
                query,
                msg,
                base_url,
                api_token,
                session_map,
                security_profile,
            )
            .await
        }
        MemoryCommand::Recent { limit } => {
            memory_recent_text(
                limit,
                msg,
                base_url,
                api_token,
                session_map,
                security_profile,
            )
            .await
        }
        MemoryCommand::Save { text } => {
            memory_save_text(
                text,
                msg,
                base_url,
                api_token,
                session_map,
                security_profile,
            )
            .await
        }
        MemoryCommand::Scopes => {
            memory_scopes_text(msg, base_url, api_token, session_map, security_profile).await
        }
        MemoryCommand::Delete {
            memory_id,
            confirmed,
        } => {
            memory_delete_text(
                memory_id,
                confirmed,
                msg,
                base_url,
                api_token,
                security_profile,
            )
            .await
        }
    }
}

async fn workspace_command_text(
    action: WorkspaceCommand,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    match action {
        WorkspaceCommand::Help => workspace_help_text(),
        WorkspaceCommand::Show => workspace_show_text(msg, base_url, api_token, session_map).await,
        WorkspaceCommand::Status => {
            workspace_status_text(msg, base_url, api_token, session_map).await
        }
        WorkspaceCommand::Files { query } => {
            workspace_files_text(query, msg, base_url, api_token, session_map).await
        }
        WorkspaceCommand::Branch => {
            workspace_branch_text(msg, base_url, api_token, session_map).await
        }
    }
}

async fn mcp_command_text(
    action: McpCommand,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
) -> String {
    match action {
        McpCommand::Help => mcp_help_text(ChannelSecurityProfile::Operator),
        McpCommand::List => mcp_list_text(base_url, api_token).await,
        McpCommand::Tools { server } => mcp_tools_text(server, base_url, api_token).await,
        McpCommand::Resources => mcp_resources_text(base_url, api_token).await,
        McpCommand::Status => mcp_status_text(base_url, api_token).await,
        McpCommand::Connect { name } => mcp_connect_text(name, base_url, api_token).await,
        McpCommand::Disconnect { name } => mcp_disconnect_text(name, base_url, api_token).await,
        McpCommand::Refresh { name } => mcp_refresh_text(name, base_url, api_token).await,
        McpCommand::ChannelEnable { name } => {
            let mut prefs = load_channel_tool_preferences(&msg.channel, &msg.scope.id).await;
            if !prefs.enabled_mcp_servers.contains(&name) {
                prefs.enabled_mcp_servers.push(name.clone());
            }
            save_channel_tool_preferences(&msg.channel, &msg.scope.id, prefs).await;
            format!("✅ MCP server `{}` enabled for this channel.", name)
        }
        McpCommand::ChannelDisable { name } => {
            let mut prefs = load_channel_tool_preferences(&msg.channel, &msg.scope.id).await;
            prefs.enabled_mcp_servers.retain(|s| s != &name);
            save_channel_tool_preferences(&msg.channel, &msg.scope.id, prefs).await;
            format!("🚫 MCP server `{}` disabled for this channel.", name)
        }
    }
}

async fn packs_command_text(action: PacksCommand, base_url: &str, api_token: &str) -> String {
    match action {
        PacksCommand::Help => packs_help_text(),
        PacksCommand::List => packs_list_text(base_url, api_token).await,
        PacksCommand::Show { selector } => packs_show_text(selector, base_url, api_token).await,
        PacksCommand::Updates { selector } => {
            packs_updates_text(selector, base_url, api_token).await
        }
        PacksCommand::Install { target } => packs_install_text(target, base_url, api_token).await,
        PacksCommand::Uninstall {
            selector,
            confirmed,
        } => packs_uninstall_text(selector, confirmed, base_url, api_token).await,
    }
}

async fn config_command_text(action: ConfigCommand, base_url: &str, api_token: &str) -> String {
    match action {
        ConfigCommand::Help => config_help_text(ChannelSecurityProfile::Operator),
        ConfigCommand::Show => config_show_text(base_url, api_token).await,
        ConfigCommand::Providers => providers_text(base_url, api_token).await,
        ConfigCommand::Channels => config_channels_text(base_url, api_token).await,
        ConfigCommand::Model => config_model_text(base_url, api_token).await,
        ConfigCommand::SetModel { model_id } => set_model_text(model_id, base_url, api_token).await,
    }
}

async fn automations_list_text(base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::GET,
        "/automations/v2",
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let items = json
                .get("automations")
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default();
            if items.is_empty() {
                return "ℹ️ No automations found.".to_string();
            }
            let lines = items
                .iter()
                .take(12)
                .map(|item| {
                    let id = value_string(item, &["id", "automationId", "automation_id"])
                        .unwrap_or("unknown");
                    let name = value_string(item, &["name"]).unwrap_or("Untitled");
                    let status = value_string(item, &["status"]).unwrap_or("unknown");
                    format!("• `{}` {} ({})", short_id(id), name, status)
                })
                .collect::<Vec<_>>();
            format!(
                "⚙️ Automations ({} total):\n{}\nUse `/automations show <id>` for details.",
                items.len(),
                lines.join("\n")
            )
        }
        Err(error) => format!("⚠️ Could not list automations: {error}"),
    }
}

async fn automation_show_text(automation_id: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::GET,
        &format!(
            "/automations/v2/{}",
            sanitize_resource_segment(&automation_id)
        ),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let automation = json.get("automation").unwrap_or(&json);
            let name = value_string(automation, &["name"]).unwrap_or("Untitled");
            let status = value_string(automation, &["status"]).unwrap_or("unknown");
            let workspace =
                value_string(automation, &["workspace_root", "workspaceRoot"]).unwrap_or("-");
            let schedule = automation
                .get("schedule")
                .map(compact_json)
                .unwrap_or_else(|| "null".to_string());
            format!(
                "⚙️ Automation `{}`\nName: {}\nStatus: {}\nWorkspace: {}\nSchedule: {}",
                automation_id, name, status, workspace, schedule
            )
        }
        Err(error) => format!("⚠️ Could not load automation `{automation_id}`: {error}"),
    }
}

async fn automation_runs_text(
    automation_id: String,
    limit: usize,
    base_url: &str,
    api_token: &str,
) -> String {
    match json_request(
        reqwest::Method::GET,
        &format!(
            "/automations/v2/{}/runs?limit={}",
            sanitize_resource_segment(&automation_id),
            limit
        ),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => format_runs_list(&json, &format!("Runs for `{automation_id}`")),
        Err(error) => format!("⚠️ Could not list runs for `{automation_id}`: {error}"),
    }
}

async fn automation_run_now_text(automation_id: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::POST,
        &format!(
            "/automations/v2/{}/run_now",
            sanitize_resource_segment(&automation_id)
        ),
        Some(serde_json::json!({})),
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let run_id = json
                .get("run")
                .and_then(|value| value.get("runId").or_else(|| value.get("run_id")))
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            format!(
                "▶️ Started automation `{automation_id}`.\nRun: `{}`",
                short_id(run_id)
            )
        }
        Err(error) => format!("⚠️ Could not run automation `{automation_id}`: {error}"),
    }
}

async fn automation_pause_text(automation_id: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::POST,
        &format!(
            "/automations/v2/{}/pause",
            sanitize_resource_segment(&automation_id)
        ),
        Some(serde_json::json!({})),
        base_url,
        api_token,
    )
    .await
    {
        Ok(_) => format!("⏸️ Paused automation `{automation_id}`."),
        Err(error) => format!("⚠️ Could not pause automation `{automation_id}`: {error}"),
    }
}

async fn automation_resume_text(automation_id: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::POST,
        &format!(
            "/automations/v2/{}/resume",
            sanitize_resource_segment(&automation_id)
        ),
        Some(serde_json::json!({})),
        base_url,
        api_token,
    )
    .await
    {
        Ok(_) => format!("▶️ Resumed automation `{automation_id}`."),
        Err(error) => format!("⚠️ Could not resume automation `{automation_id}`: {error}"),
    }
}

async fn automation_delete_text(
    automation_id: String,
    confirmed: bool,
    base_url: &str,
    api_token: &str,
) -> String {
    if !confirmed {
        return yes_required_text(
            "delete automation",
            &automation_id,
            &format!("/automations delete {automation_id}"),
        );
    }
    match json_request(
        reqwest::Method::DELETE,
        &format!(
            "/automations/v2/{}",
            sanitize_resource_segment(&automation_id)
        ),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(_) => format!("🗑️ Deleted automation `{automation_id}`."),
        Err(error) => format!("⚠️ Could not delete automation `{automation_id}`: {error}"),
    }
}

fn format_runs_list(json: &serde_json::Value, title: &str) -> String {
    let runs = json
        .get("runs")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    if runs.is_empty() {
        return format!("ℹ️ {title}: no runs found.");
    }
    let lines = runs
        .iter()
        .take(12)
        .map(|run| {
            let run_id = value_string(run, &["runId", "run_id", "id"]).unwrap_or("unknown");
            let status = value_string(run, &["status"]).unwrap_or("unknown");
            format!("• `{}` {}", short_id(run_id), status)
        })
        .collect::<Vec<_>>();
    format!("{title}\n{}", lines.join("\n"))
}

async fn runs_list_text(limit: usize, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::GET,
        &format!("/automations/v2/runs?limit={limit}"),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => format_runs_list(&json, "🏃 Recent automation runs"),
        Err(error) => format!("⚠️ Could not list runs: {error}"),
    }
}

async fn run_show_text(run_id: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::GET,
        &format!(
            "/automations/v2/runs/{}",
            sanitize_resource_segment(&run_id)
        ),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let run = json.get("run").unwrap_or(&json);
            let status = value_string(run, &["status"]).unwrap_or("unknown");
            let automation_id =
                value_string(run, &["automationId", "automation_id"]).unwrap_or("-");
            let active_sessions = run
                .get("activeSessionIds")
                .or_else(|| run.get("active_session_ids"))
                .and_then(|value| value.as_array())
                .map(|items| items.len())
                .unwrap_or(0);
            format!(
                "🏃 Run `{}`\nStatus: {}\nAutomation: {}\nActive sessions: {}",
                run_id, status, automation_id, active_sessions
            )
        }
        Err(error) => format!("⚠️ Could not load run `{run_id}`: {error}"),
    }
}

async fn run_pause_text(run_id: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::POST,
        &format!(
            "/automations/v2/runs/{}/pause",
            sanitize_resource_segment(&run_id)
        ),
        Some(serde_json::json!({})),
        base_url,
        api_token,
    )
    .await
    {
        Ok(_) => format!("⏸️ Paused run `{run_id}`."),
        Err(error) => format!("⚠️ Could not pause run `{run_id}`: {error}"),
    }
}

async fn run_resume_text(run_id: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::POST,
        &format!(
            "/automations/v2/runs/{}/resume",
            sanitize_resource_segment(&run_id)
        ),
        Some(serde_json::json!({})),
        base_url,
        api_token,
    )
    .await
    {
        Ok(_) => format!("▶️ Resumed run `{run_id}`."),
        Err(error) => format!("⚠️ Could not resume run `{run_id}`: {error}"),
    }
}

async fn run_cancel_text(run_id: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::POST,
        &format!(
            "/automations/v2/runs/{}/cancel",
            sanitize_resource_segment(&run_id)
        ),
        Some(serde_json::json!({})),
        base_url,
        api_token,
    )
    .await
    {
        Ok(_) => format!("🛑 Cancelled run `{run_id}`."),
        Err(error) => format!("⚠️ Could not cancel run `{run_id}`: {error}"),
    }
}

async fn run_artifacts_text(run_id: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::GET,
        &format!(
            "/automations/runs/{}/artifacts",
            sanitize_resource_segment(&run_id)
        ),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let artifacts = json
                .get("artifacts")
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default();
            if artifacts.is_empty() {
                return format!("ℹ️ Run `{run_id}` has no artifacts.");
            }
            let lines = artifacts
                .iter()
                .take(12)
                .map(|artifact| {
                    let kind = value_string(artifact, &["kind"]).unwrap_or("artifact");
                    let uri = value_string(artifact, &["uri"]).unwrap_or("-");
                    format!("• {} — {}", kind, truncate_for_channel(uri, 90))
                })
                .collect::<Vec<_>>();
            format!("📎 Artifacts for `{run_id}`:\n{}", lines.join("\n"))
        }
        Err(error) => format!("⚠️ Could not list artifacts for `{run_id}`: {error}"),
    }
}

async fn memory_search_text(
    query: String,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
    security_profile: ChannelSecurityProfile,
) -> String {
    if security_profile == ChannelSecurityProfile::PublicDemo {
        let mut args = public_channel_memory_tool_args(msg, session_map).await;
        args["query"] = serde_json::json!(query);
        args["limit"] = serde_json::json!(5);
        args["tier"] = serde_json::json!("project");
        return match tool_execute("memory_search", args, base_url, api_token).await {
            Ok(json) => {
                let results = parse_tool_output_rows(&json);
                if results.is_empty() {
                    return "ℹ️ No matching memory entries found.".to_string();
                }
                let lines = results
                    .iter()
                    .take(5)
                    .map(|item| {
                        let id = value_string(item, &["id", "chunk_id"]).unwrap_or("unknown");
                        let content = value_string(item, &["content", "text"]).unwrap_or("");
                        format!(
                            "• `{}` {}",
                            short_id(id),
                            truncate_for_channel(content, 120)
                        )
                    })
                    .collect::<Vec<_>>();
                format!("🧠 Memory search results:\n{}", lines.join("\n"))
            }
            Err(error) => format!("⚠️ Could not search memory: {error}"),
        };
    }

    match json_request(
        reqwest::Method::POST,
        "/memory/search",
        Some(serde_json::json!({ "query": query, "limit": 5 })),
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let results = json
                .get("results")
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default();
            if results.is_empty() {
                return "ℹ️ No matching memory entries found.".to_string();
            }
            let lines = results
                .iter()
                .take(5)
                .map(|item| {
                    let id = value_string(item, &["id", "chunk_id"]).unwrap_or("unknown");
                    let content = value_string(item, &["content", "text"]).unwrap_or("");
                    format!(
                        "• `{}` {}",
                        short_id(id),
                        truncate_for_channel(content, 120)
                    )
                })
                .collect::<Vec<_>>();
            format!("🧠 Memory search results:\n{}", lines.join("\n"))
        }
        Err(error) => format!("⚠️ Could not search memory: {error}"),
    }
}

async fn public_channel_memory_tool_args(
    msg: &ChannelMessage,
    session_map: &SessionMap,
) -> serde_json::Value {
    let mut args = serde_json::json!({
        "__project_id": public_channel_memory_scope_key(msg),
        "__memory_max_visible_scope": "project"
    });
    if let Some(session_id) = active_session_id(msg, session_map).await {
        args["__session_id"] = serde_json::json!(session_id);
    }
    args
}

