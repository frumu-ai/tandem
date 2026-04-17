
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

pub(crate) fn automation_effective_required_output_path_for_run(
    automation: &AutomationV2Spec,
    node: &AutomationFlowNode,
    run_id: &str,
    started_at_ms: u64,
) -> Option<String> {
    let runtime_values = automation_prompt_runtime_values(Some(started_at_ms));
    automation_node_required_output_path_with_runtime_for_run(
        node,
        Some(run_id),
        Some(&runtime_values),
    )
    .or_else(|| {
        if is_agent_standup_automation(automation) && node.node_id == "standup_synthesis" {
            resolve_standup_report_path_for_run(automation, started_at_ms)
        } else {
            None
        }
    })
}

/// Derives the receipt path from the standup report path by inserting a
/// "receipt-" prefix on the filename and replacing the extension with ".json".
/// Example: "docs/standups/2026-04-05.md" → "docs/standups/receipt-2026-04-05.json"
pub(crate) fn standup_receipt_path_for_report(report_path: &str) -> String {
    let p = std::path::Path::new(report_path);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("standup");
    let dir = p
        .parent()
        .and_then(|d| d.to_str())
        .filter(|d| !d.is_empty())
        .unwrap_or("docs/standups");
    format!("{dir}/receipt-{stem}.json")
}

