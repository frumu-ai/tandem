fn parse_tool_output_rows(json: &serde_json::Value) -> Vec<serde_json::Value> {
    serde_json::from_str::<serde_json::Value>(&extract_tool_output(json))
        .ok()
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
}

async fn memory_recent_text(
    limit: usize,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
    security_profile: ChannelSecurityProfile,
) -> String {
    if security_profile == ChannelSecurityProfile::PublicDemo {
        let mut args = public_channel_memory_tool_args(msg, session_map).await;
        args["limit"] = serde_json::json!(limit);
        args["tier"] = serde_json::json!("project");
        return match tool_execute("memory_list", args, base_url, api_token).await {
            Ok(json) => {
                let items = parse_tool_output_rows(&json);
                if items.is_empty() {
                    return "ℹ️ No memory entries found.".to_string();
                }
                let lines = items
                    .iter()
                    .take(limit)
                    .map(|item| {
                        let id = value_string(item, &["id", "chunk_id"]).unwrap_or("unknown");
                        let text = value_string(item, &["content", "text"]).unwrap_or("");
                        format!("• `{}` {}", short_id(id), truncate_for_channel(text, 120))
                    })
                    .collect::<Vec<_>>();
                format!("🧠 Recent memory:\n{}", lines.join("\n"))
            }
            Err(error) => format!("⚠️ Could not list memory: {error}"),
        };
    }

    match json_request(
        reqwest::Method::GET,
        &format!("/memory?limit={limit}"),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let items = json
                .get("items")
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default();
            if items.is_empty() {
                return "ℹ️ No memory entries found.".to_string();
            }
            let lines = items
                .iter()
                .take(limit)
                .map(|item| {
                    let id = value_string(item, &["id", "chunk_id"]).unwrap_or("unknown");
                    let text = value_string(item, &["content", "text"]).unwrap_or("");
                    format!("• `{}` {}", short_id(id), truncate_for_channel(text, 120))
                })
                .collect::<Vec<_>>();
            format!("🧠 Recent memory:\n{}", lines.join("\n"))
        }
        Err(error) => format!("⚠️ Could not list memory: {error}"),
    }
}

async fn memory_save_text(
    text: String,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
    security_profile: ChannelSecurityProfile,
) -> String {
    if security_profile == ChannelSecurityProfile::PublicDemo {
        let mut args = public_channel_memory_tool_args(msg, session_map).await;
        let session_id = active_session_id(msg, session_map).await;
        args["content"] = serde_json::json!(text);
        args["tier"] = serde_json::json!("project");
        args["source"] = serde_json::json!("public_channel_memory");
        args["metadata"] = serde_json::json!({
            "security_profile": "public_demo",
            "channel": msg.channel,
            "scope_id": msg.scope.id,
            "scope_kind": format!("{:?}", msg.scope.kind).to_ascii_lowercase(),
            "sender": msg.sender,
            "active_session_id": session_id,
        });
        return match tool_execute("memory_store", args, base_url, api_token).await {
            Ok(json) => {
                let id = json
                    .get("metadata")
                    .and_then(|v| v.get("chunk_ids"))
                    .and_then(|v| v.as_array())
                    .and_then(|v| v.first())
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                format!("💾 Saved memory entry `{}`.", short_id(id))
            }
            Err(error) => format!("⚠️ Could not save memory: {error}"),
        };
    }

    match json_request(
        reqwest::Method::POST,
        "/memory/put",
        Some(serde_json::json!({ "text": text })),
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let id = value_string(&json, &["id", "chunk_id"]).unwrap_or("unknown");
            format!("💾 Saved memory entry `{}`.", short_id(id))
        }
        Err(error) => format!("⚠️ Could not save memory: {error}"),
    }
}

async fn memory_scopes_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
    security_profile: ChannelSecurityProfile,
) -> String {
    let sid = active_session_id(msg, session_map).await;
    if security_profile == ChannelSecurityProfile::PublicDemo {
        let project_id = public_channel_memory_scope_key(msg);
        return format!(
            "🧠 Memory scopes\nSession: {}\nPublic channel scope: {}\nWorkspace: disabled\nGlobal: disabled\n\nThis public memory is quarantined to the current channel scope.",
            sid.as_deref().unwrap_or("-"),
            project_id,
        );
    }
    let details = active_session_details(msg, base_url, api_token, session_map).await;
    let project_id = details
        .as_ref()
        .and_then(|value| value.get("project_id"))
        .and_then(|value| value.as_str())
        .unwrap_or("-");
    let workspace_root = details
        .as_ref()
        .and_then(|value| value.get("workspace_root"))
        .and_then(|value| value.as_str())
        .unwrap_or("-");
    format!(
        "🧠 Memory scopes\nSession: {}\nProject: {}\nWorkspace: {}\nGlobal: enabled via default memory search behavior",
        sid.as_deref().unwrap_or("-"),
        project_id,
        workspace_root
    )
}

