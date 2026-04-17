impl GrepSearchState {
    fn new(
        cancel: CancellationToken,
        limit: usize,
        chunk_size: usize,
        progress: Option<SharedToolProgressSink>,
    ) -> Self {
        Self {
            hits: Mutex::new(Vec::new()),
            hit_count: AtomicUsize::new(0),
            stop: AtomicBool::new(false),
            cancel,
            limit,
            chunk_size,
            progress,
        }
    }

    fn should_stop(&self) -> bool {
        self.stop.load(AtomicOrdering::Acquire) || self.cancel.is_cancelled()
    }

    fn reserve_hit(&self) -> Option<usize> {
        if self.should_stop() {
            return None;
        }
        match self.hit_count.fetch_update(
            AtomicOrdering::AcqRel,
            AtomicOrdering::Acquire,
            |current| (current < self.limit).then_some(current + 1),
        ) {
            Ok(previous) => {
                let ordinal = previous + 1;
                if ordinal >= self.limit {
                    self.stop.store(true, AtomicOrdering::Release);
                }
                Some(ordinal)
            }
            Err(_) => {
                self.stop.store(true, AtomicOrdering::Release);
                None
            }
        }
    }

    fn push_hit(&self, hit: GrepHit) {
        if let Ok(mut hits) = self.hits.lock() {
            hits.push(hit);
        }
    }

    fn sorted_hits(&self) -> Vec<GrepHit> {
        let mut hits = self
            .hits
            .lock()
            .map(|hits| hits.clone())
            .unwrap_or_default();
        hits.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then_with(|| a.line.cmp(&b.line))
                .then_with(|| a.text.cmp(&b.text))
                .then_with(|| a.ordinal.cmp(&b.ordinal))
        });
        hits
    }
}

struct GrepParallelVisitorBuilder {
    matcher: Arc<RegexMatcher>,
    state: Arc<GrepSearchState>,
    tool: String,
}

struct GrepParallelVisitor {
    matcher: Arc<RegexMatcher>,
    state: Arc<GrepSearchState>,
    searcher: grep_searcher::Searcher,
    tool: String,
}

impl<'s> ParallelVisitorBuilder<'s> for GrepParallelVisitorBuilder {
    fn build(&mut self) -> Box<dyn ParallelVisitor + 's> {
        Box::new(GrepParallelVisitor {
            matcher: Arc::clone(&self.matcher),
            state: Arc::clone(&self.state),
            searcher: build_grep_searcher(),
            tool: self.tool.clone(),
        })
    }
}

impl ParallelVisitor for GrepParallelVisitor {
    fn visit(&mut self, entry: Result<ignore::DirEntry, ignore::Error>) -> WalkState {
        if self.state.should_stop() {
            return WalkState::Quit;
        }
        let Ok(entry) = entry else {
            return WalkState::Continue;
        };
        if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            return WalkState::Continue;
        }
        let path = entry.path();
        if is_discovery_ignored_path(path) {
            return WalkState::Continue;
        }
        let Ok(file) = std::fs::File::open(path) else {
            return WalkState::Continue;
        };
        let path_display = path.display().to_string();
        let state = Arc::clone(&self.state);
        let progress = state.progress.clone();
        let tool = self.tool.clone();
        let mut pending_chunk = Vec::with_capacity(state.chunk_size);
        let _ = self.searcher.search_file(
            &*self.matcher,
            &file,
            Lossy(|line_number, line| {
                if state.should_stop() {
                    return Ok(false);
                }
                let Some(ordinal) = state.reserve_hit() else {
                    return Ok(false);
                };
                let line = line.trim_end_matches(['\r', '\n']);
                let hit = GrepHit {
                    path: path_display.clone(),
                    line: line_number as usize,
                    text: line.to_string(),
                    ordinal,
                };
                state.push_hit(hit.clone());
                pending_chunk.push(hit);
                if pending_chunk.len() >= state.chunk_size {
                    emit_grep_progress_chunk(progress.as_ref(), &tool, &pending_chunk);
                    pending_chunk.clear();
                }
                if state.should_stop() {
                    return Ok(false);
                }
                Ok(true)
            }),
        );
        emit_grep_progress_chunk(progress.as_ref(), &tool, &pending_chunk);
        if state.should_stop() {
            WalkState::Quit
        } else {
            WalkState::Continue
        }
    }
}

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
        let pattern = args["pattern"].as_str().unwrap_or("");
        let root = args["path"].as_str().unwrap_or(".");
        let Some(root_path) = resolve_walk_root(root, &args) else {
            return Ok(sandbox_path_denied_result(root, &args));
        };
        let matcher = build_grep_matcher(pattern)?;
        let state = Arc::new(GrepSearchState::new(
            cancel.clone(),
            100,
            8,
            progress.clone(),
        ));
        let mut builder = GrepParallelVisitorBuilder {
            matcher: Arc::new(matcher),
            state: Arc::clone(&state),
            tool: "grep".to_string(),
        };
        WalkBuilder::new(&root_path)
            .build_parallel()
            .visit(&mut builder);
        let out = state.sorted_hits();
        let limit_reached = out.len() >= 100;
        emit_grep_progress_done(
            progress.as_ref(),
            "grep",
            &root_path,
            out.len(),
            limit_reached,
            cancel.is_cancelled(),
        );
        Ok(ToolResult {
            output: out
                .iter()
                .map(|hit| format!("{}:{}:{}", hit.path, hit.line, hit.text))
                .collect::<Vec<_>>()
                .join("\n"),
            metadata: json!({
                "count": out.len(),
                "path": root_path.to_string_lossy(),
                "truncated": limit_reached,
                "cancelled": cancel.is_cancelled(),
                "streaming": progress.is_some()
            }),
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
                metadata,
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
        .unwrap_or(raw_results.len() > results.len());
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
