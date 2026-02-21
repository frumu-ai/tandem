use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use ignore::WalkBuilder;
use regex::Regex;
use serde_json::{json, Value};
use tandem_skills::SkillService;
use tokio::fs;
use tokio::process::Command;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use futures_util::StreamExt;
use tandem_memory::types::{MemorySearchResult, MemoryTier};
use tandem_memory::MemoryManager;
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
        map.insert("spawn_agent".to_string(), Arc::new(SpawnAgentTool));
        map.insert("skill".to_string(), Arc::new(SkillTool));
        map.insert("memory_store".to_string(), Arc::new(MemoryStoreTool));
        map.insert("memory_list".to_string(), Arc::new(MemoryListTool));
        map.insert("memory_search".to_string(), Arc::new(MemorySearchTool));
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
        let tool = {
            let tools = self.tools.read().await;
            resolve_registered_tool(&tools, name)
        };
        let Some(tool) = tool else {
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
        let tool = {
            let tools = self.tools.read().await;
            resolve_registered_tool(&tools, name)
        };
        let Some(tool) = tool else {
            return Ok(ToolResult {
                output: format!("Unknown tool: {name}"),
                metadata: json!({}),
            });
        };
        tool.execute_with_cancel(args, cancel).await
    }
}

fn canonical_tool_name(name: &str) -> String {
    match name.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "todowrite" | "update_todo_list" | "update_todos" => "todo_write".to_string(),
        "run_command" | "shell" | "powershell" | "cmd" => "bash".to_string(),
        other => other.to_string(),
    }
}

fn strip_known_tool_namespace(name: &str) -> Option<String> {
    const PREFIXES: [&str; 8] = [
        "default_api:",
        "default_api.",
        "functions.",
        "function.",
        "tools.",
        "tool.",
        "builtin:",
        "builtin.",
    ];
    for prefix in PREFIXES {
        if let Some(rest) = name.strip_prefix(prefix) {
            let trimmed = rest.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn resolve_registered_tool(
    tools: &HashMap<String, Arc<dyn Tool>>,
    requested_name: &str,
) -> Option<Arc<dyn Tool>> {
    let canonical = canonical_tool_name(requested_name);
    if let Some(tool) = tools.get(&canonical) {
        return Some(tool.clone());
    }
    if let Some(stripped) = strip_known_tool_namespace(&canonical) {
        let stripped = canonical_tool_name(&stripped);
        if let Some(tool) = tools.get(&stripped) {
            return Some(tool.clone());
        }
    }
    None
}

fn is_batch_wrapper_tool_name(name: &str) -> bool {
    matches!(
        canonical_tool_name(name).as_str(),
        "default_api" | "default" | "api" | "function" | "functions" | "tool" | "tools"
    )
}

fn non_empty_batch_str<'a>(value: Option<&'a Value>) -> Option<&'a str> {
    value
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn resolve_batch_call_tool_name(call: &Value) -> Option<String> {
    let tool = non_empty_batch_str(call.get("tool"))
        .or_else(|| {
            call.get("tool")
                .and_then(|v| v.as_object())
                .and_then(|obj| non_empty_batch_str(obj.get("name")))
        })
        .or_else(|| {
            call.get("function")
                .and_then(|v| v.as_object())
                .and_then(|obj| non_empty_batch_str(obj.get("tool")))
        })
        .or_else(|| {
            call.get("function_call")
                .and_then(|v| v.as_object())
                .and_then(|obj| non_empty_batch_str(obj.get("tool")))
        })
        .or_else(|| {
            call.get("call")
                .and_then(|v| v.as_object())
                .and_then(|obj| non_empty_batch_str(obj.get("tool")))
        });
    let name = non_empty_batch_str(call.get("name"))
        .or_else(|| {
            call.get("function")
                .and_then(|v| v.as_object())
                .and_then(|obj| non_empty_batch_str(obj.get("name")))
        })
        .or_else(|| {
            call.get("function_call")
                .and_then(|v| v.as_object())
                .and_then(|obj| non_empty_batch_str(obj.get("name")))
        })
        .or_else(|| {
            call.get("call")
                .and_then(|v| v.as_object())
                .and_then(|obj| non_empty_batch_str(obj.get("name")))
        })
        .or_else(|| {
            call.get("tool")
                .and_then(|v| v.as_object())
                .and_then(|obj| non_empty_batch_str(obj.get("name")))
        });

    match (tool, name) {
        (Some(t), Some(n)) => {
            if is_batch_wrapper_tool_name(t) {
                Some(n.to_string())
            } else if let Some(stripped) = strip_known_tool_namespace(t) {
                Some(stripped)
            } else {
                Some(t.to_string())
            }
        }
        (Some(t), None) => {
            if is_batch_wrapper_tool_name(t) {
                None
            } else if let Some(stripped) = strip_known_tool_namespace(t) {
                Some(stripped)
            } else {
                Some(t.to_string())
            }
        }
        (None, Some(n)) => Some(n.to_string()),
        (None, None) => None,
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
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
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return false;
    }
    if is_root_only_path_token(trimmed) || is_malformed_tool_path_token(trimmed) {
        return false;
    }
    let raw = Path::new(trimmed);
    if raw.is_absolute() {
        return false;
    }
    !raw.components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
}

fn is_root_only_path_token(path: &str) -> bool {
    if matches!(path, "/" | "\\" | "." | ".." | "~") {
        return true;
    }
    let bytes = path.as_bytes();
    if bytes.len() == 2 && bytes[1] == b':' && (bytes[0] as char).is_ascii_alphabetic() {
        return true;
    }
    if bytes.len() == 3
        && bytes[1] == b':'
        && (bytes[0] as char).is_ascii_alphabetic()
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return true;
    }
    false
}

fn is_malformed_tool_path_token(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    if lower.contains("<tool_call")
        || lower.contains("</tool_call")
        || lower.contains("<function=")
        || lower.contains("<parameter=")
        || lower.contains("</function>")
        || lower.contains("</parameter>")
    {
        return true;
    }
    if path.contains('\n') || path.contains('\r') {
        return true;
    }
    if path.contains('*') || path.contains('?') {
        return true;
    }
    false
}

struct BashTool;
#[async_trait]
impl Tool for BashTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "bash".to_string(),
            description: "Run shell command".to_string(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "command":{"type":"string"}
                },
                "required":["command"]
            }),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let cmd = args["command"].as_str().unwrap_or("").trim();
        if cmd.is_empty() {
            anyhow::bail!("BASH_COMMAND_MISSING");
        }
        let shell = match build_shell_command(cmd) {
            ShellCommandPlan::Execute(plan) => plan,
            ShellCommandPlan::Blocked(result) => return Ok(result),
        };
        let ShellExecutionPlan {
            mut command,
            translated_command,
            os_guardrail_applied,
            guardrail_reason,
        } = shell;
        if let Some(env) = args.get("env").and_then(|v| v.as_object()) {
            for (k, v) in env {
                if let Some(value) = v.as_str() {
                    command.env(k, value);
                }
            }
        }
        let output = command.output().await?;
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let metadata = shell_metadata(
            translated_command.as_deref(),
            os_guardrail_applied,
            guardrail_reason.as_deref(),
            stderr,
        );
        Ok(ToolResult {
            output: String::from_utf8_lossy(&output.stdout).to_string(),
            metadata,
        })
    }

    async fn execute_with_cancel(
        &self,
        args: Value,
        cancel: CancellationToken,
    ) -> anyhow::Result<ToolResult> {
        let cmd = args["command"].as_str().unwrap_or("").trim();
        if cmd.is_empty() {
            anyhow::bail!("BASH_COMMAND_MISSING");
        }
        let shell = match build_shell_command(cmd) {
            ShellCommandPlan::Execute(plan) => plan,
            ShellCommandPlan::Blocked(result) => return Ok(result),
        };
        let ShellExecutionPlan {
            mut command,
            translated_command,
            os_guardrail_applied,
            guardrail_reason,
        } = shell;
        if let Some(env) = args.get("env").and_then(|v| v.as_object()) {
            for (k, v) in env {
                if let Some(value) = v.as_str() {
                    command.env(k, value);
                }
            }
        }
        command.stdout(Stdio::null());
        command.stderr(Stdio::piped());
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
        let stderr = match child.stderr.take() {
            Some(mut handle) => {
                use tokio::io::AsyncReadExt;
                let mut buf = Vec::new();
                let _ = handle.read_to_end(&mut buf).await;
                String::from_utf8_lossy(&buf).to_string()
            }
            None => String::new(),
        };
        let mut metadata = shell_metadata(
            translated_command.as_deref(),
            os_guardrail_applied,
            guardrail_reason.as_deref(),
            stderr,
        );
        if let Some(obj) = metadata.as_object_mut() {
            obj.insert("exit_code".to_string(), json!(status.code()));
        }
        Ok(ToolResult {
            output: format!("command exited: {}", status),
            metadata,
        })
    }
}

