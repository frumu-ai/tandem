use crate::{IncidentMonitorConfig, IncidentMonitorLabelMode, IncidentMonitorProviderPreference};
use serde_json::json;
use tandem_types::RuntimeAuthMode;

pub(crate) fn resolve_run_stale_ms() -> u64 {
    std::env::var("TANDEM_RUN_STALE_MS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(120_000)
        .clamp(30_000, 600_000)
}

pub(crate) fn resolve_token_cost_per_1k_usd() -> f64 {
    std::env::var("TANDEM_TOKEN_COST_PER_1K_USD")
        .ok()
        .and_then(|v| v.trim().parse::<f64>().ok())
        .unwrap_or(0.0)
        .max(0.0)
}

pub(crate) fn resolve_automation_strict_research_quality() -> bool {
    std::env::var("TANDEM_AUTOMATION_STRICT_RESEARCH_QUALITY")
        .ok()
        .and_then(|v| match v.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
        .unwrap_or(true)
}

pub(crate) fn resolve_automation_quality_legacy_rollback_enabled() -> bool {
    std::env::var("TANDEM_AUTOMATION_QUALITY_LEGACY_ROLLBACK")
        .ok()
        .and_then(|v| match v.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
        .unwrap_or(false)
}

pub(crate) fn resolve_allow_unsigned_dev_webhooks() -> bool {
    std::env::var("TANDEM_AUTOMATION_WEBHOOK_ALLOW_UNSIGNED_DEV_MODE")
        .ok()
        .and_then(|v| match v.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
        .unwrap_or(false)
}

pub(crate) fn prometheus_metrics_enabled() -> bool {
    std::env::var("TANDEM_OBSERVABILITY_PROMETHEUS_ENABLED")
        .ok()
        .and_then(|v| match v.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
        .unwrap_or(false)
}

pub(crate) fn resolve_runtime_auth_mode() -> RuntimeAuthMode {
    std::env::var("TANDEM_RUNTIME_AUTH_MODE")
        .ok()
        .and_then(|value| RuntimeAuthMode::parse(&value).ok())
        .unwrap_or_default()
}

fn env_value_present(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

pub(crate) fn context_assertion_verifier_configured() -> bool {
    [
        "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS",
        "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS_FILE",
        "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY",
        "TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY_FILE",
    ]
    .iter()
    .any(|name| env_value_present(name))
}

pub(crate) fn hosted_control_plane_configured() -> bool {
    [
        "HOSTED_CONTROL_PANEL_PUBLIC_URL",
        "HOSTED_PUBLIC_URL",
        "TANDEM_HOSTED_CONTROL_PLANE_URL",
        "TANDEM_ENTERPRISE_CONTROL_PLANE_URL",
    ]
    .iter()
    .any(|name| env_value_present(name))
}

pub(crate) fn cross_tenant_grant_signing_key_configured() -> bool {
    [
        "TANDEM_CROSS_TENANT_GRANT_SIGNING_KEY",
        "TANDEM_CROSS_TENANT_GRANT_SIGNING_KEY_FILE",
    ]
    .iter()
    .any(|name| env_value_present(name))
}

pub(crate) fn resolve_automation_execute_node_timeout_ms() -> u64 {
    std::env::var("TANDEM_AUTOMATION_EXECUTE_NODE_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1_800_000)
        .clamp(180_000, 3_600_000)
}

pub(crate) fn resolve_incident_monitor_env_config() -> IncidentMonitorConfig {
    fn env_value(new_name: &str, legacy_name: &str) -> Option<String> {
        std::env::var(new_name)
            .ok()
            .or_else(|| std::env::var(legacy_name).ok())
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    }

    fn env_bool(new_name: &str, legacy_name: &str, default: bool) -> bool {
        env_value(new_name, legacy_name)
            .map(|value| parse_bool_like(&value, default))
            .unwrap_or(default)
    }

    fn parse_bool_like(value: &str, default: bool) -> bool {
        match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            _ => default,
        }
    }

    let provider_preference = match env_value(
        "TANDEM_INCIDENT_MONITOR_PROVIDER_PREFERENCE",
        "TANDEM_FAILURE_REPORTER_PROVIDER_PREFERENCE",
    )
    .unwrap_or_default()
    .trim()
    .to_ascii_lowercase()
    .as_str()
    {
        "official_github" | "official-github" | "github" => {
            IncidentMonitorProviderPreference::OfficialGithub
        }
        "composio" => IncidentMonitorProviderPreference::Composio,
        "arcade" => IncidentMonitorProviderPreference::Arcade,
        _ => IncidentMonitorProviderPreference::Auto,
    };
    let provider_id = env_value(
        "TANDEM_INCIDENT_MONITOR_PROVIDER_ID",
        "TANDEM_FAILURE_REPORTER_PROVIDER_ID",
    );
    let model_id = env_value(
        "TANDEM_INCIDENT_MONITOR_MODEL_ID",
        "TANDEM_FAILURE_REPORTER_MODEL_ID",
    );
    let model_policy = match (provider_id, model_id) {
        (Some(provider_id), Some(model_id)) => Some(json!({
            "default_model": {
                "provider_id": provider_id,
                "model_id": model_id,
            }
        })),
        _ => None,
    };
    IncidentMonitorConfig {
        enabled: env_bool(
            "TANDEM_INCIDENT_MONITOR_ENABLED",
            "TANDEM_FAILURE_REPORTER_ENABLED",
            false,
        ),
        paused: env_bool(
            "TANDEM_INCIDENT_MONITOR_PAUSED",
            "TANDEM_FAILURE_REPORTER_PAUSED",
            false,
        ),
        workspace_root: env_value(
            "TANDEM_INCIDENT_MONITOR_WORKSPACE_ROOT",
            "TANDEM_FAILURE_REPORTER_WORKSPACE_ROOT",
        ),
        repo: env_value(
            "TANDEM_INCIDENT_MONITOR_REPO",
            "TANDEM_FAILURE_REPORTER_REPO",
        ),
        mcp_server: env_value(
            "TANDEM_INCIDENT_MONITOR_MCP_SERVER",
            "TANDEM_FAILURE_REPORTER_MCP_SERVER",
        ),
        provider_preference,
        model_policy,
        auto_create_new_issues: env_bool(
            "TANDEM_INCIDENT_MONITOR_AUTO_CREATE_NEW_ISSUES",
            "TANDEM_FAILURE_REPORTER_AUTO_CREATE_NEW_ISSUES",
            true,
        ),
        require_approval_for_new_issues: env_bool(
            "TANDEM_INCIDENT_MONITOR_REQUIRE_APPROVAL_FOR_NEW_ISSUES",
            "TANDEM_FAILURE_REPORTER_REQUIRE_APPROVAL_FOR_NEW_ISSUES",
            false,
        ),
        auto_comment_on_matched_open_issues: env_bool(
            "TANDEM_INCIDENT_MONITOR_AUTO_COMMENT_ON_MATCHED_OPEN_ISSUES",
            "TANDEM_FAILURE_REPORTER_AUTO_COMMENT_ON_MATCHED_OPEN_ISSUES",
            true,
        ),
        label_mode: IncidentMonitorLabelMode::ReporterOnly,
        triage_timeout_ms: env_value(
            "TANDEM_INCIDENT_MONITOR_TRIAGE_TIMEOUT_MS",
            "TANDEM_FAILURE_REPORTER_TRIAGE_TIMEOUT_MS",
        )
        .as_deref()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .map(Some)
        .unwrap_or(Some(1_800_000)),
        monitored_projects: Vec::new(),
        destinations: Vec::new(),
        routes: Vec::new(),
        default_destination_ids: Vec::new(),
        safety_defaults: Default::default(),
        updated_at_ms: 0,
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SchedulerMode {
    Single,
    Multi,
}

pub(crate) fn resolve_scheduler_mode() -> SchedulerMode {
    match std::env::var("TANDEM_SCHEDULER_MODE")
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("single") => SchedulerMode::Single,
        Some("multi") => SchedulerMode::Multi,
        _ => SchedulerMode::Multi,
    }
}

pub(crate) fn resolve_scheduler_max_concurrent_runs() -> usize {
    std::env::var("TANDEM_SCHEDULER_MAX_CONCURRENT_RUNS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(8)
}

pub(crate) fn resolve_scheduler_shutdown_timeout_secs() -> u64 {
    std::env::var("TANDEM_SCHEDULER_SHUTDOWN_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(30)
}
