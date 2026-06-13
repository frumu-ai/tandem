use crate::{
    WorkflowBenchmarkComparison, WorkflowBenchmarkObservation, WorkflowBenchmarkRegression,
    WorkflowBenchmarkReport, WorkflowBenchmarkScenarioReport, WorkflowBenchmarkSuite,
    WorkflowBenchmarkThresholds,
};

impl WorkflowBenchmarkSuite {
    pub fn report(&self, thresholds: WorkflowBenchmarkThresholds) -> WorkflowBenchmarkReport {
        let mut totals_baseline = WorkflowBenchmarkObservation::default();
        let mut totals_graph_guided = WorkflowBenchmarkObservation::default();
        let mut scenarios = Vec::new();
        let mut regressions = Vec::new();

        for scenario in &self.scenarios {
            add_observation(&mut totals_baseline, &scenario.baseline);
            add_observation(&mut totals_graph_guided, &scenario.graph_guided);

            let comparison = compare_observations(&scenario.baseline, &scenario.graph_guided);
            let scenario_regressions =
                detect_regressions(Some(&scenario.scenario_id), &comparison, thresholds);
            regressions.extend(scenario_regressions.clone());
            scenarios.push(WorkflowBenchmarkScenarioReport {
                scenario_id: scenario.scenario_id.clone(),
                comparison,
                regressions: scenario_regressions,
            });
        }

        let totals = compare_observations(&totals_baseline, &totals_graph_guided);
        regressions.extend(detect_regressions(None, &totals, thresholds));

        WorkflowBenchmarkReport {
            suite_id: self.suite_id.clone(),
            scenario_count: self.scenarios.len(),
            totals,
            scenarios,
            regressions,
        }
    }
}

fn compare_observations(
    baseline: &WorkflowBenchmarkObservation,
    graph_guided: &WorkflowBenchmarkObservation,
) -> WorkflowBenchmarkComparison {
    let comparable_run_counts = baseline.completed_runs == graph_guided.completed_runs;
    let baseline_tokens = baseline.total_tokens();
    let graph_guided_tokens = graph_guided.total_tokens();
    let baseline_wrong_tool_call_rate_bps =
        rate_bps(baseline.wrong_tool_calls, baseline.tool_calls);
    let graph_guided_wrong_tool_call_rate_bps =
        rate_bps(graph_guided.wrong_tool_calls, graph_guided.tool_calls);
    let baseline_policy_failure_rate_bps =
        rate_bps(baseline.policy_failures, baseline.policy_checks);
    let graph_guided_policy_failure_rate_bps =
        rate_bps(graph_guided.policy_failures, graph_guided.policy_checks);
    let baseline_preflight_success_rate_bps = success_rate_bps(
        baseline
            .preflight_checks
            .saturating_sub(baseline.preflight_failures),
        baseline.preflight_checks,
    );
    let graph_guided_preflight_success_rate_bps = success_rate_bps(
        graph_guided
            .preflight_checks
            .saturating_sub(graph_guided.preflight_failures),
        graph_guided.preflight_checks,
    );
    let baseline_rerun_reuse_rate_bps =
        rate_bps(baseline.rerun_steps_reused, baseline.rerun_steps_considered);
    let graph_guided_rerun_reuse_rate_bps = rate_bps(
        graph_guided.rerun_steps_reused,
        graph_guided.rerun_steps_considered,
    );
    let graph_guided_parallel_latency_savings_ms = comparable_savings(
        comparable_run_counts,
        graph_guided.sequential_latency_ms,
        graph_guided.scheduled_latency_ms,
    );

    WorkflowBenchmarkComparison {
        baseline: baseline.clone(),
        graph_guided: graph_guided.clone(),
        token_savings: comparable_savings(
            comparable_run_counts,
            baseline_tokens,
            graph_guided_tokens,
        ),
        token_savings_rate_bps: comparable_savings_rate_bps(
            comparable_run_counts,
            baseline_tokens,
            graph_guided_tokens,
        ),
        runtime_latency_savings_ms: comparable_savings(
            comparable_run_counts,
            baseline.latency_ms,
            graph_guided.latency_ms,
        ),
        runtime_latency_savings_rate_bps: comparable_savings_rate_bps(
            comparable_run_counts,
            baseline.latency_ms,
            graph_guided.latency_ms,
        ),
        baseline_wrong_tool_call_rate_bps,
        graph_guided_wrong_tool_call_rate_bps,
        wrong_tool_call_rate_delta_bps: i64::from(baseline_wrong_tool_call_rate_bps)
            - i64::from(graph_guided_wrong_tool_call_rate_bps),
        baseline_policy_failure_rate_bps,
        graph_guided_policy_failure_rate_bps,
        policy_failure_rate_delta_bps: i64::from(baseline_policy_failure_rate_bps)
            - i64::from(graph_guided_policy_failure_rate_bps),
        baseline_preflight_success_rate_bps,
        graph_guided_preflight_success_rate_bps,
        preflight_success_rate_delta_bps: i64::from(graph_guided_preflight_success_rate_bps)
            - i64::from(baseline_preflight_success_rate_bps),
        baseline_rerun_reuse_rate_bps,
        graph_guided_rerun_reuse_rate_bps,
        rerun_reuse_rate_delta_bps: i64::from(graph_guided_rerun_reuse_rate_bps)
            - i64::from(baseline_rerun_reuse_rate_bps),
        graph_guided_parallel_latency_savings_ms,
        graph_guided_parallel_latency_savings_rate_bps: comparable_savings_rate_bps(
            comparable_run_counts,
            graph_guided.sequential_latency_ms,
            graph_guided.scheduled_latency_ms,
        ),
    }
}