async fn memory_delete_text(
    memory_id: String,
    confirmed: bool,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    security_profile: ChannelSecurityProfile,
) -> String {
    if !confirmed {
        return yes_required_text(
            "delete memory",
            &memory_id,
            &format!("/memory delete {memory_id}"),
        );
    }
    if security_profile == ChannelSecurityProfile::PublicDemo {
        let args = serde_json::json!({
            "chunk_id": memory_id,
            "tier": "project",
            "__project_id": public_channel_memory_scope_key(msg),
            "__memory_max_visible_scope": "project"
        });
        return match tool_execute("memory_delete", args, base_url, api_token).await {
            Ok(json) => {
                let deleted = json
                    .get("metadata")
                    .and_then(|v| v.get("deleted"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                if deleted {
                    format!("🗑️ Deleted memory `{memory_id}`.")
                } else {
                    let detail = extract_tool_output(&json);
                    format!("⚠️ Could not delete memory `{memory_id}`: {detail}")
                }
            }
            Err(error) => format!("⚠️ Could not delete memory `{memory_id}`: {error}"),
        };
    }

    match json_request(
        reqwest::Method::DELETE,
        &format!("/memory/{}", sanitize_resource_segment(&memory_id)),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(_) => format!("🗑️ Deleted memory `{memory_id}`."),
        Err(error) => format!("⚠️ Could not delete memory `{memory_id}`: {error}"),
    }
}

async fn workspace_show_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let Some(details) = active_session_details(msg, base_url, api_token, session_map).await else {
        return "ℹ️ No active session or workspace binding yet.".to_string();
    };
    let session_id = value_string(&details, &["id"]).unwrap_or("-");
    let title = value_string(&details, &["title"]).unwrap_or("Untitled");
    let project_id = value_string(&details, &["project_id"]).unwrap_or("-");
    let workspace_root = value_string(&details, &["workspace_root", "directory"]).unwrap_or("-");
    format!(
        "📁 Workspace binding\nSession: `{}`\nTitle: {}\nProject: {}\nWorkspace: {}",
        short_id(session_id),
        title,
        project_id,
        workspace_root
    )
}

async fn workspace_status_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let Some(details) = active_session_details(msg, base_url, api_token, session_map).await else {
        return "ℹ️ No active session or workspace binding yet.".to_string();
    };
    let message_count = details
        .get("messages")
        .and_then(|value| value.as_array())
        .map(|items| items.len())
        .unwrap_or(0);
    let project_id = value_string(&details, &["project_id"]).unwrap_or("-");
    let workspace_root = value_string(&details, &["workspace_root", "directory"]).unwrap_or("-");
    format!(
        "📁 Workspace status\nMessages: {}\nProject: {}\nWorkspace: {}",
        message_count, project_id, workspace_root
    )
}

async fn workspace_files_text(
    query: String,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let Some(details) = active_session_details(msg, base_url, api_token, session_map).await else {
        return "ℹ️ No active session or workspace binding yet.".to_string();
    };
    let Some(workspace_root) = value_string(&details, &["workspace_root", "directory"]) else {
        return "ℹ️ No workspace root is bound to this session.".to_string();
    };
    let pattern = format!("**/*{query}*");
    match tool_execute(
        "glob",
        serde_json::json!({
            "pattern": pattern,
            "__workspace_root": workspace_root,
            "__effective_cwd": workspace_root,
        }),
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let output = extract_tool_output(&json);
            if output.is_empty() {
                return format!("ℹ️ No files matching `{query}`.");
            }
            let lines = output.lines().take(12).collect::<Vec<_>>();
            format!("📁 Files matching `{query}`:\n{}", lines.join("\n"))
        }
        Err(error) => format!("⚠️ Could not search workspace files: {error}"),
    }
}

async fn workspace_branch_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let Some(details) = active_session_details(msg, base_url, api_token, session_map).await else {
        return "ℹ️ No active session or workspace binding yet.".to_string();
    };
    let Some(workspace_root) = value_string(&details, &["workspace_root", "directory"]) else {
        return "ℹ️ No workspace root is bound to this session.".to_string();
    };
    match tool_execute(
        "bash",
        serde_json::json!({
            "command": "git rev-parse --abbrev-ref HEAD",
            "__workspace_root": workspace_root,
            "__effective_cwd": workspace_root,
            "timeout_ms": 5000,
        }),
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let branch = extract_tool_output(&json);
            if branch.is_empty() {
                "ℹ️ No git branch information found.".to_string()
            } else {
                format!("🌿 Current branch: `{}`", branch.trim())
            }
        }
        Err(error) => format!("⚠️ Could not read workspace branch: {error}"),
    }
}

