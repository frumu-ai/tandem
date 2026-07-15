// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn execution_error_blocker_category(detail: &str) -> &'static str {
    let lowered = detail.trim().to_ascii_lowercase();
    if lowered.contains("failed to reach provider")
        || lowered.contains("error sending request")
        || lowered.contains("request error")
        || lowered.contains("connect timeout")
        || lowered.contains("connection refused")
        || lowered.contains("dns error")
        || lowered.contains("timed out")
    {
        "provider_connect_timeout"
    } else if lowered.contains("provider returned error")
        || lowered.contains("provider stream chunk error")
        || lowered.contains("provider_server_error")
        || lowered.contains("server error")
    {
        "provider_server_error"
    } else if lowered.contains("authentication") || lowered.contains("unauthorized") {
        "provider_auth"
    } else {
        "execution_error"
    }
}

fn normalize_execution_error_detail(detail: &str) -> String {
    let trimmed = detail.trim();
    if trimmed.is_empty() {
        return "node execution failed before producing a final response".to_string();
    }
    if trimmed.eq_ignore_ascii_case("Provider returned error") {
        return "provider returned error before any node response was recorded".to_string();
    }
    trimmed.to_string()
}

fn execution_error_retry_floor(detail: &str, blocker_category: &str) -> Option<u32> {
    if matches!(
        blocker_category,
        "provider_connect_timeout" | "provider_server_error"
    ) {
        return Some(3);
    }
    let lowered = detail.trim().to_ascii_lowercase();
    if lowered.contains("required output") && lowered.contains("was not created") {
        return Some(3);
    }
    if lowered.contains("truncated source identity")
        || lowered.contains("read the full upstream artifact")
    {
        return Some(3);
    }
    if lowered.contains("connector source artifact only materialized the truncated preview rows")
        || lowered.contains("connector_truncated_preview_only")
    {
        return Some(3);
    }
    None
}

fn automation_node_execution_error_max_attempts(
    node: &crate::automation_v2::types::AutomationFlowNode,
    detail: &str,
    blocker_category: &str,
) -> u32 {
    let configured = crate::app::state::automation_node_max_attempts(node);
    execution_error_retry_floor(detail, blocker_category)
        .map(|floor| configured.max(floor))
        .unwrap_or(configured)
}

fn automation_node_max_attempts_for_recorded_output(
    node: &crate::automation_v2::types::AutomationFlowNode,
    output: Option<&Value>,
) -> u32 {
    let Some(output) = output else {
        return crate::app::state::automation_node_max_attempts(node);
    };
    let detail = output
        .get("blocked_reason")
        .or_else(|| output.get("summary"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let blocker_category = output
        .get("blocker_category")
        .and_then(Value::as_str)
        .unwrap_or_else(|| execution_error_blocker_category(detail));
    automation_node_execution_error_max_attempts(node, detail, blocker_category)
}

fn automation_node_recorded_attempts_exhausted(
    run: &crate::automation_v2::types::AutomationV2RunRecord,
    node_id: &str,
    node: &crate::automation_v2::types::AutomationFlowNode,
) -> bool {
    let attempts = run
        .checkpoint
        .node_attempts
        .get(node_id)
        .copied()
        .unwrap_or(0);
    let output = run.checkpoint.node_outputs.get(node_id);
    attempts >= automation_node_max_attempts_for_recorded_output(node, output)
}

fn retry_failure_class_from_blocker_category(blocker_category: &str) -> &'static str {
    match blocker_category {
        "provider_connect_timeout" | "provider_server_error" => "provider_transient",
        "provider_auth" => "provider_terminal",
        "tool_resolution_failed" => "tool_resolution",
        _ => "contract_miss",
    }
}

fn automation_node_retry_decision(
    node: &crate::automation_v2::types::AutomationFlowNode,
    detail: &str,
    attempts: u32,
    max_attempts: u32,
    failure_class: &str,
    occurred_at_ms: u64,
) -> tandem_automation::RetryDecision {
    let mut policy =
        tandem_automation::RetryPolicy::from_node_retry_policy(node.retry_policy.as_ref(), max_attempts);
    policy.max_attempts = max_attempts;
    if failure_class == "provider_transient"
        && matches!(
            policy.backoff.strategy,
            tandem_automation::RetryBackoffStrategy::None
        )
    {
        policy.backoff = tandem_automation::RetryBackoffPolicy::transient_provider_default();
    }
    policy.decide(tandem_automation::RetryDecisionInput {
        failure_class,
        reason: detail,
        attempt: attempts,
        occurred_at_ms,
        elapsed_ms: None,
    })
}

fn transient_provider_retry_backoff_ms(detail: &str, attempts: u32) -> Option<u64> {
    match execution_error_blocker_category(detail) {
        "provider_connect_timeout" | "provider_server_error" => {
            tandem_automation::RetryBackoffPolicy::transient_provider_default()
                .delay_ms_for_attempt(attempts)
        }
        _ => None,
    }
}

#[cfg(test)]
mod retry_policy_tests {
    use super::*;
    use serde_json::json;

    fn retry_node() -> crate::automation_v2::types::AutomationFlowNode {
        crate::automation_v2::types::AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "retry-node".to_string(),
            agent_id: "agent".to_string(),
            objective: "Retry".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: None,
            tool_policy: None,
            mcp_policy: None,
            retry_policy: None,
            timeout_ms: None,
            max_tool_calls: None,
            stage_kind: None,
            gate: None,
            wait: None,
            metadata: None,
        }
    }

    #[test]
    fn retry_decision_records_provider_backoff_metadata() {
        let node = retry_node();
        let decision = automation_node_retry_decision(
            &node,
            "provider stream connect timeout after 90000 ms",
            2,
            3,
            "provider_transient",
            1_000,
        );

        assert_eq!(decision.decision, "retry_scheduled");
        assert_eq!(decision.failure_class, "provider_transient");
        assert_eq!(decision.backoff_ms, Some(5_000));
        assert_eq!(decision.next_retry_at_ms, Some(6_000));
        assert!(!decision.terminal);
    }

    #[test]
    fn retry_decision_honors_configured_failure_classes() {
        let mut node = retry_node();
        node.retry_policy = Some(json!({
            "max_attempts": 5,
            "retryable_failure_classes": ["provider_transient"]
        }));

        let decision = automation_node_retry_decision(
            &node,
            "required output was not created",
            1,
            5,
            "contract_miss",
            10,
        );

        assert_eq!(decision.decision, "not_retryable");
        assert!(decision.terminal);
    }
}
