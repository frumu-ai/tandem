use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_TRACKED_PENDING: usize = 2_000;

#[derive(Debug, Clone, Default, Serialize)]
pub struct MetricSummary {
    pub count: u64,
    pub sum_ms: u64,
    pub max_ms: u64,
}

impl MetricSummary {
    fn record(&mut self, value_ms: u64) {
        self.count = self.count.saturating_add(1);
        self.sum_ms = self.sum_ms.saturating_add(value_ms);
        self.max_ms = self.max_ms.max(value_ms);
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ObservabilityMetricsSnapshot {
    pub scheduler_tick_latency_ms: MetricSummary,
    pub run_duration_ms_by_status: BTreeMap<String, MetricSummary>,
    pub gate_wait_ms_by_decision: BTreeMap<String, MetricSummary>,
    pub tool_call_decisions_total: BTreeMap<String, u64>,
    pub provider_errors_total: BTreeMap<String, u64>,
}

#[derive(Debug, Default)]
struct MetricsState {
    snapshot: ObservabilityMetricsSnapshot,
    pending_runs: BTreeMap<String, u64>,
    pending_gates: BTreeMap<String, u64>,
}

static METRICS_STATE: OnceLock<Mutex<MetricsState>> = OnceLock::new();

fn metrics_state() -> &'static Mutex<MetricsState> {
    METRICS_STATE.get_or_init(|| Mutex::new(MetricsState::default()))
}

pub fn observability_metrics_snapshot() -> ObservabilityMetricsSnapshot {
    metrics_state()
        .lock()
        .expect("observability metrics mutex poisoned")
        .snapshot
        .clone()
}

pub fn record_scheduler_tick_latency_ms(duration_ms: u64) {
    metrics::histogram!("tandem_scheduler_tick_latency_ms").record(duration_ms as f64);
    let mut state = metrics_state()
        .lock()
        .expect("observability metrics mutex poisoned");
    state.snapshot.scheduler_tick_latency_ms.record(duration_ms);
}

pub fn record_run_duration_ms(status: &str, duration_ms: u64) {
    let status = safe_label(status);
    metrics::histogram!("tandem_run_duration_ms", "status" => status.clone())
        .record(duration_ms as f64);
    let mut state = metrics_state()
        .lock()
        .expect("observability metrics mutex poisoned");
    state
        .snapshot
        .run_duration_ms_by_status
        .entry(status)
        .or_default()
        .record(duration_ms);
}

pub fn record_gate_wait_ms(decision: &str, duration_ms: u64) {
    let decision = safe_label(decision);
    metrics::histogram!("tandem_gate_wait_ms", "decision" => decision.clone())
        .record(duration_ms as f64);
    let mut state = metrics_state()
        .lock()
        .expect("observability metrics mutex poisoned");
    state
        .snapshot
        .gate_wait_ms_by_decision
        .entry(decision)
        .or_default()
        .record(duration_ms);
}

pub fn record_tool_call_decision(decision: &str) {
    let decision = safe_label(decision);
    metrics::counter!("tandem_tool_call_decisions_total", "decision" => decision.clone())
        .increment(1);
    let mut state = metrics_state()
        .lock()
        .expect("observability metrics mutex poisoned");
    *state
        .snapshot
        .tool_call_decisions_total
        .entry(decision)
        .or_default() += 1;
}

pub fn record_provider_error(provider_id: &str, error_code: &str) {
    let provider_id = safe_label(provider_id);
    let error_code = safe_label(error_code);
    metrics::counter!(
        "tandem_provider_errors_total",
        "provider_id" => provider_id.clone(),
        "error_code" => error_code.clone()
    )
    .increment(1);
    let key = format!("provider_id={provider_id},error_code={error_code}");
    let mut state = metrics_state()
        .lock()
        .expect("observability metrics mutex poisoned");
    *state.snapshot.provider_errors_total.entry(key).or_default() += 1;
}

pub fn record_engine_event_metrics(event_type: &str, properties: &Value) {
    match event_type {
        "session.run.started" => {
            if let Some(key) = run_metric_key(properties) {
                track_pending_run(key, event_time_ms(properties).unwrap_or_else(now_ms));
            }
        }
        "session.run.finished" => {
            let status = properties
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("completed");
            if let Some(key) = run_metric_key(properties) {
                if let Some(started_at_ms) = take_pending_run(&key) {
                    let finished_at_ms = event_time_ms(properties).unwrap_or_else(now_ms);
                    record_run_duration_ms(status, finished_at_ms.saturating_sub(started_at_ms));
                }
            }
        }
        "permission.asked" => {
            record_tool_call_decision("ask");
            if let Some(request_id) = string_property(properties, &["requestID", "request_id"]) {
                let requested_at_ms = properties
                    .get("requestedAtMs")
                    .and_then(Value::as_u64)
                    .unwrap_or_else(now_ms);
                track_pending_gate(request_id.to_string(), requested_at_ms);
            }
        }
        "permission.auto_approved" => {
            record_tool_call_decision("allow");
        }
        "permission.wait.timeout" => {
            record_tool_call_decision("deny");
            let timeout_ms = properties.get("timeoutMs").and_then(Value::as_u64);
            if let Some(request_id) = string_property(properties, &["requestID", "request_id"]) {
                let duration_ms = take_pending_gate(request_id)
                    .map(|requested_at_ms| {
                        timeout_ms.unwrap_or_else(|| now_ms().saturating_sub(requested_at_ms))
                    })
                    .or(timeout_ms);
                if let Some(duration_ms) = duration_ms {
                    record_gate_wait_ms("deny", duration_ms);
                }
            } else if let Some(duration_ms) = timeout_ms {
                record_gate_wait_ms("deny", duration_ms);
            }
        }
        "permission.replied" => {
            let reply = string_property(properties, &["reply", "decision"]).unwrap_or("unknown");
            let decision = normalize_decision(reply);
            record_tool_call_decision(decision);
            if let Some(request_id) = string_property(properties, &["requestID", "request_id"]) {
                if let Some(requested_at_ms) = take_pending_gate(request_id) {
                    let decided_at_ms = properties
                        .get("decidedAtMs")
                        .and_then(Value::as_u64)
                        .unwrap_or_else(now_ms);
                    record_gate_wait_ms(decision, decided_at_ms.saturating_sub(requested_at_ms));
                }
            }
        }
        "policy.decision.recorded" => {
            if let Some(decision) = properties
                .get("record")
                .and_then(|record| record.get("decision"))
                .and_then(Value::as_str)
            {
                record_tool_call_decision(normalize_decision(decision));
            }
        }
        "provider.call.error" | "provider.call.iteration.error" => {
            let provider_id = string_property(
                properties,
                &["providerID", "providerId", "provider", "provider_id"],
            )
            .unwrap_or("unknown");
            let error_code =
                string_property(properties, &["errorCode", "code", "error_code", "status"])
                    .unwrap_or(event_type);
            record_provider_error(provider_id, error_code);
        }
        _ => {}
    }
}

pub fn render_observability_metrics_prometheus() -> String {
    let snapshot = observability_metrics_snapshot();
    let mut out = String::new();
    render_summary(
        &mut out,
        "tandem_scheduler_tick_latency_ms",
        &[],
        &snapshot.scheduler_tick_latency_ms,
    );
    for (status, summary) in snapshot.run_duration_ms_by_status {
        render_summary(
            &mut out,
            "tandem_run_duration_ms",
            &[("status", status.as_str())],
            &summary,
        );
    }
    for (decision, summary) in snapshot.gate_wait_ms_by_decision {
        render_summary(
            &mut out,
            "tandem_gate_wait_ms",
            &[("decision", decision.as_str())],
            &summary,
        );
    }
    for (decision, count) in snapshot.tool_call_decisions_total {
        render_metric_line(
            &mut out,
            "tandem_tool_call_decisions_total",
            &[("decision", decision.as_str())],
            count,
        );
    }
    for (key, count) in snapshot.provider_errors_total {
        let labels = parse_provider_error_key(&key);
        render_metric_line(
            &mut out,
            "tandem_provider_errors_total",
            &[
                ("provider_id", labels.provider_id.as_str()),
                ("error_code", labels.error_code.as_str()),
            ],
            count,
        );
    }
    out
}

fn track_pending_run(key: String, started_at_ms: u64) {
    let mut state = metrics_state()
        .lock()
        .expect("observability metrics mutex poisoned");
    trim_pending(&mut state.pending_runs);
    state.pending_runs.insert(key, started_at_ms);
}

fn take_pending_run(key: &str) -> Option<u64> {
    metrics_state()
        .lock()
        .expect("observability metrics mutex poisoned")
        .pending_runs
        .remove(key)
}

fn track_pending_gate(key: String, requested_at_ms: u64) {
    let mut state = metrics_state()
        .lock()
        .expect("observability metrics mutex poisoned");
    trim_pending(&mut state.pending_gates);
    state.pending_gates.insert(key, requested_at_ms);
}

fn take_pending_gate(key: &str) -> Option<u64> {
    metrics_state()
        .lock()
        .expect("observability metrics mutex poisoned")
        .pending_gates
        .remove(key)
}

fn trim_pending(map: &mut BTreeMap<String, u64>) {
    while map.len() >= MAX_TRACKED_PENDING {
        let Some(oldest) = map
            .iter()
            .min_by_key(|(_, timestamp_ms)| *timestamp_ms)
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        map.remove(&oldest);
    }
}

fn run_metric_key(properties: &Value) -> Option<String> {
    string_property(properties, &["runID", "runId", "run_id"])
        .or_else(|| string_property(properties, &["sessionID", "sessionId", "session_id"]))
        .map(ToString::to_string)
}

fn event_time_ms(properties: &Value) -> Option<u64> {
    properties
        .get("timestampMs")
        .or_else(|| properties.get("createdAtMs"))
        .or_else(|| properties.get("startedAtMs"))
        .or_else(|| properties.get("finishedAtMs"))
        .and_then(Value::as_u64)
}

fn string_property<'a>(properties: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| properties.get(*key).and_then(Value::as_str))
}

