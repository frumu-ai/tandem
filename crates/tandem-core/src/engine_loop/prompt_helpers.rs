use super::*;

pub(super) fn resolve_model_route(
    request_model: Option<&ModelSpec>,
    session_model: Option<&ModelSpec>,
) -> Option<(String, String)> {
    fn normalize(spec: &ModelSpec) -> Option<(String, String)> {
        let provider_id = spec.provider_id.trim();
        let model_id = spec.model_id.trim();
        if provider_id.is_empty() || model_id.is_empty() {
            return None;
        }
        Some((provider_id.to_string(), model_id.to_string()))
    }

    request_model
        .and_then(normalize)
        .or_else(|| session_model.and_then(normalize))
}

pub(super) fn strip_model_control_markers(input: &str) -> String {
    let mut cleaned = input.to_string();
    for marker in ["<|eom|>", "<|eot_id|>", "<|im_end|>", "<|end|>"] {
        if cleaned.contains(marker) {
            cleaned = cleaned.replace(marker, "");
        }
    }
    cleaned
}

pub(super) fn truncate_text(input: &str, max_len: usize) -> String {
    if input.len() <= max_len {
        return input.to_string();
    }
    let mut end = 0usize;
    for (idx, ch) in input.char_indices() {
        let next = idx + ch.len_utf8();
        if next > max_len {
            break;
        }
        end = next;
    }
    let mut out = input[..end].to_string();
    out.push_str("...<truncated>");
    out
}

pub(super) fn build_post_tool_final_narrative_prompt(tool_outputs: &[String]) -> String {
    format!(
        "Tool observations:\n{}\n\nUsing the tool observations and the existing conversation instructions, provide the required final answer now. Preserve any requested output contract, required JSON structure, required handoff fields, and required final status object from the original task. Do not call tools. Do not stop at a tool summary if the task requires a structured final response.",
        summarize_tool_outputs(tool_outputs)
    )
}

pub(super) fn provider_error_code(error_text: &str) -> &'static str {
    let lower = error_text.to_lowercase();
    if lower.contains("invalid_function_parameters")
        || lower.contains("array schema missing items")
        || lower.contains("tool schema")
    {
        return "TOOL_SCHEMA_INVALID";
    }
    if lower.contains("rate limit") || lower.contains("too many requests") || lower.contains("429")
    {
        return "RATE_LIMIT_EXCEEDED";
    }
    if lower.contains("context length")
        || lower.contains("max tokens")
        || lower.contains("token limit")
    {
        return "CONTEXT_LENGTH_EXCEEDED";
    }
    if lower.contains("unauthorized")
        || lower.contains("authentication")
        || lower.contains("401")
        || lower.contains("403")
    {
        return "AUTHENTICATION_ERROR";
    }
    if is_transient_provider_stream_error(error_text) {
        return "PROVIDER_SERVER_ERROR";
    }
    if lower.contains("timeout") || lower.contains("timed out") {
        return "TIMEOUT";
    }
    if lower.contains("server error")
        || lower.contains("500")
        || lower.contains("502")
        || lower.contains("503")
        || lower.contains("504")
    {
        return "PROVIDER_SERVER_ERROR";
    }
    "PROVIDER_REQUEST_FAILED"
}

pub(super) fn is_transient_provider_stream_error(error_text: &str) -> bool {
    let lower = error_text.to_ascii_lowercase();
    if lower.contains("invalid_function_parameters")
        || lower.contains("array schema missing items")
        || lower.contains("tool schema")
        || lower.contains("context length")
        || lower.contains("max tokens")
        || lower.contains("token limit")
        || lower.contains("unauthorized")
        || lower.contains("authentication")
        || lower.contains("401")
        || lower.contains("403")
    {
        return false;
    }
    lower.contains("provider stream chunk error")
        || lower.contains("stream chunk error")
        || lower.contains("error decoding response body")
        || lower.contains("unexpected eof")
        || lower.contains("incomplete streamed response")
}

pub(super) fn normalize_tool_name(name: &str) -> String {
    let mut normalized = name.trim().to_ascii_lowercase().replace('-', "_");
    for prefix in [
        "default_api:",
        "default_api.",
        "functions.",
        "function.",
        "tools.",
        "tool.",
        "builtin:",
        "builtin.",
    ] {
        if let Some(rest) = normalized.strip_prefix(prefix) {
            let trimmed = rest.trim();
            if !trimmed.is_empty() {
                normalized = trimmed.to_string();
                break;
            }
        }
    }
    match normalized.as_str() {
        "todowrite" | "update_todo_list" | "update_todos" => "todo_write".to_string(),
        "run_command" | "shell" | "powershell" | "cmd" => "bash".to_string(),
        other => other.to_string(),
    }
}

