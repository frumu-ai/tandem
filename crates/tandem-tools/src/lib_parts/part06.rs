#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex, OnceLock};
    use tandem_types::ToolProgressSink;
    use tempfile::TempDir;
    use tokio::fs;
    use tokio_util::sync::CancellationToken;

    #[derive(Clone, Default)]
    struct RecordingProgressSink {
        events: Arc<Mutex<Vec<ToolProgressEvent>>>,
    }

    impl ToolProgressSink for RecordingProgressSink {
        fn publish(&self, event: ToolProgressEvent) {
            self.events.lock().expect("progress lock").push(event);
        }
    }

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
    async fn grep_tool_streams_chunk_and_done_events() {
        let tempdir = TempDir::new().expect("tempdir");
        let root = tempdir.path();
        let first = root.join("a.txt");
        let second = root.join("b.txt");

        std::fs::write(
            &first,
            [
                "needle a1",
                "needle a2",
                "needle a3",
                "needle a4",
                "needle a5",
                "needle a6",
            ]
            .join("\n"),
        )
        .expect("write first file");
        std::fs::write(
            &second,
            [
                "needle b1",
                "needle b2",
                "needle b3",
                "needle b4",
                "needle b5",
                "needle b6",
            ]
            .join("\n"),
        )
        .expect("write second file");

        let sink = RecordingProgressSink::default();
        let events = Arc::clone(&sink.events);
        let progress: SharedToolProgressSink = Arc::new(sink);

        let tool = GrepTool;
        let result = tool
            .execute_with_progress(
                grep_args(root, "needle"),
                CancellationToken::new(),
                Some(progress),
            )
            .await
            .expect("grep result");

        assert_eq!(result.metadata["count"], json!(12));
        let lines = result.output.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 12);
        assert!(lines[0].starts_with(&first.display().to_string()));
        assert!(lines[11].starts_with(&second.display().to_string()));

        let events = events.lock().expect("events").clone();
        assert!(!events.is_empty());
        assert!(events
            .iter()
            .any(|event| event.event_type == "tool.search.chunk"));
        let done = events
            .iter()
            .rev()
            .find(|event| event.event_type == "tool.search.done")
            .expect("done event");
        assert_eq!(done.properties["count"], json!(12));
        assert_eq!(done.properties["tool"], json!("grep"));
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
