use axum::middleware as axum_middleware;
use axum::Router;
use tower_http::cors::{Any, CorsLayer};

use super::*;

pub(super) fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let mut router: Router<AppState> = Router::new();

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
    router = super::routes_routines_automations::apply(router);
    router = super::routes_permissions_questions::apply(router);
    router = super::routes_resources::apply(router);
    router = super::routes_capabilities::apply(router);
    router = super::routes_mcp::apply(router);
    router = super::routes_presets::apply(router);
    router = super::routes_pack_builder::apply(router);
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
