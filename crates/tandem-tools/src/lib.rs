use std::collections::{hash_map::DefaultHasher, HashMap};
use std::hash::{Hash, Hasher};
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

use futures_util::StreamExt;
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
        map.insert(
            "webfetch_document".to_string(),
            Arc::new(WebFetchDocumentTool),
        );
        map.insert("mcp_debug".to_string(), Arc::new(McpDebugTool));
        map.insert("websearch".to_string(), Arc::new(WebSearchTool));
        map.insert("codesearch".to_string(), Arc::new(CodeSearchTool));
        let todo_tool: Arc<dyn Tool> = Arc::new(TodoWriteTool);
        map.insert("todo_write".to_string(), todo_tool.clone());
        map.insert("todowrite".to_string(), todo_tool.clone());
        map.insert("update_todo_list".to_string(), todo_tool);
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
        let mut dedup: HashMap<String, ToolSchema> = HashMap::new();
        for schema in self.tools.read().await.values().map(|t| t.schema()) {
            dedup.entry(schema.name.clone()).or_insert(schema);
        }
        let mut schemas = dedup.into_values().collect::<Vec<_>>();
        schemas.sort_by(|a, b| a.name.cmp(&b.name));
        schemas
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolSchemaValidationError {
    pub tool_name: String,
    pub path: String,
    pub reason: String,
}

impl std::fmt::Display for ToolSchemaValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid tool schema `{}` at `{}`: {}",
            self.tool_name, self.path, self.reason
        )
    }
}

impl std::error::Error for ToolSchemaValidationError {}

pub fn validate_tool_schemas(schemas: &[ToolSchema]) -> Result<(), ToolSchemaValidationError> {
    for schema in schemas {
        validate_schema_node(&schema.name, "$", &schema.input_schema)?;
    }
    Ok(())
}

