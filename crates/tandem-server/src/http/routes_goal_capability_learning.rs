use axum::routing::{get, post};
use axum::Router;

use crate::AppState;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            "/goal-capability-learning/discover",
            post(super::goal_capability_learning::discover_goal_capabilities),
        )
        .route(
            "/goal-capability-learning/decisions",
            get(super::goal_capability_learning::list_discovery_decisions),
        )
        .route(
            "/goal-capability-learning/decisions/:decision_id",
            get(super::goal_capability_learning::get_discovery_decision),
        )
}
