use axum::routing::{delete, get, patch, post, put};
use axum::Router;

use crate::AppState;

use super::channels_api::{
    admin_reload_config, channels_config, channels_delete, channels_put, channels_status,
    channels_verify,
};
use super::config_providers::{global_config, global_config_patch};
use super::global::*;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/global/health", get(global_health))
        .route("/browser/status", get(browser_status))
        .route("/global/event", get(events))
        .route("/event", get(events))
        .route("/global/lease/acquire", post(global_lease_acquire))
        .route("/global/lease/renew", post(global_lease_renew))
        .route("/global/lease/release", post(global_lease_release))
        .route("/global/storage/files", get(global_storage_files))
        .route("/global/storage/repair", post(global_storage_repair))
        .route(
            "/global/config",
            get(global_config).patch(global_config_patch),
        )
        .route("/global/dispose", post(global_dispose))
        .route("/admin/reload-config", post(admin_reload_config))
        .route("/tool/ids", get(tool_ids))
        .route("/tool", get(tool_list_for_model))
        .route("/tool/execute", post(execute_tool))
        .route("/run/{id}/events", get(run_events))
        .route("/api/run/{id}/events", get(run_events))
        .route("/project", get(list_projects))
        .route("/channels/config", get(channels_config))
        .route("/channels/status", get(channels_status))
        .route("/channels/{name}/verify", post(channels_verify))
        .route(
            "/channels/{name}",
            put(channels_put).delete(channels_delete),
        )
        .route(
            "/worktree",
            get(list_worktrees)
                .post(create_worktree)
                .delete(delete_worktree),
        )
        .route("/worktree/reset", post(reset_worktree))
        .route("/agent", get(agent_list))
        .route("/instance/dispose", post(instance_dispose))
        .route("/log", post(push_log))
        .route("/doc", get(openapi_doc))
}
