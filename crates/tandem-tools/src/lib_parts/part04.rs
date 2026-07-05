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
        tool_schema_with_capabilities(
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
            memory_search_capabilities(),
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
        let channel_gateway = channel_memory_gateway_context(&args, session_id.clone(), project_id.clone());
        let allow_global = if channel_gateway.is_some() {
            false
        } else {
            global_memory_enabled(&args)
        };
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
        if channel_gateway.is_some() && (allow_global || matches!(tier, Some(MemoryTier::Global))) {
            return Ok(ToolResult {
                output: "channel memory_search cannot read global memory".to_string(),
                metadata: json!({"ok": false, "reason": "channel_global_scope_blocked"}),
            });
        }
        if matches!(tier, Some(MemoryTier::Global)) && !allow_global {
            return Ok(ToolResult {
                output: "tier=global requires allow_global=true".to_string(),
                metadata: json!({"ok": false, "reason": "global_scope_disabled"}),
            });
        }

        let mut limit = args
            .get("limit")
            .and_then(|v| v.as_i64())
            .unwrap_or(5)
            .clamp(1, 20);
        if channel_gateway.is_some() {
            limit = limit.min(CHANNEL_MEMORY_MAX_TOP_K as i64);
        }
        if let Some(gateway) = channel_gateway.as_ref() {
            if let Some(reason) = suspicious_channel_memory_query_reason(query) {
                return Ok(ToolResult {
                    output: reason.to_string(),
                    metadata: json!({
                        "ok": false,
                        "reason": "channel_query_pattern_blocked",
                        "detail": reason,
                        "retrieval_gateway": gateway.audit_value(),
                    }),
                });
            }
            if !consume_channel_memory_query_budget(gateway) {
                return Ok(ToolResult {
                    output: "channel memory_search query budget exhausted".to_string(),
                    metadata: json!({
                        "ok": false,
                        "reason": "channel_query_budget_exhausted",
                        "retrieval_gateway": gateway.audit_value(),
                    }),
                });
            }
        }

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
        if let Some(gateway) = channel_gateway.as_ref() {
            merged.retain(|result| channel_gateway_allows_chunk(gateway, &result.chunk));
        }
        merged.sort_by(|a, b| b.similarity.total_cmp(&a.similarity));
        merged.truncate(limit as usize);
        let gateway_budget_exhausted = if let Some(gateway) = channel_gateway.as_ref() {
            apply_channel_memory_result_budget(gateway, &mut merged)
        } else {
            false
        };

        let output_rows = merged
            .iter()
            .map(|item| {
                let trust_label = channel_memory_trust_label(item.chunk.metadata.as_ref());
                json!({
                    "chunk_id": item.chunk.id,
                    "tier": item.chunk.tier.to_string(),
                    "session_id": item.chunk.session_id,
                    "project_id": item.chunk.project_id,
                    "source": item.chunk.source,
                    "similarity": item.similarity,
                    "content": item.chunk.content,
                    "created_at": item.chunk.created_at,
                    "memory_trust": channel_memory_trust_payload(trust_label),
                    "rendering_role": channel_memory_rendering_role(trust_label),
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
                "retrieval_gateway": channel_gateway.as_ref().map(ChannelMemoryGatewayContext::audit_value),
                "gateway_budget_exhausted": gateway_budget_exhausted,
            }),
        })
    }
}

const CHANNEL_MEMORY_QUERY_WINDOW_MS: u64 = 5 * 60 * 1000;
const CHANNEL_MEMORY_MAX_QUERIES_PER_WINDOW: u32 = 10;
const CHANNEL_MEMORY_MAX_TOP_K: usize = 5;
const CHANNEL_MEMORY_MAX_TOKENS: i64 = 200;
const CHANNEL_MEMORY_MAX_CHARS: usize = 1000;
const CHANNEL_MEMORY_MAX_RESULTS_PER_WINDOW: u32 = 20;
const CHANNEL_MEMORY_MAX_TOKENS_PER_WINDOW: i64 = 800;
const CHANNEL_MEMORY_MAX_CHARS_PER_WINDOW: usize = 4_000;

