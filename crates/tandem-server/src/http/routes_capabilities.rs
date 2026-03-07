use axum::routing::{get, post};
use axum::Router;

use super::*;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            "/capabilities/bindings",
            get(capabilities_bindings_get).put(capabilities_bindings_put),
        )
        .route(
            "/capabilities/bindings/refresh-builtins",
            post(capabilities_bindings_refresh_builtins),
        )
        .route(
            "/capabilities/bindings/reset-to-builtins",
            post(capabilities_bindings_reset_to_builtins),
        )
        .route("/capabilities/discovery", get(capabilities_discovery))
        .route("/capabilities/resolve", post(capabilities_resolve))
        .route("/capabilities/readiness", post(capabilities_readiness))
}
