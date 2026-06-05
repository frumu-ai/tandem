/// Engine Executor for Eval-Runner Stub/Live Modes
///
/// Submits an `EvalTestCase` to the Tandem engine via
/// `AppState::create_automation_v2_run`, polls until the run reaches a terminal
/// status (`Completed | Blocked | Failed | Cancelled`), and extracts the resulting
/// `AutomationV2RunRecord` into an `EvalRunResult` for the eval-runner's metrics
/// aggregation.
///
/// Used by the eval-runner CLI in `--engine-mode stub` (with `ScriptedEvalProvider`
/// swapped into `state.providers`) and `--engine-mode live` (with the configured
/// production provider, requires API keys).
///
/// This module deliberately does NOT construct the `AppState` itself — callers wire
/// up `AppState` and provider injection. That keeps engine-executor testable in
/// isolation and avoids dragging the entire engine setup into unit tests of the
/// extraction logic.
use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::app::state::AppState;
use crate::automation_v2::types::{AutomationRunStatus, AutomationV2RunRecord};
use crate::eval::dataset::{ArtifactStatus, EvalTestCase};
use crate::eval::metrics::EvalRunResult;
use crate::eval::spec_mapper::{test_case_to_spec, test_case_to_stub_spec, EVAL_TRIGGER_TYPE};
use crate::failures::{classify_error_text, AIFailureMode};

/// How often to poll `get_automation_v2_run` for status updates.
pub const DEFAULT_POLL_INTERVAL_MS: u64 = 250;
/// Hard ceiling on a single test case's execution time.
pub const DEFAULT_MAX_DURATION_SECS: u64 = 300;

pub struct EngineExecutor {
    state: AppState,
    poll_interval: Duration,
    max_duration: Duration,
    use_stub_inline_artifacts: bool,
}

impl EngineExecutor {
    pub fn new(state: AppState) -> Self {
        Self {
            state,
            poll_interval: Duration::from_millis(DEFAULT_POLL_INTERVAL_MS),
            max_duration: Duration::from_secs(DEFAULT_MAX_DURATION_SECS),
            use_stub_inline_artifacts: false,
        }
    }

    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    pub fn with_max_duration(mut self, max_duration: Duration) -> Self {
        self.max_duration = max_duration;
        self
    }

    pub fn with_stub_inline_artifacts(mut self, enabled: bool) -> Self {
        self.use_stub_inline_artifacts = enabled;
        self
    }

    /// Submit a single eval test case, wait for it to reach a terminal status, and
    /// return the resulting `EvalRunResult`.
    pub async fn run_test_case(&self, case: &EvalTestCase) -> EvalRunResult {
        let started = Instant::now();
        let spec = if self.use_stub_inline_artifacts {
            test_case_to_stub_spec(case)
        } else {
            test_case_to_spec(case)
        };

        let initial_run = match self
            .state
            .create_automation_v2_run(&spec, EVAL_TRIGGER_TYPE)
            .await
        {
            Ok(run) => run,
            Err(err) => {
                return submission_error(case, started, format!("submit_failed: {err}"));
            }
        };

        let final_run = match self.poll_until_terminal(&initial_run.run_id).await {
            Ok(run) => run,
            Err(err) => return submission_error(case, started, err),
        };

        extract_eval_result(case, &final_run, started.elapsed())
    }

    async fn poll_until_terminal(&self, run_id: &str) -> Result<AutomationV2RunRecord, String> {
        let deadline = Instant::now() + self.max_duration;
        loop {
            if Instant::now() > deadline {
                let diagnostic = self
                    .state
                    .get_automation_v2_run(run_id)
                    .await
                    .map(|run| format!("; {}", run_timeout_diagnostic(&run)))
                    .unwrap_or_default();
                return Err(format!(
                    "eval timeout after {}s{}",
                    self.max_duration.as_secs(),
                    diagnostic
                ));
            }

            let Some(run) = self.state.get_automation_v2_run(run_id).await else {
                return Err(format!("run {} disappeared", run_id));
            };

            if is_terminal(&run.status) {
                return Ok(run);
            }

            tokio::time::sleep(self.poll_interval).await;
        }
    }
}