#[derive(Debug, Clone)]
struct ChannelMemoryGatewayContext {
    platform: String,
    user_id: String,
    scope_id: String,
    session_id: Option<String>,
    project_id: Option<String>,
}

impl ChannelMemoryGatewayContext {
    fn budget_key(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}",
            self.platform,
            self.scope_id,
            self.user_id,
            self.session_id.as_deref().unwrap_or("*"),
            self.project_id.as_deref().unwrap_or("*")
        )
    }

    fn audit_value(&self) -> Value {
        json!({
            "grant_id": format!(
                "channel-{}-{}-{}",
                sanitize_memory_gateway_segment(&self.platform),
                sanitize_memory_gateway_segment(&self.scope_id),
                sanitize_memory_gateway_segment(&self.user_id)
            ),
            "subject": format!("channel:{}:{}", self.platform, self.user_id),
            "channel": self.platform,
            "user_id": self.user_id,
            "scope_id": self.scope_id,
            "session_id": self.session_id,
            "project_id": self.project_id,
            "data_classes": ["public", "internal"],
            "budgets": {
                "max_queries_per_window": CHANNEL_MEMORY_MAX_QUERIES_PER_WINDOW,
                "window_ms": CHANNEL_MEMORY_QUERY_WINDOW_MS,
                "max_top_k": CHANNEL_MEMORY_MAX_TOP_K,
                "max_tokens": CHANNEL_MEMORY_MAX_TOKENS,
                "max_chars": CHANNEL_MEMORY_MAX_CHARS,
                "max_results_per_window": CHANNEL_MEMORY_MAX_RESULTS_PER_WINDOW,
                "max_tokens_per_window": CHANNEL_MEMORY_MAX_TOKENS_PER_WINDOW,
                "max_chars_per_window": CHANNEL_MEMORY_MAX_CHARS_PER_WINDOW,
            }
        })
    }
}

fn channel_memory_gateway_context(
    args: &Value,
    session_id: Option<String>,
    project_id: Option<String>,
) -> Option<ChannelMemoryGatewayContext> {
    let platform = hidden_arg_string(args, "__channel_platform")?;
    let user_id = hidden_arg_string(args, "__channel_user_id")?;
    let scope_id = hidden_arg_string(args, "__channel_scope_id")?;
    Some(ChannelMemoryGatewayContext {
        platform,
        user_id,
        scope_id,
        session_id,
        project_id,
    })
}

fn hidden_arg_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

#[derive(Debug, Clone, Copy)]
struct ChannelMemoryBudgetWindow {
    started_at_ms: u64,
    query_count: u32,
    result_count: u32,
    token_count: i64,
    char_count: usize,
}

impl ChannelMemoryBudgetWindow {
    fn new(started_at_ms: u64) -> Self {
        Self {
            started_at_ms,
            query_count: 0,
            result_count: 0,
            token_count: 0,
            char_count: 0,
        }
    }

    fn reset(&mut self, started_at_ms: u64) {
        *self = Self::new(started_at_ms);
    }
}

fn channel_memory_budget_windows() -> &'static Mutex<HashMap<String, ChannelMemoryBudgetWindow>> {
    static WINDOWS: OnceLock<Mutex<HashMap<String, ChannelMemoryBudgetWindow>>> = OnceLock::new();
    WINDOWS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn suspicious_channel_memory_query_reason(query: &str) -> Option<&'static str> {
    let lowered = query.trim().to_ascii_lowercase();
    let broad_terms = [
        "dump",
        "everything",
        "entire memory",
        "all memory",
        "all documents",
        "all records",
        "full database",
        "bulk",
    ];
    if broad_terms.iter().any(|term| lowered.contains(term)) {
        return Some("channel memory_search blocked broad export query");
    }
    let export_patterns = [
        "export all",
        "export everything",
        "export entire",
        "export the entire",
        "export full",
        "export the full",
        "bulk export",
    ];
    if export_patterns
        .iter()
        .any(|pattern| lowered.contains(pattern))
    {
        return Some("channel memory_search blocked broad export query");
    }
    let starts = ["list all", "show all", "give me all", "print all", "return all"];
    if starts.iter().any(|term| lowered.starts_with(term)) {
        return Some("channel memory_search blocked broad enumeration query");
    }
    None
}

