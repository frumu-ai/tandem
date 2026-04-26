// ============================================================================
// OpenCode: Plugins + MCP Config Commands
// ============================================================================

/// List configured OpenCode plugins for the given scope.
#[tauri::command]
pub fn opencode_list_plugins(
    state: State<'_, AppState>,
    scope: crate::tandem_config::TandemConfigScope,
) -> Result<Vec<String>> {
    let workspace = state.get_workspace_path();
    let ws = workspace.as_deref();
    let path = crate::tandem_config::get_config_path(scope, ws)?;

    let cfg = crate::tandem_config::read_config(&path)?;
    let plugins = cfg
        .get("plugin")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_else(Vec::new);

    Ok(plugins)
}

/// Add a plugin to OpenCode config for the given scope (idempotent).
#[tauri::command]
pub fn opencode_add_plugin(
    state: State<'_, AppState>,
    scope: crate::tandem_config::TandemConfigScope,
    name: String,
) -> Result<Vec<String>> {
    let workspace = state.get_workspace_path();
    let ws = workspace.as_deref();

    let updated = crate::tandem_config::update_config(scope, ws, |cfg| {
        crate::tandem_config::ensure_schema(cfg);

        let root = cfg.as_object_mut().ok_or_else(|| {
            TandemError::InvalidConfig("OpenCode config must be an object".into())
        })?;

        let entry = root
            .entry("plugin".to_string())
            .or_insert_with(|| serde_json::Value::Array(Vec::new()));

        // Normalize non-array values.
        if !entry.is_array() {
            *entry = serde_json::Value::Array(Vec::new());
        }

        let arr = entry.as_array_mut().unwrap();
        let already = arr.iter().any(|v| v.as_str() == Some(name.as_str()));
        if !already {
            arr.push(serde_json::Value::String(name.clone()));
        }
        Ok(())
    })?;

    Ok(updated
        .get("plugin")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_else(Vec::new))
}