pub(super) fn mcp_server_from_tool_name(tool_name: &str) -> Option<&str> {
    let mut parts = tool_name.split('.');
    let prefix = parts.next()?;
    if prefix != "mcp" {
        return None;
    }
    parts.next().filter(|server| !server.is_empty())
}

pub(super) fn concrete_mcp_tools_required_before_write(
    tool_allowlist: &HashSet<String>,
) -> Vec<String> {
    let mut tools = tool_allowlist
        .iter()
        .filter_map(|tool| {
            let normalized = normalize_tool_name(tool);
            if normalized == "mcp_list"
                || !normalized.starts_with("mcp.")
                || normalized.contains('*')
                || normalized.split('.').count() < 3
                || is_mcp_source_evidence_tool(&normalized)
            {
                return None;
            }
            Some(normalized)
        })
        .collect::<Vec<_>>();
    tools.sort();
    tools.dedup();
    tools
}

pub(super) fn mcp_source_wildcards_required_before_write(
    tool_allowlist: &HashSet<String>,
) -> Vec<String> {
    let mut patterns = tool_allowlist
        .iter()
        .filter_map(|tool| {
            let normalized = normalize_tool_name(tool);
            if normalized == "mcp_list"
                || !normalized.starts_with("mcp.")
                || !normalized.ends_with(".*")
            {
                return None;
            }
            Some(normalized)
        })
        .collect::<Vec<_>>();
    patterns.extend(tool_allowlist.iter().filter_map(|tool| {
        let normalized = normalize_tool_name(tool);
        if !is_mcp_source_evidence_tool(&normalized) {
            return None;
        }
        let server = mcp_server_from_tool_name(&normalized)?;
        Some(format!("mcp.{server}.*"))
    }));
    patterns.sort();
    patterns.dedup();
    patterns
}

fn is_mcp_source_evidence_tool(tool_name: &str) -> bool {
    let normalized = normalize_tool_name(tool_name);
    if normalized == "mcp_list"
        || !normalized.starts_with("mcp.")
        || normalized.contains('*')
        || normalized.split('.').count() < 3
    {
        return false;
    }
    let action = normalized.rsplit('.').next().unwrap_or_default();
    let source_markers = [
        "fetch", "find", "get", "list", "read", "retrieve", "search", "top",
    ];
    let mutation_markers = [
        "add", "create", "delete", "edit", "merge", "move", "push", "reply", "request", "run",
        "update", "write",
    ];
    source_markers.iter().any(|marker| action.contains(marker))
        && !mutation_markers
            .iter()
            .any(|marker| action.contains(marker))
}

pub(super) fn has_attempted_concrete_mcp_for_wildcard(
    wildcard_patterns: &[String],
    tool_call_counts: &HashMap<String, usize>,
) -> bool {
    if wildcard_patterns.is_empty() {
        return true;
    }
    tool_call_counts.iter().any(|(tool, count)| {
        *count > 0
            && tool != "mcp_list"
            && tool.starts_with("mcp.")
            && tool.split('.').count() >= 3
            && wildcard_patterns
                .iter()
                .any(|pattern| tool_name_matches_policy(pattern, tool))
    })
}

pub(super) fn concrete_mcp_tool_matches_wildcard(
    tool_name: &str,
    wildcard_patterns: &[String],
) -> bool {
    let normalized = normalize_tool_name(tool_name);
    normalized.starts_with("mcp.")
        && normalized != "mcp_list"
        && normalized.split('.').count() >= 3
        && wildcard_patterns
            .iter()
            .any(|pattern| tool_name_matches_policy(pattern, &normalized))
}

pub(super) fn has_unattempted_required_mcp_tool(
    required_tools: &[String],
    tool_call_counts: &HashMap<String, usize>,
) -> bool {
    !unattempted_required_mcp_tools(required_tools, tool_call_counts).is_empty()
}

pub(super) fn unattempted_required_mcp_tools(
    required_tools: &[String],
    tool_call_counts: &HashMap<String, usize>,
) -> HashSet<String> {
    required_tools
        .iter()
        .filter(|tool| tool_call_counts.get(*tool).copied().unwrap_or(0) == 0)
        .cloned()
        .collect()
}

