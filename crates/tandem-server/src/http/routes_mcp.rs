use axum::routing::{get, post};
use axum::Router;

use super::*;

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/mcp", get(list_mcp).post(add_mcp))
        .route("/mcp/{name}/connect", post(connect_mcp))
        .route("/mcp/{name}/disconnect", post(disconnect_mcp))
        .route(
            "/mcp/{name}",
            axum::routing::patch(patch_mcp).delete(delete_mcp),
        )
        .route("/mcp/{name}/refresh", post(refresh_mcp))
        .route("/mcp/{name}/auth", post(auth_mcp).delete(delete_auth_mcp))
        .route(
            "/mcp/{name}/auth/callback",
            get(callback_mcp_get).post(callback_mcp),
        )
        .route("/mcp/{name}/auth/authenticate", post(authenticate_mcp))
        .route("/mcp/catalog", get(mcp_discovery::mcp_catalog_index))
        .route(
            "/mcp/request-capability",
            post(mcp_discovery::mcp_request_capability),
        )
        .route(
            "/mcp/catalog/{slug}/toml",
            get(mcp_discovery::mcp_catalog_toml),
        )
        .route("/mcp/tools", get(mcp_tools))
        .route("/mcp/resources", get(mcp_resources))
}
