use crate::{
    WorkflowBenchmarkObservation, WorkflowBenchmarkScenario, WorkflowBenchmarkSuite,
    WorkflowBenchmarkThresholds,
};

#[test]
fn workflow_benchmark_reports_graph_guided_savings_and_rates() {
    let suite = WorkflowBenchmarkSuite {
        suite_id: "workflow-graph-rollout".to_string(),
        scenarios: vec![WorkflowBenchmarkScenario {
            scenario_id: "research-publish".to_string(),
            baseline: observation(1_000, 7_000, 3_000, 10, 3, 8, 2, 4, 1, 5, 1, 1_000, 1_000),
            graph_guided: observation(700, 5_000, 2_000, 8, 1, 8, 0, 4, 0, 5, 3, 1_000, 600),
        }],
    };

    let report = suite.report(WorkflowBenchmarkThresholds::default());

    assert_eq!(report.suite_id, "workflow-graph-rollout");
    assert_eq!(report.scenario_count, 1);
    assert!(report.regressions.is_empty());
    assert_eq!(report.totals.token_savings, 3_000);
    assert_eq!(report.totals.token_savings_rate_bps, 3_000);
    assert_eq!(report.totals.runtime_latency_savings_ms, 300);
    assert_eq!(report.totals.runtime_latency_savings_rate_bps, 3_000);
    assert_eq!(report.totals.baseline_wrong_tool_call_rate_bps, 3_000);
    assert_eq!(report.totals.graph_guided_wrong_tool_call_rate_bps, 1_250);
    assert_eq!(report.totals.wrong_tool_call_rate_delta_bps, 1_750);
    assert_eq!(report.totals.baseline_policy_failure_rate_bps, 2_500);
    assert_eq!(report.totals.graph_guided_policy_failure_rate_bps, 0);
    assert_eq!(report.totals.baseline_preflight_success_rate_bps, 7_500);
    assert_eq!(
        report.totals.graph_guided_preflight_success_rate_bps,
        10_000
    );
    assert_eq!(report.totals.rerun_reuse_rate_delta_bps, 4_000);
    assert_eq!(report.totals.graph_guided_parallel_latency_savings_ms, 400);
    assert_eq!(
        report.totals.graph_guided_parallel_latency_savings_rate_bps,
        4_000
    );
}

#[test]
fn workflow_benchmark_tracks_graph_guided_regressions() {
    let suite = WorkflowBenchmarkSuite {
        suite_id: "workflow-graph-rollout".to_string(),
        scenarios: vec![WorkflowBenchmarkScenario {
            scenario_id: "risky-send".to_string(),
            baseline: observation(1_000, 7_000, 3_000, 10, 1, 8, 1, 4, 0, 5, 3, 1_000, 800),
            graph_guided: observation(1_300, 8_000, 4_000, 10, 4, 8, 3, 4, 1, 5, 1, 1_000, 900),
        }],
    };

    let report = suite.report(WorkflowBenchmarkThresholds {
        max_token_regression_bps: 500,
        max_latency_regression_bps: 500,
        max_wrong_tool_rate_regression_bps: 500,
        max_policy_failure_rate_regression_bps: 500,
        max_preflight_success_drop_bps: 500,
        max_rerun_reuse_drop_bps: 500,
    });

    let metrics = report
        .regressions
        .iter()
        .map(|regression| regression.metric.as_str())
        .collect::<Vec<_>>();
    assert!(metrics.contains(&"token_savings_rate_bps"));
    assert!(metrics.contains(&"runtime_latency_savings_rate_bps"));
    assert!(metrics.contains(&"wrong_tool_call_rate_delta_bps"));
    assert!(metrics.contains(&"policy_failure_rate_delta_bps"));
    assert!(metrics.contains(&"preflight_success_rate_delta_bps"));
    assert!(metrics.contains(&"rerun_reuse_rate_delta_bps"));
    assert!(report
        .regressions
        .iter()
        .any(|regression| regression.scenario_id.as_deref() == Some("risky-send")));
    assert!(report
        .regressions
        .iter()
        .any(|regression| regression.scenario_id.is_none()));
}

