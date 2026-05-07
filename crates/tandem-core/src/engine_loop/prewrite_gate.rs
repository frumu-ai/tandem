use tandem_types::{PrewriteCoverageMode, PrewriteRequirements};

#[derive(Debug, Clone, Copy)]
pub(super) struct PrewriteProgress {
    pub productive_write_tool_calls_total: usize,
    pub productive_workspace_inspection_total: usize,
    pub productive_concrete_read_total: usize,
    pub productive_web_research_total: usize,
    pub successful_web_research_total: usize,
    pub required_write_retry_count: usize,
    pub unmet_prewrite_repair_retry_count: usize,
    pub prewrite_gate_waived: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PrewriteGateDecision {
    pub prewrite_satisfied: bool,
    pub unmet_codes: Vec<&'static str>,
    pub gate_write: bool,
    pub allow_repair_tools: bool,
    pub force_write_only_retry: bool,
}

pub(super) fn evaluate_prewrite_gate(
    requested_write_required: bool,
    requirements: &PrewriteRequirements,
    progress: PrewriteProgress,
) -> PrewriteGateDecision {
    let prewrite_satisfied = prewrite_requirements_satisfied(
        requirements,
        progress.productive_workspace_inspection_total > 0,
        progress.productive_concrete_read_total > 0,
        progress.productive_web_research_total > 0,
        progress.successful_web_research_total > 0,
    );
    let unmet_codes = collect_unmet_prewrite_requirement_codes(
        requirements,
        progress.productive_workspace_inspection_total > 0,
        progress.productive_concrete_read_total > 0,
        progress.productive_web_research_total > 0,
        progress.successful_web_research_total > 0,
    );
    let gate_write = should_gate_write_until_prewrite_satisfied(
        requirements.repair_on_unmet_requirements,
        progress.productive_write_tool_calls_total,
        prewrite_satisfied,
    ) && !progress.prewrite_gate_waived;
    let allow_repair_tools = requested_write_required
        && progress.unmet_prewrite_repair_retry_count > 0
        && !prewrite_satisfied
        && !progress.prewrite_gate_waived;
    let force_write_only_retry = requested_write_required
        && progress.required_write_retry_count > 0
        && (progress.productive_write_tool_calls_total == 0 || prewrite_satisfied)
        && !gate_write
        && (prewrite_satisfied
            || !requirements.repair_on_unmet_requirements
            || progress.prewrite_gate_waived);

    PrewriteGateDecision {
        prewrite_satisfied,
        unmet_codes,
        gate_write,
        allow_repair_tools,
        force_write_only_retry,
    }
}

fn should_gate_write_until_prewrite_satisfied(
    repair_on_unmet_requirements: bool,
    productive_write_tool_calls_total: usize,
    prewrite_satisfied: bool,
) -> bool {
    repair_on_unmet_requirements && productive_write_tool_calls_total == 0 && !prewrite_satisfied
}

fn prewrite_requirements_satisfied(
    requirements: &PrewriteRequirements,
    workspace_inspection_satisfied: bool,
    concrete_read_satisfied: bool,
    web_research_satisfied: bool,
    successful_web_research_satisfied: bool,
) -> bool {
    (!requirements.workspace_inspection_required || workspace_inspection_satisfied)
        && (!requirements.web_research_required || web_research_satisfied)
        && (!requirements.concrete_read_required || concrete_read_satisfied)
        && (!requirements.successful_web_research_required || successful_web_research_satisfied)
}

fn describe_unmet_prewrite_requirements(
    requirements: &PrewriteRequirements,
    workspace_inspection_satisfied: bool,
    concrete_read_satisfied: bool,
    web_research_satisfied: bool,
    successful_web_research_satisfied: bool,
) -> Vec<&'static str> {
    let mut unmet = Vec::new();
    if requirements.workspace_inspection_required && !workspace_inspection_satisfied {
        unmet.push("inspect the workspace with `glob`/`read` before writing");
    }
    if requirements.concrete_read_required && !concrete_read_satisfied {
        unmet.push("use `read` on the concrete files you cite before finalizing");
    }
    if requirements.web_research_required && !web_research_satisfied {
        unmet.push("use `websearch` before finalizing the file");
    }
    if requirements.successful_web_research_required && !successful_web_research_satisfied {
        unmet.push("obtain at least one successful web research result instead of only timed-out or empty searches");
    }
    unmet
}

