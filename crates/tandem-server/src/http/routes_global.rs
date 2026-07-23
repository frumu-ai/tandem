// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::routing::{get, post, put};
use axum::Router;

use crate::AppState;

use super::channels_api::{
    admin_reload_config, channel_scopes_get, channel_tool_preferences_get,
    channel_tool_preferences_put, channels_config, channels_delete, channels_put, channels_status,
    channels_verify,
};
use super::config_providers::{global_config, global_config_patch};
use super::global::*;

async fn enterprise_status() -> axum::Json<tandem_enterprise_contract::EnterpriseStatus> {
    use tandem_enterprise_contract::{EnterpriseBridge, NoopEnterpriseBridge};

    axum::Json(NoopEnterpriseBridge.status())
}

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/global/health", get(global_health))
        .route("/global/diagnostics", get(global_diagnostics))
        .route("/global/workspace", get(global_workspace))
        .route("/enterprise/status", get(enterprise_status))
        .route("/browser/status", get(browser_status))
        .route("/browser/install", post(browser_install))
        .route("/browser/smoke-test", post(browser_smoke_test))
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
        .route("/channels/{name}/scopes", get(channel_scopes_get))
        .route("/channels/{name}/verify", post(channels_verify))
        .route(
            "/channels/{name}",
            put(channels_put).delete(channels_delete),
        )
        .route(
            "/channels/{name}/tool-preferences",
            get(channel_tool_preferences_get).put(channel_tool_preferences_put),
        )
        .route(
            "/worktree",
            get(list_worktrees)
                .post(create_worktree)
                .delete(delete_worktree),
        )
        .route("/worktree/reset", post(reset_worktree))
        .route("/worktree/cleanup", post(cleanup_worktrees))
        .route("/agent", get(agent_list))
        .route("/instance/dispose", post(instance_dispose))
        .route("/log", post(push_log))
        .route("/doc", get(openapi_doc))
}
