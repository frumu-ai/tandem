use axum::extract::State;
use axum::http::header;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};

use crate::app::state::automation::scheduler::AutomationSchedulerMetrics;
use crate::AppState;

pub(super) async fn prometheus_metrics(State(state): State<AppState>) -> Response {
    if !crate::config::env::prometheus_metrics_enabled() {
        return StatusCode::NOT_FOUND.into_response();
    }

    let scheduler = state.automation_scheduler.read().await.metrics();
    let mut body = render_scheduler_metrics(&scheduler);
    body.push_str(&tandem_observability::render_observability_metrics_prometheus());

    (
        [(
            header::CONTENT_TYPE,
            "text/plain; version=0.0.4; charset=utf-8",
        )],
        body,
    )
        .into_response()
}

fn render_scheduler_metrics(metrics: &AutomationSchedulerMetrics) -> String {
    let mut out = String::new();
    render_metric_line(
        &mut out,
        "tandem_scheduler_active_runs",
        &[],
        metrics.active_runs as u64,
    );
    render_metric_line(
        &mut out,
        "tandem_scheduler_admitted_total",
        &[],
        metrics.admitted_total,
    );
    render_metric_line(
        &mut out,
        "tandem_scheduler_completed_total",
        &[],
        metrics.completed_total,
    );
    render_metric_line(
        &mut out,
        "tandem_scheduler_queue_wait_ms_avg",
        &[],
        metrics.avg_wait_ms,
    );
    render_metric_line(
        &mut out,
        "tandem_scheduler_queue_wait_ms_p95",
        &[],
        metrics.p95_wait_ms,
    );
    for (reason, count) in &metrics.queued_runs_by_reason {
        render_metric_line(
            &mut out,
            "tandem_scheduler_queued_runs",
            &[("reason", reason.as_str())],
            *count as u64,
        );
    }
    out
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
