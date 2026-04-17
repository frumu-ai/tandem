#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
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
            default_model: "gpt-5.4".to_string(),
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
            .stream(messages, None, ToolMode::Auto, Some(tools), cancel)
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
        assert!(body.contains("\"gpt-5.4\""));
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

    #[test]
    fn codex_supported_models_include_extended_catalog() {
        let models = codex_supported_models(272_000);
        let ids = models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>();
        assert!(ids.contains(&"gpt-5.4"));
        assert!(ids.contains(&"gpt-5.2-codex"));
        assert!(ids.contains(&"gpt-5.1-codex-max"));
        assert!(ids.contains(&"gpt-5.4-mini"));
        assert!(ids.contains(&"gpt-5.3-codex"));
        assert!(ids.contains(&"gpt-5.3-codex-spark"));
        assert!(ids.contains(&"gpt-5.1-codex-mini"));
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
}
