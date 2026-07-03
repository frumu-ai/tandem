use super::*;

struct EnvGuard {
    name: &'static str,
    previous: Option<String>,
}

impl EnvGuard {
    fn set(name: &'static str, value: Option<&str>) -> Self {
        let previous = std::env::var(name).ok();
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
        Self { name, previous }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        match self.previous.as_deref() {
            Some(value) => std::env::set_var(self.name, value),
            None => std::env::remove_var(self.name),
        }
    }
}

#[tokio::test]
#[serial_test::serial(observability_metrics_env)]
async fn metrics_route_is_disabled_by_default() {
    let _guard = EnvGuard::set("TANDEM_OBSERVABILITY_PROMETHEUS_ENABLED", None);
    let state = test_state().await;
    let app = app_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
#[serial_test::serial(observability_metrics_env)]
async fn metrics_route_renders_prometheus_when_enabled() {
    let _guard = EnvGuard::set("TANDEM_OBSERVABILITY_PROMETHEUS_ENABLED", Some("true"));
    tandem_observability::record_scheduler_tick_latency_ms(7);
    tandem_observability::record_scheduler_clock_regression_ms(250);
    tandem_observability::record_tool_call_decision("allow");
    let state = test_state().await;
    let app = app_router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let body = String::from_utf8(body.to_vec()).expect("utf8");
    assert!(body.contains("tandem_scheduler_active_runs"));
    assert!(body.contains("tandem_scheduler_tick_latency_ms_count"));
    assert!(body.contains("tandem_scheduler_clock_regressions_total"));
    assert!(body.contains("tandem_scheduler_clock_regression_ms_count"));
    assert!(body.contains("tandem_tool_call_decisions_total{decision=\"allow\"}"));
}