async fn tools_command_text(
    action: ToolsCommand,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
) -> String {
    match action {
        ToolsCommand::Help => tools_help_text(ChannelSecurityProfile::Operator),
        ToolsCommand::List => {
            let prefs = load_channel_tool_preferences(&msg.channel, &msg.scope.id).await;
            let enabled: std::collections::HashSet<String> =
                prefs.enabled_tools.iter().cloned().collect();
            let disabled: std::collections::HashSet<String> =
                prefs.disabled_tools.iter().cloned().collect();

            let all_tools = [
                "read",
                "glob",
                "ls",
                "list",
                "grep",
                "codesearch",
                "search",
                "websearch",
                "webfetch",
                "webfetch_html",
                "bash",
                "write",
                "edit",
                "apply_patch",
                "todowrite",
                "memory_search",
                "memory_store",
                "memory_list",
                "skill",
                "task",
                "question",
                "pack_builder",
            ];

            let mut default_lines: Vec<String> = Vec::new();
            let mut disabled_lines: Vec<String> = Vec::new();

            for tool in all_tools {
                if disabled.contains(tool) {
                    disabled_lines.push(tool.to_string());
                } else if !prefs.enabled_tools.is_empty() && !enabled.contains(tool) {
                    disabled_lines.push(tool.to_string());
                } else {
                    default_lines.push(tool.to_string());
                }
            }

            let mut lines = Vec::new();
            if !default_lines.is_empty() {
                lines.push(format!("*Enabled:* {}", default_lines.join(", ")));
            }
            if !disabled_lines.is_empty() {
                lines.push(format!("*Disabled:* {}", disabled_lines.join(", ")));
            }

            let mcp_servers = mcp_servers_for_channel(base_url, api_token).await;
            if !mcp_servers.is_empty() {
                let enabled_mcp: std::collections::HashSet<String> =
                    prefs.enabled_mcp_servers.iter().cloned().collect();
                let mut mcp_lines = Vec::new();
                for server in &mcp_servers {
                    if !prefs.enabled_mcp_servers.is_empty() && !enabled_mcp.contains(server) {
                        mcp_lines.push(format!("{} (disabled)", server));
                    } else {
                        mcp_lines.push(format!("{} (enabled)", server));
                    }
                }
                lines.push(format!("\n*MCP servers:*\n{}", mcp_lines.join(", ")));
            }

            if lines.is_empty() {
                "ℹ️ No tool preferences set. All built-in tools are available by default."
                    .to_string()
            } else {
                format!("🛠 *Tool Scope for this channel*\n\n{}", lines.join("\n\n"))
            }
        }
        ToolsCommand::Enable { tools } => {
            let mut prefs = load_channel_tool_preferences(&msg.channel, &msg.scope.id).await;
            let mut added = Vec::new();
            for tool in &tools {
                if !prefs.enabled_tools.contains(tool) {
                    prefs.enabled_tools.push(tool.clone());
                    added.push(tool.clone());
                }
                prefs.disabled_tools.retain(|t| t != tool);
            }
            if added.is_empty() {
                "ℹ️ No new tools were enabled.".to_string()
            } else {
                save_channel_tool_preferences(&msg.channel, &msg.scope.id, prefs).await;
                format!("✅ Enabled for this channel: {}", added.join(", "))
            }
        }
        ToolsCommand::Disable { tools } => {
            let mut prefs = load_channel_tool_preferences(&msg.channel, &msg.scope.id).await;
            let mut added = Vec::new();
            for tool in &tools {
                if !prefs.disabled_tools.contains(tool) {
                    prefs.disabled_tools.push(tool.clone());
                    added.push(tool.clone());
                }
                prefs.enabled_tools.retain(|t| t != tool);
            }
            if added.is_empty() {
                "ℹ️ No new tools were disabled.".to_string()
            } else {
                save_channel_tool_preferences(&msg.channel, &msg.scope.id, prefs).await;
                format!("🚫 Disabled for this channel: {}", added.join(", "))
            }
        }
        ToolsCommand::Reset => {
            let prefs = ChannelToolPreferences::default();
            save_channel_tool_preferences(&msg.channel, &msg.scope.id, prefs).await;
            "🔄 Tool preferences reset. All built-in tools are now available by default."
                .to_string()
        }
    }
}

async fn mcp_servers_for_channel(base_url: &str, api_token: &str) -> Vec<String> {
    match json_request(reqwest::Method::GET, "/mcp", None, base_url, api_token).await {
        Ok(json) => {
            let obj = json.as_object();
            obj.map(|m| m.keys().cloned().collect()).unwrap_or_default()
        }
        Err(_) => Vec::new(),
    }
}