fn normalize_decision(value: &str) -> &'static str {
    match value.trim().to_ascii_lowercase().as_str() {
        "allow" | "allowed" | "always" | "approve" | "approved" => "allow",
        "ask" | "asked" | "pending" | "approval_required" => "ask",
        "deny" | "denied" | "reject" | "rejected" | "timeout" => "deny",
        _ => "unknown",
    }
}

fn safe_label(input: &str) -> String {
    let mut out = input
        .chars()
        .take(80)
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':') {
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

fn render_summary(out: &mut String, name: &str, labels: &[(&str, &str)], summary: &MetricSummary) {
    render_metric_line(out, &format!("{name}_count"), labels, summary.count);
    render_metric_line(out, &format!("{name}_sum"), labels, summary.sum_ms);
    render_metric_line(out, &format!("{name}_max"), labels, summary.max_ms);
}

fn render_metric_line(out: &mut String, name: &str, labels: &[(&str, &str)], value: u64) {
    out.push_str(name);
    if !labels.is_empty() {
        out.push('{');
        for (idx, (key, value)) in labels.iter().enumerate() {
            if idx > 0 {
                out.push(',');
            }
            out.push_str(key);
            out.push_str("=\"");
            out.push_str(&escape_label_value(value));
            out.push('"');
        }
        out.push('}');
    }
    out.push(' ');
    out.push_str(&value.to_string());
    out.push('\n');
}

fn escape_label_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('"', "\\\"")
}

struct ProviderErrorLabels {
    provider_id: String,
    error_code: String,
}

fn parse_provider_error_key(key: &str) -> ProviderErrorLabels {
    let mut provider_id = "unknown".to_string();
    let mut error_code = "unknown".to_string();
    for part in key.split(',') {
        if let Some(value) = part.strip_prefix("provider_id=") {
            provider_id = value.to_string();
        } else if let Some(value) = part.strip_prefix("error_code=") {
            error_code = value.to_string();
        }
    }
    ProviderErrorLabels {
        provider_id,
        error_code,
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) fn reset_observability_metrics_for_tests() {
    *metrics_state()
        .lock()
        .expect("observability metrics mutex poisoned") = MetricsState::default();
}
