use chrono::{DateTime, Utc};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::Level;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

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
    match level {
        Level::ERROR => tracing::error!(
            target: "tandem.obs",
            process = process.as_str(),
            component = event.component,
            event = event.event,
            correlation_id = event.correlation_id.unwrap_or(""),
            session_id = event.session_id.unwrap_or(""),
            run_id = event.run_id.unwrap_or(""),
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
}
