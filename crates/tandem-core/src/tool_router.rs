use std::collections::HashSet;

use tandem_types::ToolSchema;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolIntent {
    Chitchat,
    Knowledge,
    WorkspaceRead,
    WorkspaceWrite,
    ShellExec,
    WebLookup,
    MemoryOps,
    McpExplicit,
}

#[derive(Debug, Clone)]
pub struct ToolRoutingDecision {
    pub pass: u8,
    pub mode: &'static str,
    pub intent: ToolIntent,
    pub selected_count: usize,
    pub total_available_count: usize,
    pub mcp_included: bool,
}

pub fn tool_router_enabled() -> bool {
    std::env::var("TANDEM_TOOL_ROUTER_ENABLED")
        .ok()
        .map(|raw| {
            !matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "0" | "false" | "off" | "no"
            )
        })
        .unwrap_or(true)
}

pub fn max_tools_per_call() -> usize {
    std::env::var("TANDEM_TOOL_ROUTER_MAX_TOOLS")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(12)
}

pub fn max_tools_per_call_expanded() -> usize {
    std::env::var("TANDEM_TOOL_ROUTER_MAX_TOOLS_EXPANDED")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|v| *v > 0)
        .unwrap_or(24)
}

pub fn is_short_simple_prompt(input: &str) -> bool {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.len() > 72 {
        return false;
    }
    let words = trimmed.split_whitespace().count();
    words > 0 && words <= 10
}

pub fn classify_intent(input: &str) -> ToolIntent {
    let lower = input.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return ToolIntent::Knowledge;
    }

    if lower.contains("mcp")
        || lower.contains("arcade")
        || lower.contains("mcp.")
        || lower.contains("integration")
    {
        return ToolIntent::McpExplicit;
    }

    if contains_any(
        &lower,
        &[
            "memory_search",
            "memory_store",
            "memory_list",
            "remember",
            "memory",
        ],
    ) {
        return ToolIntent::MemoryOps;
    }

    if contains_any(
        &lower,
        &[
            "websearch",
            "web search",
            "search web",
            "internet",
            "online",
            "website",
            "url",
        ],
    ) {
        return ToolIntent::WebLookup;
    }

    if contains_any(
        &lower,
        &[
            "run ",
            "execute",
            "bash",
            "shell",
            "command",
            "terminal",
            "powershell",
            "cmd",
        ],
    ) {
        return ToolIntent::ShellExec;
    }

    if contains_any(
        &lower,
        &[
            "write",
            "edit",
            "patch",
            "modify",
            "update file",
            "create file",
            "refactor",
            "apply",
        ],
    ) {
        return ToolIntent::WorkspaceWrite;
    }

    if contains_any(
        &lower,
        &[
            "read",
            "open file",
            "search",
            "grep",
            "find in",
            "codebase",
            "repository",
            "repo",
            ".rs",
            ".ts",
            ".py",
            "/src",
            "file",
            "folder",
            "directory",
        ],
    ) {
        return ToolIntent::WorkspaceRead;
    }

    if is_chitchat_phrase(&lower) {
        return ToolIntent::Chitchat;
    }

    ToolIntent::Knowledge
}

pub fn should_escalate_auto_tools(
    intent: ToolIntent,
    user_text: &str,
    first_pass_completion: &str,
) -> bool {
    if matches!(
        intent,
        ToolIntent::WorkspaceRead
            | ToolIntent::WorkspaceWrite
            | ToolIntent::ShellExec
            | ToolIntent::WebLookup
            | ToolIntent::MemoryOps
            | ToolIntent::McpExplicit
    ) {
        return true;
    }

    let completion = first_pass_completion.to_ascii_lowercase();
    if contains_any(
        &completion,
        &[
            "need to inspect",
            "need to read",
            "need to check files",
            "cannot access local files",
            "use tools",
            "tool access",
            "need to run",
            "need to search",
        ],
    ) {
        return true;
    }

    let lower_user = user_text.to_ascii_lowercase();
    contains_any(
        &lower_user,
        &[
            " in engine/",
            " in src/",
            " in docs/",
            "from code",
            "local code",
        ],
    )
}