fn run_timeout_diagnostic(run: &AutomationV2RunRecord) -> String {
    let lifecycle_events = run
        .checkpoint
        .lifecycle_history
        .iter()
        .rev()
        .take(5)
        .map(|event| event.event.clone())
        .collect::<Vec<_>>();
    let last_failure = run
        .checkpoint
        .last_failure
        .as_ref()
        .map(|failure| failure.reason.clone());
    format!(
        "run_status={:?}, pending_nodes={}, completed_nodes={}, blocked_nodes={}, lifecycle_tail={:?}, last_failure={:?}, detail={:?}",
        run.status,
        run.checkpoint.pending_nodes.len(),
        run.checkpoint.completed_nodes.len(),
        run.checkpoint.blocked_nodes.len(),
        lifecycle_events,
        last_failure,
        run.detail
    )
}

/// Convert engine-native run state into an `EvalRunResult`.
pub fn extract_eval_result(
    case: &EvalTestCase,
    run: &AutomationV2RunRecord,
    elapsed: Duration,
) -> EvalRunResult {
    let repair_iterations = run
        .checkpoint
        .node_attempts
        .values()
        .max()
        .copied()
        .unwrap_or(0)
        .saturating_sub(1);

    let tokens_used = if run.total_tokens > 0 {
        run.total_tokens
    } else {
        run.prompt_tokens.saturating_add(run.completion_tokens)
    };

    let duration_ms = match (run.finished_at_ms, run.started_at_ms) {
        (Some(f), Some(s)) => f.saturating_sub(s),
        _ => elapsed.as_millis() as u64,
    };

    let (validators_passed, validators_failed) = extract_validator_outcomes(case, run);
    let artifact_status = map_artifact_status_with_expected_evidence(case, run, &validators_failed);
    let passed = artifact_status == case.expected_output.artifact_status
        && expected_validators_satisfied(
            case,
            run,
            artifact_status,
            &validators_passed,
            &validators_failed,
        );

    let (failure_mode, error_message) = if passed {
        (None, None)
    } else {
        let detail = run
            .detail
            .clone()
            .or_else(|| run.stop_reason.clone())
            .or_else(|| {
                run.checkpoint
                    .last_failure
                    .as_ref()
                    .map(|f| f.reason.clone())
            });
        let mode = detail
            .as_deref()
            .map(|text| classify_error_text(text, None))
            .or_else(|| Some(failure_mode_from_status(&run.status)));
        (mode, detail)
    };

    EvalRunResult {
        test_id: case.id.clone(),
        description: case.description.clone(),
        passed,
        artifact_status,
        repair_iterations,
        tokens_used,
        cost_usd: run.estimated_cost_usd,
        duration_ms,
        validators_passed,
        validators_failed,
        failure_mode,
        error_message,
        tags: case.tags.clone(),
    }
}

fn map_artifact_status_with_expected_evidence(
    case: &EvalTestCase,
    run: &AutomationV2RunRecord,
    validators_failed: &[String],
) -> ArtifactStatus {
    let status = map_artifact_status(&run.status);
    if status == ArtifactStatus::Failed
        && case.expected_output.artifact_status == ArtifactStatus::Blocked
        && blocked_validator_evidence_observed(case, run, validators_failed)
    {
        ArtifactStatus::Blocked
    } else {
        status
    }
}

fn blocked_validator_evidence_observed(
    case: &EvalTestCase,
    run: &AutomationV2RunRecord,
    validators_failed: &[String],
) -> bool {
    !case.expected_output.required_validators.is_empty()
        && case
            .expected_output
            .required_validators
            .iter()
            .all(|validator| {
                validators_failed.iter().any(|seen| seen == validator)
                    || validator_observed_in_outputs(run, validator)
            })
}

fn expected_validators_satisfied(
    case: &EvalTestCase,
    run: &AutomationV2RunRecord,
    artifact_status: ArtifactStatus,
    validators_passed: &[String],
    validators_failed: &[String],
) -> bool {
    if case.expected_output.required_validators.is_empty() {
        return true;
    }
    case.expected_output
        .required_validators
        .iter()
        .all(|validator| {
            if artifact_status == ArtifactStatus::Blocked {
                blocked_validator_evidence_observed(case, run, validators_failed)
            } else {
                validators_passed.iter().any(|seen| seen == validator)
                    && !validators_failed.iter().any(|seen| seen == validator)
            }
        })
}

