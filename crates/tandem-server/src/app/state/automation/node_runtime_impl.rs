use super::*;

fn is_agent_standup_automation(automation: &AutomationV2Spec) -> bool {
    automation
        .metadata
        .as_ref()
        .and_then(|value| value.get("feature"))
        .and_then(Value::as_str)
        .map(|value| value == "agent_standup")
        .unwrap_or(false)
}

fn resolve_standup_report_path_template(automation: &AutomationV2Spec) -> Option<String> {
    automation
        .metadata
        .as_ref()
        .and_then(|value| value.get("standup"))
        .and_then(|value| value.get("report_path_template"))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn resolve_standup_report_path_for_run(
    automation: &AutomationV2Spec,
    started_at_ms: u64,
) -> Option<String> {
    let template = resolve_standup_report_path_template(automation)?;
    if !template.contains("{{date}}") {
        return Some(template);
    }
    let date = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(started_at_ms as i64)
        .unwrap_or_else(chrono::Utc::now)
        .format("%Y-%m-%d")
        .to_string();
    Some(template.replace("{{date}}", &date))
}

pub(crate) fn automation_workspace_project_id(workspace_root: &str) -> String {
    tandem_core::workspace_project_id(workspace_root)
        .unwrap_or_else(|| "workspace-unknown".to_string())
}

pub(crate) fn merge_automation_agent_allowlist(
    agent: &AutomationAgentProfile,
    template: Option<&tandem_orchestrator::AgentTemplate>,
) -> Vec<String> {
    let mut allowlist = if agent.tool_policy.allowlist.is_empty() {
        template
            .map(|value| value.capabilities.tool_allowlist.clone())
            .unwrap_or_default()
    } else {
        agent.tool_policy.allowlist.clone()
    };
    allowlist.sort();
    allowlist.dedup();
    allowlist
}

pub(crate) fn automation_node_builder_priority(node: &AutomationFlowNode) -> i32 {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(|builder| builder.get("priority"))
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .unwrap_or(0)
}

pub(crate) fn automation_node_output_extension(node: &AutomationFlowNode) -> Option<String> {
    automation_node_required_output_path(node)
        .as_deref()
        .and_then(|value| std::path::Path::new(value).extension())
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
}

pub(crate) fn automation_node_task_kind(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "task_kind")
        .or_else(|| automation_node_builder_metadata(node, "task_class"))
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

pub(crate) fn automation_node_projects_backlog_tasks(node: &AutomationFlowNode) -> bool {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get("project_backlog_tasks"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub(crate) fn automation_node_task_id(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "task_id")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn automation_node_repo_root(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "repo_root")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn automation_node_write_scope(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "write_scope")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn automation_node_acceptance_criteria(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "acceptance_criteria")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn automation_node_task_dependencies(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "task_dependencies")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn automation_node_task_owner(node: &AutomationFlowNode) -> Option<String> {
    automation_node_builder_metadata(node, "task_owner")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn automation_node_is_code_workflow(node: &AutomationFlowNode) -> bool {
    if automation_node_task_kind(node)
        .as_deref()
        .is_some_and(|kind| matches!(kind, "code_change" | "repo_fix" | "implementation"))
    {
        return true;
    }
    let Some(extension) = automation_node_output_extension(node) else {
        return false;
    };
    let code_extensions = [
        "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt", "kts", "c", "cc", "cpp", "h",
        "hpp", "cs", "rb", "php", "swift", "scala", "sh", "bash", "zsh",
    ];
    code_extensions.contains(&extension.as_str())
}

pub(crate) fn automation_output_validator_kind(
    node: &AutomationFlowNode,
) -> crate::AutomationOutputValidatorKind {
    if let Some(validator) = node
        .output_contract
        .as_ref()
        .and_then(|contract| contract.validator)
    {
        return validator;
    }
    if automation_node_is_code_workflow(node) {
        return crate::AutomationOutputValidatorKind::CodePatch;
    }
    match node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("brief") => crate::AutomationOutputValidatorKind::ResearchBrief,
        Some("review") => crate::AutomationOutputValidatorKind::ReviewDecision,
        Some("structured_json") => crate::AutomationOutputValidatorKind::StructuredJson,
        _ => crate::AutomationOutputValidatorKind::GenericArtifact,
    }
}

fn automation_routine_for_node<'a>(
    plan: &'a compiler_api::PlanPackage,
    node_id: &str,
) -> Option<&'a compiler_api::RoutinePackage> {
    plan.routine_graph
        .iter()
        .find(|routine| routine.steps.iter().any(|step| step.step_id == node_id))
}

fn routine_is_complete(
    routine: &compiler_api::RoutinePackage,
    completed_nodes: &HashSet<String>,
) -> bool {
    routine
        .steps
        .iter()
        .all(|step| completed_nodes.contains(&step.step_id))
}

pub(crate) fn automation_node_routine_dependencies_blocked(
    automation: &AutomationV2Spec,
    run: &AutomationV2RunRecord,
    node: &AutomationFlowNode,
) -> bool {
    let Some(plan) = automation_plan_package(automation) else {
        return false;
    };
    let Some(routine) = automation_routine_for_node(&plan, &node.node_id) else {
        return false;
    };
    let completed_nodes = run
        .checkpoint
        .completed_nodes
        .iter()
        .cloned()
        .collect::<HashSet<_>>();
    routine.dependencies.iter().any(|dependency| {
        if !matches!(dependency.mode, compiler_api::DependencyMode::Hard) {
            return false;
        }
        plan.routine_graph
            .iter()
            .find(|candidate_routine| candidate_routine.routine_id == dependency.routine_id)
            .is_some_and(|candidate_routine| {
                !routine_is_complete(candidate_routine, &completed_nodes)
            })
    })
}

fn path_looks_like_source_file(path: &str) -> bool {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return false;
    }
    let normalized = trimmed.replace('\\', "/");
    let path = std::path::Path::new(&normalized);
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    if extension.as_deref().is_some_and(|extension| {
        [
            "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "kt", "kts", "c", "cc", "cpp", "h",
            "hpp", "cs", "rb", "php", "swift", "scala", "sh", "bash", "zsh", "toml", "yaml", "yml",
            "json",
        ]
        .contains(&extension)
    }) {
        return true;
    }
    path.file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .is_some_and(|name| {
            matches!(
                name.as_str(),
                "cargo.toml"
                    | "cargo.lock"
                    | "package.json"
                    | "package-lock.json"
                    | "pnpm-lock.yaml"
                    | "tsconfig.json"
                    | "deno.json"
                    | "deno.jsonc"
                    | "jest.config.js"
                    | "jest.config.ts"
                    | "vite.config.ts"
                    | "vite.config.js"
                    | "webpack.config.js"
                    | "webpack.config.ts"
                    | "next.config.js"
                    | "next.config.mjs"
                    | "pyproject.toml"
                    | "requirements.txt"
                    | "makefile"
                    | "dockerfile"
            )
        })
}

