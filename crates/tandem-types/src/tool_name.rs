const KNOWN_TOOL_NAMESPACE_PREFIXES: &[&str] = &[
    "default_api:",
    "default_api.",
    "functions.",
    "function.",
    "tools.",
    "tool.",
    "builtin:",
    "builtin.",
];

pub fn canonical_tool_name(name: &str) -> String {
    let normalized = name.trim().to_ascii_lowercase().replace('-', "_");
    let stripped = strip_known_tool_namespace(&normalized).unwrap_or(normalized.as_str());
    match stripped {
        "todowrite" | "update_todo_list" | "update_todos" => "todo_write".to_string(),
        "run_command" | "shell" | "powershell" | "cmd" => "bash".to_string(),
        "code_search" => "codesearch".to_string(),
        "task_create" => "taskcreate".to_string(),
        "task_list" => "tasklist".to_string(),
        "task_update" => "taskupdate".to_string(),
        "team_create" => "teamcreate".to_string(),
        "send_message" => "sendmessage".to_string(),
        "web_search" => "websearch".to_string(),
        other => other.to_string(),
    }
}

pub fn strip_known_tool_namespace(name: &str) -> Option<&str> {
    KNOWN_TOOL_NAMESPACE_PREFIXES.iter().find_map(|prefix| {
        name.strip_prefix(prefix)
            .map(str::trim)
            .filter(|rest| !rest.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalizes_aliases_case_and_known_namespaces() {
        let cases = [
            (" READ ", "read"),
            ("todo-write", "todo_write"),
            ("todowrite", "todo_write"),
            ("update_todos", "todo_write"),
            ("functions.shell", "bash"),
            ("default_api:run_command", "bash"),
            ("code_search", "codesearch"),
            ("task_create", "taskcreate"),
            ("team_create", "teamcreate"),
            ("web_search", "websearch"),
            ("tools.write", "write"),
            ("builtin:webfetch", "webfetch"),
        ];
        for (raw, expected) in cases {
            assert_eq!(canonical_tool_name(raw), expected, "{raw}");
        }
    }
}
