use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionRuleTemplate {
    pub permission: String,
    pub pattern: String,
    pub action: String,
}

fn canonical_tool_name(raw: &str) -> String {
    let cleaned = raw.trim().to_lowercase().replace('-', "_");
    match cleaned.as_str() {
        "update_todos" => "update_todo_list".to_string(),
        "todo_write" => "todowrite".to_string(),
        other => other.to_string(),
    }
}

fn allows_any(allowed_tools: Option<&[String]>, names: &[&str]) -> bool {
    let Some(allowed) = allowed_tools else {
        return true;
    };
    names
        .iter()
        .map(|name| canonical_tool_name(name))
        .any(|candidate| allowed.iter().any(|t| canonical_tool_name(t) == candidate))
}

pub fn build_mode_permission_rules(
    allowed_tools: Option<&[String]>,
) -> Vec<PermissionRuleTemplate> {
    let mut rules = Vec::new();

    if allows_any(allowed_tools, &["pack_builder"]) {
        rules.push(PermissionRuleTemplate {
            permission: "pack_builder".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        });
    }

    if allows_any(
        allowed_tools,
        &["ls", "list", "glob", "search", "grep", "codesearch"],
    ) {
        for permission in ["ls", "list", "glob", "search", "grep", "codesearch"] {
            rules.push(PermissionRuleTemplate {
                permission: permission.to_string(),
                pattern: "*".to_string(),
                action: "allow".to_string(),
            });
        }
    }

    if allows_any(allowed_tools, &["read"]) {
        rules.push(PermissionRuleTemplate {
            permission: "read".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        });
    }

    if allows_any(allowed_tools, &["write"]) {
        rules.push(PermissionRuleTemplate {
            permission: "write".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        });
    }
    if allows_any(allowed_tools, &["edit"]) {
        rules.push(PermissionRuleTemplate {
            permission: "edit".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        });
    }
    if allows_any(allowed_tools, &["apply_patch"]) {
        rules.push(PermissionRuleTemplate {
            permission: "apply_patch".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        });
    }

    if allows_any(
        allowed_tools,
        &["todowrite", "todo_write", "new_task", "update_todo_list"],
    ) {
        rules.push(PermissionRuleTemplate {
            permission: "todowrite".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        });
        rules.push(PermissionRuleTemplate {
            permission: "todo_write".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        });
    }

    if allows_any(allowed_tools, &["websearch"]) {
        rules.push(PermissionRuleTemplate {
            permission: "websearch".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        });
    }

    if allows_any(allowed_tools, &["webfetch"]) {
        rules.push(PermissionRuleTemplate {
            permission: "webfetch".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        });
    }

    if allows_any(allowed_tools, &["webfetch_html"]) {
        rules.push(PermissionRuleTemplate {
            permission: "webfetch_html".to_string(),
            pattern: "*".to_string(),
            action: "allow".to_string(),
        });
    }

    if allows_any(
        allowed_tools,
        &["bash", "shell", "cmd", "terminal", "run_command"],
    ) {
        rules.push(PermissionRuleTemplate {
            permission: "bash".to_string(),
            pattern: "*".to_string(),
            action: "ask".to_string(),
        });
    }

    rules
}

pub fn default_tui_permission_rules() -> Vec<PermissionRuleTemplate> {
    build_mode_permission_rules(None)
}

#[cfg(test)]
mod tests {
    use super::{build_mode_permission_rules, default_tui_permission_rules};

    #[test]
    fn defaults_allow_pack_builder() {
        let rules = default_tui_permission_rules();
        assert!(rules.iter().any(|rule| {
            rule.permission == "pack_builder" && rule.pattern == "*" && rule.action == "allow"
        }));
    }

    #[test]
    fn allowlist_controls_pack_builder_rule() {
        let denied = vec!["read".to_string()];
        let rules = build_mode_permission_rules(Some(&denied));
        assert!(!rules.iter().any(|rule| rule.permission == "pack_builder"));

        let allowed = vec!["pack_builder".to_string()];
        let rules = build_mode_permission_rules(Some(&allowed));
        assert!(rules.iter().any(|rule| rule.permission == "pack_builder"));
    }
}
