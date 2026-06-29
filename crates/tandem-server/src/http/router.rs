use axum::http::{header, HeaderName, Method};
use axum::middleware as axum_middleware;
use axum::Router;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    limit::RequestBodyLimitLayer,
};

use super::*;

fn build_cors_layer() -> CorsLayer {
    let allowed_origins = std::env::var("TANDEM_CORS_ORIGINS")
        .unwrap_or_else(|_| {
            "http://localhost:5173,http://localhost:3000,http://localhost:8080,http://127.0.0.1,https://localhost,tauri://".to_string()
        });

    let origins: Vec<String> = allowed_origins
        .split(',')
        .map(|s| s.trim().to_string())
        .collect();

    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin, _request_parts| {
            if let Ok(origin_str) = origin.to_str() {
                origins
                    .iter()
                    .any(|allowed| origin_matches_allowed(origin_str, allowed))
            } else {
                false
            }
        }))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::PATCH,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::AUTHORIZATION,
            HeaderName::from_static("x-tandem-correlation-id"),
            HeaderName::from_static("x-tandem-org-id"),
            HeaderName::from_static("x-tandem-workspace-id"),
            HeaderName::from_static("x-tandem-actor-id"),
            HeaderName::from_static("x-tandem-request-source"),
        ])
}

fn origin_matches_allowed(origin: &str, allowed: &str) -> bool {
    let allowed = allowed.trim();
    if allowed.is_empty() {
        return false;
    }
    if allowed == origin {
        return true;
    }

    if let Some(domain) = allowed.strip_prefix("https://*.") {
        return origin_host_matches_wildcard(origin, "https", domain);
    }
    if let Some(domain) = allowed.strip_prefix("http://*.") {
        return origin_host_matches_wildcard(origin, "http", domain);
    }

    let Ok(origin_url) = reqwest::Url::parse(origin) else {
        return allowed.ends_with("://") && origin.starts_with(allowed);
    };
    let Ok(allowed_url) = reqwest::Url::parse(allowed) else {
        return allowed.ends_with("://") && origin.starts_with(allowed);
    };
    if origin_url.scheme() != allowed_url.scheme() {
        return false;
    }
    if origin_url.host_str() != allowed_url.host_str() {
        return false;
    }
    match allowed_url.port() {
        Some(port) => origin_url.port() == Some(port),
        None => true,
    }
}

fn origin_host_matches_wildcard(origin: &str, scheme: &str, domain: &str) -> bool {
    let Ok(origin_url) = reqwest::Url::parse(origin) else {
        return false;
    };
    if origin_url.scheme() != scheme {
        return false;
    }
    let Some(host) = origin_url.host_str() else {
        return false;
    };
    host != domain && host.ends_with(&format!(".{domain}"))
}

pub(super) fn build_router(state: AppState, route_extensions: &[super::RouteRegistrar]) -> Router {
    let cors = build_cors_layer();
    let body_limit = RequestBodyLimitLayer::new(10 * 1024 * 1024);

    let mut router: Router<AppState> = Router::new();

    router = super::routes_approvals::apply(router);
    router = super::routes_automation_webhooks::apply(router);
    router = router.route(
        "/audit/protected",
        axum::routing::get(super::audit_stream::protected_audit_events),
    );
    router = router.route(
        "/audit/stream",
        axum::routing::get(super::audit_stream::audit_stream),
    );
    router = router.route(
        "/audit/ledger/manifest",
        axum::routing::get(super::audit_stream::audit_ledger_manifest),
    );
    router = router.route(
        "/audit/ledger/export",
        axum::routing::get(super::audit_stream::audit_ledger_export),
    );
    router = router.route(
        "/metrics",
        axum::routing::get(super::observability_metrics::prometheus_metrics),
    );
    router = router.route(
        "/channels/enroll",
        axum::routing::post(super::channel_enrollment::channel_enroll),
    );
    router = router.route(
        "/channels/step-up",
        axum::routing::post(super::channel_enrollment::channel_step_up),
    );
    router = router.route(
        "/channels/slack/interactions",
        axum::routing::post(super::slack_interactions::slack_interactions),
    );
    router = router.route(
        "/channels/discord/interactions",
        axum::routing::post(super::discord_interactions::discord_interactions),
    );
    router = router.route(
        "/channels/telegram/interactions",
        axum::routing::post(super::telegram_interactions::telegram_interactions),
    );
    router = super::routes_coder::apply(router);
    router = super::routes_context::apply(router);
    router = super::routes_sessions::apply(router);
    router = router.route(
        "/runs/{run_id}/events",
        axum::routing::get(super::runtime_events::get_run_events),
    );
    router = router
        .route(
            "/stateful-runtime/runs",
            axum::routing::get(super::stateful_runtime_api::list_stateful_runs),
        )
        .route(
            "/stateful-runtime/runs/{run_id}",
            axum::routing::get(super::stateful_runtime_api::get_stateful_run),
        )
        .route(
            "/stateful-runtime/runs/{run_id}/events",
            axum::routing::get(super::stateful_runtime_api::get_stateful_run_events),
        )
        .route(
            "/stateful-runtime/runs/{run_id}/snapshots",
            axum::routing::get(super::stateful_runtime_api::list_stateful_run_snapshots),
        )
        .route(
            "/stateful-runtime/runs/{run_id}/snapshots/{snapshot_id}",
            axum::routing::get(super::stateful_runtime_api::get_stateful_run_snapshot),
        );
    router = super::routes_bug_monitor::apply(router);
    router = super::routes_external_actions::apply(router);
    router = super::routes_goal_capability_learning::apply(router);
    // ensure modules wired exactly once
    // routes_mcp already applied above
    router = super::routes_skills_memory::apply(router);
    router = super::routes_missions_teams::apply(router);
    router = super::routes_mission_builder::apply(router);
    router = super::routes_optimizations::apply(router);
    router = super::routes_config_providers::apply(router);
    router = super::routes_system_api::apply(router);
    router = super::routes_channel_automation_drafts::apply(router);
    router = super::routes_routines_automations::apply(router);
    router = super::routes_automation_webhook_management::apply(router);
    router = super::routes_governance::apply(router);
    router = super::routes_permissions_questions::apply(router);
    router = super::routes_resources::apply(router);
    router = super::routes_capabilities::apply(router);
    router = super::routes_mcp::apply(router);
    router = super::routes_presets::apply(router);
    router = super::routes_pack_builder::apply(router);
    router = super::routes_marketplace::apply(router);
    router = super::routes_packs::apply(router);
    router = super::routes_task_intake::apply(router);
    router = super::routes_workflow_planner::apply(router);
    router = super::routes_workflows::apply(router);
    router = super::routes_setup_understanding::apply(router);
    router = super::routes_global::apply(router);

    for route_extension in route_extensions {
        router = route_extension(router);
    }

    if state.web_ui_enabled() {
        router = router.merge(crate::webui::web_ui_router(&state.web_ui_prefix()));
    }

    router
        .layer(cors)
        .layer(body_limit)
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            super::middleware::startup_gate,
        ))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            super::middleware::auth_gate,
        ))
        .with_state(state)
}
