// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde_json::Value;

/// Planner generation can persist a session and dispatch a provider before a
/// draft is written. Keep its write grant live at each of those boundaries.
pub(super) fn require_live_planner_write(
    state: &crate::AppState,
    tenant: &tandem_types::TenantContext,
    verified: Option<&tandem_types::VerifiedTenantContext>,
) -> Result<(), &'static str> {
    // Standalone/local planning keeps its existing behavior. A configured but
    // unsynchronized hosted policy is an error, not a local-mode fallback.
    if state.enterprise.hosted_policy.current()?.is_none() {
        if verified.is_some_and(|verified| verified.policy_version.is_some()) {
            return Err("hosted_policy_source_unavailable");
        }
        return Ok(());
    }
    let verified = verified.ok_or("hosted_policy_identity_required")?;
    if verified.is_expired_at(crate::now_ms())
        || !super::tenant_matches(tenant, &verified.tenant_context)
        || tenant.actor_id.as_deref() != Some(verified.human_actor.actor_id.as_str())
    {
        return Err("hosted_planner_identity_mismatch");
    }
    state.enterprise.hosted_policy.authorize_permission(
        Some(verified),
        tandem_types::AccessPermission::HostedAutomationWrite,
    )
}

pub(super) fn planner_test_override_payload(
    primary_env: &str,
    include_legacy: bool,
) -> Option<Value> {
    let raw = std::env::var(primary_env).ok().or_else(|| {
        include_legacy
            .then(|| std::env::var("TANDEM_WORKFLOW_PLANNER_TEST_RESPONSE").ok())
            .flatten()
    })?;
    if raw.trim().is_empty() {
        return None;
    }
    tandem_plan_compiler::api::extract_json_value_from_text(&raw)
}

pub(super) fn planner_build_timeout_ms() -> u64 {
    std::env::var("TANDEM_WORKFLOW_PLANNER_BUILD_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|value| value.clamp(250, 600_000))
        .unwrap_or(600_000)
}

pub(super) fn planner_revision_timeout_ms() -> u64 {
    std::env::var("TANDEM_WORKFLOW_PLANNER_REVISION_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|value| value.clamp(250, 600_000))
        .unwrap_or_else(planner_build_timeout_ms)
}

pub(super) fn classify_planner_provider_failure_reason(error: &str) -> &'static str {
    let lower = error.to_ascii_lowercase();
    if lower.contains("array too long") || lower.contains("maximum length 128") {
        "tool_schema_too_large"
    } else if lower.contains("user not found")
        || lower.contains("unauthorized")
        || lower.contains("authentication")
        || lower.contains("invalid api key")
        || lower.contains("403")
        || lower.contains("401")
    {
        "provider_auth_failed"
    } else if lower.contains("invalid function name")
        || lower.contains("function_declarations")
        || lower.contains("tools[0]")
    {
        "provider_tool_schema_invalid"
    } else {
        "provider_request_failed"
    }
}

#[cfg(test)]
mod tests {
    use super::{planner_build_timeout_ms, planner_revision_timeout_ms};
    use std::sync::{Mutex, MutexGuard, OnceLock};

    fn planner_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct PlannerEnvGuard {
        _guard: MutexGuard<'static, ()>,
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl PlannerEnvGuard {
        fn new(vars: &[&'static str]) -> Self {
            let guard = planner_env_lock().lock().expect("planner env lock");
            let saved = vars
                .iter()
                .copied()
                .map(|key| (key, std::env::var(key).ok()))
                .collect::<Vec<_>>();
            Self {
                _guard: guard,
                saved,
            }
        }

        fn remove(&self, key: &'static str) {
            std::env::remove_var(key);
        }

        fn set(&self, key: &'static str, value: &str) {
            std::env::set_var(key, value);
        }
    }

    impl Drop for PlannerEnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..) {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }

    #[test]
    #[serial_test::serial]
    fn planner_revision_timeout_defaults_to_build_timeout() {
        let guard = PlannerEnvGuard::new(&[
            "TANDEM_WORKFLOW_PLANNER_BUILD_TIMEOUT_MS",
            "TANDEM_WORKFLOW_PLANNER_REVISION_TIMEOUT_MS",
        ]);
        guard.remove("TANDEM_WORKFLOW_PLANNER_BUILD_TIMEOUT_MS");
        guard.remove("TANDEM_WORKFLOW_PLANNER_REVISION_TIMEOUT_MS");
        assert_eq!(planner_revision_timeout_ms(), planner_build_timeout_ms());
    }

    #[test]
    #[serial_test::serial]
    fn planner_revision_timeout_honors_explicit_override() {
        let guard = PlannerEnvGuard::new(&[
            "TANDEM_WORKFLOW_PLANNER_BUILD_TIMEOUT_MS",
            "TANDEM_WORKFLOW_PLANNER_REVISION_TIMEOUT_MS",
        ]);
        guard.set("TANDEM_WORKFLOW_PLANNER_BUILD_TIMEOUT_MS", "600000");
        guard.set("TANDEM_WORKFLOW_PLANNER_REVISION_TIMEOUT_MS", "180000");
        assert_eq!(planner_revision_timeout_ms(), 180_000);
    }

    #[test]
    #[serial_test::serial]
    fn planner_build_timeout_defaults_to_longer_budget() {
        let guard = PlannerEnvGuard::new(&["TANDEM_WORKFLOW_PLANNER_BUILD_TIMEOUT_MS"]);
        guard.remove("TANDEM_WORKFLOW_PLANNER_BUILD_TIMEOUT_MS");
        assert_eq!(planner_build_timeout_ms(), 600_000);
    }
}