fn validator_observed_in_outputs(run: &AutomationV2RunRecord, validator: &str) -> bool {
    run.checkpoint
        .node_outputs
        .values()
        .any(|output| output.to_string().contains(validator))
}

/// Remote Engine Executor - submits test cases to a remote Tandem engine via HTTP
///
/// Uses the same polling logic as EngineExecutor but via HTTP API calls instead of
/// direct AppState methods. Requires engine URL and authentication token.
pub struct RemoteEngineExecutor {
    engine_url: String,
    engine_token: String,
    client: reqwest::Client,
    poll_interval: Duration,
    max_duration: Duration,
}

impl RemoteEngineExecutor {
    pub fn new(engine_url: String, engine_token: String) -> Self {
        Self {
            engine_url,
            engine_token,
            client: reqwest::Client::new(),
            poll_interval: Duration::from_millis(DEFAULT_POLL_INTERVAL_MS),
            max_duration: Duration::from_secs(DEFAULT_MAX_DURATION_SECS),
        }
    }

    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    pub fn with_max_duration(mut self, max_duration: Duration) -> Self {
        self.max_duration = max_duration;
        self
    }

    /// Submit a single eval test case via HTTP, wait for it to reach a terminal status,
    /// and return the resulting `EvalRunResult`.
    pub async fn run_test_case(&self, case: &EvalTestCase) -> EvalRunResult {
        let started = Instant::now();
        let spec = test_case_to_spec(case);

        let initial_run = match self.submit_spec(&spec).await {
            Ok(run) => run,
            Err(err) => {
                return submission_error(case, started, format!("submit_failed: {err}"));
            }
        };

        let final_run = match self.poll_until_terminal(&initial_run.run_id).await {
            Ok(run) => run,
            Err(err) => return submission_error(case, started, err),
        };

        extract_eval_result(case, &final_run, started.elapsed())
    }

