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

const FIRST_PARTY_PRODUCT_READ_TOOLS: &[&str] =
    &["orchestration_validate", "goal_get", "wait_inspect"];
const FIRST_PARTY_PRODUCT_DRAFT_TOOLS: &[&str] = &["orchestration_create_draft"];
const FIRST_PARTY_PRODUCT_CONSEQUENTIAL_TOOLS: &[&str] = &[
    "orchestration_publish",
    "goal_start",
    "goal_cancel",
    "handoff_emit",
    "handoff_approve",
    "wait_resolve",
];

fn push_tool_rules(
    rules: &mut Vec<PermissionRuleTemplate>,
    allowed_tools: Option<&[String]>,
    names: &[&str],
    action: &str,
) {
    for permission in names {
        if allows_any(allowed_tools, &[*permission]) {
            rules.push(PermissionRuleTemplate {
                permission: (*permission).to_string(),
                pattern: "*".to_string(),
                action: action.to_string(),
            });
        }
    }
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

    push_tool_rules(
        &mut rules,
        allowed_tools,
        FIRST_PARTY_PRODUCT_READ_TOOLS,
        "allow",
    );
    push_tool_rules(
        &mut rules,
        allowed_tools,
        FIRST_PARTY_PRODUCT_DRAFT_TOOLS,
        "allow",
    );
    push_tool_rules(
        &mut rules,
        allowed_tools,
        FIRST_PARTY_PRODUCT_CONSEQUENTIAL_TOOLS,
        "ask",
    );

    if allows_any(
        allowed_tools,
        &[
            "ls",
            "list",
            "glob",
            "search",
            "grep",
            "codesearch",
            "repo.context_bundle",
            "repo.search",
            "repo.symbol",
            "repo.neighbors",
            "repo.impact",
            "repo.test_targets",
        ],
    ) {
        for permission in [
            "ls",
            "list",
            "glob",
            "search",
            "grep",
            "codesearch",
            "repo.context_bundle",
            "repo.search",
            "repo.symbol",
            "repo.neighbors",
            "repo.impact",
            "repo.test_targets",
        ] {
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

    let default_write_action = if allowed_tools.is_none() {
        "ask"
    } else {
        "allow"
    };
    if allows_any(allowed_tools, &["write"]) {
        rules.push(PermissionRuleTemplate {
            permission: "write".to_string(),
            pattern: "*".to_string(),
            action: default_write_action.to_string(),
        });
    }
    if allows_any(allowed_tools, &["edit"]) {
        rules.push(PermissionRuleTemplate {
            permission: "edit".to_string(),
            pattern: "*".to_string(),
            action: default_write_action.to_string(),
        });
    }
    if allows_any(allowed_tools, &["apply_patch"]) {
        rules.push(PermissionRuleTemplate {
            permission: "apply_patch".to_string(),
            pattern: "*".to_string(),
            action: default_write_action.to_string(),
        });
    }
    if allows_any(allowed_tools, &["repo.index", "repo.update_changed_files"]) {
        for permission in ["repo.index", "repo.update_changed_files"] {
            rules.push(PermissionRuleTemplate {
                permission: permission.to_string(),
                pattern: "*".to_string(),
                action: default_write_action.to_string(),
            });
        }
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

    #[test]
    fn default_write_tools_require_prompt() {
        let rules = default_tui_permission_rules();
        for permission in ["write", "edit", "apply_patch"] {
            assert!(rules.iter().any(|rule| {
                rule.permission == permission && rule.pattern == "*" && rule.action == "ask"
            }));
        }
    }

    #[test]
    fn product_drafts_are_allowed_but_consequential_controls_ask() {
        let rules = default_tui_permission_rules();
        assert!(rules.iter().any(|rule| {
            rule.permission == "orchestration_create_draft" && rule.action == "allow"
        }));
        for permission in ["orchestration_publish", "goal_start", "goal_cancel"] {
            assert!(rules
                .iter()
                .any(|rule| rule.permission == permission && rule.action == "ask"));
        }
    }

    #[test]
    fn product_rules_respect_an_explicit_tool_allowlist() {
        let allowed = vec![
            "orchestration_create_draft".to_string(),
            "orchestration_validate".to_string(),
        ];
        let rules = build_mode_permission_rules(Some(&allowed));
        assert!(rules
            .iter()
            .any(|rule| rule.permission == "orchestration_create_draft"));
        assert!(rules
            .iter()
            .any(|rule| rule.permission == "orchestration_validate"));
        assert!(!rules
            .iter()
            .any(|rule| rule.permission == "orchestration_publish"));
    }
}
