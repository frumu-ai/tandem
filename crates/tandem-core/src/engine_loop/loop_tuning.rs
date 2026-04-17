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

pub(super) fn tool_exec_timeout_ms() -> usize {
    std::env::var("TANDEM_TOOL_EXEC_TIMEOUT_MS")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(45_000)
}
