use chrono::{DateTime, Utc};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::Level;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

mod metrics;
pub use metrics::{
    observability_metrics_snapshot, record_engine_event_metrics, record_gate_wait_ms,
    record_provider_error, record_run_duration_ms, record_scheduler_clock_regression_ms,
    record_scheduler_tick_latency_ms, record_tool_call_decision,
    render_observability_metrics_prometheus, MetricSummary, ObservabilityMetricsSnapshot,
};

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessKind {
    Engine,
    Desktop,
    Tui,
}

impl ProcessKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ProcessKind::Engine => "engine",
            ProcessKind::Desktop => "desktop",
            ProcessKind::Tui => "tui",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct LoggingInitInfo {
    pub process: String,
    pub logs_dir: String,
    pub prefix: String,
    pub retention_days: u64,
    pub initialized_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObservabilityEvent<'a> {
    pub event: &'a str,
    pub component: &'a str,
    pub org_id: Option<&'a str>,
    pub workspace_id: Option<&'a str>,
    pub correlation_id: Option<&'a str>,
    pub session_id: Option<&'a str>,
    pub run_id: Option<&'a str>,
    pub message_id: Option<&'a str>,
    pub provider_id: Option<&'a str>,
    pub model_id: Option<&'a str>,
    pub status: Option<&'a str>,
    pub error_code: Option<&'a str>,
    pub detail: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScrubbedObservabilityExport {
    pub process: String,
    pub event: String,
    pub component: String,
    pub org_id: Option<String>,
    pub workspace_id: Option<String>,
    pub correlation_id: Option<String>,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub message_id: Option<String>,
    pub provider_id: Option<String>,
    pub model_id: Option<String>,
    pub status: Option<String>,
    pub error_code: Option<String>,
}

pub fn scrubbed_observability_export(
    process: ProcessKind,
    event: &ObservabilityEvent<'_>,
    org_id: Option<&str>,
    workspace_id: Option<&str>,
) -> ScrubbedObservabilityExport {
    ScrubbedObservabilityExport {
        process: process.as_str().to_string(),
        event: safe_id(event.event),
        component: safe_id(event.component),
        org_id: safe_option_id(org_id.or(event.org_id)),
        workspace_id: safe_option_id(workspace_id.or(event.workspace_id)),
        correlation_id: safe_option_id(event.correlation_id),
        session_id: safe_option_id(event.session_id),
        run_id: safe_option_id(event.run_id),
        message_id: safe_option_id(event.message_id),
        provider_id: safe_option_id(event.provider_id),
        model_id: safe_option_id(event.model_id),
        status: safe_option_id(event.status),
        error_code: safe_option_id(event.error_code),
    }
}

pub fn redact_text(input: &str) -> String {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    format!(
        "[redacted len={} sha256={}]",
        trimmed.len(),
        short_hash(trimmed)
    )
}

pub fn short_hash(input: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    input.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

pub fn emit_event(level: Level, process: ProcessKind, event: ObservabilityEvent<'_>) {
    emit_event_with_tenant(level, process, event, None, None);
}

pub fn emit_event_with_tenant(
    level: Level,
    process: ProcessKind,
    event: ObservabilityEvent<'_>,
    org_id: Option<&str>,
    workspace_id: Option<&str>,
) {
    let org_id = org_id.or(event.org_id).unwrap_or("");
    let workspace_id = workspace_id.or(event.workspace_id).unwrap_or("");
    #[cfg(feature = "sentry")]
    if level == Level::ERROR {
        let _ = capture_sentry_error_event(process, &event, Some(org_id), Some(workspace_id));
    }

    match level {
        Level::ERROR => tracing::error!(
            target: "tandem.obs",
            process = process.as_str(),
            component = event.component,
            event = event.event,
            correlation_id = event.correlation_id.unwrap_or(""),
            session_id = event.session_id.unwrap_or(""),
            run_id = event.run_id.unwrap_or(""),
            org_id,
            workspace_id,
            message_id = event.message_id.unwrap_or(""),
            provider_id = event.provider_id.unwrap_or(""),
            model_id = event.model_id.unwrap_or(""),
            status = event.status.unwrap_or(""),
            error_code = event.error_code.unwrap_or(""),
            detail = event.detail.unwrap_or(""),
            "observability_event"
        ),
        Level::WARN => tracing::warn!(
            target: "tandem.obs",
            process = process.as_str(),
            component = event.component,
            event = event.event,
            correlation_id = event.correlation_id.unwrap_or(""),
            session_id = event.session_id.unwrap_or(""),
            run_id = event.run_id.unwrap_or(""),
            org_id,
            workspace_id,
            message_id = event.message_id.unwrap_or(""),
            provider_id = event.provider_id.unwrap_or(""),
            model_id = event.model_id.unwrap_or(""),
            status = event.status.unwrap_or(""),
            error_code = event.error_code.unwrap_or(""),
            detail = event.detail.unwrap_or(""),
            "observability_event"
        ),
        _ => tracing::info!(
            target: "tandem.obs",
            process = process.as_str(),
            component = event.component,
            event = event.event,
            correlation_id = event.correlation_id.unwrap_or(""),
            session_id = event.session_id.unwrap_or(""),
            run_id = event.run_id.unwrap_or(""),
            org_id,
            workspace_id,
            message_id = event.message_id.unwrap_or(""),
            provider_id = event.provider_id.unwrap_or(""),
            model_id = event.model_id.unwrap_or(""),
            status = event.status.unwrap_or(""),
            error_code = event.error_code.unwrap_or(""),
            detail = event.detail.unwrap_or(""),
            "observability_event"
        ),
    }
}

pub fn init_process_logging(
    process: ProcessKind,
    logs_dir: &Path,
    retention_days: u64,
) -> anyhow::Result<(WorkerGuard, LoggingInitInfo)> {
    fs::create_dir_all(logs_dir)?;
    cleanup_old_jsonl(logs_dir, process.as_str(), retention_days)?;

    let file_appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix(format!("tandem.{}", process.as_str()))
        .filename_suffix("jsonl")
        .build(logs_dir)?;

    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let file_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_current_span(false)
        .with_span_list(false);

    let console_layer = tracing_subscriber::fmt::layer()
        .compact()
        .with_target(true)
        .with_ansi(true);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(console_layer)
        .with(file_layer)
        .try_init()
        .ok();

    let info = LoggingInitInfo {
        process: process.as_str().to_string(),
        logs_dir: logs_dir.display().to_string(),
        prefix: format!("tandem.{}", process.as_str()),
        retention_days,
        initialized_at: Utc::now(),
    };

    Ok((guard, info))
}

fn cleanup_old_jsonl(logs_dir: &Path, process: &str, retention_days: u64) -> anyhow::Result<()> {
    let cutoff = Utc::now() - chrono::Duration::days(retention_days as i64);
    let prefix = format!("tandem.{}.", process);

    for entry in fs::read_dir(logs_dir)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if !name.starts_with(&prefix) || !name.ends_with(".jsonl") {
            continue;
        }

        // expected: tandem.<proc>.YYYY-MM-DD.jsonl
        let date_part = name.trim_start_matches(&prefix).trim_end_matches(".jsonl");

        let Ok(date) = chrono::NaiveDate::parse_from_str(date_part, "%Y-%m-%d") else {
            continue;
        };

        let Some(dt) = date.and_hms_opt(0, 0, 0) else {
            continue;
        };

        if DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc) < cutoff {
            let _ = fs::remove_file(path);
        }
    }

    Ok(())
}

