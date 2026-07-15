// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::routing::{get, post};
use axum::Router;

use super::*;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/pack-builder/preview", post(pack_builder_preview))
        .route("/pack-builder/apply", post(pack_builder_apply))
        .route("/pack-builder/cancel", post(pack_builder_cancel))
        .route("/pack-builder/pending", get(pack_builder_pending))
}