struct ShellExecutionPlan {
    command: Command,
    translated_command: Option<String>,
    os_guardrail_applied: bool,
    guardrail_reason: Option<String>,
}

fn shell_metadata(
    translated_command: Option<&str>,
    os_guardrail_applied: bool,
    guardrail_reason: Option<&str>,
    stderr: String,
) -> Value {
    let mut metadata = json!({
        "stderr": stderr,
        "os_guardrail_applied": os_guardrail_applied,
    });
    if let Some(obj) = metadata.as_object_mut() {
        if let Some(translated) = translated_command {
            obj.insert(
                "translated_command".to_string(),
                Value::String(translated.to_string()),
            );
        }
        if let Some(reason) = guardrail_reason {
            obj.insert(
                "guardrail_reason".to_string(),
                Value::String(reason.to_string()),
            );
        }
    }
    metadata
}

enum ShellCommandPlan {
    Execute(ShellExecutionPlan),
    Blocked(ToolResult),
}

fn build_shell_command(raw_cmd: &str) -> ShellCommandPlan {
    #[cfg(windows)]
    {
        let reason = windows_guardrail_reason(raw_cmd);
        let translated = translate_windows_shell_command(raw_cmd);
        let translated_applied = translated.is_some();
        if let Some(reason) = reason {
            if translated.is_none() {
                return ShellCommandPlan::Blocked(ToolResult {
                    output: format!(
                        "Shell command blocked on Windows ({reason}). Use cross-platform tools (`read`, `glob`, `grep`) or PowerShell-native syntax."
                    ),
                    metadata: json!({
                        "os_guardrail_applied": true,
                        "guardrail_reason": reason,
                        "blocked": true
                    }),
                });
            }
        }
        let effective = translated.clone().unwrap_or_else(|| raw_cmd.to_string());
        let mut command = Command::new("powershell");
        command.args(["-NoProfile", "-Command", &effective]);
        return ShellCommandPlan::Execute(ShellExecutionPlan {
            command,
            translated_command: translated,
            os_guardrail_applied: reason.is_some() || translated_applied,
            guardrail_reason: reason.map(str::to_string),
        });
    }

    #[allow(unreachable_code)]
    {
        let mut command = Command::new("sh");
        command.args(["-lc", raw_cmd]);
        ShellCommandPlan::Execute(ShellExecutionPlan {
            command,
            translated_command: None,
            os_guardrail_applied: false,
            guardrail_reason: None,
        })
    }
}

