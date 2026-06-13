use crate::{
    GraphQueryAudit, GraphQueryEnvelope, GraphQueryOutput, WorkflowApprovalPosture,
    WorkflowBlocker, WorkflowExecutionHintsQuery, WorkflowExecutionHintsReport, WorkflowGraph,
    WorkflowModelTier, WorkflowRiskTier, WorkflowRoutingMetrics, WorkflowStepDependencySummary,
    WorkflowStepExecutionHint, WorkflowStepFailureHistory, WorkflowToolRiskHint,
};

const DEFAULT_BUDGET_TOKENS: u64 = 4_000;

impl WorkflowGraph {
    pub fn workflow_execution_hints(
        &self,
        envelope: &GraphQueryEnvelope,
        query: WorkflowExecutionHintsQuery,
    ) -> GraphQueryOutput<WorkflowExecutionHintsReport> {
        let mut audit = GraphQueryAudit::default();
        let blockers = self.envelope_blockers(envelope);
        if !blockers.is_empty() {
            for blocker in &blockers {
                audit.deny(blocker.detail.clone());
            }
            return GraphQueryOutput::new(empty_report(blockers), audit);
        }

        let default_budget_tokens = query.default_budget_tokens.unwrap_or(DEFAULT_BUDGET_TOKENS);
        let mut step_hints = Vec::new();
        for (step_id, summary) in &self.step_dependencies {
            if !step_visible(step_id, summary, envelope, &mut audit) {
                continue;
            }
            step_hints.push(step_hint(
                step_id,
                summary,
                &query.tool_risk_hints,
                history_for_step(step_id, &query.failure_history),
                default_budget_tokens,
            ));
        }
        let metrics = routing_metrics(&step_hints, default_budget_tokens, &query.failure_history);

        GraphQueryOutput::new(
            WorkflowExecutionHintsReport {
                step_hints,
                metrics,
                blockers,
            },
            audit,
        )
    }
}

fn empty_report(blockers: Vec<WorkflowBlocker>) -> WorkflowExecutionHintsReport {
    WorkflowExecutionHintsReport {
        step_hints: Vec::new(),
        metrics: WorkflowRoutingMetrics::default(),
        blockers,
    }
}

fn step_visible(
    step_id: &str,
    summary: &WorkflowStepDependencySummary,
    envelope: &GraphQueryEnvelope,
    audit: &mut GraphQueryAudit,
) -> bool {
    let mut visible = true;
    for tool in &summary.required_tools {
        if !envelope.allows_tool(tool) {
            audit.deny(format!(
                "execution hints for step `{step_id}` reference tool `{tool}` outside the query envelope"
            ));
            visible = false;
        }
    }
    for tier in &summary.memory_tiers {
        if !envelope.allows_memory_tier(tier) {
            audit.deny(format!(
                "execution hints for step `{step_id}` reference memory tier `{tier}` outside the query envelope"
            ));
            visible = false;
        }
    }
    visible
}