/// Builds an operator-facing JSON receipt for a completed standup run.
/// Sources all data from existing structures: run checkpoint, lifecycle history,
/// node outputs, and the coordinator's assessment score.
/// Returns None if the run data is not available or this is not a standup run.
fn build_standup_run_receipt(
    run: &AutomationV2RunRecord,
    automation: &AutomationV2Spec,
    run_id: &str,
    report_path: &str,
    coordinator_assessment: &ArtifactCandidateAssessment,
) -> Option<Value> {
    let completed_at_iso = run
        .finished_at_ms
        .or(run.started_at_ms)
        .map(|ms| {
            chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms as i64)
                .unwrap_or_else(chrono::Utc::now)
                .to_rfc3339()
        })
        .unwrap_or_else(|| "unknown".to_string());

    // Count lifecycle events by type for summary
    let lifecycle_events = &run.checkpoint.lifecycle_history;
    let total_events = lifecycle_events.len();
    let total_repair_cycles = lifecycle_events
        .iter()
        .filter(|e| e.event == "node_repair_requested")
        .count();
    // Filler rejections are repair cycles on standup_update nodes
    let total_filler_rejections = lifecycle_events
        .iter()
        .filter(|e| {
            e.event == "node_repair_requested"
                && e.metadata
                    .as_ref()
                    .and_then(|m| m.get("contract_kind"))
                    .and_then(Value::as_str)
                    .is_some_and(|k| k == "standup_update")
        })
        .count();

    // Build per-participant summaries from node outputs
    let participants: Vec<Value> = automation
        .flow
        .nodes
        .iter()
        .filter(|n| n.node_id != "standup_synthesis")
        .map(|participant_node| {
            let node_output = run
                .checkpoint
                .node_outputs
                .get(&participant_node.node_id);
            let attempts = run
                .checkpoint
                .node_attempts
                .get(&participant_node.node_id)
                .copied()
                .unwrap_or(0);
            let status = node_output
                .and_then(|o| o.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            // Extract yesterday/today from the participant's standup JSON,
            // stored in the node output content text
            let standup_json = node_output
                .and_then(|o| o.get("content"))
                .and_then(|c| c.get("text").or_else(|| c.get("raw_assistant_text")))
                .and_then(Value::as_str)
                .and_then(|text| serde_json::from_str::<Value>(text).ok());
            let yesterday = standup_json
                .as_ref()
                .and_then(|v| v.get("yesterday"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let today = standup_json
                .as_ref()
                .and_then(|v| v.get("today"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let filler_rejected = lifecycle_events.iter().any(|e| {
                e.event == "node_repair_requested"
                    && e.metadata
                        .as_ref()
                        .and_then(|m| m.get("node_id"))
                        .and_then(Value::as_str)
                        .is_some_and(|id| id == participant_node.node_id)
            });
            // Derive a readable name from the node_id (e.g., "participant_0_copywriter")
            let display_name = participant_node
                .node_id
                .splitn(3, '_')
                .nth(2)
                .unwrap_or(&participant_node.node_id)
                .replace('_', " ");
            json!({
                "node_id": participant_node.node_id,
                "display_name": display_name,
                "attempts": attempts,
                "status": status,
                "filler_rejected": filler_rejected,
                "yesterday_summary": if yesterday.is_empty() { Value::Null } else { json!(yesterday) },
                "today_summary": if today.is_empty() { Value::Null } else { json!(today) },
            })
        })
        .collect();

    let coordinator_attempts = run
        .checkpoint
        .node_attempts
        .get("standup_synthesis")
        .copied()
        .unwrap_or(0);

    Some(json!({
        "run_id": run_id,
        "automation_id": automation.automation_id,
        "automation_name": automation.name,
        "completed_at_iso": completed_at_iso,
        "report_path": report_path,
        "participants": participants,
        "coordinator": {
            "node_id": "standup_synthesis",
            "attempts": coordinator_attempts,
            "report_path": report_path,
            "assessment": assessment::artifact_candidate_summary(coordinator_assessment, true),
        },
        "lifecycle_event_count": total_events,
        "total_repair_cycles": total_repair_cycles,
        "total_filler_rejections": total_filler_rejections,
    }))
}

fn automation_workspace_project_id(workspace_root: &str) -> String {
    node_runtime_impl::automation_workspace_project_id(workspace_root)
}

fn merge_automation_agent_allowlist(
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

fn automation_node_task_kind(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_task_kind(node)
}

fn automation_node_knowledge_task_family(node: &AutomationFlowNode) -> String {
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

fn automation_node_projects_backlog_tasks(node: &AutomationFlowNode) -> bool {
    node_runtime_impl::automation_node_projects_backlog_tasks(node)
}

fn automation_node_task_id(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_task_id(node)
}

fn automation_node_repo_root(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_repo_root(node)
}

fn automation_node_write_scope(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_write_scope(node)
}

fn automation_node_acceptance_criteria(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_acceptance_criteria(node)
}

fn automation_node_task_dependencies(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_task_dependencies(node)
}

fn automation_node_task_owner(node: &AutomationFlowNode) -> Option<String> {
    node_runtime_impl::automation_node_task_owner(node)
}

fn automation_node_is_code_workflow(node: &AutomationFlowNode) -> bool {
    node_runtime_impl::automation_node_is_code_workflow(node)
}

pub(crate) fn automation_output_validator_kind(
    node: &AutomationFlowNode,
) -> crate::AutomationOutputValidatorKind {
    node_runtime_impl::automation_output_validator_kind(node)
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

fn automation_node_execution_mode(node: &AutomationFlowNode, workspace_root: &str) -> &'static str {
    node_runtime_impl::automation_node_execution_mode(node, workspace_root)
}

pub(crate) fn normalize_automation_requested_tools(
    node: &AutomationFlowNode,
    workspace_root: &str,
    raw: Vec<String>,
) -> Vec<String> {
    let handoff_only_structured_json = automation_output_validator_kind(node)
        == crate::AutomationOutputValidatorKind::StructuredJson
        && automation_node_required_output_path(node).is_none();
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
    if handoff_only_structured_json {
        normalized.retain(|tool| !matches!(tool.as_str(), "write" | "edit" | "apply_patch"));
    }
    normalized.sort();
    normalized.dedup();
    normalized
}

fn automation_tool_name_is_email_delivery(tool_name: &str) -> bool {
    node_runtime_impl::automation_tool_name_is_email_delivery(tool_name)
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
    let workspace_inspection_required = requested_tools
        .iter()
        .any(|tool| matches!(tool.as_str(), "glob" | "ls" | "list" | "read"));
    let web_research_required =
        web_research_expected && requested_tools.iter().any(|tool| tool == "websearch");
    let brief_research_node = validation_profile == "local_research";
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
            && !research_finalize
            && explicit_input_files.is_empty(),
        web_research_required: web_research_required && !research_finalize,
        concrete_read_required,
        successful_web_research_required,
        repair_on_unmet_requirements: brief_research_node
            || has_any_required_tools
            || !enforcement.retry_on_missing.is_empty(),
        coverage_mode: if brief_research_node {
            PrewriteCoverageMode::ResearchCorpus
        } else {
            PrewriteCoverageMode::None
        },
    })
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
    if has_unmet("current_attempt_output_missing") {
        Some("required output was not created in the current attempt".to_string())
    } else if has_unmet("structured_handoff_missing") {
        Some("structured handoff was not returned in the final response".to_string())
    } else if has_unmet("workspace_inspection_required") {
        Some("structured handoff completed without required workspace inspection".to_string())
    } else if has_unmet("mcp_discovery_missing") {
        Some("connector-backed work completed without discovering available MCP tools".to_string())
    } else if has_unmet("missing_successful_web_research") {
        Some("research completed without required current web research".to_string())
    } else if has_unmet("required_source_paths_not_read") {
        Some("research completed without reading the exact required source files".to_string())
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
            .filter(|spec| crate::app::routines::provider_catalog_has_model(&providers, spec))
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

pub(crate) fn automation_node_allows_preexisting_output_reuse(node: &AutomationFlowNode) -> bool {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
        .and_then(|builder| builder.get("allow_preexisting_output_reuse"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn automation_node_explicit_input_files(node: &AutomationFlowNode) -> Vec<String> {
    let mut files = automation_node_builder_string_array(node, "input_files");
    files.sort();
    files.dedup();
    files
}

fn automation_node_explicit_output_files(node: &AutomationFlowNode) -> Vec<String> {
    let mut files = automation_node_builder_string_array(node, "output_files");
    files.sort();
    files.dedup();
    files
}

fn automation_declared_output_target_aliases(
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

fn automation_path_matches_declared_output_target(
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

fn automation_node_is_terminal_for_automation(
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

fn automation_node_can_access_declared_output_targets(
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

fn automation_node_effective_output_files_for_automation(
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

fn automation_node_must_write_files(node: &AutomationFlowNode) -> Vec<String> {
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

fn automation_keyword_variants(token: &str) -> Vec<String> {
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

fn automation_keyword_set(text: &str) -> HashSet<String> {
    text.split(|ch: char| !ch.is_ascii_alphanumeric())
        .flat_map(automation_keyword_variants)
        .collect()
}

fn automation_output_target_matches_node_objective(
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
    let mut files = automation_node_must_write_files(node)
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

fn automation_node_bootstrap_missing_files(node: &AutomationFlowNode) -> Vec<String> {
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
    scope: AutomationArtifactPublishScope,
    path: String,
    mode: AutomationArtifactPublishMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutomationVerifiedOutputResolutionKind {
    Direct,
    LegacyPromoted,
    SessionTextRecovery,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AutomationVerifiedOutputResolution {
    path: PathBuf,
    legacy_workspace_artifact_promoted_from: Option<PathBuf>,
    materialized_by_current_attempt: bool,
    resolution_kind: AutomationVerifiedOutputResolutionKind,
}

fn automation_node_publish_spec(
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

fn automation_output_path_uses_legacy_workspace_artifact_contract(
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
    let Ok(relative) = resolved.strip_prefix(&workspace) else {
        return false;
    };
    let relative = normalize_automation_path_text(relative.to_string_lossy().as_ref())
        .unwrap_or_default()
        .replace('\\', "/");
    relative == ".tandem/artifacts" || relative.starts_with(".tandem/artifacts/")
}

fn maybe_promote_legacy_workspace_artifact_for_run(
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

fn resolve_automation_published_output_path(
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

fn display_automation_published_output_path(
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

fn automation_output_target_publish_specs(
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

fn automation_node_web_research_expected(node: &AutomationFlowNode) -> bool {
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

fn resolve_automation_output_path(
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

fn normalize_automation_path_text(raw_path: &str) -> Option<String> {
    let trimmed = raw_path.trim();
    if trimmed.is_empty() {
        return None;
    }
    let path = PathBuf::from(trimmed);
    let is_absolute = path.is_absolute();
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() && !is_absolute {
                    normalized.push("..");
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    let normalized = normalized.to_string_lossy().trim().to_string();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn automation_run_artifact_root(run_id: &str) -> Option<String> {
    let trimmed = run_id.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(format!(".tandem/runs/{trimmed}/artifacts"))
    }
}

pub(crate) fn automation_run_scoped_output_path(run_id: &str, output_path: &str) -> Option<String> {
    let normalized = normalize_automation_path_text(output_path)?.replace('\\', "/");
    let prefix = ".tandem/artifacts/";
    if let Some(suffix) = normalized.strip_prefix(prefix) {
        let root = automation_run_artifact_root(run_id)?;
        return Some(if suffix.is_empty() {
            root
        } else {
            format!("{root}/{suffix}")
        });
    }
    Some(normalized)
}

fn automation_run_scoped_absolute_output_path(
    workspace_root: &str,
    run_id: &str,
    output_path: &str,
) -> Option<String> {
    let candidate = PathBuf::from(normalize_automation_path_text(output_path)?);
    if !candidate.is_absolute() {
        return None;
    }
    let workspace = PathBuf::from(normalize_automation_path_text(workspace_root)?);
    let relative = candidate.strip_prefix(&workspace).ok()?;
    let relative_text =
        normalize_automation_path_text(relative.to_string_lossy().as_ref())?.replace('\\', "/");
    if relative_text == ".tandem/artifacts" {
        return automation_run_artifact_root(run_id);
    }
    let suffix = relative_text.strip_prefix(".tandem/artifacts/")?;
    let root = automation_run_artifact_root(run_id)?;
    Some(if suffix.is_empty() {
        root
    } else {
        format!("{root}/{suffix}")
    })
}

pub(crate) fn resolve_automation_output_path_for_run(
    workspace_root: &str,
    run_id: &str,
    output_path: &str,
) -> anyhow::Result<PathBuf> {
    let scoped_output_path =
        automation_run_scoped_absolute_output_path(workspace_root, run_id, output_path)
            .or_else(|| automation_run_scoped_output_path(run_id, output_path))
            .unwrap_or_else(|| output_path.trim().to_string());
    resolve_automation_output_path(workspace_root, &scoped_output_path)
}

fn automation_node_output_sibling_extensions(node: &AutomationFlowNode) -> &'static [&'static str] {
    let kind = node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.trim())
        .unwrap_or("structured_json");
    if kind.eq_ignore_ascii_case("report_markdown") {
        &["html", "htm", "md", "markdown", "txt"]
    } else {
        &[]
    }
}

pub(crate) fn automation_output_path_candidates(
    workspace_root: &str,
    run_id: &str,
    node: &AutomationFlowNode,
    output_path: &str,
) -> anyhow::Result<Vec<PathBuf>> {
    let resolved = resolve_automation_output_path_for_run(workspace_root, run_id, output_path)?;
    let mut candidates = vec![resolved.clone()];
    let sibling_extensions = automation_node_output_sibling_extensions(node);
    if sibling_extensions.is_empty() {
        return Ok(candidates);
    }

    let Some(parent) = resolved.parent() else {
        return Ok(candidates);
    };
    let Some(stem) = resolved.file_stem().and_then(|value| value.to_str()) else {
        return Ok(candidates);
    };

    let Ok(entries) = std::fs::read_dir(parent) else {
        return Ok(candidates);
    };
    let mut siblings = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && *path != resolved)
        .filter(|path| {
            path.file_stem()
                .and_then(|value| value.to_str())
                .is_some_and(|candidate_stem| candidate_stem == stem)
        })
        .filter(|path| {
            path.extension()
                .and_then(|value| value.to_str())
                .is_some_and(|extension| {
                    sibling_extensions
                        .iter()
                        .any(|candidate| candidate.eq_ignore_ascii_case(extension))
                })
        })
        .collect::<Vec<_>>();
    siblings.sort_by(|left, right| left.to_string_lossy().cmp(&right.to_string_lossy()));
    siblings.dedup();
    candidates.extend(siblings);
    candidates.dedup();
    Ok(candidates)
}

fn session_write_paths_for_output_candidates(
    session: &Session,
    workspace_root: &str,
    run_id: &str,
    candidate_paths: &[PathBuf],
) -> Vec<PathBuf> {
    let candidate_paths = candidate_paths.iter().cloned().collect::<HashSet<_>>();
    let mut paths = Vec::new();
    for message in &session.messages {
        for part in &message.parts {
            let MessagePart::ToolInvocation {
                tool, args, error, ..
            } = part
            else {
                continue;
            };
            if !tool.eq_ignore_ascii_case("write")
                || error.as_ref().is_some_and(|value| !value.trim().is_empty())
            {
                continue;
            }
            let Some(args) = tool_args_object(args) else {
                continue;
            };
            let Some(path) = args.get("path").and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            let Ok(candidate_path) =
                resolve_automation_output_path_for_run(workspace_root, run_id, path)
            else {
                continue;
            };
            if !candidate_paths.contains(&candidate_path) {
                continue;
            }
            if !paths.iter().any(|existing| existing == &candidate_path) {
                paths.push(candidate_path);
            }
        }
    }
    paths
}

pub(crate) fn automation_resolve_verified_output_path(
    session: &Session,
    workspace_root: &str,
    run_id: &str,
    node: &AutomationFlowNode,
    output_path: &str,
) -> anyhow::Result<Option<PathBuf>> {
    let candidates = automation_output_path_candidates(workspace_root, run_id, node, output_path)?;
    let session_written_candidates =
        session_write_paths_for_output_candidates(session, workspace_root, run_id, &candidates);
    Ok(session_written_candidates
        .into_iter()
        .chain(candidates.into_iter())
        .find(|candidate| candidate.exists() && candidate.is_file()))
}

pub(crate) async fn reconcile_automation_resolve_verified_output_path(
    session: &Session,
    workspace_root: &str,
    run_id: &str,
    node: &AutomationFlowNode,
    output_path: &str,
    max_wait_ms: u64,
    poll_interval_ms: u64,
) -> anyhow::Result<Option<AutomationVerifiedOutputResolution>> {
    let output_touched = session_write_touched_output_for_output(
        session,
        workspace_root,
        output_path,
        Some(run_id),
        None,
    );
    let poll_interval_ms = poll_interval_ms.max(1);
    let start_ms = now_ms() as u64;

    loop {
        let candidates =
            automation_output_path_candidates(workspace_root, run_id, node, output_path)?;
        let session_written_candidates =
            session_write_paths_for_output_candidates(session, workspace_root, run_id, &candidates);
        if let Some(resolved) = automation_resolve_verified_output_path(
            session,
            workspace_root,
            run_id,
            node,
            output_path,
        )? {
            let materialized_by_current_attempt = session_written_candidates
                .iter()
                .any(|candidate| candidate == &resolved);
            return Ok(Some(AutomationVerifiedOutputResolution {
                path: resolved,
                legacy_workspace_artifact_promoted_from: None,
                materialized_by_current_attempt,
                resolution_kind: AutomationVerifiedOutputResolutionKind::Direct,
            }));
        }
        if let Some(promoted) = maybe_promote_legacy_workspace_artifact_for_run(
            session,
            workspace_root,
            run_id,
            output_path,
        )? {
            return Ok(Some(AutomationVerifiedOutputResolution {
                materialized_by_current_attempt: output_touched
                    || promoted.materialized_by_current_attempt,
                ..promoted
            }));
        }
        if let Some(recovered) =
            recover_required_output_from_session_text(session, workspace_root, run_id, output_path)?
        {
            return Ok(Some(AutomationVerifiedOutputResolution {
                path: recovered,
                legacy_workspace_artifact_promoted_from: None,
                materialized_by_current_attempt: true,
                resolution_kind: AutomationVerifiedOutputResolutionKind::SessionTextRecovery,
            }));
        }
        if !output_touched {
            return Ok(None);
        }
        let elapsed_ms = now_ms() as u64 - start_ms;
        if elapsed_ms >= max_wait_ms {
            return Ok(None);
        }
        let sleep_ms = poll_interval_ms.min(max_wait_ms.saturating_sub(elapsed_ms));
        tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
    }
}

fn recover_required_output_from_session_text(
    session: &Session,
    workspace_root: &str,
    run_id: &str,
    output_path: &str,
) -> anyhow::Result<Option<PathBuf>> {
    let resolved = resolve_automation_output_path_for_run(workspace_root, run_id, output_path)?;
    let Some(extension) = resolved.extension().and_then(|value| value.to_str()) else {
        return Ok(None);
    };
    if !extension.eq_ignore_ascii_case("json") {
        return Ok(None);
    }
    let payload = extract_recoverable_json_from_session(session);
    let Some(payload) = payload else {
        return Ok(None);
    };
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let serialized = serde_json::to_string_pretty(&payload)?;
    std::fs::write(&resolved, serialized)?;
    Ok(Some(resolved))
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