fn translate_windows_shell_command(raw_cmd: &str) -> Option<String> {
    let trimmed = raw_cmd.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.starts_with("ls") {
        return translate_windows_ls_command(trimmed);
    }
    if lowered.starts_with("find ") {
        return translate_windows_find_command(trimmed);
    }
    None
}

fn translate_windows_ls_command(trimmed: &str) -> Option<String> {
    let mut force = false;
    let mut paths: Vec<&str> = Vec::new();
    for token in trimmed.split_whitespace().skip(1) {
        if token.starts_with('-') {
            let flags = token.trim_start_matches('-').to_ascii_lowercase();
            if flags.contains('a') {
                force = true;
            }
            continue;
        }
        paths.push(token);
    }

    let mut translated = String::from("Get-ChildItem");
    if force {
        translated.push_str(" -Force");
    }
    if !paths.is_empty() {
        translated.push_str(" -Path ");
        translated.push_str(&quote_powershell_single(&paths.join(" ")));
    }
    Some(translated)
}

fn translate_windows_find_command(trimmed: &str) -> Option<String> {
    let tokens: Vec<&str> = trimmed.split_whitespace().collect();
    if tokens.is_empty() || !tokens[0].eq_ignore_ascii_case("find") {
        return None;
    }

    let mut idx = 1usize;
    let mut path = ".".to_string();
    let mut file_only = false;
    let mut patterns: Vec<String> = Vec::new();

    if idx < tokens.len() && !tokens[idx].starts_with('-') {
        path = normalize_shell_token(tokens[idx]);
        idx += 1;
    }

    while idx < tokens.len() {
        let token = tokens[idx].to_ascii_lowercase();
        match token.as_str() {
            "-type" => {
                if idx + 1 < tokens.len() && tokens[idx + 1].eq_ignore_ascii_case("f") {
                    file_only = true;
                }
                idx += 2;
            }
            "-name" => {
                if idx + 1 < tokens.len() {
                    let pattern = normalize_shell_token(tokens[idx + 1]);
                    if !pattern.is_empty() {
                        patterns.push(pattern);
                    }
                }
                idx += 2;
            }
            "-o" | "-or" | "(" | ")" => {
                idx += 1;
            }
            _ => {
                idx += 1;
            }
        }
    }

    let mut translated = format!("Get-ChildItem -Path {}", quote_powershell_single(&path));
    translated.push_str(" -Recurse");
    if file_only {
        translated.push_str(" -File");
    }

    if patterns.len() == 1 {
        translated.push_str(" -Filter ");
        translated.push_str(&quote_powershell_single(&patterns[0]));
    } else if patterns.len() > 1 {
        translated.push_str(" -Include ");
        let include_list = patterns
            .iter()
            .map(|p| quote_powershell_single(p))
            .collect::<Vec<_>>()
            .join(",");
        translated.push_str(&include_list);
    }

    Some(translated)
}

fn normalize_shell_token(token: &str) -> String {
    let trimmed = token.trim();
    if trimmed.len() >= 2
        && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
            || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
    {
        return trimmed[1..trimmed.len() - 1].to_string();
    }
    trimmed.to_string()
}

fn quote_powershell_single(input: &str) -> String {
    format!("'{}'", input.replace('\'', "''"))
}

fn windows_guardrail_reason(raw_cmd: &str) -> Option<&'static str> {
    let trimmed = raw_cmd.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return None;
    }
    let unix_only_prefixes = [
        "awk ", "sed ", "xargs ", "chmod ", "chown ", "sudo ", "apt ", "apt-get ", "yum ", "dnf ",
        "brew ", "zsh ", "bash ", "sh ", "uname", "pwd",
    ];
    if unix_only_prefixes
        .iter()
        .any(|prefix| trimmed.starts_with(prefix))
    {
        return Some("unix_command_untranslatable");
    }
    if trimmed.contains("/dev/null") || trimmed.contains("~/.") {
        return Some("posix_path_pattern");
    }
    None
}

struct ReadTool;
#[async_trait]
impl Tool for ReadTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "read".to_string(),
            description: "Read file contents".to_string(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string"}
                },
                "required":["path"]
            }),
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
            input_schema: json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string"},
                    "content":{"type":"string"},
                    "allow_empty":{"type":"boolean"}
                },
                "required":["path", "content"]
            }),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let path = args["path"].as_str().unwrap_or("").trim();
        let content = args["content"].as_str();
        let allow_empty = args
            .get("allow_empty")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !is_path_allowed(path) {
            return Ok(ToolResult {
                output: "path denied by sandbox policy".to_string(),
                metadata: json!({"path": path}),
            });
        }
        let Some(content) = content else {
            return Ok(ToolResult {
                output: "write requires `content`".to_string(),
                metadata: json!({"ok": false, "reason": "missing_content", "path": path}),
            });
        };
        if content.is_empty() && !allow_empty {
            return Ok(ToolResult {
                output: "write requires non-empty `content` (or set allow_empty=true)".to_string(),
                metadata: json!({"ok": false, "reason": "empty_content", "path": path}),
            });
        }
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).await?;
            }
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
            input_schema: json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string"},
                    "old":{"type":"string"},
                    "new":{"type":"string"}
                },
                "required":["path", "old", "new"]
            }),
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
            if is_discovery_ignored_path(&path) {
                continue;
            }
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

