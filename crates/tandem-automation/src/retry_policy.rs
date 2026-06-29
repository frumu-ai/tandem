use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const RETRY_POLICY_SCHEMA_VERSION: u32 = 1;
pub const RETRY_POLICY_MAX_ATTEMPTS_CAP: u32 = 10;

fn retry_policy_schema_version() -> u32 {
    RETRY_POLICY_SCHEMA_VERSION
}

fn default_retry_max_attempts() -> u32 {
    3
}

fn default_retryable_failure_classes() -> Vec<String> {
    [
        "artifact_contract_unmet",
        "contract_miss",
        "execution_error",
        "missing_config",
        "provider_terminal",
        "provider_transient",
        "tool_resolution",
        "tool_resolution_failed",
        "validation_error",
        "wait_wakeup",
        "webhook_delivery",
        "outbox_send",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RetryBackoffStrategy {
    None,
    Fixed,
    Exponential,
}

impl Default for RetryBackoffStrategy {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetryBackoffPolicy {
    #[serde(default)]
    pub strategy: RetryBackoffStrategy,
    #[serde(default)]
    pub initial_delay_ms: u64,
    #[serde(default)]
    pub max_delay_ms: u64,
    #[serde(default = "default_backoff_multiplier")]
    pub multiplier: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jitter_ms: Option<u64>,
}

fn default_backoff_multiplier() -> f64 {
    2.0
}

impl Default for RetryBackoffPolicy {
    fn default() -> Self {
        Self {
            strategy: RetryBackoffStrategy::None,
            initial_delay_ms: 0,
            max_delay_ms: 0,
            multiplier: default_backoff_multiplier(),
            jitter_ms: None,
        }
    }
}

impl RetryBackoffPolicy {
    pub fn transient_provider_default() -> Self {
        Self {
            strategy: RetryBackoffStrategy::Exponential,
            initial_delay_ms: 2_000,
            max_delay_ms: 8_000,
            multiplier: 2.5,
            jitter_ms: None,
        }
    }

    pub fn delay_ms_for_attempt(&self, attempt: u32) -> Option<u64> {
        if matches!(self.strategy, RetryBackoffStrategy::None) || self.initial_delay_ms == 0 {
            return None;
        }
        let raw = match self.strategy {
            RetryBackoffStrategy::None => return None,
            RetryBackoffStrategy::Fixed => self.initial_delay_ms as f64,
            RetryBackoffStrategy::Exponential => {
                let exponent = attempt.saturating_sub(1) as i32;
                self.initial_delay_ms as f64 * self.multiplier.max(1.0).powi(exponent)
            }
        };
        let capped = if self.max_delay_ms > 0 {
            raw.min(self.max_delay_ms as f64)
        } else {
            raw
        };
        Some(capped.round().max(0.0) as u64)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetryTerminalMode {
    FailRun,
    PauseForReview,
    DeadLetter,
}

impl Default for RetryTerminalMode {
    fn default() -> Self {
        Self::FailRun
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetryTerminalBehavior {
    #[serde(default)]
    pub mode: RetryTerminalMode,
    #[serde(default)]
    pub dead_letter: bool,
}

impl Default for RetryTerminalBehavior {
    fn default() -> Self {
        Self {
            mode: RetryTerminalMode::FailRun,
            dead_letter: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetryManualOverridePolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_manual_override_requires_reason")]
    pub requires_reason: bool,
}

fn default_manual_override_requires_reason() -> bool {
    true
}

impl Default for RetryManualOverridePolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            requires_reason: true,
        }
    }
}

/// Canonical retry policy shared by automation nodes, webhooks, waits, outbox
/// sends, and external effects.
///
/// Existing automation definitions still store `retry_policy` as JSON. Use
/// `RetryPolicy::from_node_retry_policy` to normalize that JSON into this
/// schema without requiring a definition migration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetryPolicy {
    #[serde(default = "retry_policy_schema_version")]
    pub version: u32,
    #[serde(default = "default_retry_max_attempts")]
    pub max_attempts: u32,
    #[serde(default = "default_retryable_failure_classes")]
    pub retryable_failure_classes: Vec<String>,
    #[serde(default)]
    pub backoff: RetryBackoffPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_elapsed_ms: Option<u64>,
    #[serde(default)]
    pub terminal_behavior: RetryTerminalBehavior,
    #[serde(default)]
    pub manual_override: RetryManualOverridePolicy,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self::default_with_max_attempts(default_retry_max_attempts())
    }
}

impl RetryPolicy {
    pub fn default_with_max_attempts(max_attempts: u32) -> Self {
        Self {
            version: RETRY_POLICY_SCHEMA_VERSION,
            max_attempts: clamp_max_attempts(max_attempts),
            retryable_failure_classes: default_retryable_failure_classes(),
            backoff: RetryBackoffPolicy::default(),
            max_elapsed_ms: None,
            terminal_behavior: RetryTerminalBehavior::default(),
            manual_override: RetryManualOverridePolicy::default(),
        }
    }

    pub fn from_node_retry_policy(value: Option<&Value>, default_max_attempts: u32) -> Self {
        let mut policy = Self::default_with_max_attempts(default_max_attempts);
        let Some(value) = value else {
            return policy;
        };

        if let Some(version) = number_field(value, &["version"]) {
            policy.version = version.max(1) as u32;
        }
        if let Some(max_attempts) = number_field(value, &["max_attempts", "maxAttempts"]) {
            policy.max_attempts = clamp_max_attempts(max_attempts as u32);
        }
        if let Some(classes) = string_array_field(
            value,
            &[
                "retryable_failure_classes",
                "retryableFailureClasses",
                "retry_on",
                "retryOn",
            ],
        ) {
            policy.retryable_failure_classes = classes;
        }
        if let Some(max_elapsed_ms) = number_field(
            value,
            &["max_elapsed_ms", "maxElapsedMs", "max_elapsed_time_ms"],
        ) {
            policy.max_elapsed_ms = Some(max_elapsed_ms);
        }

        policy.backoff = parse_backoff_policy(value, policy.backoff.clone());
        policy.terminal_behavior = parse_terminal_behavior(value, policy.terminal_behavior);
        policy.manual_override = parse_manual_override(value, policy.manual_override);
        policy
    }

    pub fn policy_version_id(&self) -> String {
        let serialized = serde_json::to_string(self).unwrap_or_default();
        format!(
            "retry-policy-v{}-{:016x}",
            self.version,
            stable_hash(serialized.as_bytes())
        )
    }

    pub fn is_failure_class_retryable(&self, failure_class: &str) -> bool {
        if self.retryable_failure_classes.is_empty() {
            return true;
        }
        let failure_class = normalize_class(failure_class);
        self.retryable_failure_classes
            .iter()
            .map(|value| normalize_class(value))
            .any(|value| value == "*" || value == failure_class)
    }

    pub fn decide(&self, input: RetryDecisionInput<'_>) -> RetryDecision {
        let attempt = input.attempt.max(1);
        let max_attempts = self.max_attempts.max(1);
        let retryable = self.is_failure_class_retryable(input.failure_class);
        let attempts_exhausted = attempt >= max_attempts;
        let elapsed_exhausted = input
            .elapsed_ms
            .zip(self.max_elapsed_ms)
            .is_some_and(|(elapsed, max)| elapsed >= max);
        let terminal = !retryable || attempts_exhausted || elapsed_exhausted;
        let backoff_ms = if terminal {
            None
        } else {
            self.backoff.delay_ms_for_attempt(attempt)
        };
        let next_retry_at_ms = backoff_ms.map(|delay| input.occurred_at_ms.saturating_add(delay));
        let decision = if !retryable {
            "not_retryable"
        } else if attempts_exhausted {
            "attempts_exhausted"
        } else if elapsed_exhausted {
            "elapsed_time_exhausted"
        } else if backoff_ms.is_some() {
            "retry_scheduled"
        } else {
            "retry_allowed"
        };

        RetryDecision {
            version: self.version,
            policy_version_id: self.policy_version_id(),
            decision: decision.to_string(),
            failure_class: normalize_class(input.failure_class),
            reason: input.reason.to_string(),
            attempt,
            max_attempts,
            retryable,
            terminal,
            next_retry_at_ms,
            backoff_ms,
            terminal_behavior: self.terminal_behavior.clone(),
            manual_override_allowed: self.manual_override.enabled,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RetryDecisionInput<'a> {
    pub failure_class: &'a str,
    pub reason: &'a str,
    pub attempt: u32,
    pub occurred_at_ms: u64,
    pub elapsed_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetryDecision {
    pub version: u32,
    pub policy_version_id: String,
    pub decision: String,
    pub failure_class: String,
    pub reason: String,
    pub attempt: u32,
    pub max_attempts: u32,
    pub retryable: bool,
    pub terminal: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_retry_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backoff_ms: Option<u64>,
    pub terminal_behavior: RetryTerminalBehavior,
    pub manual_override_allowed: bool,
}

fn parse_backoff_policy(value: &Value, default: RetryBackoffPolicy) -> RetryBackoffPolicy {
    let source = value.get("backoff").unwrap_or(value);
    let mut backoff = default;
    if let Some(strategy) = string_field(source, &["strategy", "backoff_strategy"]) {
        backoff.strategy = match normalize_class(&strategy).as_str() {
            "fixed" => RetryBackoffStrategy::Fixed,
            "exponential" => RetryBackoffStrategy::Exponential,
            _ => RetryBackoffStrategy::None,
        };
    }
    if let Some(initial) = number_field(
        source,
        &["initial_delay_ms", "initialDelayMs", "delay_ms", "delayMs"],
    ) {
        backoff.initial_delay_ms = initial;
    }
    if let Some(max) = number_field(source, &["max_delay_ms", "maxDelayMs"]) {
        backoff.max_delay_ms = max;
    }
    if let Some(multiplier) = float_field(source, &["multiplier"]) {
        backoff.multiplier = multiplier.max(1.0);
    }
    if let Some(jitter) = number_field(source, &["jitter_ms", "jitterMs"]) {
        backoff.jitter_ms = Some(jitter);
    }
    backoff
}

fn parse_terminal_behavior(value: &Value, default: RetryTerminalBehavior) -> RetryTerminalBehavior {
    let source = value.get("terminal_behavior").unwrap_or(value);
    let mut behavior = default;
    if let Some(mode) = string_field(source, &["mode", "terminal_mode", "terminalMode"]) {
        behavior.mode = match normalize_class(&mode).as_str() {
            "pause_for_review" => RetryTerminalMode::PauseForReview,
            "dead_letter" => RetryTerminalMode::DeadLetter,
            _ => RetryTerminalMode::FailRun,
        };
    }
    if let Some(dead_letter) = bool_field(source, &["dead_letter", "deadLetter"]) {
        behavior.dead_letter = dead_letter;
    }
    behavior
}

fn parse_manual_override(
    value: &Value,
    default: RetryManualOverridePolicy,
) -> RetryManualOverridePolicy {
    let source = value.get("manual_override").unwrap_or(value);
    let mut manual = default;
    if let Some(enabled) = bool_field(source, &["enabled", "manual_override", "manualOverride"]) {
        manual.enabled = enabled;
    }
    if let Some(requires_reason) = bool_field(source, &["requires_reason", "requiresReason"]) {
        manual.requires_reason = requires_reason;
    }
    manual
}

fn clamp_max_attempts(value: u32) -> u32 {
    value.clamp(1, RETRY_POLICY_MAX_ATTEMPTS_CAP)
}

fn number_field(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| value.get(*key))
        .and_then(|value| value.as_u64())
}

fn float_field(value: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| value.get(*key))
        .and_then(|value| value.as_f64())
}

fn bool_field(value: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| value.get(*key))
        .and_then(|value| value.as_bool())
}

fn string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| value.get(*key))
        .and_then(|value| value.as_str())
        .map(str::to_string)
}

fn string_array_field(value: &Value, keys: &[&str]) -> Option<Vec<String>> {
    let rows = keys
        .iter()
        .find_map(|key| value.get(*key))
        .and_then(Value::as_array)?;
    Some(
        rows.iter()
            .filter_map(Value::as_str)
            .map(normalize_class)
            .collect(),
    )
}

fn normalize_class(value: &str) -> String {
    value.trim().to_ascii_lowercase().replace(['-', ' '], "_")
}

fn stable_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
