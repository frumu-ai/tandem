#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    fn cfg(
        provider_ids: &[&str],
        default_provider: Option<&str>,
        include_openai_key: bool,
    ) -> AppConfig {
        let mut providers = HashMap::new();
        for id in provider_ids {
            let api_key = if *id == "openai" && include_openai_key {
                Some("sk-test".to_string())
            } else {
                None
            };
            providers.insert(
                (*id).to_string(),
                ProviderConfig {
                    api_key,
                    url: None,
                    default_model: Some(format!("{id}-model")),
                },
            );
        }
        AppConfig {
            providers,
            default_provider: default_provider.map(|s| s.to_string()),
        }
    }

    #[tokio::test]
    async fn explicit_provider_wins_over_default_provider() {
        let registry = ProviderRegistry::new(cfg(&["openai", "openrouter"], Some("openai"), true));
        let provider = registry
            .select_provider(Some("openrouter"))
            .await
            .expect("provider");
        assert_eq!(provider.info().id, "openrouter");
    }

    #[tokio::test]
    async fn uses_default_provider_when_explicit_provider_missing() {
        let registry =
            ProviderRegistry::new(cfg(&["openai", "openrouter"], Some("openrouter"), true));
        let provider = registry.select_provider(None).await.expect("provider");
        assert_eq!(provider.info().id, "openrouter");
    }

    #[tokio::test]
    async fn falls_back_to_first_provider_when_default_provider_missing() {
        let registry = ProviderRegistry::new(cfg(&["openai"], Some("anthropic"), true));
        let provider = registry.select_provider(None).await.expect("provider");
        assert_eq!(provider.info().id, "openai");
    }

    #[tokio::test]
    async fn explicit_unknown_provider_errors() {
        let registry = ProviderRegistry::new(cfg(&["openai"], None, true));
        let err = registry
            .select_provider(Some("openruter"))
            .await
            .err()
            .expect("expected error");
        assert!(err
            .to_string()
            .contains("provider `openruter` is not configured"));
    }

    #[tokio::test]
    async fn codex_auth_overrides_are_task_scoped_and_tenant_isolated() {
        let registry = ProviderRegistry::new(cfg(&["openai-codex"], None, false));
        let tenant_a = TenantContext::explicit("org-a", "workspace-a", None);
        let tenant_b = TenantContext::explicit("org-b", "workspace-b", None);
        let tenant_missing = TenantContext::explicit("org-c", "workspace-c", None);
        registry
            .set_tenant_provider_bearer_token(
                &tenant_a,
                "openai-codex",
                "tenant-a-token".to_string(),
            )
            .await;
        registry
            .set_tenant_provider_bearer_token(
                &tenant_b,
                "openai-codex",
                "tenant-b-token".to_string(),
            )
            .await;

        let (auth_a, auth_b) = tokio::join!(
            registry.scope_tenant_provider_auth(
                tenant_a.clone(),
                registry.auth_override_for_provider("openai-codex")
            ),
            registry.scope_tenant_provider_auth(
                tenant_b.clone(),
                registry.auth_override_for_provider("openai-codex")
            ),
        );
        assert!(matches!(auth_a, ProviderAuthOverride::Bearer(token) if token == "tenant-a-token"));
        assert!(matches!(auth_b, ProviderAuthOverride::Bearer(token) if token == "tenant-b-token"));

        let missing = registry
            .scope_tenant_provider_auth(
                tenant_missing,
                registry.auth_override_for_provider("openai-codex"),
            )
            .await;
        assert!(matches!(missing, ProviderAuthOverride::Suppress));
        let local = registry
            .scope_tenant_provider_auth(
                TenantContext::local_implicit(),
                registry.auth_override_for_provider("openai-codex"),
            )
            .await;
        assert!(matches!(local, ProviderAuthOverride::Inherit));

        registry
            .clear_tenant_provider_bearer_token(&tenant_a, "openai-codex")
            .await;
        assert!(
            !registry
                .tenant_provider_auth_is_loaded(&tenant_a, "openai-codex")
                .await
        );
        assert!(
            registry
                .tenant_provider_auth_is_loaded(&tenant_b, "openai-codex")
                .await
        );
    }

    #[derive(Clone)]
    struct CapturingCodexProvider {
        attempts: Arc<AtomicUsize>,
        seen_auth: Arc<Mutex<Vec<ProviderAuthOverride>>>,
        fail_auth_attempts: usize,
    }

    #[async_trait]
    impl Provider for CapturingCodexProvider {
        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                id: "openai-codex".to_string(),
                name: "capturing codex".to_string(),
                models: Vec::new(),
            }
        }

        async fn complete(
            &self,
            prompt: &str,
            _model_override: Option<&str>,
        ) -> anyhow::Result<String> {
            Ok(prompt.to_string())
        }

        async fn complete_with_auth_override(
            &self,
            prompt: &str,
            _model_override: Option<&str>,
            auth_override: ProviderAuthOverride,
        ) -> anyhow::Result<String> {
            self.seen_auth
                .lock()
                .expect("capture lock")
                .push(auth_override);
            let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
            if attempt < self.fail_auth_attempts {
                return Err(ProviderAuthenticationError::new(
                    401,
                    "provider request failed with status 401",
                )
                .into());
            }
            Ok(prompt.to_string())
        }
    }

    #[tokio::test]
    async fn hosted_dispatches_never_inherit_local_or_other_tenant_codex_auth() {
        let registry = ProviderRegistry::new(cfg(&[], None, false));
        let seen_auth = Arc::new(Mutex::new(Vec::new()));
        registry
            .replace_for_test(
                vec![Arc::new(CapturingCodexProvider {
                    attempts: Arc::new(AtomicUsize::new(0)),
                    seen_auth: seen_auth.clone(),
                    fail_auth_attempts: 0,
                })],
                Some("openai-codex".to_string()),
            )
            .await;
        let tenant_a = TenantContext::explicit("org-a", "workspace-a", None);
        let tenant_b = TenantContext::explicit("org-b", "workspace-b", None);
        let tenant_missing = TenantContext::explicit("org-missing", "workspace-missing", None);
        registry
            .set_tenant_provider_bearer_token(
                &tenant_a,
                "openai-codex",
                "tenant-a-token".to_string(),
            )
            .await;
        registry
            .set_tenant_provider_bearer_token(
                &tenant_b,
                "openai-codex",
                "tenant-b-token".to_string(),
            )
            .await;

        let (a, b, missing, local) = tokio::join!(
            registry.scope_tenant_provider_auth(
                tenant_a,
                registry.complete_for_provider(Some("openai-codex"), "a", None),
            ),
            registry.scope_tenant_provider_auth(
                tenant_b,
                registry.complete_for_provider(Some("openai-codex"), "b", None),
            ),
            registry.scope_tenant_provider_auth(
                tenant_missing,
                registry.complete_for_provider(Some("openai-codex"), "missing", None),
            ),
            registry.scope_tenant_provider_auth(
                TenantContext::local_implicit(),
                registry.complete_for_provider(Some("openai-codex"), "local", None),
            ),
        );
        assert_eq!(a.expect("tenant a"), "a");
        assert_eq!(b.expect("tenant b"), "b");
        assert_eq!(missing.expect("missing tenant"), "missing");
        assert_eq!(local.expect("local"), "local");

        let captured = seen_auth.lock().expect("capture lock").clone();
        assert!(captured.iter().any(
            |auth| matches!(auth, ProviderAuthOverride::Bearer(token) if token == "tenant-a-token")
        ));
        assert!(captured.iter().any(
            |auth| matches!(auth, ProviderAuthOverride::Bearer(token) if token == "tenant-b-token")
        ));
        assert!(captured
            .iter()
            .any(|auth| matches!(auth, ProviderAuthOverride::Suppress)));
        assert!(captured
            .iter()
            .any(|auth| matches!(auth, ProviderAuthOverride::Inherit)));
    }

    #[tokio::test]
    async fn typed_auth_failure_retries_only_one_provider_dispatch_with_fresh_auth() {
        let registry = ProviderRegistry::new(cfg(&[], None, false));
        let tenant = TenantContext::explicit("org-hosted", "workspace-hosted", None);
        registry
            .set_tenant_provider_bearer_token(&tenant, "openai-codex", "expired-token".to_string())
            .await;
        let attempts = Arc::new(AtomicUsize::new(0));
        let seen_auth = Arc::new(Mutex::new(Vec::new()));
        registry
            .replace_for_test(
                vec![Arc::new(CapturingCodexProvider {
                    attempts: attempts.clone(),
                    seen_auth: seen_auth.clone(),
                    fail_auth_attempts: 1,
                })],
                Some("openai-codex".to_string()),
            )
            .await;
        let refreshes = Arc::new(AtomicUsize::new(0));
        let recovery = ProviderAuthRecovery::new({
            let registry = registry.clone();
            let tenant = tenant.clone();
            let refreshes = refreshes.clone();
            move |_| {
                let registry = registry.clone();
                let tenant = tenant.clone();
                let refreshes = refreshes.clone();
                async move {
                    refreshes.fetch_add(1, Ordering::SeqCst);
                    registry
                        .set_tenant_provider_bearer_token(
                            &tenant,
                            "openai-codex",
                            "fresh-token".to_string(),
                        )
                        .await;
                    Ok(true)
                }
            }
        });

        let output = registry
            .scope_tenant_provider_auth_with_recovery(
                tenant,
                recovery,
                false,
                registry.complete_for_provider(Some("openai-codex"), "request", None),
            )
            .await
            .expect("recovered dispatch");
        assert_eq!(output, "request");
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert_eq!(refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(
            seen_auth.lock().expect("capture lock").as_slice(),
            [
                ProviderAuthOverride::Bearer("expired-token".to_string()),
                ProviderAuthOverride::Bearer("fresh-token".to_string()),
            ]
        );
    }

    #[tokio::test]
    async fn second_typed_auth_failure_is_returned_without_a_third_dispatch() {
        let registry = ProviderRegistry::new(cfg(&[], None, false));
        let tenant = TenantContext::explicit("org-hosted", "workspace-hosted", None);
        let attempts = Arc::new(AtomicUsize::new(0));
        registry
            .replace_for_test(
                vec![Arc::new(CapturingCodexProvider {
                    attempts: attempts.clone(),
                    seen_auth: Arc::new(Mutex::new(Vec::new())),
                    fail_auth_attempts: usize::MAX,
                })],
                Some("openai-codex".to_string()),
            )
            .await;
        let refreshes = Arc::new(AtomicUsize::new(0));
        let recovery = ProviderAuthRecovery::new({
            let refreshes = refreshes.clone();
            move |_| {
                let refreshes = refreshes.clone();
                async move {
                    refreshes.fetch_add(1, Ordering::SeqCst);
                    Ok(true)
                }
            }
        });

        let error = registry
            .scope_tenant_provider_auth_with_recovery(
                tenant,
                recovery,
                false,
                registry.complete_for_provider(Some("openai-codex"), "request", None),
            )
            .await
            .expect_err("second 401 must surface");
        assert!(error
            .downcast_ref::<ProviderAuthenticationError>()
            .is_some());
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert_eq!(refreshes.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn custom_provider_id_is_supported_from_config() {
        let registry = ProviderRegistry::new(cfg(&["custom"], Some("custom"), false));
        let provider = registry
            .select_provider(Some("custom"))
            .await
            .expect("provider");
        assert_eq!(provider.info().id, "custom");
    }

    #[test]
    fn normalize_base_handles_common_openai_compatible_inputs() {
        assert_eq!(
            normalize_base("http://localhost:8080"),
            "http://localhost:8080/v1"
        );
        assert_eq!(
            normalize_base("http://localhost:8080/v1"),
            "http://localhost:8080/v1"
        );
        assert_eq!(
            normalize_base("http://localhost:8080/v1/"),
            "http://localhost:8080/v1"
        );
        assert_eq!(
            normalize_base("http://localhost:8080/v1/chat/completions"),
            "http://localhost:8080/v1"
        );
        assert_eq!(
            normalize_base("http://localhost:8080/v1/models"),
            "http://localhost:8080/v1"
        );
        assert_eq!(
            normalize_base("http://localhost:8080/v1/v1"),
            "http://localhost:8080/v1"
        );
    }

    #[tokio::test]
    async fn private_provider_target_requires_explicit_standalone_network_policy() {
        let denied = PROVIDER_PRIVATE_ENDPOINTS_ALLOWED
            .scope(
                false,
                resolve_provider_request_target("http://127.0.0.1:11434/v1", "ollama"),
            )
            .await;
        let denied_error = match denied {
            Ok(_) => panic!("private provider target must be denied by default"),
            Err(error) => error,
        };
        assert!(denied_error.to_string().contains("https"));

        let allowed = PROVIDER_PRIVATE_ENDPOINTS_ALLOWED
            .scope(
                true,
                resolve_provider_request_target("http://127.0.0.1:11434/v1", "ollama"),
            )
            .await;
        assert!(allowed.is_ok(), "standalone network policy should allow ollama");
    }

    #[test]
    fn normalize_openai_messages_merges_system_messages_to_front() {
        let normalized = normalize_openai_messages(vec![
            ChatMessage {
                role: "system".to_string(),
                content: "base instructions".to_string(),
                attachments: Vec::new(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
                attachments: Vec::new(),
            },
            ChatMessage {
                role: "system".to_string(),
                content: "memory scope".to_string(),
                attachments: Vec::new(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "hello".to_string(),
                attachments: Vec::new(),
            },
        ]);

        assert_eq!(normalized.len(), 3);
        assert_eq!(normalized[0].role, "system");
        assert_eq!(normalized[0].content, "base instructions\n\nmemory scope");
        assert_eq!(normalized[1].role, "user");
        assert_eq!(normalized[2].role, "assistant");
    }

    #[test]
    fn normalize_openai_messages_leaves_non_system_order_unchanged() {
        let normalized = normalize_openai_messages(vec![
            ChatMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
                attachments: Vec::new(),
            },
            ChatMessage {
                role: "assistant".to_string(),
                content: "hello".to_string(),
                attachments: Vec::new(),
            },
        ]);

        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0].role, "user");
        assert_eq!(normalized[1].role, "assistant");
    }

    #[tokio::test]
    async fn complete_cheapest_picks_ollama_first() {
        // Test priority parsing logic
        let registry = ProviderRegistry::new(cfg(&["openai", "groq", "ollama"], None, true));
        let cheapest = registry.select_cheapest_provider_id().await;
        assert_eq!(cheapest, Some("ollama"));

        let registry = ProviderRegistry::new(cfg(&["openai", "openai", "openrouter"], None, true));
        let cheapest = registry.select_cheapest_provider_id().await;
        assert_eq!(cheapest, Some("openrouter"));

        let registry = ProviderRegistry::new(cfg(&["unknown_provider"], None, true));
        let cheapest = registry.select_cheapest_provider_id().await;
        assert_eq!(cheapest, None);
    }

    #[test]
    fn sanitize_openai_function_name_rewrites_invalid_chars() {
        assert_eq!(
            sanitize_openai_function_name("mcp.arcade.gmail_sendemail"),
            "mcp_arcade_gmail_sendemail"
        );
        assert_eq!(sanitize_openai_function_name("  "), "tool");
        assert_eq!(
            sanitize_openai_function_name("clickup-getSpaces"),
            "clickup-getSpaces"
        );
    }

    #[test]
    fn build_openai_tool_aliases_preserves_roundtrip_and_uniqueness() {
        let tools = vec![
            ToolSchema::new("mcp.arcade.gmail.send", "a", json!({"type":"object"})),
            ToolSchema::new("mcp_arcade_gmail_send", "b", json!({"type":"object"})),
        ];
        let (forward, reverse) = build_openai_tool_aliases(&tools);
        let alias_a = forward
            .get("mcp.arcade.gmail.send")
            .expect("alias for dotted name");
        let alias_b = forward
            .get("mcp_arcade_gmail_send")
            .expect("alias for underscore name");
        assert_ne!(alias_a, alias_b, "aliases must be unique");
        assert_eq!(
            reverse.get(alias_a).map(String::as_str),
            Some("mcp.arcade.gmail.send")
        );
        assert_eq!(
            reverse.get(alias_b).map(String::as_str),
            Some("mcp_arcade_gmail_send")
        );
    }

    fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        if needle.is_empty() {
            return Some(0);
        }
        haystack
            .windows(needle.len())
            .position(|window| window == needle)
    }

    async fn read_single_http_request(
        socket: &mut tokio::net::TcpStream,
    ) -> (String, String, String) {
        let mut buffer = Vec::new();
        let header_end = loop {
            let mut chunk = [0u8; 1024];
            let read = socket.read(&mut chunk).await.expect("read request");
            assert!(
                read > 0,
                "connection closed before request headers were read"
            );
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(pos) = find_subsequence(&buffer, b"\r\n\r\n") {
                break pos + 4;
            }
        };

        let headers = String::from_utf8(buffer[..header_end].to_vec()).expect("utf8 headers");
        let content_length = headers
            .lines()
            .find_map(|line| {
                line.strip_prefix("Content-Length: ")
                    .or_else(|| line.strip_prefix("content-length: "))
            })
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);

        let mut body = buffer[header_end..].to_vec();
        while body.len() < content_length {
            let mut chunk = [0u8; 1024];
            let read = socket.read(&mut chunk).await.expect("read request body");
            if read == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..read]);
        }

        let request_line = headers.lines().next().unwrap_or("").to_string();
        let body = String::from_utf8(body).expect("utf8 body");
        (request_line, headers, body)
    }

    #[tokio::test]
    async fn openai_codex_stream_uses_responses_transport() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("listener address");
        let (tx, rx) = oneshot::channel();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept connection");
            let request = read_single_http_request(&mut socket).await;
            let response_body = concat!(
                "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"delta\":\"Hello\"}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":5,\"output_tokens\":7,\"total_tokens\":12}}}\n\n",
                "data: [DONE]\n\n"
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.as_bytes().len(),
                response_body
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write response");
            socket.shutdown().await.expect("shutdown socket");
            tx.send(request).expect("send request");
        });

        let provider = OpenAIResponsesProvider {
            id: "openai-codex".to_string(),
            name: "OpenAI Codex".to_string(),
            base_url: format!("http://{}/codex", addr),
            api_key: Some("codex-test-token".to_string()),
            default_model: "gpt-5.5".to_string(),
            models: codex_supported_models(272_000),
            client: Client::new(),
        };

        let messages = vec![
            ChatMessage {
                role: "system".to_string(),
                content: "Be concise.".to_string(),
                attachments: Vec::new(),
            },
            ChatMessage {
                role: "user".to_string(),
                content: "hi".to_string(),
                attachments: Vec::new(),
            },
        ];
        let tools = vec![ToolSchema::new(
            "browser_wait",
            "Wait for a selector.",
            json!({
                "type": "object",
                "properties": {
                    "session_id": { "type": "string" },
                    "selector": { "type": "string" }
                },
                "required": ["session_id"],
                "anyOf": [
                    { "required": ["selector"] }
                ]
            }),
        )];
        let cancel = CancellationToken::new();
        let stream = provider
            .stream(
                messages,
                None,
                ToolMode::Auto,
                Some(tools),
                SamplingParams::default(),
                cancel,
            )
            .await
            .expect("stream");

        let mut chunks = Vec::new();
        futures::pin_mut!(stream);
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.expect("stream chunk");
            let is_done = matches!(chunk, StreamChunk::Done { .. });
            chunks.push(chunk);
            if is_done {
                break;
            }
        }

        let (request_line, headers, body) = rx.await.expect("request");
        server.await.expect("server task");

        assert_eq!(request_line, "POST /codex/responses HTTP/1.1");
        assert!(headers
            .to_ascii_lowercase()
            .contains("authorization: bearer codex-test-token"));
        assert!(body.contains("\"input\""));
        assert!(body.contains("\"store\":false"));
        assert!(body.contains("\"tools\":["));
        assert!(body.contains("\"tool_choice\":\"auto\""));
        assert!(body.contains("\"parallel_tool_calls\":false"));
        assert!(body.contains("\"instructions\":\"Be concise.\""));
        assert!(body.contains("\"gpt-5.5\""));
        assert!(body.contains("\"browser_wait\""));
        assert!(!body.contains("\"anyOf\""));
        assert!(!body.contains("\"role\":\"developer\""));
        assert!(!body.contains("\"max_output_tokens\""));

        let text_deltas = chunks
            .iter()
            .filter_map(|chunk| match chunk {
                StreamChunk::TextDelta(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(text_deltas, vec!["Hello"]);

        let done_chunks = chunks
            .iter()
            .filter(|chunk| matches!(chunk, StreamChunk::Done { .. }))
            .count();
        assert_eq!(done_chunks, 1);

        let done = chunks
            .iter()
            .find_map(|chunk| match chunk {
                StreamChunk::Done {
                    finish_reason,
                    usage,
                } => Some((
                    finish_reason.as_str(),
                    usage.as_ref().map(|usage| usage.total_tokens),
                )),
                _ => None,
            })
            .expect("done chunk");
        assert_eq!(done.0, "stop");
        assert_eq!(done.1, Some(12));
    }

    #[tokio::test]
    async fn openai_codex_stream_recovers_function_call_args_without_deltas() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("listener address");
        let (tx, rx) = oneshot::channel();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept connection");
            let request = read_single_http_request(&mut socket).await;
            let response_body = concat!(
                "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_abc\",\"name\":\"write\"}}\n\n",
                "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_abc\",\"name\":\"write\",\"arguments\":\"{\\\"path\\\":\\\"assess.json\\\",\\\"content\\\":\\\"{}\\\"}\"}}\n\n",
                "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output\":[{\"type\":\"function_call\",\"id\":\"fc_1\",\"call_id\":\"call_abc\",\"name\":\"write\",\"arguments\":\"{\\\"path\\\":\\\"assess.json\\\",\\\"content\\\":\\\"{}\\\"}\"}],\"usage\":{\"input_tokens\":10,\"output_tokens\":20,\"total_tokens\":30}}}\n\n",
                "data: [DONE]\n\n"
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.as_bytes().len(),
                response_body
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write response");
            socket.shutdown().await.expect("shutdown socket");
            tx.send(request).expect("send request");
        });

        let provider = OpenAIResponsesProvider {
            id: "openai-codex".to_string(),
            name: "OpenAI Codex".to_string(),
            base_url: format!("http://{}/codex", addr),
            api_key: Some("codex-test-token".to_string()),
            default_model: "gpt-5.4-mini".to_string(),
            models: codex_supported_models(272_000),
            client: Client::new(),
        };

        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: "write a file".to_string(),
            attachments: Vec::new(),
        }];
        let tools = vec![ToolSchema::new(
            "write",
            "Write a file.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "content": { "type": "string" }
                },
                "required": ["path", "content"]
            }),
        )];
        let cancel = CancellationToken::new();
        let stream = provider
            .stream(
                messages,
                None,
                ToolMode::Auto,
                Some(tools),
                SamplingParams::default(),
                cancel,
            )
            .await
            .expect("stream");

        let mut chunks = Vec::new();
        futures::pin_mut!(stream);
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.expect("stream chunk");
            let is_done = matches!(chunk, StreamChunk::Done { .. });
            chunks.push(chunk);
            if is_done {
                break;
            }
        }

        let _ = rx.await.expect("request");
        server.await.expect("server task");

        let tool_start_count = chunks
            .iter()
            .filter(|chunk| matches!(chunk, StreamChunk::ToolCallStart { .. }))
            .count();
        assert_eq!(tool_start_count, 1, "expected exactly one ToolCallStart");

        let tool_end_count = chunks
            .iter()
            .filter(|chunk| matches!(chunk, StreamChunk::ToolCallEnd { .. }))
            .count();
        assert_eq!(tool_end_count, 1, "expected exactly one ToolCallEnd");

        let accumulated_args = chunks
            .iter()
            .filter_map(|chunk| match chunk {
                StreamChunk::ToolCallDelta { args_delta, .. } => Some(args_delta.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .concat();
        assert!(
            accumulated_args.contains("\"path\":\"assess.json\""),
            "recovered args missing path: {accumulated_args}"
        );
        assert!(
            accumulated_args.contains("\"content\""),
            "recovered args missing content key: {accumulated_args}"
        );

        let done = chunks
            .iter()
            .find_map(|chunk| match chunk {
                StreamChunk::Done { finish_reason, .. } => Some(finish_reason.as_str()),
                _ => None,
            })
            .expect("done chunk");
        assert_eq!(done, "toolUse");
    }

    #[tokio::test]
    async fn openai_codex_stream_accepts_sse_event_headers_without_json_type() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("listener address");
        let (tx, rx) = oneshot::channel();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept connection");
            let request = read_single_http_request(&mut socket).await;
            let response_body = concat!(
                "event: response.output_text.delta\n",
                "data: {\"item_id\":\"msg_1\",\"delta\":\"Hello\"}\n\n",
                "event: response.completed\n",
                "data: {\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":5,\"output_tokens\":7,\"total_tokens\":12}}}\n\n",
                "data: [DONE]\n\n"
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.as_bytes().len(),
                response_body
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write response");
            socket.shutdown().await.expect("shutdown socket");
            tx.send(request).expect("send request");
        });

        let provider = OpenAIResponsesProvider {
            id: "openai-codex".to_string(),
            name: "OpenAI Codex".to_string(),
            base_url: format!("http://{}/codex", addr),
            api_key: Some("codex-test-token".to_string()),
            default_model: "gpt-5.5".to_string(),
            models: codex_supported_models(272_000),
            client: Client::new(),
        };

        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: "hi".to_string(),
            attachments: Vec::new(),
        }];
        let cancel = CancellationToken::new();
        let stream = provider
            .stream(
                messages,
                None,
                ToolMode::Auto,
                None,
                SamplingParams::default(),
                cancel,
            )
            .await
            .expect("stream");

        let mut chunks = Vec::new();
        futures::pin_mut!(stream);
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.expect("stream chunk");
            let is_done = matches!(chunk, StreamChunk::Done { .. });
            chunks.push(chunk);
            if is_done {
                break;
            }
        }

        let _ = rx.await.expect("request");
        server.await.expect("server task");

        let text_deltas = chunks
            .iter()
            .filter_map(|chunk| match chunk {
                StreamChunk::TextDelta(text) => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(text_deltas, vec!["Hello"]);

        let done = chunks
            .iter()
            .find_map(|chunk| match chunk {
                StreamChunk::Done {
                    finish_reason,
                    usage,
                } => Some((
                    finish_reason.as_str(),
                    usage.as_ref().map(|usage| usage.total_tokens),
                )),
                _ => None,
            })
            .expect("done chunk");
        assert_eq!(done.0, "stop");
        assert_eq!(done.1, Some(12));
    }

    #[tokio::test]
    async fn openai_codex_complete_recovers_when_responses_requires_streaming() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("listener address");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let server = tokio::spawn(async move {
            let mut request_count = 0usize;
            while request_count < 2 {
                let (mut socket, _) = listener.accept().await.expect("accept connection");
                let request = read_single_http_request(&mut socket).await;
                request_count += 1;
                if request_count == 1 {
                    let response_body = "{\"detail\":\"Stream must be set to true\"}";
                    let response = format!(
                        "HTTP/1.1 400 Bad Request\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        response_body.as_bytes().len(),
                        response_body
                    );
                    socket
                        .write_all(response.as_bytes())
                        .await
                        .expect("write first response");
                } else {
                    let response_body = concat!(
                        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"Recovered\"}\n\n",
                        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"output_text\":\"Recovered\"}}\n\n",
                        "data: [DONE]\n\n"
                    );
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        response_body.as_bytes().len(),
                        response_body
                    );
                    socket
                        .write_all(response.as_bytes())
                        .await
                        .expect("write second response");
                }
                socket.shutdown().await.expect("shutdown socket");
                tx.send(request).expect("send request");
            }
        });

        let provider = OpenAIResponsesProvider {
            id: "openai-codex".to_string(),
            name: "OpenAI Codex".to_string(),
            base_url: format!("http://{}/codex", addr),
            api_key: Some("codex-test-token".to_string()),
            default_model: "gpt-5.5".to_string(),
            models: codex_supported_models(272_000),
            client: Client::new(),
        };

        let text = provider
            .complete("recover completion", None)
            .await
            .expect("completion");
        assert_eq!(text, "Recovered");

        let first = rx.recv().await.expect("first request");
        let second = rx.recv().await.expect("second request");
        server.await.expect("server task");

        assert_eq!(first.0, "POST /codex/responses HTTP/1.1");
        assert!(first.2.contains("\"stream\":false"));
        assert_eq!(second.0, "POST /codex/responses HTTP/1.1");
        assert!(second.2.contains("\"stream\":true"));
    }

    #[test]
    fn codex_supported_models_include_extended_catalog() {
        let models = codex_supported_models(272_000);
        let ids = models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(OPENAI_CODEX_DEFAULT_MODEL, "gpt-5.6-terra");
        assert_eq!(
            openai_codex_effective_default_model(None),
            "gpt-5.6-terra"
        );
        assert!(ids.contains(&"gpt-5.6-sol"));
        assert!(ids.contains(&"gpt-5.6-terra"));
        assert!(ids.contains(&"gpt-5.6-luna"));
        assert!(!ids.contains(&"gpt-5.6"));
        assert!(ids.contains(&"gpt-5.5"));
        assert!(ids.contains(&"gpt-5.4"));
        assert!(ids.contains(&"gpt-5.2-codex"));
        assert!(ids.contains(&"gpt-5.4-mini"));
        assert!(ids.contains(&"gpt-5.3-codex"));
        assert!(ids.contains(&"gpt-5.3-codex-spark"));
        assert!(ids.contains(&"gpt-5.1-codex-mini"));
        // Retired phantom model must not reappear in the catalog.
        assert!(!ids.contains(&"gpt-5.1-codex-max"));
    }

    #[test]
    fn extract_openai_tool_call_chunks_supports_content_array_tool_calls() {
        let mut alias_to_original = HashMap::new();
        alias_to_original.insert("write_alias".to_string(), "write".to_string());
        let choice = json!({
            "message": {
                "content": [
                    {
                        "type": "tool_call",
                        "id": "call-1",
                        "function": {
                            "name": "write_alias",
                            "arguments": "{\"path\":\"README.md\",\"content\":\"hi\"}"
                        }
                    }
                ]
            }
        });
        let calls = extract_openai_tool_call_chunks(&choice, &alias_to_original);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call-1");
        assert_eq!(calls[0].name, "write");
        assert!(calls[0].args_delta.contains("\"README.md\""));
    }

    #[test]
    fn resolve_openai_tool_call_stream_id_keeps_multichunk_write_args_on_same_id() {
        let mut alias_to_original = HashMap::new();
        alias_to_original.insert("write_alias".to_string(), "write".to_string());

        let first_choice = json!({
            "delta": {
                "tool_calls": [
                    {
                        "index": 2,
                        "id": "call_ghi",
                        "function": {
                            "name": "write_alias",
                            "arguments": ""
                        }
                    }
                ]
            }
        });
        let continuation_choice = json!({
            "delta": {
                "tool_calls": [
                    {
                        "index": 2,
                        "function": {
                            "arguments": "{\"path\":\"game.html\",\"content\":\"hi\"}"
                        }
                    }
                ]
            }
        });

        let first_calls = extract_openai_tool_call_chunks(&first_choice, &alias_to_original);
        let continuation_calls =
            extract_openai_tool_call_chunks(&continuation_choice, &alias_to_original);

        assert_eq!(first_calls.len(), 1);
        assert_eq!(first_calls[0].id, "call_ghi");
        assert_eq!(first_calls[0].name, "write");
        assert_eq!(first_calls[0].index, 2);

        assert_eq!(continuation_calls.len(), 1);
        assert_eq!(continuation_calls[0].id, "tool_call_2");
        assert_eq!(continuation_calls[0].name, "");
        assert_eq!(continuation_calls[0].index, 2);

        let mut real_ids_by_index = HashMap::new();
        let mut args_by_id = HashMap::<String, String>::new();
        for call in first_calls.into_iter().chain(continuation_calls) {
            let effective_id = resolve_openai_tool_call_stream_id(&call, &mut real_ids_by_index);
            args_by_id
                .entry(effective_id)
                .or_default()
                .push_str(&call.args_delta);
        }

        assert_eq!(
            real_ids_by_index.get(&2).map(String::as_str),
            Some("call_ghi")
        );
        assert_eq!(
            args_by_id.get("call_ghi").map(String::as_str),
            Some("{\"path\":\"game.html\",\"content\":\"hi\"}")
        );
        assert!(!args_by_id.contains_key("tool_call_2"));
    }

    #[test]
    fn push_openai_text_fragments_reads_nested_text_parts() {
        let value = json!([
            {"type":"text","text":"first"},
            {"type":"output_text","text":{"value":"second"}},
            {"type":"text","content":"third"}
        ]);
        let mut fragments = Vec::new();
        push_openai_text_fragments(&value, &mut fragments);
        assert_eq!(fragments, vec!["first", "second", "third"]);
    }

    #[test]
    fn normalize_openai_function_parameters_adds_missing_properties() {
        let normalized = normalize_openai_function_parameters(json!({"type":"object"}));
        assert_eq!(
            normalized
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
            "object"
        );
        assert!(
            normalized
                .get("properties")
                .and_then(|v| v.as_object())
                .is_some(),
            "properties object should exist"
        );
    }

    #[test]
    fn normalize_openai_function_parameters_recovers_non_object_schema() {
        let normalized = normalize_openai_function_parameters(json!("bad"));
        assert_eq!(
            normalized
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
            "object"
        );
        assert!(
            normalized
                .get("properties")
                .and_then(|v| v.as_object())
                .is_some(),
            "properties object should exist"
        );
    }

    #[test]
    fn normalize_openai_function_parameters_rewrites_tuple_array_items() {
        let normalized = normalize_openai_function_parameters(json!({
            "type": "object",
            "properties": {
                "fieldIds": {
                    "type": "array",
                    "items": [
                        { "$ref": "#/properties/fieldIds/items" }
                    ]
                }
            }
        }));
        assert!(
            normalized["properties"]["fieldIds"]["items"].is_object(),
            "array items should be object/bool for OpenAI-compatible tools"
        );
    }

    #[test]
    fn normalize_openai_function_parameters_adds_nested_object_properties() {
        let normalized = normalize_openai_function_parameters(json!({
            "type": "object",
            "properties": {
                "filters": {
                    "type": "object"
                }
            }
        }));
        assert!(
            normalized["properties"]["filters"]["properties"].is_object(),
            "nested object schemas should include properties for OpenAI validation"
        );
    }

    #[test]
    fn normalize_codex_function_parameters_strips_root_combinators() {
        let normalized = normalize_codex_function_parameters(json!({
            "type": "object",
            "properties": {
                "session_id": { "type": "string" },
                "selector": { "type": "string" }
            },
            "required": ["session_id"],
            "anyOf": [
                { "required": ["selector"] }
            ],
            "not": {
                "required": ["forbidden"]
            }
        }));

        assert_eq!(
            normalized.get("type").and_then(|value| value.as_str()),
            Some("object")
        );
        assert!(normalized
            .get("properties")
            .and_then(|value| value.as_object())
            .is_some());
        assert!(normalized.get("anyOf").is_none());
        assert!(normalized.get("not").is_none());
    }

    #[test]
    fn openrouter_affordability_retry_uses_affordable_cap() {
        let detail = r#"{"error":{"message":"This request requires more credits, or fewer max_tokens. You requested up to 16384 tokens, but can only afford 14605."}}"#;
        assert_eq!(
            openrouter_affordability_retry_max_tokens(
                "openrouter",
                reqwest::StatusCode::PAYMENT_REQUIRED,
                detail,
                16_384,
            ),
            Some(14_605)
        );
    }

    #[test]
    fn openrouter_tool_choice_retry_detects_unsupported_required_mode() {
        assert!(openrouter_tool_choice_retry_supported(
            "openrouter",
            &ToolMode::Required,
            "No endpoints found that support the provided 'tool_choice' value."
        ));
        assert!(!openrouter_tool_choice_retry_supported(
            "openrouter",
            &ToolMode::Auto,
            "No endpoints found that support the provided 'tool_choice' value."
        ));
        assert!(!openrouter_tool_choice_retry_supported(
            "openai",
            &ToolMode::Required,
            "No endpoints found that support the provided 'tool_choice' value."
        ));
    }

    #[test]
    fn provider_specific_max_tokens_override_is_respected() {
        std::env::remove_var("TANDEM_PROVIDER_MAX_TOKENS");
        std::env::set_var("TANDEM_PROVIDER_MAX_TOKENS_OPENROUTER", "24576");
        assert_eq!(provider_max_tokens_for("openrouter"), 24_576);
        std::env::remove_var("TANDEM_PROVIDER_MAX_TOKENS_OPENROUTER");
        assert_eq!(provider_max_tokens_for("openrouter"), 16_384);
    }

    // OpenAI sends usage in a separate trailing chunk (choices:[]) when
    // stream_options.include_usage is set.  The Done event must carry that
    // usage even though it arrives after the finish_reason chunk.
    #[tokio::test]
    async fn chat_completions_trailing_usage_chunk_reaches_done_event() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("listener address");
        let (tx, rx) = oneshot::channel::<(String, String, String)>();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept connection");
            let request = read_single_http_request(&mut socket).await;
            // finish_reason chunk arrives first, usage chunk arrives separately
            let response_body = concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"Hi\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":5,\"total_tokens\":15}}\n\n",
                "data: [DONE]\n\n",
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.as_bytes().len(),
                response_body
            );
            socket.write_all(response.as_bytes()).await.expect("write");
            socket.shutdown().await.expect("shutdown");
            tx.send(request).expect("send");
        });

        let provider = OpenAICompatibleProvider {
            id: "ollama".to_string(),
            name: "Ollama".to_string(),
            base_url: format!("http://{}/v1", addr),
            api_key: Some("sk-test".to_string()),
            default_model: "gpt-4o-mini".to_string(),
        };

        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: "hi".to_string(),
            attachments: Vec::new(),
        }];
        let cancel = CancellationToken::new();
        let stream = PROVIDER_PRIVATE_ENDPOINTS_ALLOWED
            .scope(
                true,
                provider.stream(
                    messages,
                    None,
                    ToolMode::Auto,
                    None,
                    SamplingParams::default(),
                    cancel,
                ),
            )
            .await
            .expect("stream");

        let mut chunks = Vec::new();
        futures::pin_mut!(stream);
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.expect("chunk");
            let is_done = matches!(chunk, StreamChunk::Done { .. });
            chunks.push(chunk);
            if is_done {
                break;
            }
        }

        let (_, _, body) = rx.await.expect("request");
        server.await.expect("server");

        assert!(
            body.contains("\"include_usage\":true"),
            "request body must include stream_options.include_usage: {body}"
        );

        let done = chunks
            .iter()
            .find_map(|c| match c {
                StreamChunk::Done {
                    finish_reason,
                    usage,
                } => Some((
                    finish_reason.as_str(),
                    usage.as_ref().map(|u| u.total_tokens),
                )),
                _ => None,
            })
            .expect("done chunk");
        assert_eq!(done.0, "stop");
        assert_eq!(
            done.1,
            Some(15),
            "trailing usage chunk must reach the Done event"
        );
    }

    #[tokio::test]
    async fn provider_completion_rejects_oversized_response_before_decoding() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("listener address");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept connection");
            let _ = read_single_http_request(&mut socket).await;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                PROVIDER_RESPONSE_MAX_BYTES + 1
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write response");
            socket.shutdown().await.expect("shutdown socket");
        });
        let provider = OpenAICompatibleProvider {
            id: "ollama".to_string(),
            name: "Ollama".to_string(),
            base_url: format!("http://{addr}/v1"),
            api_key: None,
            default_model: "local-model".to_string(),
        };

        let error = PROVIDER_PRIVATE_ENDPOINTS_ALLOWED
            .scope(true, provider.complete("hello", None))
            .await
            .expect_err("oversized response must fail before JSON decoding");
        server.await.expect("server");
        assert!(error.to_string().contains("provider response exceeds"));
    }

    #[tokio::test]
    async fn provider_response_reader_rejects_chunked_body_over_limit() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind test server");
        let addr = listener.local_addr().expect("listener address");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept connection");
            let _ = read_single_http_request(&mut socket).await;
            let body = "A".repeat(65);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:X}\r\n{}\r\n0\r\n\r\n",
                body.len(),
                body
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write response");
            socket.shutdown().await.expect("shutdown socket");
        });
        let response = Client::new()
            .get(format!("http://{addr}/body"))
            .send()
            .await
            .expect("request");

        let error = read_provider_response_bytes_with_limit(response, 64)
            .await
            .expect_err("chunked response over the streaming limit must fail");
        server.await.expect("server");
        assert!(error.to_string().contains("provider response exceeds 64 bytes"));
    }

    // ── Per-role sampling parameter mapping & clamping ───────────────────────

    #[test]
    fn openai_chat_sampling_maps_all_fields() {
        let mut body = json!({ "model": "gpt-4o", "max_tokens": 16384 });
        apply_openai_chat_sampling(
            &mut body,
            "openai",
            "gpt-4o",
            SamplingParams {
                temperature: Some(0.1),
                top_p: Some(0.9),
                max_tokens: Some(2048),
            },
        );
        assert_eq!(body["temperature"], json!(0.1_f32));
        assert_eq!(body["top_p"], json!(0.9_f32));
        // Explicit max_tokens overrides the engine default budget.
        assert_eq!(body["max_tokens"], json!(2048));
    }

    #[test]
    fn empty_sampling_leaves_request_body_untouched() {
        // Omitting sampling must produce a byte-identical request to today.
        let original = json!({
            "model": "gpt-4o",
            "messages": [],
            "stream": true,
            "max_tokens": 16384,
        });
        let mut body = original.clone();
        apply_openai_chat_sampling(&mut body, "openai", "gpt-4o", SamplingParams::default());
        assert_eq!(body, original);
    }

    #[test]
    fn openai_chat_sampling_clamps_out_of_range_values() {
        let mut body = json!({ "model": "gpt-4o" });
        apply_openai_chat_sampling(
            &mut body,
            "openai",
            "gpt-4o",
            SamplingParams {
                temperature: Some(5.0),
                top_p: Some(2.0),
                max_tokens: Some(0),
            },
        );
        // OpenAI temperature caps at 2.0, top_p at 1.0, max_tokens at >= 1.
        assert_eq!(body["temperature"], json!(2.0_f32));
        assert_eq!(body["top_p"], json!(1.0_f32));
        assert_eq!(body["max_tokens"], json!(1));
    }

    #[test]
    fn reasoning_model_drops_temperature_without_failing() {
        let mut body = json!({ "model": "o3-mini" });
        apply_openai_chat_sampling(
            &mut body,
            "openai",
            "o3-mini",
            SamplingParams {
                temperature: Some(0.2),
                top_p: None,
                max_tokens: Some(1000),
            },
        );
        // Model rejects temperature → dropped (with a warning), run continues.
        assert!(body.get("temperature").is_none());
        assert_eq!(body["max_tokens"], json!(1000));
    }

    #[test]
    fn openai_responses_sampling_uses_max_output_tokens() {
        let mut body = json!({ "model": "gpt-4o" });
        apply_openai_responses_sampling(
            &mut body,
            "openai-codex",
            "gpt-4o",
            SamplingParams {
                temperature: Some(0.3),
                top_p: None,
                max_tokens: Some(4096),
            },
        );
        assert_eq!(body["temperature"], json!(0.3_f32));
        assert_eq!(body["max_output_tokens"], json!(4096));
        assert!(body.get("max_tokens").is_none());
    }

    #[test]
    fn anthropic_sampling_clamps_temperature_to_one() {
        let mut body = json!({ "model": "claude-sonnet-4-6", "max_tokens": 1024 });
        apply_anthropic_sampling(
            &mut body,
            "claude-sonnet-4-6",
            SamplingParams {
                temperature: Some(1.8),
                top_p: Some(0.5),
                max_tokens: Some(8192),
            },
        );
        // Anthropic caps temperature at 1.0.
        assert_eq!(body["temperature"], json!(1.0_f32));
        assert_eq!(body["top_p"], json!(0.5_f32));
        assert_eq!(body["max_tokens"], json!(8192));
    }

    #[test]
    fn model_rejects_temperature_matches_reasoning_families() {
        assert!(model_rejects_temperature("o1"));
        assert!(model_rejects_temperature("o3-mini"));
        assert!(model_rejects_temperature("o4-mini"));
        assert!(model_rejects_temperature("gpt-5-thinking"));
        assert!(!model_rejects_temperature("gpt-4o"));
        assert!(!model_rejects_temperature("claude-sonnet-4-6"));
    }

    #[test]
    fn openrouter_affordability_retry_respects_max_tokens_override() {
        let status = reqwest::StatusCode::PAYMENT_REQUIRED;
        let detail = "can only afford 32000";
        // With a max_tokens override of 64k, an affordable cap of 32k is below
        // the requested value and must trigger a retry (regression: previously
        // compared against the 16k default and bailed).
        assert_eq!(
            openrouter_affordability_retry_max_tokens("openrouter", status, detail, 64_000),
            Some(32_000)
        );
        // Affordable cap at/above the requested value does not retry.
        assert_eq!(
            openrouter_affordability_retry_max_tokens("openrouter", status, detail, 16_000),
            None
        );
        // Only OpenRouter 402s qualify.
        assert_eq!(
            openrouter_affordability_retry_max_tokens("openai", status, detail, 64_000),
            None
        );
    }
}
