use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use async_trait::async_trait;
use grep_matcher::LineTerminator;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::sinks::Lossy;
use grep_searcher::{BinaryDetection, MmapChoice, SearcherBuilder};
use ignore::WalkBuilder;
use regex::Regex;
use serde_json::{json, Value};
use tandem_memory::embeddings::{get_embedding_service, EmbeddingService};
use tandem_skills::SkillService;
use tokio::fs;
use tokio::process::Command;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use futures_util::StreamExt;
use tandem_agent_teams::compat::{
    send_message_schema, task_create_schema, task_list_schema, task_schema, task_update_schema,
    team_create_schema,
};
use tandem_agent_teams::{
    AgentTeamPaths, SendMessageInput, SendMessageType, TaskCreateInput, TaskInput, TaskListInput,
    TaskUpdateInput, TeamCreateInput,
};
use tandem_memory::types::{MemorySearchResult, MemoryTier};
use tandem_memory::MemoryManager;
use tandem_types::{ToolResult, ToolSchema};

mod builtin_tools;
mod tool_metadata;
use builtin_tools::*;
use tool_metadata::*;

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
    tool_vectors: Arc<RwLock<HashMap<String, Vec<f32>>>>,
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
        map.insert("webfetch_html".to_string(), Arc::new(WebFetchHtmlTool));
        map.insert("mcp_debug".to_string(), Arc::new(McpDebugTool));
        let search_backend = SearchBackend::from_env();
        if search_backend.is_enabled() {
            map.insert(
                "websearch".to_string(),
                Arc::new(WebSearchTool {
                    backend: search_backend,
                }),
            );
        } else {
            tracing::info!(
                reason = search_backend.disabled_reason().unwrap_or("unknown"),
                "builtin websearch disabled because no search backend is configured"
            );
        }
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
        map.insert("memory_delete".to_string(), Arc::new(MemoryDeleteTool));
        map.insert("apply_patch".to_string(), Arc::new(ApplyPatchTool));
        map.insert("batch".to_string(), Arc::new(BatchTool));
        map.insert("lsp".to_string(), Arc::new(LspTool));
        map.insert("teamcreate".to_string(), Arc::new(TeamCreateTool));
        map.insert("taskcreate".to_string(), Arc::new(TaskCreateCompatTool));
        map.insert("taskupdate".to_string(), Arc::new(TaskUpdateCompatTool));
        map.insert("tasklist".to_string(), Arc::new(TaskListCompatTool));
        map.insert("sendmessage".to_string(), Arc::new(SendMessageCompatTool));
        Self {
            tools: Arc::new(RwLock::new(map)),
            tool_vectors: Arc::new(RwLock::new(HashMap::new())),
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

    pub async fn register_tool(&self, name: String, tool: Arc<dyn Tool>) {
        let schema = tool.schema();
        self.tools.write().await.insert(name.clone(), tool);
        self.index_tool_schema(&schema).await;
        if name != schema.name {
            self.index_tool_name(&name, &schema).await;
        }
    }

    pub async fn unregister_tool(&self, name: &str) -> bool {
        let removed = self.tools.write().await.remove(name);
        self.tool_vectors.write().await.remove(name);
        if let Some(tool) = removed {
            let schema_name = tool.schema().name;
            self.tool_vectors.write().await.remove(&schema_name);
            return true;
        }
        false
    }

    pub async fn unregister_by_prefix(&self, prefix: &str) -> usize {
        let mut tools = self.tools.write().await;
        let keys = tools
            .keys()
            .filter(|name| name.starts_with(prefix))
            .cloned()
            .collect::<Vec<_>>();
        let removed = keys.len();
        let mut removed_schema_names = Vec::new();
        for key in keys {
            if let Some(tool) = tools.remove(&key) {
                removed_schema_names.push(tool.schema().name);
            }
        }
        drop(tools);
        let mut vectors = self.tool_vectors.write().await;
        vectors.retain(|name, _| {
            !name.starts_with(prefix) && !removed_schema_names.iter().any(|schema| schema == name)
        });
        removed
    }

    pub async fn index_all(&self) {
        let schemas = self.list().await;
        if schemas.is_empty() {
            self.tool_vectors.write().await.clear();
            return;
        }
        let texts = schemas
            .iter()
            .map(|schema| format!("{}: {}", schema.name, schema.description))
            .collect::<Vec<_>>();
        let service = get_embedding_service().await;
        let service = service.lock().await;
        if !service.is_available() {
            return;
        }
        let Ok(vectors) = service.embed_batch(&texts).await else {
            return;
        };
        drop(service);
        let mut indexed = HashMap::new();
        for (schema, vector) in schemas.into_iter().zip(vectors) {
            indexed.insert(schema.name, vector);
        }
        *self.tool_vectors.write().await = indexed;
    }

    async fn index_tool_schema(&self, schema: &ToolSchema) {
        self.index_tool_name(&schema.name, schema).await;
    }

    async fn index_tool_name(&self, name: &str, schema: &ToolSchema) {
        let text = format!("{}: {}", schema.name, schema.description);
        let service = get_embedding_service().await;
        let service = service.lock().await;
        if !service.is_available() {
            return;
        }
        let Ok(vector) = service.embed(&text).await else {
            return;
        };
        drop(service);
        self.tool_vectors
            .write()
            .await
            .insert(name.to_string(), vector);
    }

    pub async fn retrieve(&self, query: &str, k: usize) -> Vec<ToolSchema> {
        if k == 0 {
            return Vec::new();
        }
        let service = get_embedding_service().await;
        let service = service.lock().await;
        if !service.is_available() {
            drop(service);
            return self.list().await;
        }
        let Ok(query_vec) = service.embed(query).await else {
            drop(service);
            return self.list().await;
        };
        drop(service);

        let vectors = self.tool_vectors.read().await;
        if vectors.is_empty() {
            drop(vectors);
            return self.list().await;
        }
        let tools = self.tools.read().await;
        let mut scored = vectors
            .iter()
            .map(|(name, vector)| {
                (
                    EmbeddingService::cosine_similarity(&query_vec, vector),
                    name.clone(),
                )
            })
            .collect::<Vec<_>>();
        scored.sort_by(|a, b| {
            b.0.partial_cmp(&a.0)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.1.cmp(&b.1))
        });
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for (_, name) in scored.into_iter().take(k) {
            let Some(tool) = tools.get(&name) else {
                continue;
            };
            let schema = tool.schema();
            if seen.insert(schema.name.clone()) {
                out.push(schema);
            }
        }
        if out.is_empty() {
            self.list().await
        } else {
            out
        }
    }

    pub async fn mcp_server_names(&self) -> Vec<String> {
        let mut names = HashSet::new();
        for schema in self.list().await {
            let mut parts = schema.name.split('.');
            if parts.next() == Some("mcp") {
                if let Some(server) = parts.next() {
                    if !server.trim().is_empty() {
                        names.insert(server.to_string());
                    }
                }
            }
        }
        let mut sorted = names.into_iter().collect::<Vec<_>>();
        sorted.sort();
        sorted
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum SearchBackendKind {
    Disabled,
    Auto,
    Tandem,
    Searxng,
    Exa,
    Brave,
}

#[derive(Clone, Debug)]
enum SearchBackend {
    Disabled {
        reason: String,
    },
    Auto {
        backends: Vec<SearchBackend>,
    },
    Tandem {
        base_url: String,
        timeout_ms: u64,
    },
    Searxng {
        base_url: String,
        engines: Option<String>,
        timeout_ms: u64,
    },
    Exa {
        api_key: String,
        timeout_ms: u64,
    },
    Brave {
        api_key: String,
        timeout_ms: u64,
    },
}

impl SearchBackend {
    fn from_env() -> Self {
        let explicit = std::env::var("TANDEM_SEARCH_BACKEND")
            .ok()
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty());
        let timeout_ms = search_backend_timeout_ms();

        match explicit.as_deref() {
            Some("none") | Some("disabled") => {
                return Self::Disabled {
                    reason: "TANDEM_SEARCH_BACKEND explicitly disabled websearch".to_string(),
                };
            }
            Some("auto") => {
                return search_backend_from_auto_env(timeout_ms);
            }
            Some("tandem") => {
                return search_backend_from_tandem_env(timeout_ms, true);
            }
            Some("searxng") => {
                return search_backend_from_searxng_env(timeout_ms).unwrap_or_else(|| {
                    Self::Disabled {
                        reason: "TANDEM_SEARCH_BACKEND=searxng but TANDEM_SEARXNG_URL is missing"
                            .to_string(),
                    }
                });
            }
            Some("exa") => {
                return search_backend_from_exa_env(timeout_ms).unwrap_or_else(|| Self::Disabled {
                    reason:
                        "TANDEM_SEARCH_BACKEND=exa but EXA_API_KEY/TANDEM_EXA_API_KEY is missing"
                            .to_string(),
                });
            }
            Some("brave") => {
                return search_backend_from_brave_env(timeout_ms).unwrap_or_else(|| {
                    Self::Disabled {
                        reason:
                            "TANDEM_SEARCH_BACKEND=brave but BRAVE_SEARCH_API_KEY/TANDEM_BRAVE_SEARCH_API_KEY is missing"
                                .to_string(),
                    }
                });
            }
            Some(other) => {
                return Self::Disabled {
                    reason: format!(
                        "TANDEM_SEARCH_BACKEND `{other}` is unsupported; expected auto, tandem, searxng, exa, brave, or none"
                    ),
                };
            }
            None => {}
        }
        search_backend_from_auto_env(timeout_ms)
    }

    fn is_enabled(&self) -> bool {
        !matches!(self, Self::Disabled { .. })
    }

    fn kind(&self) -> SearchBackendKind {
        match self {
            Self::Disabled { .. } => SearchBackendKind::Disabled,
            Self::Auto { .. } => SearchBackendKind::Auto,
            Self::Tandem { .. } => SearchBackendKind::Tandem,
            Self::Searxng { .. } => SearchBackendKind::Searxng,
            Self::Exa { .. } => SearchBackendKind::Exa,
            Self::Brave { .. } => SearchBackendKind::Brave,
        }
    }

    fn name(&self) -> &'static str {
        match self.kind() {
            SearchBackendKind::Disabled => "disabled",
            SearchBackendKind::Auto => "auto",
            SearchBackendKind::Tandem => "tandem",
            SearchBackendKind::Searxng => "searxng",
            SearchBackendKind::Exa => "exa",
            SearchBackendKind::Brave => "brave",
        }
    }

    fn disabled_reason(&self) -> Option<&str> {
        match self {
            Self::Disabled { reason } => Some(reason.as_str()),
            _ => None,
        }
    }

    fn schema_description(&self) -> String {
        match self {
            Self::Auto { .. } => {
                "Search web results using the configured search backends with automatic failover"
                    .to_string()
            }
            Self::Tandem { .. } => {
                "Search web results using Tandem's hosted search backend".to_string()
            }
            Self::Searxng { .. } => {
                "Search web results using the configured SearxNG backend".to_string()
            }
            Self::Exa { .. } => "Search web results using the configured Exa backend".to_string(),
            Self::Brave { .. } => {
                "Search web results using the configured Brave Search backend".to_string()
            }
            Self::Disabled { .. } => {
                "Search web results using the configured search backend".to_string()
            }
        }
    }
}

