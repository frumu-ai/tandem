#[async_trait]
impl Provider for OpenAICompatibleProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            models: vec![ModelInfo {
                id: self.default_model.clone(),
                provider_id: self.id.clone(),
                display_name: self.default_model.clone(),
                context_window: 128_000,
            }],
        }
    }

    async fn complete(&self, prompt: &str, model_override: Option<&str>) -> anyhow::Result<String> {
        let model = model_override
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or(self.default_model.as_str());
        let url = format!("{}/chat/completions", self.base_url);
        let mut response_opt = None;
        let mut last_send_err: Option<reqwest::Error> = None;
        let mut last_error_detail: Option<String> = None;
        let mut max_tokens = provider_max_tokens_for(&self.id);
        for attempt in 0..3 {
            let mut req = self.client.post(url.clone()).json(&json!({
                "model": model,
                "messages": [{"role":"user","content": prompt}],
                "stream": false,
                "max_tokens": max_tokens,
            }));
            if self.id == "openrouter" {
                req = req
                    .header("HTTP-Referer", "https://tandem.ac")
                    .header("X-Title", protocol_title_header());
            }
            if let Some(api_key) = &self.api_key {
                req = req.bearer_auth(api_key);
            }

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if !status.is_success() {
                        let text = resp.text().await.unwrap_or_default();
                        if let Some(affordable_max) = openrouter_affordability_retry_max_tokens(
                            &self.id, status, &text, max_tokens,
                        ) {
                            max_tokens = affordable_max;
                            if attempt < 2 {
                                continue;
                            }
                        }
                        last_error_detail = Some(format_openai_error_response(status, &text));
                        break;
                    }
                    response_opt = Some(resp);
                    break;
                }
                Err(err) => {
                    let retryable = err.is_connect() || err.is_timeout();
                    if retryable && attempt < 2 {
                        sleep(Duration::from_millis(300 * (attempt + 1) as u64)).await;
                        last_send_err = Some(err);
                        continue;
                    }
                    last_send_err = Some(err);
                    break;
                }
            }
        }

        let response = if let Some(resp) = response_opt {
            resp
        } else if let Some(detail) = last_error_detail {
            anyhow::bail!(detail);
        } else {
            let err = last_send_err.expect("send error should be captured");
            let category = if err.is_connect() {
                "connection error"
            } else if err.is_timeout() {
                "timeout"
            } else {
                "request error"
            };
            anyhow::bail!(
                "failed to reach provider `{}` at {} ({}): {}. Verify endpoint is reachable and OpenAI-compatible.",
                self.id,
                self.base_url,
                category,
                err
            );
        };
        let value: serde_json::Value = response.json().await?;

        if let Some(detail) = extract_openai_error(&value) {
            anyhow::bail!(detail);
        }

        if let Some(text) = extract_openai_text(&value) {
            return Ok(text);
        }

        let body_preview = truncate_for_error(&value.to_string(), 500);
        anyhow::bail!(
            "provider returned no completion content for model `{}` (response: {})",
            model,
            body_preview
        );
    }

    async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        model_override: Option<&str>,
        tool_mode: ToolMode,
        tools: Option<Vec<ToolSchema>>,
        cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let model = model_override
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or(self.default_model.as_str());
        let url = format!("{}/chat/completions", self.base_url);
        let has_image_inputs = messages.iter().any(|m| !m.attachments.is_empty());
        if has_image_inputs && !model_supports_vision_input(model) {
            anyhow::bail!(
                "selected model `{}` does not appear to support image input. choose a vision-capable model.",
                model
            );
        }

        let wire_messages = normalize_openai_messages(messages)
            .into_iter()
            .map(chat_message_to_openai_wire)
            .collect::<Vec<_>>();

        let tools = tools.unwrap_or_default();
        let (original_to_alias, alias_to_original) = build_openai_tool_aliases(&tools);
        let wire_tools = tools
            .into_iter()
            .map(|tool| {
                let safe_name = original_to_alias
                    .get(tool.name.as_str())
                    .cloned()
                    .unwrap_or_else(|| sanitize_openai_function_name(&tool.name));
                json!({
                    "type": "function",
                    "function": {
                        "name": safe_name,
                        "description": tool.description,
                        "parameters": normalize_openai_function_parameters(tool.input_schema),
                    }
                })
            })
            .collect::<Vec<_>>();
        let has_tools = !wire_tools.is_empty();

        let mut max_tokens = provider_max_tokens_for(&self.id);
        let mut body = json!({
            "model": model,
            "messages": wire_messages,
            "stream": true,
            "max_tokens": max_tokens,
        });
        if has_tools {
            body["tools"] = serde_json::Value::Array(wire_tools);
            body["tool_choice"] = json!(openai_tool_choice(&tool_mode));
        }

        let mut resp_opt = None;
        let mut last_send_err: Option<reqwest::Error> = None;
        let mut last_error_detail: Option<String> = None;
        let mut downgraded_openrouter_tool_choice = false;
        for attempt in 0..3 {
            let mut req = self.client.post(url.clone()).json(&body);
            if self.id == "openrouter" {
                req = req
                    .header("HTTP-Referer", "https://tandem.ac")
                    .header("X-Title", protocol_title_header());
            }
            if let Some(api_key) = &self.api_key {
                req = req.bearer_auth(api_key);
            }

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if !status.is_success() {
                        let text = resp.text().await.unwrap_or_default();
                        if has_tools
                            && !downgraded_openrouter_tool_choice
                            && openrouter_tool_choice_retry_supported(&self.id, &tool_mode, &text)
                        {
                            body["tool_choice"] = json!("auto");
                            downgraded_openrouter_tool_choice = true;
                            if attempt < 2 {
                                continue;
                            }
                        }
                        if let Some(affordable_max) = openrouter_affordability_retry_max_tokens(
                            &self.id, status, &text, max_tokens,
                        ) {
                            max_tokens = affordable_max;
                            body["max_tokens"] = json!(max_tokens);
                            if attempt < 2 {
                                continue;
                            }
                        }
                        last_error_detail = Some(format_openai_error_response(status, &text));
                        break;
                    }
                    resp_opt = Some(resp);
                    break;
                }
                Err(err) => {
                    let retryable = err.is_connect() || err.is_timeout();
                    if retryable && attempt < 2 {
                        sleep(Duration::from_millis(300 * (attempt + 1) as u64)).await;
                        last_send_err = Some(err);
                        continue;
                    }
                    last_send_err = Some(err);
                    break;
                }
            }
        }

        let resp = if let Some(resp) = resp_opt {
            resp
        } else if let Some(detail) = last_error_detail {
            anyhow::bail!(detail);
        } else {
            let err = last_send_err.expect("send error should be captured");
            let category = if err.is_connect() {
                "connection error"
            } else if err.is_timeout() {
                "timeout"
            } else {
                "request error"
            };
            anyhow::bail!(
                "failed to reach provider `{}` at {} ({}): {}. Verify endpoint is reachable and OpenAI-compatible.",
                self.id,
                self.base_url,
                category,
                err
            );
        };
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            if text.contains("Failed to authenticate request with Clerk") {
                let key_hint = provider_api_key_env_hint(&self.id);
                anyhow::bail!(
                    "provider authentication failed ({}) for `{}`. Verify the provider API key (set `{}` or configure the key in Settings) and retry.",
                    status,
                    self.id,
                    key_hint
                );
            }
            anyhow::bail!(
                "provider stream request failed with status {}: {}",
                status,
                truncate_for_error(&text, 500)
            );
        }

        let mut bytes = resp.bytes_stream();
        let alias_to_original = alias_to_original.clone();
        let stream = try_stream! {
            let mut buffer = String::new();
            let mut tool_call_real_ids = HashMap::new();
            while let Some(chunk) = bytes.next().await {
                if cancel.is_cancelled() {
                    yield StreamChunk::Done {
                        finish_reason: "cancelled".to_string(),
                        usage: None,
                    };
                    break;
                }

                let chunk = chunk?;
                buffer.push_str(str::from_utf8(&chunk).unwrap_or_default());

                while let Some(pos) = buffer.find("\n\n") {
                    let frame = buffer[..pos].to_string();
                    buffer = buffer[pos + 2..].to_string();
                    for line in frame.lines() {
                        if !line.starts_with("data: ") {
                            continue;
                        }
                        let payload = line.trim_start_matches("data: ").trim();
                        if payload == "[DONE]" {
                            yield StreamChunk::Done {
                                finish_reason: "stop".to_string(),
                                usage: None,
                            };
                            continue;
                        }

                        let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
                            continue;
                        };

                        if let Some(detail) = extract_openai_error(&value) {
                            Err(anyhow::anyhow!(detail))?;
                        }

                        let choices = value
                            .get("choices")
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default();
                        for choice in choices {
                            let delta = choice.get("delta").cloned().unwrap_or_default();
                            let message = choice.get("message").cloned().unwrap_or_default();

                            let mut emitted_text = false;
                            let mut text_fragments = Vec::new();
                            push_openai_text_fragments(&delta.get("content").cloned().unwrap_or_default(), &mut text_fragments);
                            if text_fragments.is_empty() {
                                push_openai_text_fragments(&message.get("content").cloned().unwrap_or_default(), &mut text_fragments);
                            }
                            for text in text_fragments {
                                if !text.is_empty() {
                                    emitted_text = true;
                                    yield StreamChunk::TextDelta(text);
                                }
                            }

                            if !emitted_text {
                                if let Some(text) = message.get("content").and_then(|v| v.as_str()) {
                                    if !text.is_empty() {
                                        yield StreamChunk::TextDelta(text.to_string());
                                    }
                                }
                            }

                            for call in extract_openai_tool_call_chunks(&choice, &alias_to_original) {
                                let effective_id =
                                    resolve_openai_tool_call_stream_id(&call, &mut tool_call_real_ids);
                                if !effective_id.is_empty() && !call.name.is_empty() {
                                    yield StreamChunk::ToolCallStart {
                                        id: effective_id.clone(),
                                        name: call.name.clone(),
                                    };
                                }
                                if !effective_id.is_empty() && !call.args_delta.is_empty() {
                                    yield StreamChunk::ToolCallDelta {
                                        id: effective_id.clone(),
                                        args_delta: call.args_delta.clone(),
                                    };
                                }
                                if !effective_id.is_empty() {
                                    yield StreamChunk::ToolCallEnd { id: effective_id };
                                }
                            }

                            if let Some(reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                                if !reason.is_empty() {
                                    let usage = extract_usage(&value);
                                    yield StreamChunk::Done {
                                        finish_reason: reason.to_string(),
                                        usage,
                                    };
                                }
                            }
                        }
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }
}

struct OpenAIResponsesProvider {
    id: String,
    name: String,
    base_url: String,
    api_key: Option<String>,
    default_model: String,
    models: Vec<ModelInfo>,
    client: Client,
}

fn codex_supported_models(context_window: usize) -> Vec<ModelInfo> {
    let rows = [
        ("gpt-5.4", "GPT-5.4"),
        ("gpt-5.2-codex", "GPT-5.2-Codex"),
        ("gpt-5.1-codex-max", "GPT-5.1-Codex-Max"),
        ("gpt-5.4-mini", "GPT-5.4-Mini"),
        ("gpt-5.3-codex", "GPT-5.3-Codex"),
        ("gpt-5.3-codex-spark", "GPT-5.3-Codex-Spark"),
        ("gpt-5.1-codex-mini", "GPT-5.1-Codex-Mini"),
        ("gpt-5.4-pro", "GPT-5.4-Pro"),
    ];
    rows.into_iter()
        .map(|(id, display_name)| ModelInfo {
            id: id.to_string(),
            provider_id: "openai-codex".to_string(),
            display_name: display_name.to_string(),
            context_window,
        })
        .collect()
}

#[async_trait]
impl Provider for OpenAIResponsesProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            models: self.models.clone(),
        }
    }

    async fn complete(&self, prompt: &str, model_override: Option<&str>) -> anyhow::Result<String> {
        let model = model_override
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or(self.default_model.as_str());
        let url = format!("{}/responses", self.base_url);
        let mut body = json!({
            "model": model,
            "store": false,
            "instructions": default_openai_responses_instructions(),
            "tools": [],
            "tool_choice": "auto",
            "parallel_tool_calls": false,
            "input": [{
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": prompt
                }]
            }],
            "stream": false,
            "include": [],
        });
        if should_default_openai_reasoning(model) {
            body["reasoning"] = json!({
                "effort": "high",
                "summary": "auto"
            });
            body["include"] = json!(["reasoning.encrypted_content"]);
        }

        let mut response_opt = None;
        let mut last_send_err: Option<reqwest::Error> = None;
        let mut last_error_detail: Option<String> = None;
        for attempt in 0..3 {
            let mut req = self.client.post(url.clone()).json(&body);
            if let Some(api_key) = &self.api_key {
                req = req.bearer_auth(api_key);
            }

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if !status.is_success() {
                        let text = resp.text().await.unwrap_or_default();
                        last_error_detail = Some(format_openai_error_response(status, &text));
                        break;
                    }
                    response_opt = Some(resp);
                    break;
                }
                Err(err) => {
                    let retryable = err.is_connect() || err.is_timeout();
                    if retryable && attempt < 2 {
                        sleep(Duration::from_millis(300 * (attempt + 1) as u64)).await;
                        last_send_err = Some(err);
                        continue;
                    }
                    last_send_err = Some(err);
                    break;
                }
            }
        }

        let response = if let Some(resp) = response_opt {
            resp
        } else if let Some(detail) = last_error_detail {
            anyhow::bail!(detail);
        } else {
            let err = last_send_err.expect("send error should be captured");
            let category = if err.is_connect() {
                "connection error"
            } else if err.is_timeout() {
                "timeout"
            } else {
                "request error"
            };
            anyhow::bail!(
                "failed to reach provider `{}` at {} ({}): {}. Verify endpoint is reachable and OpenAI-compatible.",
                self.id,
                self.base_url,
                category,
                err
            );
        };

        let value: serde_json::Value = response.json().await?;
        if let Some(detail) = extract_openai_error(&value) {
            anyhow::bail!(detail);
        }
        if let Some(text) = extract_openai_text(&value) {
            return Ok(text);
        }

        let body_preview = truncate_for_error(&value.to_string(), 500);
        anyhow::bail!(
            "provider returned no completion content for model `{}` (response: {})",
            model,
            body_preview
        );
    }

    async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        model_override: Option<&str>,
        tool_mode: ToolMode,
        tools: Option<Vec<ToolSchema>>,
        cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let model = model_override
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or(self.default_model.as_str());
        let url = format!("{}/responses", self.base_url);
        let has_image_inputs = messages.iter().any(|m| !m.attachments.is_empty());
        if has_image_inputs && !model_supports_vision_input(model) {
            anyhow::bail!(
                "selected model `{}` does not appear to support image input. choose a vision-capable model.",
                model
            );
        }

        let (instructions, input_messages) = split_openai_responses_instructions(messages);
        let wire_messages = normalize_openai_messages(input_messages)
            .into_iter()
            .map(chat_message_to_openai_responses_wire)
            .collect::<Vec<_>>();

        let tools = tools.unwrap_or_default();
        let (original_to_alias, alias_to_original) = build_openai_tool_aliases(&tools);
        let wire_tools = tools
            .into_iter()
            .map(|tool| {
                let safe_name = original_to_alias
                    .get(tool.name.as_str())
                    .cloned()
                    .unwrap_or_else(|| sanitize_openai_function_name(&tool.name));
                json!({
                    "type": "function",
                    "name": safe_name,
                    "description": tool.description,
                    "parameters": normalize_codex_function_parameters(tool.input_schema),
                })
            })
            .collect::<Vec<_>>();
        let mut body = json!({
            "model": model,
            "store": false,
            "instructions": instructions,
            "tools": wire_tools,
            "tool_choice": openai_tool_choice(&tool_mode),
            "parallel_tool_calls": false,
            "input": wire_messages,
            "stream": true,
            "include": [],
        });
        if should_default_openai_reasoning(model) {
            body["reasoning"] = json!({
                "effort": "high",
                "summary": "auto"
            });
            body["include"] = json!(["reasoning.encrypted_content"]);
        }

        let mut resp_opt = None;
        let mut last_send_err: Option<reqwest::Error> = None;
        let mut last_error_detail: Option<String> = None;
        for attempt in 0..3 {
            let mut req = self.client.post(url.clone()).json(&body);
            if let Some(api_key) = &self.api_key {
                req = req.bearer_auth(api_key);
            }

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if !status.is_success() {
                        let text = resp.text().await.unwrap_or_default();
                        last_error_detail = Some(format_openai_error_response(status, &text));
                        break;
                    }
                    resp_opt = Some(resp);
                    break;
                }
                Err(err) => {
                    let retryable = err.is_connect() || err.is_timeout();
                    if retryable && attempt < 2 {
                        sleep(Duration::from_millis(300 * (attempt + 1) as u64)).await;
                        last_send_err = Some(err);
                        continue;
                    }
                    last_send_err = Some(err);
                    break;
                }
            }
        }

        let resp = if let Some(resp) = resp_opt {
            resp
        } else if let Some(detail) = last_error_detail {
            anyhow::bail!(detail);
        } else {
            let err = last_send_err.expect("send error should be captured");
            let category = if err.is_connect() {
                "connection error"
            } else if err.is_timeout() {
                "timeout"
            } else {
                "request error"
            };
            anyhow::bail!(
                "failed to reach provider `{}` at {} ({}): {}. Verify endpoint is reachable and OpenAI-compatible.",
                self.id,
                self.base_url,
                category,
                err
            );
        };

        let mut bytes = resp.bytes_stream();
        let alias_to_original = alias_to_original.clone();
        let stream = try_stream! {
            let mut buffer = String::new();
            let mut tool_call_names: HashMap<String, String> = HashMap::new();
            let mut started_tool_calls: HashSet<String> = HashSet::new();
            let mut ended_tool_calls: HashSet<String> = HashSet::new();
            let mut emitted_message_text: HashSet<String> = HashSet::new();
            let mut emitted_reasoning_text: HashSet<String> = HashSet::new();
            let mut saw_completion = false;
            let mut saw_tool_calls = false;

            while let Some(chunk) = bytes.next().await {
                if cancel.is_cancelled() {
                    if !saw_completion {
                        for call_id in started_tool_calls.iter().cloned().collect::<Vec<_>>() {
                            if ended_tool_calls.insert(call_id.clone()) {
                                yield StreamChunk::ToolCallEnd { id: call_id };
                            }
                        }
                        saw_completion = true;
                        yield StreamChunk::Done {
                            finish_reason: "cancelled".to_string(),
                            usage: None,
                        };
                    }
                    break;
                }

                let chunk = chunk?;
                buffer.push_str(str::from_utf8(&chunk).unwrap_or_default());

                while let Some(pos) = buffer.find("\n\n") {
                    let frame = buffer[..pos].to_string();
                    buffer = buffer[pos + 2..].to_string();
                    for line in frame.lines() {
                        if !line.starts_with("data: ") {
                            continue;
                        }
                        let payload = line.trim_start_matches("data: ").trim();
                        if payload == "[DONE]" {
                            if !saw_completion {
                                for call_id in started_tool_calls.iter().cloned().collect::<Vec<_>>() {
                                    if ended_tool_calls.insert(call_id.clone()) {
                                        yield StreamChunk::ToolCallEnd { id: call_id };
                                    }
                                }
                                yield StreamChunk::Done {
                                    finish_reason: if saw_tool_calls {
                                        "toolUse".to_string()
                                    } else {
                                        "stop".to_string()
                                    },
                                    usage: None,
                                };
                                saw_completion = true;
                            }
                            continue;
                        }

                        let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
                            continue;
                        };

                        if let Some(detail) = extract_openai_error(&value) {
                            Err(anyhow::anyhow!(detail))?;
                        }

                        let event_type = value
                            .get("type")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default();
                        match event_type {
                            "response.output_item.added" => {
                                if let Some(item) = value.get("item").and_then(|v| v.as_object()) {
                                    let item_type = item
                                        .get("type")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or_default();
                                    if item_type == "function_call" {
                                        saw_tool_calls = true;
                                        let call_id = item
                                            .get("call_id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or_default()
                                            .trim();
                                        let item_id = item
                                            .get("id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or_default()
                                            .trim();
                                        let call_key = responses_tool_call_key(
                                            call_id,
                                            item_id,
                                            value.get("output_index").and_then(|v| v.as_u64()),
                                        );
                                        if !call_key.is_empty() {
                                            let name = responses_tool_call_name(item, &alias_to_original);
                                            if !name.is_empty() {
                                                tool_call_names.insert(call_key.clone(), name.clone());
                                            }
                                            if started_tool_calls.insert(call_key.clone()) {
                                                yield StreamChunk::ToolCallStart {
                                                    id: call_key.clone(),
                                                    name: if name.is_empty() {
                                                        "tool".to_string()
                                                    } else {
                                                        name
                                                    },
                                                };
                                            }
                                        }
                                    }
                                }
                            }
                            "response.reasoning_summary_text.delta" => {
                                let item_id = value
                                    .get("item_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .trim()
                                    .to_string();
                                if !item_id.is_empty() {
                                    emitted_reasoning_text.insert(item_id.clone());
                                }
                                let delta = value
                                    .get("delta")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                if !delta.is_empty() {
                                    yield StreamChunk::ReasoningDelta(delta);
                                }
                            }
                            "response.output_text.delta" | "response.refusal.delta" => {
                                let item_id = value
                                    .get("item_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .trim()
                                    .to_string();
                                if !item_id.is_empty() {
                                    emitted_message_text.insert(item_id.clone());
                                }
                                let delta = value
                                    .get("delta")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                if !delta.is_empty() {
                                    yield StreamChunk::TextDelta(delta);
                                }
                            }
                            "response.function_call_arguments.delta" => {
                                saw_tool_calls = true;
                                let call_id = value
                                    .get("call_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .trim();
                                let item_id = value
                                    .get("item_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .trim();
                                let call_key = responses_tool_call_key(
                                    call_id,
                                    item_id,
                                    value.get("output_index").and_then(|v| v.as_u64()),
                                );
                                if call_key.is_empty() {
                                    continue;
                                }
                                if started_tool_calls.insert(call_key.clone()) {
                                    let name = tool_call_names
                                        .get(&call_key)
                                        .cloned()
                                        .unwrap_or_else(|| "tool".to_string());
                                    yield StreamChunk::ToolCallStart {
                                        id: call_key.clone(),
                                        name,
                                    };
                                }
                                let delta = value
                                    .get("delta")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .to_string();
                                if !delta.is_empty() {
                                    yield StreamChunk::ToolCallDelta {
                                        id: call_key,
                                        args_delta: delta,
                                    };
                                }
                            }
                            "response.function_call_arguments.done" => {
                                saw_tool_calls = true;
                                let call_id = value
                                    .get("call_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .trim();
                                let item_id = value
                                    .get("item_id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .trim();
                                let call_key = responses_tool_call_key(
                                    call_id,
                                    item_id,
                                    value.get("output_index").and_then(|v| v.as_u64()),
                                );
                                if !call_key.is_empty() && ended_tool_calls.insert(call_key.clone()) {
                                    yield StreamChunk::ToolCallEnd { id: call_key };
                                }
                            }
                            "response.output_item.done" => {
                                if let Some(item) = value.get("item").and_then(|v| v.as_object()) {
                                    let item_type = item
                                        .get("type")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or_default();
                                    if item_type == "message" {
                                        let item_id = item
                                            .get("id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or_default()
                                            .trim()
                                            .to_string();
                                        if !item_id.is_empty() && !emitted_message_text.contains(&item_id) {
                                            if let Some(text) = extract_openai_text(&serde_json::Value::Object(item.clone())) {
                                                emitted_message_text.insert(item_id);
                                                yield StreamChunk::TextDelta(text);
                                            }
                                        }
                                    } else if item_type == "reasoning" {
                                        let item_id = item
                                            .get("id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or_default()
                                            .trim()
                                            .to_string();
                                        if !item_id.is_empty() && !emitted_reasoning_text.contains(&item_id) {
                                            if let Some(summary) = extract_responses_reasoning_summary(item) {
                                                emitted_reasoning_text.insert(item_id);
                                                yield StreamChunk::ReasoningDelta(summary);
                                            }
                                        }
                                    } else if item_type == "function_call" {
                                        let call_id = item
                                            .get("call_id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or_default()
                                            .trim();
                                        let item_id = item
                                            .get("id")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or_default()
                                            .trim();
                                        let call_key = responses_tool_call_key(
                                            call_id,
                                            item_id,
                                            value.get("output_index").and_then(|v| v.as_u64()),
                                        );
                                        if !call_key.is_empty() && ended_tool_calls.insert(call_key.clone()) {
                                            yield StreamChunk::ToolCallEnd { id: call_key };
                                        }
                                    }
                                }
                            }
                            "response.completed" => {
                                let response = value
                                    .get("response")
                                    .and_then(|v| v.as_object())
                                    .cloned()
                                    .unwrap_or_default();
                                let usage = extract_responses_usage(&response);
                                let mut finish_reason = map_responses_stop_reason(
                                    response.get("status").and_then(|v| v.as_str()),
                                    response
                                        .get("incomplete_details")
                                        .and_then(|v| v.as_object())
                                        .and_then(|obj| obj.get("reason"))
                                        .and_then(|v| v.as_str()),
                                );
                                if finish_reason == "stop" && saw_tool_calls {
                                    finish_reason = "toolUse".to_string();
                                }
                                if !saw_completion {
                                    for call_id in started_tool_calls.iter().cloned().collect::<Vec<_>>() {
                                        if ended_tool_calls.insert(call_id.clone()) {
                                            yield StreamChunk::ToolCallEnd { id: call_id };
                                        }
                                    }
                                    yield StreamChunk::Done {
                                        finish_reason,
                                        usage,
                                    };
                                    saw_completion = true;
                                }
                            }
                            "error" => {
                                let code = value
                                    .get("code")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                let message = value
                                    .get("message")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("Unknown error");
                                Err(anyhow::anyhow!("Error Code {}: {}", code, message))?;
                            }
                            "response.failed" => {
                                let response = value
                                    .get("response")
                                    .and_then(|v| v.as_object())
                                    .cloned()
                                    .unwrap_or_default();
                                let detail = response
                                    .get("error")
                                    .and_then(|v| v.as_object())
                                    .map(|err| {
                                        let code = err
                                            .get("code")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("unknown");
                                        let message = err
                                            .get("message")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("no message");
                                        format!("{code}: {message}")
                                    })
                                    .or_else(|| {
                                        response
                                            .get("incomplete_details")
                                            .and_then(|v| v.as_object())
                                            .and_then(|obj| obj.get("reason"))
                                            .and_then(|v| v.as_str())
                                            .map(|reason| format!("incomplete: {reason}"))
                                    })
                                    .unwrap_or_else(|| "Unknown error (no error details in response)".to_string());
                                Err(anyhow::anyhow!(detail))?;
                            }
                            _ => {}
                        }
                    }
                }
            }

            if !saw_completion {
                for call_id in started_tool_calls.iter().cloned().collect::<Vec<_>>() {
                    if ended_tool_calls.insert(call_id.clone()) {
                        yield StreamChunk::ToolCallEnd { id: call_id };
                    }
                }
                yield StreamChunk::Done {
                    finish_reason: if saw_tool_calls {
                        "toolUse".to_string()
                    } else {
                        "stop".to_string()
                    },
                    usage: None,
                };
            }
        };

        Ok(Box::pin(stream))
    }
}

struct AnthropicProvider {
    api_key: Option<String>,
    default_model: String,
    client: Client,
}

struct CohereProvider {
    api_key: Option<String>,
    base_url: String,
    default_model: String,
    client: Client,
}

#[async_trait]
impl Provider for AnthropicProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "anthropic".to_string(),
            name: "Anthropic".to_string(),
            models: vec![ModelInfo {
                id: self.default_model.clone(),
                provider_id: "anthropic".to_string(),
                display_name: self.default_model.clone(),
                context_window: 200_000,
            }],
        }
    }

    async fn complete(&self, prompt: &str, model_override: Option<&str>) -> anyhow::Result<String> {
        let model = model_override
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or(self.default_model.as_str());
        let mut req = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": model,
                "max_tokens": 1024,
                "messages": [{"role":"user","content": prompt}],
            }));
        if let Some(key) = &self.api_key {
            req = req.header("x-api-key", key);
        }
        let value: serde_json::Value = req.send().await?.json().await?;
        let text = value["content"][0]["text"]
            .as_str()
            .unwrap_or("No completion content.")
            .to_string();
        Ok(text)
    }

    async fn stream(
        &self,
        messages: Vec<ChatMessage>,
        model_override: Option<&str>,
        _tool_mode: ToolMode,
        _tools: Option<Vec<ToolSchema>>,
        cancel: CancellationToken,
    ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>> {
        let model = model_override
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or(self.default_model.as_str());
        let mut req = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": model,
                "max_tokens": 1024,
                "stream": true,
                "messages": messages
                    .into_iter()
                    .map(|m| json!({"role": m.role, "content": m.content}))
                    .collect::<Vec<_>>(),
            }));
        if let Some(key) = &self.api_key {
            req = req.header("x-api-key", key);
        }

        let resp = req.send().await?;
        let mut bytes = resp.bytes_stream();
        let stream = try_stream! {
            let mut buffer = String::new();
            while let Some(chunk) = bytes.next().await {
                if cancel.is_cancelled() {
                    yield StreamChunk::Done {
                        finish_reason: "cancelled".to_string(),
                        usage: None,
                    };
                    break;
                }
                let chunk = chunk?;
                buffer.push_str(str::from_utf8(&chunk).unwrap_or_default());

                while let Some(pos) = buffer.find("\n\n") {
                    let frame = buffer[..pos].to_string();
                    buffer = buffer[pos + 2..].to_string();
                    for line in frame.lines() {
                        if !line.starts_with("data: ") {
                            continue;
                        }
                        let payload = line.trim_start_matches("data: ").trim();
                        if payload == "[DONE]" {
                            yield StreamChunk::Done {
                                finish_reason: "stop".to_string(),
                                usage: None,
                            };
                            continue;
                        }
                        let Ok(value) = serde_json::from_str::<serde_json::Value>(payload) else {
                            continue;
                        };
                        match value.get("type").and_then(|v| v.as_str()).unwrap_or_default() {
                            "content_block_delta" => {
                                if let Some(delta) = value.get("delta").and_then(|v| v.get("text")).and_then(|v| v.as_str()) {
                                    yield StreamChunk::TextDelta(delta.to_string());
                                }
                                if let Some(reasoning) = value.get("delta").and_then(|v| v.get("thinking")).and_then(|v| v.as_str()) {
                                    yield StreamChunk::ReasoningDelta(reasoning.to_string());
                                }
                            }
                            "message_stop" => {
                                yield StreamChunk::Done {
                                    finish_reason: "stop".to_string(),
                                    usage: None,
                                };
                            }
                            _ => {}
                        }
                    }
                }
            }
        };
        Ok(Box::pin(stream))
    }
}

