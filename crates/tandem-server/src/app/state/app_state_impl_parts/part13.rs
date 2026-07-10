// Provider OAuth credential upkeep for AppState.
//
// Included into `app/state/mod.rs`, so it shares that module's imports.

const PROVIDER_OAUTH_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);
const PROVIDER_OAUTH_REFRESH_TENANT_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(30);
const PROVIDER_OAUTH_REFRESH_CONCURRENCY: usize = 4;

#[derive(Debug, Default, PartialEq, Eq)]
struct ProviderOAuthRefreshSweep {
    succeeded: usize,
    failed: usize,
    timed_out: usize,
    cancelled: usize,
}

async fn refresh_provider_oauth_tenants_bounded<Refresh, RefreshFuture, OnTimeout>(
    tenants: Vec<tandem_types::TenantContext>,
    cancel: tokio_util::sync::CancellationToken,
    tenant_timeout: std::time::Duration,
    concurrency: usize,
    refresh: Refresh,
    on_timeout: OnTimeout,
) -> ProviderOAuthRefreshSweep
where
    Refresh: Fn(tandem_types::TenantContext) -> RefreshFuture + Clone,
    RefreshFuture: std::future::Future<Output = anyhow::Result<()>>,
    OnTimeout: Fn(&tandem_types::TenantContext) + Clone,
{
    use futures::StreamExt;

    enum Outcome {
        Succeeded,
        Failed,
        TimedOut,
        Cancelled,
    }

    let mut outcomes = futures::stream::iter(tenants.into_iter().map(|tenant| {
        let cancel = cancel.clone();
        let refresh = refresh.clone();
        let on_timeout = on_timeout.clone();
        async move {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => Outcome::Cancelled,
                result = tokio::time::timeout(tenant_timeout, refresh(tenant.clone())) => {
                    match result {
                        Ok(Ok(())) => Outcome::Succeeded,
                        Ok(Err(error)) => {
                            tracing::warn!(
                                target: "tandem_server::provider_oauth_refresh",
                                org_id = %tenant.org_id,
                                workspace_id = %tenant.workspace_id,
                                deployment_id = tenant.deployment_id.as_deref().unwrap_or(""),
                                failure_code = crate::http::config_providers::openai_codex_oauth_refresh_failure_code(&error),
                                "background Codex OAuth refresh failed"
                            );
                            Outcome::Failed
                        }
                        Err(_) => {
                            on_timeout(&tenant);
                            tracing::warn!(
                                target: "tandem_server::provider_oauth_refresh",
                                org_id = %tenant.org_id,
                                workspace_id = %tenant.workspace_id,
                                deployment_id = tenant.deployment_id.as_deref().unwrap_or(""),
                                failure_code = "refresh_timeout",
                                timeout_ms = tenant_timeout.as_millis(),
                                "background Codex OAuth refresh timed out"
                            );
                            Outcome::TimedOut
                        }
                    }
                }
            }
        }
    }))
    .buffer_unordered(concurrency.max(1));

    let mut summary = ProviderOAuthRefreshSweep::default();
    while let Some(outcome) = outcomes.next().await {
        match outcome {
            Outcome::Succeeded => summary.succeeded += 1,
            Outcome::Failed => summary.failed += 1,
            Outcome::TimedOut => summary.timed_out += 1,
            Outcome::Cancelled => summary.cancelled += 1,
        }
    }
    summary
}

impl AppState {
    /// Start one tracked refresh worker. Repeated runtime initialization cannot
    /// create duplicate loops, and graceful shutdown cancels in-flight tenant
    /// requests before joining the task.
    pub fn spawn_provider_oauth_refresh(&self) {
        let state = self.clone();
        self.oauth.spawn_provider_refresh_task(move |cancel| {
            tokio::spawn(async move {
                state
                    .refresh_provider_oauth_once_with_cancel(cancel.clone())
                    .await;
                let mut ticker = tokio::time::interval(PROVIDER_OAUTH_REFRESH_INTERVAL);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                // Consume the interval's immediate first tick; startup already
                // completed an explicit refresh pass above.
                ticker.tick().await;
                loop {
                    tokio::select! {
                        biased;
                        _ = cancel.cancelled() => return,
                        _ = ticker.tick() => {
                            state
                                .refresh_provider_oauth_once_with_cancel(cancel.clone())
                                .await;
                        }
                    }
                }
            })
        });
    }

