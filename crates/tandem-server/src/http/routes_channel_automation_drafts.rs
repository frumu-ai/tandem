use axum::routing::{get, post};
use axum::Router;

use super::channel_automation_drafts::*;
use crate::AppState;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            "/automations/channel-drafts",
            post(channel_automation_drafts_start),
        )
        .route(
            "/automations/channel-drafts/pending",
            get(channel_automation_drafts_pending),
        )
        .route(
            "/automations/channel-drafts/{draft_id}/answer",
            post(channel_automation_drafts_answer),
        )
        .route(
            "/automations/channel-drafts/{draft_id}/confirm",
            post(channel_automation_drafts_confirm),
        )
        .route(
            "/automations/channel-drafts/{draft_id}/cancel",
            post(channel_automation_drafts_cancel),
        )
}