#[test]
fn workflow_benchmark_rejects_mismatched_completed_run_counts() {
    let mut baseline = observation(2_000, 14_000, 6_000, 20, 4, 8, 1, 4, 0, 10, 4, 2_000, 2_000);
    baseline.completed_runs = 2;
    let mut graph_guided = observation(700, 5_000, 2_000, 8, 1, 8, 0, 4, 0, 5, 3, 1_000, 600);
    graph_guided.completed_runs = 1;
    let suite = WorkflowBenchmarkSuite {
        suite_id: "workflow-graph-rollout".to_string(),
        scenarios: vec![WorkflowBenchmarkScenario {
            scenario_id: "partial-crash".to_string(),
            baseline,
            graph_guided,
        }],
    };

    let report = suite.report(WorkflowBenchmarkThresholds::default());

    assert_eq!(report.totals.token_savings, 0);
    assert_eq!(report.totals.token_savings_rate_bps, 0);
    assert_eq!(report.totals.runtime_latency_savings_ms, 0);
    assert!(report.regressions.iter().any(|regression| {
        regression.metric == "completed_runs"
            && regression.scenario_id.as_deref() == Some("partial-crash")
            && regression.baseline_value == 2
            && regression.graph_guided_value == 1
    }));
    assert!(report.regressions.iter().any(
        |regression| regression.metric == "completed_runs" && regression.scenario_id.is_none()
    ));
}

#[test]
fn workflow_benchmark_aggregates_multiple_scenarios() {
    let suite = WorkflowBenchmarkSuite {
        suite_id: "workflow-graph-rollout".to_string(),
        scenarios: vec![
            WorkflowBenchmarkScenario {
                scenario_id: "research".to_string(),
                baseline: observation(1_000, 5_000, 2_000, 10, 2, 5, 1, 2, 1, 4, 1, 900, 900),
                graph_guided: observation(800, 4_000, 1_500, 8, 1, 5, 0, 2, 0, 4, 2, 900, 700),
            },
            WorkflowBenchmarkScenario {
                scenario_id: "rerun".to_string(),
                baseline: observation(600, 3_000, 1_000, 5, 1, 3, 1, 1, 0, 6, 2, 600, 600),
                graph_guided: observation(300, 1_500, 700, 4, 0, 3, 0, 1, 0, 6, 5, 600, 300),
            },
        ],
    };

    let report = suite.report(WorkflowBenchmarkThresholds::default());

    assert_eq!(report.scenario_count, 2);
    assert_eq!(report.scenarios.len(), 2);
    assert_eq!(report.totals.baseline.total_tokens(), 11_000);
    assert_eq!(report.totals.graph_guided.total_tokens(), 7_700);
    assert_eq!(report.totals.token_savings, 3_300);
    assert_eq!(report.totals.baseline.completed_runs, 2);
    assert_eq!(report.totals.graph_guided.completed_runs, 2);
}

fn observation(
    latency_ms: u64,
    input_tokens: u64,
    output_tokens: u64,
    tool_calls: u64,
    wrong_tool_calls: u64,
    policy_checks: u64,
    policy_failures: u64,
    preflight_checks: u64,
    preflight_failures: u64,
    rerun_steps_considered: u64,
    rerun_steps_reused: u64,
    sequential_latency_ms: u64,
    scheduled_latency_ms: u64,
) -> WorkflowBenchmarkObservation {
    WorkflowBenchmarkObservation {
        completed_runs: 1,
        latency_ms,
        input_tokens,
        output_tokens,
        tool_calls,
        wrong_tool_calls,
        policy_checks,
        policy_failures,
        preflight_checks,
        preflight_failures,
        rerun_steps_considered,
        rerun_steps_reused,
        sequential_latency_ms,
        scheduled_latency_ms,
    }
}