fn has_nonempty_env_var(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn search_backend_timeout_ms() -> u64 {
    std::env::var("TANDEM_SEARCH_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(10_000)
        .clamp(1_000, 120_000)
}

fn search_backend_from_tandem_env(timeout_ms: u64, allow_default_url: bool) -> SearchBackend {
    const DEFAULT_TANDEM_SEARCH_URL: &str = "https://search.tandem.ac";
    let base_url = std::env::var("TANDEM_SEARCH_URL")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| allow_default_url.then(|| DEFAULT_TANDEM_SEARCH_URL.to_string()));
    match base_url {
        Some(base_url) => SearchBackend::Tandem {
            base_url,
            timeout_ms,
        },
        None => SearchBackend::Disabled {
            reason: "TANDEM_SEARCH_BACKEND=tandem but TANDEM_SEARCH_URL is missing".to_string(),
        },
    }
}

fn search_backend_from_searxng_env(timeout_ms: u64) -> Option<SearchBackend> {
    let base_url = std::env::var("TANDEM_SEARXNG_URL").ok()?;
    let base_url = base_url.trim().trim_end_matches('/').to_string();
    if base_url.is_empty() {
        return None;
    }
    let engines = std::env::var("TANDEM_SEARXNG_ENGINES")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    Some(SearchBackend::Searxng {
        base_url,
        engines,
        timeout_ms,
    })
}

fn search_backend_from_exa_env(timeout_ms: u64) -> Option<SearchBackend> {
    let api_key = std::env::var("TANDEM_EXA_API_KEY")
        .ok()
        .or_else(|| std::env::var("TANDEM_EXA_SEARCH_API_KEY").ok())
        .or_else(|| std::env::var("EXA_API_KEY").ok())?;
    let api_key = api_key.trim().to_string();
    if api_key.is_empty() {
        return None;
    }
    Some(SearchBackend::Exa {
        api_key,
        timeout_ms,
    })
}

fn search_backend_from_brave_env(timeout_ms: u64) -> Option<SearchBackend> {
    let api_key = std::env::var("TANDEM_BRAVE_SEARCH_API_KEY")
        .ok()
        .or_else(|| std::env::var("BRAVE_SEARCH_API_KEY").ok())?;
    let api_key = api_key.trim().to_string();
    if api_key.is_empty() {
        return None;
    }
    Some(SearchBackend::Brave {
        api_key,
        timeout_ms,
    })
}

fn search_backend_auto_candidates(timeout_ms: u64) -> Vec<SearchBackend> {
    let mut backends = Vec::new();

    if has_nonempty_env_var("TANDEM_SEARCH_URL") {
        backends.push(search_backend_from_tandem_env(timeout_ms, false));
    }
    if let Some(config) = search_backend_from_searxng_env(timeout_ms) {
        backends.push(config);
    }
    if let Some(config) = search_backend_from_brave_env(timeout_ms) {
        backends.push(config);
    }
    if let Some(config) = search_backend_from_exa_env(timeout_ms) {
        backends.push(config);
    }
    if backends.is_empty() {
        backends.push(search_backend_from_tandem_env(timeout_ms, true));
    }

    backends
        .into_iter()
        .filter(|backend| !matches!(backend, SearchBackend::Disabled { .. }))
        .collect()
}

fn search_backend_from_auto_env(timeout_ms: u64) -> SearchBackend {
    let backends = search_backend_auto_candidates(timeout_ms);
    match backends.len() {
        0 => SearchBackend::Disabled {
            reason:
                "set TANDEM_SEARCH_URL or configure tandem, searxng, brave, or exa to enable websearch"
                    .to_string(),
        },
        1 => backends.into_iter().next().expect("single backend"),
        _ => SearchBackend::Auto { backends },
    }
}

#[derive(Clone, Debug, serde::Serialize)]
struct SearchResultEntry {
    title: String,
    url: String,
    snippet: String,
    source: String,
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

fn non_empty_batch_str(value: Option<&Value>) -> Option<&str> {
    trimmed_non_empty_str(value)
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

fn workspace_root_from_args(args: &Value) -> Option<PathBuf> {
    args.get("__workspace_root")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

fn effective_cwd_from_args(args: &Value) -> PathBuf {
    args.get("__effective_cwd")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .or_else(|| workspace_root_from_args(args))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

fn normalize_path_for_compare(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                let _ = normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn normalize_existing_or_lexical(path: &Path) -> PathBuf {
    path.canonicalize()
        .unwrap_or_else(|_| normalize_path_for_compare(path))
}

fn is_within_workspace_root(path: &Path, workspace_root: &Path) -> bool {
    // First compare lexical-normalized paths so non-existent target files under symlinked
    // workspace roots still pass containment checks.
    let candidate_lexical = normalize_path_for_compare(path);
    let root_lexical = normalize_path_for_compare(workspace_root);
    if candidate_lexical.starts_with(&root_lexical) {
        return true;
    }

    // Fallback to canonical comparison when available (best for existing paths and symlink
    // resolution consistency).
    let candidate = normalize_existing_or_lexical(path);
    let root = normalize_existing_or_lexical(workspace_root);
    candidate.starts_with(root)
}

fn resolve_tool_path(path: &str, args: &Value) -> Option<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed == "." || trimmed == "./" || trimmed == ".\\" {
        let cwd = effective_cwd_from_args(args);
        if let Some(workspace_root) = workspace_root_from_args(args) {
            if !is_within_workspace_root(&cwd, &workspace_root) {
                return None;
            }
        }
        return Some(cwd);
    }
    if is_root_only_path_token(trimmed) || is_malformed_tool_path_token(trimmed) {
        return None;
    }
    let raw = Path::new(trimmed);
    if !raw.is_absolute()
        && raw
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return None;
    }

    let resolved = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        effective_cwd_from_args(args).join(raw)
    };

    if let Some(workspace_root) = workspace_root_from_args(args) {
        if !is_within_workspace_root(&resolved, &workspace_root) {
            return None;
        }
    } else if raw.is_absolute() {
        return None;
    }

    Some(resolved)
}

fn resolve_walk_root(path: &str, args: &Value) -> Option<PathBuf> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return None;
    }
    if is_malformed_tool_path_token(trimmed) {
        return None;
    }
    resolve_tool_path(path, args)
}

fn resolve_read_path_fallback(path: &str, args: &Value) -> Option<PathBuf> {
    let token = path.trim();
    if token.is_empty() {
        return None;
    }
    let raw = Path::new(token);
    if raw.is_absolute() || token.contains('\\') || token.contains('/') || raw.extension().is_none()
    {
        return None;
    }

    let workspace_root = workspace_root_from_args(args);
    let effective_cwd = effective_cwd_from_args(args);
    let mut search_roots = vec![effective_cwd.clone()];
    if let Some(root) = workspace_root.as_ref() {
        if *root != effective_cwd {
            search_roots.push(root.clone());
        }
    }

    let token_lower = token.to_lowercase();
    for root in search_roots {
        if let Some(workspace_root) = workspace_root.as_ref() {
            if !is_within_workspace_root(&root, workspace_root) {
                continue;
            }
        }

        let mut matches = Vec::new();
        for entry in WalkBuilder::new(&root).build().flatten() {
            if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                continue;
            }
            let candidate = entry.path();
            if let Some(workspace_root) = workspace_root.as_ref() {
                if !is_within_workspace_root(candidate, workspace_root) {
                    continue;
                }
            }
            let file_name = candidate
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_lowercase();
            if file_name == token_lower || file_name.ends_with(&token_lower) {
                matches.push(candidate.to_path_buf());
                if matches.len() > 8 {
                    break;
                }
            }
        }

        if matches.len() == 1 {
            return matches.into_iter().next();
        }
    }

    None
}

fn sandbox_path_denied_result(path: &str, args: &Value) -> ToolResult {
    let requested = path.trim();
    let workspace_root = workspace_root_from_args(args);
    let effective_cwd = effective_cwd_from_args(args);
    let suggested_path = Path::new(requested)
        .file_name()
        .filter(|name| !name.is_empty())
        .map(PathBuf::from)
        .map(|name| {
            if let Some(root) = workspace_root.as_ref() {
                if is_within_workspace_root(&effective_cwd, root) {
                    effective_cwd.join(name)
                } else {
                    root.join(name)
                }
            } else {
                effective_cwd.join(name)
            }
        });

    let mut output =
        "path denied by sandbox policy (outside workspace root, malformed path, or missing workspace context)"
            .to_string();
    if let Some(suggested) = suggested_path.as_ref() {
        output.push_str(&format!(
            "\nrequested: {}\ntry: {}",
            requested,
            suggested.to_string_lossy()
        ));
    }
    if let Some(root) = workspace_root.as_ref() {
        output.push_str(&format!("\nworkspace_root: {}", root.to_string_lossy()));
    }

    ToolResult {
        output,
        metadata: json!({
            "path": path,
            "workspace_root": workspace_root.map(|p| p.to_string_lossy().to_string()),
            "effective_cwd": effective_cwd.to_string_lossy().to_string(),
            "suggested_path": suggested_path.map(|p| p.to_string_lossy().to_string())
        }),
    }
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
    if path.contains('*') {
        return true;
    }
    // Allow Windows verbatim prefixes (\\?\C:\... / //?/C:/... / \\?\UNC\...).
    // These can appear in tool outputs and should not be treated as malformed.
    if path.contains('?') {
        let trimmed = path.trim();
        let is_windows_verbatim = trimmed.starts_with("\\\\?\\") || trimmed.starts_with("//?/");
        if !is_windows_verbatim {
            return true;
        }
    }
    false
}

fn is_malformed_tool_pattern_token(pattern: &str) -> bool {
    let lower = pattern.to_ascii_lowercase();
    if lower.contains("<tool_call")
        || lower.contains("</tool_call")
        || lower.contains("<function=")
        || lower.contains("<parameter=")
        || lower.contains("</function>")
        || lower.contains("</parameter>")
    {
        return true;
    }
    if pattern.contains('\n') || pattern.contains('\r') {
        return true;
    }
    false
}

// Builtin shell/read tool implementations live in `builtin_tools`.

struct WriteTool;
#[async_trait]
impl Tool for WriteTool {
    fn schema(&self) -> ToolSchema {
        tool_schema_with_capabilities(
            "write",
            "Write file contents",
            json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string"},
                    "content":{"type":"string"},
                    "allow_empty":{"type":"boolean"}
                },
                "required":["path", "content"]
            }),
            workspace_write_capabilities(),
        )
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let path = args["path"].as_str().unwrap_or("").trim();
        let content = args["content"].as_str();
        let allow_empty = args
            .get("allow_empty")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let Some(path_buf) = resolve_tool_path(path, &args) else {
            return Ok(sandbox_path_denied_result(path, &args));
        };
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
        if let Some(parent) = path_buf.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).await?;
            }
        }
        fs::write(&path_buf, content).await?;
        Ok(ToolResult {
            output: "ok".to_string(),
            metadata: json!({"path": path_buf.to_string_lossy()}),
        })
    }
}

struct EditTool;
#[async_trait]
impl Tool for EditTool {
    fn schema(&self) -> ToolSchema {
        tool_schema_with_capabilities(
            "edit",
            "String replacement edit",
            json!({
                "type":"object",
                "properties":{
                    "path":{"type":"string"},
                    "old":{"type":"string"},
                    "new":{"type":"string"}
                },
                "required":["path", "old", "new"]
            }),
            workspace_write_capabilities(),
        )
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let path = args["path"].as_str().unwrap_or("");
        let old = args["old"].as_str().unwrap_or("");
        let new = args["new"].as_str().unwrap_or("");
        let Some(path_buf) = resolve_tool_path(path, &args) else {
            return Ok(sandbox_path_denied_result(path, &args));
        };
        let content = fs::read_to_string(&path_buf).await.unwrap_or_default();
        let updated = content.replace(old, new);
        fs::write(&path_buf, updated).await?;
        Ok(ToolResult {
            output: "ok".to_string(),
            metadata: json!({"path": path_buf.to_string_lossy()}),
        })
    }
}

struct GlobTool;

fn normalize_recursive_wildcard_pattern(pattern: &str) -> Option<String> {
    let mut changed = false;
    let normalized = pattern
        .split('/')
        .flat_map(|component| {
            if let Some(tail) = component.strip_prefix("**") {
                if !tail.is_empty() {
                    changed = true;
                    let normalized_tail = if tail.starts_with('.') || tail.starts_with('{') {
                        format!("*{tail}")
                    } else {
                        tail.to_string()
                    };
                    return vec!["**".to_string(), normalized_tail];
                }
            }
            vec![component.to_string()]
        })
        .collect::<Vec<_>>()
        .join("/");
    changed.then_some(normalized)
}

#[async_trait]
impl Tool for GlobTool {
    fn schema(&self) -> ToolSchema {
        tool_schema_with_capabilities(
            "glob",
            "Find files by glob",
            json!({"type":"object","properties":{"pattern":{"type":"string"}}}),
            workspace_search_capabilities(),
        )
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let pattern = args["pattern"].as_str().unwrap_or("*");
        if pattern.contains("..") {
            return Ok(ToolResult {
                output: "pattern denied by sandbox policy".to_string(),
                metadata: json!({"pattern": pattern}),
            });
        }
        if is_malformed_tool_pattern_token(pattern) {
            return Ok(ToolResult {
                output: "pattern denied by sandbox policy".to_string(),
                metadata: json!({"pattern": pattern}),
            });
        }
        let workspace_root = workspace_root_from_args(&args);
        let effective_cwd = effective_cwd_from_args(&args);
        let scoped_pattern = if Path::new(pattern).is_absolute() {
            pattern.to_string()
        } else {
            effective_cwd.join(pattern).to_string_lossy().to_string()
        };
        let mut files = Vec::new();
        let mut effective_pattern = scoped_pattern.clone();
        let paths = match glob::glob(&scoped_pattern) {
            Ok(paths) => paths,
            Err(err) => {
                if let Some(normalized) = normalize_recursive_wildcard_pattern(&scoped_pattern) {
                    if let Ok(paths) = glob::glob(&normalized) {
                        effective_pattern = normalized;
                        paths
                    } else {
                        return Err(err.into());
                    }
                } else {
                    return Err(err.into());
                }
            }
        };
        for path in paths.flatten() {
            if is_discovery_ignored_path(&path) {
                continue;
            }
            if let Some(root) = workspace_root.as_ref() {
                if !is_within_workspace_root(&path, root) {
                    continue;
                }
            }
            files.push(path.display().to_string());
            if files.len() >= 100 {
                break;
            }
        }
        Ok(ToolResult {
            output: files.join("\n"),
            metadata: json!({
                "count": files.len(),
                "effective_cwd": effective_cwd,
                "workspace_root": workspace_root,
                "pattern": pattern,
                "effective_pattern": effective_pattern
            }),
        })
    }
}

fn is_discovery_ignored_path(path: &Path) -> bool {
    let components: Vec<_> = path.components().collect();
    for (idx, component) in components.iter().enumerate() {
        if component.as_os_str() == ".tandem" {
            let next = components
                .get(idx + 1)
                .map(|component| component.as_os_str());
            return next != Some(std::ffi::OsStr::new("artifacts"));
        }
    }
    false
}

struct GrepTool;

fn build_grep_matcher(pattern: &str) -> anyhow::Result<RegexMatcher> {
    let matcher = RegexMatcherBuilder::new()
        .line_terminator(Some(b'\n'))
        .build(pattern);
    match matcher {
        Ok(matcher) => Ok(matcher),
        Err(_) => RegexMatcherBuilder::new()
            .build(pattern)
            .map_err(|err| anyhow!(err.to_string())),
    }
}

fn build_grep_searcher() -> grep_searcher::Searcher {
    let mut builder = SearcherBuilder::new();
    builder
        .line_number(true)
        // Use ripgrep's auto mmap heuristic as the fast path for read-only workspace search.
        .memory_map(unsafe { MmapChoice::auto() })
        .binary_detection(BinaryDetection::quit(b'\0'))
        .bom_sniffing(false)
        .line_terminator(LineTerminator::byte(b'\n'));
    builder.build()
}

fn collect_grep_matches(
    searcher: &mut grep_searcher::Searcher,
    matcher: &RegexMatcher,
    path: &Path,
    out: &mut Vec<String>,
    limit: usize,
) {
    let Ok(file) = std::fs::File::open(path) else {
        return;
    };
    let path_display = path.display().to_string();
    let _ = searcher.search_file(
        matcher,
        &file,
        Lossy(|line_number, line| {
            if out.len() >= limit {
                return Ok(false);
            }
            let line = line.trim_end_matches(['\r', '\n']);
            out.push(format!("{}:{}:{}", path_display, line_number, line));
            Ok(out.len() < limit)
        }),
    );
}

#[async_trait]
impl Tool for GrepTool {
    fn schema(&self) -> ToolSchema {
        tool_schema_with_capabilities(
            "grep",
            "Regex search in files",
            json!({"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string"}}}),
            workspace_search_capabilities(),
        )
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let pattern = args["pattern"].as_str().unwrap_or("");
        let root = args["path"].as_str().unwrap_or(".");
        let Some(root_path) = resolve_walk_root(root, &args) else {
            return Ok(sandbox_path_denied_result(root, &args));
        };
        let matcher = build_grep_matcher(pattern)?;
        let mut searcher = build_grep_searcher();
        let mut out = Vec::new();
        for entry in WalkBuilder::new(&root_path).build().flatten() {
            if out.len() >= 100 {
                break;
            }
            if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                continue;
            }
            let path = entry.path();
            if is_discovery_ignored_path(path) {
                continue;
            }
            collect_grep_matches(&mut searcher, &matcher, path, &mut out, 100);
        }
        Ok(ToolResult {
            output: out.join("\n"),
            metadata: json!({"count": out.len(), "path": root_path.to_string_lossy()}),
        })
    }
}

