use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use ignore::WalkBuilder;
use regex::Regex;
use serde_json::{json, Value};
use tokio::fs;
use tokio::process::Command;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use tandem_types::{ToolResult, ToolSchema};

#[async_trait]
pub trait Tool: Send + Sync {
    fn schema(&self) -> ToolSchema;
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult>;
    async fn execute_with_cancel(
        &self,
        args: Value,
        _cancel: CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        self.execute(args).await
    }
}

#[derive(Clone)]
pub struct ToolRegistry {
    tools: Arc<RwLock<HashMap<String, Arc<dyn Tool>>>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        let mut map: HashMap<String, Arc<dyn Tool>> = HashMap::new();
        map.insert("bash".to_string(), Arc::new(BashTool));
        map.insert("read".to_string(), Arc::new(ReadTool));
        map.insert("write".to_string(), Arc::new(WriteTool));
        map.insert("edit".to_string(), Arc::new(EditTool));
        map.insert("glob".to_string(), Arc::new(GlobTool));
        map.insert("grep".to_string(), Arc::new(GrepTool));
        map.insert("webfetch".to_string(), Arc::new(WebFetchTool));
        map.insert("websearch".to_string(), Arc::new(WebSearchTool));
        map.insert("codesearch".to_string(), Arc::new(CodeSearchTool));
        map.insert("todo_write".to_string(), Arc::new(TodoWriteTool));
        map.insert("task".to_string(), Arc::new(TaskTool));
        map.insert("question".to_string(), Arc::new(QuestionTool));
        map.insert("skill".to_string(), Arc::new(SkillTool));
        map.insert("apply_patch".to_string(), Arc::new(ApplyPatchTool));
        map.insert("batch".to_string(), Arc::new(BatchTool));
        map.insert("lsp".to_string(), Arc::new(LspTool));
        Self {
            tools: Arc::new(RwLock::new(map)),
        }
    }

    pub async fn list(&self) -> Vec<ToolSchema> {
        self.tools
            .read()
            .await
            .values()
            .map(|t| t.schema())
            .collect()
    }

    pub async fn execute(&self, name: &str, args: Value) -> anyhow::Result<ToolResult> {
        let tools = self.tools.read().await;
        let Some(tool) = tools.get(name) else {
            return Ok(ToolResult {
                output: format!("Unknown tool: {name}"),
                metadata: json!({}),
            });
        };
        tool.execute(args).await
    }

    pub async fn execute_with_cancel(
        &self,
        name: &str,
        args: Value,
        cancel: CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        let tools = self.tools.read().await;
        let Some(tool) = tools.get(name) else {
            return Ok(ToolResult {
                output: format!("Unknown tool: {name}"),
                metadata: json!({}),
            });
        };
        tool.execute_with_cancel(args, cancel).await
    }
}

fn is_path_allowed(path: &str) -> bool {
    let raw = Path::new(path);
    if raw.is_absolute() {
        return false;
    }
    !raw.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
}

struct BashTool;
#[async_trait]
impl Tool for BashTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "bash".to_string(),
            description: "Run shell command".to_string(),
            input_schema: json!({"type":"object","properties":{"command":{"type":"string"}}}),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let cmd = args["command"].as_str().unwrap_or("");
        let mut command = Command::new("powershell");
        command.args(["-NoProfile", "-Command", cmd]);
        if let Some(env) = args.get("env").and_then(|v| v.as_object()) {
            for (k, v) in env {
                if let Some(value) = v.as_str() {
                    command.env(k, value);
                }
            }
        }
        let output = command.output().await?;
        Ok(ToolResult {
            output: String::from_utf8_lossy(&output.stdout).to_string(),
            metadata: json!({"stderr": String::from_utf8_lossy(&output.stderr)}),
        })
    }

    async fn execute_with_cancel(
        &self,
        args: Value,
        cancel: CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        let cmd = args["command"].as_str().unwrap_or("");
        let mut command = Command::new("powershell");
        command.args(["-NoProfile", "-Command", cmd]);
        if let Some(env) = args.get("env").and_then(|v| v.as_object()) {
            for (k, v) in env {
                if let Some(value) = v.as_str() {
                    command.env(k, value);
                }
            }
        }
        let mut child = command.spawn()?;
        let status = tokio::select! {
            _ = cancel.cancelled() => {
                let _ = child.kill().await;
                return Ok(ToolResult {
                    output: "command cancelled".to_string(),
                    metadata: json!({"cancelled": true}),
                });
            }
            result = child.wait() => result?
        };
        Ok(ToolResult {
            output: format!("command exited: {}", status),
            metadata: json!({}),
        })
    }
}

