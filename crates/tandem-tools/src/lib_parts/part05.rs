fn extract_apply_patch_paths(patch: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in patch.lines() {
        let trimmed = line.trim();
        let marker = if let Some(value) = trimmed.strip_prefix("*** Add File: ") {
            Some(value)
        } else if let Some(value) = trimmed.strip_prefix("*** Update File: ") {
            Some(value)
        } else {
            trimmed.strip_prefix("*** Delete File: ")
        };
        let Some(path) = marker.map(str::trim).filter(|value| !value.is_empty()) else {
            continue;
        };
        if !paths.iter().any(|existing| existing == path) {
            paths.push(path.to_string());
        }
    }
    paths
}

async fn resolve_git_root_for_dir(dir: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .current_dir(dir)
        .arg("rev-parse")
        .arg("--show-toplevel")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if root.is_empty() {
        None
    } else {
        Some(PathBuf::from(root))
    }
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0)
}

struct BatchTool;
#[async_trait]
impl Tool for BatchTool {
    fn schema(&self) -> ToolSchema {
        tool_schema(
            "batch",
            "Execute multiple tool calls sequentially",
            json!({
                "type":"object",
                "properties":{
                    "tool_calls":{
                        "type":"array",
                        "items":{
                            "type":"object",
                            "properties":{
                                "tool":{"type":"string"},
                                "name":{"type":"string"},
                                "args":{"type":"object"}
                            }
                        }
                    }
                }
            }),
        )
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.execute_with_cancel(args, CancellationToken::new())
            .await
    }

    async fn execute_with_cancel(
        &self,
        args: Value,
        cancel: CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        self.execute_with_progress(args, cancel, None).await
    }

    async fn execute_with_progress(
        &self,
        args: Value,
        cancel: CancellationToken,
        progress: Option<SharedToolProgressSink>,
    ) -> anyhow::Result<ToolResult> {
        let calls = args["tool_calls"].as_array().cloned().unwrap_or_default();
        let registry = ToolRegistry::new();
        let mut outputs = Vec::new();
        for call in calls.iter().take(20) {
            if cancel.is_cancelled() {
                break;
            }
            let Some(tool) = resolve_batch_call_tool_name(call) else {
                continue;
            };
            if tool.is_empty() || tool == "batch" {
                continue;
            }
            let call_args = call.get("args").cloned().unwrap_or_else(|| json!({}));
            let mut result = registry
                .execute_with_cancel_and_progress(
                    &tool,
                    call_args.clone(),
                    cancel.clone(),
                    progress.clone(),
                )
                .await?;
            if result.output.starts_with("Unknown tool:") {
                if let Some(fallback_name) = call
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty() && *s != tool)
                {
                    result = registry
                        .execute_with_cancel_and_progress(
                            fallback_name,
                            call_args,
                            cancel.clone(),
                            progress.clone(),
                        )
                        .await?;
                }
            }
            outputs.push(json!({
                "tool": tool,
                "output": result.output,
                "metadata": result.metadata
            }));
        }
        let count = outputs.len();
        Ok(ToolResult {
            output: serde_json::to_string_pretty(&outputs).unwrap_or_default(),
            metadata: json!({"count": count}),
        })
    }
}

struct LspTool;
#[async_trait]
impl Tool for LspTool {
    fn schema(&self) -> ToolSchema {
        tool_schema(
            "lsp",
            "LSP-like workspace diagnostics and symbol operations",
            json!({"type":"object","properties":{"operation":{"type":"string"},"filePath":{"type":"string"},"symbol":{"type":"string"},"query":{"type":"string"}}}),
        )
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let operation = args["operation"].as_str().unwrap_or("symbols");
        let workspace_root =
            workspace_root_from_args(&args).unwrap_or_else(|| effective_cwd_from_args(&args));
        let output = match operation {
            "diagnostics" => {
                let path = args["filePath"].as_str().unwrap_or("");
                match resolve_tool_path(path, &args) {
                    Some(resolved_path) => {
                        diagnostics_for_path(&resolved_path.to_string_lossy()).await
                    }
                    None => "missing or unsafe filePath".to_string(),
                }
            }
            "definition" => {
                let symbol = args["symbol"].as_str().unwrap_or("");
                find_symbol_definition(symbol, &workspace_root).await
            }
            "references" => {
                let symbol = args["symbol"].as_str().unwrap_or("");
                find_symbol_references(symbol, &workspace_root).await
            }
            _ => {
                let query = args["query"]
                    .as_str()
                    .or_else(|| args["symbol"].as_str())
                    .unwrap_or("");
                list_symbols(query, &workspace_root).await
            }
        };
        Ok(ToolResult {
            output,
            metadata: json!({"operation": operation, "workspace_root": workspace_root.to_string_lossy()}),
        })
    }
}

