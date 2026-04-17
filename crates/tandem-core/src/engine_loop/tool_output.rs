use std::path::Path;

use serde_json::Value;

use super::{is_web_research_tool, stable_hash};

pub(super) fn is_productive_tool_output(tool_name: &str, output: &str) -> bool {
    let normalized_tool = super::normalize_tool_name(tool_name);
    if normalized_tool == "batch" && is_non_productive_batch_output(output) {
        return false;
    }
    if is_auth_required_tool_output(output) {
        return false;
    }
    if normalized_tool == "glob" {
        return true;
    }
    let Some(result_body) = extract_tool_result_body(output) else {
        return false;
    };
    !is_non_productive_tool_result_body(result_body)
}

pub(super) fn is_successful_web_research_output(tool_name: &str, output: &str) -> bool {
    if !is_web_research_tool(tool_name) {
        return false;
    }
    let Some(result_body) = extract_tool_result_body(output) else {
        return false;
    };
    if is_non_productive_tool_result_body(result_body) {
        return false;
    }
    let lower = result_body.to_ascii_lowercase();
    !(lower.contains("search timed out")
        || lower.contains("timed out")
        || lower.contains("no results received")
        || lower.contains("no search results")
        || lower.contains("no relevant results"))
}

pub(super) fn extract_tool_result_body(output: &str) -> Option<&str> {
    let trimmed = output.trim();
    let rest = trimmed.strip_prefix("Tool `")?;
    let (_, result_body) = rest.split_once("` result:")?;
    Some(result_body.trim())
}

pub(super) fn is_non_productive_tool_result_body(output: &str) -> bool {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    lower.starts_with("unknown tool:")
        || lower.contains("call skipped")
        || lower.contains("guard budget exceeded")
        || lower.contains("invalid_function_parameters")
        || is_terminal_tool_error_reason(trimmed)
}

pub(super) fn is_terminal_tool_error_reason(output: &str) -> bool {
    let first_line = output.lines().next().unwrap_or_default().trim();
    if first_line.is_empty() {
        return false;
    }
    let normalized = first_line.to_ascii_uppercase();
    matches!(
        normalized.as_str(),
        "TOOL_ARGUMENTS_MISSING"
            | "WEBSEARCH_QUERY_MISSING"
            | "BASH_COMMAND_MISSING"
            | "FILE_PATH_MISSING"
            | "WRITE_CONTENT_MISSING"
            | "WRITE_ARGS_EMPTY_FROM_PROVIDER"
            | "WRITE_ARGS_UNPARSEABLE_FROM_PROVIDER"
            | "WEBFETCH_URL_MISSING"
            | "PACK_BUILDER_PLAN_ID_MISSING"
            | "PACK_BUILDER_GOAL_MISSING"
            | "PROVIDER_REQUEST_FAILED"
            | "AUTHENTICATION_ERROR"
            | "CONTEXT_LENGTH_EXCEEDED"
            | "RATE_LIMIT_EXCEEDED"
    ) || normalized.ends_with("_MISSING")
        || normalized.ends_with("_ERROR")
}

pub(super) fn is_non_productive_batch_output(output: &str) -> bool {
    let Ok(value) = serde_json::from_str::<Value>(output.trim()) else {
        return false;
    };
    let Some(items) = value.as_array() else {
        return false;
    };
    if items.is_empty() {
        return true;
    }
    items.iter().all(|item| {
        let text = item
            .get("output")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or_default()
            .to_ascii_lowercase();
        text.is_empty()
            || text.starts_with("unknown tool:")
            || text.contains("call skipped")
            || text.contains("guard budget exceeded")
    })
}

pub(super) fn is_auth_required_tool_output(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    (lower.contains("authorization required")
        || lower.contains("requires authorization")
        || lower.contains("authorization pending"))
        && (lower.contains("authorize here") || lower.contains("http"))
}

#[derive(Debug, Clone)]
pub(super) struct McpAuthRequiredMetadata {
    pub(super) challenge_id: String,
    pub(super) authorization_url: String,
    pub(super) message: String,
    pub(super) server: Option<String>,
    pub(super) pending: bool,
    pub(super) blocked: bool,
    pub(super) retry_after_ms: Option<u64>,
}

pub(super) fn extract_mcp_auth_required_metadata(
    metadata: &Value,
) -> Option<McpAuthRequiredMetadata> {
    let auth = metadata.get("mcpAuth")?;
    if !auth
        .get("required")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return None;
    }
    let authorization_url = auth
        .get("authorizationUrl")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())?
        .to_string();
    let message = auth
        .get("message")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("This tool requires authorization before it can run.")
        .to_string();
    let challenge_id = auth
        .get("challengeId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("unknown")
        .to_string();
    let server = metadata
        .get("server")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string);
    let pending = auth
        .get("pending")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let blocked = auth
        .get("blocked")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let retry_after_ms = auth.get("retryAfterMs").and_then(|v| v.as_u64());
    Some(McpAuthRequiredMetadata {
        challenge_id,
        authorization_url,
        message,
        server,
        pending,
        blocked,
        retry_after_ms,
    })
}

pub(super) fn extract_mcp_auth_required_from_error_text(
    tool_name: &str,
    error_text: &str,
) -> Option<McpAuthRequiredMetadata> {
    let lower = error_text.to_ascii_lowercase();
    let auth_hint = lower.contains("authorization")
        || lower.contains("oauth")
        || lower.contains("invalid oauth token")
        || lower.contains("requires authorization");
    if !auth_hint {
        return None;
    }
    let authorization_url = find_first_url(error_text)?;
    let challenge_id = stable_hash(&format!("{tool_name}:{authorization_url}"));
    let server = tool_name
        .strip_prefix("mcp.")
        .and_then(|rest| rest.split('.').next())
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    Some(McpAuthRequiredMetadata {
        challenge_id,
        authorization_url,
        message: "This integration requires authorization before this action can run.".to_string(),
        server,
        pending: false,
        blocked: false,
        retry_after_ms: None,
    })
}