fn workspace_has_git_repo(workspace_root: &str) -> bool {
    std::process::Command::new("git")
        .current_dir(workspace_root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub(crate) fn automation_node_execution_mode(
    node: &AutomationFlowNode,
    workspace_root: &str,
) -> &'static str {
    if !automation_node_is_code_workflow(node) {
        return "artifact_write";
    }
    if workspace_has_git_repo(workspace_root) {
        "git_patch"
    } else {
        "filesystem_patch"
    }
}

pub(crate) fn normalize_automation_requested_tools(
    node: &AutomationFlowNode,
    workspace_root: &str,
    raw: Vec<String>,
) -> Vec<String> {
    let mut normalized = config::channels::normalize_allowed_tools(raw);
    let had_wildcard = normalized.iter().any(|tool| tool == "*");
    if had_wildcard {
        normalized.retain(|tool| tool != "*");
    }
    normalized.extend(automation_node_required_tools(node));
    match automation_node_execution_mode(node, workspace_root) {
        "git_patch" => {
            normalized.extend([
                "glob".to_string(),
                "read".to_string(),
                "edit".to_string(),
                "apply_patch".to_string(),
                "write".to_string(),
                "bash".to_string(),
            ]);
        }
        "filesystem_patch" => {
            normalized.extend([
                "glob".to_string(),
                "read".to_string(),
                "edit".to_string(),
                "write".to_string(),
                "bash".to_string(),
            ]);
        }
        _ => {
            if automation_node_required_output_path(node).is_some() {
                normalized.push("write".to_string());
            }
        }
    }
    if !node.input_refs.is_empty() {
        normalized.push("read".to_string());
    }
    let has_read = normalized.iter().any(|tool| tool == "read");
    let has_workspace_probe = normalized
        .iter()
        .any(|tool| matches!(tool.as_str(), "glob" | "ls" | "list"));
    if has_read && !has_workspace_probe {
        normalized.push("glob".to_string());
    }
    if automation_node_web_research_expected(node) {
        normalized.push("websearch".to_string());
    }
    normalized.sort();
    normalized.dedup();
    normalized
}

pub(crate) fn automation_node_delivery_method(node: &AutomationFlowNode) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/delivery/method")
                .or_else(|| value.pointer("/builder/delivery/method"))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
}