fn is_discovery_ignored_path(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str() == ".tandem")
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
            if is_discovery_ignored_path(path) {
                continue;
            }
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

struct SpawnAgentTool;
#[async_trait]
impl Tool for SpawnAgentTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "spawn_agent".to_string(),
            description: "Spawn an agent-team instance through server policy enforcement."
                .to_string(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "missionID":{"type":"string"},
                    "parentInstanceID":{"type":"string"},
                    "templateID":{"type":"string"},
                    "role":{"type":"string","enum":["orchestrator","delegator","worker","watcher","reviewer","tester","committer"]},
                    "source":{"type":"string","enum":["tool_call"]},
                    "justification":{"type":"string"},
                    "budgetOverride":{"type":"object"}
                },
                "required":["role","justification"]
            }),
        }
    }

    async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
        Ok(ToolResult {
            output: "spawn_agent must be executed through the engine runtime.".to_string(),
            metadata: json!({
                "ok": false,
                "code": "SPAWN_HOOK_UNAVAILABLE"
            }),
        })
    }
}

struct MemorySearchTool;
#[async_trait]
impl Tool for MemorySearchTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "memory_search".to_string(),
            description: "Search tandem memory across session/project/global tiers. Global scope is opt-in via allow_global=true (or TANDEM_ENABLE_GLOBAL_MEMORY=1).".to_string(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "query":{"type":"string"},
                    "session_id":{"type":"string"},
                    "project_id":{"type":"string"},
                    "tier":{"type":"string","enum":["session","project","global"]},
                    "limit":{"type":"integer","minimum":1,"maximum":20},
                    "allow_global":{"type":"boolean"},
                    "db_path":{"type":"string"}
                },
                "required":["query"]
            }),
        }
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let query = args
            .get("query")
            .or_else(|| args.get("q"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or("");
        if query.is_empty() {
            return Ok(ToolResult {
                output: "memory_search requires a non-empty query".to_string(),
                metadata: json!({"ok": false, "reason": "missing_query"}),
            });
        }

        let session_id = args
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let project_id = args
            .get("project_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let allow_global = global_memory_enabled(&args);
        if session_id.is_none() && project_id.is_none() && !allow_global {
            return Ok(ToolResult {
                output: "memory_search requires at least one scope: session_id or project_id (or allow_global=true)"
                    .to_string(),
                metadata: json!({"ok": false, "reason": "missing_scope"}),
            });
        }

        let tier = match args
            .get("tier")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_ascii_lowercase())
        {
            Some(t) if t == "session" => Some(MemoryTier::Session),
            Some(t) if t == "project" => Some(MemoryTier::Project),
            Some(t) if t == "global" => Some(MemoryTier::Global),
            Some(_) => {
                return Ok(ToolResult {
                    output: "memory_search tier must be one of: session, project, global"
                        .to_string(),
                    metadata: json!({"ok": false, "reason": "invalid_tier"}),
                });
            }
            None => None,
        };
        if matches!(tier, Some(MemoryTier::Session)) && session_id.is_none() {
            return Ok(ToolResult {
                output: "tier=session requires session_id".to_string(),
                metadata: json!({"ok": false, "reason": "missing_session_scope"}),
            });
        }
        if matches!(tier, Some(MemoryTier::Project)) && project_id.is_none() {
            return Ok(ToolResult {
                output: "tier=project requires project_id".to_string(),
                metadata: json!({"ok": false, "reason": "missing_project_scope"}),
            });
        }
        if matches!(tier, Some(MemoryTier::Global)) && !allow_global {
            return Ok(ToolResult {
                output: "tier=global requires allow_global=true".to_string(),
                metadata: json!({"ok": false, "reason": "global_scope_disabled"}),
            });
        }

        let limit = args
            .get("limit")
            .and_then(|v| v.as_i64())
            .unwrap_or(5)
            .clamp(1, 20);

        let db_path = resolve_memory_db_path(&args);
        let db_exists = db_path.exists();
        if !db_exists {
            return Ok(ToolResult {
                output: "memory database not found".to_string(),
                metadata: json!({
                    "ok": false,
                    "reason": "memory_db_missing",
                    "db_path": db_path,
                }),
            });
        }

        let manager = MemoryManager::new(&db_path).await?;
        let health = manager.embedding_health().await;
        if health.status != "ok" {
            return Ok(ToolResult {
                output: "memory embeddings unavailable; semantic search is disabled".to_string(),
                metadata: json!({
                    "ok": false,
                    "reason": "embeddings_unavailable",
                    "embedding_status": health.status,
                    "embedding_reason": health.reason,
                }),
            });
        }

        let mut results: Vec<MemorySearchResult> = Vec::new();
        match tier {
            Some(MemoryTier::Session) => {
                results.extend(
                    manager
                        .search(
                            query,
                            Some(MemoryTier::Session),
                            project_id.as_deref(),
                            session_id.as_deref(),
                            Some(limit),
                        )
                        .await?,
                );
            }
            Some(MemoryTier::Project) => {
                results.extend(
                    manager
                        .search(
                            query,
                            Some(MemoryTier::Project),
                            project_id.as_deref(),
                            session_id.as_deref(),
                            Some(limit),
                        )
                        .await?,
                );
            }
            Some(MemoryTier::Global) => {
                results.extend(
                    manager
                        .search(query, Some(MemoryTier::Global), None, None, Some(limit))
                        .await?,
                );
            }
            _ => {
                if session_id.is_some() {
                    results.extend(
                        manager
                            .search(
                                query,
                                Some(MemoryTier::Session),
                                project_id.as_deref(),
                                session_id.as_deref(),
                                Some(limit),
                            )
                            .await?,
                    );
                }
                if project_id.is_some() {
                    results.extend(
                        manager
                            .search(
                                query,
                                Some(MemoryTier::Project),
                                project_id.as_deref(),
                                session_id.as_deref(),
                                Some(limit),
                            )
                            .await?,
                    );
                }
                if allow_global {
                    results.extend(
                        manager
                            .search(query, Some(MemoryTier::Global), None, None, Some(limit))
                            .await?,
                    );
                }
            }
        }

        let mut dedup: HashMap<String, MemorySearchResult> = HashMap::new();
        for result in results {
            match dedup.get(&result.chunk.id) {
                Some(existing) if existing.similarity >= result.similarity => {}
                _ => {
                    dedup.insert(result.chunk.id.clone(), result);
                }
            }
        }
        let mut merged = dedup.into_values().collect::<Vec<_>>();
        merged.sort_by(|a, b| b.similarity.total_cmp(&a.similarity));
        merged.truncate(limit as usize);

        let output_rows = merged
            .iter()
            .map(|item| {
                json!({
                    "chunk_id": item.chunk.id,
                    "tier": item.chunk.tier.to_string(),
                    "session_id": item.chunk.session_id,
                    "project_id": item.chunk.project_id,
                    "source": item.chunk.source,
                    "similarity": item.similarity,
                    "content": item.chunk.content,
                    "created_at": item.chunk.created_at,
                })
            })
            .collect::<Vec<_>>();

        Ok(ToolResult {
            output: serde_json::to_string_pretty(&output_rows).unwrap_or_default(),
            metadata: json!({
                "ok": true,
                "count": output_rows.len(),
                "limit": limit,
                "query": query,
                "session_id": session_id,
                "project_id": project_id,
                "allow_global": allow_global,
                "embedding_status": health.status,
                "embedding_reason": health.reason,
                "strict_scope": !allow_global,
            }),
        })
    }
}

