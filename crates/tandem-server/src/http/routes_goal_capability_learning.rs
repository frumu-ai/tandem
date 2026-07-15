// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

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
            "/goal-capability-learning/decisions/{decision_id}",
            get(super::goal_capability_learning::get_discovery_decision),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Building the router must not panic. Guards against the axum 0.7+ path
    /// capture syntax regressing back to `:param` (which panics at construction).
    #[test]
    fn routes_build_without_panicking() {
        let _router: Router<AppState> = apply(Router::new());
    }
}