pub(crate) fn automation_node_delivery_target(node: &AutomationFlowNode) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/delivery/to")
                .or_else(|| value.pointer("/builder/delivery/to"))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| extract_email_address_from_text(&node.objective))
}

fn extract_email_address_from_text(text: &str) -> Option<String> {
    text.split_whitespace().find_map(|token| {
        let candidate = token
            .trim_matches(|ch: char| {
                ch.is_ascii_punctuation() && ch != '@' && ch != '.' && ch != '_' && ch != '-'
            })
            .trim();
        if candidate.is_empty()
            || !candidate.contains('@')
            || candidate.starts_with('@')
            || candidate.ends_with('@')
        {
            return None;
        }
        let mut parts = candidate.split('@');
        let local = parts.next()?.trim();
        let domain = parts.next()?.trim();
        if parts.next().is_some()
            || local.is_empty()
            || domain.is_empty()
            || !domain.contains('.')
            || domain.starts_with('.')
            || domain.ends_with('.')
        {
            return None;
        }
        Some(candidate.to_string())
    })
}

pub(crate) fn automation_node_email_content_type(node: &AutomationFlowNode) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/delivery/content_type")
                .or_else(|| value.pointer("/builder/delivery/content_type"))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(crate) fn automation_node_inline_body_only(node: &AutomationFlowNode) -> Option<bool> {
    node.metadata
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/delivery/inline_body_only")
                .or_else(|| value.pointer("/builder/delivery/inline_body_only"))
        })
        .and_then(Value::as_bool)
}

pub(crate) fn automation_node_allows_attachments(node: &AutomationFlowNode) -> Option<bool> {
    node.metadata
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/delivery/attachments")
                .or_else(|| value.pointer("/builder/delivery/attachments"))
        })
        .and_then(Value::as_bool)
}

pub(crate) fn automation_node_requires_email_delivery(node: &AutomationFlowNode) -> bool {
    if automation_node_delivery_method(node)
        .as_deref()
        .is_some_and(|method| method == "email")
    {
        return true;
    }
    if !automation_node_is_outbound_action(node) {
        return false;
    }
    if automation_node_delivery_target(node).is_some() {
        return true;
    }
    let objective = node.objective.to_ascii_lowercase();
    let contains_phrase = [
        "send email",
        "send the email",
        "send by email",
        "send the report by email",
        "email the ",
        "email report",
        "draft email",
        "draft the email",
        "gmail draft",
        "gmail_send",
        "notify by email",
        "notify the operator by email",
    ]
    .iter()
    .any(|needle| objective.contains(needle));
    if contains_phrase {
        return true;
    }
    false
}