fn detect_regressions(
    scenario_id: Option<&str>,
    comparison: &WorkflowBenchmarkComparison,
    thresholds: WorkflowBenchmarkThresholds,
) -> Vec<WorkflowBenchmarkRegression> {
    let mut regressions = Vec::new();
    push_run_count_regression(&mut regressions, scenario_id, comparison);
    push_savings_regression(
        &mut regressions,
        scenario_id,
        "token_savings_rate_bps",
        comparison.token_savings_rate_bps,
        thresholds.max_token_regression_bps,
        "graph-guided runs used more tokens than baseline",
    );
    push_savings_regression(
        &mut regressions,
        scenario_id,
        "runtime_latency_savings_rate_bps",
        comparison.runtime_latency_savings_rate_bps,
        thresholds.max_latency_regression_bps,
        "graph-guided runs were slower than baseline",
    );
    push_rate_regression(
        &mut regressions,
        scenario_id,
        RateRegressionCheck {
            metric: "wrong_tool_call_rate_delta_bps",
            delta_bps: comparison.wrong_tool_call_rate_delta_bps,
            threshold_bps: thresholds.max_wrong_tool_rate_regression_bps,
            baseline_value_bps: comparison.baseline_wrong_tool_call_rate_bps,
            graph_guided_value_bps: comparison.graph_guided_wrong_tool_call_rate_bps,
            detail: "graph-guided runs made wrong tool calls more often than baseline",
        },
    );
    push_rate_regression(
        &mut regressions,
        scenario_id,
        RateRegressionCheck {
            metric: "policy_failure_rate_delta_bps",
            delta_bps: comparison.policy_failure_rate_delta_bps,
            threshold_bps: thresholds.max_policy_failure_rate_regression_bps,
            baseline_value_bps: comparison.baseline_policy_failure_rate_bps,
            graph_guided_value_bps: comparison.graph_guided_policy_failure_rate_bps,
            detail: "graph-guided runs hit policy failures more often than baseline",
        },
    );
    push_rate_regression(
        &mut regressions,
        scenario_id,
        RateRegressionCheck {
            metric: "preflight_success_rate_delta_bps",
            delta_bps: comparison.preflight_success_rate_delta_bps,
            threshold_bps: thresholds.max_preflight_success_drop_bps,
            baseline_value_bps: comparison.baseline_preflight_success_rate_bps,
            graph_guided_value_bps: comparison.graph_guided_preflight_success_rate_bps,
            detail: "graph-guided preflight success rate dropped below baseline",
        },
    );
    push_rate_regression(
        &mut regressions,
        scenario_id,
        RateRegressionCheck {
            metric: "rerun_reuse_rate_delta_bps",
            delta_bps: comparison.rerun_reuse_rate_delta_bps,
            threshold_bps: thresholds.max_rerun_reuse_drop_bps,
            baseline_value_bps: comparison.baseline_rerun_reuse_rate_bps,
            graph_guided_value_bps: comparison.graph_guided_rerun_reuse_rate_bps,
            detail: "graph-guided reruns reused fewer steps than baseline",
        },
    );
    regressions
}

fn push_run_count_regression(
    regressions: &mut Vec<WorkflowBenchmarkRegression>,
    scenario_id: Option<&str>,
    comparison: &WorkflowBenchmarkComparison,
) {
    if comparison.baseline.completed_runs == comparison.graph_guided.completed_runs {
        return;
    }
    regressions.push(WorkflowBenchmarkRegression {
        scenario_id: scenario_id.map(str::to_string),
        metric: "completed_runs".to_string(),
        baseline_value: comparison.baseline.completed_runs.min(i64::MAX as u64) as i64,
        graph_guided_value: comparison.graph_guided.completed_runs.min(i64::MAX as u64) as i64,
        threshold_bps: 0,
        detail: "baseline and graph-guided observations completed different run counts; savings metrics were suppressed".to_string(),
    });
}

