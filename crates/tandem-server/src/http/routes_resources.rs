// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::routing::get;
use axum::Router;

use super::*;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/resource", get(resource_list))
        .route("/resource/events", get(resource_events))
        .route(
            "/resource/{*key}",
            get(resource_get)
                .put(resource_put)
                .patch(resource_patch)
                .delete(resource_delete),
        )
}