pub(super) fn summarize_auth_pending_outputs(outputs: &[String]) -> Option<String> {
    if outputs.is_empty()
        || !outputs
            .iter()
            .all(|output| is_auth_required_tool_output(output))
    {
        return None;
    }
    let mut auth_lines = outputs
        .iter()
        .filter_map(|output| {
            let trimmed = output.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect::<Vec<_>>();
    auth_lines.sort();
    auth_lines.dedup();
    if auth_lines.is_empty() {
        return None;
    }
    Some(format!(
        "Authorization is required before I can continue with this action.\n\n{}",
        auth_lines.join("\n\n")
    ))
}

pub(super) fn summarize_guard_budget_outputs(outputs: &[String]) -> Option<String> {
    if outputs.is_empty()
        || !outputs
            .iter()
            .all(|output| is_guard_budget_tool_output(output))
    {
        return None;
    }
    let mut lines = outputs
        .iter()
        .filter_map(|output| {
            let trimmed = output.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect::<Vec<_>>();
    lines.sort();
    lines.dedup();
    if lines.is_empty() {
        return None;
    }
    Some(format!(
        "This run hit the per-run tool guard budget, so I paused tool execution to avoid runaway retries.\n\n{}\n\nSend a new message to start a fresh run.",
        lines.join("\n")
    ))
}

pub(super) fn summarize_duplicate_signature_outputs(outputs: &[String]) -> Option<String> {
    if outputs.is_empty()
        || !outputs
            .iter()
            .all(|output| is_duplicate_signature_limit_output(output))
    {
        return None;
    }
    let mut lines = outputs
        .iter()
        .filter_map(|output| {
            let trimmed = output.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
        .collect::<Vec<_>>();
    lines.sort();
    lines.dedup();
    if lines.is_empty() {
        return None;
    }
    Some(format!(
        "This run paused because the same tool call kept repeating.\n\n{}\n\nRephrase the request or start a new message with a clearer command target.",
        lines.join("\n")
    ))
}

pub(super) fn find_first_url(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|token| {
        if token.starts_with("https://") || token.starts_with("http://") {
            let cleaned = token.trim_end_matches(&[')', ']', '}', '"', '\'', ',', '.'][..]);
            if cleaned.len() > "https://".len() {
                return Some(cleaned.to_string());
            }
        }
        None
    })
}

pub(super) fn is_guard_budget_tool_output(output: &str) -> bool {
    output
        .to_ascii_lowercase()
        .contains("per-run guard budget exceeded")
}

pub(super) fn is_duplicate_signature_limit_output(output: &str) -> bool {
    output
        .to_ascii_lowercase()
        .contains("duplicate call signature retry limit reached")
}

pub(super) fn is_sensitive_path_candidate(path: &Path) -> bool {
    let lowered = path.to_string_lossy().to_ascii_lowercase();

    // SSH / GPG directories
    if lowered.contains("/.ssh/") || lowered.ends_with("/.ssh") {
        return true;
    }
    if lowered.contains("/.gnupg/") || lowered.ends_with("/.gnupg") {
        return true;
    }

    // Cloud credential files
    if lowered.contains("/.aws/credentials")
        || lowered.contains("/.config/gcloud/")
        || lowered.contains("/.docker/config.json")
        || lowered.contains("/.kube/config")
        || lowered.contains("/.git-credentials")
    {
        return true;
    }

    // Package manager / tool secrets
    if lowered.ends_with("/.npmrc") || lowered.ends_with("/.netrc") || lowered.ends_with("/.pypirc")
    {
        return true;
    }

    // Known private key file names (use file_name() to avoid false positives on paths)
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        let n = name.to_ascii_lowercase();
        // .env files (but not .env.example — check no extra extension after .env)
        if n == ".env"
            || n.starts_with(".env.") && !n.ends_with(".example") && !n.ends_with(".sample")
        {
            return true;
        }
        // Key identity files
        if n.starts_with("id_rsa")
            || n.starts_with("id_ed25519")
            || n.starts_with("id_ecdsa")
            || n.starts_with("id_dsa")
        {
            return true;
        }
    }

    // Certificate / private key extensions — use extension() to avoid substring false positives
    // e.g. keyboard.rs has no .key extension, so it won't match here.
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        let ext_lower = ext.to_ascii_lowercase();
        if matches!(
            ext_lower.as_str(),
            "pem" | "p12" | "pfx" | "key" | "keystore" | "jks"
        ) {
            return true;
        }
    }

    false
}

pub(super) fn shell_command_targets_sensitive_path(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    let patterns = [
        "/.ssh/",
        "/.gnupg/",
        "/.aws/credentials",
        "/.config/gcloud/",
        "/.docker/config.json",
        "/.kube/config",
        "/.git-credentials",
        "id_rsa",
        "id_ed25519",
        "id_ecdsa",
        "id_dsa",
        ".npmrc",
        ".netrc",
        ".pypirc",
    ];
    // Check structural path patterns
    if patterns.iter().any(|p| lower.contains(p)) {
        return true;
    }
    // Check .env (standalone, not .env.example)
    if let Some(pos) = lower.find(".env") {
        let after = &lower[pos + 4..];
        if after.is_empty() || after.starts_with(' ') || after.starts_with('/') {
            return true;
        }
    }
    false
}