struct MemoryStoreTool;
#[async_trait]
impl Tool for MemoryStoreTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "memory_store".to_string(),
            description: "Store memory chunks in session/project/global tiers. Global writes are opt-in via allow_global=true (or TANDEM_ENABLE_GLOBAL_MEMORY=1).".to_string(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "content":{"type":"string"},
                    "tier":{"type":"string","enum":["session","project","global"]},
                    "session_id":{"type":"string"},
                    "project_id":{"type":"string"},
                    "source":{"type":"string"},
                    "metadata":{"type":"object"},
                    "allow_global":{"type":"boolean"},
                    "db_path":{"type":"string"}
                },
                "required":["content"]
            }),
        }
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or("");
        if content.is_empty() {
            return Ok(ToolResult {
                output: "memory_store requires non-empty content".to_string(),
                metadata: json!({"ok": false, "reason": "missing_content"}),
            });
        }

        let session_id = args
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let project_id = args
            .get("project_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let allow_global = global_memory_enabled(&args);

        let tier = match args
            .get("tier")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_ascii_lowercase())
        {
            Some(t) if t == "session" => MemoryTier::Session,
            Some(t) if t == "project" => MemoryTier::Project,
            Some(t) if t == "global" => MemoryTier::Global,
            Some(_) => {
                return Ok(ToolResult {
                    output: "memory_store tier must be one of: session, project, global"
                        .to_string(),
                    metadata: json!({"ok": false, "reason": "invalid_tier"}),
                });
            }
            None => {
                if project_id.is_some() {
                    MemoryTier::Project
                } else if session_id.is_some() {
                    MemoryTier::Session
                } else if allow_global {
                    MemoryTier::Global
                } else {
                    return Ok(ToolResult {
                        output: "memory_store requires scope: session_id or project_id (or allow_global=true)"
                            .to_string(),
                        metadata: json!({"ok": false, "reason": "missing_scope"}),
                    });
                }
            }
        };

        if matches!(tier, MemoryTier::Session) && session_id.is_none() {
            return Ok(ToolResult {
                output: "tier=session requires session_id".to_string(),
                metadata: json!({"ok": false, "reason": "missing_session_scope"}),
            });
        }
        if matches!(tier, MemoryTier::Project) && project_id.is_none() {
            return Ok(ToolResult {
                output: "tier=project requires project_id".to_string(),
                metadata: json!({"ok": false, "reason": "missing_project_scope"}),
            });
        }
        if matches!(tier, MemoryTier::Global) && !allow_global {
            return Ok(ToolResult {
                output: "tier=global requires allow_global=true".to_string(),
                metadata: json!({"ok": false, "reason": "global_scope_disabled"}),
            });
        }

        let db_path = resolve_memory_db_path(&args);
        let manager = MemoryManager::new(&db_path).await?;
        let health = manager.embedding_health().await;
        if health.status != "ok" {
            return Ok(ToolResult {
                output: "memory embeddings unavailable; semantic memory store is disabled"
                    .to_string(),
                metadata: json!({
                    "ok": false,
                    "reason": "embeddings_unavailable",
                    "embedding_status": health.status,
                    "embedding_reason": health.reason,
                }),
            });
        }

        let source = args
            .get("source")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("agent_note")
            .to_string();
        let metadata = args.get("metadata").cloned();

        let request = tandem_memory::types::StoreMessageRequest {
            content: content.to_string(),
            tier,
            session_id: session_id.clone(),
            project_id: project_id.clone(),
            source,
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: None,
            metadata,
        };
        let chunk_ids = manager.store_message(request).await?;

        Ok(ToolResult {
            output: format!("stored {} chunk(s) in {} memory", chunk_ids.len(), tier),
            metadata: json!({
                "ok": true,
                "chunk_ids": chunk_ids,
                "count": chunk_ids.len(),
                "tier": tier.to_string(),
                "session_id": session_id,
                "project_id": project_id,
                "allow_global": allow_global,
                "embedding_status": health.status,
                "embedding_reason": health.reason,
                "db_path": db_path,
            }),
        })
    }
}