fn consume_channel_memory_query_budget(gateway: &ChannelMemoryGatewayContext) -> bool {
    let now = now_ms_u64();
    let key = gateway.budget_key();
    let mut guard = channel_memory_budget_windows()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let window = guard
        .entry(key)
        .or_insert_with(|| ChannelMemoryBudgetWindow::new(now));
    if now.saturating_sub(window.started_at_ms) >= CHANNEL_MEMORY_QUERY_WINDOW_MS {
        window.reset(now);
    }
    if window.query_count >= CHANNEL_MEMORY_MAX_QUERIES_PER_WINDOW {
        return false;
    }
    window.query_count = window.query_count.saturating_add(1);
    true
}

fn channel_gateway_allows_chunk(
    gateway: &ChannelMemoryGatewayContext,
    chunk: &tandem_memory::types::MemoryChunk,
) -> bool {
    channel_gateway_allows_memory_metadata(
        gateway,
        chunk.project_id.as_deref(),
        chunk.metadata.as_ref(),
    )
}

fn channel_gateway_allows_memory_metadata(
    gateway: &ChannelMemoryGatewayContext,
    project_id: Option<&str>,
    metadata: Option<&Value>,
) -> bool {
    if let Some(project_id) = project_id {
        if Some(project_id) != gateway.project_id.as_deref() {
            return false;
        }
    }
    matches!(memory_data_class_label(metadata), "public" | "internal")
}

fn memory_data_class_label(metadata: Option<&Value>) -> &str {
    metadata
        .and_then(|metadata| {
            metadata
                .get("enterprise_source_binding")
                .and_then(|binding| binding.get("data_class"))
                .or_else(|| metadata.get("classification"))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("internal")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChannelMemoryTrustLabel {
    ExternalUserSupplied,
    ConnectorSourced,
    Verified,
    HumanApproved,
    SystemGenerated,
}

impl ChannelMemoryTrustLabel {
    fn as_str(self) -> &'static str {
        match self {
            Self::ExternalUserSupplied => "external_user_supplied",
            Self::ConnectorSourced => "connector_sourced",
            Self::Verified => "verified",
            Self::HumanApproved => "human_approved",
            Self::SystemGenerated => "system_generated",
        }
    }

    fn trusted_for_promotion(self) -> bool {
        matches!(self, Self::Verified | Self::HumanApproved | Self::SystemGenerated)
    }
}

fn channel_memory_trust_label(metadata: Option<&Value>) -> ChannelMemoryTrustLabel {
    match metadata
        .and_then(|metadata| metadata.get("memory_trust"))
        .and_then(|trust| trust.get("label"))
        .and_then(Value::as_str)
    {
        Some("external_user_supplied") => ChannelMemoryTrustLabel::ExternalUserSupplied,
        Some("connector_sourced") => ChannelMemoryTrustLabel::ConnectorSourced,
        Some("verified") => ChannelMemoryTrustLabel::Verified,
        Some("human_approved") => ChannelMemoryTrustLabel::HumanApproved,
        _ => ChannelMemoryTrustLabel::SystemGenerated,
    }
}

fn channel_memory_rendering_role(label: ChannelMemoryTrustLabel) -> &'static str {
    if label.trusted_for_promotion() {
        "context"
    } else {
        "evidence"
    }
}

fn channel_memory_trust_payload(label: ChannelMemoryTrustLabel) -> Value {
    json!({
        "label": label.as_str(),
        "trusted_for_promotion": label.trusted_for_promotion(),
        "rendering_role": channel_memory_rendering_role(label),
    })
}

fn apply_channel_memory_result_budget(
    gateway: &ChannelMemoryGatewayContext,
    results: &mut Vec<MemorySearchResult>,
) -> bool {
    let mut chars_used = 0usize;
    let mut tokens_used = 0i64;
    let original_len = results.len();
    results.retain(|result| {
        let char_count = result.chunk.content.chars().count();
        let token_count = result.chunk.content.split_whitespace().count() as i64;
        if chars_used.saturating_add(char_count) > CHANNEL_MEMORY_MAX_CHARS
            || tokens_used.saturating_add(token_count) > CHANNEL_MEMORY_MAX_TOKENS
        {
            return false;
        }
        chars_used = chars_used.saturating_add(char_count);
        tokens_used = tokens_used.saturating_add(token_count);
        true
    });
    let response_budget_exhausted = results.len() < original_len;
    let mut window_budget_exhausted = false;
    let now = now_ms_u64();
    let key = gateway.budget_key();
    let mut guard = channel_memory_budget_windows()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let window = guard
        .entry(key)
        .or_insert_with(|| ChannelMemoryBudgetWindow::new(now));
    if now.saturating_sub(window.started_at_ms) >= CHANNEL_MEMORY_QUERY_WINDOW_MS {
        window.reset(now);
    }
    results.retain(|result| {
        let char_count = result.chunk.content.chars().count();
        let token_count = result.chunk.content.split_whitespace().count() as i64;
        if window.result_count >= CHANNEL_MEMORY_MAX_RESULTS_PER_WINDOW
            || window.token_count.saturating_add(token_count) > CHANNEL_MEMORY_MAX_TOKENS_PER_WINDOW
            || window.char_count.saturating_add(char_count) > CHANNEL_MEMORY_MAX_CHARS_PER_WINDOW
        {
            window_budget_exhausted = true;
            return false;
        }
        window.result_count = window.result_count.saturating_add(1);
        window.token_count = window.token_count.saturating_add(token_count);
        window.char_count = window.char_count.saturating_add(char_count);
        true
    });
    response_budget_exhausted || window_budget_exhausted
}

fn sanitize_memory_gateway_segment(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "unknown".to_string()
    } else {
        sanitized
    }
}