struct ReadTool;
#[async_trait]
impl Tool for ReadTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "read".to_string(),
            description: "Read file contents".to_string(),
            input_schema: json!({"type":"object","properties":{"path":{"type":"string"}}}),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let path = args["path"].as_str().unwrap_or("");
        if !is_path_allowed(path) {
            return Ok(ToolResult {
                output: "path denied by sandbox policy".to_string(),
                metadata: json!({"path": path}),
            });
        }
        let data = fs::read_to_string(path).await.unwrap_or_default();
        Ok(ToolResult {
            output: data,
            metadata: json!({}),
        })
    }
}

struct WriteTool;
#[async_trait]
impl Tool for WriteTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "write".to_string(),
            description: "Write file contents".to_string(),
            input_schema: json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}}}),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let path = args["path"].as_str().unwrap_or("");
        let content = args["content"].as_str().unwrap_or("");
        if !is_path_allowed(path) {
            return Ok(ToolResult {
                output: "path denied by sandbox policy".to_string(),
                metadata: json!({"path": path}),
            });
        }
        fs::write(path, content).await?;
        Ok(ToolResult {
            output: "ok".to_string(),
            metadata: json!({}),
        })
    }
}

struct EditTool;
#[async_trait]
impl Tool for EditTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "edit".to_string(),
            description: "String replacement edit".to_string(),
            input_schema: json!({"type":"object","properties":{"path":{"type":"string"},"old":{"type":"string"},"new":{"type":"string"}}}),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let path = args["path"].as_str().unwrap_or("");
        let old = args["old"].as_str().unwrap_or("");
        let new = args["new"].as_str().unwrap_or("");
        if !is_path_allowed(path) {
            return Ok(ToolResult {
                output: "path denied by sandbox policy".to_string(),
                metadata: json!({"path": path}),
            });
        }
        let content = fs::read_to_string(path).await.unwrap_or_default();
        let updated = content.replace(old, new);
        fs::write(path, updated).await?;
        Ok(ToolResult {
            output: "ok".to_string(),
            metadata: json!({}),
        })
    }
}

struct GlobTool;
#[async_trait]
impl Tool for GlobTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "glob".to_string(),
            description: "Find files by glob".to_string(),
            input_schema: json!({"type":"object","properties":{"pattern":{"type":"string"}}}),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let pattern = args["pattern"].as_str().unwrap_or("*");
        if pattern.contains("..") {
            return Ok(ToolResult {
                output: "pattern denied by sandbox policy".to_string(),
                metadata: json!({"pattern": pattern}),
            });
        }
        let mut files = Vec::new();
        for path in (glob::glob(pattern)?).flatten() {
            files.push(path.display().to_string());
            if files.len() >= 100 {
                break;
            }
        }
        Ok(ToolResult {
            output: files.join("\n"),
            metadata: json!({"count": files.len()}),
        })
    }
}

