// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::routing::{get, post, put};
use axum::Router;

use super::*;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/presets/index", get(presets_index))
        .route("/presets/compose/preview", post(presets_compose_preview))
        .route("/presets/fork", post(presets_fork))
        .route(
            "/presets/overrides/{kind}/{id}",
            put(presets_override_put).delete(presets_override_delete),
        )
        .route(
            "/presets/capability_summary",
            post(presets_capability_summary),
        )
        .route("/presets/export_overrides", post(presets_export_overrides))
}
