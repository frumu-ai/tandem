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

