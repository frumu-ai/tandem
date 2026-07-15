// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::routing::get;
use axum::Router;

use super::*;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/marketplace/catalog", get(marketplace_catalog))
        .route(
            "/marketplace/packs/{pack_id}/files/{*path}",
            get(marketplace_pack_file_get),
        )
}