fn step_hint(
    step_id: &str,
    summary: &WorkflowStepDependencySummary,
    tool_risk_hints: &[WorkflowToolRiskHint],
    failure_history: Option<&WorkflowStepFailureHistory>,
    default_budget_tokens: u64,
) -> WorkflowStepExecutionHint {
    let mut score = 0u32;
    let mut reasons = Vec::new();
    let mut approval_required = !summary.approval_gates.is_empty();
    let mut side_effects = false;

    for hint in matching_tool_hints(&summary.required_tools, tool_risk_hints) {
        let authority = hint.authority_level.to_ascii_lowercase();
        if matches!(authority.as_str(), "admin" | "elevated" | "write") {
            score += 2;
            reasons.push(format!(
                "tool `{}` has `{}` authority",
                hint.tool_name, hint.authority_level
            ));
        }
        if hint.side_effects {
            score += 2;
            side_effects = true;
            reasons.push(format!(
                "tool `{}` has external side effects",
                hint.tool_name
            ));
        }
        if hint.approval_required {
            score += 2;
            approval_required = true;
            reasons.push(format!("tool `{}` requires approval", hint.tool_name));
        }
        if hint
            .data_classes
            .iter()
            .any(|class| sensitive_data_class(class))
        {
            score += 2;
            reasons.push(format!(
                "tool `{}` handles sensitive data classes",
                hint.tool_name
            ));
        }
    }

    if !summary.policy_scopes.is_empty() {
        score += 1;
        reasons.push("step is governed by policy scopes".to_string());
    }
    if !summary.memory_tiers.is_empty() {
        score += 1;
        reasons.push("step reads or writes governed memory tiers".to_string());
    }
    if !summary.approval_gates.is_empty() {
        score += 2;
        reasons.push("step has workflow approval gates".to_string());
    }
    if let Some(history) = failure_history.filter(|history| history.failure_count > 0) {
        score += history.failure_count.min(3);
        reasons.push(format!(
            "step has {} historical failure(s)",
            history.failure_count
        ));
        if let Some(kind) = &history.last_failure_kind {
            reasons.push(format!("last failure kind was `{kind}`"));
        }
        if let Some(rate_bps) = history.recent_failure_rate_bps {
            reasons.push(format!("recent failure rate was {rate_bps} basis points"));
        }
    }

    let risk_tier = risk_tier(score);
    WorkflowStepExecutionHint {
        step_id: step_id.to_string(),
        model_tier: model_tier(&risk_tier),
        budget_tokens: budget_tokens(default_budget_tokens, &risk_tier),
        timeout_ms: timeout_ms(&risk_tier),
        max_retries: max_retries(&risk_tier, failure_history),
        approval_posture: approval_posture(&risk_tier, approval_required, side_effects),
        risk_tier,
        reasons,
        required_tools: summary.required_tools.clone(),
        policy_scopes: summary.policy_scopes.clone(),
    }
}

fn matching_tool_hints<'a>(
    required_tools: &[String],
    tool_risk_hints: &'a [WorkflowToolRiskHint],
) -> Vec<&'a WorkflowToolRiskHint> {
    tool_risk_hints
        .iter()
        .filter(|hint| required_tools.iter().any(|tool| tool == &hint.tool_name))
        .collect()
}

fn sensitive_data_class(data_class: &str) -> bool {
    matches!(
        data_class.to_ascii_lowercase().as_str(),
        "credential" | "financial" | "pii" | "secret" | "sensitive"
    )
}

fn risk_tier(score: u32) -> WorkflowRiskTier {
    match score {
        0..=1 => WorkflowRiskTier::Low,
        2..=4 => WorkflowRiskTier::Medium,
        _ => WorkflowRiskTier::High,
    }
}

fn model_tier(risk_tier: &WorkflowRiskTier) -> WorkflowModelTier {
    match risk_tier {
        WorkflowRiskTier::Low => WorkflowModelTier::Small,
        WorkflowRiskTier::Medium => WorkflowModelTier::Standard,
        WorkflowRiskTier::High => WorkflowModelTier::Large,
    }
}

fn budget_tokens(default_budget_tokens: u64, risk_tier: &WorkflowRiskTier) -> u64 {
    match risk_tier {
        WorkflowRiskTier::Low => default_budget_tokens,
        WorkflowRiskTier::Medium => default_budget_tokens.saturating_mul(3) / 2,
        WorkflowRiskTier::High => default_budget_tokens.saturating_mul(2),
    }
}

fn timeout_ms(risk_tier: &WorkflowRiskTier) -> u64 {
    match risk_tier {
        WorkflowRiskTier::Low => 45_000,
        WorkflowRiskTier::Medium => 90_000,
        WorkflowRiskTier::High => 120_000,
    }
}

fn max_retries(
    risk_tier: &WorkflowRiskTier,
    failure_history: Option<&WorkflowStepFailureHistory>,
) -> u8 {
    if failure_history.is_some_and(|history| history.failure_count >= 3) {
        return 1;
    }
    match risk_tier {
        WorkflowRiskTier::Low => 3,
        WorkflowRiskTier::Medium => 2,
        WorkflowRiskTier::High => 1,
    }
}

fn approval_posture(
    risk_tier: &WorkflowRiskTier,
    approval_required: bool,
    side_effects: bool,
) -> WorkflowApprovalPosture {
    if matches!(risk_tier, WorkflowRiskTier::High) && (approval_required || side_effects) {
        WorkflowApprovalPosture::StrongReview
    } else if approval_required {
        WorkflowApprovalPosture::HumanReview
    } else {
        WorkflowApprovalPosture::None
    }
}

