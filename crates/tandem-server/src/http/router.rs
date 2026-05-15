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
                origins.iter().any(|allowed| {
                    if allowed.ends_with("*") {
                        let prefix = &allowed[..allowed.len() - 1];
                        origin_str.starts_with(prefix)
                    } else {
                        origin_str == allowed || origin_str.starts_with(&format!("{}:", allowed))
                    }
                })
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

pub(super) fn build_router(state: AppState) -> Router {
    let cors = build_cors_layer();
    let body_limit = RequestBodyLimitLayer::new(10 * 1024 * 1024);

    let mut router: Router<AppState> = Router::new();

    router = super::routes_approvals::apply(router);
    router = router.route(
        "/audit/stream",
        axum::routing::get(super::audit_stream::audit_stream),
    );
    router = router.route(
        "/channels/enroll",
        axum::routing::post(super::channel_enrollment::channel_enroll),
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
    router = super::routes_bug_monitor::apply(router);
    router = super::routes_external_actions::apply(router);
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