pub fn canonical_logs_dir_from_root(root: &Path) -> PathBuf {
    root.join("logs")
}

fn safe_option_id(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(safe_id)
}

fn safe_id(value: &str) -> String {
    let mut out = value
        .chars()
        .take(128)
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/') {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();
    if out.trim_matches('_').is_empty() {
        out = "unknown".to_string();
    }
    out
}

#[cfg(feature = "sentry")]
pub fn init_sentry_export(
    dsn: &str,
    environment: Option<&str>,
) -> anyhow::Result<sentry::ClientInitGuard> {
    use std::sync::Arc;

    let dsn = dsn.parse()?;
    let environment = environment.map(|value| value.to_string().into());
    Ok(sentry::init(sentry::ClientOptions {
        dsn: Some(dsn),
        environment,
        before_send: Some(Arc::new(scrub_sentry_protocol_event)),
        ..Default::default()
    }))
}

#[cfg(feature = "sentry")]
pub fn capture_sentry_error_event(
    process: ProcessKind,
    event: &ObservabilityEvent<'_>,
    org_id: Option<&str>,
    workspace_id: Option<&str>,
) -> Option<String> {
    if event.error_code.is_none() && event.status != Some("failed") && event.status != Some("error")
    {
        return None;
    }
    let export = scrubbed_observability_export(process, event, org_id, workspace_id);
    let mut sentry_event = sentry::protocol::Event {
        level: sentry::Level::Error,
        logger: Some("tandem.observability".into()),
        message: Some("tandem observability error".into()),
        ..Default::default()
    };
    sentry_event.tags.insert("process".into(), export.process);
    sentry_event.tags.insert("event".into(), export.event);
    sentry_event
        .tags
        .insert("component".into(), export.component);
    if let Some(org_id) = export.org_id {
        sentry_event.tags.insert("org_id".into(), org_id);
    }
    if let Some(workspace_id) = export.workspace_id {
        sentry_event
            .tags
            .insert("workspace_id".into(), workspace_id);
    }
    if let Some(error_code) = export.error_code {
        sentry_event.tags.insert("error_code".into(), error_code);
    }
    if let Some(status) = export.status {
        sentry_event.tags.insert("status".into(), status);
    }
    Some(sentry::capture_event(sentry_event).to_string())
}