struct GrepTool;
#[async_trait]
impl Tool for GrepTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "grep".to_string(),
            description: "Regex search in files".to_string(),
            input_schema: json!({"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string"}}}),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let pattern = args["pattern"].as_str().unwrap_or("");
        let root = args["path"].as_str().unwrap_or(".");
        if !is_path_allowed(root) {
            return Ok(ToolResult {
                output: "path denied by sandbox policy".to_string(),
                metadata: json!({"path": root}),
            });
        }
        let regex = Regex::new(pattern)?;
        let mut out = Vec::new();
        for entry in WalkBuilder::new(root).build().flatten() {
            if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                continue;
            }
            let path = entry.path();
            if let Ok(content) = fs::read_to_string(path).await {
                for (idx, line) in content.lines().enumerate() {
                    if regex.is_match(line) {
                        out.push(format!("{}:{}:{}", path.display(), idx + 1, line));
                        if out.len() >= 100 {
                            break;
                        }
                    }
                }
            }
            if out.len() >= 100 {
                break;
            }
        }
        Ok(ToolResult {
            output: out.join("\n"),
            metadata: json!({"count": out.len()}),
        })
    }
}

struct WebFetchTool;
#[async_trait]
impl Tool for WebFetchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "webfetch".to_string(),
            description: "Fetch URL text".to_string(),
            input_schema: json!({"type":"object","properties":{"url":{"type":"string"}}}),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let url = args["url"].as_str().unwrap_or("");
        let body = reqwest::get(url).await?.text().await?;
        Ok(ToolResult {
            output: body.chars().take(20_000).collect(),
            metadata: json!({"truncated": body.len() > 20_000}),
        })
    }
}

struct WebSearchTool;
#[async_trait]
impl Tool for WebSearchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "websearch".to_string(),
            description: "Search web results from DuckDuckGo HTML endpoint".to_string(),
            input_schema: json!({"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"integer"}}}),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let query = args["query"].as_str().unwrap_or("").trim();
        if query.is_empty() {
            return Ok(ToolResult {
                output: "missing query".to_string(),
                metadata: json!({"count": 0}),
            });
        }
        let limit = args["limit"]
            .as_u64()
            .map(|v| v.clamp(1, 10) as usize)
            .unwrap_or(5);
        let encoded = query.replace(' ', "+");
        let url = format!("https://duckduckgo.com/html/?q={encoded}");
        let body = reqwest::get(&url).await?.text().await?;
        let re = Regex::new(r#"<a[^>]*class="result__a"[^>]*>(.*?)</a>"#)?;
        let tag_re = Regex::new(r"<[^>]+>")?;
        let mut lines = Vec::new();
        for cap in re.captures_iter(&body).take(limit) {
            let title = cap
                .get(1)
                .map(|m| m.as_str())
                .unwrap_or("")
                .replace("&amp;", "&")
                .replace("&quot;", "\"")
                .replace("&#x27;", "'");
            let clean = tag_re.replace_all(&title, "").to_string();
            if !clean.trim().is_empty() {
                lines.push(clean.trim().to_string());
            }
        }
        if lines.is_empty() {
            lines.push(format!("No search results parsed for query: {query}"));
        }
        Ok(ToolResult {
            output: lines.join("\n"),
            metadata: json!({"count": lines.len(), "query": query}),
        })
    }
}

struct CodeSearchTool;
#[async_trait]
impl Tool for CodeSearchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "codesearch".to_string(),
            description: "Search code in workspace files".to_string(),
            input_schema: json!({"type":"object","properties":{"query":{"type":"string"},"path":{"type":"string"},"limit":{"type":"integer"}}}),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let query = args["query"].as_str().unwrap_or("").trim();
        if query.is_empty() {
            return Ok(ToolResult {
                output: "missing query".to_string(),
                metadata: json!({"count": 0}),
            });
        }
        let root = args["path"].as_str().unwrap_or(".");
        if !is_path_allowed(root) {
            return Ok(ToolResult {
                output: "path denied by sandbox policy".to_string(),
                metadata: json!({"path": root}),
            });
        }
        let limit = args["limit"]
            .as_u64()
            .map(|v| v.clamp(1, 200) as usize)
            .unwrap_or(50);
        let mut hits = Vec::new();
        let lower = query.to_lowercase();
        for entry in WalkBuilder::new(root).build().flatten() {
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let path = entry.path();
            let ext = path.extension().and_then(|v| v.to_str()).unwrap_or("");
            if !matches!(
                ext,
                "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "md" | "toml" | "json"
            ) {
                continue;
            }
            if let Ok(content) = fs::read_to_string(path).await {
                for (idx, line) in content.lines().enumerate() {
                    if line.to_lowercase().contains(&lower) {
                        hits.push(format!("{}:{}:{}", path.display(), idx + 1, line.trim()));
                        if hits.len() >= limit {
                            break;
                        }
                    }
                }
            }
            if hits.len() >= limit {
                break;
            }
        }
        Ok(ToolResult {
            output: hits.join("\n"),
            metadata: json!({"count": hits.len(), "query": query}),
        })
    }
}

