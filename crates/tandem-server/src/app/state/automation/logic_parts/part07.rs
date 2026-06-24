pub(crate) fn automation_workspace_project_id(workspace_root: &str) -> String {
    node_runtime_impl::automation_workspace_project_id(workspace_root)
}

pub(crate) fn merge_automation_agent_allowlist(
    agent: &AutomationAgentProfile,
    template: Option<&tandem_orchestrator::AgentTemplate>,
) -> Vec<String> {
    node_runtime_impl::merge_automation_agent_allowlist(agent, template)
}

pub(crate) fn automation_node_output_contract_kind(node: &AutomationFlowNode) -> Option<String> {
    node.output_contract
        .as_ref()
        .map(|contract| contract.kind.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
}

pub(crate) fn automation_node_task_kind(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_task_kind(node)
}

pub(crate) fn automation_node_knowledge_task_family(node: &AutomationFlowNode) -> String {
    let explicit_family = automation_node_builder_metadata(node, "task_family")
        .or_else(|| automation_node_builder_metadata(node, "knowledge_task_family"))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(family) = explicit_family {
        let normalized = tandem_orchestrator::normalize_knowledge_segment(&family);
        if !normalized.is_empty() {
            return normalized;
        }
    }

    if let Some(task_kind) = automation_node_task_kind(node) {
        let mapped = match task_kind.as_str() {
            "code_change" | "repo_fix" | "implementation" | "debugging" | "bug_fix" => Some("code"),
            "research" | "analysis" | "synthesis" | "research_brief" => Some("research"),
            "support" | "ops" | "runbook" | "incident" | "triage" => Some("ops"),
            "plan" | "planning" | "roadmap" => Some("planning"),
            "verification" | "test" | "qa" => Some("verification"),
            _ => None,
        };
        if let Some(mapped) = mapped {
            return mapped.to_string();
        }
        let normalized = tandem_orchestrator::normalize_knowledge_segment(&task_kind);
        if !normalized.is_empty() {
            return normalized;
        }
    }

    let workflow_class = automation_node_workflow_class(node);
    if workflow_class != "artifact" {
        return workflow_class;
    }

    if let Some(contract_kind) = automation_node_output_contract_kind(node) {
        let normalized = tandem_orchestrator::normalize_knowledge_segment(&contract_kind);
        if !normalized.is_empty() {
            return normalized;
        }
    }

    let fallback = tandem_orchestrator::normalize_knowledge_segment(&node.node_id);
    if fallback.is_empty() {
        workflow_class
    } else {
        fallback
    }
}

pub(crate) fn automation_node_projects_backlog_tasks(node: &AutomationFlowNode) -> bool {
    node_runtime_impl::automation_node_projects_backlog_tasks(node)
}

pub(crate) fn automation_node_task_id(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_task_id(node)
}

pub(crate) fn automation_node_repo_root(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_repo_root(node)
}

pub(crate) fn automation_node_write_scope(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_write_scope(node)
}

pub(crate) fn automation_node_acceptance_criteria(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_acceptance_criteria(node)
}

pub(crate) fn automation_node_task_dependencies(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_task_dependencies(node)
}

pub(crate) fn automation_node_task_owner(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_task_owner(node)
}

pub(crate) fn automation_node_is_code_workflow(node: &AutomationFlowNode) -> bool {
    node_runtime_impl::automation_node_is_code_workflow(node)
}

pub(crate) fn automation_output_validator_kind(
    node: &AutomationFlowNode,
) -> crate::AutomationOutputValidatorKind {
    node_runtime_impl::automation_output_validator_kind(node)
}

pub(crate) fn path_looks_like_source_file(path: &str) -> bool {
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

pub(crate) fn workspace_has_git_repo(workspace_root: &str) -> bool {
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
    node_runtime_impl::automation_node_execution_mode(node, workspace_root)
}

pub(crate) fn normalize_automation_requested_tools(
    node: &AutomationFlowNode,
    workspace_root: &str,
    raw: Vec<String>,
) -> Vec<String> {
    let review_decision_node = automation_output_validator_kind(node)
        == crate::AutomationOutputValidatorKind::ReviewDecision;
    let handoff_only_structured_json =
        node_runtime_impl::automation_node_is_handoff_only_structured_json(node);
    let node_tool_allowlist = node_runtime_impl::automation_node_metadata_tool_allowlist(node);
    let connector_hint_mentions =
        tandem_plan_compiler::api::workflow_plan_mentions_connector_backed_sources(
            &automation_connector_hint_text(node),
        );
    let explicit_connector_tool_allowlist = !automation_node_is_code_workflow(node)
        && node_runtime_impl::automation_node_has_explicit_tool_policy(node)
        && (connector_hint_mentions
            || node_tool_allowlist
                .iter()
                .any(|tool| tool.starts_with("mcp.")));
    let upstream_synthesis_node =
        enforcement::automation_node_consumes_upstream_artifacts_for_delivery(node);
    let mut normalized = if explicit_connector_tool_allowlist {
        node_tool_allowlist
    } else {
        config::channels::normalize_allowed_tools(raw)
    };
    if explicit_connector_tool_allowlist
        && normalized.iter().any(|tool| tool.starts_with("mcp."))
        && automation_mcp_list_needed_for_tools(&normalized)
    {
        normalized.push("mcp_list".to_string());
    }
    let had_wildcard = normalized.iter().any(|tool| tool == "*");
    if had_wildcard {
        normalized.retain(|tool| tool != "*");
    }
    normalized.extend(automation_node_required_tools(node));
    if !automation_mcp_list_needed_for_tools(&normalized) {
        normalized.retain(|tool| tool != "mcp_list");
    }
    if explicit_connector_tool_allowlist {
        if node_runtime_impl::automation_node_requires_artifact_write_tool(node) {
            normalized.push("write".to_string());
        }
    } else {
        match automation_node_execution_mode(node, workspace_root) {
            "git_patch" => {
                normalized.extend([
                    "repo.context_bundle".to_string(),
                    "repo.search".to_string(),
                    "repo.symbol".to_string(),
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
                    "repo.context_bundle".to_string(),
                    "repo.search".to_string(),
                    "glob".to_string(),
                    "read".to_string(),
                    "edit".to_string(),
                    "apply_patch".to_string(),
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
    }
    let connector_source_node = !automation_node_is_code_workflow(node)
        && !upstream_synthesis_node
        && !enforcement::automation_node_allows_optional_connector_references(node)
        && (connector_hint_mentions || normalized.iter().any(|tool| tool.starts_with("mcp.")));
    if explicit_connector_tool_allowlist || connector_source_node {
        normalized.retain(|tool| {
            !matches!(
                tool.as_str(),
                "codesearch" | "read" | "edit" | "apply_patch" | "glob" | "grep" | "bash"
            )
        });
    }
    if !node.input_refs.is_empty()
        && !(explicit_connector_tool_allowlist && upstream_synthesis_node)
    {
        normalized.push("read".to_string());
    }
    let has_read = normalized.iter().any(|tool| tool == "read");
    let has_workspace_probe = normalized.iter().any(|tool| {
        matches!(
            tool.as_str(),
            "glob" | "ls" | "list" | "repo.context_bundle" | "repo.search" | "repo.symbol"
        )
    });
    if has_read
        && !has_workspace_probe
        && !explicit_connector_tool_allowlist
        && !connector_source_node
        && !handoff_only_structured_json
    {
        normalized.push("glob".to_string());
    }
    if automation_node_web_research_expected(node)
        || enforcement::automation_node_allows_optional_web_research(node)
    {
        normalized.push("websearch".to_string());
        if enforcement::automation_node_allows_optional_web_research(node) {
            normalized.push("webfetch".to_string());
        }
    }
    if handoff_only_structured_json {
        normalized.retain(|tool| !matches!(tool.as_str(), "write" | "edit" | "apply_patch"));
    }
    if review_decision_node {
        normalized.retain(|tool| matches!(tool.as_str(), "read" | "glob" | "grep"));
        if !normalized.iter().any(|tool| tool == "read") {
            normalized.push("read".to_string());
        }
        if !normalized.iter().any(|tool| tool == "glob") {
            normalized.push("glob".to_string());
        }
    }
    normalized.sort();
    normalized.dedup();
    normalized
}

pub(crate) fn automation_tool_name_is_email_delivery(tool_name: &str) -> bool {
    node_runtime_impl::automation_tool_name_is_email_delivery(tool_name)
}

pub(crate) fn discover_automation_tools_for_capability(
    capability_id: &str,
    available_tool_names: &HashSet<String>,
) -> Vec<String> {
    node_runtime_impl::discover_automation_tools_for_capability(capability_id, available_tool_names)
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
    node_runtime_impl::resolve_automation_node_tool_envelope(
        node,
        workspace_root,
        raw,
        available_tool_names,
    )
    .tools
}

pub(crate) fn automation_node_prewrite_requirements(
    node: &AutomationFlowNode,
    requested_tools: &[String],
) -> Option<PrewriteRequirements> {
    automation_node_prewrite_requirements_impl(node, requested_tools)
}

pub(crate) fn automation_node_prewrite_requirements_impl(
    node: &AutomationFlowNode,
    requested_tools: &[String],
) -> Option<PrewriteRequirements> {
    let write_required = automation_node_required_output_path(node).is_some();
    if !write_required {
        return None;
    }
    let enforcement = automation_node_output_enforcement(node);
    let required_tools = enforcement.required_tools.clone();
    let web_research_expected = enforcement_requires_external_sources(&enforcement);
    let validation_profile = enforcement
        .validation_profile
        .as_deref()
        .unwrap_or("artifact_only");
    let connector_source_node = !automation_node_is_code_workflow(node)
        && !enforcement::automation_node_allows_optional_connector_references(node)
        && !super::prompting_impl::automation_node_concrete_mcp_tool_allowlist(node).is_empty();
    let workspace_inspection_required = requested_tools.iter().any(|tool| {
        matches!(
            tool.as_str(),
            "glob"
                | "ls"
                | "list"
                | "read"
                | "repo.context_bundle"
                | "repo.search"
                | "repo.symbol"
                | "repo.neighbors"
                | "repo.impact"
                | "repo.test_targets"
        )
    });
    let legacy_web_research_expected = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get("web_research_expected"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let web_research_required =
        web_research_expected && requested_tools.iter().any(|tool| tool == "websearch");
    let brief_research_node = validation_profile == "local_research";
    let external_research_node = validation_profile == "external_research";
    let research_finalize = validation_profile == "research_synthesis";
    let optional_workspace_reads =
        enforcement::automation_node_allows_optional_workspace_reads(node);
    let explicit_input_files = automation_node_explicit_input_files(node);
    let has_required_read = required_tools.iter().any(|tool| tool == "read");
    let has_required_websearch = required_tools.iter().any(|tool| tool == "websearch");
    let has_any_required_tools = !required_tools.is_empty();
    let concrete_read_required = if !explicit_input_files.is_empty() {
        !research_finalize && requested_tools.iter().any(|tool| tool == "read")
    } else {
        !research_finalize
            && !optional_workspace_reads
            && ((brief_research_node || validation_profile == "local_research")
                || has_required_read
                || enforcement
                    .prewrite_gates
                    .iter()
                    .any(|gate| gate == "concrete_reads"))
            && requested_tools.iter().any(|tool| tool == "read")
    };
    let successful_web_research_required = !research_finalize
        && ((validation_profile == "external_research")
            || has_required_websearch
            || enforcement
                .prewrite_gates
                .iter()
                .any(|gate| gate == "successful_web_research"))
        && web_research_expected
        && requested_tools.iter().any(|tool| tool == "websearch");
    Some(PrewriteRequirements {
        workspace_inspection_required: workspace_inspection_required
            && (!external_research_node || legacy_web_research_expected)
            && !connector_source_node
            && !research_finalize
            && explicit_input_files.is_empty(),
        web_research_required: web_research_required && !research_finalize,
        concrete_read_required,
        successful_web_research_required,
        repair_on_unmet_requirements: brief_research_node
            || has_any_required_tools
            || !enforcement.retry_on_missing.is_empty(),
        repair_budget: enforcement.repair_budget,
        repair_exhaustion_behavior: Some(
            if matches!(
                enforcement::automation_node_quality_mode_resolution(node).requested,
                Some(enforcement::AutomationQualityMode::Legacy)
            ) {
                tandem_types::PrewriteRepairExhaustionBehavior::WaiveAndWrite
            } else if enforcement::automation_node_is_strict_quality(node) {
                tandem_types::PrewriteRepairExhaustionBehavior::FailClosed
            } else {
                tandem_types::PrewriteRepairExhaustionBehavior::WaiveAndWrite
            },
        ),
        coverage_mode: if brief_research_node {
            PrewriteCoverageMode::ResearchCorpus
        } else {
            PrewriteCoverageMode::None
        },
    })
}

pub(crate) fn validation_requirement_is_warning(profile: &str, requirement: &str) -> bool {
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
        "local_research" => matches!(requirement, "files_reviewed_missing"),
        "artifact_only" => matches!(
            requirement,
            "editorial_substance_missing" | "markdown_structure_missing"
        ),
        _ => false,
    }
}

pub(crate) fn semantic_block_reason_for_requirements(
    unmet_requirements: &[String],
) -> Option<String> {
    let has_unmet = |needle: &str| unmet_requirements.iter().any(|value| value == needle);
    if has_unmet("read_only_source_mutations") {
        Some("read-only source-of-truth mutation detected".to_string())
    } else if has_unmet("artifact_status_not_terminal") {
        Some("artifact reported a non-terminal status".to_string())
    } else if has_unmet("output_schema_invalid") {
        Some("artifact does not match the declared output contract schema".to_string())
    } else if has_unmet("provider_required_tool_mode_unsatisfied") {
        Some("artifact contains a provider required-tool/write-required failure marker".to_string())
    } else if has_unmet("placeholder_artifact") {
        Some("artifact is placeholder-like or incomplete".to_string())
    } else if has_unmet("mcp_required_tool_missing") {
        Some("required MCP tool calls were not completed".to_string())
    } else if has_unmet("external_mutation_failed") {
        Some(
            "external delivery mutation failed and no later successful mutation was recorded"
                .to_string(),
        )
    } else if has_unmet("mcp_required_tool_failed") {
        Some("required MCP tool call failed and needs repair".to_string())
    } else if has_unmet("mcp_connector_source_artifact_missing") {
        Some(
            "connector-backed source artifact contains connector inventory only, not source evidence"
                .to_string(),
        )
    } else if has_unmet("mcp_connector_source_missing") {
        Some(
            "connector-backed source research completed without using a concrete connector tool"
                .to_string(),
        )
    } else if has_unmet("required_source_paths_not_read") {
        Some("research completed without reading the exact required source files".to_string())
    } else if has_unmet("current_attempt_output_missing") {
        Some("required output was not created in the current attempt".to_string())
    } else if has_unmet("structured_handoff_missing") {
        Some("structured handoff was not returned in the final response".to_string())
    } else if has_unmet("workspace_inspection_required") {
        Some("structured handoff completed without required workspace inspection".to_string())
    } else if has_unmet("mcp_discovery_missing") {
        Some("connector-backed work completed without discovering available MCP tools".to_string())
    } else if has_unmet("missing_successful_web_research") {
        Some("research completed without required current web research".to_string())
    } else if has_unmet("web_research_artifact_contradicts_tool_receipts") {
        Some(
            "artifact claims web research was unavailable even though web research succeeded in this run"
                .to_string(),
        )
    } else if has_unmet("upstream_notion_identity_overstated") {
        Some(
            "synthesis overstated an upstream Notion inspection that was explicitly unconfirmed"
                .to_string(),
        )
    } else if has_unmet("uncited_market_claims_from_limited_web_artifact") {
        Some(
            "synthesis made market/web-backed claims even though upstream external citations were missing"
                .to_string(),
        )
    } else if has_unmet("no_concrete_reads") || has_unmet("concrete_read_required") {
        Some(
            "research completed without concrete file reads or required source coverage"
                .to_string(),
        )
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
    } else if has_unmet("bare_relative_artifact_href") {
        Some(
            "final artifact contains a bare relative artifact href; use a canonical run-scoped link or plain text instead"
                .to_string(),
        )
    } else if has_unmet("required_workspace_files_missing") {
        Some("required workspace files were not written for this run".to_string())
    } else if has_unmet("upstream_evidence_not_synthesized") {
        Some(
            "final artifact does not adequately synthesize the available upstream evidence"
                .to_string(),
        )
    } else if has_unmet("markdown_structure_missing") {
        Some("editorial artifact is missing expected markdown structure".to_string())
    } else if has_unmet("editorial_substance_missing") {
        Some("editorial artifact is too weak or placeholder-like".to_string())
    } else {
        None
    }
}

pub(crate) async fn resolve_automation_agent_model(
    state: &AppState,
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
    if let Some(model) = template
        .and_then(|value| value.default_model.as_ref())
        .and_then(crate::app::routines::parse_model_spec)
    {
        return Some(model);
    }

    let providers = state.providers.list().await;
    let effective_config = state.config.get_effective_value().await;
    if let Some(config_default) =
        crate::app::state::default_model_spec_from_effective_config(&effective_config)
    {
        return Some(config_default);
    }

    providers.into_iter().find_map(|provider| {
        let model = provider.models.first()?;
        Some(ModelSpec {
            provider_id: provider.id,
            model_id: model.id.clone(),
        })
    })
}

pub(crate) fn automation_node_inline_artifact_payload(node: &AutomationFlowNode) -> Option<Value> {
    node_runtime_impl::automation_node_inline_artifact_payload(node)
}

pub(crate) fn write_automation_inline_artifact(
    workspace_root: &str,
    run_id: &str,
    output_path: &str,
    payload: &Value,
) -> anyhow::Result<(String, String)> {
    let resolved = resolve_automation_output_path_for_run(workspace_root, run_id, output_path)?;
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
    let display_path = resolved
        .strip_prefix(PathBuf::from(workspace_root))
        .ok()
        .and_then(|value| value.to_str().map(str::to_string))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| output_path.to_string());
    Ok((display_path, file_text))
}

pub(crate) fn automation_node_required_output_path_for_run(
    node: &AutomationFlowNode,
    run_id: Option<&str>,
) -> Option<String> {
    node_runtime_impl::automation_node_required_output_path_for_run(node, run_id)
}

pub fn automation_node_required_output_path(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_required_output_path(node)
}

pub(crate) fn automation_node_is_handoff_only_structured_json(node: &AutomationFlowNode) -> bool {
    node_runtime_impl::automation_node_is_handoff_only_structured_json(node)
}

pub(crate) fn automation_node_allows_preexisting_output_reuse(node: &AutomationFlowNode) -> bool {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get("allow_preexisting_output_reuse"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn automation_input_file_value_looks_like_tool_identifier(value: &str) -> bool {
    let lowered = value.trim().trim_matches('`').to_ascii_lowercase();
    lowered == "mcp_list"
        || lowered == "mcp_list_catalog"
        || lowered == "mcp_request_capability"
        || lowered.starts_with("mcp.")
}

pub(crate) fn automation_node_explicit_input_files(node: &AutomationFlowNode) -> Vec<String> {
    let mut files = automation_node_builder_string_array(node, "input_files");
    files.retain(|path| !automation_input_file_value_looks_like_tool_identifier(path));
    files.sort();
    files.dedup();
    files
}

pub(crate) fn automation_node_explicit_output_files(node: &AutomationFlowNode) -> Vec<String> {
    let mut files = automation_node_builder_string_array(node, "output_files");
    files.sort();
    files.dedup();
    files
}

pub(crate) fn automation_declared_output_target_aliases(
    automation: &AutomationV2Spec,
    runtime_values: Option<&AutomationPromptRuntimeValues>,
) -> HashSet<String> {
    let mut aliases = HashSet::new();
    for target in &automation.output_targets {
        let replaced = automation_runtime_placeholder_replace(target, runtime_values);
        for candidate in [target.as_str(), replaced.as_str()] {
            let trimmed = candidate.trim().trim_matches('`');
            if trimmed.is_empty() {
                continue;
            }
            let normalized = trimmed
                .strip_prefix("file://")
                .unwrap_or(trimmed)
                .trim()
                .replace('\\', "/");
            if normalized.is_empty() {
                continue;
            }
            aliases.insert(normalized.to_ascii_lowercase());
            if let Some(root) = automation.workspace_root.as_deref() {
                if let Some(relative) = normalize_workspace_display_path(root, &normalized) {
                    aliases.insert(relative.replace('\\', "/").to_ascii_lowercase());
                }
            }
        }
    }
    aliases
}

pub(crate) fn automation_path_matches_declared_output_target(
    automation: &AutomationV2Spec,
    blocked_targets: &HashSet<String>,
    path: &str,
) -> bool {
    let trimmed = path.trim().trim_matches('`');
    if trimmed.is_empty() {
        return false;
    }
    let normalized = trimmed
        .strip_prefix("file://")
        .unwrap_or(trimmed)
        .trim()
        .replace('\\', "/");
    let lowered = normalized.to_ascii_lowercase();
    if blocked_targets.contains(&lowered) {
        return true;
    }
    automation
        .workspace_root
        .as_deref()
        .and_then(|root| normalize_workspace_display_path(root, &normalized))
        .map(|relative| blocked_targets.contains(&relative.replace('\\', "/").to_ascii_lowercase()))
        .unwrap_or(false)
}

pub(crate) fn automation_node_is_terminal_for_automation(
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
) -> bool {
    !automation.flow.nodes.iter().any(|candidate| {
        candidate.node_id != node.node_id
            && (candidate.depends_on.iter().any(|dep| dep == &node.node_id)
                || candidate
                    .input_refs
                    .iter()
                    .any(|input| input.from_step_id == node.node_id))
    })
}

pub(crate) fn automation_node_can_access_declared_output_targets(
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
) -> bool {
    if automation_node_publish_spec(node).is_some() {
        return true;
    }
    automation_node_is_terminal_for_automation(automation, node)
        && automation
            .output_targets
            .iter()
            .any(|target| automation_output_target_matches_node_objective(target, &node.objective))
}

pub(crate) fn automation_node_effective_input_files_for_automation(
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    runtime_values: Option<&AutomationPromptRuntimeValues>,
) -> Vec<String> {
    let mut files = automation_node_explicit_input_files(node);
    if automation_node_can_access_declared_output_targets(automation, node) {
        files.sort();
        files.dedup();
        return files;
    }
    let blocked_targets = automation_declared_output_target_aliases(automation, runtime_values);
    files.retain(|path| {
        !automation_path_matches_declared_output_target(automation, &blocked_targets, path)
    });
    files.sort();
    files.dedup();
    files
}

pub(crate) fn automation_node_effective_output_files_for_automation(
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    runtime_values: Option<&AutomationPromptRuntimeValues>,
) -> Vec<String> {
    let mut files = automation_node_explicit_output_files(node);
    if automation_node_can_access_declared_output_targets(automation, node) {
        files.sort();
        files.dedup();
        return files;
    }
    let blocked_targets = automation_declared_output_target_aliases(automation, runtime_values);
    files.retain(|path| {
        !automation_path_matches_declared_output_target(automation, &blocked_targets, path)
    });
    files.sort();
    files.dedup();
    files
}

pub(crate) fn automation_node_must_write_files(node: &AutomationFlowNode) -> Vec<String> {
    let explicit_output_files = automation_node_explicit_output_files(node);
    let read_only_files = enforcement::automation_node_read_only_source_of_truth_files(node)
        .into_iter()
        .map(|path| path.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    if !explicit_output_files.is_empty() {
        let mut files = explicit_output_files
            .into_iter()
            .filter(|path| !read_only_files.contains(&path.to_ascii_lowercase()))
            .collect::<Vec<_>>();
        files.sort();
        files.dedup();
        return files;
    }
    let builder = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object);
    let explicit_must_write_files =
        builder.is_some_and(|builder| builder.contains_key("must_write_files"));
    let mut files = builder
        .and_then(|builder| builder.get("must_write_files"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if !explicit_must_write_files {
        let inferred = automation_node_bootstrap_missing_files(node);
        if !inferred.is_empty() {
            tracing::warn!(
                node_id = %node.node_id,
                inferred_files = ?inferred,
                "automation bootstrap file inference is deprecated; set builder.must_write_files explicitly"
            );
            files.extend(inferred);
        }
    }
    files.retain(|path| !read_only_files.contains(&path.to_ascii_lowercase()));
    files.sort();
    files.dedup();
    files
}

pub(crate) fn automation_runtime_placeholder_replace(
    text: &str,
    runtime_values: Option<&AutomationPromptRuntimeValues>,
) -> String {
    let Some(runtime_values) = runtime_values else {
        return text.to_string();
    };
    let hm_dashed = if runtime_values.current_time.len() == 4 {
        format!(
            "{}-{}",
            &runtime_values.current_time[..2],
            &runtime_values.current_time[2..]
        )
    } else {
        runtime_values.current_time.clone()
    };
    let hm_colon = hm_dashed.replace('-', ":");
    let hms_dashed = if runtime_values.current_time_hms.len() == 6 {
        format!(
            "{}-{}-{}",
            &runtime_values.current_time_hms[..2],
            &runtime_values.current_time_hms[2..4],
            &runtime_values.current_time_hms[4..]
        )
    } else {
        runtime_values.current_time_hms.clone()
    };
    let hms_colon = hms_dashed.replace('-', ":");
    let timestamp_compact = format!(
        "{}_{}",
        runtime_values.current_date, runtime_values.current_time
    );
    let timestamp_hyphen_compact = format!(
        "{}-{}",
        runtime_values.current_date, runtime_values.current_time
    );
    let timestamp_compact_hms = format!(
        "{}_{}",
        runtime_values.current_date, runtime_values.current_time_hms
    );
    let timestamp_hyphen_compact_hms = format!(
        "{}-{}",
        runtime_values.current_date, runtime_values.current_time_hms
    );
    let compact_timestamp = format!(
        "{}_{}",
        runtime_values.current_date_compact, runtime_values.current_time
    );
    let compact_timestamp_hms = format!(
        "{}_{}",
        runtime_values.current_date_compact, runtime_values.current_time_hms
    );
    let timestamp_filename_hyphen = runtime_values.current_timestamp_filename.replace('_', "-");
    let date_hm_dashed = format!("{}_{}", runtime_values.current_date, hm_dashed);
    let date_hm_hyphen = format!("{}-{}", runtime_values.current_date, hm_dashed);

    let replacements = [
        (
            "{{current_timestamp_filename}}",
            runtime_values.current_timestamp_filename.as_str(),
        ),
        (
            "{current_timestamp_filename}",
            runtime_values.current_timestamp_filename.as_str(),
        ),
        ("{{current_date}}", runtime_values.current_date.as_str()),
        ("{{current_time}}", runtime_values.current_time.as_str()),
        (
            "{{current_timestamp}}",
            runtime_values.current_timestamp.as_str(),
        ),
        ("{current_date}", runtime_values.current_date.as_str()),
        ("{current_time}", runtime_values.current_time.as_str()),
        (
            "{current_timestamp}",
            runtime_values.current_timestamp.as_str(),
        ),
        ("{{date}}", runtime_values.current_date.as_str()),
        ("{date}", runtime_values.current_date.as_str()),
        (
            "YYYY-MM-DD_HH-MM-SS",
            runtime_values.current_timestamp_filename.as_str(),
        ),
        ("YYYY-MM-DD-HH-MM-SS", timestamp_filename_hyphen.as_str()),
        ("YYYY-MM-DD_HHMMSS", timestamp_compact_hms.as_str()),
        ("YYYY-MM-DD-HHMMSS", timestamp_hyphen_compact_hms.as_str()),
        ("YYYY-MM-DD_HH-MM", date_hm_dashed.as_str()),
        ("YYYY-MM-DD-HH-MM", date_hm_hyphen.as_str()),
        ("YYYY-MM-DD_HHMM", timestamp_compact.as_str()),
        ("YYYY-MM-DD-HHMM", timestamp_hyphen_compact.as_str()),
        ("YYYYMMDD_HHMMSS", compact_timestamp_hms.as_str()),
        ("YYYYMMDD_HHMM", compact_timestamp.as_str()),
        ("YYYYMMDD", runtime_values.current_date_compact.as_str()),
        ("YYYY-MM-DD", runtime_values.current_date.as_str()),
        ("HH-MM-SS", hms_dashed.as_str()),
        ("HH:MM:SS", hms_colon.as_str()),
        ("HHMMSS", runtime_values.current_time_hms.as_str()),
        ("HH-MM", hm_dashed.as_str()),
        ("HH:MM", hm_colon.as_str()),
        ("HHMM", runtime_values.current_time.as_str()),
    ];

    let mut replaced = text.to_string();
    for (needle, value) in replacements {
        replaced = replaced.replace(needle, value);
    }
    replaced
}

pub(crate) fn automation_node_required_output_path_with_runtime_for_run(
    node: &AutomationFlowNode,
    run_id: Option<&str>,
    runtime_values: Option<&AutomationPromptRuntimeValues>,
) -> Option<String> {
    automation_node_required_output_path_for_run(node, run_id)
        .map(|path| automation_runtime_placeholder_replace(&path, runtime_values))
}

pub(crate) fn resolve_automation_output_path_with_runtime_for_run(
    workspace_root: &str,
    run_id: &str,
    output_path: &str,
    runtime_values: Option<&AutomationPromptRuntimeValues>,
) -> anyhow::Result<PathBuf> {
    let resolved_output_path = automation_runtime_placeholder_replace(output_path, runtime_values);
    resolve_automation_output_path_for_run(workspace_root, run_id, &resolved_output_path)
}

pub(crate) fn automation_keyword_variants(token: &str) -> Vec<String> {
    let lowered = token.trim().to_ascii_lowercase();
    if lowered.len() < 3
        || lowered.chars().all(|ch| ch.is_ascii_digit())
        || matches!(
            lowered.as_str(),
            "md" | "json"
                | "jsonl"
                | "yaml"
                | "yml"
                | "txt"
                | "csv"
                | "toml"
                | "current"
                | "date"
                | "time"
                | "timestamp"
        )
    {
        return Vec::new();
    }
    let mut variants = vec![lowered.clone()];
    if let Some(stripped) = lowered.strip_suffix("ies") {
        if stripped.len() >= 2 {
            variants.push(format!("{stripped}y"));
        }
    } else if let Some(stripped) = lowered.strip_suffix('s') {
        if stripped.len() >= 3 {
            variants.push(stripped.to_string());
        }
    }
    variants.sort();
    variants.dedup();
    variants
}

pub(crate) fn automation_keyword_set(text: &str) -> HashSet<String> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric())
        .flat_map(automation_keyword_variants)
        .collect()
}

pub(crate) fn automation_output_target_matches_node_objective(
    output_target: &str,
    objective_text: &str,
) -> bool {
    let objective_lower = objective_text.to_ascii_lowercase();
    let output_lower = output_target.to_ascii_lowercase();
    if objective_lower.contains(&output_lower) {
        return true;
    }
    let basename = std::path::Path::new(output_target)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(output_target)
        .to_ascii_lowercase();
    if !basename.is_empty() && objective_lower.contains(&basename) {
        return true;
    }
    let objective_keywords = automation_keyword_set(objective_text);
    let target_keywords = automation_keyword_set(output_target);
    let overlap = target_keywords
        .intersection(&objective_keywords)
        .cloned()
        .collect::<HashSet<_>>();
    if overlap.len() >= 2 {
        return true;
    }
    overlap.iter().any(|keyword| {
        matches!(
            keyword.as_str(),
            "pipeline"
                | "shortlist"
                | "recap"
                | "ledger"
                | "finding"
                | "findings"
                | "overview"
                | "positioning"
                | "resume"
                | "target"
                | "state"
        )
    })
}

pub(crate) fn automation_node_must_write_files_for_automation(
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    runtime_values: Option<&AutomationPromptRuntimeValues>,
) -> Vec<String> {
    let read_only_names =
        enforcement::automation_read_only_source_of_truth_name_variants_for_automation(automation);
    let mut declared_files = automation_node_must_write_files(node);
    declared_files.extend(
        super::prompting_impl::automation_node_declared_artifacts_to_create(node, runtime_values),
    );
    let mut files = declared_files
        .into_iter()
        .map(|path| automation_runtime_placeholder_replace(&path, runtime_values))
        .filter(|path| {
            let trimmed = path.trim();
            if trimmed.is_empty() {
                return false;
            }
            let lowered = trimmed.to_ascii_lowercase();
            if read_only_names.contains(&lowered) {
                return false;
            }
            let filename = std::path::Path::new(trimmed)
                .file_name()
                .and_then(|value| value.to_str())
                .map(|value| value.to_ascii_lowercase());
            if filename
                .as_ref()
                .is_some_and(|value| read_only_names.contains(value))
            {
                return false;
            }
            if let Some(root) = automation.workspace_root.as_deref() {
                if let Some(normalized) = normalize_workspace_display_path(root, trimmed) {
                    let normalized_lower = normalized.to_ascii_lowercase();
                    if read_only_names.contains(&normalized_lower) {
                        return false;
                    }
                    let normalized_filename = std::path::Path::new(&normalized)
                        .file_name()
                        .and_then(|value| value.to_str())
                        .map(|value| value.to_ascii_lowercase());
                    if normalized_filename
                        .as_ref()
                        .is_some_and(|value| read_only_names.contains(value))
                    {
                        return false;
                    }
                }
            }
            true
        })
        .collect::<Vec<_>>();
    if !automation_node_can_access_declared_output_targets(automation, node) {
        let blocked_targets = automation_declared_output_target_aliases(automation, runtime_values);
        files.retain(|path| {
            !automation_path_matches_declared_output_target(automation, &blocked_targets, path)
        });
    }
    files.sort();
    files.dedup();
    files
}

pub(crate) fn automation_node_bootstrap_missing_files(node: &AutomationFlowNode) -> Vec<String> {
    enforcement::automation_node_inferred_bootstrap_required_files(node)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutomationArtifactPublishScope {
    Workspace,
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutomationArtifactPublishMode {
    SnapshotReplace,
    AppendJsonl,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutomationArtifactPublishSpec {
    pub(crate) scope: AutomationArtifactPublishScope,
    pub(crate) path: String,
    pub(crate) mode: AutomationArtifactPublishMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutomationVerifiedOutputResolutionKind {
    Direct,
    LegacyPromoted,
    SessionTextRecovery,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutomationVerifiedOutputResolution {
    pub(crate) path: PathBuf,
    pub(crate) legacy_workspace_artifact_promoted_from: Option<PathBuf>,
    pub(crate) materialized_by_current_attempt: bool,
    pub(crate) resolution_kind: AutomationVerifiedOutputResolutionKind,
}

pub(crate) fn automation_node_publish_spec(
    node: &AutomationFlowNode,
) -> Option<AutomationArtifactPublishSpec> {
    let publish = node
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get("publish"))
        .and_then(Value::as_object)?;
    let scope = match publish
        .get("scope")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_ascii_lowercase()
        .as_str()
    {
        "workspace" => AutomationArtifactPublishScope::Workspace,
        "global" => AutomationArtifactPublishScope::Global,
        _ => return None,
    };
    let path = publish
        .get("path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let mode = match publish
        .get("mode")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("snapshot_replace")
        .to_ascii_lowercase()
        .as_str()
    {
        "snapshot_replace" => AutomationArtifactPublishMode::SnapshotReplace,
        "append_jsonl" => AutomationArtifactPublishMode::AppendJsonl,
        _ => return None,
    };
    Some(AutomationArtifactPublishSpec { scope, path, mode })
}

pub(crate) fn automation_output_path_uses_legacy_workspace_artifact_contract(
    workspace_root: &str,
    output_path: &str,
) -> bool {
    let normalized = normalize_automation_path_text(output_path)
        .unwrap_or_else(|| output_path.trim().to_string())
        .replace('\\', "/");
    if normalized == ".tandem/artifacts" || normalized.starts_with(".tandem/artifacts/") {
        return true;
    }
    let Ok(resolved) = resolve_automation_output_path(workspace_root, output_path) else {
        return false;
    };
    let workspace = PathBuf::from(
        normalize_automation_path_text(workspace_root)
            .unwrap_or_else(|| workspace_root.trim().to_string()),
    );
    let workspace = if workspace.is_absolute() {
        workspace
    } else {
        let Ok(current_dir) = std::env::current_dir() else {
            return false;
        };
        current_dir.join(workspace)
    };
    let Ok(relative) = resolved.strip_prefix(&workspace) else {
        return false;
    };
    let relative = normalize_automation_path_text(relative.to_string_lossy().as_ref())
        .unwrap_or_default()
        .replace('\\', "/");
    relative == ".tandem/artifacts" || relative.starts_with(".tandem/artifacts/")
}

pub(crate) fn maybe_promote_legacy_workspace_artifact_for_run(
    session: &Session,
    workspace_root: &str,
    run_id: &str,
    output_path: &str,
) -> anyhow::Result<Option<AutomationVerifiedOutputResolution>> {
    if !automation_output_path_uses_legacy_workspace_artifact_contract(workspace_root, output_path)
    {
        return Ok(None);
    }
    if !session_write_touched_output_for_output(session, workspace_root, output_path, None, None) {
        return Ok(None);
    }

    let legacy_path = resolve_automation_output_path(workspace_root, output_path)?;
    let run_scoped_path =
        resolve_automation_output_path_for_run(workspace_root, run_id, output_path)?;
    if legacy_path == run_scoped_path {
        return Ok(None);
    }
    if !legacy_path.exists() || !legacy_path.is_file() {
        return Ok(None);
    }
    if let Some(parent) = run_scoped_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(&legacy_path, &run_scoped_path).map_err(|error| {
        anyhow::anyhow!(
            "failed to promote legacy workspace artifact `{}` into run-scoped artifact `{}`: {}",
            legacy_path.display(),
            run_scoped_path.display(),
            error
        )
    })?;
    Ok(Some(AutomationVerifiedOutputResolution {
        path: run_scoped_path,
        legacy_workspace_artifact_promoted_from: Some(legacy_path),
        materialized_by_current_attempt: true,
        resolution_kind: AutomationVerifiedOutputResolutionKind::LegacyPromoted,
    }))
}

pub(crate) fn resolve_automation_published_output_path(
    workspace_root: &str,
    spec: &AutomationArtifactPublishSpec,
) -> anyhow::Result<PathBuf> {
    match spec.scope {
        AutomationArtifactPublishScope::Workspace => {
            resolve_automation_output_path(workspace_root, &spec.path)
        }
        AutomationArtifactPublishScope::Global => {
            let trimmed = spec.path.trim();
            if trimmed.is_empty() {
                anyhow::bail!("global publication path is empty");
            }
            let relative = PathBuf::from(trimmed);
            if relative.is_absolute() {
                anyhow::bail!(
                    "global publication path `{}` must be relative to the Tandem publication root",
                    trimmed
                );
            }
            let base = config::paths::resolve_automation_published_artifacts_dir();
            let candidate = base.join(relative);
            let normalized = PathBuf::from(
                normalize_automation_path_text(candidate.to_string_lossy().as_ref())
                    .unwrap_or_else(|| candidate.to_string_lossy().to_string()),
            );
            if !normalized.starts_with(&base) {
                anyhow::bail!(
                    "global publication path `{}` must stay inside `{}`",
                    trimmed,
                    base.display()
                );
            }
            Ok(normalized)
        }
    }
}

pub(crate) fn display_automation_published_output_path(
    workspace_root: &str,
    resolved: &PathBuf,
    spec: &AutomationArtifactPublishSpec,
) -> String {
    match spec.scope {
        AutomationArtifactPublishScope::Workspace => resolved
            .strip_prefix(workspace_root)
            .ok()
            .and_then(|value| value.to_str().map(str::to_string))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| spec.path.clone()),
        AutomationArtifactPublishScope::Global => resolved.to_string_lossy().to_string(),
    }
}

pub(crate) fn publish_automation_verified_output(
    workspace_root: &str,
    automation: &AutomationV2Spec,
    run_id: &str,
    node: &AutomationFlowNode,
    verified_output: &(String, String),
    spec: &AutomationArtifactPublishSpec,
) -> anyhow::Result<Value> {
    let source_path = resolve_automation_output_path(workspace_root, &verified_output.0)?;
    let destination = resolve_automation_published_output_path(workspace_root, spec)?;
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if source_path == destination {
        return Ok(json!({
            "scope": match spec.scope {
                AutomationArtifactPublishScope::Workspace => "workspace",
                AutomationArtifactPublishScope::Global => "global",
            },
            "mode": match spec.mode {
                AutomationArtifactPublishMode::SnapshotReplace => "snapshot_replace",
                AutomationArtifactPublishMode::AppendJsonl => "append_jsonl",
            },
            "path": display_automation_published_output_path(workspace_root, &destination, spec),
            "source_artifact_path": verified_output.0,
            "appended_records": None::<u64>,
            "copied": false,
        }));
    }

    let mut appended_records = None;
    match spec.mode {
        AutomationArtifactPublishMode::SnapshotReplace => {
            std::fs::copy(&source_path, &destination).map_err(|error| {
                anyhow::anyhow!(
                    "failed to publish validated run artifact `{}` to `{}`: {}",
                    source_path.display(),
                    destination.display(),
                    error
                )
            })?;
        }
        AutomationArtifactPublishMode::AppendJsonl => {
            use std::io::Write;

            let content = std::fs::read_to_string(&source_path).map_err(|error| {
                anyhow::anyhow!(
                    "failed to read validated run artifact `{}` before publication: {}",
                    source_path.display(),
                    error
                )
            })?;
            let appended_record = json!({
                "automation_id": automation.automation_id,
                "run_id": run_id,
                "node_id": node.node_id,
                "source_artifact_path": verified_output.0,
                "published_at_ms": now_ms(),
                "content": serde_json::from_str::<Value>(&content).unwrap_or_else(|_| Value::String(content.clone())),
            });
            let line = serde_json::to_string(&appended_record)?;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&destination)
                .map_err(|error| {
                    anyhow::anyhow!(
                        "failed to open publication target `{}` for append_jsonl: {}",
                        destination.display(),
                        error
                    )
                })?;
            writeln!(file, "{line}").map_err(|error| {
                anyhow::anyhow!(
                    "failed to append published run artifact to `{}`: {}",
                    destination.display(),
                    error
                )
            })?;
            appended_records = Some(1);
        }
    }

    Ok(json!({
        "scope": match spec.scope {
            AutomationArtifactPublishScope::Workspace => "workspace",
            AutomationArtifactPublishScope::Global => "global",
        },
        "mode": match spec.mode {
            AutomationArtifactPublishMode::SnapshotReplace => "snapshot_replace",
            AutomationArtifactPublishMode::AppendJsonl => "append_jsonl",
        },
        "path": display_automation_published_output_path(workspace_root, &destination, spec),
        "source_artifact_path": verified_output.0,
        "appended_records": appended_records,
        "copied": true,
    }))
}

pub(crate) fn automation_output_target_publish_specs(
    targets: &[String],
) -> Vec<AutomationArtifactPublishSpec> {
    let mut specs = Vec::new();
    let mut seen = HashSet::new();
    for target in targets {
        let trimmed = target.trim();
        if trimmed.is_empty() {
            continue;
        }
        let normalized = trimmed.strip_prefix("file://").unwrap_or(trimmed).trim();
        if normalized.is_empty() || normalized.contains("://") {
            continue;
        }
        let spec = AutomationArtifactPublishSpec {
            scope: AutomationArtifactPublishScope::Workspace,
            path: normalized.to_string(),
            mode: AutomationArtifactPublishMode::SnapshotReplace,
        };
        if seen.insert(spec.path.clone()) {
            specs.push(spec);
        }
    }
    specs
}

pub(crate) fn publish_automation_verified_outputs(
    workspace_root: &str,
    automation: &AutomationV2Spec,
    run_id: &str,
    node: &AutomationFlowNode,
    verified_output: &(String, String),
) -> anyhow::Result<Value> {
    if !automation_node_can_access_declared_output_targets(automation, node) {
        anyhow::bail!(
            "node `{}` is not allowed to publish to automation-level output targets",
            node.node_id
        );
    }
    let publications = automation_output_target_publish_specs(&automation.output_targets)
        .into_iter()
        .map(|spec| {
            publish_automation_verified_output(
                workspace_root,
                automation,
                run_id,
                node,
                verified_output,
                &spec,
            )
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(json!({ "targets": publications }))
}

pub(crate) fn automation_node_web_research_expected(node: &AutomationFlowNode) -> bool {
    node_runtime_impl::automation_node_web_research_expected(node)
}

pub(crate) fn automation_node_required_tools(node: &AutomationFlowNode) -> Vec<String> {
    node_runtime_impl::automation_node_required_tools(node)
}

pub(crate) fn automation_node_execution_policy(
    node: &AutomationFlowNode,
    workspace_root: &str,
) -> Value {
    node_runtime_impl::automation_node_execution_policy(node, workspace_root)
}

pub(crate) fn resolve_automation_output_path(
    workspace_root: &str,
    output_path: &str,
) -> anyhow::Result<PathBuf> {
    let trimmed = output_path.trim();
    if trimmed.is_empty() {
        anyhow::bail!("required output path is empty");
    }
    let workspace = PathBuf::from(
        normalize_automation_path_text(workspace_root)
            .unwrap_or_else(|| workspace_root.trim().to_string()),
    );
    let workspace = if workspace.is_absolute() {
        workspace
    } else {
        std::env::current_dir()?.join(workspace)
    };
    let candidate = PathBuf::from(trimmed);
    let resolved = if candidate.is_absolute() {
        candidate
    } else {
        workspace.join(candidate)
    };
    let normalized_resolved = PathBuf::from(
        normalize_automation_path_text(resolved.to_string_lossy().as_ref())
            .unwrap_or_else(|| resolved.to_string_lossy().to_string()),
    );
    if !normalized_resolved.starts_with(&workspace) {
        anyhow::bail!(
            "required output path `{}` must stay inside workspace `{}`",
            trimmed,
            workspace_root
        );
    }
    Ok(normalized_resolved)
}
