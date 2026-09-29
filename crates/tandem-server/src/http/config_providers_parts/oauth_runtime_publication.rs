// OAuth runtime publication shares the config-providers module's imports and
// caller authority types. Keep authorization and the registry commit together.

async fn clear_openai_codex_runtime_token_for_caller(
    state: &AppState,
    tenant_context: &TenantContext,
    previous_token: &tandem_providers::TenantProviderBearerTokenSnapshot,
    caller: OAuthRefreshCaller<'_>,
) -> anyhow::Result<bool> {
    state
        .providers
        .clear_tenant_provider_bearer_token_if_unchanged_guarded(
            tenant_context,
            OPENAI_CODEX_PROVIDER_ID,
            previous_token,
            |commit| {
                state
                    .enterprise
                    .hosted_policy
                    .with_current_policy(|policy| {
                        caller.require_use_under_policy(tenant_context, policy)?;
                        commit()
                    })
                    .map_err(anyhow::Error::msg)?
            },
        )
        .await
}

async fn publish_openai_codex_runtime_token_for_caller(
    state: &AppState,
    tenant_context: &TenantContext,
    runtime_token: String,
    caller: OAuthRefreshCaller<'_>,
) -> anyhow::Result<()> {
    state
        .providers
        .set_tenant_provider_bearer_token_guarded(
            tenant_context,
            OPENAI_CODEX_PROVIDER_ID,
            runtime_token,
            |commit| {
                state
                    .enterprise
                    .hosted_policy
                    .with_current_policy(|policy| {
                        caller.require_use_under_policy(tenant_context, policy)?;
                        commit()
                    })
                    .map_err(anyhow::Error::msg)?
            },
        )
        .await
}

/// A credential can be deleted or replaced while provider loading awaits.
/// Re-read it under the bearer registry write lock so a stale load cannot
/// publish after a completed delete or overwrite. The admin delete path
/// clears the registry after its durable write, so a delete racing after
/// this comparison still removes this publication.
async fn publish_loaded_openai_codex_runtime_token_for_caller(
    state: &AppState,
    tenant_context: &TenantContext,
    loaded_credential: &tandem_core::OAuthProviderCredential,
    runtime_token: String,
    caller: OAuthRefreshCaller<'_>,
) -> anyhow::Result<bool> {
    let mut stale_credential = false;
    let publication = state
        .providers
        .set_tenant_provider_bearer_token_guarded(
            tenant_context,
            OPENAI_CODEX_PROVIDER_ID,
            runtime_token,
            |commit| {
                let credential_is_current = openai_codex_oauth_credential(state, tenant_context)
                    .as_ref()
                    == Some(loaded_credential);
                state
                    .enterprise
                    .hosted_policy
                    .with_current_policy(|policy| {
                        caller.require_use_under_policy(tenant_context, policy)?;
                        if !credential_is_current {
                            stale_credential = true;
                            anyhow::bail!(
                                "OAuth credential changed while runtime hydration was in flight"
                            );
                        }
                        commit()
                    })
                    .map_err(anyhow::Error::msg)?
            },
        )
        .await;
    if stale_credential {
        Ok(false)
    } else {
        publication.map(|()| true)
    }
}

#[cfg(test)]
pub(crate) async fn publish_loaded_openai_codex_runtime_token_for_request_test(
    state: &AppState,
    tenant_context: &TenantContext,
    verified: &tandem_types::VerifiedTenantContext,
    loaded_credential: &tandem_core::OAuthProviderCredential,
    runtime_token: String,
) -> anyhow::Result<bool> {
    publish_loaded_openai_codex_runtime_token_for_caller(
        state,
        tenant_context,
        loaded_credential,
        runtime_token,
        OAuthRefreshCaller::Http(Some(verified)),
    )
    .await
}

#[cfg(test)]
pub(crate) async fn publish_openai_codex_runtime_token_for_request_test(
    state: &AppState,
    tenant_context: &TenantContext,
    verified: &tandem_types::VerifiedTenantContext,
    runtime_token: String,
) -> anyhow::Result<()> {
    publish_openai_codex_runtime_token_for_caller(
        state,
        tenant_context,
        runtime_token,
        OAuthRefreshCaller::Http(Some(verified)),
    )
    .await
}
