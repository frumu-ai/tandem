use axum::routing::{get, post};
use axum::Router;

use super::*;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/packs", get(packs_list))
        .route("/packs/{selector}", get(packs_get))
        .route("/packs/{selector}/files/{*path}", get(packs_file_get))
        .route("/packs/install", post(packs_install))
        .route(
            "/packs/install_from_attachment",
            post(packs_install_from_attachment),
        )
        .route("/packs/uninstall", post(packs_uninstall))
        .route("/packs/export", post(packs_export))
        .route("/packs/detect", post(packs_detect))
        .route("/packs/{selector}/updates", get(packs_updates_get))
        .route("/packs/{selector}/update", post(packs_update_post))
}
