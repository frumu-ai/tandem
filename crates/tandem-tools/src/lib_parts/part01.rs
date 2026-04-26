use std::collections::{hash_map::DefaultHasher, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::anyhow;
use async_trait::async_trait;
use grep_matcher::LineTerminator;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::sinks::Lossy;
use grep_searcher::{BinaryDetection, MmapChoice, SearcherBuilder};
use ignore::{ParallelVisitor, ParallelVisitorBuilder, WalkBuilder, WalkState};
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
use tandem_types::{SharedToolProgressSink, ToolProgressEvent, ToolResult, ToolSchema};

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
    async fn execute_with_progress(
        &self,
        args: Value,
        cancel: CancellationToken,
        progress: Option<SharedToolProgressSink>,
    ) -> anyhow::Result<ToolResult> {
        let _ = progress;
        self.execute_with_cancel(args, cancel).await
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
        // `websearch` stays registered and resolves the live managed settings on demand so
        // control-panel changes take effect without restarting tandem-engine.
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
        self.execute_with_cancel_and_progress(name, args, cancel, None)
            .await
    }

    pub async fn execute_with_cancel_and_progress(
        &self,
        name: &str,
        args: Value,
        cancel: CancellationToken,
        progress: Option<SharedToolProgressSink>,
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
        tool.execute_with_progress(args, cancel, progress).await
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
        let managed_env = load_managed_search_env();
        let explicit = search_setting_value(&managed_env, &["TANDEM_SEARCH_BACKEND"])
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty());
        let timeout_ms = search_backend_timeout_ms(&managed_env);

        match explicit.as_deref() {
            Some("none") | Some("disabled") => {
                return Self::Disabled {
                    reason: "TANDEM_SEARCH_BACKEND explicitly disabled websearch".to_string(),
                };
            }
            Some("auto") => {
                return search_backend_from_auto_env(&managed_env, timeout_ms);
            }
            Some("tandem") => {
                return search_backend_from_tandem_env(&managed_env, timeout_ms, true);
            }
            Some("searxng") => {
                return search_backend_from_searxng_env(&managed_env, timeout_ms).unwrap_or_else(
                    || Self::Disabled {
                        reason: "TANDEM_SEARCH_BACKEND=searxng but TANDEM_SEARXNG_URL is missing"
                            .to_string(),
                    },
                );
            }
            Some("exa") => {
                return search_backend_from_exa_env(&managed_env, timeout_ms).unwrap_or_else(|| {
                    Self::Disabled {
                        reason:
                            "TANDEM_SEARCH_BACKEND=exa but EXA_API_KEY/TANDEM_EXA_API_KEY is missing"
                                .to_string(),
                    }
                });
            }
            Some("brave") => {
                return search_backend_from_brave_env(&managed_env, timeout_ms).unwrap_or_else(|| {
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
        search_backend_from_auto_env(&managed_env, timeout_ms)
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

const DEFAULT_MANAGED_SEARCH_SETTINGS_PATH: &str = "/etc/tandem/engine.env";

fn managed_search_settings_path() -> PathBuf {
    std::env::var("TANDEM_SEARCH_SETTINGS_FILE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_MANAGED_SEARCH_SETTINGS_PATH))
}

fn load_managed_search_env() -> HashMap<String, String> {
    let path = managed_search_settings_path();
    let Ok(raw) = std::fs::read_to_string(path) else {
        return HashMap::new();
    };

    let mut env = HashMap::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }
        let mut value = value.trim().to_string();
        if ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
            && value.len() >= 2
        {
            value = value[1..value.len() - 1].to_string();
        }
        env.insert(key.to_string(), value);
    }
    env
}

fn search_setting_value(file_env: &HashMap<String, String>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = file_env.get(*key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    for key in keys {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn has_nonempty_search_setting(file_env: &HashMap<String, String>, name: &str) -> bool {
    search_setting_value(file_env, &[name]).is_some()
}

fn search_backend_timeout_ms(file_env: &HashMap<String, String>) -> u64 {
    search_setting_value(file_env, &["TANDEM_SEARCH_TIMEOUT_MS"])
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(10_000)
        .clamp(1_000, 120_000)
}

fn search_backend_from_tandem_env(
    file_env: &HashMap<String, String>,
    timeout_ms: u64,
    allow_default_url: bool,
) -> SearchBackend {
    const DEFAULT_TANDEM_SEARCH_URL: &str = "https://search.tandem.ac";
    let base_url = search_setting_value(file_env, &["TANDEM_SEARCH_URL"])
        .map(|value| value.trim().trim_end_matches('/').to_string())
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

fn search_backend_from_searxng_env(
    file_env: &HashMap<String, String>,
    timeout_ms: u64,
) -> Option<SearchBackend> {
    let base_url = search_setting_value(file_env, &["TANDEM_SEARXNG_URL"])?;
    let base_url = base_url.trim().trim_end_matches('/').to_string();
    if base_url.is_empty() {
        return None;
    }
    let engines = search_setting_value(file_env, &["TANDEM_SEARXNG_ENGINES"])
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    Some(SearchBackend::Searxng {
        base_url,
        engines,
        timeout_ms,
    })
}

fn search_backend_from_exa_env(
    file_env: &HashMap<String, String>,
    timeout_ms: u64,
) -> Option<SearchBackend> {
    let api_key = search_setting_value(
        file_env,
        &[
            "TANDEM_EXA_API_KEY",
            "TANDEM_EXA_SEARCH_API_KEY",
            "EXA_API_KEY",
        ],
    )?;
    let api_key = api_key.trim().to_string();
    if api_key.is_empty() {
        return None;
    }
    Some(SearchBackend::Exa {
        api_key,
        timeout_ms,
    })
}

fn search_backend_from_brave_env(
    file_env: &HashMap<String, String>,
    timeout_ms: u64,
) -> Option<SearchBackend> {
    let api_key = search_setting_value(
        file_env,
        &["TANDEM_BRAVE_SEARCH_API_KEY", "BRAVE_SEARCH_API_KEY"],
    )?;
    let api_key = api_key.trim().to_string();
    if api_key.is_empty() {
        return None;
    }
    Some(SearchBackend::Brave {
        api_key,
        timeout_ms,
    })
}

fn search_backend_auto_candidates(
    file_env: &HashMap<String, String>,
    timeout_ms: u64,
) -> Vec<SearchBackend> {
    let mut backends = Vec::new();

    if has_nonempty_search_setting(file_env, "TANDEM_SEARCH_URL") {
        backends.push(search_backend_from_tandem_env(file_env, timeout_ms, false));
    }
    if let Some(config) = search_backend_from_searxng_env(file_env, timeout_ms) {
        backends.push(config);
    }
    if let Some(config) = search_backend_from_brave_env(file_env, timeout_ms) {
        backends.push(config);
    }
    if let Some(config) = search_backend_from_exa_env(file_env, timeout_ms) {
        backends.push(config);
    }
    if backends.is_empty() {
        backends.push(search_backend_from_tandem_env(file_env, timeout_ms, true));
    }

    backends
        .into_iter()
        .filter(|backend| !matches!(backend, SearchBackend::Disabled { .. }))
        .collect()
}

fn search_backend_from_auto_env(
    file_env: &HashMap<String, String>,
    timeout_ms: u64,
) -> SearchBackend {
    let backends = search_backend_auto_candidates(file_env, timeout_ms);
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

#[derive(Debug, Clone)]
struct GrepHit {
    path: String,
    line: usize,
    text: String,
    ordinal: usize,
}

fn grep_hit_to_value(hit: &GrepHit) -> Value {
    json!({
        "path": hit.path,
        "line": hit.line,
        "text": hit.text,
        "ordinal": hit.ordinal,
    })
}

fn emit_grep_progress_chunk(
    progress: Option<&SharedToolProgressSink>,
    tool: &str,
    hits: &[GrepHit],
) {
    let Some(progress) = progress else {
        return;
    };
    if hits.is_empty() {
        return;
    }
    progress.publish(ToolProgressEvent::new(
        "tool.search.chunk",
        json!({
            "tool": tool,
            "hits": hits.iter().map(grep_hit_to_value).collect::<Vec<_>>(),
        }),
    ));
}

fn emit_grep_progress_done(
    progress: Option<&SharedToolProgressSink>,
    tool: &str,
    path: &Path,
    total_hits: usize,
    truncated: bool,
    cancelled: bool,
) {
    let Some(progress) = progress else {
        return;
    };
    progress.publish(ToolProgressEvent::new(
        "tool.search.done",
        json!({
            "tool": tool,
            "path": path.to_string_lossy(),
            "count": total_hits,
            "truncated": truncated,
            "cancelled": cancelled,
        }),
    ));
}

struct GrepSearchState {
    hits: Mutex<Vec<GrepHit>>,
    hit_count: AtomicUsize,
    stop: AtomicBool,
    cancel: CancellationToken,
    limit: usize,
    chunk_size: usize,
    progress: Option<SharedToolProgressSink>,
}
