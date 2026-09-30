// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::HashSet;

use axum::http::StatusCode;

// Credential deletion takes a native file lock. Await the complete purge off
// the async executor before callers restart listeners against the new config.
pub(super) async fn purge_selected(secret_ids: HashSet<String>) -> Result<(), StatusCode> {
    crate::encrypted_file_store::spawn_protected_blocking(move || {
        for secret_id in secret_ids {
            let _ = tandem_core::delete_provider_auth(&secret_id);
        }
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub(super) async fn purge_deleted(channel: &'static str) -> Result<(), StatusCode> {
    crate::encrypted_file_store::spawn_protected_blocking(move || {
        if let Some(secret_id) = tandem_core::channel_secret_store_id(channel) {
            let _ = tandem_core::delete_provider_auth(&secret_id);
        }
        if channel == "slack" {
            tandem_core::purge_slack_channel_secrets();
        }
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[cfg(test)]
#[path = "channel_credential_purge_tests.rs"]
mod tests;