async fn mcp_list_text(base_url: &str, api_token: &str) -> String {
    match json_request(reqwest::Method::GET, "/mcp", None, base_url, api_token).await {
        Ok(json) => {
            let Some(obj) = json.as_object() else {
                return "ℹ️ No MCP servers configured.".to_string();
            };
            if obj.is_empty() {
                return "ℹ️ No MCP servers configured.".to_string();
            }
            let mut lines = obj
                .iter()
                .take(20)
                .map(|(name, value)| {
                    let enabled = value
                        .get("enabled")
                        .and_then(|entry| entry.as_bool())
                        .unwrap_or(true);
                    format!(
                        "• {} ({})",
                        name,
                        if enabled { "enabled" } else { "disabled" }
                    )
                })
                .collect::<Vec<_>>();
            lines.sort();
            format!("🔌 MCP servers:\n{}", lines.join("\n"))
        }
        Err(error) => format!("⚠️ Could not list MCP servers: {error}"),
    }
}

async fn mcp_tools_text(server: Option<String>, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::GET,
        "/mcp/tools",
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let tools = json.as_array().cloned().unwrap_or_default();
            if tools.is_empty() {
                return "ℹ️ No MCP tools discovered.".to_string();
            }
            let filtered = tools
                .iter()
                .filter(|tool| {
                    if let Some(server_name) = server.as_ref() {
                        value_string(tool, &["server", "server_name", "mcp_server"])
                            .map(|name| name == server_name)
                            .unwrap_or(false)
                    } else {
                        true
                    }
                })
                .take(20)
                .map(|tool| {
                    let name = value_string(tool, &["name", "tool", "tool_name"]).unwrap_or("tool");
                    let srv =
                        value_string(tool, &["server", "server_name", "mcp_server"]).unwrap_or("?");
                    format!("• {} ({})", name, srv)
                })
                .collect::<Vec<_>>();
            if filtered.is_empty() {
                return format!(
                    "ℹ️ No MCP tools found{}.",
                    server
                        .as_ref()
                        .map(|name| format!(" for `{name}`"))
                        .unwrap_or_default()
                );
            }
            format!("🔧 MCP tools:\n{}", filtered.join("\n"))
        }
        Err(error) => format!("⚠️ Could not list MCP tools: {error}"),
    }
}

async fn mcp_resources_text(base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::GET,
        "/mcp/resources",
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let resources = json.as_array().cloned().unwrap_or_default();
            if resources.is_empty() {
                return "ℹ️ No MCP resources discovered.".to_string();
            }
            let lines = resources
                .iter()
                .take(12)
                .map(|value| truncate_for_channel(&compact_json(value), 120))
                .collect::<Vec<_>>();
            format!("📚 MCP resources:\n{}", lines.join("\n"))
        }
        Err(error) => format!("⚠️ Could not list MCP resources: {error}"),
    }
}

async fn mcp_status_text(base_url: &str, api_token: &str) -> String {
    mcp_list_text(base_url, api_token).await
}

async fn mcp_connect_text(name: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::POST,
        &format!("/mcp/{}/connect", sanitize_resource_segment(&name)),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(_) => format!("🔌 Connected MCP server `{name}`."),
        Err(error) => format!("⚠️ Could not connect `{name}`: {error}"),
    }
}

async fn mcp_disconnect_text(name: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::POST,
        &format!("/mcp/{}/disconnect", sanitize_resource_segment(&name)),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(_) => format!("🔌 Disconnected MCP server `{name}`."),
        Err(error) => format!("⚠️ Could not disconnect `{name}`: {error}"),
    }
}

async fn mcp_refresh_text(name: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::POST,
        &format!("/mcp/{}/refresh", sanitize_resource_segment(&name)),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let count = value_u64(&json, &["count"]).unwrap_or(0);
            format!("🔄 Refreshed MCP server `{name}` ({} tool(s)).", count)
        }
        Err(error) => format!("⚠️ Could not refresh `{name}`: {error}"),
    }
}

async fn packs_list_text(base_url: &str, api_token: &str) -> String {
    match json_request(reqwest::Method::GET, "/packs", None, base_url, api_token).await {
        Ok(json) => {
            let packs = json
                .get("packs")
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default();
            if packs.is_empty() {
                return "ℹ️ No packs installed.".to_string();
            }
            let lines = packs
                .iter()
                .take(12)
                .map(|pack| {
                    let name = value_string(pack, &["name"]).unwrap_or("pack");
                    let version = value_string(pack, &["version"]).unwrap_or("?");
                    format!("• {} ({})", name, version)
                })
                .collect::<Vec<_>>();
            format!("📦 Installed packs:\n{}", lines.join("\n"))
        }
        Err(error) => format!("⚠️ Could not list packs: {error}"),
    }
}