struct TodoWriteTool;
#[async_trait]
impl Tool for TodoWriteTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "todo_write".to_string(),
            description: "Update todo list".to_string(),
            input_schema: json!({"type":"object","properties":{"todos":{"type":"array"}}}),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let todos = normalize_todos(args["todos"].as_array().cloned().unwrap_or_default());
        Ok(ToolResult {
            output: format!("todo list updated: {} items", todos.len()),
            metadata: json!({"todos": todos}),
        })
    }
}

struct TaskTool;
#[async_trait]
impl Tool for TaskTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "task".to_string(),
            description: "Create a subtask summary for orchestrator".to_string(),
            input_schema: json!({"type":"object","properties":{"description":{"type":"string"},"prompt":{"type":"string"}}}),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let description = args["description"].as_str().unwrap_or("subtask");
        Ok(ToolResult {
            output: format!("Subtask planned: {description}"),
            metadata: json!({"description": description, "prompt": args["prompt"]}),
        })
    }
}

struct QuestionTool;
#[async_trait]
impl Tool for QuestionTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "question".to_string(),
            description: "Emit a question request for the user".to_string(),
            input_schema: json!({"type":"object","properties":{"questions":{"type":"array"}}}),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        Ok(ToolResult {
            output: "Question requested. Use /question endpoints to respond.".to_string(),
            metadata: json!({"questions": args["questions"]}),
        })
    }
}

struct SkillTool;
#[async_trait]
impl Tool for SkillTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "skill".to_string(),
            description: "Inspect installed Codex skills".to_string(),
            input_schema: json!({"type":"object","properties":{"name":{"type":"string"}}}),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let name = args["name"].as_str().unwrap_or("unknown");
        let mut found = Vec::new();
        for root in [".codex/skills", ".codex/skills/.system"] {
            let path = PathBuf::from(root).join(name).join("SKILL.md");
            if path.exists() {
                found.push(path.display().to_string());
            }
        }
        Ok(ToolResult {
            output: if found.is_empty() {
                format!("Skill `{name}` not found in local skill directories.")
            } else {
                format!("Skill `{name}` found:\n{}", found.join("\n"))
            },
            metadata: json!({"name": name, "matches": found}),
        })
    }
}

struct ApplyPatchTool;
#[async_trait]
impl Tool for ApplyPatchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "apply_patch".to_string(),
            description: "Validate patch text and report applicability".to_string(),
            input_schema: json!({"type":"object","properties":{"patchText":{"type":"string"}}}),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let patch = args["patchText"].as_str().unwrap_or("");
        let has_begin = patch.contains("*** Begin Patch");
        let has_end = patch.contains("*** End Patch");
        let file_ops = patch
            .lines()
            .filter(|line| {
                line.starts_with("*** Add File:")
                    || line.starts_with("*** Update File:")
                    || line.starts_with("*** Delete File:")
            })
            .count();
        let valid = has_begin && has_end && file_ops > 0;
        Ok(ToolResult {
            output: if valid {
                "Patch format validated. Host-level patch application must execute this patch."
                    .to_string()
            } else {
                "Invalid patch format. Expected Begin/End markers and at least one file operation."
                    .to_string()
            },
            metadata: json!({"valid": valid, "fileOps": file_ops}),
        })
    }
}