struct MemoryListTool;
#[async_trait]
impl Tool for MemoryListTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "memory_list".to_string(),
            description: "List stored memory chunks for auditing and knowledge-base browsing."
                .to_string(),
            input_schema: json!({
                "type":"object",
                "properties":{
                    "tier":{"type":"string","enum":["session","project","global","all"]},
                    "session_id":{"type":"string"},
                    "project_id":{"type":"string"},
                    "limit":{"type":"integer","minimum":1,"maximum":200},
                    "allow_global":{"type":"boolean"},
                    "db_path":{"type":"string"}
                }
            }),
        }
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let session_id = args
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let project_id = args
            .get("project_id")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let allow_global = global_memory_enabled(&args);
        let limit = args
            .get("limit")
            .and_then(|v| v.as_i64())
            .unwrap_or(50)
            .clamp(1, 200) as usize;

        let tier = args
            .get("tier")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_else(|| "all".to_string());
        if tier == "global" && !allow_global {
            return Ok(ToolResult {
                output: "tier=global requires allow_global=true".to_string(),
                metadata: json!({"ok": false, "reason": "global_scope_disabled"}),
            });
        }
        if session_id.is_none() && project_id.is_none() && tier != "global" && !allow_global {
            return Ok(ToolResult {
                output: "memory_list requires session_id/project_id, or allow_global=true for global listing".to_string(),
                metadata: json!({"ok": false, "reason": "missing_scope"}),
            });
        }

        let db_path = resolve_memory_db_path(&args);
        let manager = MemoryManager::new(&db_path).await?;

        let mut chunks: Vec<tandem_memory::types::MemoryChunk> = Vec::new();
        match tier.as_str() {
            "session" => {
                let Some(sid) = session_id.as_deref() else {
                    return Ok(ToolResult {
                        output: "tier=session requires session_id".to_string(),
                        metadata: json!({"ok": false, "reason": "missing_session_scope"}),
                    });
                };
                chunks.extend(manager.db().get_session_chunks(sid).await?);
            }
            "project" => {
                let Some(pid) = project_id.as_deref() else {
                    return Ok(ToolResult {
                        output: "tier=project requires project_id".to_string(),
                        metadata: json!({"ok": false, "reason": "missing_project_scope"}),
                    });
                };
                chunks.extend(manager.db().get_project_chunks(pid).await?);
            }
            "global" => {
                chunks.extend(manager.db().get_global_chunks(limit as i64).await?);
            }
            "all" => {
                if let Some(sid) = session_id.as_deref() {
                    chunks.extend(manager.db().get_session_chunks(sid).await?);
                }
                if let Some(pid) = project_id.as_deref() {
                    chunks.extend(manager.db().get_project_chunks(pid).await?);
                }
                if allow_global {
                    chunks.extend(manager.db().get_global_chunks(limit as i64).await?);
                }
            }
            _ => {
                return Ok(ToolResult {
                    output: "memory_list tier must be one of: session, project, global, all"
                        .to_string(),
                    metadata: json!({"ok": false, "reason": "invalid_tier"}),
                });
            }
        }

        chunks.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        chunks.truncate(limit);
        let rows = chunks
            .iter()
            .map(|chunk| {
                json!({
                    "chunk_id": chunk.id,
                    "tier": chunk.tier.to_string(),
                    "session_id": chunk.session_id,
                    "project_id": chunk.project_id,
                    "source": chunk.source,
                    "content": chunk.content,
                    "created_at": chunk.created_at,
                    "metadata": chunk.metadata,
                })
            })
            .collect::<Vec<_>>();

        Ok(ToolResult {
            output: serde_json::to_string_pretty(&rows).unwrap_or_default(),
            metadata: json!({
                "ok": true,
                "count": rows.len(),
                "limit": limit,
                "tier": tier,
                "session_id": session_id,
                "project_id": project_id,
                "allow_global": allow_global,
                "db_path": db_path,
            }),
        })
    }
}

fn resolve_memory_db_path(args: &Value) -> PathBuf {
    if let Some(path) = args
        .get("db_path")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return PathBuf::from(path);
    }
    if let Ok(path) = std::env::var("TANDEM_MEMORY_DB_PATH") {
        let trimmed = path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    PathBuf::from("memory.sqlite")
}

fn global_memory_enabled(args: &Value) -> bool {
    if args
        .get("allow_global")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return true;
    }
    let Ok(raw) = std::env::var("TANDEM_ENABLE_GLOBAL_MEMORY") else {
        return false;
    };
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