fn history_for_step<'a>(
    step_id: &str,
    history: &'a [WorkflowStepFailureHistory],
) -> Option<&'a WorkflowStepFailureHistory> {
    history.iter().find(|history| history.step_id == step_id)
}

fn routing_metrics(
    step_hints: &[WorkflowStepExecutionHint],
    default_budget_tokens: u64,
    failure_history: &[WorkflowStepFailureHistory],
) -> WorkflowRoutingMetrics {
    let baseline_budget_tokens = default_budget_tokens.saturating_mul(step_hints.len() as u64);
    let recommended_budget_tokens = step_hints
        .iter()
        .map(|hint| hint.budget_tokens)
        .sum::<u64>();
    WorkflowRoutingMetrics {
        high_risk_steps: step_hints
            .iter()
            .filter(|hint| hint.risk_tier == WorkflowRiskTier::High)
            .count(),
        historical_failure_steps: step_hints
            .iter()
            .filter(|hint| {
                hint.reasons
                    .iter()
                    .any(|reason| reason.contains("historical failure"))
            })
            .count(),
        approval_required_steps: step_hints
            .iter()
            .filter(|hint| hint.approval_posture != WorkflowApprovalPosture::None)
            .count(),
        baseline_budget_tokens,
        recommended_budget_tokens,
        budget_delta_tokens: budget_delta_tokens(recommended_budget_tokens, baseline_budget_tokens),
        historical_failure_rate_bps: average_failure_rate_bps(
            step_hints,
            failure_history,
            FailureRateMode::Historical,
        ),
        graph_guided_failure_rate_bps: average_failure_rate_bps(
            step_hints,
            failure_history,
            FailureRateMode::GraphGuided,
        ),
    }
}

#[derive(Clone, Copy)]
enum FailureRateMode {
    Historical,
    GraphGuided,
}

fn budget_delta_tokens(recommended_budget_tokens: u64, baseline_budget_tokens: u64) -> i64 {
    if recommended_budget_tokens >= baseline_budget_tokens {
        (recommended_budget_tokens - baseline_budget_tokens).min(i64::MAX as u64) as i64
    } else {
        -((baseline_budget_tokens - recommended_budget_tokens).min(i64::MAX as u64) as i64)
    }
}

fn average_failure_rate_bps(
    step_hints: &[WorkflowStepExecutionHint],
    failure_history: &[WorkflowStepFailureHistory],
    mode: FailureRateMode,
) -> Option<u32> {
    let mut total = 0u64;
    let mut count = 0u64;
    for hint in step_hints {
        let Some(history) = history_for_step(&hint.step_id, failure_history) else {
            continue;
        };
        let Some(rate_bps) = history.recent_failure_rate_bps else {
            continue;
        };
        let rate_bps = match mode {
            FailureRateMode::Historical => rate_bps,
            FailureRateMode::GraphGuided => graph_guided_failure_rate_bps(rate_bps, hint),
        };
        total += u64::from(rate_bps);
        count += 1;
    }
    (count > 0).then(|| ((total + count / 2) / count) as u32)
}

fn graph_guided_failure_rate_bps(rate_bps: u32, hint: &WorkflowStepExecutionHint) -> u32 {
    let model_multiplier_bps = match hint.model_tier {
        WorkflowModelTier::Small => 10_000u64,
        WorkflowModelTier::Standard => 9_000,
        WorkflowModelTier::Large => 8_000,
    };
    let approval_multiplier_bps = match hint.approval_posture {
        WorkflowApprovalPosture::None => 10_000u64,
        WorkflowApprovalPosture::HumanReview => 9_000,
        WorkflowApprovalPosture::StrongReview => 7_500,
    };
    let retry_multiplier_bps = match hint.max_retries {
        0 => 10_000u64,
        1 => 9_000,
        _ => 8_500,
    };

    (u64::from(rate_bps)
        .saturating_mul(model_multiplier_bps)
        .saturating_mul(approval_multiplier_bps)
        .saturating_mul(retry_multiplier_bps)
        / 1_000_000_000_000u64) as u32
}
