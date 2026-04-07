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
    automation_node_legacy_builder(node)
        .and_then(|builder| builder.get("web_research_expected"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
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
            } else if automation_node_is_research_finalize(node)
                || ((is_research_contract || citations_contract)
                    && matches!(
                        contract_kind.as_str(),
                        "brief" | "report_markdown" | "text_summary"
                    ))
            {
                "research_synthesis".to_string()
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
        // standup_update: require read but not glob — participants that go straight to
        // reading relevant files satisfy the quality bar without a glob pass first.
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
    if code_patch_contract && !enforcement.required_tools.iter().any(|tool| tool == "read") {
        enforcement.required_tools.push("read".to_string());
    }

    if enforcement.required_evidence.is_empty() {
        if is_local_research
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

    if enforcement.prewrite_gates.is_empty() && automation_node_required_output_path(node).is_some()
    {
        if is_standup_update {
            // Only require a concrete read. workspace_inspection (glob) is not a hard gate
            // for standup participants — they get a repair signal instead of a hard block.
            enforcement
                .prewrite_gates
                .push("concrete_reads".to_string());
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