#[allow(dead_code)]
fn _safe_path(path: &str) -> PathBuf {
    PathBuf::from(path)
}

static TODO_SEQ: AtomicU64 = AtomicU64::new(1);

fn normalize_todos(items: Vec<Value>) -> Vec<Value> {
    items
        .into_iter()
        .filter_map(|item| {
            let obj = item.as_object()?;
            let content = obj
                .get("content")
                .and_then(|v| v.as_str())
                .or_else(|| obj.get("text").and_then(|v| v.as_str()))
                .unwrap_or("")
                .trim()
                .to_string();
            if content.is_empty() {
                return None;
            }
            let id = obj
                .get("id")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| {
                    format!("todo-{}", TODO_SEQ.fetch_add(1, AtomicOrdering::Relaxed))
                });
            let status = obj
                .get("status")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| "pending".to_string());
            Some(json!({"id": id, "content": content, "status": status}))
        })
        .collect()
}

async fn diagnostics_for_path(path: &str) -> String {
    let Ok(content) = fs::read_to_string(path).await else {
        return "File not found".to_string();
    };
    let mut issues = Vec::new();
    let mut balance = 0i64;
    for (idx, line) in content.lines().enumerate() {
        for ch in line.chars() {
            if ch == '{' {
                balance += 1;
            } else if ch == '}' {
                balance -= 1;
            }
        }
        if line.contains("TODO") {
            issues.push(format!("{path}:{}: TODO marker", idx + 1));
        }
    }
    if balance != 0 {
        issues.push(format!("{path}:1: Unbalanced braces"));
    }
    if issues.is_empty() {
        "No diagnostics.".to_string()
    } else {
        issues.join("\n")
    }
}

async fn list_symbols(query: &str, root: &Path) -> String {
    let query = query.to_lowercase();
    let rust_fn = Regex::new(r"^\s*(pub\s+)?(async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)")
        .unwrap_or_else(|_| Regex::new("$^").expect("regex"));
    let mut out = Vec::new();
    for entry in WalkBuilder::new(root).build().flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let ext = path.extension().and_then(|v| v.to_str()).unwrap_or("");
        if !matches!(ext, "rs" | "ts" | "tsx" | "js" | "jsx" | "py") {
            continue;
        }
        if let Ok(content) = fs::read_to_string(path).await {
            for (idx, line) in content.lines().enumerate() {
                if let Some(captures) = rust_fn.captures(line) {
                    let name = captures
                        .get(3)
                        .map(|m| m.as_str().to_string())
                        .unwrap_or_default();
                    if query.is_empty() || name.to_lowercase().contains(&query) {
                        out.push(format!("{}:{}:fn {}", path.display(), idx + 1, name));
                        if out.len() >= 100 {
                            return out.join("\n");
                        }
                    }
                }
            }
        }
    }
    out.join("\n")
}

async fn find_symbol_definition(symbol: &str, root: &Path) -> String {
    if symbol.trim().is_empty() {
        return "missing symbol".to_string();
    }
    let listed = list_symbols(symbol, root).await;
    listed
        .lines()
        .find(|line| line.ends_with(&format!("fn {symbol}")))
        .map(ToString::to_string)
        .unwrap_or_else(|| "symbol not found".to_string())
}

