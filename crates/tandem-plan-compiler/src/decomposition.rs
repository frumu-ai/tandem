// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

use crate::workflow_plan::{
    workflow_plan_mentions_connector_backed_sources, workflow_plan_mentions_web_research_tools,
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowDecompositionTier {
    Simple,
    Moderate,
    Complex,
    VeryComplex,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowDecompositionProfile {
    pub complexity_score: u8,
    pub tier: WorkflowDecompositionTier,
    pub recommended_min_leaf_tasks: u8,
    pub recommended_max_leaf_tasks: u8,
    pub recommended_phase_count: u8,
    pub requires_phased_dag: bool,
    #[serde(default)]
    pub signals: Vec<String>,
    #[serde(default)]
    pub guidance: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowStepDecompositionHints {
    pub phase_id: String,
    pub task_class: String,
    pub task_family: String,
    pub retry_class: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_step_id: Option<String>,
}

fn push_signal(signals: &mut Vec<String>, signal: impl Into<String>) {
    let signal = signal.into();
    if !signal.trim().is_empty() && !signals.iter().any(|existing| existing == &signal) {
        signals.push(signal);
    }
}

fn has_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn score_text_signals(lowered: &str, score: &mut u16, signals: &mut Vec<String>) {
    let word_count = lowered.split_whitespace().count();
    if word_count >= 20 {
        *score += 4;
        push_signal(signals, format!("prompt_word_count>=20:{word_count}"));
    }
    if word_count >= 40 {
        *score += 6;
        push_signal(signals, format!("prompt_word_count>=40:{word_count}"));
    }
    if word_count >= 80 {
        *score += 10;
        push_signal(signals, format!("prompt_word_count>=80:{word_count}"));
    }

    let conjunction_count = [
        " and ", " then ", " after ", " before ", " while ", " plus ", " also ",
    ]
    .iter()
    .map(|needle| lowered.matches(needle).count())
    .sum::<usize>();
    if conjunction_count >= 3 {
        *score += 4;
        push_signal(signals, format!("multi_clause_prompt={conjunction_count}"));
    }
}

fn classify_task_class(lowered: &str, output_contract_kind: Option<&str>) -> String {
    let output_contract_kind = output_contract_kind
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty());

    if output_contract_kind.as_deref() == Some("code_patch")
        || has_any(
            lowered,
            &[
                "code",
                "patch",
                "implement",
                "implementation",
                "refactor",
                "edit source",
                "bug fix",
                "repo fix",
                "fix the code",
            ],
        )
    {
        return "code_change".to_string();
    }

    if has_any(
        lowered,
        &[
            "send ",
            "send the ",
            "email",
            "publish",
            "post ",
            "deliver",
            "notify",
            "share ",
            "submit ",
        ],
    ) {
        return "delivery".to_string();
    }

    if has_any(
        lowered,
        &[
            "repair", "retry", "recover", "restore", "debug", "fix ", "failure", "blocked",
        ],
    ) {
        return "repair".to_string();
    }

    if has_any(
        lowered,
        &[
            "verify", "validate", "test", "review", "check", "qa", "approval", "gate",
        ],
    ) {
        return "validation".to_string();
    }

    if has_any(
        lowered,
        &[
            "research", "search", "collect", "inspect", "gather", "discover", "source",
        ],
    ) {
        return "research".to_string();
    }

    if output_contract_kind.as_deref().is_some_and(|kind| {
        matches!(
            kind,
            "brief" | "citations" | "report_markdown" | "text_summary"
        )
    }) || has_any(
        lowered,
        &[
            "synthes",
            "summar",
            "compare",
            "analy",
            "consolidat",
            "brief",
            "report",
            "recap",
            "digest",
        ],
    ) {
        return "synthesis".to_string();
    }

    if has_any(lowered, &["triage", "assess", "intake", "classify"]) {
        return "triage".to_string();
    }

    "general".to_string()
}

fn task_family_for_task_class(task_class: &str) -> String {
    match task_class {
        "code_change" => "code",
        "research" | "synthesis" => "research",
        "validation" => "verification",
        "delivery" | "repair" | "triage" => "ops",
        _ => "planning",
    }
    .to_string()
}

fn phase_label_for_profile(
    step_index: usize,
    step_count: usize,
    profile: &WorkflowDecompositionProfile,
) -> String {
    if profile.recommended_phase_count <= 1 || step_count <= 1 {
        return "phase_1_main".to_string();
    }

    let phase_count = usize::from(profile.recommended_phase_count.max(1));
    let bucket = ((step_index * phase_count) / step_count).min(phase_count.saturating_sub(1));
    let label = match phase_count {
        1 => "main",
        2 => match bucket {
            0 => "discover",
            _ => "deliver",
        },
        3 => match bucket {
            0 => "discover",
            1 => "synthesize",
            _ => "deliver",
        },
        4 => match bucket {
            0 => "discover",
            1 => "synthesize",
            2 => "validate",
            _ => "deliver",
        },
        _ => "phase",
    };
    format!("phase_{}_{}", bucket + 1, label)
}

fn retry_class_for_task_class(task_class: &str, output_contract_kind: Option<&str>) -> String {
    if output_contract_kind
        .map(|value| value.trim().eq_ignore_ascii_case("code_patch"))
        .unwrap_or(false)
        || task_class == "code_change"
    {
        return "inspect_patch_test_repair".to_string();
    }
    match task_class {
        "research" | "synthesis" => "artifact_write".to_string(),
        "validation" => "verification_loop".to_string(),
        "delivery" => "delivery_only".to_string(),
        "repair" => "repair_only".to_string(),
        "triage" => "triage_gate".to_string(),
        _ => "standard".to_string(),
    }
}

fn score_output_targets(target_count: usize, score: &mut u16, signals: &mut Vec<String>) {
    if target_count >= 1 {
        *score += 4;
        push_signal(signals, format!("output_targets={target_count}"));
    }
    if target_count >= 2 {
        *score += 6;
    }
    if target_count >= 3 {
        *score += 4;
    }
}

fn score_connector_signals(
    prompt: &str,
    allowed_mcp_servers: &[String],
    score: &mut u16,
    signals: &mut Vec<String>,
) {
    if !allowed_mcp_servers.is_empty() {
        *score += 6;
        push_signal(
            signals,
            format!("allowed_mcp_servers={}", allowed_mcp_servers.len()),
        );
        if allowed_mcp_servers.len() > 1 {
            *score += 3;
        }
    }
    if workflow_plan_mentions_connector_backed_sources(prompt) {
        *score += 6;
        push_signal(signals, "connector_backed_sources");
    }
    if workflow_plan_mentions_web_research_tools(prompt) {
        *score += 6;
        push_signal(signals, "web_research_tools");
    }
}

fn score_workflow_terms(lowered: &str, score: &mut u16, signals: &mut Vec<String>) {
    let categories = [
        (
            "research",
            has_any(
                lowered,
                &[
                    "research", "search", "collect", "inspect", "source", "evidence",
                ],
            ),
        ),
        (
            "synthesis",
            has_any(
                lowered,
                &[
                    "synthes", "summar", "compare", "analy", "brief", "report", "recap",
                ],
            ),
        ),
        (
            "delivery",
            has_any(
                lowered,
                &[
                    "send", "publish", "post ", "deliver", "notify", "share", "submit",
                ],
            ),
        ),
        (
            "validation",
            has_any(
                lowered,
                &[
                    "verify", "validate", "test", "review", "check", "approval", "gate",
                ],
            ),
        ),
        (
            "repair",
            has_any(
                lowered,
                &["repair", "retry", "fix", "recover", "debug", "blocked"],
            ),
        ),
        (
            "implementation",
            has_any(
                lowered,
                &["code", "patch", "implement", "refactor", "edit source"],
            ),
        ),
    ];

    let mut active = BTreeSet::new();
    for (name, matched) in categories {
        if matched {
            active.insert(name);
        }
    }
    if active.len() >= 2 {
        *score += 8;
        push_signal(signals, format!("task_categories={}", active.len()));
    }
    if active.len() >= 3 {
        *score += 8;
    }
    if active.contains("repair") && active.contains("validation") {
        *score += 4;
    }
    if active.contains("delivery") && active.contains("synthesis") {
        *score += 4;
    }
}

pub fn derive_workflow_decomposition_profile(
    prompt: &str,
    allowed_mcp_servers: &[String],
    explicit_output_targets: &[String],
    has_explicit_schedule: bool,
) -> WorkflowDecompositionProfile {
    let lowered = prompt.trim().to_ascii_lowercase();
    let mut score: u16 = 0;
    let mut signals = Vec::new();

    score_text_signals(&lowered, &mut score, &mut signals);
    score_output_targets(explicit_output_targets.len(), &mut score, &mut signals);
    score_connector_signals(prompt, allowed_mcp_servers, &mut score, &mut signals);
    score_workflow_terms(&lowered, &mut score, &mut signals);

    if has_explicit_schedule {
        score += 4;
        push_signal(&mut signals, "scheduled_workflow");
    }

    let complexity_score = score.min(100) as u8;
    let (tier, recommended_min_leaf_tasks, recommended_max_leaf_tasks, recommended_phase_count) =
        match complexity_score {
            0..=24 => (WorkflowDecompositionTier::Simple, 5, 10, 1),
            25..=44 => (WorkflowDecompositionTier::Moderate, 10, 20, 2),
            45..=69 => (WorkflowDecompositionTier::Complex, 20, 30, 3),
            _ => (WorkflowDecompositionTier::VeryComplex, 30, 50, 4),
        };
    let requires_phased_dag = recommended_phase_count > 1;
    let guidance = match tier {
        WorkflowDecompositionTier::Simple => vec![
            "Keep the plan at 5-10 leaf tasks.".to_string(),
            "A single phase is acceptable when the work has one evidence source and one output.".to_string(),
        ],
        WorkflowDecompositionTier::Moderate => vec![
            "Target 10-20 leaf tasks.".to_string(),
            "Split discovery from synthesis or delivery when they are separate responsibilities.".to_string(),
        ],
        WorkflowDecompositionTier::Complex => vec![
            "Target 20-30 leaf tasks.".to_string(),
            "Use explicit phases for discover, synthesize, validate, and deliver/repair leaves.".to_string(),
            "Do not combine multiple major outputs in one leaf task.".to_string(),
        ],
        WorkflowDecompositionTier::VeryComplex => vec![
            "Use 30-50 leaf tasks only as phased sub-DAGs.".to_string(),
            "Keep each leaf to one primary objective, one output contract, and one validation path.".to_string(),
            "Use parent_step_id and phase_id hints so the runtime can narrow retries instead of reopening the whole graph.".to_string(),
        ],
    };

    WorkflowDecompositionProfile {
        complexity_score,
        tier,
        recommended_min_leaf_tasks,
        recommended_max_leaf_tasks,
        recommended_phase_count,
        requires_phased_dag,
        signals,
        guidance,
    }
}

pub fn workflow_plan_decomposition_sections(profile: &WorkflowDecompositionProfile) -> String {
    let signals = if profile.signals.is_empty() {
        "none".to_string()
    } else {
        profile.signals.join(", ")
    };
    let tier = match &profile.tier {
        WorkflowDecompositionTier::Simple => "simple",
        WorkflowDecompositionTier::Moderate => "moderate",
        WorkflowDecompositionTier::Complex => "complex",
        WorkflowDecompositionTier::VeryComplex => "very_complex",
    };
    let guidance = if profile.guidance.is_empty() {
        "- keep each leaf task narrow".to_string()
    } else {
        profile
            .guidance
            .iter()
            .map(|line| format!("- {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!(
        concat!(
            "Decomposition profile:\n",
            "- complexity_score: {}\n",
            "- tier: {}\n",
            "- recommended_leaf_tasks: {}-{}\n",
            "- recommended_phase_count: {}\n",
            "- requires_phased_dag: {}\n",
            "- signals: {}\n",
            "Decomposition rules:\n",
            "{}\n",
            "- use phase_id, task_class, retry_class, and parent_step_id metadata when they help preserve the hierarchy\n",
            "- keep one primary objective, one primary output contract, and one validation path per leaf task\n",
        ),
        profile.complexity_score,
        tier,
        profile.recommended_min_leaf_tasks,
        profile.recommended_max_leaf_tasks,
        profile.recommended_phase_count,
        profile.requires_phased_dag,
        signals,
        guidance,
    )
}

pub fn workflow_plan_decomposition_observation(
    profile: &WorkflowDecompositionProfile,
    generated_step_count: usize,
) -> Value {
    let budget_status = if generated_step_count < profile.recommended_min_leaf_tasks as usize {
        "below_recommended"
    } else if generated_step_count > profile.recommended_max_leaf_tasks as usize {
        "above_recommended"
    } else {
        "within_recommended"
    };
    json!({
        "decomposition_profile": profile,
        "generated_step_count": generated_step_count,
        "budget_status": budget_status,
    })
}

pub fn derive_step_decomposition_hints(
    step_id: &str,
    kind: &str,
    objective: &str,
    output_contract_kind: Option<&str>,
    depends_on: &[String],
    step_index: usize,
    step_count: usize,
    profile: &WorkflowDecompositionProfile,
) -> WorkflowStepDecompositionHints {
    let lowered = format!("{step_id} {kind} {objective}").to_ascii_lowercase();
    let task_class = classify_task_class(&lowered, output_contract_kind);
    let phase_id = phase_label_for_profile(step_index, step_count, profile);
    let retry_class = retry_class_for_task_class(&task_class, output_contract_kind);
    let task_family = task_family_for_task_class(&task_class);
    let parent_step_id = depends_on
        .first()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    WorkflowStepDecompositionHints {
        phase_id,
        task_class,
        task_family,
        retry_class,
        parent_step_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decomposition_profile_scales_with_task_breadth_and_connectors() {
        let simple =
            derive_workflow_decomposition_profile("Write a quick summary", &[], &[], false);
        let complex = derive_workflow_decomposition_profile(
            "Research Reddit, compare findings, draft a report, and email it to the team",
            &["github".to_string(), "tandem-mcp".to_string()],
            &[
                "reports/summary.md".to_string(),
                "docs/standups/2026-04-15.md".to_string(),
            ],
            true,
        );

        assert_eq!(simple.tier, WorkflowDecompositionTier::Simple);
        assert!(simple.complexity_score < complex.complexity_score);
        assert!(complex.requires_phased_dag);
        assert!(complex.recommended_min_leaf_tasks >= 10);
        assert!(complex.recommended_phase_count >= 2);
        assert!(complex
            .signals
            .iter()
            .any(|signal| signal.contains("output_targets")));
        assert!(complex
            .signals
            .iter()
            .any(|signal| signal == "connector_backed_sources"));
    }

    #[test]
    fn step_hints_prefer_explicit_task_classes_and_dependency_parents() {
        let profile = WorkflowDecompositionProfile {
            complexity_score: 72,
            tier: WorkflowDecompositionTier::VeryComplex,
            recommended_min_leaf_tasks: 30,
            recommended_max_leaf_tasks: 50,
            recommended_phase_count: 4,
            requires_phased_dag: true,
            signals: vec!["manual_profile".to_string()],
            guidance: vec!["Split the work across explicit phases.".to_string()],
        };
        let hints = derive_step_decomposition_hints(
            "repair_code",
            "repair",
            "Patch the code and verify it with tests",
            Some("code_patch"),
            &["inspect".to_string()],
            2,
            4,
            &profile,
        );

        assert_eq!(hints.task_class, "code_change");
        assert_eq!(hints.phase_id, "phase_3_validate");
        assert_eq!(hints.task_family, "code");
        assert_eq!(hints.retry_class, "inspect_patch_test_repair");
        assert_eq!(hints.parent_step_id.as_deref(), Some("inspect"));
    }

    #[test]
    fn decomposition_sections_include_budget_guidance() {
        let profile = derive_workflow_decomposition_profile(
            "Research, compare, and deliver an email report",
            &["gmail".to_string()],
            &["reports/report.md".to_string()],
            true,
        );
        let sections = workflow_plan_decomposition_sections(&profile);

        assert!(sections.contains("Decomposition profile:"));
        assert!(sections.contains("recommended_leaf_tasks"));
        assert!(sections.contains("phase_id"));
        assert!(sections.contains("one primary objective"));
    }

    #[test]
    fn decomposition_profile_requires_phases_for_resume_job_search_prompt() {
        let prompt = "Analyze the local `RESUME.md` file and use it as the source of truth for skills, role targets, seniority, technologies, and geography preferences.

This workflow must stay simple and deterministic.

## Core rules

- Never edit, rewrite, rename, move, or delete `RESUME.md`
- Only read from `RESUME.md`
- If `resume_overview.md` does not exist, create it
- If `resume_overview.md` already exists, reuse it and do not regenerate it unless it is missing
- Use the `websearch` tool to find relevant job boards and recruitment sites in Europe where jobs are posted for the skills found in `RESUME.md`
- Save all results to a daily timestamped results file
- This workflow may run many times in one day, so it must append new findings to the same daily file instead of creating many separate files for the same date";

        let explicit_output_targets = crate::workflow_plan::infer_explicit_output_targets(prompt);
        let profile =
            derive_workflow_decomposition_profile(prompt, &[], &explicit_output_targets, true);

        assert!(profile.requires_phased_dag);
        assert!(profile.recommended_phase_count >= 2);
        assert!(profile.complexity_score >= 25);
        assert!(profile
            .signals
            .iter()
            .any(|signal| signal.contains("output_targets")));
        assert!(profile
            .signals
            .iter()
            .any(|signal| signal == "web_research_tools"));
    }
}