#[async_trait]
impl Provider for CohereProvider {
    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: "cohere".to_string(),
            name: "Cohere".to_string(),
            models: vec![ModelInfo {
                id: self.default_model.clone(),
                provider_id: "cohere".to_string(),
                display_name: self.default_model.clone(),
                context_window: 128_000,
            }],
        }
    }

    async fn complete(&self, prompt: &str, model_override: Option<&str>) -> anyhow::Result<String> {
        let model = model_override
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .unwrap_or(self.default_model.as_str());
        let mut req = self
            .client
            .post(format!("{}/chat", self.base_url))
            .json(&json!({
                "model": model,
                "messages": [{"role":"user","content": prompt}],
            }));
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let value: serde_json::Value = req.send().await?.json().await?;
        let text = value["message"]["content"][0]["text"]
            .as_str()
            .or_else(|| value["text"].as_str())
            .unwrap_or("No completion content.")
            .to_string();
        Ok(text)
    }
}

fn chat_message_to_openai_wire(message: ChatMessage) -> serde_json::Value {
    if message.attachments.is_empty() {
        return json!({
            "role": message.role,
            "content": message.content
        });
    }

    let mut content = Vec::new();
    if !message.content.trim().is_empty() {
        content.push(json!({
            "type": "text",
            "text": message.content
        }));
    }

    for attachment in message.attachments {
        match attachment {
            ChatAttachment::ImageUrl { url } => content.push(json!({
                "type": "image_url",
                "image_url": { "url": url }
            })),
        }
    }

    if content.is_empty() {
        content.push(json!({"type": "text", "text": ""}));
    }

    json!({
        "role": message.role,
        "content": content
    })
}