struct WebFetchTool;
#[async_trait]
impl Tool for WebFetchTool {
    fn schema(&self) -> ToolSchema {
        tool_schema_with_capabilities(
            "webfetch",
            "Fetch URL content and return a structured markdown document",
            json!({
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
            web_fetch_capabilities(),
        )
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
        let return_mode = args["return"].as_str().unwrap_or("markdown");
        let timeout_ms = args["timeout_ms"]
            .as_u64()
            .unwrap_or(15_000)
            .clamp(1_000, 120_000);
        let max_bytes = args["max_bytes"].as_u64().unwrap_or(500_000).min(5_000_000) as usize;
        let max_redirects = args["max_redirects"].as_u64().unwrap_or(5).min(20) as usize;

        let started = std::time::Instant::now();
        let fetched = fetch_url_with_limits(url, timeout_ms, max_bytes, max_redirects).await?;
        let raw = String::from_utf8_lossy(&fetched.buffer).to_string();

        let cleaned = strip_html_noise(&raw);
        let title = extract_title(&cleaned).unwrap_or_default();
        let canonical = extract_canonical(&cleaned);
        let links = extract_links(&cleaned);

        let markdown = if fetched.content_type.contains("html") || fetched.content_type.is_empty() {
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
            "final_url": fetched.final_url,
            "title": title,
            "content_type": fetched.content_type,
            "markdown": markdown_out,
            "text": text_out,
            "links": links,
            "meta": {
                "canonical": canonical,
                "mode": mode
            },
            "stats": {
                "bytes_in": fetched.buffer.len(),
                "bytes_out": markdown_chars,
                "raw_chars": raw_chars,
                "markdown_chars": markdown_chars,
                "reduction_pct": reduction_pct,
                "elapsed_ms": started.elapsed().as_millis(),
                "truncated": fetched.truncated
            }
        });

        Ok(ToolResult {
            output: serde_json::to_string_pretty(&output)?,
            metadata: json!({
                "url": url,
                "final_url": fetched.final_url,
                "content_type": fetched.content_type,
                "truncated": fetched.truncated
            }),
        })
    }
}

struct WebFetchHtmlTool;
#[async_trait]
impl Tool for WebFetchHtmlTool {
    fn schema(&self) -> ToolSchema {
        tool_schema_with_capabilities(
            "webfetch_html",
            "Fetch URL and return raw HTML content",
            json!({
                "type":"object",
                "properties":{
                    "url":{"type":"string"},
                    "max_bytes":{"type":"integer"},
                    "timeout_ms":{"type":"integer"},
                    "max_redirects":{"type":"integer"}
                }
            }),
            web_fetch_capabilities(),
        )
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let url = args["url"].as_str().unwrap_or("").trim();
        if url.is_empty() {
            return Ok(ToolResult {
                output: "url is required".to_string(),
                metadata: json!({"url": url}),
            });
        }
        let timeout_ms = args["timeout_ms"]
            .as_u64()
            .unwrap_or(15_000)
            .clamp(1_000, 120_000);
        let max_bytes = args["max_bytes"].as_u64().unwrap_or(500_000).min(5_000_000) as usize;
        let max_redirects = args["max_redirects"].as_u64().unwrap_or(5).min(20) as usize;

        let started = std::time::Instant::now();
        let fetched = fetch_url_with_limits(url, timeout_ms, max_bytes, max_redirects).await?;
        let output = String::from_utf8_lossy(&fetched.buffer).to_string();

        Ok(ToolResult {
            output,
            metadata: json!({
                "url": url,
                "final_url": fetched.final_url,
                "content_type": fetched.content_type,
                "truncated": fetched.truncated,
                "bytes_in": fetched.buffer.len(),
                "elapsed_ms": started.elapsed().as_millis()
            }),
        })
    }
}

struct FetchedResponse {
    final_url: String,
    content_type: String,
    buffer: Vec<u8>,
    truncated: bool,
}

async fn fetch_url_with_limits(
    url: &str,
    timeout_ms: u64,
    max_bytes: usize,
    max_redirects: usize,
) -> anyhow::Result<FetchedResponse> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .redirect(reqwest::redirect::Policy::limited(max_redirects))
        .build()?;

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

    Ok(FetchedResponse {
        final_url,
        content_type,
        buffer,
        truncated,
    })
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
        tool_schema(
            "mcp_debug",
            "Call an MCP tool and return the raw response",
            json!({
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
        )
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

struct WebSearchTool {
    backend: SearchBackend,
}
#[async_trait]
impl Tool for WebSearchTool {
    fn schema(&self) -> ToolSchema {
        tool_schema_with_capabilities(
            "websearch",
            self.backend.schema_description(),
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer" }
                },
                "required": ["query"]
            }),
            web_fetch_capabilities(),
        )
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
        let outcome = execute_websearch_backend(&self.backend, &query, num_results).await?;
        let configured_backend = self.backend.name();
        let backend_used = outcome
            .backend_used
            .as_deref()
            .unwrap_or(configured_backend);
        let mut metadata = json!({
            "query": query,
            "query_source": query_source,
            "query_hash": query_hash,
            "backend": backend_used,
            "configured_backend": configured_backend,
            "attempted_backends": outcome.attempted_backends,
            "loop_guard_triggered": false,
            "count": outcome.results.len(),
            "partial": outcome.partial
        });
        if let Some(kind) = outcome.unavailable_kind {
            metadata["error"] = json!(kind);
        }

        if let Some(message) = outcome.unavailable_message {
            return Ok(ToolResult {
                output: message,
                metadata: metadata,
            });
        }

        let output = json!({
            "query": query,
            "backend": backend_used,
            "configured_backend": configured_backend,
            "attempted_backends": metadata["attempted_backends"],
            "result_count": outcome.results.len(),
            "partial": outcome.partial,
            "results": outcome.results,
        });

        Ok(ToolResult {
            output: serde_json::to_string_pretty(&output)?,
            metadata,
        })
    }
}

struct SearchExecutionOutcome {
    results: Vec<SearchResultEntry>,
    partial: bool,
    unavailable_message: Option<String>,
    unavailable_kind: Option<&'static str>,
    backend_used: Option<String>,
    attempted_backends: Vec<String>,
}

async fn execute_websearch_backend(
    backend: &SearchBackend,
    query: &str,
    num_results: u64,
) -> anyhow::Result<SearchExecutionOutcome> {
    match backend {
        SearchBackend::Auto { backends } => {
            let mut attempted_backends = Vec::new();
            let mut best_unavailable: Option<SearchExecutionOutcome> = None;

            for candidate in backends {
                let mut outcome =
                    execute_websearch_backend_once(candidate, query, num_results).await?;
                attempted_backends.extend(outcome.attempted_backends.iter().cloned());
                if outcome.unavailable_kind.is_none() {
                    if outcome.backend_used.is_none() {
                        outcome.backend_used = Some(candidate.name().to_string());
                    }
                    outcome.attempted_backends = attempted_backends;
                    return Ok(outcome);
                }

                let should_replace = best_unavailable
                    .as_ref()
                    .map(|current| {
                        search_unavailability_priority(outcome.unavailable_kind)
                            > search_unavailability_priority(current.unavailable_kind)
                    })
                    .unwrap_or(true);
                outcome.attempted_backends = attempted_backends.clone();
                if should_replace {
                    best_unavailable = Some(outcome);
                }
            }

            let mut outcome = best_unavailable.unwrap_or_else(search_backend_unavailable_outcome);
            outcome.attempted_backends = attempted_backends;
            Ok(outcome)
        }
        _ => execute_websearch_backend_once(backend, query, num_results).await,
    }
}

async fn execute_websearch_backend_once(
    backend: &SearchBackend,
    query: &str,
    num_results: u64,
) -> anyhow::Result<SearchExecutionOutcome> {
    match backend {
        SearchBackend::Disabled { reason } => Ok(SearchExecutionOutcome {
            results: Vec::new(),
            partial: false,
            unavailable_message: Some(format!(
                "Search backend is unavailable for `websearch`: {reason}"
            )),
            unavailable_kind: Some("backend_unavailable"),
            backend_used: Some("disabled".to_string()),
            attempted_backends: vec!["disabled".to_string()],
        }),
        SearchBackend::Tandem {
            base_url,
            timeout_ms,
        } => execute_tandem_search(base_url, *timeout_ms, query, num_results).await,
        SearchBackend::Searxng {
            base_url,
            engines,
            timeout_ms,
        } => {
            execute_searxng_search(
                base_url,
                engines.as_deref(),
                *timeout_ms,
                query,
                num_results,
            )
            .await
        }
        SearchBackend::Exa {
            api_key,
            timeout_ms,
        } => execute_exa_search(api_key, *timeout_ms, query, num_results).await,
        SearchBackend::Brave {
            api_key,
            timeout_ms,
        } => execute_brave_search(api_key, *timeout_ms, query, num_results).await,
        SearchBackend::Auto { .. } => unreachable!("auto backend should be handled by the wrapper"),
    }
}

fn search_backend_unavailable_outcome() -> SearchExecutionOutcome {
    SearchExecutionOutcome {
        results: Vec::new(),
        partial: false,
        unavailable_message: Some(
            "Web search is currently unavailable for `websearch`.\nContinue with local file reads and note that external research could not be completed in this run."
                .to_string(),
        ),
        unavailable_kind: Some("backend_unavailable"),
        backend_used: None,
        attempted_backends: Vec::new(),
    }
}

fn search_backend_authorization_required_outcome() -> SearchExecutionOutcome {
    SearchExecutionOutcome {
        results: Vec::new(),
        partial: false,
        unavailable_message: Some(
            "Authorization required for `websearch`.\nThis integration requires authorization before this action can run."
                .to_string(),
        ),
        unavailable_kind: Some("authorization_required"),
        backend_used: None,
        attempted_backends: Vec::new(),
    }
}

fn search_backend_rate_limited_outcome(
    backend_name: &str,
    retry_after_secs: Option<u64>,
) -> SearchExecutionOutcome {
    let retry_hint = retry_after_secs
        .map(|value| format!("\nRetry after about {value} second(s)."))
        .unwrap_or_default();
    SearchExecutionOutcome {
        results: Vec::new(),
        partial: false,
        unavailable_message: Some(format!(
            "Web search is currently rate limited for `websearch` on the {backend_name} backend.\nContinue with local file reads and note that external research could not be completed in this run.{retry_hint}"
        )),
        unavailable_kind: Some("rate_limited"),
        backend_used: Some(backend_name.to_string()),
        attempted_backends: vec![backend_name.to_string()],
    }
}

fn search_unavailability_priority(kind: Option<&'static str>) -> u8 {
    match kind {
        Some("authorization_required") => 3,
        Some("rate_limited") => 2,
        Some("backend_unavailable") => 1,
        _ => 0,
    }
}

async fn execute_tandem_search(
    base_url: &str,
    timeout_ms: u64,
    query: &str,
    num_results: u64,
) -> anyhow::Result<SearchExecutionOutcome> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()?;
    let endpoint = format!("{}/search", base_url.trim_end_matches('/'));
    let response = match client
        .post(&endpoint)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&json!({
            "query": query,
            "limit": num_results,
        }))
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => {
            let mut outcome = search_backend_unavailable_outcome();
            outcome.backend_used = Some("tandem".to_string());
            outcome.attempted_backends = vec!["tandem".to_string()];
            return Ok(outcome);
        }
    };
    let status = response.status();
    if matches!(
        status,
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) {
        let mut outcome = search_backend_authorization_required_outcome();
        outcome.backend_used = Some("tandem".to_string());
        outcome.attempted_backends = vec!["tandem".to_string()];
        return Ok(outcome);
    }
    if !status.is_success() {
        let mut outcome = search_backend_unavailable_outcome();
        outcome.backend_used = Some("tandem".to_string());
        outcome.attempted_backends = vec!["tandem".to_string()];
        return Ok(outcome);
    }
    let body: Value = match response.json().await {
        Ok(value) => value,
        Err(_) => {
            let mut outcome = search_backend_unavailable_outcome();
            outcome.backend_used = Some("tandem".to_string());
            outcome.attempted_backends = vec!["tandem".to_string()];
            return Ok(outcome);
        }
    };
    let raw_results = body
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let results = normalize_tandem_results(&raw_results, num_results as usize);
    let partial = body
        .get("partial")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| raw_results.len() > results.len());
    Ok(SearchExecutionOutcome {
        results,
        partial,
        unavailable_message: None,
        unavailable_kind: None,
        backend_used: Some("tandem".to_string()),
        attempted_backends: vec!["tandem".to_string()],
    })
}

async fn execute_searxng_search(
    base_url: &str,
    engines: Option<&str>,
    timeout_ms: u64,
    query: &str,
    num_results: u64,
) -> anyhow::Result<SearchExecutionOutcome> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()?;
    let endpoint = format!("{}/search", base_url.trim_end_matches('/'));
    let mut params: Vec<(&str, String)> = vec![
        ("q", query.to_string()),
        ("format", "json".to_string()),
        ("pageno", "1".to_string()),
    ];
    if let Some(engines) = engines {
        params.push(("engines", engines.to_string()));
    }
    let response = client.get(&endpoint).query(&params).send().await?;

    let status = response.status();
    if status == reqwest::StatusCode::FORBIDDEN {
        let mut outcome = search_backend_authorization_required_outcome();
        outcome.backend_used = Some("searxng".to_string());
        outcome.attempted_backends = vec!["searxng".to_string()];
        return Ok(outcome);
    }
    if !status.is_success() {
        let mut outcome = search_backend_unavailable_outcome();
        outcome.backend_used = Some("searxng".to_string());
        outcome.attempted_backends = vec!["searxng".to_string()];
        return Ok(outcome);
    }
    let status_for_error = status;
    let body: Value = match response.json().await {
        Ok(value) => value,
        Err(_) => {
            let mut outcome = search_backend_unavailable_outcome();
            outcome.backend_used = Some("searxng".to_string());
            outcome.attempted_backends = vec!["searxng".to_string()];
            return Ok(outcome);
        }
    };
    let raw_results = body
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let results = normalize_searxng_results(&raw_results, num_results as usize);
    let partial = raw_results.len() > results.len()
        || status_for_error == reqwest::StatusCode::PARTIAL_CONTENT;
    Ok(SearchExecutionOutcome {
        results,
        partial,
        unavailable_message: None,
        unavailable_kind: None,
        backend_used: Some("searxng".to_string()),
        attempted_backends: vec!["searxng".to_string()],
    })
}