struct BatchTool;
#[async_trait]
impl Tool for BatchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "batch".to_string(),
            description: "Execute multiple tool calls sequentially".to_string(),
            input_schema: json!({"type":"object","properties":{"tool_calls":{"type":"array"}}}),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let calls = args["tool_calls"].as_array().cloned().unwrap_or_default();
        let registry = ToolRegistry::new();
        let mut outputs = Vec::new();
        for call in calls.iter().take(20) {
            let tool = call
                .get("tool")
                .or_else(|| call.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if tool.is_empty() || tool == "batch" {
                continue;
            }
            let call_args = call.get("args").cloned().unwrap_or_else(|| json!({}));
            let result = registry.execute(tool, call_args).await?;
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
        ToolSchema {
            name: "lsp".to_string(),
            description: "LSP-like workspace diagnostics and symbol operations".to_string(),
            input_schema: json!({"type":"object","properties":{"operation":{"type":"string"},"filePath":{"type":"string"},"symbol":{"type":"string"},"query":{"type":"string"}}}),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let operation = args["operation"].as_str().unwrap_or("symbols");
        let output = match operation {
            "diagnostics" => {
                let path = args["filePath"].as_str().unwrap_or("");
                if path.is_empty() || !is_path_allowed(path) {
                    "missing or unsafe filePath".to_string()
                } else {
                    diagnostics_for_path(path).await
                }
            }
            "definition" => {
                let symbol = args["symbol"].as_str().unwrap_or("");
                find_symbol_definition(symbol).await
            }
            "references" => {
                let symbol = args["symbol"].as_str().unwrap_or("");
                find_symbol_references(symbol).await
            }
            _ => {
                let query = args["query"]
                    .as_str()
                    .or_else(|| args["symbol"].as_str())
                    .unwrap_or("");
                list_symbols(query).await
            }
        };
        Ok(ToolResult {
            output,
            metadata: json!({"operation": operation}),
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
                .unwrap_or_else(|| format!("todo-{}", TODO_SEQ.fetch_add(1, Ordering::Relaxed)));
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

async fn list_symbols(query: &str) -> String {
    let query = query.to_lowercase();
    let rust_fn = Regex::new(r"^\s*(pub\s+)?(async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)")
        .unwrap_or_else(|_| Regex::new("$^").expect("regex"));
    let mut out = Vec::new();
    for entry in WalkBuilder::new(".").build().flatten() {
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

async fn find_symbol_definition(symbol: &str) -> String {
    if symbol.trim().is_empty() {
        return "missing symbol".to_string();
    }
    let listed = list_symbols(symbol).await;
    listed
        .lines()
        .find(|line| line.ends_with(&format!("fn {symbol}")))
        .map(ToString::to_string)
        .unwrap_or_else(|| "symbol not found".to_string())
}

async fn find_symbol_references(symbol: &str) -> String {
    if symbol.trim().is_empty() {
        return "missing symbol".to_string();
    }
    let escaped = regex::escape(symbol);
    let re = Regex::new(&format!(r"\b{}\b", escaped));
    let Ok(re) = re else {
        return "invalid symbol".to_string();
    };
    let mut refs = Vec::new();
    for entry in WalkBuilder::new(".").build().flatten() {
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        if let Ok(content) = fs::read_to_string(path).await {
            for (idx, line) in content.lines().enumerate() {
                if re.is_match(line) {
                    refs.push(format!("{}:{}:{}", path.display(), idx + 1, line.trim()));
                    if refs.len() >= 200 {
                        return refs.join("\n");
                    }
                }
            }
        }
    }
    refs.join("\n")
}