pub(crate) fn automation_tool_name_is_email_delivery(tool_name: &str) -> bool {
    let tokens = automation_tool_name_tokens(tool_name);
    tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "email"
                | "mail"
                | "gmail"
                | "outlook"
                | "smtp"
                | "imap"
                | "inbox"
                | "mailbox"
                | "mailer"
                | "exchange"
                | "sendgrid"
                | "mailgun"
                | "postmark"
                | "resend"
                | "ses"
        )
    })
}

fn discover_automation_tools_for_capability(
    capability_id: &str,
    available_tool_names: &HashSet<String>,
) -> Vec<String> {
    if available_tool_names.is_empty() {
        return vec!["*".to_string()];
    }
    let mut matches = available_tool_names
        .iter()
        .filter(|tool_name| automation_capability_matches_tool(capability_id, tool_name))
        .cloned()
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();
    matches
}

pub(crate) fn filter_requested_tools_to_available(
    requested_tools: Vec<String>,
    available_tool_names: &HashSet<String>,
) -> Vec<String> {
    if requested_tools.iter().any(|tool| tool == "*") {
        return requested_tools;
    }
    requested_tools
        .into_iter()
        .filter(|tool| available_tool_names.contains(tool))
        .collect()
}

pub(crate) fn automation_requested_tools_for_node(
    node: &AutomationFlowNode,
    workspace_root: &str,
    raw: Vec<String>,
    available_tool_names: &HashSet<String>,
) -> Vec<String> {
    let execution_mode = automation_node_execution_mode(node, workspace_root);
    let mut requested_tools = filter_requested_tools_to_available(
        normalize_automation_requested_tools(node, workspace_root, raw),
        available_tool_names,
    );
    for capability_id in automation_tool_capability_ids(node, execution_mode) {
        requested_tools.extend(discover_automation_tools_for_capability(
            &capability_id,
            available_tool_names,
        ));
    }
    requested_tools.sort();
    requested_tools.dedup();
    requested_tools
}

pub(crate) fn automation_node_prewrite_requirements(
    node: &AutomationFlowNode,
    requested_tools: &[String],
) -> Option<PrewriteRequirements> {
    super::automation_node_prewrite_requirements_impl(node, requested_tools)
}

fn validation_requirement_is_warning(profile: &str, requirement: &str) -> bool {
    match profile {
        "external_research" => matches!(
            requirement,
            "files_reviewed_missing"
                | "files_reviewed_not_backed_by_read"
                | "relevant_files_not_reviewed_or_skipped"
                | "web_sources_reviewed_missing"
                | "files_reviewed_contains_nonconcrete_paths"
        ),
        "research_synthesis" => matches!(
            requirement,
            "files_reviewed_missing"
                | "files_reviewed_not_backed_by_read"
                | "relevant_files_not_reviewed_or_skipped"
                | "web_sources_reviewed_missing"
                | "files_reviewed_contains_nonconcrete_paths"
                | "workspace_inspection_required"
        ),
        "local_research" => matches!(
            requirement,
            "files_reviewed_missing" | "relevant_files_not_reviewed_or_skipped"
        ),
        "artifact_only" => matches!(
            requirement,
            "editorial_substance_missing" | "markdown_structure_missing"
        ),
        _ => false,
    }
}