async fn execute_exa_search(
    api_key: &str,
    timeout_ms: u64,
    query: &str,
    num_results: u64,
) -> anyhow::Result<SearchExecutionOutcome> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()?;
    let response = match client
        .post("https://api.exa.ai/search")
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .header("x-api-key", api_key)
        .json(&json!({
            "query": query,
            "numResults": num_results,
        }))
        .send()
        .await
    {
        Ok(response) => response,
        Err(_) => {
            let mut outcome = search_backend_unavailable_outcome();
            outcome.backend_used = Some("exa".to_string());
            outcome.attempted_backends = vec!["exa".to_string()];
            return Ok(outcome);
        }
    };
    let status = response.status();
    if matches!(
        status,
        reqwest::StatusCode::UNAUTHORIZED
            | reqwest::StatusCode::FORBIDDEN
            | reqwest::StatusCode::PAYMENT_REQUIRED
    ) {
        let mut outcome = search_backend_authorization_required_outcome();
        outcome.backend_used = Some("exa".to_string());
        outcome.attempted_backends = vec!["exa".to_string()];
        return Ok(outcome);
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let retry_after_secs = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok());
        return Ok(search_backend_rate_limited_outcome("exa", retry_after_secs));
    }
    if !status.is_success() {
        let mut outcome = search_backend_unavailable_outcome();
        outcome.backend_used = Some("exa".to_string());
        outcome.attempted_backends = vec!["exa".to_string()];
        return Ok(outcome);
    }
    let body: Value = match response.json().await {
        Ok(value) => value,
        Err(_) => {
            let mut outcome = search_backend_unavailable_outcome();
            outcome.backend_used = Some("exa".to_string());
            outcome.attempted_backends = vec!["exa".to_string()];
            return Ok(outcome);
        }
    };
    let raw_results = body
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let results = normalize_exa_results(&raw_results, num_results as usize);
    Ok(SearchExecutionOutcome {
        partial: raw_results.len() > results.len(),
        results,
        unavailable_message: None,
        unavailable_kind: None,
        backend_used: Some("exa".to_string()),
        attempted_backends: vec!["exa".to_string()],
    })
}

async fn execute_brave_search(
    api_key: &str,
    timeout_ms: u64,
    query: &str,
    num_results: u64,
) -> anyhow::Result<SearchExecutionOutcome> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()?;
    let count = num_results.to_string();
    let response = match client
        .get("https://api.search.brave.com/res/v1/web/search")
        .header("Accept", "application/json")
        .header("X-Subscription-Token", api_key)
        .query(&[("q", query), ("count", count.as_str())])
        .send()
        .await
    {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!("brave websearch request failed: {}", error);
            let mut outcome = search_backend_unavailable_outcome();
            outcome.backend_used = Some("brave".to_string());
            outcome.attempted_backends = vec!["brave".to_string()];
            return Ok(outcome);
        }
    };
    let status = response.status();
    if matches!(
        status,
        reqwest::StatusCode::UNAUTHORIZED
            | reqwest::StatusCode::FORBIDDEN
            | reqwest::StatusCode::PAYMENT_REQUIRED
    ) {
        let mut outcome = search_backend_authorization_required_outcome();
        outcome.backend_used = Some("brave".to_string());
        outcome.attempted_backends = vec!["brave".to_string()];
        return Ok(outcome);
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let retry_after_secs = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok());
        return Ok(search_backend_rate_limited_outcome(
            "brave",
            retry_after_secs,
        ));
    }
    if !status.is_success() {
        tracing::warn!("brave websearch returned non-success status: {}", status);
        let mut outcome = search_backend_unavailable_outcome();
        outcome.backend_used = Some("brave".to_string());
        outcome.attempted_backends = vec!["brave".to_string()];
        return Ok(outcome);
    }
    let body_text = match response.text().await {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!("brave websearch body read failed: {}", error);
            let mut outcome = search_backend_unavailable_outcome();
            outcome.backend_used = Some("brave".to_string());
            outcome.attempted_backends = vec!["brave".to_string()];
            return Ok(outcome);
        }
    };
    let body: Value = match serde_json::from_str(&body_text) {
        Ok(value) => value,
        Err(error) => {
            let snippet = body_text.chars().take(200).collect::<String>();
            tracing::warn!(
                "brave websearch JSON parse failed: {} body_prefix={:?}",
                error,
                snippet
            );
            let mut outcome = search_backend_unavailable_outcome();
            outcome.backend_used = Some("brave".to_string());
            outcome.attempted_backends = vec!["brave".to_string()];
            return Ok(outcome);
        }
    };
    let raw_results = body
        .get("web")
        .and_then(|value| value.get("results"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let results = normalize_brave_results(&raw_results, num_results as usize);
    Ok(SearchExecutionOutcome {
        partial: raw_results.len() > results.len(),
        results,
        unavailable_message: None,
        unavailable_kind: None,
        backend_used: Some("brave".to_string()),
        attempted_backends: vec!["brave".to_string()],
    })
}

fn normalize_tandem_results(raw_results: &[Value], limit: usize) -> Vec<SearchResultEntry> {
    raw_results
        .iter()
        .filter_map(|entry| {
            let title = entry
                .get("title")
                .or_else(|| entry.get("name"))
                .and_then(Value::as_str)?
                .trim()
                .to_string();
            let url = entry.get("url").and_then(Value::as_str)?.trim().to_string();
            if title.is_empty() || url.is_empty() {
                return None;
            }
            let snippet = entry
                .get("snippet")
                .or_else(|| entry.get("content"))
                .or_else(|| entry.get("description"))
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or_default()
                .to_string();
            let source = entry
                .get("source")
                .or_else(|| entry.get("provider"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("tandem")
                .to_string();
            Some(SearchResultEntry {
                title,
                url,
                snippet,
                source,
            })
        })
        .take(limit)
        .collect()
}

fn normalize_searxng_results(raw_results: &[Value], limit: usize) -> Vec<SearchResultEntry> {
    raw_results
        .iter()
        .filter_map(|entry| {
            let title = entry
                .get("title")
                .and_then(Value::as_str)?
                .trim()
                .to_string();
            let url = entry.get("url").and_then(Value::as_str)?.trim().to_string();
            if title.is_empty() || url.is_empty() {
                return None;
            }
            let snippet = entry
                .get("content")
                .and_then(Value::as_str)
                .or_else(|| entry.get("snippet").and_then(Value::as_str))
                .unwrap_or("")
                .trim()
                .to_string();
            let source = entry
                .get("engine")
                .and_then(Value::as_str)
                .map(|engine| format!("searxng:{engine}"))
                .unwrap_or_else(|| "searxng".to_string());
            Some(SearchResultEntry {
                title,
                url,
                snippet,
                source,
            })
        })
        .take(limit)
        .collect()
}

fn normalize_exa_results(raw_results: &[Value], limit: usize) -> Vec<SearchResultEntry> {
    raw_results
        .iter()
        .filter_map(|entry| {
            let title = entry
                .get("title")
                .and_then(Value::as_str)?
                .trim()
                .to_string();
            let url = entry.get("url").and_then(Value::as_str)?.trim().to_string();
            if title.is_empty() || url.is_empty() {
                return None;
            }
            let snippet = entry
                .get("text")
                .and_then(Value::as_str)
                .or_else(|| {
                    entry
                        .get("highlights")
                        .and_then(Value::as_array)
                        .and_then(|items| items.iter().find_map(Value::as_str))
                })
                .unwrap_or("")
                .chars()
                .take(400)
                .collect::<String>()
                .trim()
                .to_string();
            Some(SearchResultEntry {
                title,
                url,
                snippet,
                source: "exa".to_string(),
            })
        })
        .take(limit)
        .collect()
}

fn normalize_brave_results(raw_results: &[Value], limit: usize) -> Vec<SearchResultEntry> {
    raw_results
        .iter()
        .filter_map(|entry| {
            let title = entry
                .get("title")
                .and_then(Value::as_str)?
                .trim()
                .to_string();
            let url = entry.get("url").and_then(Value::as_str)?.trim().to_string();
            if title.is_empty() || url.is_empty() {
                return None;
            }
            let snippet = entry
                .get("description")
                .and_then(Value::as_str)
                .or_else(|| entry.get("snippet").and_then(Value::as_str))
                .unwrap_or("")
                .trim()
                .to_string();
            let source = entry
                .get("profile")
                .and_then(|value| value.get("long_name"))
                .and_then(Value::as_str)
                .map(|value| format!("brave:{value}"))
                .unwrap_or_else(|| "brave".to_string());
            Some(SearchResultEntry {
                title,
                url,
                snippet,
                source,
            })
        })
        .take(limit)
        .collect()
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
            if let Some(cleaned) = sanitize_websearch_query_candidate(query) {
                return Some(cleaned);
            }
        }
    }

    // Some tool-call envelopes nest args.
    for container in ["arguments", "args", "input", "params"] {
        if let Some(obj) = args.get(container) {
            for key in QUERY_KEYS {
                if let Some(query) = obj.get(key).and_then(|v| v.as_str()) {
                    if let Some(cleaned) = sanitize_websearch_query_candidate(query) {
                        return Some(cleaned);
                    }
                }
            }
        }
    }

    // Last resort: plain string args.
    args.as_str().and_then(sanitize_websearch_query_candidate)
}

fn sanitize_websearch_query_candidate(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    if let Some(start) = lower.find("<arg_value>") {
        let value_start = start + "<arg_value>".len();
        let tail = &trimmed[value_start..];
        let value = if let Some(end) = tail.to_ascii_lowercase().find("</arg_value>") {
            &tail[..end]
        } else {
            tail
        };
        let cleaned = value.trim();
        if !cleaned.is_empty() {
            return Some(cleaned.to_string());
        }
    }

    let without_wrappers = trimmed
        .replace("<arg_key>", " ")
        .replace("</arg_key>", " ")
        .replace("<arg_value>", " ")
        .replace("</arg_value>", " ");
    let collapsed = without_wrappers
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.is_empty() {
        return None;
    }

    let collapsed_lower = collapsed.to_ascii_lowercase();
    if let Some(rest) = collapsed_lower.strip_prefix("websearch query ") {
        let offset = collapsed.len() - rest.len();
        let q = collapsed[offset..].trim();
        if !q.is_empty() {
            return Some(q.to_string());
        }
    }
    if let Some(rest) = collapsed_lower.strip_prefix("query ") {
        let offset = collapsed.len() - rest.len();
        let q = collapsed[offset..].trim();
        if !q.is_empty() {
            return Some(q.to_string());
        }
    }

    Some(collapsed)
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
        tool_schema_with_capabilities(
            "codesearch",
            "Search code in workspace files",
            json!({"type":"object","properties":{"query":{"type":"string"},"path":{"type":"string"},"limit":{"type":"integer"}}}),
            workspace_search_capabilities(),
        )
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
        let Some(root_path) = resolve_walk_root(root, &args) else {
            return Ok(sandbox_path_denied_result(root, &args));
        };
        let limit = args["limit"]
            .as_u64()
            .map(|v| v.clamp(1, 200) as usize)
            .unwrap_or(50);
        let mut hits = Vec::new();
        let lower = query.to_lowercase();
        for entry in WalkBuilder::new(&root_path).build().flatten() {
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
            metadata: json!({"count": hits.len(), "query": query, "path": root_path.to_string_lossy()}),
        })
    }
}