fn validate_schema_node(
    tool_name: &str,
    path: &str,
    value: &Value,
) -> Result<(), ToolSchemaValidationError> {
    let Some(obj) = value.as_object() else {
        if let Some(arr) = value.as_array() {
            for (idx, item) in arr.iter().enumerate() {
                validate_schema_node(tool_name, &format!("{path}[{idx}]"), item)?;
            }
        }
        return Ok(());
    };

    if obj.get("type").and_then(|t| t.as_str()) == Some("array") && !obj.contains_key("items") {
        return Err(ToolSchemaValidationError {
            tool_name: tool_name.to_string(),
            path: path.to_string(),
            reason: "array schema missing items".to_string(),
        });
    }

    if let Some(items) = obj.get("items") {
        validate_schema_node(tool_name, &format!("{path}.items"), items)?;
    }
    if let Some(props) = obj.get("properties").and_then(|v| v.as_object()) {
        for (key, child) in props {
            validate_schema_node(tool_name, &format!("{path}.properties.{key}"), child)?;
        }
    }
    if let Some(additional_props) = obj.get("additionalProperties") {
        validate_schema_node(
            tool_name,
            &format!("{path}.additionalProperties"),
            additional_props,
        )?;
    }
    if let Some(one_of) = obj.get("oneOf").and_then(|v| v.as_array()) {
        for (idx, child) in one_of.iter().enumerate() {
            validate_schema_node(tool_name, &format!("{path}.oneOf[{idx}]"), child)?;
        }
    }
    if let Some(any_of) = obj.get("anyOf").and_then(|v| v.as_array()) {
        for (idx, child) in any_of.iter().enumerate() {
            validate_schema_node(tool_name, &format!("{path}.anyOf[{idx}]"), child)?;
        }
    }
    if let Some(all_of) = obj.get("allOf").and_then(|v| v.as_array()) {
        for (idx, child) in all_of.iter().enumerate() {
            validate_schema_node(tool_name, &format!("{path}.allOf[{idx}]"), child)?;
        }
    }

    Ok(())
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

struct WebFetchDocumentTool;
#[async_trait]
impl Tool for WebFetchDocumentTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "webfetch_document".to_string(),
            description: "Fetch URL content and return a structured markdown document".to_string(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "url":{"type":"string"},
                    "mode":{"type":"string"},
                    "return":{"type":"string"},
                    "max_bytes":{"type":"integer"},
                    "timeout_ms":{"type":"integer"},
                    "max_redirects":{"type":"integer"}
                }
            }),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let url = args["url"].as_str().unwrap_or("").trim();
        if url.is_empty() {
            return Ok(ToolResult {
                output: "url is required".to_string(),
                metadata: json!({"url": url}),
            });
        }
        let mode = args["mode"].as_str().unwrap_or("auto");
        let return_mode = args["return"].as_str().unwrap_or("both");
        let timeout_ms = args["timeout_ms"]
            .as_u64()
            .unwrap_or(15_000)
            .clamp(1_000, 120_000);
        let max_bytes = args["max_bytes"].as_u64().unwrap_or(500_000).min(5_000_000) as usize;
        let max_redirects = args["max_redirects"].as_u64().unwrap_or(5).min(20) as usize;

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(timeout_ms))
            .redirect(reqwest::redirect::Policy::limited(max_redirects))
            .build()?;

        let started = std::time::Instant::now();
        let res = client
            .get(url)
            .header(
                "Accept",
                "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
            )
            .send()
            .await?;
        let final_url = res.url().to_string();
        let content_type = res
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let mut stream = res.bytes_stream();
        let mut buffer: Vec<u8> = Vec::new();
        let mut truncated = false;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if buffer.len() + chunk.len() > max_bytes {
                let remaining = max_bytes.saturating_sub(buffer.len());
                buffer.extend_from_slice(&chunk[..remaining]);
                truncated = true;
                break;
            }
            buffer.extend_from_slice(&chunk);
        }
        let raw = String::from_utf8_lossy(&buffer).to_string();

        let cleaned = strip_html_noise(&raw);
        let title = extract_title(&cleaned).unwrap_or_default();
        let canonical = extract_canonical(&cleaned);
        let links = extract_links(&cleaned);

        let markdown = if content_type.contains("html") || content_type.is_empty() {
            html2md::parse_html(&cleaned)
        } else {
            cleaned.clone()
        };

        let text = markdown_to_text(&markdown);

        let markdown_out = if return_mode == "text" {
            String::new()
        } else {
            markdown
        };
        let text_out = if return_mode == "markdown" {
            String::new()
        } else {
            text
        };

        let raw_chars = raw.chars().count();
        let markdown_chars = markdown_out.chars().count();
        let reduction_pct = if raw_chars == 0 {
            0.0
        } else {
            ((raw_chars.saturating_sub(markdown_chars)) as f64 / raw_chars as f64) * 100.0
        };

        let output = json!({
            "url": url,
            "final_url": final_url,
            "title": title,
            "content_type": content_type,
            "markdown": markdown_out,
            "text": text_out,
            "links": links,
            "meta": {
                "canonical": canonical,
                "mode": mode
            },
            "stats": {
                "bytes_in": buffer.len(),
                "bytes_out": markdown_chars,
                "raw_chars": raw_chars,
                "markdown_chars": markdown_chars,
                "reduction_pct": reduction_pct,
                "elapsed_ms": started.elapsed().as_millis(),
                "truncated": truncated
            }
        });

        Ok(ToolResult {
            output: serde_json::to_string_pretty(&output)?,
            metadata: json!({
                "url": url,
                "final_url": final_url,
                "content_type": content_type,
                "truncated": truncated
            }),
        })
    }
}

fn strip_html_noise(input: &str) -> String {
    let script_re = Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap();
    let style_re = Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap();
    let noscript_re = Regex::new(r"(?is)<noscript[^>]*>.*?</noscript>").unwrap();
    let cleaned = script_re.replace_all(input, "");
    let cleaned = style_re.replace_all(&cleaned, "");
    let cleaned = noscript_re.replace_all(&cleaned, "");
    cleaned.to_string()
}

fn extract_title(input: &str) -> Option<String> {
    let title_re = Regex::new(r"(?is)<title[^>]*>(.*?)</title>").ok()?;
    let caps = title_re.captures(input)?;
    let raw = caps.get(1)?.as_str();
    let tag_re = Regex::new(r"(?is)<[^>]+>").ok()?;
    Some(tag_re.replace_all(raw, "").trim().to_string())
}

