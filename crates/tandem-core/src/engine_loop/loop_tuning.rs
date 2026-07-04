/// How many of the session's most recent tool invocations keep their full
/// (compacted) projection in provider-facing history. Older invocations are
/// demoted to a one-line summary with provenance handles — the raw records
/// stay untouched in session storage. 0 demotes everything.
pub(super) fn tool_result_keep_recent() -> usize {
    std::env::var("TANDEM_TOOL_RESULT_KEEP_RECENT")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(8)
}

pub(super) fn max_tool_iterations() -> usize {
    let default_iterations = 25usize;
    std::env::var("TANDEM_MAX_TOOL_ITERATIONS")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_iterations)
}

pub(super) fn strict_write_retry_max_attempts() -> usize {
    std::env::var("TANDEM_STRICT_WRITE_RETRY_MAX_ATTEMPTS")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(3)
}

pub(super) fn provider_stream_connect_timeout_ms() -> usize {
    std::env::var("TANDEM_PROVIDER_STREAM_CONNECT_TIMEOUT_MS")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(90_000)
}

pub(super) fn provider_stream_idle_timeout_ms() -> usize {
    std::env::var("TANDEM_PROVIDER_STREAM_IDLE_TIMEOUT_MS")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(90_000)
}

pub(super) fn provider_stream_decode_retry_attempts() -> usize {
    std::env::var("TANDEM_PROVIDER_STREAM_DECODE_RETRY_ATTEMPTS")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(2)
}

pub(super) fn prompt_context_hook_timeout_ms() -> usize {
    std::env::var("TANDEM_PROMPT_CONTEXT_HOOK_TIMEOUT_MS")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(5_000)
}

pub(super) fn permission_wait_timeout_ms() -> usize {
    std::env::var("TANDEM_PERMISSION_WAIT_TIMEOUT_MS")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(15_000)
}

/// Soft char budget for Full context mode prompts. Crossing it emits a
/// warning event with the top contributors but does not block the send.
/// Roughly 60k tokens at the shared 4-chars-per-token fallback estimate.
pub(super) fn full_context_soft_budget_chars() -> usize {
    std::env::var("TANDEM_FULL_CONTEXT_SOFT_BUDGET_CHARS")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(240_000)
}

/// Hard char budget for Full context mode prompts. Crossing it fails the run
/// closed before provider send unless the override env is set.
/// Roughly 120k tokens at the shared 4-chars-per-token fallback estimate.
pub(super) fn full_context_hard_budget_chars() -> usize {
    std::env::var("TANDEM_FULL_CONTEXT_HARD_BUDGET_CHARS")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(480_000)
}

pub(super) fn full_context_hard_budget_override() -> bool {
    std::env::var("TANDEM_FULL_CONTEXT_HARD_BUDGET_OVERRIDE")
        .map(|raw| {
            let value = raw.trim().to_ascii_lowercase();
            value == "1" || value == "true" || value == "yes"
        })
        .unwrap_or(false)
}

pub(super) fn tool_exec_timeout_ms() -> usize {
    std::env::var("TANDEM_TOOL_EXEC_TIMEOUT_MS")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(45_000)
}