#[cfg(feature = "sentry")]
fn scrub_sentry_protocol_event(
    event: sentry::protocol::Event<'static>,
) -> Option<sentry::protocol::Event<'static>> {
    let allowed_tags = [
        "process",
        "event",
        "component",
        "org_id",
        "workspace_id",
        "error_code",
        "status",
    ];
    let tags = event
        .tags
        .into_iter()
        .filter(|(key, _)| allowed_tags.contains(&key.as_str()))
        .map(|(key, value)| (key, safe_id(&value)))
        .collect();
    Some(sentry::protocol::Event {
        event_id: event.event_id,
        timestamp: event.timestamp,
        level: event.level,
        logger: Some("tandem.observability".into()),
        message: Some("tandem observability event".into()),
        tags,
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redact_text_masks_content() {
        let raw = "super-secret-token-123";
        let redacted = redact_text(raw);
        assert!(redacted.contains("[redacted len="));
        assert!(!redacted.contains("super-secret-token-123"));
    }

    #[test]
    fn canonical_logs_dir_joins_logs_folder() {
        let root = PathBuf::from("C:/tmp/tandem");
        let logs = canonical_logs_dir_from_root(&root);
        assert_eq!(logs, PathBuf::from("C:/tmp/tandem").join("logs"));
    }

    #[test]
    fn observability_event_serializes_tenant_ids() {
        let event = ObservabilityEvent {
            event: "provider.call.error",
            component: "engine.loop",
            org_id: Some("org-a"),
            workspace_id: Some("workspace-a"),
            correlation_id: None,
            session_id: Some("session-a"),
            run_id: Some("run-a"),
            message_id: None,
            provider_id: Some("openai"),
            model_id: Some("gpt-test"),
            status: Some("failed"),
            error_code: Some("RATE_LIMIT"),
            detail: Some("provider failed"),
        };

        let value = serde_json::to_value(&event).expect("event json");
        assert_eq!(value["org_id"], "org-a");
        assert_eq!(value["workspace_id"], "workspace-a");
    }

    #[test]
    fn scrubbed_export_excludes_prompt_and_tool_argument_text() {
        let canary = "CANARY_PROMPT_TEXT tool_arg=secret@example.com";
        let event = ObservabilityEvent {
            event: "provider.call.error",
            component: "engine.loop",
            org_id: Some("org-a"),
            workspace_id: Some("workspace-a"),
            correlation_id: Some("corr-a"),
            session_id: Some("session-a"),
            run_id: Some("run-a"),
            message_id: Some("message-a"),
            provider_id: Some("openai"),
            model_id: Some("gpt-test"),
            status: Some("failed"),
            error_code: Some("RATE_LIMIT"),
            detail: Some(canary),
        };

        let export = scrubbed_observability_export(ProcessKind::Engine, &event, None, None);
        let json = serde_json::to_string(&export).expect("export json");
        assert!(json.contains("org-a"));
        assert!(json.contains("RATE_LIMIT"));
        assert!(!json.contains(canary));
        assert!(!json.contains("secret@example.com"));
    }

    #[test]
    fn prometheus_renderer_exposes_scrubbed_metric_labels() {
        metrics::reset_observability_metrics_for_tests();
        record_scheduler_tick_latency_ms(12);
        record_scheduler_clock_regression_ms(25);
        record_tool_call_decision("allow");
        record_provider_error("openai/prod", "rate limit with spaces");
        record_engine_event_metrics(
            "permission.asked",
            &serde_json::json!({
                "requestID": "permission-a",
                "requestedAtMs": 100,
            }),
        );
        record_engine_event_metrics(
            "permission.wait.timeout",
            &serde_json::json!({
                "requestID": "permission-a",
                "timeoutMs": 250,
            }),
        );
        record_engine_event_metrics(
            "provider.call.iteration.error",
            &serde_json::json!({
                "providerID": "openai",
                "errorCode": "STREAM_IDLE_TIMEOUT",
            }),
        );

        let rendered = render_observability_metrics_prometheus();
        assert!(rendered.contains("tandem_scheduler_tick_latency_ms_count 1"));
        assert!(rendered.contains("tandem_scheduler_clock_regressions_total 1"));
        assert!(rendered.contains("tandem_scheduler_clock_regression_ms_count 1"));
        assert!(rendered.contains("tandem_tool_call_decisions_total{decision=\"allow\"} 1"));
        assert!(rendered.contains("tandem_tool_call_decisions_total{decision=\"deny\"} 1"));
        assert!(rendered.contains("tandem_gate_wait_ms_count{decision=\"deny\"} 1"));
        assert!(rendered.contains("tandem_gate_wait_ms_sum{decision=\"deny\"} 250"));
        assert!(rendered.contains(
            "tandem_provider_errors_total{provider_id=\"openai_prod\",error_code=\"rate_limit_with_spaces\"} 1"
        ));
        assert!(rendered.contains(
            "tandem_provider_errors_total{provider_id=\"openai\",error_code=\"STREAM_IDLE_TIMEOUT\"} 1"
        ));
    }
}