    pub async fn stop_provider_oauth_refresh(&self) {
        self.oauth.stop_provider_refresh_task().await;
    }

    pub(crate) async fn refresh_provider_oauth_once(&self) {
        self.refresh_provider_oauth_once_with_cancel(tokio_util::sync::CancellationToken::new())
            .await;
    }

    async fn refresh_provider_oauth_once_with_cancel(
        &self,
        cancel: tokio_util::sync::CancellationToken,
    ) {
        let tenants = crate::http::config_providers::openai_codex_refreshable_tenants(self);
        let state = self.clone();
        let _ = refresh_provider_oauth_tenants_bounded(
            tenants,
            cancel,
            PROVIDER_OAUTH_REFRESH_TENANT_TIMEOUT,
            PROVIDER_OAUTH_REFRESH_CONCURRENCY,
            move |tenant| {
                let state = state.clone();
                async move {
                    crate::http::config_providers::refresh_openai_codex_oauth_if_needed(
                        &state, &tenant,
                    )
                    .await
                }
            },
            {
                let state = self.clone();
                move |tenant| {
                    state.event_bus.publish(crate::EngineEvent::new(
                        "provider.oauth.refresh.failed",
                        serde_json::json!({
                            "providerID": "openai-codex",
                            "tenantContext": tenant,
                            "refreshMode": "proactive",
                            "failureCode": "refresh_timeout",
                            "occurredAtMs": crate::now_ms(),
                        }),
                    ));
                }
            },
        )
        .await;
    }
}

#[cfg(test)]
mod provider_oauth_refresh_worker_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn tenant(id: &str) -> tandem_types::TenantContext {
        tandem_types::TenantContext::explicit(format!("org-{id}"), format!("workspace-{id}"), None)
    }

    #[tokio::test]
    async fn stalled_tenant_times_out_without_blocking_other_tenants() {
        let completed = Arc::new(AtomicUsize::new(0));
        let timeouts = Arc::new(AtomicUsize::new(0));
        let summary = refresh_provider_oauth_tenants_bounded(
            vec![tenant("stalled"), tenant("a"), tenant("b")],
            tokio_util::sync::CancellationToken::new(),
            std::time::Duration::from_millis(50),
            2,
            {
                let completed = completed.clone();
                move |tenant| {
                    let completed = completed.clone();
                    async move {
                        if tenant.org_id == "org-stalled" {
                            std::future::pending::<()>().await;
                        }
                        completed.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    }
                }
            },
            {
                let timeouts = timeouts.clone();
                move |_| {
                    timeouts.fetch_add(1, Ordering::SeqCst);
                }
            },
        )
        .await;
        assert_eq!(completed.load(Ordering::SeqCst), 2);
        assert_eq!(summary.succeeded, 2);
        assert_eq!(summary.timed_out, 1);
        assert_eq!(timeouts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn shutdown_cancellation_drops_in_flight_tenant_refresh() {
        let cancel = tokio_util::sync::CancellationToken::new();
        let task = tokio::spawn(refresh_provider_oauth_tenants_bounded(
            vec![tenant("stalled")],
            cancel.clone(),
            std::time::Duration::from_secs(60),
            1,
            |_| async {
                std::future::pending::<()>().await;
                Ok(())
            },
            |_| {},
        ));
        tokio::task::yield_now().await;
        cancel.cancel();
        let summary = tokio::time::timeout(std::time::Duration::from_secs(1), task)
            .await
            .expect("cancelled sweep should join promptly")
            .expect("sweep task");
        assert_eq!(summary.cancelled, 1);
    }

    #[tokio::test]
    async fn cancellation_stops_and_joins_tracked_refresh_worker() {
        let state = AppState::new_starting("oauth-refresh-worker-test".to_string(), true);
        state.spawn_provider_oauth_refresh();
        assert!(state.oauth.provider_refresh_task_is_running());
        state.stop_provider_oauth_refresh().await;
        assert!(!state.oauth.provider_refresh_task_is_running());
    }
}