/// Remove a plugin from OpenCode config for the given scope.
#[tauri::command]
pub fn opencode_remove_plugin(
    state: State<'_, AppState>,
    scope: crate::tandem_config::TandemConfigScope,
    name: String,
) -> Result<Vec<String>> {
    let workspace = state.get_workspace_path();
    let ws = workspace.as_deref();

    let updated = crate::tandem_config::update_config(scope, ws, |cfg| {
        let root = cfg.as_object_mut().ok_or_else(|| {
            TandemError::InvalidConfig("OpenCode config must be an object".into())
        })?;
        if let Some(v) = root.get_mut("plugin") {
            if let Some(arr) = v.as_array_mut() {
                arr.retain(|p| p.as_str() != Some(name.as_str()));
            }
        }
        Ok(())
    })?;

    Ok(updated
        .get("plugin")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_else(Vec::new))
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OpencodeMcpServerEntry {
    pub name: String,
    pub config: serde_json::Value,
}

/// List configured MCP servers for the given scope.
#[tauri::command]
pub fn opencode_list_mcp_servers(
    state: State<'_, AppState>,
    scope: crate::tandem_config::TandemConfigScope,
) -> Result<Vec<OpencodeMcpServerEntry>> {
    let workspace = state.get_workspace_path();
    let ws = workspace.as_deref();
    let path = crate::tandem_config::get_config_path(scope, ws)?;

    let cfg = crate::tandem_config::read_config(&path)?;
    let mut out: Vec<OpencodeMcpServerEntry> = Vec::new();

    if let Some(mcp) = cfg.get("mcp").and_then(|v| v.as_object()) {
        for (name, config) in mcp {
            out.push(OpencodeMcpServerEntry {
                name: name.clone(),
                config: config.clone(),
            });
        }
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Add or update an MCP server config for the given scope.
#[tauri::command]
pub fn opencode_add_mcp_server(
    state: State<'_, AppState>,
    scope: crate::tandem_config::TandemConfigScope,
    name: String,
    config: serde_json::Value,
) -> Result<Vec<OpencodeMcpServerEntry>> {
    let workspace = state.get_workspace_path();
    let ws = workspace.as_deref();

    crate::tandem_config::update_config(scope, ws, |cfg| {
        crate::tandem_config::ensure_schema(cfg);

        let root = cfg.as_object_mut().ok_or_else(|| {
            TandemError::InvalidConfig("OpenCode config must be an object".into())
        })?;
        let mcp_val = root
            .entry("mcp".to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if !mcp_val.is_object() {
            *mcp_val = serde_json::Value::Object(serde_json::Map::new());
        }
        let mcp_obj = mcp_val.as_object_mut().unwrap();
        mcp_obj.insert(name.clone(), config.clone());
        Ok(())
    })?;

    opencode_list_mcp_servers(state, scope)
}

/// Remove an MCP server config for the given scope.
#[tauri::command]
pub fn opencode_remove_mcp_server(
    state: State<'_, AppState>,
    scope: crate::tandem_config::TandemConfigScope,
    name: String,
) -> Result<Vec<OpencodeMcpServerEntry>> {
    let workspace = state.get_workspace_path();
    let ws = workspace.as_deref();

    crate::tandem_config::update_config(scope, ws, |cfg| {
        let root = cfg.as_object_mut().ok_or_else(|| {
            TandemError::InvalidConfig("OpenCode config must be an object".into())
        })?;
        if let Some(mcp_val) = root.get_mut("mcp") {
            if let Some(mcp_obj) = mcp_val.as_object_mut() {
                mcp_obj.remove(&name);
            }
        }
        Ok(())
    })?;

    opencode_list_mcp_servers(state, scope)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OpencodeMcpTestResult {
    // "connected" | "auth_required" | "wrong_url" | "wrong_method" | "gone" | "unreachable"
    // | "failed" | "invalid_response" | "not_supported" | "not_found"
    pub status: String,
    pub ok: bool,
    pub http_status: Option<u16>,
    pub error: Option<String>,
}

/// Best-effort connectivity probe for MCP servers (HTTP only).
#[tauri::command]
pub async fn opencode_test_mcp_connection(
    state: State<'_, AppState>,
    scope: crate::tandem_config::TandemConfigScope,
    name: String,
) -> Result<OpencodeMcpTestResult> {
    use futures::StreamExt;
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, CONTENT_TYPE};
    use std::time::Duration;

    let workspace = state.get_workspace_path();
    let ws = workspace.as_deref();
    let path = crate::tandem_config::get_config_path(scope, ws)?;
    let cfg = crate::tandem_config::read_config(&path)?;

    let server = match cfg
        .get("mcp")
        .and_then(|v| v.as_object())
        .and_then(|m| m.get(&name))
    {
        Some(v) => v,
        None => {
            return Ok(OpencodeMcpTestResult {
                status: "not_found".to_string(),
                ok: false,
                http_status: None,
                error: None,
            })
        }
    };

    let server_type = server
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    if server_type != "remote" {
        return Ok(OpencodeMcpTestResult {
            status: "not_supported".to_string(),
            ok: false,
            http_status: None,
            error: None,
        });
    }

    let url = server
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| TandemError::InvalidConfig("Remote MCP server missing 'url'".into()))?;

    // MCP protocol version used for the `initialize` handshake.
    // This should track the MCP spec date-version.
    const MCP_PROTOCOL_VERSION: &str = "2025-11-25";

    let debug_enabled = std::env::var("TANDEM_MCP_DEBUG")
        .ok()
        .is_some_and(|v| v != "0" && !v.is_empty());

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
        .map_err(|e| TandemError::Sidecar(format!("Failed to build HTTP client: {}", e)))?;

    // Build request headers (defaults + user-provided).
    let mut headers = HeaderMap::new();
    headers.insert(
        ACCEPT,
        HeaderValue::from_static("application/json, text/event-stream"),
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if let Some(arr) = server.get("headers").and_then(|v| v.as_array()) {
        for h in arr {
            let Some(line) = h.as_str() else { continue };
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            let name = name.trim();
            let value = value.trim();
            if name.is_empty() {
                continue;
            }
            let Ok(hn) = HeaderName::from_bytes(name.as_bytes()) else {
                continue;
            };
            let Ok(hv) = HeaderValue::from_str(value) else {
                continue;
            };
            headers.insert(hn, hv);
        }
    }

    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {
                "name": "tandem",
                "version": env!("CARGO_PKG_VERSION"),
            }
        }
    });
    let body_bytes = serde_json::to_vec(&body).map_err(TandemError::Serialization)?;

    if debug_enabled {
        let mut header_lines: Vec<String> = Vec::new();
        for (k, v) in headers.iter() {
            let key = k.as_str();
            let key_lc = key.to_ascii_lowercase();
            let val = if key_lc == "authorization"
                || key_lc == "proxy-authorization"
                || key_lc.contains("api-key")
                || key_lc.contains("apikey")
                || key_lc.contains("token")
            {
                "<redacted>".to_string()
            } else {
                v.to_str().unwrap_or("<binary>").to_string()
            };
            header_lines.push(format!("{}: {}", key, val));
        }
        let body_preview = String::from_utf8_lossy(&body_bytes)
            .chars()
            .take(2048)
            .collect::<String>();
        tracing::info!(
            "[mcp-test] POST {} headers=[{}] body={}",
            url,
            header_lines.join(", "),
            body_preview
        );
    }

    let resp = client
        .post(url)
        .headers(headers)
        .body(body_bytes)
        .send()
        .await;

    match resp {
        Ok(r) => {
            let http_status = r.status().as_u16();
            let resp_headers = r.headers().clone();
            let content_type = r
                .headers()
                .get(CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();

            if debug_enabled {
                let mut header_lines: Vec<String> = Vec::new();
                for (k, v) in resp_headers.iter() {
                    let key = k.as_str();
                    let key_lc = key.to_ascii_lowercase();
                    let val = if key_lc == "set-cookie"
                        || key_lc == "authorization"
                        || key_lc == "proxy-authorization"
                        || key_lc.contains("api-key")
                        || key_lc.contains("apikey")
                        || key_lc.contains("token")
                    {
                        "<redacted>".to_string()
                    } else {
                        v.to_str().unwrap_or("<binary>").to_string()
                    };
                    header_lines.push(format!("{}: {}", key, val));
                }
                tracing::info!("[mcp-test] response headers=[{}]", header_lines.join(", "));
            }

            // Best-effort read of SSE response bodies that may not terminate.
            async fn read_sse_first_event_data_json(
                resp: reqwest::Response,
                max_bytes: usize,
            ) -> std::result::Result<(String, serde_json::Value), String> {
                let mut buf: Vec<u8> = Vec::new();
                let mut stream = resp.bytes_stream();

                while let Some(next) = stream.next().await {
                    let chunk = next.map_err(|e| e.to_string())?;
                    buf.extend_from_slice(&chunk);
                    if buf.len() > max_bytes {
                        break;
                    }

                    // Find end of first SSE event.
                    let event_end = buf
                        .windows(4)
                        .position(|w| w == b"\r\n\r\n")
                        .map(|i| (i, 4))
                        .or_else(|| buf.windows(2).position(|w| w == b"\n\n").map(|i| (i, 2)));

                    let Some((idx, sep_len)) = event_end else {
                        continue;
                    };

                    let event_str = String::from_utf8_lossy(&buf[..idx]);
                    let mut data_lines: Vec<&str> = Vec::new();
                    for line in event_str.lines() {
                        let line = line.trim_end_matches('\r');
                        if let Some(rest) = line.strip_prefix("data:") {
                            data_lines.push(rest.trim_start());
                        }
                    }

                    if !data_lines.is_empty() {
                        let data = data_lines.join("\n");
                        let json: serde_json::Value =
                            serde_json::from_str(&data).map_err(|e| e.to_string())?;

                        let snippet = String::from_utf8_lossy(&buf[..idx + sep_len])
                            .chars()
                            .take(2048)
                            .collect::<String>();
                        return Ok((snippet, json));
                    }

                    // No data lines; continue reading (but avoid unbounded memory).
                    if buf.len() > max_bytes / 2 {
                        buf.drain(..idx + sep_len);
                    }
                }

                let snippet = String::from_utf8_lossy(&buf)
                    .chars()
                    .take(2048)
                    .collect::<String>();
                Err(format!(
                    "SSE response did not include a JSON data event within {} bytes. Snippet: {}",
                    max_bytes, snippet
                ))
            }

            // Read body (JSON or SSE) so we can provide actionable feedback.
            let (body_snippet, json, parse_err) =
                if http_status == 200 && content_type.starts_with("text/event-stream") {
                    match read_sse_first_event_data_json(r, 64 * 1024).await {
                        Ok((snippet, json)) => (Some(snippet), Some(json), None),
                        Err(e) => (None, None, Some(e)),
                    }
                } else {
                    // Small bodies are expected for `initialize`; safe to read fully.
                    let text = r.text().await.unwrap_or_default();
                    let snippet = text.chars().take(2048).collect::<String>();
                    let json = serde_json::from_str::<serde_json::Value>(&text).ok();
                    (Some(snippet), json, None)
                };

            if debug_enabled {
                let snippet = body_snippet.clone().unwrap_or_default();
                tracing::info!(
                    "[mcp-test] response status={} content-type={} snippet={}",
                    http_status,
                    content_type,
                    snippet
                );
            }

            // Status mapping (protocol-aware)
            match http_status {
                200 => {
                    let Some(v) = json else {
                        return Ok(OpencodeMcpTestResult {
                            status: "invalid_response".to_string(),
                            ok: false,
                            http_status: Some(http_status),
                            error: Some(
                                parse_err.unwrap_or_else(|| {
                                    "Server returned 200 but response was not valid JSON-RPC."
                                        .into()
                                }),
                            ),
                        });
                    };

                    let ok_jsonrpc = v
                        .get("jsonrpc")
                        .and_then(|x| x.as_str())
                        .is_some_and(|s| s == "2.0");

                    if ok_jsonrpc && v.get("result").is_some() {
                        Ok(OpencodeMcpTestResult {
                            status: "connected".to_string(),
                            ok: true,
                            http_status: Some(http_status),
                            error: None,
                        })
                    } else if ok_jsonrpc && v.get("error").is_some() {
                        let msg = v
                            .get("error")
                            .and_then(|e| e.get("message"))
                            .and_then(|m| m.as_str())
                            .unwrap_or("MCP server returned an error");
                        Ok(OpencodeMcpTestResult {
                            status: "failed".to_string(),
                            ok: false,
                            http_status: Some(http_status),
                            error: Some(msg.to_string()),
                        })
                    } else {
                        Ok(OpencodeMcpTestResult {
                            status: "invalid_response".to_string(),
                            ok: false,
                            http_status: Some(http_status),
                            error: Some(
                                "Server returned 200 but response did not look like JSON-RPC 2.0."
                                    .into(),
                            ),
                        })
                    }
                }
                401 | 403 => Ok(OpencodeMcpTestResult {
                    status: "auth_required".to_string(),
                    ok: false,
                    http_status: Some(http_status),
                    error: Some("Authentication required. Add an Authorization header or API key.".into()),
                }),
                404 => Ok(OpencodeMcpTestResult {
                    status: "wrong_url".to_string(),
                    ok: false,
                    http_status: Some(http_status),
                    error: Some("Endpoint not found (404). Check the URL/path.".into()),
                }),
                405 => Ok(OpencodeMcpTestResult {
                    status: "wrong_method".to_string(),
                    ok: false,
                    http_status: Some(http_status),
                    error: Some(
                        "Method not allowed (405). This endpoint may require a different MCP transport or path."
                            .into(),
                    ),
                }),
                406 => Ok(OpencodeMcpTestResult {
                    status: "wrong_method".to_string(),
                    ok: false,
                    http_status: Some(http_status),
                    error: Some(
                        "Not acceptable (406). Some MCP servers require Accept: application/json, text/event-stream."
                            .into(),
                    ),
                }),
                410 => {
                    let hint = if url.contains("/sse") {
                        "Endpoint is gone (410). DeepWiki deprecated /sse; use https://mcp.deepwiki.com/mcp instead."
                    } else {
                        "Endpoint is gone (410). The server may have deprecated this URL."
                    };
                    Ok(OpencodeMcpTestResult {
                        status: "gone".to_string(),
                        ok: false,
                        http_status: Some(http_status),
                        error: Some(hint.into()),
                    })
                }
                _ => Ok(OpencodeMcpTestResult {
                    status: "failed".to_string(),
                    ok: false,
                    http_status: Some(http_status),
                    error: body_snippet
                        .filter(|s| !s.is_empty())
                        .or_else(|| Some(format!("HTTP {}", http_status))),
                }),
            }
        }
        Err(e) => Ok(OpencodeMcpTestResult {
            status: "unreachable".to_string(),
            ok: false,
            http_status: None,
            error: Some(e.to_string()),
        }),
    }
}

// ============================================================================
// Plan Management Commands
// ============================================================================

/// Information about a plan file
#[derive(serde::Serialize, Clone)]
pub struct PlanInfo {
    /// Session name (parent directory name)
    pub session_name: String,
    /// File name (e.g., "PLAN_jwt_tokens.md")
    pub file_name: String,
    /// Full absolute path to the plan file
    pub full_path: String,
    /// Last modified timestamp (Unix timestamp in milliseconds)
    pub last_modified: u64,
}

/// List all plan files in the workspace, grouped by session
#[tauri::command]
pub fn list_plans(state: State<'_, AppState>) -> Result<Vec<PlanInfo>> {
    let workspace = state.workspace_path.read().unwrap();
    let workspace_path = workspace
        .as_ref()
        .ok_or_else(|| TandemError::InvalidConfig("No active workspace".to_string()))?;

    let (canonical_plans_dir, legacy_plans_dir) = workspace_plans_dirs(workspace_path);

    // Create canonical plans directory if it doesn't exist
    if !canonical_plans_dir.exists() {
        tracing::debug!(
            "[list_plans] Canonical plans directory doesn't exist, creating: {:?}",
            canonical_plans_dir
        );
        fs::create_dir_all(&canonical_plans_dir).map_err(TandemError::Io)?;
    }

    let mut plans = Vec::new();

    let mut scan_plan_root = |plans_root: &Path| -> Result<()> {
        if !plans_root.exists() {
            return Ok(());
        }
        for session_entry in fs::read_dir(plans_root).map_err(TandemError::Io)? {
            let session_entry = session_entry.map_err(TandemError::Io)?;
            let session_path = session_entry.path();

            if !session_path.is_dir() {
                continue;
            }

            let session_name = session_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            for plan_entry in fs::read_dir(&session_path).map_err(TandemError::Io)? {
                let plan_entry = plan_entry.map_err(TandemError::Io)?;
                let plan_path = plan_entry.path();

                if let Some(file_name) = plan_path.file_name().and_then(|n| n.to_str()) {
                    if file_name.ends_with(".md") && file_name.starts_with("PLAN_") {
                        let metadata = fs::metadata(&plan_path).map_err(TandemError::Io)?;
                        let last_modified = metadata
                            .modified()
                            .map_err(TandemError::Io)?
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;

                        plans.push(PlanInfo {
                            session_name: session_name.clone(),
                            file_name: file_name.to_string(),
                            full_path: plan_path.to_string_lossy().to_string(),
                            last_modified,
                        });
                    }
                }
            }
        }
        Ok(())
    };

    scan_plan_root(&canonical_plans_dir)?;
    scan_plan_root(&legacy_plans_dir)?;

    // Sort by last modified (newest first)
    plans.sort_by_key(|b| std::cmp::Reverse(b.last_modified));

    tracing::debug!("[list_plans] Found {} plans", plans.len());
    Ok(plans)
}

/// Read the content of a plan file
#[tauri::command]
pub fn read_plan_content(plan_path: String) -> Result<String> {
    let path = PathBuf::from(&plan_path);
    let normalized_path = normalize_path_for_match(&plan_path);

    // Security check: ensure the path is within .tandem/plans/ or legacy .opencode/plans/
    let is_allowed = path.components().any(|c| c.as_os_str() == ".tandem")
        || path.components().any(|c| c.as_os_str() == ".opencode");
    let in_plans = normalized_path.contains("/.tandem/plans/")
        || normalized_path.starts_with(".tandem/plans/")
        || normalized_path.contains("/.opencode/plans/")
        || normalized_path.starts_with(".opencode/plans/")
        || normalized_path.ends_with("/.tandem/plans")
        || normalized_path.ends_with("/.opencode/plans");
    if !is_allowed || !in_plans {
        return Err(TandemError::InvalidConfig(
            "Plan path must be within .tandem/plans/ or .opencode/plans/".to_string(),
        ));
    }

    fs::read_to_string(&path).map_err(TandemError::Io)
}

/// Result of starting a plan session
#[derive(serde::Serialize)]
pub struct PlanSessionResult {
    pub session: Session,
    pub plan_path: String,
}

/// Start a new planning session with a guaranteed pre-created plan file
#[tauri::command]
pub async fn start_plan_session(
    app: AppHandle,
    state: State<'_, AppState>,
    goal: Option<String>,
) -> Result<PlanSessionResult> {
    // 1. Generate Session ID and Plan Name
    let session_id = Uuid::new_v4().to_string();

    // If goal is provided, sanitize it for the filename. Otherwise use "draft".
    let plan_name = if let Some(g) = goal.as_ref() {
        let sanitized: String = g
            .chars()
            .map(|c| {
                if c.is_alphanumeric() {
                    c.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        format!("PLAN_{}", sanitized)
    } else {
        "PLAN_draft".to_string()
    };

    // 2. Prepare directory structure: .tandem/plans/{session_id}/
    // We use session_id for the folder to ensure uniqueness and "frictionless" start (no name collision)
    let workspace_path = state
        .get_workspace_path()
        .ok_or_else(|| TandemError::InvalidConfig("No workspace selected".to_string()))?;

    let plans_dir = PathBuf::from(&workspace_path)
        .join(".tandem")
        .join("plans")
        .join(&session_id);
    let plan_file_path = plans_dir.join(format!("{}.md", plan_name));

    // 3. Pre-create the file
    fs::create_dir_all(&plans_dir).map_err(TandemError::Io)?;

    let template = format!(
        "# Plan: {}\n\n## Goal\n{}\n\n## Proposed Changes\n- [ ] Analyze requirements\n- [ ] Design solution\n\n## Verification\n- [ ] Test case 1",
        goal.as_deref().unwrap_or("Draft Plan"),
        goal.as_deref().unwrap_or("Describe the goal here")
    );

    fs::write(&plan_file_path, template).map_err(TandemError::Io)?;

    let absolute_path = plan_file_path.to_string_lossy().to_string();
    tracing::info!("Pre-created plan file at: {}", absolute_path);

    // 4. Create Sidecar Session
    // We explicitly instruct the AI about the file we just made via the title or a follow-up message.
    // Ideally we would inject a system prompt, but we'll handle that by ensuring the frontend
    // or the "System" recognizes this session type.
    // For now, we create the session with a specific Title that hints at the plan.

    let config_snapshot = { state.providers_config.read().unwrap().clone() };
    let model_spec =
        resolve_required_model_spec(&config_snapshot, None, None, "Plan session creation")?;
    let mode_resolution = resolve_effective_mode(&app, &state, Some("plan"), None)?;
    if let Some(reason) = mode_resolution.fallback_reason.as_ref() {
        tracing::warn!("[start_plan_session] {}", reason);
    }
    validate_model_provider_auth_if_required(
        &app,
        &config_snapshot,
        Some(model_spec.model_id.as_str()),
        Some(model_spec.provider_id.as_str()),
    )
    .await?;

    let session = state
        .sidecar
        .create_session(CreateSessionRequest {
            parent_id: None,
            title: Some(goal.clone().unwrap_or_else(|| "Plan Mode".to_string())),
            model: build_sidecar_session_model(
                Some(model_spec.model_id.clone()),
                Some(model_spec.provider_id.clone()),
            ),
            provider: Some(model_spec.provider_id.clone()),
            permission: sidecar_permissions_for_mode(&mode_resolution.mode),
            directory: Some(workspace_path.to_string_lossy().to_string()),
            workspace_root: Some(workspace_path.to_string_lossy().to_string()),
            project_id: None,
        })
        .await?;
    set_session_mode(&state, &session.id, mode_resolution.mode);

    // 5. Inject System Directive (as a user message, since we can't set system role easily)
    // This ensures the AI context is primed with the file path immediately.
    let system_directive = format!(
        "SYSTEM NOTE: A dedicated plan file has been pre-created at:\n`{}`\n\nYour GOAL is: \"{}\".\n\nYour FIRST action MUST be to use the `write_file` tool to update this exact file. Do not create a new plan file. Edit this one directly.",
        absolute_path.replace("\\", "/"),
        goal.as_deref().unwrap_or("Draft a new plan")
    );

    // We fire-and-forget this message so the frontend doesn't hang waiting for a response (though it returns quickly)
    // Actually, we should wait to ensure it's in history before the user chats.
    let mut request = SendMessageRequest::text(system_directive);
    request.model = Some(ModelSpec {
        provider_id: model_spec.provider_id.clone(),
        model_id: model_spec.model_id.clone(),
    });

    // We ignore the result of the message send itself, as long as session exists
    if let Err(e) = state
        .sidecar
        .append_message_and_start_run(&session.id, request)
        .await
    {
        tracing::warn!("Failed to inject plan directive: {}", e);
    } else {
        tracing::info!("Injected plan directive into session {}", session.id);
    }

    Ok(PlanSessionResult {
        session,
        plan_path: absolute_path,
    })
}

// ============================================================================
