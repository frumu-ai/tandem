use std::collections::HashSet;

use crate::tool_capabilities::{
    canonical_tool_name, tool_schema_matches_profile, tool_schema_risk_tier, ToolCapabilityProfile,
};
use crate::tool_policy::tool_name_matches_policy;
use tandem_types::{ToolRiskTier, ToolSchema};

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
    ProductAuthoring,
    ProductAuthoringWithMcp,
    ProductControl,
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
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "on" | "yes"
            )
        })
        .unwrap_or(false)
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

pub(crate) fn is_mcp_tool_or_discovery(tool_name: &str) -> bool {
    let normalized = normalize_tool_name(tool_name);
    normalized.starts_with("mcp.")
        || matches!(
            normalized.as_str(),
            "mcp_list" | "mcp_list_catalog" | "mcp_request_capability"
        )
}

fn is_product_authoring_intent(intent: ToolIntent) -> bool {
    matches!(
        intent,
        ToolIntent::ProductAuthoring | ToolIntent::ProductAuthoringWithMcp
    )
}

fn is_connector_only_allowlist(request_allowlist: &HashSet<String>) -> bool {
    !request_allowlist.is_empty()
        && request_allowlist
            .iter()
            .all(|tool| is_mcp_tool_or_discovery(tool))
        && request_allowlist
            .iter()
            .any(|tool| normalize_tool_name(tool).starts_with("mcp."))
}

pub(crate) fn tool_survives_explicit_allowlist(
    schema: &ToolSchema,
    request_allowlist: &HashSet<String>,
    intent: ToolIntent,
) -> bool {
    let normalized = normalize_tool_name(&schema.name);
    request_allowlist
        .iter()
        .any(|pattern| tool_name_matches_policy(pattern, &normalized))
        || (is_product_authoring_intent(intent)
            && is_connector_only_allowlist(request_allowlist)
            && is_product_authoring_tool(schema))
}

pub(crate) fn product_authoring_needs_catalog_fallback(
    intent: ToolIntent,
    candidate_tools: &[ToolSchema],
    all_tools: &[ToolSchema],
) -> bool {
    if !is_product_authoring_intent(intent) {
        return false;
    }
    let has_planner_start = |tools: &[ToolSchema]| {
        tools
            .iter()
            .any(|schema| normalize_tool_name(&schema.name) == "workflow_plan_start")
    };
    has_planner_start(all_tools) && !has_planner_start(candidate_tools)
}

pub(crate) fn product_authoring_execution_scope(
    request_allowlist: &HashSet<String>,
    intent: ToolIntent,
    all_tools: &[ToolSchema],
) -> Vec<String> {
    let mut scope = request_allowlist
        .iter()
        .filter(|tool| intent != ToolIntent::ProductAuthoring || !is_mcp_tool_or_discovery(tool))
        .cloned()
        .collect::<HashSet<_>>();
    if is_product_authoring_intent(intent) && is_connector_only_allowlist(request_allowlist) {
        scope.extend(
            all_tools
                .iter()
                .filter(|schema| {
                    is_product_authoring_tool(schema) && !is_mcp_tool_or_discovery(&schema.name)
                })
                .map(|schema| normalize_tool_name(&schema.name)),
        );
    }
    let mut scope = scope.into_iter().collect::<Vec<_>>();
    scope.sort();
    scope
}

