// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::routing::post;
use axum::Router;

use super::mission_builder::*;
use crate::http::AppState;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            "/mission-builder/generate-draft",
            post(mission_builder_generate_draft),
        )
        .route(
            "/mission-builder/compile-preview",
            post(mission_builder_preview),
        )
        .route("/mission-builder/apply", post(mission_builder_apply))
}