async fn packs_show_text(selector: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::GET,
        &format!("/packs/{}", sanitize_resource_segment(&selector)),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let installed = json
                .get("pack")
                .and_then(|value| value.get("installed"))
                .unwrap_or(&json);
            let name = value_string(installed, &["name"]).unwrap_or("pack");
            let version = value_string(installed, &["version"]).unwrap_or("?");
            let pack_id = value_string(installed, &["pack_id", "packId"]).unwrap_or("-");
            format!(
                "📦 Pack `{}`\nName: {}\nVersion: {}\nPack ID: {}",
                selector, name, version, pack_id
            )
        }
        Err(error) => format!("⚠️ Could not inspect pack `{selector}`: {error}"),
    }
}

async fn packs_updates_text(selector: String, base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::GET,
        &format!("/packs/{}/updates", sanitize_resource_segment(&selector)),
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let updates = json
                .get("updates")
                .and_then(|value| value.as_array())
                .cloned()
                .unwrap_or_default();
            if updates.is_empty() {
                return format!("ℹ️ No updates available for `{selector}`.");
            }
            let lines = updates
                .iter()
                .take(10)
                .map(|item| truncate_for_channel(&compact_json(item), 120))
                .collect::<Vec<_>>();
            format!("📦 Updates for `{selector}`:\n{}", lines.join("\n"))
        }
        Err(error) => format!("⚠️ Could not check updates for `{selector}`: {error}"),
    }
}

async fn packs_install_text(target: String, base_url: &str, api_token: &str) -> String {
    let body = if target.starts_with("http://") || target.starts_with("https://") {
        serde_json::json!({ "url": target })
    } else {
        serde_json::json!({ "path": target })
    };
    match json_request(
        reqwest::Method::POST,
        "/packs/install",
        Some(body),
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let installed = json.get("installed").unwrap_or(&json);
            let name = value_string(installed, &["name"]).unwrap_or("pack");
            let version = value_string(installed, &["version"]).unwrap_or("?");
            format!("📦 Installed pack {} ({}).", name, version)
        }
        Err(error) => format!("⚠️ Could not install pack: {error}"),
    }
}

async fn packs_uninstall_text(
    selector: String,
    confirmed: bool,
    base_url: &str,
    api_token: &str,
) -> String {
    if !confirmed {
        return yes_required_text(
            "uninstall pack",
            &selector,
            &format!("/packs uninstall {selector}"),
        );
    }
    match json_request(
        reqwest::Method::POST,
        "/packs/uninstall",
        Some(serde_json::json!({ "name": selector })),
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let removed = json.get("removed").unwrap_or(&json);
            let name = value_string(removed, &["name"]).unwrap_or("pack");
            format!("🗑️ Uninstalled pack {}.", name)
        }
        Err(error) => format!("⚠️ Could not uninstall pack: {error}"),
    }
}

async fn config_show_text(base_url: &str, api_token: &str) -> String {
    match json_request(reqwest::Method::GET, "/config", None, base_url, api_token).await {
        Ok(json) => {
            let default_provider = json
                .get("providers")
                .and_then(|value| value.get("default"))
                .and_then(|value| value.as_str())
                .or_else(|| json.get("default").and_then(|value| value.as_str()))
                .unwrap_or("-");
            let provider_count =
                json.get("providers")
                    .and_then(|value| value.get("providers").or_else(|| value.get("all")))
                    .map(|value| {
                        value.as_object().map(|obj| obj.len()).unwrap_or_else(|| {
                            value.as_array().map(|items| items.len()).unwrap_or(0)
                        })
                    })
                    .unwrap_or(0);
            format!(
                "🛠️ Config summary\nDefault provider: {}\nConfigured providers: {}\nUse `/config providers`, `/config channels`, or `/config model` for details.",
                default_provider, provider_count
            )
        }
        Err(error) => format!("⚠️ Could not load config: {error}"),
    }
}

async fn config_channels_text(base_url: &str, api_token: &str) -> String {
    match json_request(
        reqwest::Method::GET,
        "/channels/status",
        None,
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => format!(
            "📡 Channel status\n{}",
            truncate_for_channel(&compact_json(&json), 500)
        ),
        Err(error) => format!("⚠️ Could not load channel status: {error}"),
    }
}

async fn config_model_text(base_url: &str, api_token: &str) -> String {
    let client = reqwest::Client::new();
    match fetch_default_model_spec(&client, base_url, api_token).await {
        Ok(Some(spec)) => {
            let provider = value_string(&spec, &["provider_id"]).unwrap_or("-");
            let model = value_string(&spec, &["model_id"]).unwrap_or("-");
            format!("🧠 Default model\nProvider: {}\nModel: {}", provider, model)
        }
        Ok(None) => "ℹ️ No default model is configured.".to_string(),
        Err(error) => format!("⚠️ Could not load default model: {error}"),
    }
}