fn extract_canonical(input: &str) -> Option<String> {
    let canon_re =
        Regex::new(r#"(?is)<link[^>]*rel=["']canonical["'][^>]*href=["']([^"']+)["'][^>]*>"#)
            .ok()?;
    let caps = canon_re.captures(input)?;
    Some(caps.get(1)?.as_str().trim().to_string())
}

fn extract_links(input: &str) -> Vec<Value> {
    let link_re = Regex::new(r#"(?is)<a[^>]*href=["']([^"']+)["'][^>]*>(.*?)</a>"#).unwrap();
    let tag_re = Regex::new(r"(?is)<[^>]+>").unwrap();
    let mut out = Vec::new();
    for caps in link_re.captures_iter(input).take(200) {
        let href = caps.get(1).map(|m| m.as_str()).unwrap_or("").trim();
        let raw_text = caps.get(2).map(|m| m.as_str()).unwrap_or("");
        let text = tag_re.replace_all(raw_text, "");
        if !href.is_empty() {
            out.push(json!({
                "text": text.trim(),
                "href": href
            }));
        }
    }
    out
}

fn markdown_to_text(input: &str) -> String {
    let code_block_re = Regex::new(r"(?s)```.*?```").unwrap();
    let inline_code_re = Regex::new(r"`[^`]*`").unwrap();
    let link_re = Regex::new(r"\[([^\]]+)\]\([^)]+\)").unwrap();
    let emphasis_re = Regex::new(r"[*_~]+").unwrap();
    let cleaned = code_block_re.replace_all(input, "");
    let cleaned = inline_code_re.replace_all(&cleaned, "");
    let cleaned = link_re.replace_all(&cleaned, "$1");
    let cleaned = emphasis_re.replace_all(&cleaned, "");
    let cleaned = cleaned.replace('#', "");
    let whitespace_re = Regex::new(r"\n{3,}").unwrap();
    let cleaned = whitespace_re.replace_all(&cleaned, "\n\n");
    cleaned.trim().to_string()
}

struct McpDebugTool;
#[async_trait]
impl Tool for McpDebugTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "mcp_debug".to_string(),
            description: "Call an MCP tool and return the raw response".to_string(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "url":{"type":"string"},
                    "tool":{"type":"string"},
                    "args":{"type":"object"},
                    "headers":{"type":"object"},
                    "timeout_ms":{"type":"integer"},
                    "max_bytes":{"type":"integer"}
                }
            }),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let url = args["url"].as_str().unwrap_or("").trim();
        let tool = args["tool"].as_str().unwrap_or("").trim();
        if url.is_empty() || tool.is_empty() {
            return Ok(ToolResult {
                output: "url and tool are required".to_string(),
                metadata: json!({"url": url, "tool": tool}),
            });
        }
        let timeout_ms = args["timeout_ms"]
            .as_u64()
            .unwrap_or(15_000)
            .clamp(1_000, 120_000);
        let max_bytes = args["max_bytes"].as_u64().unwrap_or(200_000).min(5_000_000) as usize;
        let request_args = args.get("args").cloned().unwrap_or_else(|| json!({}));

        #[derive(serde::Serialize)]
        struct McpCallRequest {
            jsonrpc: String,
            id: u32,
            method: String,
            params: McpCallParams,
        }

        #[derive(serde::Serialize)]
        struct McpCallParams {
            name: String,
            arguments: Value,
        }

        let request = McpCallRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "tools/call".to_string(),
            params: McpCallParams {
                name: tool.to_string(),
                arguments: request_args,
            },
        };

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(timeout_ms))
            .build()?;

        let mut builder = client
            .post(url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream");

        if let Some(headers) = args.get("headers").and_then(|v| v.as_object()) {
            for (key, value) in headers {
                if let Some(value) = value.as_str() {
                    builder = builder.header(key, value);
                }
            }
        }

        let res = builder.json(&request).send().await?;
        let status = res.status().as_u16();

        let mut response_headers = serde_json::Map::new();
        for (key, value) in res.headers().iter() {
            if let Ok(value) = value.to_str() {
                response_headers.insert(key.to_string(), Value::String(value.to_string()));
            }
        }

        let mut stream = res.bytes_stream();
        let mut buffer: Vec<u8> = Vec::new();
        let mut truncated = false;

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            if buffer.len() + chunk.len() > max_bytes {
                let remaining = max_bytes.saturating_sub(buffer.len());
                buffer.extend_from_slice(&chunk[..remaining]);
                truncated = true;
                break;
            }
            buffer.extend_from_slice(&chunk);
        }

        let body = String::from_utf8_lossy(&buffer).to_string();
        let output = json!({
            "status": status,
            "headers": response_headers,
            "body": body,
            "truncated": truncated,
            "bytes": buffer.len()
        });

        Ok(ToolResult {
            output: serde_json::to_string_pretty(&output)?,
            metadata: json!({
                "url": url,
                "tool": tool,
                "timeout_ms": timeout_ms,
                "max_bytes": max_bytes
            }),
        })
    }
}