fn semantic_block_reason_for_requirements(unmet_requirements: &[String]) -> Option<String> {
    let has_unmet = |needle: &str| unmet_requirements.iter().any(|value| value == needle);
    if has_unmet("structured_handoff_missing") {
        Some("structured handoff was not returned in the final response".to_string())
    } else if has_unmet("workspace_inspection_required") {
        Some("structured handoff completed without required workspace inspection".to_string())
    } else if has_unmet("no_concrete_reads") || has_unmet("concrete_read_required") {
        Some(
            "research completed without concrete file reads or required source coverage"
                .to_string(),
        )
    } else if has_unmet("missing_successful_web_research") {
        Some("research completed without required current web research".to_string())
    } else if has_unmet("relevant_files_not_reviewed_or_skipped") {
        Some(
            "research completed without covering or explicitly skipping relevant discovered files"
                .to_string(),
        )
    } else if has_unmet("citations_missing") {
        Some("research completed without citation-backed claims".to_string())
    } else if has_unmet("web_sources_reviewed_missing") {
        Some("research completed without a web sources reviewed section".to_string())
    } else if has_unmet("files_reviewed_contains_nonconcrete_paths") {
        Some(
            "research artifact contains non-concrete paths (wildcards or directory placeholders) in source audit"
                .to_string(),
        )
    } else if has_unmet("files_reviewed_missing") || has_unmet("files_reviewed_not_backed_by_read")
    {
        Some("research completed without a source-backed files reviewed section".to_string())
    } else if has_unmet("markdown_structure_missing") {
        Some("editorial artifact is missing expected markdown structure".to_string())
    } else if has_unmet("editorial_substance_missing") {
        Some("editorial artifact is too weak or placeholder-like".to_string())
    } else {
        None
    }
}

fn resolve_automation_agent_model(
    agent: &AutomationAgentProfile,
    template: Option<&tandem_orchestrator::AgentTemplate>,
) -> Option<ModelSpec> {
    if let Some(model) = agent
        .model_policy
        .as_ref()
        .and_then(|policy| policy.get("default_model"))
        .and_then(crate::app::routines::parse_model_spec)
    {
        return Some(model);
    }
    template
        .and_then(|value| value.default_model.as_ref())
        .and_then(crate::app::routines::parse_model_spec)
}

pub(crate) fn automation_node_inline_artifact_payload(node: &AutomationFlowNode) -> Option<Value> {
    if node.node_id == "collect_inputs" {
        return node
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("inputs"))
            .filter(|value| !value.is_null())
            .cloned();
    }
    None
}

pub(crate) fn write_automation_inline_artifact(
    workspace_root: &str,
    output_path: &str,
    payload: &Value,
) -> anyhow::Result<(String, String)> {
    let resolved = resolve_automation_output_path(workspace_root, output_path)?;
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            anyhow::anyhow!(
                "failed to create parent directory for required output `{}`: {}",
                output_path,
                error
            )
        })?;
    }
    let file_text = serde_json::to_string_pretty(payload)?;
    std::fs::write(&resolved, &file_text).map_err(|error| {
        anyhow::anyhow!(
            "failed to write deterministic workflow artifact `{}`: {}",
            output_path,
            error
        )
    })?;
    Ok((output_path.to_string(), file_text))
}

pub(crate) fn automation_node_declared_output_path(node: &AutomationFlowNode) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get("output_path"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| automation_node_default_output_path(node))
}

pub(crate) fn automation_node_required_output_path_for_run(
    node: &AutomationFlowNode,
    run_id: Option<&str>,
) -> Option<String> {
    let output_path = automation_node_declared_output_path(node)?;
    run_id
        .and_then(|run_id| super::automation_run_scoped_output_path(run_id, &output_path))
        .or(Some(output_path))
}

pub fn automation_node_required_output_path(node: &AutomationFlowNode) -> Option<String> {
    automation_node_required_output_path_for_run(node, None)
}

