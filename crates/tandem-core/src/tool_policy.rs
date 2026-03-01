pub fn tool_name_matches_policy(pattern: &str, tool_name: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() {
        return false;
    }
    if pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return tool_name.starts_with(prefix);
    }
    pattern == tool_name
}

pub fn any_policy_matches(patterns: &[String], tool_name: &str) -> bool {
    patterns
        .iter()
        .any(|pattern| tool_name_matches_policy(pattern, tool_name))
}

#[cfg(test)]
mod tests {
    use super::{any_policy_matches, tool_name_matches_policy};

    #[test]
    fn tool_name_matches_policy_supports_exact_and_wildcards() {
        assert!(tool_name_matches_policy("read", "read"));
        assert!(!tool_name_matches_policy("read", "write"));
        assert!(tool_name_matches_policy("*", "mcp.github.issues_list"));
        assert!(tool_name_matches_policy(
            "mcp.github.*",
            "mcp.github.issues_list"
        ));
        assert!(!tool_name_matches_policy(
            "mcp.github.*",
            "mcp.composio.gmail_send"
        ));
        assert!(tool_name_matches_policy("mcp.composio.", "mcp.composio."));
        assert!(!tool_name_matches_policy("", "read"));
    }

    #[test]
    fn any_policy_matches_handles_mixed_patterns() {
        let patterns = vec![
            "write".to_string(),
            "mcp.composio.*".to_string(),
            "read".to_string(),
        ];
        assert!(any_policy_matches(&patterns, "read"));
        assert!(any_policy_matches(&patterns, "mcp.composio.gmail_send"));
        assert!(!any_policy_matches(&patterns, "mcp.github.issues_list"));
    }
}
