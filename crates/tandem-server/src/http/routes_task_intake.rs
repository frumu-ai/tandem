use axum::routing::post;
use axum::Router;

use super::task_intake::*;
use crate::AppState;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router.route("/task-intake/preview", post(task_intake_preview))
}
