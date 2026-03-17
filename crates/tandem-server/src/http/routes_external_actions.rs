use axum::routing::get;
use axum::Router;

use super::external_actions::*;
use crate::AppState;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/external-actions", get(list_external_actions))
        .route("/external-actions/{id}", get(get_external_action))
}