struct WebSearchTool;
#[async_trait]
impl Tool for WebSearchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "websearch".to_string(),
            description: "Search web results using Exa.ai MCP endpoint".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer" }
                },
                "required": ["query"]
            }),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let query = extract_websearch_query(&args).unwrap_or_default();
        let query_source = args
            .get("__query_source")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                if query.is_empty() {
                    "missing".to_string()
                } else {
                    "tool_args".to_string()
                }
            });
        let query_hash = if query.is_empty() {
            None
        } else {
            Some(stable_hash(&query))
        };
        if query.is_empty() {
            tracing::warn!("WebSearchTool missing query. Args: {}", args);
            return Ok(ToolResult {
                output: format!("missing query. Received args: {}", args),
                metadata: json!({
                    "count": 0,
                    "error": "missing_query",
                    "query_source": query_source,
                    "query_hash": query_hash,
                    "loop_guard_triggered": false
                }),
            });
        }
        let num_results = extract_websearch_limit(&args).unwrap_or(8);

        #[derive(serde::Serialize)]
        struct McpSearchRequest {
            jsonrpc: String,
            id: u32,
            method: String,
            params: McpSearchParams,
        }

        #[derive(serde::Serialize)]
        struct McpSearchParams {
            name: String,
            arguments: McpSearchArgs,
        }

        #[derive(serde::Serialize)]
        struct McpSearchArgs {
            query: String,
            #[serde(rename = "numResults")]
            num_results: u64,
        }

        let request = McpSearchRequest {
            jsonrpc: "2.0".to_string(),
            id: 1,
            method: "tools/call".to_string(),
            params: McpSearchParams {
                name: "web_search_exa".to_string(),
                arguments: McpSearchArgs {
                    query: query.to_string(),
                    num_results,
                },
            },
        };

        let client = reqwest::Client::new();
        let res = client
            .post("https://mcp.exa.ai/mcp")
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .json(&request)
            .send()
            .await?;

        if !res.status().is_success() {
            let error_text = res.text().await?;
            return Err(anyhow::anyhow!("Search error: {}", error_text));
        }

        let mut stream = res.bytes_stream();
        let mut buffer = Vec::new();
        let timeout_duration = std::time::Duration::from_secs(10); // Wait at most 10s for first chunk

        // We use a loop but breaks on first result.
        // We also want to apply a timeout to receiving ANY chunk from the stream.
        loop {
            let chunk_future = stream.next();
            match tokio::time::timeout(timeout_duration, chunk_future).await {
                Ok(Some(chunk_result)) => {
                    let chunk = chunk_result?;
                    tracing::info!("WebSearchTool received chunk size: {}", chunk.len());
                    buffer.extend_from_slice(&chunk);

                    while let Some(idx) = buffer.iter().position(|&b| b == b'\n') {
                        let line_bytes: Vec<u8> = buffer.drain(..=idx).collect();
                        let line = String::from_utf8_lossy(&line_bytes);
                        let line = line.trim();
                        tracing::info!("WebSearchTool parsing line: {}", line);

                        if let Some(data) = line.strip_prefix("data: ") {
                            if let Ok(val) = serde_json::from_str::<Value>(data.trim()) {
                                if let Some(content) = val
                                    .get("result")
                                    .and_then(|r| r.get("content"))
                                    .and_then(|c| c.as_array())
                                {
                                    if let Some(first) = content.first() {
                                        if let Some(text) =
                                            first.get("text").and_then(|t| t.as_str())
                                        {
                                            return Ok(ToolResult {
                                                output: text.to_string(),
                                                metadata: json!({
                                                    "query": query,
                                                    "query_source": query_source,
                                                    "query_hash": query_hash,
                                                    "loop_guard_triggered": false
                                                }),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(None) => {
                    tracing::info!("WebSearchTool stream ended without result.");
                    break;
                }
                Err(_) => {
                    tracing::warn!("WebSearchTool stream timed out waiting for chunk.");
                    return Ok(ToolResult {
                        output: "Search timed out. No results received.".to_string(),
                        metadata: json!({
                            "query": query,
                            "error": "timeout",
                            "query_source": query_source,
                            "query_hash": query_hash,
                            "loop_guard_triggered": false
                        }),
                    });
                }
            }
        }

        Ok(ToolResult {
            output: "No search results found.".to_string(),
            metadata: json!({
                "query": query,
                "query_source": query_source,
                "query_hash": query_hash,
                "loop_guard_triggered": false
            }),
        })
    }
}

fn stable_hash(input: &str) -> String {
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn extract_websearch_query(args: &Value) -> Option<String> {
    // Direct keys first.
    const QUERY_KEYS: [&str; 5] = ["query", "q", "search_query", "searchQuery", "keywords"];
    for key in QUERY_KEYS {
        if let Some(query) = args.get(key).and_then(|v| v.as_str()) {
            let trimmed = query.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    // Some tool-call envelopes nest args.
    for container in ["arguments", "args", "input", "params"] {
        if let Some(obj) = args.get(container) {
            for key in QUERY_KEYS {
                if let Some(query) = obj.get(key).and_then(|v| v.as_str()) {
                    let trimmed = query.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
    }

    // Last resort: plain string args.
    args.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

fn extract_websearch_limit(args: &Value) -> Option<u64> {
    let mut read_limit = |value: &Value| value.as_u64().map(|v| v.clamp(1, 10));

    if let Some(limit) = args
        .get("limit")
        .and_then(&mut read_limit)
        .or_else(|| args.get("numResults").and_then(&mut read_limit))
        .or_else(|| args.get("num_results").and_then(&mut read_limit))
    {
        return Some(limit);
    }

    for container in ["arguments", "args", "input", "params"] {
        if let Some(obj) = args.get(container) {
            if let Some(limit) = obj
                .get("limit")
                .and_then(&mut read_limit)
                .or_else(|| obj.get("numResults").and_then(&mut read_limit))
                .or_else(|| obj.get("num_results").and_then(&mut read_limit))
            {
                return Some(limit);
            }
        }
    }
    None
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
            input_schema: json!({
                "type":"object",
                "properties":{
                    "todos":{
                        "type":"array",
                        "items":{
                            "type":"object",
                            "properties":{
                                "id":{"type":"string"},
                                "content":{"type":"string"},
                                "text":{"type":"string"},
                                "status":{"type":"string"}
                            }
                        }
                    }
                }
            }),
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
            input_schema: json!({
                "type":"object",
                "properties":{
                    "questions":{
                        "type":"array",
                        "items":{
                            "type":"object",
                            "properties":{
                                "question":{"type":"string"},
                                "choices":{"type":"array","items":{"type":"string"}}
                            }
                        }
                    }
                }
            }),
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
            input_schema: json!({
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn validator_rejects_array_without_items() {
        let schemas = vec![ToolSchema {
            name: "bad".to_string(),
            description: "bad schema".to_string(),
            input_schema: json!({
                "type":"object",
                "properties":{"todos":{"type":"array"}}
            }),
        }];
        let err = validate_tool_schemas(&schemas).expect_err("expected schema validation failure");
        assert_eq!(err.tool_name, "bad");
        assert!(err.path.contains("properties.todos"));
    }

    #[tokio::test]
    async fn registry_schemas_are_unique_and_valid() {
        let registry = ToolRegistry::new();
        let schemas = registry.list().await;
        validate_tool_schemas(&schemas).expect("registry tool schemas should validate");
        let unique = schemas
            .iter()
            .map(|schema| schema.name.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(
            unique.len(),
            schemas.len(),
            "tool schemas must be unique by name"
        );
    }

    #[test]
    fn websearch_query_extraction_accepts_aliases_and_nested_shapes() {
        let direct = json!({"query":"meaning of life"});
        assert_eq!(
            extract_websearch_query(&direct).as_deref(),
            Some("meaning of life")
        );

        let alias = json!({"q":"hello"});
        assert_eq!(extract_websearch_query(&alias).as_deref(), Some("hello"));

        let nested = json!({"arguments":{"search_query":"rust tokio"}});
        assert_eq!(
            extract_websearch_query(&nested).as_deref(),
            Some("rust tokio")
        );

        let as_string = json!("find docs");
        assert_eq!(
            extract_websearch_query(&as_string).as_deref(),
            Some("find docs")
        );
    }

    #[test]
    fn websearch_limit_extraction_clamps_and_reads_nested_fields() {
        assert_eq!(extract_websearch_limit(&json!({"limit": 100})), Some(10));
        assert_eq!(
            extract_websearch_limit(&json!({"arguments":{"numResults": 0}})),
            Some(1)
        );
        assert_eq!(
            extract_websearch_limit(&json!({"input":{"num_results": 6}})),
            Some(6)
        );
    }
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
