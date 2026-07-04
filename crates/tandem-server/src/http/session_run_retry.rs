//! Transparent provider-auth recovery for engine runs.
//!
//! An expired OpenAI Codex OAuth access token surfaces mid-run as an
//! `AUTHENTICATION_ERROR`. Historically that error was relayed straight to the
//! caller (a Telegram/Discord/Slack user), because the token was only ever
//! refreshed as a side effect of the control panel polling `GET /provider/auth`
//! (TAN-593). This module wraps a single engine run so that, on such a failure,
//! the credential is refreshed and the run is retried exactly once before the
//! error is surfaced (TAN-595).
//!
//! The wrapper is a plain future: the caller's timeout and liveness ticker in
//! `execute_run` poll it directly, so the (at most one) retry shares the same
//! overall run budget and keeps emitting heartbeats.

use serde_json::json;

use super::sessions::{dispatch_error_code, publish_tenant_event};
use crate::http::AppState;
use tandem_types::{ModelSpec, SendMessageRequest, TenantContext};

const OPENAI_CODEX_PROVIDER_ID: &str = "openai-codex";

/// Whether an `AUTHENTICATION_ERROR` for this run is worth a transparent
/// refresh-and-retry.
///
/// True only when the run actually targets the Codex OAuth provider *and* a
/// refreshable credential exists. Resolving the run's provider mirrors the
/// engine's own route resolution (`resolve_model_route`): the request model's
/// provider wins, otherwise the session model's. Gating on the resolved
/// provider matters because a tenant can have a saved Codex OAuth credential
/// while a given run uses an unrelated API-key provider (Anthropic,
/// OpenRouter, …) — a bad key there must not trigger a Codex refresh and a
/// wasted rerun.
fn run_targets_codex_oauth(
    tenant_context: &TenantContext,
    request_model: Option<&ModelSpec>,
    session_model: Option<&ModelSpec>,
) -> bool {
    let provider_id = request_model
        .map(|model| model.provider_id.trim())
        .filter(|provider| !provider.is_empty())
        .or_else(|| {
            session_model
                .map(|model| model.provider_id.trim())
                .filter(|provider| !provider.is_empty())
        });
    if provider_id != Some(OPENAI_CODEX_PROVIDER_ID) {
        return false;
    }
    tandem_core::load_provider_oauth_credential_for_tenant(tenant_context, OPENAI_CODEX_PROVIDER_ID)
        .is_some()
}

/// Run one engine prompt, transparently recovering from an expired provider
/// token. On an `AUTHENTICATION_ERROR` backed by a refreshable Codex OAuth
/// credential, refresh + reload providers and retry the run once before
/// propagating the failure.
pub(super) async fn run_prompt_with_auth_retry(
    state: &AppState,
    session_id: &str,
    run_id: &str,
    req: SendMessageRequest,
    correlation_id: Option<String>,
    tenant_context: &TenantContext,
) -> anyhow::Result<()> {
    let retry_req = req.clone();
    let first = state
        .engine_loop
        .run_prompt_async_with_context(session_id.to_string(), req, correlation_id.clone())
        .await;
    let err = match first {
        Ok(()) => return Ok(()),
        Err(err) => err,
    };

    if dispatch_error_code(&err.to_string()) != "AUTHENTICATION_ERROR" {
        return Err(err);
    }
    // Only retry when this run actually targets the Codex OAuth provider.
    let session_model = state
        .storage
        .get_session(session_id)
        .await
        .and_then(|session| session.model);
    if !run_targets_codex_oauth(
        tenant_context,
        retry_req.model.as_ref(),
        session_model.as_ref(),
    ) {
        return Err(err);
    }

    tracing::info!(
        session_id = %session_id,
        run_id = %run_id,
        "AUTHENTICATION_ERROR during run — refreshing Codex OAuth and retrying once"
    );
    publish_tenant_event(
        state,
        tenant_context,
        "session.auth.refresh_retry",
        json!({ "sessionID": session_id, "runID": run_id }),
    );
    if let Err(refresh_err) =
        crate::http::config_providers::refresh_openai_codex_oauth_if_needed(state, tenant_context)
            .await
    {
        tracing::warn!(
            session_id = %session_id,
            run_id = %run_id,
            error = %refresh_err,
            "Codex OAuth refresh before retry failed"
        );
    }

    state
        .engine_loop
        .run_prompt_async_with_context(session_id.to_string(), retry_req, correlation_id)
        .await
}
