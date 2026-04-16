use super::config_providers::*;
use crate::http::AppState;
use axum::{
    routing::{delete, get, post, put},
    Router,
};

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/config", get(get_config).patch(patch_config))
        .route(
            "/config/identity",
            get(get_config_identity).patch(patch_config_identity),
        )
        .route("/config/providers", get(config_providers))
        .route("/provider", get(list_providers))
        .route("/providers", get(list_providers_legacy))
        .route("/api/providers", get(list_providers_legacy))
        .route("/provider/auth", get(provider_auth))
        .route(
            "/provider/{id}/oauth/authorize",
            post(provider_oauth_authorize),
        )
        .route("/provider/{id}/oauth/status", get(provider_oauth_status))
        .route(
            "/provider/{id}/oauth/callback",
            get(provider_oauth_callback_get).post(provider_oauth_callback_post),
        )
        .route(
            "/provider/{id}/oauth/session/local",
            post(provider_oauth_local_session),
        )
        .route(
            "/provider/{id}/oauth/session",
            delete(provider_oauth_disconnect),
        )
        .route("/auth/{id}", put(set_auth).delete(delete_auth))
        .route("/auth/token", put(set_api_token).delete(clear_api_token))
        .route("/auth/token/generate", post(generate_api_token))
}
