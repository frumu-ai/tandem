use super::*;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutomationQualityMode {
    StrictResearchV1,
    Legacy,
}

impl AutomationQualityMode {
    pub(crate) fn stable_key(self) -> &'static str {
        match self {
            Self::StrictResearchV1 => "strict_research_v1",
            Self::Legacy => "legacy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AutomationQualityModeResolution {
    pub(crate) requested: Option<AutomationQualityMode>,
    pub(crate) effective: AutomationQualityMode,
    pub(crate) legacy_rollback_enabled: bool,
}

pub(crate) fn enforcement_requires_external_sources(
    enforcement: &crate::AutomationOutputEnforcement,
) -> bool {
    enforcement
        .required_evidence
        .iter()
        .any(|item| item == "external_sources")
        || enforcement
            .required_tools
            .iter()
            .any(|tool| tool == "websearch")
        || enforcement
            .prewrite_gates
            .iter()
            .any(|gate| gate == "successful_web_research")
}

fn automation_node_legacy_builder(
    node: &AutomationFlowNode,
) -> Option<&serde_json::Map<String, Value>> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(Value::as_object)
}

fn automation_node_legacy_web_research_expected(node: &AutomationFlowNode) -> bool {
    if let Some(explicit) = automation_node_legacy_builder(node)
        .and_then(|builder| builder.get("web_research_expected"))
        .and_then(Value::as_bool)
    {
        return explicit;
    }
    let intent = automation_node_workspace_intent_text(node).to_ascii_lowercase();
    intent.contains("web")
        || intent.contains("online")
        || intent.contains("current")
        || intent.contains("latest")
}

pub(crate) fn automation_node_prefers_mcp_servers(node: &AutomationFlowNode) -> bool {
    automation_node_legacy_builder(node)
        .and_then(|builder| builder.get("preferred_mcp_servers"))
        .and_then(Value::as_array)
        .is_some_and(|servers| {
            servers
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .any(|value| !value.is_empty())
        })
}