fn push_savings_regression(
    regressions: &mut Vec<WorkflowBenchmarkRegression>,
    scenario_id: Option<&str>,
    metric: &str,
    savings_rate_bps: i64,
    threshold_bps: u32,
    detail: &str,
) {
    if savings_rate_bps >= -(i64::from(threshold_bps)) {
        return;
    }
    regressions.push(WorkflowBenchmarkRegression {
        scenario_id: scenario_id.map(str::to_string),
        metric: metric.to_string(),
        baseline_value: 0,
        graph_guided_value: savings_rate_bps,
        threshold_bps,
        detail: detail.to_string(),
    });
}

fn push_rate_regression(
    regressions: &mut Vec<WorkflowBenchmarkRegression>,
    scenario_id: Option<&str>,
    check: RateRegressionCheck<'_>,
) {
    if check.delta_bps >= -(i64::from(check.threshold_bps)) {
        return;
    }
    regressions.push(WorkflowBenchmarkRegression {
        scenario_id: scenario_id.map(str::to_string),
        metric: check.metric.to_string(),
        baseline_value: i64::from(check.baseline_value_bps),
        graph_guided_value: i64::from(check.graph_guided_value_bps),
        threshold_bps: check.threshold_bps,
        detail: check.detail.to_string(),
    });
}

struct RateRegressionCheck<'a> {
    metric: &'a str,
    delta_bps: i64,
    threshold_bps: u32,
    baseline_value_bps: u32,
    graph_guided_value_bps: u32,
    detail: &'a str,
}

fn add_observation(
    target: &mut WorkflowBenchmarkObservation,
    source: &WorkflowBenchmarkObservation,
) {
    target.completed_runs = target.completed_runs.saturating_add(source.completed_runs);
    target.latency_ms = target.latency_ms.saturating_add(source.latency_ms);
    target.input_tokens = target.input_tokens.saturating_add(source.input_tokens);
    target.output_tokens = target.output_tokens.saturating_add(source.output_tokens);
    target.tool_calls = target.tool_calls.saturating_add(source.tool_calls);
    target.wrong_tool_calls = target
        .wrong_tool_calls
        .saturating_add(source.wrong_tool_calls);
    target.policy_checks = target.policy_checks.saturating_add(source.policy_checks);
    target.policy_failures = target
        .policy_failures
        .saturating_add(source.policy_failures);
    target.preflight_checks = target
        .preflight_checks
        .saturating_add(source.preflight_checks);
    target.preflight_failures = target
        .preflight_failures
        .saturating_add(source.preflight_failures);
    target.rerun_steps_considered = target
        .rerun_steps_considered
        .saturating_add(source.rerun_steps_considered);
    target.rerun_steps_reused = target
        .rerun_steps_reused
        .saturating_add(source.rerun_steps_reused);
    target.sequential_latency_ms = target
        .sequential_latency_ms
        .saturating_add(source.sequential_latency_ms);
    target.scheduled_latency_ms = target
        .scheduled_latency_ms
        .saturating_add(source.scheduled_latency_ms);
}

fn success_rate_bps(successes: u64, denominator: u64) -> u32 {
    if denominator == 0 {
        return 0;
    }
    rate_bps(successes.min(denominator), denominator)
}

fn rate_bps(numerator: u64, denominator: u64) -> u32 {
    if denominator == 0 {
        return 0;
    }
    ((numerator.saturating_mul(10_000) + denominator / 2) / denominator) as u32
}

fn savings_rate_bps(baseline: u64, graph_guided: u64) -> i64 {
    if baseline == 0 {
        return 0;
    }
    (i128::from(baseline) - i128::from(graph_guided))
        .saturating_mul(10_000)
        .checked_div(i128::from(baseline))
        .unwrap_or(0)
        .clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

fn comparable_savings(comparable_run_counts: bool, baseline: u64, graph_guided: u64) -> i64 {
    if comparable_run_counts {
        delta_i64(baseline, graph_guided)
    } else {
        0
    }
}

fn comparable_savings_rate_bps(
    comparable_run_counts: bool,
    baseline: u64,
    graph_guided: u64,
) -> i64 {
    if comparable_run_counts {
        savings_rate_bps(baseline, graph_guided)
    } else {
        0
    }
}

fn delta_i64(baseline: u64, graph_guided: u64) -> i64 {
    (i128::from(baseline) - i128::from(graph_guided))
        .clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}