pub fn classify_intent(input: &str) -> ToolIntent {
    let lower = input.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return ToolIntent::Knowledge;
    }

    if is_product_control_request(&lower) {
        return ToolIntent::ProductControl;
    }

    let product_authoring = is_product_authoring_request(&lower);
    let live_integration_inspection = is_live_integration_inspection_request(&lower);
    if product_authoring && live_integration_inspection {
        return ToolIntent::ProductAuthoringWithMcp;
    }
    if live_integration_inspection {
        return ToolIntent::McpExplicit;
    }
    if product_authoring {
        return ToolIntent::ProductAuthoring;
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
            | ToolIntent::ProductAuthoring
            | ToolIntent::ProductAuthoringWithMcp
            | ToolIntent::ProductControl
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
    mut available: Vec<ToolSchema>,
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
    let include_mcp = matches!(
        intent,
        ToolIntent::McpExplicit | ToolIntent::ProductAuthoringWithMcp
    );
    let product_intent = matches!(
        intent,
        ToolIntent::ProductAuthoring
            | ToolIntent::ProductAuthoringWithMcp
            | ToolIntent::ProductControl
    );
    if product_intent {
        available.sort_by_key(|schema| {
            let name = normalize_tool_name(&schema.name);
            let explicitly_allowed = !request_allowlist.is_empty()
                && request_allowlist
                    .iter()
                    .any(|pattern| tool_name_matches_policy(pattern, &name));
            (!explicitly_allowed, product_tool_priority(&name, intent))
        });
    }

    for schema in available {
        let norm = normalize_tool_name(&schema.name);
        if intent == ToolIntent::ProductAuthoring && is_mcp_tool_or_discovery(&norm) {
            continue;
        }
        let explicitly_allowed = !request_allowlist.is_empty()
            && request_allowlist
                .iter()
                .any(|pattern| tool_name_matches_policy(pattern, &norm));
        let intrinsic_product_tool = product_intent
            && !is_mcp_tool_or_discovery(&norm)
            && tool_schema_matches_profile(&schema, ToolCapabilityProfile::ProductControl);
        if !request_allowlist.is_empty() && !explicitly_allowed && !intrinsic_product_tool {
            continue;
        }
        if !include_mcp && is_mcp_tool_or_discovery(&norm) && !explicitly_allowed {
            continue;
        }
        if !tool_matches_intent(intent, &schema) && !explicitly_allowed {
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

fn product_tool_priority(name: &str, intent: ToolIntent) -> u8 {
    if intent == ToolIntent::ProductControl {
        return if matches!(
            name,
            "automation_control"
                | "orchestration_publish"
                | "goal_start"
                | "goal_cancel"
                | "wait_resolve"
        ) {
            0
        } else if name.ends_with("_inspect")
            || name.ends_with("_read")
            || name.ends_with("_validate")
            || name == "goal_get"
            || name == "workflow_plan_capabilities"
        {
            1
        } else {
            2
        };
    }
    if name.starts_with("workflow_plan_")
        || matches!(
            name,
            "automation_inspect" | "automation_manage_draft" | "automation_control"
        )
        || matches!(name, "orchestration_inspect" | "orchestration_create_draft")
    {
        0
    } else {
        1
    }
}

pub fn default_mode_name() -> &'static str {
    "auto"
}

fn tool_matches_intent(intent: ToolIntent, schema: &ToolSchema) -> bool {
    let name = normalize_tool_name(&schema.name);
    match intent {
        ToolIntent::Chitchat | ToolIntent::Knowledge => false,
        ToolIntent::WorkspaceRead => {
            tool_schema_matches_profile(schema, ToolCapabilityProfile::WorkspaceRead)
                || tool_schema_matches_profile(schema, ToolCapabilityProfile::WorkspaceDiscover)
                || tool_schema_matches_profile(schema, ToolCapabilityProfile::WebResearch)
                || matches!(name.as_str(), "lsp")
        }
        ToolIntent::WorkspaceWrite => {
            tool_schema_matches_profile(schema, ToolCapabilityProfile::WorkspaceRead)
                || tool_schema_matches_profile(schema, ToolCapabilityProfile::WorkspaceDiscover)
                || tool_schema_matches_profile(schema, ToolCapabilityProfile::ArtifactWrite)
                || tool_schema_matches_profile(schema, ToolCapabilityProfile::VerifyCommand)
                || matches!(name.as_str(), "batch")
        }
        ToolIntent::ShellExec => {
            tool_schema_matches_profile(schema, ToolCapabilityProfile::ShellExecution)
                || tool_schema_matches_profile(schema, ToolCapabilityProfile::WorkspaceRead)
                || tool_schema_matches_profile(schema, ToolCapabilityProfile::WorkspaceDiscover)
                || matches!(name.as_str(), "batch")
        }
        ToolIntent::WebLookup => {
            tool_schema_matches_profile(schema, ToolCapabilityProfile::WebResearch)
                || tool_schema_matches_profile(schema, ToolCapabilityProfile::WorkspaceRead)
                || tool_schema_matches_profile(schema, ToolCapabilityProfile::WorkspaceDiscover)
        }
        ToolIntent::MemoryOps => {
            tool_schema_matches_profile(schema, ToolCapabilityProfile::MemoryOperation)
        }
        ToolIntent::ProductAuthoring => is_product_authoring_tool(schema),
        ToolIntent::ProductAuthoringWithMcp => {
            is_product_authoring_tool(schema) || is_mcp_tool_or_discovery(&name)
        }
        ToolIntent::ProductControl => {
            tool_schema_matches_profile(schema, ToolCapabilityProfile::ProductControl)
        }
        ToolIntent::McpExplicit => {
            is_mcp_tool_or_discovery(&name) || matches!(name.as_str(), "read" | "grep" | "search")
        }
    }
}

fn is_product_authoring_tool(schema: &ToolSchema) -> bool {
    tool_schema_matches_profile(schema, ToolCapabilityProfile::ProductControl)
        && matches!(
            tool_schema_risk_tier(schema),
            ToolRiskTier::ReadDiscover | ToolRiskTier::InternalWrite | ToolRiskTier::ExternalDraft
        )
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn is_product_control_request(input: &str) -> bool {
    if !has_product_resource_signal(input) || has_repository_workflow_signal(input) {
        return false;
    }
    contains_any(
        input,
        &[
            "publish",
            "enable",
            "activate",
            "disable",
            "cancel",
            "archive",
            "delete",
            "approve",
            "reject",
            "resolve wait",
            "start goal",
            "run workflow",
            "start workflow",
            "launch workflow",
        ],
    )
}

fn is_product_authoring_request(input: &str) -> bool {
    if has_repository_workflow_signal(input) {
        return false;
    }
    let action = contains_any(
        input,
        &[
            "create",
            "build",
            "make",
            "draft",
            "design",
            "plan",
            "schedule",
            "automate",
            "orchestrate",
            "revise",
            "update",
            "modify",
            "edit",
            "duplicate",
            "validate",
            "materialize",
            "inspect",
            "show",
            "list",
            "add ",
            "remove ",
            "what automation",
            "what workflow",
        ],
    );
    let pronoun_follow_up = contains_any(input, &["revise it", "revise this", "revise that"]);
    action && (has_product_resource_signal(input) || pronoun_follow_up)
}

fn is_live_integration_inspection_request(input: &str) -> bool {
    if contains_any(
        input,
        &[
            "do not use mcp",
            "don't use mcp",
            "without mcp",
            "no mcp",
            "do not use composio",
            "don't use composio",
            "without composio",
        ],
    ) {
        return false;
    }
    let integration_subject =
        contains_any(input, &["live integration", "integration tools", "mcp"]);
    let inspection_action = contains_any(
        input,
        &["inspect", "discover", "test", "list", "check", "show"],
    );
    (integration_subject && inspection_action) || has_singular_integration_inspection_clause(input)
}

fn has_singular_integration_inspection_clause(input: &str) -> bool {
    for action in ["inspect", "discover", "test", "list", "check", "show"] {
        let needle = format!("{action} ");
        for (action_index, _) in input.match_indices(&needle) {
            if action_index > 0 && input.as_bytes()[action_index - 1].is_ascii_alphanumeric() {
                continue;
            }
            let after_action = &input[action_index + needle.len()..];
            let Some(subject_index) = after_action.find("integration") else {
                continue;
            };
            if subject_index > 64 {
                continue;
            }
            let between = &after_action[..subject_index];
            if !contains_any(
                between,
                &[" and ", " then ", " but ", " to ", ".", ",", ";", "?", "!"],
            ) {
                return true;
            }
        }
    }
    false
}

fn has_repository_workflow_signal(input: &str) -> bool {
    contains_any(
        input,
        &[
            ".github/workflows",
            "github action",
            "github workflow",
            "ci workflow",
            "workflow file",
            "workflow test",
            "test workflow",
            "actions workflow",
        ],
    )
}

fn has_product_resource_signal(input: &str) -> bool {
    contains_any(
        input,
        &[
            "workflow",
            "automation",
            "orchestration",
            "pipeline",
            "workflow plan",
            "this plan",
            "approval step",
            "trigger node",
            "workflow node",
            "transition key",
        ],
    )
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
    canonical_tool_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema(name: &str) -> ToolSchema {
        ToolSchema::new(name, "", serde_json::json!({}))
    }

    fn product_schema(
        name: &str,
        effect: tandem_types::ToolEffect,
        risk_tier: ToolRiskTier,
    ) -> ToolSchema {
        ToolSchema::new(name, "", serde_json::json!({}))
            .with_capabilities(
                tandem_types::ToolCapabilities::new()
                    .effect(effect)
                    .domain(tandem_types::ToolDomain::Planning),
            )
            .with_security(tandem_types::ToolSecurityDescriptor::new().risk_tier(risk_tier))
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
    fn classifies_product_authoring_before_mcp_setup_language() {
        assert_eq!(
            classify_intent("Create a workflow that uses the Slack MCP integration"),
            ToolIntent::ProductAuthoring
        );
        assert_eq!(
            classify_intent("Add an approval step to this workflow"),
            ToolIntent::ProductAuthoring
        );
        assert_eq!(classify_intent("Revise it"), ToolIntent::ProductAuthoring);
        assert_eq!(
            classify_intent("Materialize that workflow as a draft"),
            ToolIntent::ProductAuthoring
        );
    }

    #[test]
    fn classifies_consequential_product_controls_separately() {
        assert_eq!(
            classify_intent("Publish and enable this workflow"),
            ToolIntent::ProductControl
        );
        assert_eq!(
            classify_intent("How do Tandem workflows work?"),
            ToolIntent::Knowledge
        );
        assert_eq!(classify_intent("run workflow tests"), ToolIntent::ShellExec);
        assert_eq!(
            classify_intent("edit the GitHub workflow file"),
            ToolIntent::WorkspaceWrite
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

    #[test]
    fn allowlist_patterns_can_select_mcp_tools_in_first_pass() {
        let mut allowlist = HashSet::new();
        allowlist.insert("mcp.arcade.*".to_string());
        let selected = select_tool_subset(
            vec![schema("mcp.arcade.gmail_create"), schema("read")],
            ToolIntent::Knowledge,
            &allowlist,
            false,
        );
        assert_eq!(selected.len(), 1);
        assert_eq!(
            normalize_tool_name(&selected[0].name),
            "mcp.arcade.gmail_create"
        );
    }

    #[test]
    fn product_authoring_excludes_external_tools_even_with_an_allowlist() {
        let mut allowlist = HashSet::new();
        allowlist.insert("mcp.slack.*".to_string());
        allowlist.insert("mcp_list".to_string());
        allowlist.insert("mcp_list_catalog".to_string());
        allowlist.insert("mcp_request_capability".to_string());
        let selected = select_tool_subset(
            vec![
                product_schema(
                    "orchestration_create_draft",
                    tandem_types::ToolEffect::Write,
                    ToolRiskTier::InternalWrite,
                ),
                product_schema(
                    "orchestration_validate",
                    tandem_types::ToolEffect::Read,
                    ToolRiskTier::ReadDiscover,
                ),
                product_schema(
                    "orchestration_publish",
                    tandem_types::ToolEffect::Write,
                    ToolRiskTier::ConsequentialWrite,
                ),
                schema("mcp.slack.post_message"),
                schema("mcp_list"),
                schema("mcp_list_catalog"),
                schema("mcp_request_capability"),
            ],
            ToolIntent::ProductAuthoring,
            &allowlist,
            false,
        );
        let names = selected
            .iter()
            .map(|schema| normalize_tool_name(&schema.name))
            .collect::<Vec<_>>();
        assert!(names.contains(&"orchestration_create_draft".to_string()));
        assert!(names.contains(&"orchestration_validate".to_string()));
        assert!(!names.contains(&"mcp.slack.post_message".to_string()));
        assert!(!names.contains(&"mcp_list".to_string()));
        assert!(!names.contains(&"mcp_list_catalog".to_string()));
        assert!(!names.contains(&"mcp_request_capability".to_string()));
        assert!(!names.contains(&"orchestration_publish".to_string()));
    }

    #[test]
    fn connector_only_allowlist_preserves_intrinsic_product_authoring_tools() {
        let allowlist = HashSet::from(["mcp.slack.*".to_string()]);
        let planner = product_schema(
            "workflow_plan_start",
            tandem_types::ToolEffect::Write,
            ToolRiskTier::InternalWrite,
        );
        let unrelated = schema("read");

        assert!(tool_survives_explicit_allowlist(
            &planner,
            &allowlist,
            ToolIntent::ProductAuthoring,
        ));
        assert!(!tool_survives_explicit_allowlist(
            &unrelated,
            &allowlist,
            ToolIntent::ProductAuthoring,
        ));
    }

    #[test]
    fn narrow_product_allowlist_does_not_grant_other_authoring_tools() {
        let allowlist = HashSet::from(["workflow_plan_capabilities".to_string()]);
        let capabilities = product_schema(
            "workflow_plan_capabilities",
            tandem_types::ToolEffect::Read,
            ToolRiskTier::ReadDiscover,
        );
        let planner = product_schema(
            "workflow_plan_start",
            tandem_types::ToolEffect::Write,
            ToolRiskTier::InternalWrite,
        );

        assert!(tool_survives_explicit_allowlist(
            &capabilities,
            &allowlist,
            ToolIntent::ProductAuthoring,
        ));
        assert!(!tool_survives_explicit_allowlist(
            &planner,
            &allowlist,
            ToolIntent::ProductAuthoring,
        ));
        assert_eq!(
            product_authoring_execution_scope(
                &allowlist,
                ToolIntent::ProductAuthoring,
                &[capabilities, planner],
            ),
            vec!["workflow_plan_capabilities".to_string()]
        );
    }

    #[test]
    fn discovery_only_allowlist_does_not_grant_authoring_tools() {
        let allowlist = HashSet::from([
            "mcp_list".to_string(),
            "mcp_list_catalog".to_string(),
            "mcp_request_capability".to_string(),
        ]);
        let planner = product_schema(
            "workflow_plan_start",
            tandem_types::ToolEffect::Write,
            ToolRiskTier::InternalWrite,
        );

        assert!(!tool_survives_explicit_allowlist(
            &planner,
            &allowlist,
            ToolIntent::ProductAuthoring,
        ));
        assert!(product_authoring_execution_scope(
            &allowlist,
            ToolIntent::ProductAuthoring,
            &[planner],
        )
        .is_empty());
    }

    #[test]
    fn connector_only_allowlist_extends_product_authoring_execution_scope() {
        let allowlist = HashSet::from(["mcp.slack.*".to_string()]);
        let tools = vec![
            product_schema(
                "workflow_plan_start",
                tandem_types::ToolEffect::Write,
                ToolRiskTier::InternalWrite,
            ),
            product_schema(
                "workflow_plan_materialize",
                tandem_types::ToolEffect::Write,
                ToolRiskTier::InternalWrite,
            ),
            product_schema(
                "orchestration_publish",
                tandem_types::ToolEffect::Write,
                ToolRiskTier::ConsequentialWrite,
            ),
            product_schema(
                "mcp.slack.post_message",
                tandem_types::ToolEffect::Write,
                ToolRiskTier::ExternalDraft,
            ),
            schema("read"),
        ];

        let scope =
            product_authoring_execution_scope(&allowlist, ToolIntent::ProductAuthoring, &tools);
        assert!(!scope.contains(&"mcp.slack.*".to_string()));
        assert!(scope.contains(&"workflow_plan_start".to_string()));
        assert!(scope.contains(&"workflow_plan_materialize".to_string()));
        assert!(!scope.contains(&"orchestration_publish".to_string()));
        assert!(!scope.contains(&"mcp.slack.post_message".to_string()));
        assert!(!scope.contains(&"read".to_string()));

        let mixed_scope = product_authoring_execution_scope(
            &allowlist,
            ToolIntent::ProductAuthoringWithMcp,
            &tools,
        );
        assert!(mixed_scope.contains(&"mcp.slack.*".to_string()));
        assert!(mixed_scope.contains(&"workflow_plan_start".to_string()));
        assert!(mixed_scope.contains(&"workflow_plan_materialize".to_string()));

        assert!(product_authoring_execution_scope(
            &HashSet::new(),
            ToolIntent::ProductAuthoring,
            &tools,
        )
        .is_empty());
    }

    #[test]
    fn product_authoring_falls_back_when_retrieval_misses_planner_start() {
        let candidates = vec![product_schema(
            "automation_manage_draft",
            tandem_types::ToolEffect::Write,
            ToolRiskTier::InternalWrite,
        )];
        let all_tools = vec![
            candidates[0].clone(),
            product_schema(
                "workflow_plan_start",
                tandem_types::ToolEffect::Write,
                ToolRiskTier::InternalWrite,
            ),
        ];

        assert!(product_authoring_needs_catalog_fallback(
            ToolIntent::ProductAuthoring,
            &candidates,
            &all_tools,
        ));
        assert!(!product_authoring_needs_catalog_fallback(
            ToolIntent::ProductAuthoring,
            &all_tools,
            &all_tools,
        ));
        assert!(!product_authoring_needs_catalog_fallback(
            ToolIntent::Knowledge,
            &candidates,
            &all_tools,
        ));
    }

    #[test]
    fn explicit_live_integration_inspection_uses_mcp_tools() {
        assert_eq!(
            classify_intent("Inspect the live integration tools for Slack"),
            ToolIntent::McpExplicit
        );
        assert_eq!(
            classify_intent(
                "Create a Slack automation and inspect the live integration tools first"
            ),
            ToolIntent::ProductAuthoringWithMcp
        );
        assert_eq!(
            classify_intent("Create a Slack automation and list the integration tools first"),
            ToolIntent::ProductAuthoringWithMcp
        );
        assert_eq!(
            classify_intent("Create an automation and check MCP tools before drafting"),
            ToolIntent::ProductAuthoringWithMcp
        );
        assert_eq!(
            classify_intent("Create a Slack automation and inspect the Slack MCP integration"),
            ToolIntent::ProductAuthoringWithMcp
        );
        assert_eq!(
            classify_intent("Create a Slack automation and inspect Slack MCP first"),
            ToolIntent::ProductAuthoringWithMcp
        );
        assert_eq!(
            classify_intent("Create a workflow and check the Slack integration"),
            ToolIntent::ProductAuthoringWithMcp
        );
        assert_eq!(
            classify_intent("Create a workflow and test the Slack integration"),
            ToolIntent::ProductAuthoringWithMcp
        );
        assert_eq!(
            classify_intent("Create an automation but do not use MCP"),
            ToolIntent::ProductAuthoring
        );
        assert_eq!(
            classify_intent("Create a workflow that checks integration status every hour"),
            ToolIntent::ProductAuthoring
        );
    }

    #[test]
    fn mixed_integration_authoring_offers_planner_and_mcp_tools() {
        let allowlist = HashSet::from(["mcp.slack.*".to_string()]);
        let selected = select_tool_subset(
            vec![
                product_schema(
                    "workflow_plan_start",
                    tandem_types::ToolEffect::Write,
                    ToolRiskTier::InternalWrite,
                ),
                product_schema(
                    "workflow_plan_materialize",
                    tandem_types::ToolEffect::Write,
                    ToolRiskTier::InternalWrite,
                ),
                schema("mcp.slack.search_tools"),
                schema("read"),
            ],
            ToolIntent::ProductAuthoringWithMcp,
            &allowlist,
            false,
        );
        let names = selected
            .iter()
            .map(|schema| normalize_tool_name(&schema.name))
            .collect::<Vec<_>>();

        assert!(names.contains(&"workflow_plan_start".to_string()));
        assert!(names.contains(&"workflow_plan_materialize".to_string()));
        assert!(names.contains(&"mcp.slack.search_tools".to_string()));
        assert!(!names.contains(&"read".to_string()));
    }

    #[test]
    fn explicit_product_control_can_offer_consequential_tools() {
        let selected = select_tool_subset(
            vec![product_schema(
                "orchestration_publish",
                tandem_types::ToolEffect::Write,
                ToolRiskTier::ConsequentialWrite,
            )],
            ToolIntent::ProductControl,
            &HashSet::new(),
            false,
        );
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "orchestration_publish");
    }

    #[test]
    fn product_control_prioritizes_publish_before_the_tool_cap() {
        let mut available = (0..12)
            .map(|index| {
                product_schema(
                    &format!("workflow_plan_read_{index}"),
                    tandem_types::ToolEffect::Read,
                    ToolRiskTier::ReadDiscover,
                )
            })
            .collect::<Vec<_>>();
        available.push(product_schema(
            "orchestration_publish",
            tandem_types::ToolEffect::Write,
            ToolRiskTier::ConsequentialWrite,
        ));
        let selected = select_tool_subset(
            available,
            ToolIntent::ProductControl,
            &HashSet::new(),
            false,
        );
        assert!(selected
            .iter()
            .any(|schema| schema.name == "orchestration_publish"));
    }

    #[test]
    fn external_allowlist_does_not_displace_product_tools_at_the_cap() {
        let mut available = (0..12)
            .map(|index| {
                product_schema(
                    &format!("workflow_plan_read_{index}"),
                    tandem_types::ToolEffect::Read,
                    ToolRiskTier::ReadDiscover,
                )
            })
            .collect::<Vec<_>>();
        available.push(schema("mcp.slack.post_message"));
        let selected = select_tool_subset(
            available,
            ToolIntent::ProductAuthoring,
            &HashSet::from(["mcp.slack.*".to_string()]),
            false,
        );
        assert_eq!(selected.len(), 12);
        assert!(!selected
            .iter()
            .any(|schema| schema.name == "mcp.slack.post_message"));
    }

    #[test]
    fn generic_planning_tools_are_not_product_controls() {
        let selected = select_tool_subset(
            vec![product_schema(
                "todo_write",
                tandem_types::ToolEffect::Write,
                ToolRiskTier::InternalWrite,
            )],
            ToolIntent::ProductAuthoring,
            &HashSet::new(),
            false,
        );
        assert!(selected.is_empty());
    }

    #[test]
    fn workspace_read_intent_uses_metadata_for_unknown_tool_names() {
        let selected = select_tool_subset(
            vec![
                ToolSchema::new("workspace_inspector", "", serde_json::json!({}))
                    .with_capabilities(
                        tandem_types::ToolCapabilities::new()
                            .effect(tandem_types::ToolEffect::Read)
                            .domain(tandem_types::ToolDomain::Workspace)
                            .reads_workspace(),
                    ),
            ],
            ToolIntent::WorkspaceRead,
            &HashSet::new(),
            false,
        );
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "workspace_inspector");
    }

    #[test]
    fn shell_intent_uses_metadata_for_unknown_tool_names() {
        let selected = select_tool_subset(
            vec![
                ToolSchema::new("run_local_checks", "", serde_json::json!({})).with_capabilities(
                    tandem_types::ToolCapabilities::new()
                        .effect(tandem_types::ToolEffect::Execute)
                        .domain(tandem_types::ToolDomain::Shell),
                ),
            ],
            ToolIntent::ShellExec,
            &HashSet::new(),
            false,
        );
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].name, "run_local_checks");
    }
}