pub(super) fn tool_allowed_by_session_policy(
    tool_name: &str,
    allowed_tools: &[String],
    requested_write_required: bool,
) -> bool {
    if allowed_tools.is_empty() {
        return true;
    }
    let normalized = normalize_tool_name(tool_name);
    any_policy_matches(allowed_tools, &normalized)
        || (requested_write_required && is_workspace_write_tool(tool_name))
}

pub(super) fn provider_tool_mode_for_selected_tools(
    requested_tool_mode: &ToolMode,
    selected_tool_count: usize,
) -> ToolMode {
    if selected_tool_count == 0 && matches!(requested_tool_mode, ToolMode::Required) {
        ToolMode::Auto
    } else {
        requested_tool_mode.clone()
    }
}

pub(super) fn requires_web_research_prompt(input: &str) -> bool {
    let lower = input.to_ascii_lowercase();
    [
        "research",
        "top news",
        "today's news",
        "todays news",
        "with links",
        "latest headlines",
        "current events",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

pub(super) fn requires_structured_handoff_final_response_prompt(input: &str) -> bool {
    let lower = input.to_ascii_lowercase();
    lower.contains("structured handoff expectation")
        || (lower.contains("output contract kind: structured_json")
            && lower.contains("structured handoff"))
        || (lower.contains("required structured handoff json") && lower.contains("final response"))
}

pub(super) fn structured_handoff_loop_guard_final_retry_context(outputs: &[String]) -> String {
    let mut context = "A tool loop guard just prevented another repeated tool call. Stop calling tools now and return the required structured JSON handoff in the final response body, followed by the compact JSON status object. Use JSON only, with no headings, markdown fences, prose, or follow-up questions. Preserve any evidence already gathered in this session; if evidence is incomplete, record the limitation inside the JSON and set the top-level status to completed unless the task is semantically impossible.".to_string();
    let summary = summarize_tool_outputs(outputs);
    if !summary.trim().is_empty() {
        context.push_str("\n\nLoop-guard note:\n");
        context.push_str(&summary);
    }
    context
}

pub(super) fn provider_usage_token_counts(
    provider_usage: Option<&TokenUsage>,
    estimated_prompt_chars: usize,
    completion_chars: usize,
) -> (u64, u64, u64, &'static str) {
    if let Some(usage) = provider_usage {
        return (
            usage.prompt_tokens,
            usage.completion_tokens,
            usage.total_tokens,
            "provider",
        );
    }

    let prompt_tokens = (estimated_prompt_chars / 4) as u64;
    let completion_tokens = (completion_chars / 4) as u64;
    (
        prompt_tokens,
        completion_tokens,
        prompt_tokens.saturating_add(completion_tokens),
        "estimated",
    )
}

pub(super) fn requires_email_delivery_prompt(input: &str) -> bool {
    let lower = input.to_ascii_lowercase();
    (lower.contains("send") && lower.contains("email"))
        || (lower.contains("send") && lower.contains('@') && lower.contains("to"))
        || lower.contains("email to")
}

pub(super) fn has_web_research_tools(schemas: &[ToolSchema]) -> bool {
    schemas.iter().any(|schema| {
        let name = normalize_tool_name(&schema.name);
        name == "websearch" || name == "webfetch" || name == "webfetch_html"
    })
}

pub(super) fn has_email_action_tools(schemas: &[ToolSchema]) -> bool {
    schemas
        .iter()
        .map(|schema| normalize_tool_name(&schema.name))
        .any(|name| tool_name_looks_like_email_action(&name))
}

pub(super) fn tool_name_looks_like_email_action(name: &str) -> bool {
    let normalized = normalize_tool_name(name);
    if normalized.starts_with("mcp.") {
        return normalized.contains("gmail")
            || normalized.contains("mail")
            || normalized.contains("email");
    }
    normalized.contains("mail") || normalized.contains("email")
}

pub(super) fn completion_claims_email_sent(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let has_email_marker = lower.contains("email status")
        || lower.contains("emailed")
        || lower.contains("email sent")
        || lower.contains("sent to");
    has_email_marker
        && (lower.contains("sent")
            || lower.contains("delivered")
            || lower.contains("has been sent"))
}

pub(super) fn extract_tool_candidate_paths(tool: &str, args: &Value) -> Vec<String> {
    let Some(obj) = args.as_object() else {
        return Vec::new();
    };
    // For MCP tools, probe a wider set of path-like keys since MCP schemas vary by server.
    let mcp_path_keys: &[&str] = &[
        "path",
        "file_path",
        "filePath",
        "filepath",
        "filename",
        "directory",
        "dir",
        "cwd",
        "target",
        "source",
        "dest",
        "destination",
    ];
    let keys: &[&str] = if tool.starts_with("mcp.") {
        mcp_path_keys
    } else {
        match tool {
            "read" | "write" | "edit" | "grep" | "codesearch" => &["path", "filePath", "cwd"],
            "glob" => &["pattern"],
            "lsp" => &["filePath", "path"],
            "bash" => &["cwd"],
            "apply_patch" => &[],
            _ => &["path", "cwd"],
        }
    };
    keys.iter()
        .filter_map(|key| obj.get(*key))
        .filter_map(|value| value.as_str())
        .filter(|s| {
            let t = s.trim();
            // Exclude placeholder/empty strings or obvious non-paths
            !t.is_empty()
                && (t.starts_with('/')
                    || t.starts_with('.')
                    || t.starts_with('~')
                    || t.contains('/'))
        })
        .map(ToString::to_string)
        .collect()
}

/// Returns true if the MCP server name is in the operator-configured exemption list.
/// Set `TANDEM_MCP_SANDBOX_EXEMPT_SERVERS` to a comma-separated list of server names
/// (e.g. `composio,github`) to exempt those servers from workspace path containment.
pub(super) fn is_mcp_sandbox_exempt_server(server_name: &str) -> bool {
    if matches!(
        server_name,
        "tandem_mcp" | "tandem-mcp" | "tandemDocs" | "tandem_docs" | "tandem-docs"
    ) {
        return true;
    }
    let Ok(raw) = std::env::var("TANDEM_MCP_SANDBOX_EXEMPT_SERVERS") else {
        return false;
    };
    raw.split(',')
        .any(|s| s.trim().eq_ignore_ascii_case(server_name))
}

pub(super) fn is_mcp_tool_name(tool_name: &str) -> bool {
    let normalized = normalize_tool_name(tool_name);
    normalized == "mcp_list" || normalized.starts_with("mcp.")
}

pub(super) fn agent_can_use_tool(agent: &AgentDefinition, tool_name: &str) -> bool {
    let target = normalize_tool_name(tool_name);
    match agent.tools.as_ref() {
        None => true,
        Some(list) => {
            let normalized = list
                .iter()
                .map(|t| normalize_tool_name(t))
                .collect::<Vec<_>>();
            any_policy_matches(&normalized, &target)
        }
    }
}

pub(super) fn enforce_skill_scope(
    tool_name: &str,
    args: Value,
    equipped_skills: Option<&[String]>,
) -> Result<Value, String> {
    if normalize_tool_name(tool_name) != "skill" {
        return Ok(args);
    }
    let Some(configured) = equipped_skills else {
        return Ok(args);
    };

    let mut allowed = configured
        .iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    if allowed
        .iter()
        .any(|s| s == "*" || s.eq_ignore_ascii_case("all"))
    {
        return Ok(args);
    }
    allowed.sort();
    allowed.dedup();
    if allowed.is_empty() {
        return Err("No skills are equipped for this agent.".to_string());
    }

    let requested = args
        .get("name")
        .and_then(|v| v.as_str())
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    if !requested.is_empty() && !allowed.iter().any(|s| s == &requested) {
        return Err(format!(
            "Skill '{}' is not equipped for this agent. Equipped skills: {}",
            requested,
            allowed.join(", ")
        ));
    }

    let mut out = if let Some(obj) = args.as_object() {
        Value::Object(obj.clone())
    } else {
        json!({})
    };
    if let Some(obj) = out.as_object_mut() {
        obj.insert("allowed_skills".to_string(), json!(allowed));
    }
    Ok(out)
}

pub(super) fn is_read_only_tool(tool_name: &str) -> bool {
    matches!(
        normalize_tool_name(tool_name).as_str(),
        "glob"
            | "read"
            | "grep"
            | "search"
            | "codesearch"
            | "list"
            | "ls"
            | "lsp"
            | "websearch"
            | "webfetch"
            | "webfetch_html"
    )
}

pub(super) fn is_workspace_write_tool(tool_name: &str) -> bool {
    matches!(
        normalize_tool_name(tool_name).as_str(),
        "write" | "edit" | "apply_patch"
    )
}

pub(super) fn should_start_prewrite_repair_before_first_write(
    repair_on_unmet_requirements: bool,
    productive_write_tool_calls_total: usize,
    prewrite_satisfied: bool,
    code_workflow_requested: bool,
) -> bool {
    (repair_on_unmet_requirements || code_workflow_requested)
        && productive_write_tool_calls_total == 0
        && !prewrite_satisfied
}

pub(super) fn is_batch_wrapper_tool_name(name: &str) -> bool {
    matches!(
        normalize_tool_name(name).as_str(),
        "default_api" | "default" | "api" | "function" | "functions" | "tool" | "tools"
    )
}

pub(super) fn stable_hash(input: &str) -> String {
    let mut hasher = DefaultHasher::new();
    input.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub(super) fn summarize_tool_outputs(outputs: &[String]) -> String {
    outputs
        .iter()
        .take(6)
        .map(|output| truncate_text(output, 600))
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub(super) fn summarize_user_visible_tool_outputs(outputs: &[String]) -> String {
    let filtered = outputs
        .iter()
        .filter(|output| !should_hide_tool_output_from_user_fallback(output))
        .take(3)
        .map(|output| truncate_text(output, 240))
        .collect::<Vec<_>>();
    filtered.join("\n")
}

pub(super) fn should_hide_tool_output_from_user_fallback(output: &str) -> bool {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.contains("call skipped")
        || lower.contains("it is not available in this turn")
        || is_terminal_tool_error_reason(trimmed)
    {
        return true;
    }
    extract_tool_result_body(trimmed).is_some_and(is_non_productive_tool_result_body)
}

pub(super) fn summarize_terminal_tool_failure_for_user(outputs: &[String]) -> Option<String> {
    let reasons = outputs
        .iter()
        .filter_map(|output| terminal_tool_error_reason(output))
        .collect::<Vec<_>>();
    if reasons.is_empty() {
        return None;
    }
    if reasons.iter().any(|reason| *reason == "DOC_PATH_MISSING") {
        return Some(
            "I couldn't tell which Tandem docs page to open. Please include a docs URL like `https://docs.tandem.ac/start-here/` or a docs path like `/start-here/` and try again."
                .to_string(),
        );
    }
    if reasons
        .iter()
        .any(|reason| *reason == "QUERY_MISSING" || *reason == "WEBSEARCH_QUERY_MISSING")
    {
        return Some(
            "I need a concrete search query or target URL to continue. Please include the exact thing you want searched and try again."
                .to_string(),
        );
    }
    if reasons.iter().any(|reason| *reason == "TASK_MISSING") {
        return Some(
            "I need the actual docs/help question in the prompt before I can answer it. Please resend the request with the question you want answered."
                .to_string(),
        );
    }
    None
}

pub(super) fn terminal_tool_error_reason(output: &str) -> Option<&str> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }
    let first_line = trimmed.lines().next().unwrap_or_default().trim();
    if first_line.is_empty() {
        return None;
    }
    let normalized = first_line.to_ascii_uppercase();
    if is_terminal_tool_error_reason(&normalized) {
        Some(first_line)
    } else {
        None
    }
}

pub(super) fn is_os_mismatch_tool_output(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("os error 3")
        || lower.contains("system cannot find the path specified")
        || lower.contains("command not found")
        || lower.contains("is not recognized as an internal or external command")
        || lower.contains("shell command blocked on windows")
}

pub(super) fn should_force_workspace_probe(user_text: &str, completion: &str) -> bool {
    let user = user_text.to_lowercase();
    let reply = completion.to_lowercase();

    let asked_for_project_context = [
        "what is this project",
        "what's this project",
        "what project is this",
        "explain this project",
        "analyze this project",
        "inspect this project",
        "look at the project",
        "summarize this project",
        "show me this project",
        "what files are in",
        "show files",
        "list files",
        "read files",
        "browse files",
        "use glob",
        "run glob",
    ]
    .iter()
    .any(|needle| user.contains(needle));

    if !asked_for_project_context {
        return false;
    }

    let assistant_claimed_no_access = [
        "can't inspect",
        "cannot inspect",
        "unable to inspect",
        "unable to directly inspect",
        "can't access",
        "cannot access",
        "unable to access",
        "can't read files",
        "cannot read files",
        "unable to read files",
        "tool restriction",
        "tool restrictions",
        "don't have visibility",
        "no visibility",
        "haven't been able to inspect",
        "i don't know what this project is",
        "need your help to",
        "sandbox",
        "restriction",
        "system restriction",
        "permissions restrictions",
    ]
    .iter()
    .any(|needle| reply.contains(needle));

    // If the user is explicitly asking for project inspection and the model replies with
    // a no-access narrative instead of making a tool call, force a minimal read-only probe.
    asked_for_project_context && assistant_claimed_no_access
}