fn collect_unmet_prewrite_requirement_codes(
    requirements: &PrewriteRequirements,
    workspace_inspection_satisfied: bool,
    concrete_read_satisfied: bool,
    web_research_satisfied: bool,
    successful_web_research_satisfied: bool,
) -> Vec<&'static str> {
    let mut unmet = Vec::new();
    if requirements.workspace_inspection_required && !workspace_inspection_satisfied {
        unmet.push("workspace_inspection_required");
    }
    if requirements.concrete_read_required && !concrete_read_satisfied {
        unmet.push("concrete_read_required");
    }
    if requirements.web_research_required && !web_research_satisfied {
        unmet.push("web_research_required");
    }
    if requirements.successful_web_research_required && !successful_web_research_satisfied {
        unmet.push("successful_web_research_required");
    }
    if !matches!(requirements.coverage_mode, PrewriteCoverageMode::None)
        && (!workspace_inspection_satisfied || !concrete_read_satisfied)
    {
        unmet.push("coverage_mode");
    }
    unmet
}

pub(super) fn describe_unmet_prewrite_requirements_for_prompt(
    requirements: &PrewriteRequirements,
    workspace_inspection_satisfied: bool,
    concrete_read_satisfied: bool,
    web_research_satisfied: bool,
    successful_web_research_satisfied: bool,
) -> Vec<&'static str> {
    describe_unmet_prewrite_requirements(
        requirements,
        workspace_inspection_satisfied,
        concrete_read_satisfied,
        web_research_satisfied,
        successful_web_research_satisfied,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requirements() -> PrewriteRequirements {
        PrewriteRequirements {
            workspace_inspection_required: true,
            web_research_required: true,
            concrete_read_required: true,
            successful_web_research_required: true,
            repair_on_unmet_requirements: true,
            repair_budget: None,
            repair_exhaustion_behavior: None,
            coverage_mode: PrewriteCoverageMode::ResearchCorpus,
        }
    }

    fn progress(prewrite_gate_waived: bool) -> PrewriteProgress {
        PrewriteProgress {
            productive_write_tool_calls_total: 0,
            productive_workspace_inspection_total: 0,
            productive_concrete_read_total: 0,
            productive_web_research_total: 0,
            successful_web_research_total: 0,
            required_write_retry_count: 1,
            unmet_prewrite_repair_retry_count: 1,
            prewrite_gate_waived,
        }
    }

    #[test]
    fn gate_write_is_active_before_prewrite_is_satisfied() {
        let decision = evaluate_prewrite_gate(true, &requirements(), progress(false));
        assert!(decision.gate_write);
        assert!(decision.allow_repair_tools);
        assert!(!decision.force_write_only_retry);
        assert!(decision.unmet_codes.contains(&"coverage_mode"));
    }

    #[test]
    fn waiver_disables_repair_tools_and_gate_write() {
        let decision = evaluate_prewrite_gate(true, &requirements(), progress(true));
        assert!(!decision.gate_write);
        assert!(!decision.allow_repair_tools);
    }

    #[test]
    fn waiver_enables_force_write_only_retry_after_write_retry() {
        let decision = evaluate_prewrite_gate(true, &requirements(), progress(true));
        assert!(decision.force_write_only_retry);
    }

    #[test]
    fn prompt_description_reports_missing_requirements() {
        let unmet = describe_unmet_prewrite_requirements_for_prompt(
            &requirements(),
            false,
            false,
            false,
            false,
        );
        assert!(unmet
            .iter()
            .any(|value| value.contains("glob") || value.contains("read")));
        assert!(unmet.iter().any(|value| value.contains("websearch")));
    }
}