    async fn submit_spec(
        &self,
        spec: &crate::automation_v2::types::AutomationV2Spec,
    ) -> Result<AutomationV2RunRecord, String> {
        let url = format!("{}/api/automations/v2/runs/submit", self.engine_url);

        let response = self
            .client
            .post(&url)
            .header("X-Tandem-Token", self.engine_token.clone())
            .json(&spec)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(format!(
                "HTTP {}: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            ));
        }

        response
            .json::<AutomationV2RunRecord>()
            .await
            .map_err(|e| format!("Failed to parse response: {}", e))
    }

    async fn poll_until_terminal(&self, run_id: &str) -> Result<AutomationV2RunRecord, String> {
        let deadline = Instant::now() + self.max_duration;
        loop {
            if Instant::now() > deadline {
                return Err(format!(
                    "eval timeout after {}s",
                    self.max_duration.as_secs()
                ));
            }

            let run = match self.get_run(run_id).await {
                Ok(Some(run)) => run,
                Ok(None) => return Err(format!("run {} disappeared", run_id)),
                Err(e) => return Err(e),
            };

            if is_terminal(&run.status) {
                return Ok(run);
            }

            tokio::time::sleep(self.poll_interval).await;
        }
    }

    async fn get_run(&self, run_id: &str) -> Result<Option<AutomationV2RunRecord>, String> {
        let url = format!("{}/api/automations/v2/runs/{}", self.engine_url, run_id);

        let response = self
            .client
            .get(&url)
            .header("X-Tandem-Token", self.engine_token.clone())
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        match response.status() {
            reqwest::StatusCode::OK => {
                let run = response
                    .json::<AutomationV2RunRecord>()
                    .await
                    .map_err(|e| format!("Failed to parse response: {}", e))?;
                Ok(Some(run))
            }
            reqwest::StatusCode::NOT_FOUND => Ok(None),
            status => Err(format!(
                "HTTP {}: {}",
                status,
                response.text().await.unwrap_or_default()
            )),
        }
    }
}

fn is_terminal(status: &AutomationRunStatus) -> bool {
    matches!(
        status,
        AutomationRunStatus::Completed
            | AutomationRunStatus::Blocked
            | AutomationRunStatus::Failed
            | AutomationRunStatus::Cancelled
    )
}

fn map_artifact_status(status: &AutomationRunStatus) -> ArtifactStatus {
    match status {
        AutomationRunStatus::Completed => ArtifactStatus::Completed,
        AutomationRunStatus::Blocked => ArtifactStatus::Blocked,
        AutomationRunStatus::Failed | AutomationRunStatus::Cancelled => ArtifactStatus::Failed,
        // Non-terminal states only reach here if the caller bypasses `is_terminal`;
        // treat as failed for safety.
        _ => ArtifactStatus::Failed,
    }
}

/// Pull validator pass/fail signals out of the per-node `artifact_validation` blocks.
///
/// Strategy: the test case lists the validators it expected to run
/// (`automation_spec.validators`). We start by assuming all of them passed, then walk
/// every node's `artifact_validation.unmet_requirements` array (which the engine
/// populates with the names of validators/requirements that failed) and move any
/// match from the passed bucket to the failed bucket.
fn extract_validator_outcomes(
    case: &EvalTestCase,
    run: &AutomationV2RunRecord,
) -> (Vec<String>, Vec<String>) {
    let configured: Vec<String> = case.automation_spec.validators.clone();
    let mut unmet: HashSet<String> = HashSet::new();

    for output in run.checkpoint.node_outputs.values() {
        if let Some(arr) = output
            .pointer("/artifact_validation/unmet_requirements")
            .and_then(|v| v.as_array())
        {
            for item in arr {
                if let Some(s) = item.as_str() {
                    unmet.insert(s.to_string());
                }
            }
        }
        if let Some(arr) = output
            .pointer("/validator_summary/unmet_requirements")
            .and_then(|v| v.as_array())
        {
            for item in arr {
                if let Some(s) = item.as_str() {
                    unmet.insert(s.to_string());
                }
            }
        }
    }

    // Any configured validator whose name appears in any unmet_requirements list is
    // "failed". Everything else is "passed".
    let validators_failed: Vec<String> = configured
        .iter()
        .filter(|v| unmet.contains(v.as_str()))
        .cloned()
        .collect();
    let failed_set: HashSet<&str> = validators_failed.iter().map(String::as_str).collect();
    let validators_passed: Vec<String> = configured
        .iter()
        .filter(|v| !failed_set.contains(v.as_str()))
        .cloned()
        .collect();

    (validators_passed, validators_failed)
}

fn failure_mode_from_status(status: &AutomationRunStatus) -> AIFailureMode {
    match status {
        AutomationRunStatus::Blocked => AIFailureMode::ArtifactValidationFailed {
            validator_class: "unknown".to_string(),
        },
        AutomationRunStatus::Cancelled => AIFailureMode::SessionTimeout {
            timeout_ms: 0,
            actual_ms: 0,
        },
        _ => AIFailureMode::Unknown {
            error_message: format!("run ended in non-success status: {:?}", status),
        },
    }
}

fn submission_error(case: &EvalTestCase, started: Instant, error: String) -> EvalRunResult {
    EvalRunResult {
        test_id: case.id.clone(),
        description: case.description.clone(),
        passed: false,
        artifact_status: ArtifactStatus::Failed,
        repair_iterations: 0,
        tokens_used: 0,
        cost_usd: 0.0,
        duration_ms: started.elapsed().as_millis() as u64,
        validators_passed: Vec::new(),
        validators_failed: case.automation_spec.validators.clone(),
        failure_mode: Some(classify_error_text(&error, None)),
        error_message: Some(error),
        tags: case.tags.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation_v2::types::AutomationRunCheckpoint;
    use crate::eval::dataset::{
        ArtifactStatus, AutomationSpecTest, EvalExpectedOutput, EvalTestCase, MetricTolerance,
        TestNode,
    };
    use serde_json::json;
    use std::collections::HashMap;

    fn make_case_with_validators(validators: Vec<&str>) -> EvalTestCase {
        EvalTestCase {
            id: "test_001".to_string(),
            description: "exec test".to_string(),
            priority: 1,
            automation_spec: AutomationSpecTest {
                name: "exec".to_string(),
                nodes: vec![TestNode {
                    id: "n1".to_string(),
                    node_type: "research".to_string(),
                    objective: "do it".to_string(),
                    output_contract: String::new(),
                }],
                validators: validators.iter().map(|s| s.to_string()).collect(),
                config: HashMap::new(),
            },
            expected_output: EvalExpectedOutput {
                artifact_status: ArtifactStatus::Completed,
                required_validators: validators.iter().map(|s| s.to_string()).collect(),
                optional_validators: Vec::new(),
                unmet_requirements_acceptable: false,
                max_repair_iterations: Some(2),
                output_format: "json".to_string(),
                quality_indicators: Vec::new(),
            },
            enabled: true,
            tags: vec!["tag".to_string()],
            metric_tolerance: MetricTolerance::default(),
        }
    }

    fn make_record(
        status: AutomationRunStatus,
        node_outputs: HashMap<String, serde_json::Value>,
        node_attempts: HashMap<String, u32>,
    ) -> AutomationV2RunRecord {
        AutomationV2RunRecord {
            run_id: "run-xyz".to_string(),
            automation_id: "eval-test_001".to_string(),
            tenant_context: tandem_types::TenantContext::local_implicit(),
            trigger_type: EVAL_TRIGGER_TYPE.to_string(),
            status,
            created_at_ms: 1_000,
            updated_at_ms: 2_000,
            started_at_ms: Some(1_100),
            finished_at_ms: Some(1_800),
            active_session_ids: Vec::new(),
            latest_session_id: None,
            active_instance_ids: Vec::new(),
            checkpoint: AutomationRunCheckpoint {
                completed_nodes: vec!["n1".to_string()],
                pending_nodes: Vec::new(),
                node_outputs,
                node_attempts,
                node_attempt_verdicts: HashMap::new(),
                blocked_nodes: Vec::new(),
                awaiting_gate: None,
                gate_history: Vec::new(),
                lifecycle_history: Vec::new(),
                last_failure: None,
            },
            runtime_context: None,
            automation_snapshot: None,
            pause_reason: None,
            resume_reason: None,
            detail: None,
            stop_kind: None,
            stop_reason: None,
            prompt_tokens: 400,
            completion_tokens: 100,
            total_tokens: 500,
            estimated_cost_usd: 0.05,
            scheduler: None,
            trigger_reason: None,
            consumed_handoff_id: None,
            learning_summary: None,
            effective_execution_profile:
                crate::automation_v2::execution_profile::ExecutionProfile::default(),
            requested_execution_profile: None,
        }
    }

    #[test]
    fn is_terminal_recognizes_all_four_end_states() {
        assert!(is_terminal(&AutomationRunStatus::Completed));
        assert!(is_terminal(&AutomationRunStatus::Blocked));
        assert!(is_terminal(&AutomationRunStatus::Failed));
        assert!(is_terminal(&AutomationRunStatus::Cancelled));
        assert!(!is_terminal(&AutomationRunStatus::Queued));
        assert!(!is_terminal(&AutomationRunStatus::Running));
        assert!(!is_terminal(&AutomationRunStatus::Paused));
        assert!(!is_terminal(&AutomationRunStatus::Pausing));
        assert!(!is_terminal(&AutomationRunStatus::AwaitingApproval));
    }

    #[test]
    fn map_artifact_status_covers_all_terminal_variants() {
        assert!(matches!(
            map_artifact_status(&AutomationRunStatus::Completed),
            ArtifactStatus::Completed
        ));
        assert!(matches!(
            map_artifact_status(&AutomationRunStatus::Blocked),
            ArtifactStatus::Blocked
        ));
        assert!(matches!(
            map_artifact_status(&AutomationRunStatus::Failed),
            ArtifactStatus::Failed
        ));
        assert!(matches!(
            map_artifact_status(&AutomationRunStatus::Cancelled),
            ArtifactStatus::Failed
        ));
    }

    #[test]
    fn extract_eval_result_passes_on_completed_run() {
        let case = make_case_with_validators(vec!["contract"]);
        let mut outputs = HashMap::new();
        outputs.insert(
            "n1".to_string(),
            json!({"artifact_validation": {"effective_outcome": "passed"}}),
        );
        let mut attempts = HashMap::new();
        attempts.insert("n1".to_string(), 1);

        let run = make_record(AutomationRunStatus::Completed, outputs, attempts);
        let result = extract_eval_result(&case, &run, Duration::from_millis(700));

        assert!(result.passed);
        assert!(matches!(result.artifact_status, ArtifactStatus::Completed));
        assert_eq!(result.repair_iterations, 0); // 1 attempt -> 0 repairs
        assert_eq!(result.tokens_used, 500);
        assert!((result.cost_usd - 0.05).abs() < 1e-9);
        assert_eq!(result.validators_passed, vec!["contract".to_string()]);
        assert!(result.validators_failed.is_empty());
        assert!(result.failure_mode.is_none());
        assert!(result.error_message.is_none());
    }

    #[test]
    fn extract_eval_result_records_repair_iterations() {
        let case = make_case_with_validators(vec!["contract"]);
        let mut attempts = HashMap::new();
        attempts.insert("n1".to_string(), 3);

        let run = make_record(AutomationRunStatus::Completed, HashMap::new(), attempts);
        let result = extract_eval_result(&case, &run, Duration::from_millis(0));
        assert_eq!(result.repair_iterations, 2); // 3 attempts -> 2 repairs
    }

    #[test]
    fn extract_eval_result_fails_on_blocked_status() {
        let case = make_case_with_validators(vec!["contract"]);
        let mut outputs = HashMap::new();
        outputs.insert(
            "n1".to_string(),
            json!({
                "artifact_validation": {
                    "unmet_requirements": ["contract", "citations_present"]
                }
            }),
        );
        let mut run = make_record(AutomationRunStatus::Blocked, outputs, HashMap::new());
        run.detail = Some("artifact validation failed".to_string());

        let result = extract_eval_result(&case, &run, Duration::from_millis(0));

        assert!(!result.passed);
        assert!(matches!(result.artifact_status, ArtifactStatus::Blocked));
        assert_eq!(result.validators_failed, vec!["contract".to_string()]);
        // "citations_present" is in unmet, but the case only configured "contract" — so
        // citations_present isn't tracked in either bucket; that's intentional.
        assert!(result.validators_passed.is_empty());
        assert!(result.failure_mode.is_some());
        assert!(result.error_message.is_some());
    }

    #[test]
    fn extract_eval_result_passes_when_blocked_status_is_expected() {
        let mut case = make_case_with_validators(vec!["mcp_required_tool_failed"]);
        case.expected_output.artifact_status = ArtifactStatus::Blocked;
        let mut outputs = HashMap::new();
        outputs.insert(
            "n1".to_string(),
            json!({
                "artifact_validation": {
                    "unmet_requirements": ["mcp_required_tool_failed"]
                }
            }),
        );
        let mut run = make_record(AutomationRunStatus::Blocked, outputs, HashMap::new());
        run.detail = Some("required connector preflight call failed".to_string());

        let result = extract_eval_result(&case, &run, Duration::from_millis(0));

        assert!(result.passed);
        assert!(matches!(result.artifact_status, ArtifactStatus::Blocked));
        assert_eq!(
            result.validators_failed,
            vec!["mcp_required_tool_failed".to_string()]
        );
        assert!(result.failure_mode.is_none());
        assert!(result.error_message.is_none());
    }

    #[test]
    fn extract_eval_result_maps_failed_must_block_evidence_to_blocked() {
        let mut case = make_case_with_validators(vec!["mcp_required_tool_failed"]);
        case.expected_output.artifact_status = ArtifactStatus::Blocked;
        let mut outputs = HashMap::new();
        outputs.insert(
            "n1".to_string(),
            json!({
                "artifact_validation": {
                    "unmet_requirements": ["mcp_required_tool_failed"]
                }
            }),
        );
        let mut run = make_record(AutomationRunStatus::Failed, outputs, HashMap::new());
        run.detail = Some("automation run failed from node outcomes: n1".to_string());

        let result = extract_eval_result(&case, &run, Duration::from_millis(0));

        assert!(result.passed);
        assert!(matches!(result.artifact_status, ArtifactStatus::Blocked));
        assert_eq!(
            result.validators_failed,
            vec!["mcp_required_tool_failed".to_string()]
        );
    }

    #[test]
    fn extract_eval_result_fails_when_blocked_without_expected_validator_evidence() {
        let mut case = make_case_with_validators(vec!["mcp_required_tool_failed"]);
        case.expected_output.artifact_status = ArtifactStatus::Blocked;
        let mut run = make_record(AutomationRunStatus::Blocked, HashMap::new(), HashMap::new());
        run.detail = Some("blocked before required tool execution".to_string());

        let result = extract_eval_result(&case, &run, Duration::from_millis(0));

        assert!(!result.passed);
        assert!(matches!(result.artifact_status, ArtifactStatus::Blocked));
        assert!(result.validators_failed.is_empty());
        assert!(result.failure_mode.is_some());
        assert!(result.error_message.is_some());
    }

    #[test]
    fn extract_eval_result_reads_validator_summary_path_as_fallback() {
        let case = make_case_with_validators(vec!["contract"]);
        let mut outputs = HashMap::new();
        outputs.insert(
            "n1".to_string(),
            json!({"validator_summary": {"unmet_requirements": ["contract"]}}),
        );
        let run = make_record(AutomationRunStatus::Blocked, outputs, HashMap::new());

        let result = extract_eval_result(&case, &run, Duration::from_millis(0));
        assert_eq!(result.validators_failed, vec!["contract".to_string()]);
    }

    #[test]
    fn extract_eval_result_uses_engine_duration_when_present() {
        let case = make_case_with_validators(vec!["contract"]);
        let run = make_record(
            AutomationRunStatus::Completed,
            HashMap::new(),
            HashMap::new(),
        );
        // started_at_ms=1100, finished_at_ms=1800 -> 700ms engine duration
        let result = extract_eval_result(&case, &run, Duration::from_millis(99_999));
        assert_eq!(result.duration_ms, 700);
    }

    #[test]
    fn extract_eval_result_falls_back_to_wall_clock_when_engine_times_missing() {
        let case = make_case_with_validators(vec!["contract"]);
        let mut run = make_record(
            AutomationRunStatus::Completed,
            HashMap::new(),
            HashMap::new(),
        );
        run.started_at_ms = None;
        run.finished_at_ms = None;
        let result = extract_eval_result(&case, &run, Duration::from_millis(420));
        assert_eq!(result.duration_ms, 420);
    }

    #[test]
    fn extract_eval_result_prefers_total_tokens_over_sum() {
        let case = make_case_with_validators(vec!["contract"]);
        let mut run = make_record(
            AutomationRunStatus::Completed,
            HashMap::new(),
            HashMap::new(),
        );
        run.total_tokens = 1234;
        run.prompt_tokens = 100;
        run.completion_tokens = 50;
        let result = extract_eval_result(&case, &run, Duration::from_millis(0));
        assert_eq!(result.tokens_used, 1234);
    }

    #[test]
    fn extract_eval_result_sums_tokens_when_total_zero() {
        let case = make_case_with_validators(vec!["contract"]);
        let mut run = make_record(
            AutomationRunStatus::Completed,
            HashMap::new(),
            HashMap::new(),
        );
        run.total_tokens = 0;
        run.prompt_tokens = 100;
        run.completion_tokens = 50;
        let result = extract_eval_result(&case, &run, Duration::from_millis(0));
        assert_eq!(result.tokens_used, 150);
    }

    #[test]
    fn submission_error_marks_all_validators_failed() {
        let case = make_case_with_validators(vec!["contract", "citations"]);
        let result = submission_error(&case, Instant::now(), "oh no".to_string());
        assert!(!result.passed);
        assert!(matches!(result.artifact_status, ArtifactStatus::Failed));
        assert_eq!(result.validators_failed.len(), 2);
        assert!(result.validators_passed.is_empty());
        assert!(result.failure_mode.is_some());
        assert_eq!(result.error_message.as_deref(), Some("oh no"));
    }

    #[test]
    fn extract_eval_result_uses_last_failure_when_detail_missing() {
        use crate::automation_v2::types::AutomationFailureRecord;

        let case = make_case_with_validators(vec!["contract"]);
        let mut run = make_record(AutomationRunStatus::Failed, HashMap::new(), HashMap::new());
        run.detail = None;
        run.stop_reason = None;
        run.checkpoint.last_failure = Some(AutomationFailureRecord {
            node_id: "n1".to_string(),
            reason: "provider timeout after 3 retries".to_string(),
            failed_at_ms: 1_500,
        });

        let result = extract_eval_result(&case, &run, Duration::from_millis(0));
        assert!(!result.passed);
        assert!(result.failure_mode.is_some());
        assert_eq!(
            result.error_message.as_deref(),
            Some("provider timeout after 3 retries")
        );
    }
}