async fn workflow_plan_get_request(
    plan_id: &str,
    base_url: &str,
    api_token: &str,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("could not build HTTP client: {error}"))?;
    let resp = add_auth(
        client.get(format!(
            "{base_url}/workflow-plans/{}",
            sanitize_resource_segment(plan_id)
        )),
        api_token,
    )
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

async fn schedule_plan_text(
    prompt: String,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let mut body = serde_json::json!({
        "prompt": prompt,
        "plan_source": "channel_slash_command",
    });
    if let Some(workspace_root) =
        workflow_planner_workspace_root(msg, base_url, api_token, session_map).await
    {
        body["workspace_root"] = serde_json::json!(workspace_root);
    }
    match workflow_plan_post("/workflow-plans/chat/start", body, base_url, api_token).await {
        Ok(json) => {
            let Some(plan) = json.get("plan") else {
                return "⚠️ Planner returned no plan.".to_string();
            };
            let mut sections = vec![format!(
                "🗓️ Workflow draft created.\n{}",
                workflow_plan_summary(plan)
            )];
            if let Some(text) = assistant_message_text(&json) {
                sections.push(format!("Planner notes:\n{text}"));
            }
            sections.push(
                "Next steps:\nUse `/schedule edit <plan_id> <message>` to refine it or `/schedule apply <plan_id>` to save it."
                    .to_string(),
            );
            sections.join("\n\n")
        }
        Err(error) => format!("⚠️ Could not create workflow draft: {error}"),
    }
}

async fn schedule_show_text(plan_id: String, base_url: &str, api_token: &str) -> String {
    match workflow_plan_get_request(&plan_id, base_url, api_token).await {
        Ok(json) => {
            let Some(plan) = json.get("plan") else {
                return "⚠️ Planner returned no plan.".to_string();
            };
            let conversation_count = json
                .get("conversation")
                .and_then(|value| value.get("messages"))
                .and_then(|value| value.as_array())
                .map(|items| items.len())
                .unwrap_or(0);
            format!(
                "🗓️ Current workflow draft\n{}\nConversation messages: {}",
                workflow_plan_summary(plan),
                conversation_count
            )
        }
        Err(error) => format!("⚠️ Could not load workflow draft `{plan_id}`: {error}"),
    }
}

async fn schedule_edit_text(
    plan_id: String,
    message: String,
    base_url: &str,
    api_token: &str,
) -> String {
    match workflow_plan_post(
        "/workflow-plans/chat/message",
        serde_json::json!({
            "plan_id": plan_id,
            "message": message,
        }),
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let Some(plan) = json.get("plan") else {
                return "⚠️ Planner returned no revised plan.".to_string();
            };
            let mut sections = vec![format!(
                "📝 Workflow draft updated.\n{}",
                workflow_plan_summary(plan)
            )];
            if let Some(change_summary) = workflow_plan_change_summary(&json) {
                sections.push(change_summary);
            }
            if let Some(text) = assistant_message_text(&json) {
                sections.push(format!("Planner notes:\n{text}"));
            }
            sections.join("\n\n")
        }
        Err(error) => format!("⚠️ Could not revise workflow draft: {error}"),
    }
}

async fn schedule_reset_text(plan_id: String, base_url: &str, api_token: &str) -> String {
    match workflow_plan_post(
        "/workflow-plans/chat/reset",
        serde_json::json!({ "plan_id": plan_id }),
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let Some(plan) = json.get("plan") else {
                return "⚠️ Planner returned no reset plan.".to_string();
            };
            format!(
                "↩️ Workflow draft reset to its initial version.\n{}",
                workflow_plan_summary(plan)
            )
        }
        Err(error) => format!("⚠️ Could not reset workflow draft: {error}"),
    }
}

async fn schedule_apply_text(plan_id: String, base_url: &str, api_token: &str) -> String {
    match workflow_plan_post(
        "/workflow-plans/apply",
        serde_json::json!({
            "plan_id": plan_id,
            "creator_id": "channel_slash_command",
        }),
        base_url,
        api_token,
    )
    .await
    {
        Ok(json) => {
            let automation_id = json
                .get("automation")
                .and_then(|value| value.get("id"))
                .and_then(|value| value.as_str())
                .unwrap_or("unknown");
            let automation_name = json
                .get("automation")
                .and_then(|value| value.get("name"))
                .and_then(|value| value.as_str())
                .unwrap_or("saved automation");
            format!(
                "✅ Workflow draft `{}` applied.\nCreated automation `{}` ({automation_name}).",
                plan_id, automation_id
            )
        }
        Err(error) => format!("⚠️ Could not apply workflow draft: {error}"),
    }
}