pub fn select_tool_subset(
    available: Vec<ToolSchema>,
    intent: ToolIntent,
    request_allowlist: &HashSet<String>,
    expanded: bool,
) -> Vec<ToolSchema> {
    let max_count = if expanded {
        max_tools_per_call_expanded()
    } else {
        max_tools_per_call()
    };

    let mut selected = Vec::new();
    let mut seen = HashSet::new();
    let include_mcp = intent == ToolIntent::McpExplicit;

    for schema in available {
        let norm = normalize_tool_name(&schema.name);
        let explicitly_allowed = !request_allowlist.is_empty() && request_allowlist.contains(&norm);
        if !request_allowlist.is_empty() && !explicitly_allowed {
            continue;
        }
        if !include_mcp && norm.starts_with("mcp.") && !explicitly_allowed {
            continue;
        }
        if !tool_matches_intent(intent, &norm) && !explicitly_allowed {
            continue;
        }
        if seen.insert(norm) {
            selected.push(schema);
            if selected.len() >= max_count {
                break;
            }
        }
    }

    selected
}

pub fn default_mode_name() -> &'static str {
    "auto"
}

fn tool_matches_intent(intent: ToolIntent, name: &str) -> bool {
    match intent {
        ToolIntent::Chitchat | ToolIntent::Knowledge => false,
        ToolIntent::WorkspaceRead => matches!(
            name,
            "glob"
                | "read"
                | "grep"
                | "search"
                | "codesearch"
                | "lsp"
                | "list"
                | "ls"
                | "webfetch"
                | "webfetch_html"
        ),
        ToolIntent::WorkspaceWrite => matches!(
            name,
            "glob"
                | "read"
                | "grep"
                | "search"
                | "codesearch"
                | "write"
                | "edit"
                | "apply_patch"
                | "bash"
                | "batch"
        ),
        ToolIntent::ShellExec => matches!(
            name,
            "bash" | "batch" | "glob" | "read" | "grep" | "search" | "codesearch"
        ),
        ToolIntent::WebLookup => matches!(
            name,
            "websearch" | "webfetch" | "webfetch_html" | "read" | "grep"
        ),
        ToolIntent::MemoryOps => matches!(name, "memory_search" | "memory_store" | "memory_list"),
        ToolIntent::McpExplicit => {
            name.starts_with("mcp.") || matches!(name, "read" | "grep" | "search")
        }
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn is_chitchat_phrase(input: &str) -> bool {
    let normalized = input
        .chars()
        .filter_map(|ch| {
            if ch.is_alphanumeric() || ch.is_whitespace() {
                Some(ch)
            } else {
                None
            }
        })
        .collect::<String>();
    let trimmed = normalized.trim();
    matches!(
        trimmed,
        "hi" | "hello"
            | "hey"
            | "thanks"
            | "thank you"
            | "ok"
            | "okay"
            | "yo"
            | "good morning"
            | "good afternoon"
            | "good evening"
    )
}

pub fn normalize_tool_name(name: &str) -> String {
    let lowered = name.trim().to_ascii_lowercase().replace('-', "_");
    let canonical = if let Some(stripped) = strip_known_namespace(&lowered) {
        stripped
    } else {
        lowered
    };
    match canonical.as_str() {
        "shell" | "powershell" | "cmd" | "run_command" => "bash".to_string(),
        "todowrite" | "update_todo_list" | "update_todos" => "todo_write".to_string(),
        other => other.to_string(),
    }
}

fn strip_known_namespace(name: &str) -> Option<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn schema(name: &str) -> ToolSchema {
        ToolSchema {
            name: name.to_string(),
            description: String::new(),
            input_schema: serde_json::json!({}),
        }
    }

    #[test]
    fn classifies_short_greeting_as_chitchat() {
        assert_eq!(classify_intent("hello"), ToolIntent::Chitchat);
    }

    #[test]
    fn classifies_repo_query_as_workspace_read() {
        assert_eq!(
            classify_intent("use local code evidence in engine/src/main.rs"),
            ToolIntent::WorkspaceRead
        );
    }

    #[test]
    fn allowlist_can_force_selection_even_when_intent_has_no_default_tools() {
        let mut allowlist = HashSet::new();
        allowlist.insert("read".to_string());
        let selected = select_tool_subset(
            vec![schema("read"), schema("bash")],
            ToolIntent::Knowledge,
            &allowlist,
            false,
        );
        assert_eq!(selected.len(), 1);
        assert_eq!(normalize_tool_name(&selected[0].name), "read");
    }

    #[test]
    fn mcp_tools_hidden_without_explicit_intent_or_allowlist() {
        let selected = select_tool_subset(
            vec![schema("mcp.arcade.gmail_create"), schema("read")],
            ToolIntent::WorkspaceRead,
            &HashSet::new(),
            false,
        );
        assert_eq!(selected.len(), 1);
        assert_eq!(normalize_tool_name(&selected[0].name), "read");
    }
}
