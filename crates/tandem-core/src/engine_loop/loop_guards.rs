pub(super) const MIN_TOOL_CALL_LIMIT: usize = 200;
const EMAIL_DELIVERY_TOOL_LIMIT: usize = 1;

pub(super) fn tool_budget_for(tool_name: &str) -> usize {
    let normalized = super::normalize_tool_name(tool_name);
    if super::is_email_delivery_tool_name(&normalized) {
        if let Some(override_budget) = parse_budget_override("TANDEM_TOOL_BUDGET_EMAIL_DELIVERY") {
            if override_budget == usize::MAX {
                return usize::MAX;
            }
            return override_budget.max(EMAIL_DELIVERY_TOOL_LIMIT);
        }
        return EMAIL_DELIVERY_TOOL_LIMIT;
    }
    if env_budget_guards_disabled() {
        return usize::MAX;
    }
    let env_key = match normalized.as_str() {
        "glob" => "TANDEM_TOOL_BUDGET_GLOB",
        "read" => "TANDEM_TOOL_BUDGET_READ",
        "websearch" => "TANDEM_TOOL_BUDGET_WEBSEARCH",
        "batch" => "TANDEM_TOOL_BUDGET_BATCH",
        "grep" | "search" | "codesearch" => "TANDEM_TOOL_BUDGET_SEARCH",
        _ => "TANDEM_TOOL_BUDGET_DEFAULT",
    };
    if let Some(override_budget) = parse_budget_override(env_key) {
        if override_budget == usize::MAX {
            return usize::MAX;
        }
        return override_budget.max(MIN_TOOL_CALL_LIMIT);
    }
    MIN_TOOL_CALL_LIMIT
}

pub(super) fn duplicate_signature_limit_for(tool_name: &str) -> usize {
    let normalized = super::normalize_tool_name(tool_name);
    if super::is_email_delivery_tool_name(&normalized) {
        if let Ok(raw) = std::env::var("TANDEM_TOOL_LOOP_DUPLICATE_SIGNATURE_LIMIT_EMAIL_DELIVERY")
        {
            if let Ok(parsed) = raw.trim().parse::<usize>() {
                if parsed > 0 {
                    return parsed.max(EMAIL_DELIVERY_TOOL_LIMIT);
                }
            }
        }
        return EMAIL_DELIVERY_TOOL_LIMIT;
    }
    if let Ok(raw) = std::env::var("TANDEM_TOOL_LOOP_DUPLICATE_SIGNATURE_LIMIT") {
        if let Ok(parsed) = raw.trim().parse::<usize>() {
            if parsed > 0 {
                return parsed.max(MIN_TOOL_CALL_LIMIT);
            }
        }
    }
    MIN_TOOL_CALL_LIMIT
}

pub(super) fn websearch_duplicate_signature_limit() -> Option<usize> {
    std::env::var("TANDEM_WEBSEARCH_DUPLICATE_SIGNATURE_LIMIT")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .map(|value| value.max(MIN_TOOL_CALL_LIMIT))
}

fn env_budget_guards_disabled() -> bool {
    std::env::var("TANDEM_DISABLE_TOOL_GUARD_BUDGETS")
        .ok()
        .map(|raw| {
            matches!(
                raw.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

pub(super) fn parse_budget_override(env_key: &str) -> Option<usize> {
    let raw = std::env::var(env_key).ok()?;
    let trimmed = raw.trim().to_ascii_lowercase();
    if matches!(
        trimmed.as_str(),
        "0" | "inf" | "infinite" | "unlimited" | "none"
    ) {
        return Some(usize::MAX);
    }
    trimmed
        .parse::<usize>()
        .ok()
        .and_then(|value| if value > 0 { Some(value) } else { None })
}
