use serde_json::json;

use crate::{
    RetryBackoffPolicy, RetryBackoffStrategy, RetryDecisionInput, RetryPolicy, RetryTerminalMode,
    RETRY_BACKOFF_MAX_DELAY_MS,
};

#[test]
fn legacy_node_retry_policy_clamps_max_attempts() {
    let policy = RetryPolicy::from_node_retry_policy(
        Some(&json!({
            "max_attempts": 42
        })),
        3,
    );

    assert_eq!(policy.max_attempts, 10);
}

#[test]
fn retry_decision_records_schedule_and_policy_version() {
    let policy = RetryPolicy::from_node_retry_policy(
        Some(&json!({
            "max_attempts": 4,
            "retryable_failure_classes": ["provider_transient"],
            "backoff": {
                "strategy": "exponential",
                "initial_delay_ms": 2000,
                "max_delay_ms": 8000,
                "multiplier": 2.5
            }
        })),
        3,
    );

    let decision = policy.decide(RetryDecisionInput {
        failure_class: "provider-transient",
        reason: "provider stream connect timeout",
        attempt: 2,
        occurred_at_ms: 1_000,
        elapsed_ms: None,
    });

    assert_eq!(decision.decision, "retry_scheduled");
    assert_eq!(decision.failure_class, "provider_transient");
    assert_eq!(decision.attempt, 2);
    assert_eq!(decision.max_attempts, 4);
    assert_eq!(decision.backoff_ms, Some(5_000));
    assert_eq!(decision.next_retry_at_ms, Some(6_000));
    assert!(decision.policy_version_id.starts_with("retry-policy-v1-"));
}

#[test]
fn non_retryable_failure_is_terminal_before_attempt_cap() {
    let policy = RetryPolicy::from_node_retry_policy(
        Some(&json!({
            "max_attempts": 5,
            "retryable_failure_classes": ["provider_transient"]
        })),
        3,
    );

    let decision = policy.decide(RetryDecisionInput {
        failure_class: "provider_auth",
        reason: "authentication failed",
        attempt: 1,
        occurred_at_ms: 10,
        elapsed_ms: None,
    });

    assert_eq!(decision.decision, "not_retryable");
    assert!(decision.terminal);
    assert_eq!(decision.backoff_ms, None);
}

#[test]
fn terminal_behavior_and_manual_override_are_serializable() {
    let policy = RetryPolicy::from_node_retry_policy(
        Some(&json!({
            "max_attempts": 2,
            "terminal_behavior": {
                "mode": "dead_letter",
                "dead_letter": true
            },
            "manual_override": {
                "enabled": true,
                "requires_reason": false
            }
        })),
        3,
    );

    let serialized = serde_json::to_value(&policy).expect("serializable retry policy");
    assert_eq!(serialized["version"], 1);
    assert_eq!(serialized["terminal_behavior"]["mode"], "dead_letter");
    assert_eq!(serialized["manual_override"]["enabled"], true);
    assert_eq!(policy.terminal_behavior.mode, RetryTerminalMode::DeadLetter);
    assert!(policy.manual_override.enabled);
}

#[test]
fn provider_default_backoff_matches_existing_escalation() {
    let backoff = RetryBackoffPolicy::transient_provider_default();
    assert_eq!(backoff.strategy, RetryBackoffStrategy::Exponential);
    assert_eq!(backoff.delay_ms_for_attempt(1), Some(2_000));
    assert_eq!(backoff.delay_ms_for_attempt(2), Some(5_000));
    assert_eq!(backoff.delay_ms_for_attempt(3), Some(8_000));
}

#[test]
fn retry_backoff_clamps_overflow_to_hard_ceiling() {
    let backoff = RetryBackoffPolicy {
        strategy: RetryBackoffStrategy::Exponential,
        initial_delay_ms: 1_000,
        max_delay_ms: 0,
        multiplier: 1.0e308,
        jitter_ms: None,
    };

    assert_eq!(
        backoff.delay_ms_for_attempt(10),
        Some(RETRY_BACKOFF_MAX_DELAY_MS)
    );
}

#[test]
fn retry_backoff_treats_non_finite_multiplier_as_hard_ceiling() {
    let backoff = RetryBackoffPolicy {
        strategy: RetryBackoffStrategy::Exponential,
        initial_delay_ms: 1_000,
        max_delay_ms: 0,
        multiplier: f64::INFINITY,
        jitter_ms: None,
    };

    assert_eq!(
        backoff.delay_ms_for_attempt(2),
        Some(RETRY_BACKOFF_MAX_DELAY_MS)
    );
}