struct SkillTool;
#[async_trait]
impl Tool for SkillTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema {
            name: "skill".to_string(),
            description: "List or load installed Tandem skills. Call without name to list available skills; provide name to load full SKILL.md content.".to_string(),
            input_schema: json!({"type":"object","properties":{"name":{"type":"string"}}}),
        }
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let workspace_root = std::env::current_dir().ok();
        let service = SkillService::for_workspace(workspace_root);
        let requested = args["name"].as_str().map(str::trim).unwrap_or("");
        let allowed_skills = parse_allowed_skills(&args);

        if requested.is_empty() {
            let mut skills = service.list_skills().unwrap_or_default();
            if let Some(allowed) = &allowed_skills {
                skills.retain(|s| allowed.contains(&s.name));
            }
            if skills.is_empty() {
                return Ok(ToolResult {
                    output: "No skills available.".to_string(),
                    metadata: json!({"count": 0, "skills": []}),
                });
            }
            let mut lines = vec![
                "Available Tandem skills:".to_string(),
                "<available_skills>".to_string(),
            ];
            for skill in &skills {
                lines.push("  <skill>".to_string());
                lines.push(format!("    <name>{}</name>", skill.name));
                lines.push(format!(
                    "    <description>{}</description>",
                    escape_xml_text(&skill.description)
                ));
                lines.push(format!("    <location>{}</location>", skill.path));
                lines.push("  </skill>".to_string());
            }
            lines.push("</available_skills>".to_string());
            return Ok(ToolResult {
                output: lines.join("\n"),
                metadata: json!({"count": skills.len(), "skills": skills}),
            });
        }

        if let Some(allowed) = &allowed_skills {
            if !allowed.contains(requested) {
                let mut allowed_list = allowed.iter().cloned().collect::<Vec<_>>();
                allowed_list.sort();
                return Ok(ToolResult {
                    output: format!(
                        "Skill \"{}\" is not enabled for this agent. Enabled skills: {}",
                        requested,
                        allowed_list.join(", ")
                    ),
                    metadata: json!({"name": requested, "enabled": allowed_list}),
                });
            }
        }

        let loaded = service.load_skill(requested).map_err(anyhow::Error::msg)?;
        let Some(skill) = loaded else {
            let available = service
                .list_skills()
                .unwrap_or_default()
                .into_iter()
                .map(|s| s.name)
                .collect::<Vec<_>>();
            return Ok(ToolResult {
                output: format!(
                    "Skill \"{}\" not found. Available skills: {}",
                    requested,
                    if available.is_empty() {
                        "none".to_string()
                    } else {
                        available.join(", ")
                    }
                ),
                metadata: json!({"name": requested, "matches": [], "available": available}),
            });
        };

        let files = skill
            .files
            .iter()
            .map(|f| format!("<file>{}</file>", f))
            .collect::<Vec<_>>()
            .join("\n");
        let output = [
            format!("<skill_content name=\"{}\">", skill.info.name),
            format!("# Skill: {}", skill.info.name),
            String::new(),
            skill.content.trim().to_string(),
            String::new(),
            format!("Base directory for this skill: {}", skill.base_dir),
            "Relative paths in this skill are resolved from this base directory.".to_string(),
            "Note: file list is sampled.".to_string(),
            String::new(),
            "<skill_files>".to_string(),
            files,
            "</skill_files>".to_string(),
            "</skill_content>".to_string(),
        ]
        .join("\n");
        Ok(ToolResult {
            output,
            metadata: json!({
                "name": skill.info.name,
                "dir": skill.base_dir,
                "path": skill.info.path
            }),
        })
    }
}