struct MemoryStoreTool;
#[async_trait]
impl Tool for MemoryStoreTool {
    fn schema(&self) -> ToolSchema {
        tool_schema_with_capabilities(
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
            memory_write_capabilities(),
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

        if is_channel_tool_context(&args) && matches!(tier, MemoryTier::Global) {
            return Ok(ToolResult {
                output: "channel memory_store cannot write global memory".to_string(),
                metadata: json!({"ok": false, "reason": "channel_global_scope_blocked"}),
            });
        }
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
            tenant_scope: tandem_memory::types::MemoryTenantScope::local(),
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
        tool_schema_with_capabilities(
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
            memory_read_capabilities(),
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

        chunks.sort_by_key(|chunk| std::cmp::Reverse(chunk.created_at));
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
        tool_schema_with_capabilities(
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
            memory_delete_capabilities(),
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

/// True when the engine injected a trusted channel identity into the tool args.
/// `__channel_scope_id` is only present for sessions whose `source_kind` is
/// `channel` (see `engine_loop` tool-arg injection), so it is a reliable marker
/// that the caller is a channel model whose scope must be pinned by the engine.
fn is_channel_tool_context(args: &Value) -> bool {
    hidden_arg_string(args, "__channel_scope_id").is_some()
}

fn trimmed_arg_string(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

fn memory_session_id(args: &Value) -> Option<String> {
    // For channel sessions the effective scope must come from the engine-injected
    // `__session_id`, never a model-supplied `session_id`; otherwise a
    // prompt-injected tool call could read/write another chat's memory (TAN-603).
    if is_channel_tool_context(args) {
        return trimmed_arg_string(args, "__session_id");
    }
    trimmed_arg_string(args, "session_id").or_else(|| trimmed_arg_string(args, "__session_id"))
}

fn memory_project_id(args: &Value) -> Option<String> {
    // Channel sessions are pinned to the engine-injected `__project_id` (the
    // trusted `channel-public::…` scope key derived from the channel scope id),
    // ignoring any model-supplied `project_id` override (TAN-603).
    if is_channel_tool_context(args) {
        return trimmed_arg_string(args, "__project_id");
    }
    trimmed_arg_string(args, "project_id").or_else(|| trimmed_arg_string(args, "__project_id"))
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