pub(crate) fn automation_node_default_output_path(node: &AutomationFlowNode) -> Option<String> {
    let extension = match node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.as_str())
        .unwrap_or("structured_json")
    {
        "report_markdown" => {
            let format = node
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("format"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            if format.eq_ignore_ascii_case("simple_html") {
                "html"
            } else {
                "md"
            }
        }
        "approval_gate" => return None,
        _ => "json",
    };
    let default_enabled = matches!(
        node.node_id.as_str(),
        "collect_inputs"
            | "research_sources"
            | "extract_pain_points"
            | "cluster_topics"
            | "analyze_findings"
            | "compare_results"
            | "compare_with_features"
            | "generate_report"
    );
    if !default_enabled {
        return None;
    }
    let slug = node
        .node_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if slug.is_empty() {
        return None;
    }
    Some(format!(".tandem/artifacts/{slug}.{extension}"))
}

pub(crate) fn automation_node_web_research_expected(node: &AutomationFlowNode) -> bool {
    enforcement_requires_external_sources(&automation_node_output_enforcement(node))
}

pub(crate) fn automation_node_required_tools(node: &AutomationFlowNode) -> Vec<String> {
    automation_node_output_enforcement(node).required_tools
}

pub(crate) fn automation_node_should_surface_mcp_discovery(
    node: &AutomationFlowNode,
    allowed_mcp_servers: &[String],
) -> bool {
    let connector_hint_text = [
        node.objective.as_str(),
        automation_node_builder_metadata(node, "prompt")
            .as_deref()
            .unwrap_or_default(),
    ]
    .join("\n");
    tandem_plan_compiler::api::workflow_plan_should_surface_mcp_discovery(
        &connector_hint_text,
        allowed_mcp_servers,
    )
}

pub(crate) fn automation_node_execution_policy(
    node: &AutomationFlowNode,
    workspace_root: &str,
) -> Value {
    let output_path = automation_node_required_output_path(node);
    let code_workflow = automation_node_is_code_workflow(node);
    let git_backed = workspace_has_git_repo(workspace_root);
    let mode = automation_node_execution_mode(node, workspace_root);
    let workflow_class = automation_node_workflow_class(node);
    json!({
        "mode": mode,
        "workflow_class": workflow_class,
        "code_workflow": code_workflow,
        "git_backed": git_backed,
        "declared_output_path": output_path,
        "project_backlog_tasks": automation_node_projects_backlog_tasks(node),
        "task_id": automation_node_task_id(node),
        "task_kind": automation_node_task_kind(node),
        "repo_root": automation_node_repo_root(node),
        "write_scope": automation_node_write_scope(node),
        "acceptance_criteria": automation_node_acceptance_criteria(node),
        "task_dependencies": automation_node_task_dependencies(node),
        "verification_state": automation_node_verification_state(node),
        "task_owner": automation_node_task_owner(node),
        "verification_command": automation_node_verification_command(node),
    })
}

fn resolve_automation_output_path(
    workspace_root: &str,
    output_path: &str,
) -> anyhow::Result<PathBuf> {
    let trimmed = output_path.trim();
    if trimmed.is_empty() {
        anyhow::bail!("required output path is empty");
    }
    let workspace = PathBuf::from(workspace_root);
    let candidate = PathBuf::from(trimmed);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        workspace.join(candidate)
    };
    if !resolved.starts_with(&workspace) {
        anyhow::bail!(
            "required output path `{}` must stay inside workspace `{}`",
            trimmed,
            workspace_root
        );
    }
    Ok(resolved)
}

fn is_suspicious_automation_marker_file(path: &std::path::Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let lowered = name.to_ascii_lowercase();
    lowered.starts_with(".tandem")
        || lowered == "_automation_touch.txt"
        || lowered.contains("stage-touch")
        || lowered.ends_with("-status.txt")
        || lowered.contains("touch.txt")
}

fn list_suspicious_automation_marker_files(workspace_root: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(workspace_root) else {
        return Vec::new();
    };
    let mut paths = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_suspicious_automation_marker_file(path))
        .filter_map(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
}

fn remove_suspicious_automation_marker_files(workspace_root: &str) {
    let Ok(entries) = std::fs::read_dir(workspace_root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || !is_suspicious_automation_marker_file(&path) {
            continue;
        }
        let _ = std::fs::remove_file(path);
    }
}