fn escape_xml_text(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn parse_allowed_skills(args: &Value) -> Option<HashSet<String>> {
    let values = args
        .get("allowed_skills")
        .or_else(|| args.get("allowedSkills"))
        .and_then(|v| v.as_array())?;
    let out = values
        .iter()
        .filter_map(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect::<HashSet<_>>();
    Some(out)
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
            let Some(tool) = resolve_batch_call_tool_name(call) else {
                continue;
            };
            if tool.is_empty() || tool == "batch" {
                continue;
            }
            let call_args = call.get("args").cloned().unwrap_or_else(|| json!({}));
            let mut result = registry.execute(&tool, call_args.clone()).await?;
            if result.output.starts_with("Unknown tool:") {
                if let Some(fallback_name) = call
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty() && *s != tool)
                {
                    result = registry.execute(fallback_name, call_args).await?;
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

    #[test]
    fn test_html_stripping_and_markdown_reduction() {
        let html = r#"
            <!DOCTYPE html>
            <html>
            <head>
                <title>Test Page</title>
                <style>
                    body { color: red; }
                </style>
                <script>
                    console.log("noisy script");
                </script>
            </head>
            <body>
                <h1>Hello World</h1>
                <p>This is a <a href="https://example.com">link</a>.</p>
                <noscript>Enable JS</noscript>
            </body>
            </html>
        "#;

        let cleaned = strip_html_noise(html);
        assert!(!cleaned.contains("noisy script"));
        assert!(!cleaned.contains("color: red"));
        assert!(!cleaned.contains("Enable JS"));
        assert!(cleaned.contains("Hello World"));

        let markdown = html2md::parse_html(&cleaned);
        let text = markdown_to_text(&markdown);

        // Raw length includes all the noise
        let raw_len = html.len();
        // Markdown length should be significantly smaller
        let md_len = markdown.len();

        println!("Raw: {}, Markdown: {}", raw_len, md_len);
        assert!(
            md_len < raw_len / 2,
            "Markdown should be < 50% of raw HTML size"
        );
        assert!(text.contains("Hello World"));
        assert!(text.contains("link"));
    }

    #[tokio::test]
    async fn memory_search_requires_scope() {
        let tool = MemorySearchTool;
        let result = tool
            .execute(json!({"query": "deployment strategy"}))
            .await
            .expect("memory_search should return ToolResult");
        assert!(result.output.contains("requires at least one scope"));
        assert_eq!(result.metadata["ok"], json!(false));
        assert_eq!(result.metadata["reason"], json!("missing_scope"));
    }

    #[tokio::test]
    async fn memory_search_global_requires_opt_in() {
        let tool = MemorySearchTool;
        let result = tool
            .execute(json!({
                "query": "deployment strategy",
                "session_id": "ses_1",
                "tier": "global"
            }))
            .await
            .expect("memory_search should return ToolResult");
        assert!(result.output.contains("requires allow_global=true"));
        assert_eq!(result.metadata["ok"], json!(false));
        assert_eq!(result.metadata["reason"], json!("global_scope_disabled"));
    }

    #[tokio::test]
    async fn memory_store_global_requires_opt_in() {
        let tool = MemoryStoreTool;
        let result = tool
            .execute(json!({
                "content": "global pattern",
                "tier": "global"
            }))
            .await
            .expect("memory_store should return ToolResult");
        assert!(result.output.contains("requires allow_global=true"));
        assert_eq!(result.metadata["ok"], json!(false));
        assert_eq!(result.metadata["reason"], json!("global_scope_disabled"));
    }

    #[test]
    fn translate_windows_ls_with_all_flag() {
        let translated = translate_windows_shell_command("ls -la").expect("translation");
        assert!(translated.contains("Get-ChildItem"));
        assert!(translated.contains("-Force"));
    }

    #[test]
    fn translate_windows_find_name_pattern() {
        let translated =
            translate_windows_shell_command("find . -type f -name \"*.rs\"").expect("translation");
        assert!(translated.contains("Get-ChildItem"));
        assert!(translated.contains("-Recurse"));
        assert!(translated.contains("-Filter"));
    }

    #[test]
    fn windows_guardrail_blocks_untranslatable_unix_command() {
        assert_eq!(
            windows_guardrail_reason("sed -n '1,5p' README.md"),
            Some("unix_command_untranslatable")
        );
    }

    #[test]
    fn path_policy_rejects_tool_markup_and_globs() {
        assert!(!is_path_allowed(
            "<tool_call><function=glob><parameter=pattern>**/*</parameter></function></tool_call>"
        ));
        assert!(!is_path_allowed("**/*"));
        assert!(!is_path_allowed("/"));
        assert!(!is_path_allowed("C:\\"));
    }

    #[tokio::test]
    async fn write_tool_rejects_empty_content_by_default() {
        let tool = WriteTool;
        let result = tool
            .execute(json!({
                "path":"target/write_guard_test.txt",
                "content":""
            }))
            .await
            .expect("write tool should return ToolResult");
        assert!(result.output.contains("non-empty `content`"));
        assert_eq!(result.metadata["reason"], json!("empty_content"));
        assert!(!Path::new("target/write_guard_test.txt").exists());
    }

    #[tokio::test]
    async fn registry_resolves_default_api_namespaced_tool() {
        let registry = ToolRegistry::new();
        let result = registry
            .execute("default_api:read", json!({"path":"Cargo.toml"}))
            .await
            .expect("registry execute should return ToolResult");
        assert!(!result.output.starts_with("Unknown tool:"));
    }

    #[tokio::test]
    async fn batch_resolves_default_api_namespaced_tool() {
        let tool = BatchTool;
        let result = tool
            .execute(json!({
                "tool_calls":[
                    {"tool":"default_api:read","args":{"path":"Cargo.toml"}}
                ]
            }))
            .await
            .expect("batch should return ToolResult");
        assert!(!result.output.contains("Unknown tool: default_api:read"));
    }

    #[tokio::test]
    async fn batch_prefers_name_when_tool_is_default_api_wrapper() {
        let tool = BatchTool;
        let result = tool
            .execute(json!({
                "tool_calls":[
                    {"tool":"default_api","name":"read","args":{"path":"Cargo.toml"}}
                ]
            }))
            .await
            .expect("batch should return ToolResult");
        assert!(!result.output.contains("Unknown tool: default_api"));
    }

    #[tokio::test]
    async fn batch_resolves_nested_function_name_for_wrapper_tool() {
        let tool = BatchTool;
        let result = tool
            .execute(json!({
                "tool_calls":[
                    {
                        "tool":"default_api",
                        "function":{"name":"read"},
                        "args":{"path":"Cargo.toml"}
                    }
                ]
            }))
            .await
            .expect("batch should return ToolResult");
        assert!(!result.output.contains("Unknown tool: default_api"));
    }

    #[tokio::test]
    async fn batch_drops_wrapper_calls_without_resolvable_name() {
        let tool = BatchTool;
        let result = tool
            .execute(json!({
                "tool_calls":[
                    {"tool":"default_api","args":{"path":"Cargo.toml"}}
                ]
            }))
            .await
            .expect("batch should return ToolResult");
        assert_eq!(result.metadata["count"], json!(0));
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