fn automation_node_legacy_required_tools(node: &AutomationFlowNode) -> Vec<String> {
    automation_node_legacy_builder(node)
        .and_then(|builder| builder.get("required_tools"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn automation_node_workspace_intent_text(node: &AutomationFlowNode) -> String {
    [
        node.objective.as_str(),
        automation_node_legacy_builder(node)
            .and_then(|builder| builder.get("prompt"))
            .and_then(Value::as_str)
            .unwrap_or_default(),
    ]
    .join("\n")
}

fn automation_trim_workspace_token(token: &str) -> &str {
    token
        .trim()
        .trim_matches(|c: char| {
            matches!(
                c,
                '`' | '"' | '\'' | ',' | ';' | ':' | '(' | ')' | '[' | ']' | '{' | '}'
            )
        })
        .trim_end_matches(['.', '!', '?'])
}

fn automation_text_has_workspace_tokens(text: &str) -> bool {
    text.split_whitespace().any(|token| {
        let trimmed = automation_trim_workspace_token(token);
        !trimmed.is_empty()
            && !trimmed.starts_with("http://")
            && !trimmed.starts_with("https://")
            && (trimmed.contains('/')
                || trimmed.ends_with(".md")
                || trimmed.ends_with(".yaml")
                || trimmed.ends_with(".yml")
                || trimmed.ends_with(".json")
                || trimmed.ends_with(".jsonl")
                || trimmed.ends_with(".txt")
                || trimmed.ends_with(".csv"))
    })
}

fn automation_token_looks_like_workspace_file(token: &str) -> bool {
    token.ends_with(".md")
        || token.ends_with(".markdown")
        || token.ends_with(".txt")
        || token.ends_with(".json")
        || token.ends_with(".jsonl")
        || token.ends_with(".yaml")
        || token.ends_with(".yml")
        || token.ends_with(".csv")
        || token.ends_with(".toml")
        || token.ends_with(".ini")
        || token.ends_with(".cfg")
        || token.ends_with(".conf")
        || token.ends_with(".env")
        || token.ends_with(".xml")
        || token.ends_with(".html")
        || token.ends_with(".sql")
}

fn automation_extract_workspace_file_tokens(text: &str) -> Vec<String> {
    let mut files = Vec::new();
    for token in text.split_whitespace() {
        let trimmed = automation_trim_workspace_token(token);
        if trimmed.is_empty() || trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            continue;
        }
        if trimmed.contains('/') {
            let segments = trimmed
                .split('/')
                .map(automation_trim_workspace_token)
                .filter(|segment| !segment.is_empty())
                .collect::<Vec<_>>();
            if segments.len() > 1
                && segments
                    .iter()
                    .all(|segment| automation_token_looks_like_workspace_file(segment))
            {
                files.extend(segments.into_iter().map(str::to_string));
                continue;
            }
        }
        if automation_token_looks_like_workspace_file(trimmed) {
            files.push(trimmed.to_string());
        }
    }
    files
}

fn automation_optional_read_file_tokens(text: &str) -> Vec<String> {
    let mut files = Vec::new();
    for clause in text.split(['\n', ';', ',']) {
        let lowered = clause.to_ascii_lowercase();
        let is_optional_read_clause = ["read", "inspect", "review", "open"]
            .iter()
            .any(|verb| lowered.contains(verb))
            && ["if present", "if available"]
                .iter()
                .any(|marker| lowered.contains(marker));
        if !is_optional_read_clause {
            continue;
        }
        files.extend(automation_extract_workspace_file_tokens(clause));
    }
    files.sort();
    files.dedup();
    files
}

pub(crate) fn automation_node_allows_optional_workspace_reads(node: &AutomationFlowNode) -> bool {
    let combined = automation_node_workspace_intent_text(node);
    if !automation_text_has_workspace_tokens(&combined) {
        return false;
    }
    let lowered = combined.to_ascii_lowercase();
    let has_write_intent = [
        "write",
        "update",
        "create",
        "initialize",
        "bootstrap",
        "merge",
        "append",
    ]
    .iter()
    .any(|needle| lowered.contains(needle));
    let has_bootstrap_or_missing_intent = [
        "missing",
        "initialize",
        "bootstrap",
        "directory",
        "directories",
        "folder",
        "folders",
        "workspace",
        "if present",
        "if available",
    ]
    .iter()
    .any(|needle| lowered.contains(needle));
    has_write_intent && has_bootstrap_or_missing_intent
}

pub(crate) fn automation_node_inferred_bootstrap_required_files(
    node: &AutomationFlowNode,
) -> Vec<String> {
    // Analysis and synthesis nodes with upstream dependencies consume files
    // from prior stages — they must never be required to create workspace files
    // whose names are inferred from objective text. This eliminates the class of
    // false `required_workspace_files_missing` blocks on report/analysis nodes.
    //
    // We check the output contract kind rather than `depends_on` alone, because
    // chained bootstrap nodes (structured_json with initialize/create intent)
    // legitimately write workspace files even when they have dependencies.
    let contract_kind = node
        .output_contract
        .as_ref()
        .map(|c| c.kind.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let is_report_or_synthesis_contract = matches!(
        contract_kind.as_str(),
        "brief" | "report_markdown" | "text_summary" | "citations"
    );
    if !node.depends_on.is_empty() && is_report_or_synthesis_contract {
        return Vec::new();
    }
    if is_report_or_synthesis_contract {
        return Vec::new();
    }
    let combined = automation_node_workspace_intent_text(node);
    if !automation_text_has_workspace_tokens(&combined) {
        return Vec::new();
    }
    let lowered = combined.to_ascii_lowercase();
    let has_bootstrap_write_intent = ["write", "create", "initialize", "bootstrap", "missing"]
        .iter()
        .any(|needle| lowered.contains(needle));
    if !has_bootstrap_write_intent {
        return Vec::new();
    }

    let optional_read_files = automation_optional_read_file_tokens(&combined)
        .into_iter()
        .map(|path| path.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    let mut files = automation_extract_workspace_file_tokens(&combined)
        .into_iter()
        .filter(|path| !optional_read_files.contains(&path.to_ascii_lowercase()))
        .filter(|path| {
            let path_lower = path.to_ascii_lowercase();
            let optional_read_patterns = [
                format!("read {} if present", path_lower),
                format!("read {} if available", path_lower),
                format!("inspect {} if present", path_lower),
                format!("inspect {} if available", path_lower),
                format!("review {} if present", path_lower),
                format!("review {} if available", path_lower),
                format!("open {} if present", path_lower),
                format!("open {} if available", path_lower),
            ];
            !optional_read_patterns
                .iter()
                .any(|pattern| lowered.contains(pattern))
        })
        .collect::<Vec<_>>();
    files.sort();
    files.dedup();
    files
}

fn parse_quality_mode(value: &str) -> Option<AutomationQualityMode> {
    match value.trim().to_ascii_lowercase().as_str() {
        "strict" | "strict_research_v1" | "strict-research-v1" => {
            Some(AutomationQualityMode::StrictResearchV1)
        }
        "legacy" => Some(AutomationQualityMode::Legacy),
        _ => None,
    }
}

fn requested_quality_mode_from_metadata(
    metadata: Option<&serde_json::Map<String, Value>>,
) -> Option<AutomationQualityMode> {
    metadata.and_then(|metadata| {
        metadata
            .get("quality_mode")
            .or_else(|| metadata.get("qualityMode"))
            .and_then(Value::as_str)
            .and_then(parse_quality_mode)
            .or_else(|| {
                metadata
                    .get("builder")
                    .and_then(Value::as_object)
                    .and_then(|builder| builder.get("quality_mode"))
                    .and_then(Value::as_str)
                    .and_then(parse_quality_mode)
            })
    })
}

pub(crate) fn automation_quality_mode_resolution_from_metadata(
    metadata: Option<&serde_json::Map<String, Value>>,
    strict_default: bool,
    legacy_rollback_enabled: bool,
) -> AutomationQualityModeResolution {
    let requested = requested_quality_mode_from_metadata(metadata);
    let effective = match requested {
        Some(AutomationQualityMode::Legacy) if !legacy_rollback_enabled => {
            AutomationQualityMode::StrictResearchV1
        }
        Some(mode) => mode,
        None => {
            if crate::config::env::resolve_automation_strict_research_quality() && strict_default {
                AutomationQualityMode::StrictResearchV1
            } else {
                AutomationQualityMode::Legacy
            }
        }
    };
    AutomationQualityModeResolution {
        requested,
        effective,
        legacy_rollback_enabled,
    }
}

pub(crate) fn automation_quality_mode_from_metadata(
    metadata: Option<&serde_json::Map<String, Value>>,
    strict_default: bool,
) -> AutomationQualityMode {
    automation_quality_mode_resolution_from_metadata(
        metadata,
        strict_default,
        crate::config::env::resolve_automation_quality_legacy_rollback_enabled(),
    )
    .effective
}

pub(crate) fn automation_node_quality_mode(node: &AutomationFlowNode) -> AutomationQualityMode {
    automation_quality_mode_from_metadata(node.metadata.as_ref().and_then(Value::as_object), true)
}

pub(crate) fn automation_node_quality_mode_resolution(
    node: &AutomationFlowNode,
) -> AutomationQualityModeResolution {
    automation_quality_mode_resolution_from_metadata(
        node.metadata.as_ref().and_then(Value::as_object),
        true,
        crate::config::env::resolve_automation_quality_legacy_rollback_enabled(),
    )
}

pub(crate) fn automation_node_is_strict_quality(node: &AutomationFlowNode) -> bool {
    matches!(
        automation_node_quality_mode(node),
        AutomationQualityMode::StrictResearchV1
    )
}

pub(crate) fn automation_node_output_enforcement(
    node: &AutomationFlowNode,
) -> crate::AutomationOutputEnforcement {
    let mut enforcement = node
        .output_contract
        .as_ref()
        .and_then(|contract| contract.enforcement.clone())
        .unwrap_or_default();
    let validator_kind = automation_output_validator_kind(node);
    let legacy_required_tools = automation_node_legacy_required_tools(node);
    let legacy_web_research_expected = automation_node_legacy_web_research_expected(node);
    let prefers_mcp_servers = automation_node_prefers_mcp_servers(node);
    let optional_workspace_reads = automation_node_allows_optional_workspace_reads(node);
    let is_research_contract =
        validator_kind == crate::AutomationOutputValidatorKind::ResearchBrief;
    let code_patch_contract = node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.trim().to_ascii_lowercase())
        .is_some_and(|kind| kind == "code_patch");
    let contract_kind = node
        .output_contract
        .as_ref()
        .map(|contract| contract.kind.trim().to_ascii_lowercase())
        .unwrap_or_else(|| "structured_json".to_string());
    let citations_contract = contract_kind == "citations";
    let validation_profile = enforcement
        .validation_profile
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_else(|| {
            if validator_kind == crate::AutomationOutputValidatorKind::ReviewDecision {
                "review_gate".to_string()
            } else if validator_kind == crate::AutomationOutputValidatorKind::CodePatch {
                "code_change".to_string()
            } else if node.node_id == "collect_inputs" {
                "artifact_only".to_string()
            } else if code_patch_contract {
                "code_change".to_string()
            } else if legacy_web_research_expected
                || legacy_required_tools.iter().any(|tool| tool == "websearch")
            {
                "external_research".to_string()
            } else if citations_contract && prefers_mcp_servers {
                "artifact_only".to_string()
            } else if automation_node_is_research_finalize(node)
                || ((is_research_contract || citations_contract)
                    && matches!(
                        contract_kind.as_str(),
                        "brief" | "report_markdown" | "text_summary"
                    ))
            {
                "research_synthesis".to_string()
            } else if optional_workspace_reads {
                "artifact_only".to_string()
            } else if legacy_required_tools.iter().any(|tool| tool == "read")
                || is_research_contract
                || citations_contract
            {
                "local_research".to_string()
            } else {
                "artifact_only".to_string()
            }
        });
    enforcement.validation_profile = Some(validation_profile.clone());
    let is_standup_update = validation_profile == "standup_update";
    let is_local_research = validation_profile == "local_research";
    let is_external_research = validation_profile == "external_research";
    let is_research_synthesis = validation_profile == "research_synthesis";

    if enforcement.required_tools.is_empty() {
        enforcement.required_tools = legacy_required_tools.clone();
        if is_standup_update {
            if !enforcement.required_tools.iter().any(|tool| tool == "read") {
                enforcement.required_tools.push("read".to_string());
            }
        } else if is_local_research && !enforcement.required_tools.iter().any(|tool| tool == "glob")
        {
            enforcement.required_tools.push("glob".to_string());
        }
        if is_local_research && !enforcement.required_tools.iter().any(|tool| tool == "read") {
            enforcement.required_tools.push("read".to_string());
        }
        if (is_external_research || legacy_web_research_expected)
            && !enforcement
                .required_tools
                .iter()
                .any(|tool| tool == "websearch")
        {
            enforcement.required_tools.push("websearch".to_string());
        }
    }

    if !code_patch_contract
        && enforcement
            .required_tools
            .iter()
            .all(|tool| !matches!(tool.as_str(), "glob" | "read" | "write"))
    {
        let combined = automation_node_workspace_intent_text(node);
        let has_read_intent = ["read", "review", "inspect", "examine", "open"]
            .iter()
            .any(|needle| combined.to_ascii_lowercase().contains(needle));
        let has_write_intent = [
            "write",
            "update",
            "create",
            "initialize",
            "bootstrap",
            "merge",
            "append",
        ]
        .iter()
        .any(|needle| combined.to_ascii_lowercase().contains(needle));
        let has_discovery_intent = [
            "directory",
            "directories",
            "folder",
            "folders",
            "workspace",
            "missing",
        ]
        .iter()
        .any(|needle| combined.to_ascii_lowercase().contains(needle));
        let has_workspace_files = automation_text_has_workspace_tokens(&combined);
        if has_workspace_files
            && has_discovery_intent
            && !enforcement.required_tools.iter().any(|tool| tool == "glob")
        {
            enforcement.required_tools.push("glob".to_string());
        }
        if has_workspace_files
            && has_read_intent
            && !optional_workspace_reads
            && !enforcement.required_tools.iter().any(|tool| tool == "read")
        {
            enforcement.required_tools.push("read".to_string());
        }
        if has_workspace_files
            && has_write_intent
            && !enforcement
                .required_tools
                .iter()
                .any(|tool| tool == "write")
        {
            enforcement.required_tools.push("write".to_string());
        }
    }

    if code_patch_contract && !enforcement.required_tools.iter().any(|tool| tool == "read") {
        enforcement.required_tools.push("read".to_string());
    }

    if optional_workspace_reads {
        enforcement.required_tools.retain(|tool| tool != "read");
    }

    if enforcement.required_evidence.is_empty() {
        if is_local_research && !optional_workspace_reads
            || (is_research_synthesis
                && enforcement.required_tools.iter().any(|tool| tool == "read"))
        {
            enforcement
                .required_evidence
                .push("local_source_reads".to_string());
        }
        if is_external_research
            || legacy_web_research_expected
            || (is_research_synthesis
                && enforcement
                    .required_tools
                    .iter()
                    .any(|tool| tool == "websearch"))
            || enforcement
                .required_tools
                .iter()
                .any(|tool| tool == "websearch")
        {
            enforcement
                .required_evidence
                .push("external_sources".to_string());
        }
    }
    if code_patch_contract
        && !enforcement
            .required_evidence
            .iter()
            .any(|value| value == "local_source_reads")
    {
        enforcement
            .required_evidence
            .push("local_source_reads".to_string());
    }

    if enforcement.required_sections.is_empty() && is_research_contract {
        if is_external_research {
            enforcement.required_sections.push("citations".to_string());
        } else if is_research_synthesis && enforcement_requires_external_sources(&enforcement) {
            enforcement.required_sections.push("citations".to_string());
        }
    }

    let combined_intent_lowered = automation_node_workspace_intent_text(node).to_ascii_lowercase();
    let has_bootstrap_or_missing_intent = [
        "missing",
        "initialize",
        "bootstrap",
        "directory",
        "directories",
        "folder",
        "folders",
        "workspace",
        "if present",
        "if available",
    ]
    .iter()
    .any(|needle| combined_intent_lowered.contains(needle));

    let is_bootstrap = !optional_workspace_reads
        && !is_standup_update
        && !is_local_research
        && !is_external_research
        && !code_patch_contract
        && !is_research_contract
        && has_bootstrap_or_missing_intent;
    if enforcement.prewrite_gates.is_empty() && automation_node_required_output_path(node).is_some()
    {
        if is_standup_update {
            enforcement
                .prewrite_gates
                .push("concrete_reads".to_string());
        } else if optional_workspace_reads || is_bootstrap {
            enforcement
                .prewrite_gates
                .push("workspace_inspection".to_string());
        } else if is_local_research {
            enforcement
                .prewrite_gates
                .push("workspace_inspection".to_string());
            enforcement
                .prewrite_gates
                .push("concrete_reads".to_string());
        }
        if is_external_research && enforcement_requires_external_sources(&enforcement) {
            enforcement
                .prewrite_gates
                .push("successful_web_research".to_string());
        }
    }
    if node
        .metadata
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|m| m.get("triage_gate"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
        && automation_node_required_output_path(node).is_none()
    {
        if !enforcement.required_tools.iter().any(|t| t == "glob") {
            enforcement.required_tools.push("glob".to_string());
        }
        if !enforcement.required_tools.iter().any(|t| t == "read") {
            enforcement.required_tools.push("read".to_string());
        }
    }
    if code_patch_contract
        && automation_node_required_output_path(node).is_some()
        && !enforcement
            .prewrite_gates
            .iter()
            .any(|gate| gate == "workspace_inspection")
    {
        enforcement
            .prewrite_gates
            .push("workspace_inspection".to_string());
    }
    if code_patch_contract
        && automation_node_required_output_path(node).is_some()
        && !enforcement
            .prewrite_gates
            .iter()
            .any(|gate| gate == "concrete_reads")
    {
        enforcement
            .prewrite_gates
            .push("concrete_reads".to_string());
    }

    if enforcement.retry_on_missing.is_empty() {
        enforcement
            .retry_on_missing
            .extend(enforcement.required_evidence.iter().cloned());
        enforcement
            .retry_on_missing
            .extend(enforcement.required_sections.iter().cloned());
        enforcement
            .retry_on_missing
            .extend(enforcement.prewrite_gates.iter().cloned());
    }

    if enforcement.terminal_on.is_empty() && !enforcement.retry_on_missing.is_empty() {
        enforcement.terminal_on.extend([
            "tool_unavailable".to_string(),
            "repair_budget_exhausted".to_string(),
        ]);
    }

    if enforcement.repair_budget.is_none()
        && (!enforcement.retry_on_missing.is_empty() || !enforcement.required_tools.is_empty())
    {
        enforcement.repair_budget = Some(tandem_core::prewrite_repair_retry_max_attempts() as u32);
    }

    if enforcement.session_text_recovery.is_none() {
        enforcement.session_text_recovery = Some(
            if !enforcement.prewrite_gates.is_empty()
                || enforcement
                    .required_sections
                    .iter()
                    .any(|item| item == "files_reviewed")
            {
                "require_prewrite_satisfied".to_string()
            } else {
                "allow".to_string()
            },
        );
    }

    enforcement.required_tools = super::super::normalize_non_empty_list(enforcement.required_tools);
    enforcement.required_evidence =
        super::super::normalize_non_empty_list(enforcement.required_evidence);
    enforcement.required_sections =
        super::super::normalize_non_empty_list(enforcement.required_sections);
    enforcement.prewrite_gates = super::super::normalize_non_empty_list(enforcement.prewrite_gates);
    enforcement.retry_on_missing =
        super::super::normalize_non_empty_list(enforcement.retry_on_missing);
    enforcement.terminal_on = super::super::normalize_non_empty_list(enforcement.terminal_on);
    enforcement
}