async fn active_session_id(msg: &ChannelMessage, session_map: &SessionMap) -> Option<String> {
    let map_key = session_map_key(msg);
    let legacy_key = legacy_session_map_key(msg);
    let mut guard = session_map.lock().await;
    if let Some(record) = guard.get(&map_key) {
        return Some(record.session_id.clone());
    }
    if let Some(mut record) = guard.remove(&legacy_key) {
        record.scope_id = Some(msg.scope.id.clone());
        record.scope_kind = Some(session_scope_kind_label(msg).to_string());
        let session_id = record.session_id.clone();
        guard.insert(map_key, record);
        persist_session_map(&guard).await;
        return Some(session_id);
    }
    None
}

async fn list_sessions_text(msg: &ChannelMessage, base_url: &str, api_token: &str) -> String {
    let client = reqwest::Client::new();
    let source_title_prefix = session_title_prefix(msg);

    let Ok(resp) = add_auth(client.get(format!("{base_url}/session")), api_token)
        .send()
        .await
    else {
        return "⚠️ Could not reach Tandem server.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected server response.".to_string();
    };

    let sessions = json.as_array().cloned().unwrap_or_default();
    // Filter to sessions whose title starts with the scoped channel prefix.
    let matching: Vec<_> = sessions
        .iter()
        .filter(|s| {
            s.get("title")
                .and_then(|t| t.as_str())
                .map(|t| t.starts_with(&source_title_prefix))
                .unwrap_or(false)
        })
        .take(5)
        .enumerate()
        .map(|(i, s)| {
            let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let title = s
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled");
            let msg_count = s
                .get("messages")
                .and_then(|m| m.as_array())
                .map(|a| a.len())
                .unwrap_or(0);
            format!(
                "{}. `{}` — {} ({} msgs)",
                i + 1,
                &id[..8.min(id.len())],
                title,
                msg_count
            )
        })
        .collect();

    if matching.is_empty() {
        "📋 No previous sessions found.".to_string()
    } else {
        format!("📋 Your sessions:\n{}", matching.join("\n"))
    }
}

async fn new_session_text(
    name: Option<String>,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
    security_profile: ChannelSecurityProfile,
) -> String {
    let map_key = session_map_key(msg);
    let display_name = name.clone().unwrap_or_else(|| session_title_prefix(msg));
    let client = reqwest::Client::new();
    let public_memory_project_id = if security_profile == ChannelSecurityProfile::PublicDemo {
        Some(public_channel_memory_scope_key(msg))
    } else {
        None
    };
    let body = build_channel_session_create_body(
        &display_name,
        security_profile,
        public_memory_project_id.as_deref(),
    );

    let Ok(resp) = add_auth(client.post(format!("{base_url}/session")), api_token)
        .json(&body)
        .send()
        .await
    else {
        return "⚠️ Could not create session.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected server response.".to_string();
    };

    let session_id = match json.get("id").and_then(|v| v.as_str()) {
        Some(id) => id.to_string(),
        None => return "⚠️ Server returned no session ID.".to_string(),
    };

    let mut guard = session_map.lock().await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    guard.insert(
        map_key,
        SessionRecord {
            session_id: session_id.clone(),
            created_at_ms: now,
            last_seen_at_ms: now,
            channel: msg.channel.clone(),
            sender: msg.sender.clone(),
            scope_id: Some(msg.scope.id.clone()),
            scope_kind: Some(session_scope_kind_label(msg).to_string()),
            tool_preferences: None,
        },
    );
    persist_session_map(&guard).await;

    format!(
        "✅ Started new session \"{}\" (`{}`)\nFresh context — what would you like to work on?",
        display_name,
        &session_id[..8.min(session_id.len())]
    )
}

async fn resume_session_text(
    query: String,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let map_key = session_map_key(msg);
    let source_prefix = session_title_prefix(msg);
    let client = reqwest::Client::new();

    let Ok(resp) = add_auth(client.get(format!("{base_url}/session")), api_token)
        .send()
        .await
    else {
        return "⚠️ Could not reach server.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected server response.".to_string();
    };

    let sessions = json.as_array().cloned().unwrap_or_default();
    let found = sessions.iter().find(|s| {
        // Only search sessions belonging to this sender
        let title_ok = s
            .get("title")
            .and_then(|t| t.as_str())
            .map(|t| t.starts_with(&source_prefix))
            .unwrap_or(false);
        if !title_ok {
            return false;
        }
        let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let title = s
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_lowercase();
        id.starts_with(&query) || title.contains(&query.to_lowercase())
    });

    match found {
        Some(s) => {
            let id = s.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            let title = s
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("Untitled");

            let mut guard = session_map.lock().await;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            guard.insert(
                map_key,
                SessionRecord {
                    session_id: id.to_string(),
                    created_at_ms: now,
                    last_seen_at_ms: now,
                    channel: msg.channel.clone(),
                    sender: msg.sender.clone(),
                    scope_id: Some(msg.scope.id.clone()),
                    scope_kind: Some(session_scope_kind_label(msg).to_string()),
                    tool_preferences: None,
                },
            );
            persist_session_map(&guard).await;

            format!(
                "✅ Resumed session \"{}\" (`{}`)\n→ Ready to continue.",
                title,
                &id[..8.min(id.len())]
            )
        }
        None => format!(
            "⚠️ No session matching \"{}\" found. Use /sessions to list yours.",
            query
        ),
    }
}