struct TodoWriteTool;
#[async_trait]
impl Tool for TodoWriteTool {
    fn schema(&self) -> ToolSchema {
        tool_schema(
            "todo_write",
            "Update todo list",
            json!({
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
        )
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
        tool_schema(
            "task",
            "Create a subtask summary or spawn a teammate when team_name is provided.",
            task_schema(),
        )
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let input = serde_json::from_value::<TaskInput>(args.clone())
            .map_err(|err| anyhow!("invalid Task args: {}", err))?;
        let description = input.description;
        if let Some(team_name_raw) = input.team_name {
            let team_name = sanitize_team_name(&team_name_raw)?;
            let paths = resolve_agent_team_paths(&args)?;
            fs::create_dir_all(paths.team_dir(&team_name)).await?;
            fs::create_dir_all(paths.tasks_dir(&team_name)).await?;
            fs::create_dir_all(paths.mailboxes_dir(&team_name)).await?;
            fs::create_dir_all(paths.requests_dir(&team_name)).await?;
            upsert_team_index(&paths, &team_name).await?;

            let member_name = if let Some(requested_name) = input.name {
                sanitize_member_name(&requested_name)?
            } else {
                next_default_member_name(&paths, &team_name).await?
            };
            let inserted = upsert_team_member(
                &paths,
                &team_name,
                &member_name,
                Some(input.subagent_type.clone()),
                input.model.clone(),
            )
            .await?;
            append_mailbox_message(
                &paths,
                &team_name,
                &member_name,
                json!({
                    "id": format!("msg_{}", uuid_like(now_ms_u64())),
                    "type": "task_prompt",
                    "from": args.get("sender").and_then(|v| v.as_str()).unwrap_or("team-lead"),
                    "to": member_name,
                    "content": input.prompt,
                    "summary": description,
                    "timestampMs": now_ms_u64(),
                    "read": false
                }),
            )
            .await?;
            let mut events = Vec::new();
            if inserted {
                events.push(json!({
                    "type": "agent_team.member.spawned",
                    "properties": {
                        "teamName": team_name,
                        "memberName": member_name,
                        "agentType": input.subagent_type,
                        "model": input.model,
                    }
                }));
            }
            events.push(json!({
                "type": "agent_team.mailbox.enqueued",
                "properties": {
                    "teamName": team_name,
                    "recipient": member_name,
                    "messageType": "task_prompt",
                }
            }));
            return Ok(ToolResult {
                output: format!("Teammate task queued for {}", member_name),
                metadata: json!({
                    "ok": true,
                    "team_name": team_name,
                    "teammate_name": member_name,
                    "events": events
                }),
            });
        }
        Ok(ToolResult {
            output: format!("Subtask planned: {description}"),
            metadata: json!({"description": description, "prompt": input.prompt}),
        })
    }
}

struct QuestionTool;
#[async_trait]
impl Tool for QuestionTool {
    fn schema(&self) -> ToolSchema {
        tool_schema(
            "question",
            "Emit a question request for the user",
            json!({
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
        )
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let questions = normalize_question_payload(&args);
        if questions.is_empty() {
            return Err(anyhow!(
                "QUESTION_INVALID_ARGS: expected non-empty `questions` with at least one non-empty `question` string"
            ));
        }
        Ok(ToolResult {
            output: "Question requested. Use /question endpoints to respond.".to_string(),
            metadata: json!({"questions": questions}),
        })
    }
}

fn normalize_question_payload(args: &Value) -> Vec<Value> {
    let parsed_args;
    let args = if let Some(raw) = args.as_str() {
        if let Ok(decoded) = serde_json::from_str::<Value>(raw) {
            parsed_args = decoded;
            &parsed_args
        } else {
            args
        }
    } else {
        args
    };

    let Some(obj) = args.as_object() else {
        return Vec::new();
    };

    if let Some(items) = obj.get("questions").and_then(|v| v.as_array()) {
        let normalized = items
            .iter()
            .filter_map(normalize_question_entry)
            .collect::<Vec<_>>();
        if !normalized.is_empty() {
            return normalized;
        }
    }

    let question = obj
        .get("question")
        .or_else(|| obj.get("prompt"))
        .or_else(|| obj.get("query"))
        .or_else(|| obj.get("text"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let Some(question) = question else {
        return Vec::new();
    };
    let options = obj
        .get("options")
        .or_else(|| obj.get("choices"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(normalize_question_choice)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let multiple = obj
        .get("multiple")
        .or_else(|| obj.get("multi_select"))
        .or_else(|| obj.get("multiSelect"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let custom = obj
        .get("custom")
        .and_then(|v| v.as_bool())
        .unwrap_or(options.is_empty());
    vec![json!({
        "header": obj.get("header").and_then(|v| v.as_str()).unwrap_or("Question"),
        "question": question,
        "options": options,
        "multiple": multiple,
        "custom": custom
    })]
}

fn normalize_question_entry(entry: &Value) -> Option<Value> {
    if let Some(raw) = entry.as_str() {
        let question = raw.trim();
        if question.is_empty() {
            return None;
        }
        return Some(json!({
            "header": "Question",
            "question": question,
            "options": [],
            "multiple": false,
            "custom": true
        }));
    }
    let obj = entry.as_object()?;
    let question = obj
        .get("question")
        .or_else(|| obj.get("prompt"))
        .or_else(|| obj.get("query"))
        .or_else(|| obj.get("text"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    let options = obj
        .get("options")
        .or_else(|| obj.get("choices"))
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(normalize_question_choice)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let multiple = obj
        .get("multiple")
        .or_else(|| obj.get("multi_select"))
        .or_else(|| obj.get("multiSelect"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let custom = obj
        .get("custom")
        .and_then(|v| v.as_bool())
        .unwrap_or(options.is_empty());
    Some(json!({
        "header": obj.get("header").and_then(|v| v.as_str()).unwrap_or("Question"),
        "question": question,
        "options": options,
        "multiple": multiple,
        "custom": custom
    }))
}

fn normalize_question_choice(choice: &Value) -> Option<Value> {
    if let Some(label) = choice.as_str().map(str::trim).filter(|s| !s.is_empty()) {
        return Some(json!({
            "label": label,
            "description": ""
        }));
    }
    let obj = choice.as_object()?;
    let label = obj
        .get("label")
        .or_else(|| obj.get("title"))
        .or_else(|| obj.get("name"))
        .or_else(|| obj.get("value"))
        .or_else(|| obj.get("text"))
        .and_then(|v| {
            if let Some(s) = v.as_str() {
                Some(s.trim().to_string())
            } else {
                v.as_i64()
                    .map(|n| n.to_string())
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
            }
        })
        .filter(|s| !s.is_empty())?;
    let description = obj
        .get("description")
        .or_else(|| obj.get("hint"))
        .or_else(|| obj.get("subtitle"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    Some(json!({
        "label": label,
        "description": description
    }))
}

struct SpawnAgentTool;
#[async_trait]
impl Tool for SpawnAgentTool {
    fn schema(&self) -> ToolSchema {
        tool_schema(
            "spawn_agent",
            "Spawn an agent-team instance through server policy enforcement.",
            json!({
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
        )
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

struct TeamCreateTool;
#[async_trait]
impl Tool for TeamCreateTool {
    fn schema(&self) -> ToolSchema {
        tool_schema(
            "TeamCreate",
            "Create a coordinated team and shared task context.",
            team_create_schema(),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let input = serde_json::from_value::<TeamCreateInput>(args.clone())
            .map_err(|err| anyhow!("invalid TeamCreate args: {}", err))?;
        let now_ms = now_ms_u64();
        let paths = resolve_agent_team_paths(&args)?;
        let team_name = sanitize_team_name(&input.team_name)?;
        let team_dir = paths.team_dir(&team_name);
        fs::create_dir_all(paths.tasks_dir(&team_name)).await?;
        fs::create_dir_all(paths.mailboxes_dir(&team_name)).await?;
        fs::create_dir_all(paths.requests_dir(&team_name)).await?;

        let config = json!({
            "teamName": team_name,
            "description": input.description,
            "agentType": input.agent_type,
            "createdAtMs": now_ms
        });
        write_json_file(paths.config_file(&team_name), &config).await?;

        let lead_name = args
            .get("lead_name")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("A1");
        let members = json!([{
            "name": lead_name,
            "agentType": input.agent_type.clone().unwrap_or_else(|| "lead".to_string()),
            "createdAtMs": now_ms
        }]);
        write_json_file(paths.members_file(&team_name), &members).await?;

        upsert_team_index(&paths, &team_name).await?;
        if let Some(session_id) = args.get("__session_id").and_then(|v| v.as_str()) {
            write_team_session_context(&paths, session_id, &team_name).await?;
        }

        Ok(ToolResult {
            output: format!("Team created: {}", team_name),
            metadata: json!({
                "ok": true,
                "team_name": team_name,
                "path": team_dir.to_string_lossy(),
                "events": [{
                    "type": "agent_team.team.created",
                    "properties": {
                        "teamName": team_name,
                        "path": team_dir.to_string_lossy(),
                    }
                }]
            }),
        })
    }
}

struct TaskCreateCompatTool;
#[async_trait]
impl Tool for TaskCreateCompatTool {
    fn schema(&self) -> ToolSchema {
        tool_schema(
            "TaskCreate",
            "Create a task in the shared team task list.",
            task_create_schema(),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let input = serde_json::from_value::<TaskCreateInput>(args.clone())
            .map_err(|err| anyhow!("invalid TaskCreate args: {}", err))?;
        let paths = resolve_agent_team_paths(&args)?;
        let team_name = resolve_team_name(&paths, &args).await?;
        let tasks_dir = paths.tasks_dir(&team_name);
        fs::create_dir_all(&tasks_dir).await?;
        let next_id = next_task_id(&tasks_dir).await?;
        let now_ms = now_ms_u64();
        let task = json!({
            "id": next_id.to_string(),
            "subject": input.subject,
            "description": input.description,
            "activeForm": input.active_form,
            "status": "pending",
            "owner": Value::Null,
            "blocks": [],
            "blockedBy": [],
            "metadata": input.metadata.unwrap_or_else(|| json!({})),
            "createdAtMs": now_ms,
            "updatedAtMs": now_ms
        });
        write_json_file(paths.task_file(&team_name, &next_id.to_string()), &task).await?;
        Ok(ToolResult {
            output: format!("Task created: {}", next_id),
            metadata: json!({
                "ok": true,
                "team_name": team_name,
                "task": task,
                "events": [{
                    "type": "agent_team.task.created",
                    "properties": {
                        "teamName": team_name,
                        "taskId": next_id.to_string(),
                    }
                }]
            }),
        })
    }
}

struct TaskUpdateCompatTool;
#[async_trait]
impl Tool for TaskUpdateCompatTool {
    fn schema(&self) -> ToolSchema {
        tool_schema(
            "TaskUpdate",
            "Update ownership/state/dependencies of a shared task.",
            task_update_schema(),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let input = serde_json::from_value::<TaskUpdateInput>(args.clone())
            .map_err(|err| anyhow!("invalid TaskUpdate args: {}", err))?;
        let paths = resolve_agent_team_paths(&args)?;
        let team_name = resolve_team_name(&paths, &args).await?;
        let task_path = paths.task_file(&team_name, &input.task_id);
        if !task_path.exists() {
            return Ok(ToolResult {
                output: format!("Task not found: {}", input.task_id),
                metadata: json!({"ok": false, "code": "TASK_NOT_FOUND"}),
            });
        }
        let raw = fs::read_to_string(&task_path).await?;
        let mut task = serde_json::from_str::<Value>(&raw)
            .map_err(|err| anyhow!("failed parsing task {}: {}", input.task_id, err))?;
        let Some(obj) = task.as_object_mut() else {
            return Err(anyhow!("task payload is not an object"));
        };

        if let Some(subject) = input.subject {
            obj.insert("subject".to_string(), Value::String(subject));
        }
        if let Some(description) = input.description {
            obj.insert("description".to_string(), Value::String(description));
        }
        if let Some(active) = input.active_form {
            obj.insert("activeForm".to_string(), Value::String(active));
        }
        if let Some(status) = input.status {
            if status == "deleted" {
                let _ = fs::remove_file(&task_path).await;
                return Ok(ToolResult {
                    output: format!("Task deleted: {}", input.task_id),
                    metadata: json!({
                        "ok": true,
                        "deleted": true,
                        "taskId": input.task_id,
                        "events": [{
                            "type": "agent_team.task.deleted",
                            "properties": {
                                "teamName": team_name,
                                "taskId": input.task_id
                            }
                        }]
                    }),
                });
            }
            obj.insert("status".to_string(), Value::String(status));
        }
        if let Some(owner) = input.owner {
            obj.insert("owner".to_string(), Value::String(owner));
        }
        if let Some(add_blocks) = input.add_blocks {
            let current = obj
                .get("blocks")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            obj.insert(
                "blocks".to_string(),
                Value::Array(merge_unique_strings(current, add_blocks)),
            );
        }
        if let Some(add_blocked_by) = input.add_blocked_by {
            let current = obj
                .get("blockedBy")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();
            obj.insert(
                "blockedBy".to_string(),
                Value::Array(merge_unique_strings(current, add_blocked_by)),
            );
        }
        if let Some(metadata) = input.metadata {
            if let Some(current) = obj.get_mut("metadata").and_then(|v| v.as_object_mut()) {
                if let Some(incoming) = metadata.as_object() {
                    for (k, v) in incoming {
                        if v.is_null() {
                            current.remove(k);
                        } else {
                            current.insert(k.clone(), v.clone());
                        }
                    }
                }
            } else {
                obj.insert("metadata".to_string(), metadata);
            }
        }
        obj.insert("updatedAtMs".to_string(), json!(now_ms_u64()));
        write_json_file(task_path, &task).await?;
        Ok(ToolResult {
            output: format!("Task updated: {}", input.task_id),
            metadata: json!({
                "ok": true,
                "team_name": team_name,
                "task": task,
                "events": [{
                    "type": "agent_team.task.updated",
                    "properties": {
                        "teamName": team_name,
                        "taskId": input.task_id
                    }
                }]
            }),
        })
    }
}

struct TaskListCompatTool;
#[async_trait]
impl Tool for TaskListCompatTool {
    fn schema(&self) -> ToolSchema {
        tool_schema(
            "TaskList",
            "List tasks from the shared task list.",
            task_list_schema(),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let _ = serde_json::from_value::<TaskListInput>(args.clone())
            .map_err(|err| anyhow!("invalid TaskList args: {}", err))?;
        let paths = resolve_agent_team_paths(&args)?;
        let team_name = resolve_team_name(&paths, &args).await?;
        let tasks = read_tasks(&paths.tasks_dir(&team_name)).await?;
        let mut lines = Vec::new();
        for task in &tasks {
            let id = task
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();
            let subject = task
                .get("subject")
                .and_then(|v| v.as_str())
                .unwrap_or("(untitled)")
                .to_string();
            let status = task
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("pending")
                .to_string();
            let owner = task
                .get("owner")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            lines.push(format!(
                "{} [{}] {}{}",
                id,
                status,
                subject,
                if owner.is_empty() {
                    "".to_string()
                } else {
                    format!(" (owner: {})", owner)
                }
            ));
        }
        Ok(ToolResult {
            output: if lines.is_empty() {
                "No tasks.".to_string()
            } else {
                lines.join("\n")
            },
            metadata: json!({
                "ok": true,
                "team_name": team_name,
                "tasks": tasks
            }),
        })
    }
}

struct SendMessageCompatTool;
#[async_trait]
impl Tool for SendMessageCompatTool {
    fn schema(&self) -> ToolSchema {
        tool_schema(
            "SendMessage",
            "Send teammate messages and coordination protocol responses.",
            send_message_schema(),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let input = serde_json::from_value::<SendMessageInput>(args.clone())
            .map_err(|err| anyhow!("invalid SendMessage args: {}", err))?;
        let paths = resolve_agent_team_paths(&args)?;
        let team_name = resolve_team_name(&paths, &args).await?;
        fs::create_dir_all(paths.mailboxes_dir(&team_name)).await?;
        let sender = args
            .get("sender")
            .and_then(|v| v.as_str())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("team-lead")
            .to_string();
        let now_ms = now_ms_u64();

        match input.message_type {
            SendMessageType::Message | SendMessageType::ShutdownRequest => {
                let recipient = required_non_empty(input.recipient, "recipient")?;
                let content = required_non_empty(input.content, "content")?;
                append_mailbox_message(
                    &paths,
                    &team_name,
                    &recipient,
                    json!({
                        "id": format!("msg_{}", uuid_like(now_ms)),
                        "type": message_type_name(&input.message_type),
                        "from": sender,
                        "to": recipient,
                        "content": content,
                        "summary": input.summary,
                        "timestampMs": now_ms,
                        "read": false
                    }),
                )
                .await?;
                Ok(ToolResult {
                    output: "Message queued.".to_string(),
                    metadata: json!({
                        "ok": true,
                        "team_name": team_name,
                        "events": [{
                            "type": "agent_team.mailbox.enqueued",
                            "properties": {
                                "teamName": team_name,
                                "recipient": recipient,
                                "messageType": message_type_name(&input.message_type),
                            }
                        }]
                    }),
                })
            }
            SendMessageType::Broadcast => {
                let content = required_non_empty(input.content, "content")?;
                let members = read_team_member_names(&paths, &team_name).await?;
                for recipient in members {
                    append_mailbox_message(
                        &paths,
                        &team_name,
                        &recipient,
                        json!({
                            "id": format!("msg_{}_{}", uuid_like(now_ms), recipient),
                            "type": "broadcast",
                            "from": sender,
                            "to": recipient,
                            "content": content,
                            "summary": input.summary,
                            "timestampMs": now_ms,
                            "read": false
                        }),
                    )
                    .await?;
                }
                Ok(ToolResult {
                    output: "Broadcast queued.".to_string(),
                    metadata: json!({
                        "ok": true,
                        "team_name": team_name,
                        "events": [{
                            "type": "agent_team.mailbox.enqueued",
                            "properties": {
                                "teamName": team_name,
                                "recipient": "*",
                                "messageType": "broadcast",
                            }
                        }]
                    }),
                })
            }
            SendMessageType::ShutdownResponse | SendMessageType::PlanApprovalResponse => {
                let request_id = required_non_empty(input.request_id, "request_id")?;
                let request = json!({
                    "requestId": request_id,
                    "type": message_type_name(&input.message_type),
                    "from": sender,
                    "recipient": input.recipient,
                    "approve": input.approve,
                    "content": input.content,
                    "updatedAtMs": now_ms
                });
                write_json_file(paths.request_file(&team_name, &request_id), &request).await?;
                Ok(ToolResult {
                    output: "Response recorded.".to_string(),
                    metadata: json!({
                        "ok": true,
                        "team_name": team_name,
                        "request": request,
                        "events": [{
                            "type": "agent_team.request.resolved",
                            "properties": {
                                "teamName": team_name,
                                "requestId": request_id,
                                "requestType": message_type_name(&input.message_type),
                                "approve": input.approve
                            }
                        }]
                    }),
                })
            }
        }
    }
}

fn message_type_name(ty: &SendMessageType) -> &'static str {
    match ty {
        SendMessageType::Message => "message",
        SendMessageType::Broadcast => "broadcast",
        SendMessageType::ShutdownRequest => "shutdown_request",
        SendMessageType::ShutdownResponse => "shutdown_response",
        SendMessageType::PlanApprovalResponse => "plan_approval_response",
    }
}

fn required_non_empty(value: Option<String>, field: &str) -> anyhow::Result<String> {
    let Some(v) = value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    else {
        return Err(anyhow!("{} is required", field));
    };
    Ok(v)
}

fn resolve_agent_team_paths(args: &Value) -> anyhow::Result<AgentTeamPaths> {
    let workspace_root = args
        .get("__workspace_root")
        .and_then(|v| v.as_str())
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .ok_or_else(|| anyhow!("workspace root unavailable"))?;
    Ok(AgentTeamPaths::new(workspace_root.join(".tandem")))
}

async fn resolve_team_name(paths: &AgentTeamPaths, args: &Value) -> anyhow::Result<String> {
    if let Some(name) = args
        .get("team_name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return sanitize_team_name(name);
    }
    if let Some(session_id) = args.get("__session_id").and_then(|v| v.as_str()) {
        let context_path = paths
            .root()
            .join("session-context")
            .join(format!("{}.json", session_id));
        if context_path.exists() {
            let raw = fs::read_to_string(context_path).await?;
            let parsed = serde_json::from_str::<Value>(&raw)?;
            if let Some(name) = parsed
                .get("team_name")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                return sanitize_team_name(name);
            }
        }
    }
    Err(anyhow!(
        "team_name is required (no active team context for this session)"
    ))
}

fn sanitize_team_name(input: &str) -> anyhow::Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("team_name cannot be empty"));
    }
    let sanitized = trimmed
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();
    Ok(sanitized)
}

fn sanitize_member_name(input: &str) -> anyhow::Result<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("member name cannot be empty"));
    }
    if let Some(rest) = trimmed
        .strip_prefix('A')
        .or_else(|| trimmed.strip_prefix('a'))
    {
        if let Ok(n) = rest.parse::<u32>() {
            if n > 0 {
                return Ok(format!("A{}", n));
            }
        }
    }
    let sanitized = trimmed
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>();
    if sanitized.is_empty() {
        return Err(anyhow!("member name cannot be empty"));
    }
    Ok(sanitized)
}

async fn next_default_member_name(
    paths: &AgentTeamPaths,
    team_name: &str,
) -> anyhow::Result<String> {
    let names = read_team_member_names(paths, team_name).await?;
    let mut max_index = 1u32;
    for name in names {
        let trimmed = name.trim();
        let Some(rest) = trimmed
            .strip_prefix('A')
            .or_else(|| trimmed.strip_prefix('a'))
        else {
            continue;
        };
        let Ok(index) = rest.parse::<u32>() else {
            continue;
        };
        if index > max_index {
            max_index = index;
        }
    }
    Ok(format!("A{}", max_index.saturating_add(1)))
}

async fn write_json_file(path: PathBuf, value: &Value) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }
    fs::write(path, serde_json::to_vec_pretty(value)?).await?;
    Ok(())
}

async fn upsert_team_index(paths: &AgentTeamPaths, team_name: &str) -> anyhow::Result<()> {
    let index_path = paths.index_file();
    let mut teams = if index_path.exists() {
        let raw = fs::read_to_string(&index_path).await?;
        serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default()
    } else {
        Vec::new()
    };
    if !teams.iter().any(|team| team == team_name) {
        teams.push(team_name.to_string());
        teams.sort();
    }
    write_json_file(index_path, &json!(teams)).await
}

async fn write_team_session_context(
    paths: &AgentTeamPaths,
    session_id: &str,
    team_name: &str,
) -> anyhow::Result<()> {
    let context_path = paths
        .root()
        .join("session-context")
        .join(format!("{}.json", session_id));
    write_json_file(context_path, &json!({ "team_name": team_name })).await
}

async fn next_task_id(tasks_dir: &Path) -> anyhow::Result<u64> {
    let mut max_id = 0u64;
    let mut entries = match fs::read_dir(tasks_dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(1),
        Err(err) => return Err(err.into()),
    };
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if let Ok(id) = stem.parse::<u64>() {
            max_id = max_id.max(id);
        }
    }
    Ok(max_id + 1)
}

fn merge_unique_strings(current: Vec<Value>, incoming: Vec<String>) -> Vec<Value> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for value in current {
        if let Some(text) = value.as_str() {
            let text = text.to_string();
            if seen.insert(text.clone()) {
                out.push(Value::String(text));
            }
        }
    }
    for value in incoming {
        if seen.insert(value.clone()) {
            out.push(Value::String(value));
        }
    }
    out
}

async fn read_tasks(tasks_dir: &Path) -> anyhow::Result<Vec<Value>> {
    let mut tasks = Vec::new();
    let mut entries = match fs::read_dir(tasks_dir).await {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(tasks),
        Err(err) => return Err(err.into()),
    };
    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let raw = fs::read_to_string(path).await?;
        let task = serde_json::from_str::<Value>(&raw)?;
        tasks.push(task);
    }
    tasks.sort_by_key(|task| {
        task.get("id")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0)
    });
    Ok(tasks)
}

async fn append_mailbox_message(
    paths: &AgentTeamPaths,
    team_name: &str,
    recipient: &str,
    message: Value,
) -> anyhow::Result<()> {
    let mailbox_path = paths.mailbox_file(team_name, recipient);
    if let Some(parent) = mailbox_path.parent() {
        fs::create_dir_all(parent).await?;
    }
    let line = format!("{}\n", serde_json::to_string(&message)?);
    if mailbox_path.exists() {
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(mailbox_path)
            .await?;
        file.write_all(line.as_bytes()).await?;
        file.flush().await?;
    } else {
        fs::write(mailbox_path, line).await?;
    }
    Ok(())
}

async fn read_team_member_names(
    paths: &AgentTeamPaths,
    team_name: &str,
) -> anyhow::Result<Vec<String>> {
    let members_path = paths.members_file(team_name);
    if !members_path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(members_path).await?;
    let parsed = serde_json::from_str::<Value>(&raw)?;
    let Some(items) = parsed.as_array() else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for item in items {
        if let Some(name) = item
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            out.push(name.to_string());
        }
    }
    Ok(out)
}

async fn upsert_team_member(
    paths: &AgentTeamPaths,
    team_name: &str,
    member_name: &str,
    agent_type: Option<String>,
    model: Option<String>,
) -> anyhow::Result<bool> {
    let members_path = paths.members_file(team_name);
    let mut members = if members_path.exists() {
        let raw = fs::read_to_string(&members_path).await?;
        serde_json::from_str::<Value>(&raw)?
            .as_array()
            .cloned()
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let already_present = members.iter().any(|item| {
        item.get("name")
            .and_then(|v| v.as_str())
            .map(|s| s == member_name)
            .unwrap_or(false)
    });
    if already_present {
        return Ok(false);
    }
    members.push(json!({
        "name": member_name,
        "agentType": agent_type,
        "model": model,
        "createdAtMs": now_ms_u64()
    }));
    write_json_file(members_path, &Value::Array(members)).await?;
    Ok(true)
}

fn now_ms_u64() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn uuid_like(seed: u64) -> String {
    format!("{:x}", seed)
}

struct MemorySearchTool;
#[async_trait]
impl Tool for MemorySearchTool {
    fn schema(&self) -> ToolSchema {
        tool_schema(
            "memory_search",
            "Search tandem memory across session/project/global tiers. If scope fields are omitted, the tool defaults to the current session/project context and may include global memory when policy allows it.",
            json!({
                "type":"object",
                "properties":{
                    "query":{"type":"string"},
                    "session_id":{"type":"string"},
                    "project_id":{"type":"string"},
                    "tier":{"type":"string","enum":["session","project","global"]},
                    "limit":{"type":"integer","minimum":1,"maximum":20},
                    "allow_global":{"type":"boolean"}
                },
                "required":["query"]
            }),
        )
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

        let session_id = memory_session_id(&args);
        let project_id = memory_project_id(&args);
        let allow_global = global_memory_enabled(&args);
        if session_id.is_none() && project_id.is_none() && !allow_global {
            return Ok(ToolResult {
                output: "memory_search requires a current session/project context or global memory enabled by policy"
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
        tool_schema(
            "memory_store",
            "Store memory chunks in session/project/global tiers. If scope is omitted, the tool defaults to the current project, then session, and only uses global memory when policy allows it.",
            json!({
                "type":"object",
                "properties":{
                    "content":{"type":"string"},
                    "tier":{"type":"string","enum":["session","project","global"]},
                    "session_id":{"type":"string"},
                    "project_id":{"type":"string"},
                    "source":{"type":"string"},
                    "metadata":{"type":"object"},
                    "allow_global":{"type":"boolean"}
                },
                "required":["content"]
            }),
        )
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

        let session_id = memory_session_id(&args);
        let project_id = memory_project_id(&args);
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
                        output: "memory_store requires a current session/project context or global memory enabled by policy"
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
        tool_schema(
            "memory_list",
            "List stored memory chunks for auditing and knowledge-base browsing.",
            json!({
                "type":"object",
                "properties":{
                    "tier":{"type":"string","enum":["session","project","global","all"]},
                    "session_id":{"type":"string"},
                    "project_id":{"type":"string"},
                    "limit":{"type":"integer","minimum":1,"maximum":200},
                    "allow_global":{"type":"boolean"}
                }
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let session_id = memory_session_id(&args);
        let project_id = memory_project_id(&args);
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
                output: "memory_list requires a current session/project context or global memory enabled by policy".to_string(),
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

struct MemoryDeleteTool;
#[async_trait]
impl Tool for MemoryDeleteTool {
    fn schema(&self) -> ToolSchema {
        tool_schema(
            "memory_delete",
            "Delete a stored memory chunk from session/project/global memory within the current allowed scope.",
            json!({
                "type":"object",
                "properties":{
                    "chunk_id":{"type":"string"},
                    "id":{"type":"string"},
                    "tier":{"type":"string","enum":["session","project","global"]},
                    "session_id":{"type":"string"},
                    "project_id":{"type":"string"},
                    "allow_global":{"type":"boolean"}
                },
                "required":["chunk_id"]
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let chunk_id = args
            .get("chunk_id")
            .or_else(|| args.get("id"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or("");
        if chunk_id.is_empty() {
            return Ok(ToolResult {
                output: "memory_delete requires chunk_id".to_string(),
                metadata: json!({"ok": false, "reason": "missing_chunk_id"}),
            });
        }

        let session_id = memory_session_id(&args);
        let project_id = memory_project_id(&args);
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
                    output: "memory_delete tier must be one of: session, project, global"
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
                        output: "memory_delete requires a current session/project context or global memory enabled by policy".to_string(),
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
        let deleted = manager
            .db()
            .delete_chunk(tier, chunk_id, project_id.as_deref(), session_id.as_deref())
            .await?;

        if deleted == 0 {
            return Ok(ToolResult {
                output: format!("memory chunk `{chunk_id}` not found in {tier} memory"),
                metadata: json!({
                    "ok": false,
                    "reason": "not_found",
                    "chunk_id": chunk_id,
                    "tier": tier.to_string(),
                    "session_id": session_id,
                    "project_id": project_id,
                    "allow_global": allow_global,
                    "db_path": db_path,
                }),
            });
        }

        Ok(ToolResult {
            output: format!("deleted memory chunk `{chunk_id}` from {tier} memory"),
            metadata: json!({
                "ok": true,
                "deleted": true,
                "chunk_id": chunk_id,
                "count": deleted,
                "tier": tier.to_string(),
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
        .get("__memory_db_path")
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
    if let Ok(state_dir) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = state_dir.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("memory.sqlite");
        }
    }
    if let Some(data_dir) = dirs::data_dir() {
        return data_dir.join("tandem").join("memory.sqlite");
    }
    PathBuf::from("memory.sqlite")
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MemoryVisibleScope {
    Session,
    Project,
    Global,
}

fn parse_memory_visible_scope(raw: &str) -> Option<MemoryVisibleScope> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "session" => Some(MemoryVisibleScope::Session),
        "project" | "workspace" => Some(MemoryVisibleScope::Project),
        "global" => Some(MemoryVisibleScope::Global),
        _ => None,
    }
}

fn memory_visible_scope(args: &Value) -> MemoryVisibleScope {
    if let Some(scope) = args
        .get("__memory_max_visible_scope")
        .and_then(|v| v.as_str())
        .and_then(parse_memory_visible_scope)
    {
        return scope;
    }
    if let Ok(raw) = std::env::var("TANDEM_MEMORY_MAX_VISIBLE_SCOPE") {
        if let Some(scope) = parse_memory_visible_scope(&raw) {
            return scope;
        }
    }
    MemoryVisibleScope::Global
}

fn memory_session_id(args: &Value) -> Option<String> {
    args.get("session_id")
        .or_else(|| args.get("__session_id"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

fn memory_project_id(args: &Value) -> Option<String> {
    args.get("project_id")
        .or_else(|| args.get("__project_id"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

fn global_memory_enabled(args: &Value) -> bool {
    if memory_visible_scope(args) != MemoryVisibleScope::Global {
        return false;
    }
    if let Some(explicit) = args.get("allow_global").and_then(|v| v.as_bool()) {
        return explicit;
    }
    match std::env::var("TANDEM_ENABLE_GLOBAL_MEMORY") {
        Ok(raw) => !matches!(
            raw.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ),
        Err(_) => true,
    }
}

struct SkillTool;
#[async_trait]
impl Tool for SkillTool {
    fn schema(&self) -> ToolSchema {
        tool_schema(
            "skill",
            "List or load installed Tandem skills. Call without name to list available skills; provide name to load full SKILL.md content.",
            json!({"type":"object","properties":{"name":{"type":"string"}}}),
        )
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
        tool_schema_with_capabilities(
            "apply_patch",
            "Apply a Codex-style patch in a git workspace, or validate patch text when git patching is unavailable",
            json!({"type":"object","properties":{"patchText":{"type":"string"}}}),
            apply_patch_capabilities(),
        )
    }
    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let patch = args["patchText"].as_str().unwrap_or("");
        let has_begin = patch.contains("*** Begin Patch");
        let has_end = patch.contains("*** End Patch");
        let patch_paths = extract_apply_patch_paths(patch);
        let file_ops = patch_paths.len();
        let valid = has_begin && has_end && file_ops > 0;
        if !valid {
            return Ok(ToolResult {
                output: "Invalid patch format. Expected Begin/End markers and at least one file operation."
                    .to_string(),
                metadata: json!({"valid": false, "fileOps": file_ops}),
            });
        }
        let workspace_root =
            workspace_root_from_args(&args).unwrap_or_else(|| effective_cwd_from_args(&args));
        let git_root = resolve_git_root_for_dir(&workspace_root).await;
        if let Some(git_root) = git_root {
            let denied_paths = patch_paths
                .iter()
                .filter_map(|rel| {
                    let resolved = git_root.join(rel);
                    if is_within_workspace_root(&resolved, &workspace_root) {
                        None
                    } else {
                        Some(rel.clone())
                    }
                })
                .collect::<Vec<_>>();
            if !denied_paths.is_empty() {
                return Ok(ToolResult {
                    output: format!(
                        "patch denied by workspace policy for paths: {}",
                        denied_paths.join(", ")
                    ),
                    metadata: json!({
                        "valid": true,
                        "applied": false,
                        "reason": "path_outside_workspace",
                        "paths": patch_paths
                    }),
                });
            }
            let tmp_name = format!(
                "tandem-apply-patch-{}-{}.patch",
                std::process::id(),
                now_millis()
            );
            let patch_path = std::env::temp_dir().join(tmp_name);
            fs::write(&patch_path, patch).await?;
            let output = Command::new("git")
                .current_dir(&git_root)
                .arg("apply")
                .arg("--3way")
                .arg("--recount")
                .arg("--whitespace=nowarn")
                .arg(&patch_path)
                .output()
                .await?;
            let _ = fs::remove_file(&patch_path).await;
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let ok = output.status.success();
            return Ok(ToolResult {
                output: if ok {
                    if stdout.is_empty() {
                        "ok".to_string()
                    } else {
                        stdout.clone()
                    }
                } else if stderr.is_empty() {
                    "git apply failed".to_string()
                } else {
                    stderr.clone()
                },
                metadata: json!({
                    "valid": true,
                    "applied": ok,
                    "paths": patch_paths,
                    "git_root": git_root.to_string_lossy(),
                    "stdout": stdout,
                    "stderr": stderr
                }),
            });
        }
        Ok(ToolResult {
            output: "Patch format validated, but no git workspace was detected. Use `edit` for existing files or `write` for new files in this workspace."
                .to_string(),
            metadata: json!({
                "valid": true,
                "applied": false,
                "reason": "git_workspace_unavailable",
                "paths": patch_paths
            }),
        })
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::sync::{Mutex, OnceLock};
    use tempfile::TempDir;
    use tokio::fs;
    use tokio_util::sync::CancellationToken;

    struct TestTool {
        schema: ToolSchema,
    }

    #[async_trait]
    impl Tool for TestTool {
        fn schema(&self) -> ToolSchema {
            self.schema.clone()
        }

        async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
            Ok(ToolResult {
                output: "ok".to_string(),
                metadata: json!({}),
            })
        }

        async fn execute_with_cancel(
            &self,
            args: Value,
            _cancel: CancellationToken,
        ) -> anyhow::Result<ToolResult> {
            self.execute(args).await
        }
    }

    fn search_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn clear_search_env() {
        std::env::remove_var("TANDEM_SEARCH_BACKEND");
        std::env::remove_var("TANDEM_SEARCH_URL");
        std::env::remove_var("TANDEM_SEARXNG_URL");
        std::env::remove_var("TANDEM_SEARXNG_ENGINES");
        std::env::remove_var("TANDEM_SEARCH_TIMEOUT_MS");
        std::env::remove_var("TANDEM_EXA_API_KEY");
        std::env::remove_var("TANDEM_EXA_SEARCH_API_KEY");
        std::env::remove_var("EXA_API_KEY");
        std::env::remove_var("TANDEM_BRAVE_SEARCH_API_KEY");
        std::env::remove_var("BRAVE_SEARCH_API_KEY");
    }

    #[test]
    fn validator_rejects_array_without_items() {
        let schemas = vec![ToolSchema::new(
            "bad",
            "bad schema",
            json!({
                "type":"object",
                "properties":{"todos":{"type":"array"}}
            }),
        )];
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

    #[tokio::test]
    async fn core_tool_schemas_include_expected_capabilities() {
        let registry = ToolRegistry::new();
        let schemas = registry.list().await;
        let schema_by_name = schemas
            .iter()
            .map(|schema| (schema.name.as_str(), schema))
            .collect::<HashMap<_, _>>();

        let read = schema_by_name.get("read").expect("read tool");
        assert!(read.capabilities.reads_workspace);
        assert!(read.capabilities.preferred_for_discovery);
        assert_eq!(
            read.capabilities.effects,
            vec![tandem_types::ToolEffect::Read]
        );

        let write = schema_by_name.get("write").expect("write tool");
        assert!(write.capabilities.writes_workspace);
        assert!(write.capabilities.requires_verification);
        assert_eq!(
            write.capabilities.effects,
            vec![tandem_types::ToolEffect::Write]
        );

        let grep = schema_by_name.get("grep").expect("grep tool");
        assert!(grep.capabilities.reads_workspace);
        assert!(grep.capabilities.preferred_for_discovery);
        assert_eq!(
            grep.capabilities.effects,
            vec![tandem_types::ToolEffect::Search]
        );

        let bash = schema_by_name.get("bash").expect("bash tool");
        assert!(bash.capabilities.destructive);
        assert!(bash.capabilities.network_access);
        assert_eq!(
            bash.capabilities.effects,
            vec![tandem_types::ToolEffect::Execute]
        );

        let webfetch = schema_by_name.get("webfetch").expect("webfetch tool");
        assert!(webfetch.capabilities.network_access);
        assert!(webfetch.capabilities.preferred_for_discovery);
        assert_eq!(
            webfetch.capabilities.effects,
            vec![tandem_types::ToolEffect::Fetch]
        );

        let apply_patch = schema_by_name.get("apply_patch").expect("apply_patch tool");
        assert!(apply_patch.capabilities.reads_workspace);
        assert!(apply_patch.capabilities.writes_workspace);
        assert!(apply_patch.capabilities.requires_verification);
        assert_eq!(
            apply_patch.capabilities.effects,
            vec![tandem_types::ToolEffect::Patch]
        );
    }

    fn grep_args(root: &Path, pattern: &str) -> Value {
        let root = root.to_string_lossy().to_string();
        json!({
            "pattern": pattern,
            "path": root.clone(),
            "__workspace_root": root.clone(),
            "__effective_cwd": root,
        })
    }

    #[tokio::test]
    async fn grep_tool_reports_matches_while_skipping_ignored_and_binary_paths() {
        let tempdir = TempDir::new().expect("tempdir");
        let root = tempdir.path();
        let visible = root.join("src").join("nested").join("notes.txt");
        let ignored = root.join(".tandem").join("private").join("secret.txt");
        let binary = root.join("binary.bin");

        std::fs::create_dir_all(visible.parent().expect("visible parent"))
            .expect("create visible dir");
        std::fs::create_dir_all(ignored.parent().expect("ignored parent"))
            .expect("create ignored dir");
        std::fs::write(&visible, "first line\nneedle here\nlast line").expect("write visible file");
        std::fs::write(&ignored, "needle should stay hidden").expect("write ignored file");
        std::fs::write(&binary, b"\0needle after null\n").expect("write binary file");

        let tool = GrepTool;
        let result = tool
            .execute(grep_args(root, "needle"))
            .await
            .expect("grep result");

        assert_eq!(result.metadata["count"], json!(1));
        assert_eq!(
            result.output,
            format!("{}:2:needle here", visible.display())
        );
        assert!(!result.output.contains(".tandem/private/secret.txt"));
    }

    #[tokio::test]
    async fn grep_tool_caps_results_at_100_hits() {
        let tempdir = TempDir::new().expect("tempdir");
        let root = tempdir.path();
        let source = root.join("many.txt");
        let lines = (1..=120)
            .map(|idx| format!("match line {}", idx))
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&source, lines).expect("write source file");

        let tool = GrepTool;
        let result = tool
            .execute(grep_args(root, "match"))
            .await
            .expect("grep result");

        assert_eq!(result.metadata["count"], json!(100));
        assert_eq!(result.output.lines().count(), 100);
        assert!(result.output.contains("match line 100"));
        assert!(!result.output.contains("match line 101"));
    }

    #[tokio::test]
    async fn grep_tool_rejects_invalid_regex_patterns() {
        let tempdir = TempDir::new().expect("tempdir");
        let root = tempdir.path();
        std::fs::write(root.join("notes.txt"), "needle").expect("write file");

        let tool = GrepTool;
        let err = tool.execute(grep_args(root, "(")).await;

        assert!(err.is_err(), "expected invalid regex to fail");
    }

    #[tokio::test]
    async fn mcp_server_names_returns_unique_sorted_names() {
        let registry = ToolRegistry::new();
        registry
            .register_tool(
                "mcp.notion.search_pages".to_string(),
                Arc::new(TestTool {
                    schema: ToolSchema::new("mcp.notion.search_pages", "search", json!({})),
                }),
            )
            .await;
        registry
            .register_tool(
                "mcp.github.list_prs".to_string(),
                Arc::new(TestTool {
                    schema: ToolSchema::new("mcp.github.list_prs", "list", json!({})),
                }),
            )
            .await;
        registry
            .register_tool(
                "mcp.github.get_pr".to_string(),
                Arc::new(TestTool {
                    schema: ToolSchema::new("mcp.github.get_pr", "get", json!({})),
                }),
            )
            .await;

        let servers = registry.mcp_server_names().await;
        assert_eq!(servers, vec!["github".to_string(), "notion".to_string()]);
    }

    #[tokio::test]
    async fn unregister_by_prefix_removes_index_vectors_for_removed_tools() {
        let registry = ToolRegistry::new();
        registry
            .register_tool(
                "mcp.test.search".to_string(),
                Arc::new(TestTool {
                    schema: ToolSchema::new("mcp.test.search", "search", json!({})),
                }),
            )
            .await;
        registry
            .register_tool(
                "mcp.test.get".to_string(),
                Arc::new(TestTool {
                    schema: ToolSchema::new("mcp.test.get", "get", json!({})),
                }),
            )
            .await;

        registry
            .tool_vectors
            .write()
            .await
            .insert("mcp.test.search".to_string(), vec![1.0, 0.0, 0.0]);
        registry
            .tool_vectors
            .write()
            .await
            .insert("mcp.test.get".to_string(), vec![0.0, 1.0, 0.0]);

        let removed = registry.unregister_by_prefix("mcp.test.").await;
        assert_eq!(removed, 2);
        let vectors = registry.tool_vectors.read().await;
        assert!(!vectors.contains_key("mcp.test.search"));
        assert!(!vectors.contains_key("mcp.test.get"));
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

        let malformed = json!({"query":"websearch query</arg_key><arg_value>taj card what is it benefits how to apply</arg_value>"});
        assert_eq!(
            extract_websearch_query(&malformed).as_deref(),
            Some("taj card what is it benefits how to apply")
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
    fn search_backend_defaults_to_searxng_when_configured() {
        let _guard = search_env_lock().lock().expect("env lock");
        clear_search_env();
        std::env::set_var("TANDEM_SEARXNG_URL", "http://localhost:8080");

        let backend = SearchBackend::from_env();

        match backend {
            SearchBackend::Searxng { base_url, .. } => {
                assert_eq!(base_url, "http://localhost:8080");
            }
            other => panic!("expected searxng backend, got {other:?}"),
        }

        clear_search_env();
    }

    #[test]
    fn search_backend_defaults_to_tandem_when_search_url_configured() {
        let _guard = search_env_lock().lock().expect("env lock");
        clear_search_env();
        std::env::set_var("TANDEM_SEARCH_URL", "https://search.tandem.ac");

        let backend = SearchBackend::from_env();

        match backend {
            SearchBackend::Tandem { base_url, .. } => {
                assert_eq!(base_url, "https://search.tandem.ac");
            }
            other => panic!("expected tandem backend, got {other:?}"),
        }

        clear_search_env();
    }

    #[test]
    fn search_backend_explicit_auto_is_supported() {
        let _guard = search_env_lock().lock().expect("env lock");
        clear_search_env();
        std::env::set_var("TANDEM_SEARCH_BACKEND", "auto");
        std::env::set_var("TANDEM_BRAVE_SEARCH_API_KEY", "brave-test-key");
        std::env::set_var("TANDEM_EXA_API_KEY", "exa-test-key");

        let backend = SearchBackend::from_env();

        match backend {
            SearchBackend::Auto { backends } => {
                assert_eq!(backends.len(), 2);
                assert!(matches!(backends[0], SearchBackend::Brave { .. }));
                assert!(matches!(backends[1], SearchBackend::Exa { .. }));
            }
            other => panic!("expected auto backend, got {other:?}"),
        }

        clear_search_env();
    }

    #[test]
    fn search_backend_implicit_auto_failover_when_multiple_backends_are_configured() {
        let _guard = search_env_lock().lock().expect("env lock");
        clear_search_env();
        std::env::set_var("TANDEM_BRAVE_SEARCH_API_KEY", "brave-test-key");
        std::env::set_var("TANDEM_EXA_API_KEY", "exa-test-key");

        let backend = SearchBackend::from_env();

        match backend {
            SearchBackend::Auto { backends } => {
                assert_eq!(backends.len(), 2);
                assert!(matches!(backends[0], SearchBackend::Brave { .. }));
                assert!(matches!(backends[1], SearchBackend::Exa { .. }));
            }
            other => panic!("expected auto backend, got {other:?}"),
        }

        clear_search_env();
    }

    #[test]
    fn search_backend_supports_legacy_exa_env_key() {
        let _guard = search_env_lock().lock().expect("env lock");
        clear_search_env();
        std::env::set_var("TANDEM_SEARCH_BACKEND", "exa");
        std::env::set_var("TANDEM_EXA_SEARCH_API_KEY", "legacy-exa-test-key");

        let backend = SearchBackend::from_env();

        match backend {
            SearchBackend::Exa { api_key, .. } => {
                assert_eq!(api_key, "legacy-exa-test-key");
            }
            other => panic!("expected exa backend, got {other:?}"),
        }

        clear_search_env();
    }

    #[test]
    fn normalize_brave_results_accepts_standard_web_payload_rows() {
        let raw = vec![json!({
            "title": "Agentic workflows",
            "url": "https://example.com/agentic",
            "description": "A practical overview of agentic workflows.",
            "profile": {
                "long_name": "example.com"
            }
        })];

        let results = normalize_brave_results(&raw, 5);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Agentic workflows");
        assert_eq!(results[0].url, "https://example.com/agentic");
        assert_eq!(
            results[0].snippet,
            "A practical overview of agentic workflows."
        );
        assert_eq!(results[0].source, "brave:example.com");
    }

    #[test]
    fn search_backend_explicit_none_disables_websearch() {
        let _guard = search_env_lock().lock().expect("env lock");
        clear_search_env();
        std::env::set_var("TANDEM_SEARCH_BACKEND", "none");
        std::env::set_var("TANDEM_SEARXNG_URL", "http://localhost:8080");

        let backend = SearchBackend::from_env();

        assert!(matches!(backend, SearchBackend::Disabled { .. }));

        clear_search_env();
    }

    #[tokio::test]
    async fn tool_registry_includes_websearch_by_default() {
        let _guard = search_env_lock().lock().expect("env lock");
        clear_search_env();

        let registry = ToolRegistry::new();
        let names = registry
            .list()
            .await
            .into_iter()
            .map(|schema| schema.name)
            .collect::<Vec<_>>();

        assert!(names.iter().any(|name| name == "websearch"));

        clear_search_env();
    }

    #[tokio::test]
    async fn tool_registry_omits_websearch_when_search_backend_explicitly_disabled() {
        let _guard = search_env_lock().lock().expect("env lock");
        clear_search_env();
        std::env::set_var("TANDEM_SEARCH_BACKEND", "none");

        let registry = ToolRegistry::new();
        let names = registry
            .list()
            .await
            .into_iter()
            .map(|schema| schema.name)
            .collect::<Vec<_>>();

        assert!(!names.iter().any(|name| name == "websearch"));

        clear_search_env();
    }

    #[test]
    fn normalize_searxng_results_preserves_title_url_and_engine() {
        let results = normalize_searxng_results(
            &[json!({
                "title": "Tandem Docs",
                "url": "https://docs.tandem.ac/",
                "content": "Official documentation for Tandem.",
                "engine": "duckduckgo"
            })],
            8,
        );

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Tandem Docs");
        assert_eq!(results[0].url, "https://docs.tandem.ac/");
        assert_eq!(results[0].snippet, "Official documentation for Tandem.");
        assert_eq!(results[0].source, "searxng:duckduckgo");
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

    #[test]
    fn memory_scope_defaults_to_hidden_context() {
        let args = json!({
            "__session_id": "session-123",
            "__project_id": "workspace-abc"
        });
        assert_eq!(memory_session_id(&args).as_deref(), Some("session-123"));
        assert_eq!(memory_project_id(&args).as_deref(), Some("workspace-abc"));
        assert!(global_memory_enabled(&args));
    }

    #[test]
    fn memory_scope_policy_can_disable_global_visibility() {
        let args = json!({
            "__session_id": "session-123",
            "__project_id": "workspace-abc",
            "__memory_max_visible_scope": "project"
        });
        assert_eq!(memory_visible_scope(&args), MemoryVisibleScope::Project);
        assert!(!global_memory_enabled(&args));
    }

    #[test]
    fn memory_db_path_ignores_public_db_path_arg() {
        std::env::set_var("TANDEM_MEMORY_DB_PATH", "/tmp/global-memory.sqlite");
        let resolved = resolve_memory_db_path(&json!({
            "db_path": "/home/user123/tandem"
        }));
        assert_eq!(resolved, PathBuf::from("/tmp/global-memory.sqlite"));
        std::env::remove_var("TANDEM_MEMORY_DB_PATH");
    }

    #[test]
    fn memory_db_path_accepts_hidden_override() {
        std::env::set_var("TANDEM_MEMORY_DB_PATH", "/tmp/global-memory.sqlite");
        let resolved = resolve_memory_db_path(&json!({
            "__memory_db_path": "/tmp/internal-memory.sqlite",
            "db_path": "/home/user123/tandem"
        }));
        assert_eq!(resolved, PathBuf::from("/tmp/internal-memory.sqlite"));
        std::env::remove_var("TANDEM_MEMORY_DB_PATH");
    }

    #[tokio::test]
    async fn memory_search_uses_global_by_default() {
        let tool = MemorySearchTool;
        let result = tool
            .execute(json!({
                "query": "global pattern",
                "tier": "global"
            }))
            .await
            .expect("memory_search should return ToolResult");
        assert!(
            result.output.contains("memory database not found")
                || result.output.contains("memory embeddings unavailable")
        );
        assert_eq!(result.metadata["ok"], json!(false));
        let reason = result
            .metadata
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert!(matches!(
            reason,
            "memory_db_missing" | "embeddings_unavailable"
        ));
    }

    #[tokio::test]
    async fn memory_store_uses_hidden_project_scope_by_default() {
        let tool = MemoryStoreTool;
        let result = tool
            .execute(json!({
                "content": "remember this",
                "__session_id": "session-123",
                "__project_id": "workspace-abc"
            }))
            .await
            .expect("memory_store should return ToolResult");
        assert!(
            result.output.contains("memory embeddings unavailable")
                || result.output.contains("memory database not found")
        );
        let reason = result
            .metadata
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert!(matches!(
            reason,
            "embeddings_unavailable" | "memory_db_missing"
        ));
    }

    #[tokio::test]
    async fn memory_delete_uses_hidden_project_scope_by_default() {
        let tool = MemoryDeleteTool;
        let result = tool
            .execute(json!({
                "chunk_id": "chunk-123",
                "__session_id": "session-123",
                "__project_id": "workspace-abc",
                "__memory_db_path": "/tmp/tandem-memory-delete-test.sqlite"
            }))
            .await
            .expect("memory_delete should return ToolResult");
        assert_eq!(result.metadata["tier"], json!("project"));
        assert_eq!(result.metadata["project_id"], json!("workspace-abc"));
        assert!(matches!(
            result
                .metadata
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
            "not_found"
        ));
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
        assert!(resolve_tool_path(
            "<tool_call><function=glob><parameter=pattern>**/*</parameter></function></tool_call>",
            &json!({})
        )
        .is_none());
        assert!(resolve_tool_path("**/*", &json!({})).is_none());
        assert!(resolve_tool_path("/", &json!({})).is_none());
        assert!(resolve_tool_path("C:\\", &json!({})).is_none());
    }

    #[tokio::test]
    async fn glob_allows_tandem_artifact_paths() {
        let root =
            std::env::temp_dir().join(format!("tandem-glob-artifacts-{}", uuid_like(now_ms_u64())));
        let artifacts_dir = root.join(".tandem").join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).expect("create artifacts dir");
        let artifact = artifacts_dir.join("report.json");
        std::fs::write(&artifact, "{\"ok\":true}").expect("write artifact");

        let tool = GlobTool;
        let result = tool
            .execute(json!({
                "pattern": ".tandem/artifacts/*.json",
                "__workspace_root": root.to_string_lossy().to_string(),
                "__effective_cwd": root.to_string_lossy().to_string(),
            }))
            .await
            .expect("glob result");

        assert!(
            result.output.contains(".tandem/artifacts/report.json"),
            "expected artifact path in glob output, got: {}",
            result.output
        );
    }

    #[tokio::test]
    async fn glob_still_hides_non_artifact_tandem_paths() {
        let root =
            std::env::temp_dir().join(format!("tandem-glob-hidden-{}", uuid_like(now_ms_u64())));
        let tandem_dir = root.join(".tandem");
        let artifacts_dir = tandem_dir.join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).expect("create tandem dirs");
        std::fs::write(tandem_dir.join("secrets.json"), "{\"hidden\":true}")
            .expect("write hidden file");

        let tool = GlobTool;
        let result = tool
            .execute(json!({
                "pattern": ".tandem/*.json",
                "__workspace_root": root.to_string_lossy().to_string(),
                "__effective_cwd": root.to_string_lossy().to_string(),
            }))
            .await
            .expect("glob result");

        assert!(
            result.output.trim().is_empty(),
            "expected non-artifact tandem paths to stay hidden, got: {}",
            result.output
        );
    }

    #[test]
    fn normalize_recursive_wildcard_pattern_fixes_common_invalid_forms() {
        assert_eq!(
            normalize_recursive_wildcard_pattern("docs/**.md").as_deref(),
            Some("docs/**/*.md")
        );
        assert_eq!(
            normalize_recursive_wildcard_pattern("src/**README*").as_deref(),
            Some("src/**/README*")
        );
        assert_eq!(
            normalize_recursive_wildcard_pattern("**.{md,mdx,txt}").as_deref(),
            Some("**/*.{md,mdx,txt}")
        );
        assert_eq!(normalize_recursive_wildcard_pattern("docs/**/*.md"), None);
    }

    #[tokio::test]
    async fn glob_recovers_from_invalid_recursive_wildcard_syntax() {
        let root =
            std::env::temp_dir().join(format!("tandem-glob-recover-{}", uuid_like(now_ms_u64())));
        let docs_dir = root.join("docs").join("guides");
        std::fs::create_dir_all(&docs_dir).expect("create docs dir");
        let guide = docs_dir.join("intro.md");
        std::fs::write(&guide, "# intro").expect("write guide");

        let tool = GlobTool;
        let result = tool
            .execute(json!({
                "pattern": "docs/**.md",
                "__workspace_root": root.to_string_lossy().to_string(),
                "__effective_cwd": root.to_string_lossy().to_string(),
            }))
            .await
            .expect("glob result");

        assert!(
            result.output.contains("docs/guides/intro.md"),
            "expected recovered glob output, got: {}",
            result.output
        );
        assert_eq!(
            result.metadata["effective_pattern"],
            json!(format!("{}/docs/**/*.md", root.to_string_lossy()))
        );
    }

    #[cfg(windows)]
    #[test]
    fn path_policy_allows_windows_verbatim_paths_within_workspace() {
        let args = json!({
            "__workspace_root": r"C:\tandem-examples",
            "__effective_cwd": r"C:\tandem-examples\docs"
        });
        assert!(resolve_tool_path(r"\\?\C:\tandem-examples\docs\index.html", &args).is_some());
    }

    #[cfg(not(windows))]
    #[test]
    fn path_policy_allows_absolute_linux_paths_within_workspace() {
        let args = json!({
            "__workspace_root": "/tmp/tandem-examples",
            "__effective_cwd": "/tmp/tandem-examples/docs"
        });
        assert!(resolve_tool_path("/tmp/tandem-examples/docs/index.html", &args).is_some());
        assert!(resolve_tool_path("/etc/passwd", &args).is_none());
    }

    #[test]
    fn read_fallback_resolves_unique_suffix_filename() {
        let root =
            std::env::temp_dir().join(format!("tandem-read-fallback-{}", uuid_like(now_ms_u64())));
        std::fs::create_dir_all(&root).expect("create root");
        let target = root.join("T1011U kitöltési útmutató.pdf");
        std::fs::write(&target, b"stub").expect("write test file");

        let args = json!({
            "__workspace_root": root.to_string_lossy().to_string(),
            "__effective_cwd": root.to_string_lossy().to_string()
        });
        let resolved = resolve_read_path_fallback("útmutató.pdf", &args)
            .expect("expected unique suffix match");
        assert_eq!(resolved, target);

        let _ = std::fs::remove_dir_all(&root);
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

    #[test]
    fn sanitize_member_name_normalizes_agent_aliases() {
        assert_eq!(sanitize_member_name("A2").expect("valid"), "A2");
        assert_eq!(sanitize_member_name("a7").expect("valid"), "A7");
        assert_eq!(
            sanitize_member_name("  qa reviewer ").expect("valid"),
            "qa-reviewer"
        );
        assert!(sanitize_member_name("   ").is_err());
    }

    #[tokio::test]
    async fn next_default_member_name_skips_existing_indices() {
        let root = std::env::temp_dir().join(format!(
            "tandem-agent-team-test-{}",
            uuid_like(now_ms_u64())
        ));
        let paths = AgentTeamPaths::new(root.join(".tandem"));
        let team_name = "alpha";
        fs::create_dir_all(paths.team_dir(team_name))
            .await
            .expect("create team dir");
        write_json_file(
            paths.members_file(team_name),
            &json!([
                {"name":"A1"},
                {"name":"A2"},
                {"name":"agent-x"},
                {"name":"A5"}
            ]),
        )
        .await
        .expect("write members");

        let next = next_default_member_name(&paths, team_name)
            .await
            .expect("next member");
        assert_eq!(next, "A6");

        let _ =
            fs::remove_dir_all(PathBuf::from(paths.root().parent().unwrap_or(paths.root()))).await;
    }
}

async fn find_symbol_references(symbol: &str, root: &Path) -> String {
    if symbol.trim().is_empty() {
        return "missing symbol".to_string();
    }
    let escaped = regex::escape(symbol);
    let re = Regex::new(&format!(r"\b{}\b", escaped));
    let Ok(re) = re else {
        return "invalid symbol".to_string();
    };
    let mut refs = Vec::new();
    for entry in WalkBuilder::new(root).build().flatten() {
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