async fn status_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let session_id = active_session_id(msg, session_map).await;
    let Some(sid) = session_id else {
        return "ℹ️ No active session. Send a message to start one, or use /new.".to_string();
    };

    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(client.get(format!("{base_url}/session/{sid}")), api_token)
        .send()
        .await
    else {
        return format!("ℹ️ Session: `{}`", &sid[..8.min(sid.len())]);
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return format!("ℹ️ Session: `{}`", &sid[..8.min(sid.len())]);
    };

    let title = json
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or("Untitled");
    let msgs = json
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    format!(
        "ℹ️ Session: \"{}\" (`{}`) | {} messages",
        title,
        &sid[..8.min(sid.len())],
        msgs
    )
}

async fn rename_session_text(
    name: String,
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let session_id = active_session_id(msg, session_map).await;
    let Some(sid) = session_id else {
        return "⚠️ No active session to rename. Send a message first.".to_string();
    };

    let client = reqwest::Client::new();
    let resp = add_auth(client.patch(format!("{base_url}/session/{sid}")), api_token)
        .json(&serde_json::json!({ "title": name }))
        .send()
        .await;

    match resp {
        Ok(r) if r.status().is_success() => format!("✅ Session renamed to \"{name}\"."),
        Ok(r) => format!("⚠️ Rename failed (HTTP {}).", r.status()),
        Err(e) => format!("⚠️ Rename failed: {e}"),
    }
}

async fn run_status_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let Some(sid) = active_session_id(msg, session_map).await else {
        return "ℹ️ No active session. Send a message to start one, or use /new.".to_string();
    };

    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(
        client.get(format!("{base_url}/session/{sid}/run")),
        api_token,
    )
    .send()
    .await
    else {
        return "⚠️ Could not fetch run status.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected run status response.".to_string();
    };
    let active = json
        .get("active")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    if active.is_null() {
        return "ℹ️ No active run.".to_string();
    }

    let run_id = active
        .get("run_id")
        .or_else(|| active.get("runID"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    format!(
        "🏃 Active run: `{}` on session `{}`",
        &run_id[..8.min(run_id.len())],
        &sid[..8.min(sid.len())]
    )
}

async fn cancel_run_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let Some(sid) = active_session_id(msg, session_map).await else {
        return "⚠️ No active session — nothing to cancel.".to_string();
    };
    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(
        client.post(format!("{base_url}/session/{sid}/cancel")),
        api_token,
    )
    .send()
    .await
    else {
        return "⚠️ Could not reach server to cancel.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Cancel requested, but response could not be parsed.".to_string();
    };
    let cancelled = json
        .get("cancelled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if cancelled {
        "🛑 Cancelled active run.".to_string()
    } else {
        "ℹ️ No active run to cancel.".to_string()
    }
}

async fn todos_text(
    msg: &ChannelMessage,
    base_url: &str,
    api_token: &str,
    session_map: &SessionMap,
) -> String {
    let Some(sid) = active_session_id(msg, session_map).await else {
        return "ℹ️ No active session. Send a message to start one, or use /new.".to_string();
    };
    let client = reqwest::Client::new();
    let Ok(resp) = add_auth(
        client.get(format!("{base_url}/session/{sid}/todo")),
        api_token,
    )
    .send()
    .await
    else {
        return "⚠️ Could not fetch todos.".to_string();
    };
    let Ok(json) = resp.json::<serde_json::Value>().await else {
        return "⚠️ Unexpected todos response.".to_string();
    };

    let Some(items) = json.as_array() else {
        return "⚠️ Todos response was not a list.".to_string();
    };
    if items.is_empty() {
        return "✅ No todos in this session.".to_string();
    }

    let lines = items
        .iter()
        .take(12)
        .enumerate()
        .map(|(i, item)| {
            let content = item
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("(untitled)");
            let status = item
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("pending");
            let icon = if status.eq_ignore_ascii_case("completed") {
                "✅"
            } else if status.eq_ignore_ascii_case("in_progress") {
                "⏳"
            } else {
                "⬜"
            };
            format!("{}. {} {} ({})", i + 1, icon, content, status)
        })
        .collect::<Vec<_>>();
    format!("🧾 Session todos:\n{}", lines.join("\n"))
}

