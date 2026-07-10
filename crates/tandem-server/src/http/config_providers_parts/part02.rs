use anyhow::Context as _;

fn stop_openai_codex_local_callback_server() {
    if let Ok(mut guard) = openai_codex_local_callback_shutdown_slot().lock() {
        if let Some(tx) = guard.take() {
            let _ = tx.send(());
        }
    }
}

async fn ensure_openai_codex_local_callback_server(state: AppState) -> anyhow::Result<()> {
    if openai_codex_local_callback_shutdown_slot()
        .lock()
        .map(|guard| guard.is_some())
        .unwrap_or(false)
    {
        return Ok(());
    }

    let listener = TcpListener::bind(OPENAI_CODEX_LOCAL_CALLBACK_ADDR).await?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    {
        let mut guard = openai_codex_local_callback_shutdown_slot()
            .lock()
            .map_err(|_| anyhow::anyhow!("codex callback server lock poisoned"))?;
        if guard.is_some() {
            return Ok(());
        }
        *guard = Some(shutdown_tx);
    }

    let app = Router::new()
        .route(
            "/auth/callback",
            get(openai_codex_local_callback_get).post(openai_codex_local_callback_post),
        )
        .with_state(state);

    tokio::spawn(async move {
        let result = axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                tokio::select! {
                    _ = shutdown_rx => {},
                    _ = tokio::time::sleep(Duration::from_secs(10 * 60)) => {},
                }
            })
            .await;
        if let Err(error) = result {
            tracing::warn!(%error, "OpenAI Codex local callback server stopped");
        }
        if let Ok(mut guard) = openai_codex_local_callback_shutdown_slot().lock() {
            *guard = None;
        }
    });

    Ok(())
}

fn decode_jwt_claims(token: &str) -> Option<Value> {
    let mut parts = token.split('.');
    let _header = parts.next()?;
    let payload = parts.next()?;
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    serde_json::from_slice::<Value>(&decoded).ok()
}

fn jwt_string_claim(claims: &Value, key: &str) -> Option<String> {
    claims
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn jwt_nested_string_claim(claims: &Value, scope: &str, key: &str) -> Option<String> {
    claims
        .get(scope)
        .and_then(Value::as_object)
        .and_then(|obj| obj.get(key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn resolve_openai_codex_identity(
    access_token: &str,
    id_token: Option<&str>,
) -> (Option<String>, Option<String>, Option<String>, u64) {
    let access_claims = decode_jwt_claims(access_token);
    let id_claims = id_token.and_then(decode_jwt_claims);
    let claims = id_claims.as_ref().or(access_claims.as_ref());

    let email = claims
        .and_then(|value| jwt_nested_string_claim(value, "https://api.openai.com/profile", "email"))
        .or_else(|| claims.and_then(|value| jwt_string_claim(value, "email")));
    let account_id = claims
        .and_then(|value| jwt_string_claim(value, "chatgpt_account_id"))
        .or_else(|| {
            claims.and_then(|value| {
                jwt_nested_string_claim(
                    value,
                    "https://api.openai.com/auth",
                    "chatgpt_account_user_id",
                )
            })
        })
        .or_else(|| {
            claims.and_then(|value| {
                jwt_nested_string_claim(value, "https://api.openai.com/auth", "chatgpt_user_id")
            })
        })
        .or_else(|| claims.and_then(|value| jwt_string_claim(value, "sub")));
    let display_name = email.clone().or_else(|| {
        account_id.as_deref().map(|value| {
            format!(
                "id-{}",
                base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value)
            )
        })
    });
    let expires_at_ms = access_claims
        .as_ref()
        .and_then(|value| value.get("exp"))
        .and_then(|value| value.as_i64())
        .and_then(|value| u64::try_from(value).ok())
        .map(|value| value.saturating_mul(1000))
        .unwrap_or_else(|| crate::now_ms().saturating_add(50 * 60 * 1000));

    (account_id, email, display_name, expires_at_ms)
}

async fn exchange_openai_codex_code(
    code: &str,
    redirect_uri: &str,
    code_verifier: &str,
) -> anyhow::Result<OpenAiCodexTokenExchangeResponse> {
    let body = format!(
        "grant_type=authorization_code&code={}&redirect_uri={}&client_id={}&code_verifier={}",
        urlencoding::encode(code),
        urlencoding::encode(redirect_uri),
        urlencoding::encode(OPENAI_CODEX_OAUTH_CLIENT_ID),
        urlencoding::encode(code_verifier),
    );
    let response = reqwest::Client::new()
        .post(format!("{OPENAI_CODEX_OAUTH_ISSUER}/oauth/token"))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        anyhow::bail!("token exchange failed with status {status}: {text}");
    }
    Ok(serde_json::from_str::<OpenAiCodexTokenExchangeResponse>(
        &text,
    )?)
}

async fn exchange_openai_codex_api_key(id_token: &str) -> anyhow::Result<String> {
    let body = format!(
        "grant_type={}&client_id={}&requested_token={}&subject_token={}&subject_token_type={}",
        urlencoding::encode("urn:ietf:params:oauth:grant-type:token-exchange"),
        urlencoding::encode(OPENAI_CODEX_OAUTH_CLIENT_ID),
        urlencoding::encode("api_key"),
        urlencoding::encode(id_token),
        urlencoding::encode("urn:ietf:params:oauth:token-type:id_token"),
    );
    let response = reqwest::Client::new()
        .post(format!("{OPENAI_CODEX_OAUTH_ISSUER}/oauth/token"))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await?;
    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        anyhow::bail!("api key exchange failed with status {status}: {text}");
    }
    Ok(serde_json::from_str::<OpenAiCodexApiKeyExchangeResponse>(&text)?.access_token)
}

fn openai_codex_oauth_credential(
    state: &AppState,
    tenant_context: &TenantContext,
) -> Option<tandem_core::OAuthProviderCredential> {
    tandem_core::load_provider_oauth_credential_for_tenant_in_dir(
        &provider_auth_security_dir_for_state(state),
        tenant_context,
        OPENAI_CODEX_PROVIDER_ID,
    )
}

pub(crate) fn openai_codex_refreshable_tenants(state: &AppState) -> Vec<TenantContext> {
    let security_dir = provider_auth_security_dir_for_state(state);
    tandem_core::list_provider_oauth_tenant_contexts_in_dir(&security_dir, OPENAI_CODEX_PROVIDER_ID)
        .into_iter()
        .filter(|tenant_context| {
            openai_codex_oauth_credential(state, tenant_context)
                .as_ref()
                .is_some_and(openai_codex_oauth_refreshable_in_process)
        })
        .collect()
}

pub(crate) async fn load_openai_codex_oauth_into_runtime(
    state: &AppState,
    tenant_context: &TenantContext,
) -> anyhow::Result<bool> {
    let Some(credential) = openai_codex_oauth_credential(state, tenant_context) else {
        state
            .providers
            .clear_tenant_provider_bearer_token(tenant_context, OPENAI_CODEX_PROVIDER_ID)
            .await;
        return Ok(false);
    };
    let Some(runtime_token) = tandem_core::ProviderCredential::OAuth(credential)
        .runtime_bearer_token()
        .filter(|token| !token.trim().is_empty())
        .map(str::to_string)
    else {
        state
            .providers
            .clear_tenant_provider_bearer_token(tenant_context, OPENAI_CODEX_PROVIDER_ID)
            .await;
        return Ok(false);
    };

    ensure_openai_codex_runtime_provider_loaded(state).await;
    state
        .providers
        .set_tenant_provider_bearer_token(tenant_context, OPENAI_CODEX_PROVIDER_ID, runtime_token)
        .await;
    Ok(true)
}

#[derive(Clone)]
struct OpenAiCodexOAuthMutationSnapshot {
    persisted: Option<tandem_core::OAuthProviderCredential>,
    runtime_loaded: bool,
    local_auth_token: Option<String>,
    local_runtime_api_key: Option<String>,
    committed_persisted:
        std::sync::Arc<Mutex<Option<Option<tandem_core::OAuthProviderCredential>>>>,
}

type OpenAiCodexOAuthMutationReceipt = Mutex<Option<Option<tandem_core::OAuthProviderCredential>>>;

fn openai_codex_oauth_mutation_receipts(
) -> &'static Mutex<HashMap<String, std::sync::Weak<OpenAiCodexOAuthMutationReceipt>>> {
    static RECEIPTS: OnceLock<
        Mutex<HashMap<String, std::sync::Weak<OpenAiCodexOAuthMutationReceipt>>>,
    > = OnceLock::new();
    RECEIPTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn openai_codex_oauth_mutation_key(state: &AppState, tenant_context: &TenantContext) -> String {
    format!(
        "{}\0{}",
        provider_auth_security_dir_for_state(state).display(),
        serde_json::to_string(tenant_context).unwrap_or_default()
    )
}

fn record_openai_codex_oauth_persisted_mutation(
    state: &AppState,
    tenant_context: &TenantContext,
    committed: Option<tandem_core::OAuthProviderCredential>,
) {
    let key = openai_codex_oauth_mutation_key(state, tenant_context);
    let receipt = openai_codex_oauth_mutation_receipts()
        .lock()
        .ok()
        .and_then(|mut receipts| {
            let receipt = receipts.get(&key).and_then(std::sync::Weak::upgrade);
            if receipt.is_none() {
                receipts.remove(&key);
            }
            receipt
        });
    if let Some(receipt) = receipt {
        if let Ok(mut value) = receipt.lock() {
            *value = Some(committed);
        }
    }
}

async fn snapshot_openai_codex_oauth_mutation(
    state: &AppState,
    tenant_context: &TenantContext,
) -> OpenAiCodexOAuthMutationSnapshot {
    let local = tenant_context.is_local_implicit();
    let local_auth_token = if local {
        state
            .auth
            .read()
            .await
            .get(OPENAI_CODEX_PROVIDER_ID)
            .cloned()
    } else {
        None
    };
    let local_runtime_api_key = if local {
        provider_config_value(
            &state.config.get_effective_value().await,
            OPENAI_CODEX_PROVIDER_ID,
        )
        .and_then(Value::as_object)
        .and_then(|entry| entry.get("api_key").or_else(|| entry.get("apiKey")))
        .and_then(Value::as_str)
        .map(str::to_string)
    } else {
        None
    };
    let committed_persisted = std::sync::Arc::new(Mutex::new(None));
    if let Ok(mut receipts) = openai_codex_oauth_mutation_receipts().lock() {
        receipts.insert(
            openai_codex_oauth_mutation_key(state, tenant_context),
            std::sync::Arc::downgrade(&committed_persisted),
        );
    }
    OpenAiCodexOAuthMutationSnapshot {
        persisted: openai_codex_oauth_credential(state, tenant_context),
        runtime_loaded: state
            .providers
            .tenant_provider_auth_is_loaded(tenant_context, OPENAI_CODEX_PROVIDER_ID)
            .await,
        local_auth_token,
        local_runtime_api_key,
        committed_persisted,
    }
}

async fn persist_openai_codex_oauth_credential(
    state: &AppState,
    tenant_context: &TenantContext,
    credential: tandem_core::OAuthProviderCredential,
) -> anyhow::Result<tandem_core::ProviderAuthBackend> {
    let committed = credential.clone();
    let backend = tandem_core::set_provider_oauth_credential_for_tenant_in_dir_serialized(
        &provider_auth_security_dir_for_state(state),
        tenant_context,
        OPENAI_CODEX_PROVIDER_ID,
        credential,
    )
    .await?;
    record_openai_codex_oauth_persisted_mutation(state, tenant_context, Some(committed));
    Ok(backend)
}

async fn delete_openai_codex_oauth_credential(
    state: &AppState,
    tenant_context: &TenantContext,
) -> anyhow::Result<bool> {
    let removed = tandem_core::delete_provider_credential_for_tenant_in_dir_serialized(
        &provider_auth_security_dir_for_state(state),
        tenant_context,
        OPENAI_CODEX_PROVIDER_ID,
    )
    .await?;
    if removed {
        record_openai_codex_oauth_persisted_mutation(state, tenant_context, None);
    }
    Ok(removed)
}

async fn compare_and_set_openai_codex_oauth_credential(
    state: &AppState,
    tenant_context: &TenantContext,
    expected: Option<&tandem_core::OAuthProviderCredential>,
    replacement: Option<tandem_core::OAuthProviderCredential>,
) -> anyhow::Result<bool> {
    tandem_core::compare_and_set_optional_provider_oauth_credential_for_tenant_in_dir_serialized(
        &provider_auth_security_dir_for_state(state),
        tenant_context,
        OPENAI_CODEX_PROVIDER_ID,
        expected,
        replacement,
    )
    .await
}

async fn apply_openai_codex_runtime_credential(
    state: &AppState,
    tenant_context: &TenantContext,
    credential: Option<&tandem_core::OAuthProviderCredential>,
    should_be_loaded: bool,
) {
    if tenant_context.is_local_implicit() {
        ensure_openai_codex_runtime_provider(state).await;
        state
            .providers
            .reload(state.config.get().await.into())
            .await;
    }
    let runtime_token = credential
        .cloned()
        .map(tandem_core::ProviderCredential::OAuth)
        .and_then(|credential| credential.runtime_bearer_token().map(str::to_string))
        .filter(|token| !token.trim().is_empty());
    if should_be_loaded {
        if let Some(runtime_token) = runtime_token {
            ensure_openai_codex_runtime_provider_loaded(state).await;
            state
                .providers
                .set_tenant_provider_bearer_token(
                    tenant_context,
                    OPENAI_CODEX_PROVIDER_ID,
                    runtime_token,
                )
                .await;
            return;
        }
    }
    state
        .providers
        .clear_tenant_provider_bearer_token(tenant_context, OPENAI_CODEX_PROVIDER_ID)
        .await;
}

async fn restore_openai_codex_oauth_mutation(
    state: &AppState,
    tenant_context: &TenantContext,
    snapshot: &OpenAiCodexOAuthMutationSnapshot,
) -> anyhow::Result<()> {
    let committed = snapshot
        .committed_persisted
        .lock()
        .ok()
        .and_then(|value| value.clone());
    let Some(committed) = committed else {
        return restore_openai_codex_oauth_runtime(state, tenant_context, snapshot).await;
    };
    match compare_and_set_openai_codex_oauth_credential(
        state,
        tenant_context,
        committed.as_ref(),
        snapshot.persisted.clone(),
    )
    .await
    {
        Ok(true) => restore_openai_codex_oauth_runtime(state, tenant_context, snapshot).await,
        Ok(false) => {
            reconcile_openai_codex_oauth_runtime_from_disk(state, tenant_context).await;
            Ok(())
        }
        Err(error) => {
            reconcile_openai_codex_oauth_runtime_from_disk(state, tenant_context).await;
            Err(error).context("failed to compare-and-set prior OAuth credential")
        }
    }
}

async fn restore_openai_codex_oauth_runtime(
    state: &AppState,
    tenant_context: &TenantContext,
    snapshot: &OpenAiCodexOAuthMutationSnapshot,
) -> anyhow::Result<()> {
    let config_result = if tenant_context.is_local_implicit() {
        let mut auth = state.auth.write().await;
        if let Some(token) = snapshot.local_auth_token.clone() {
            auth.insert(OPENAI_CODEX_PROVIDER_ID.to_string(), token);
        } else {
            auth.remove(OPENAI_CODEX_PROVIDER_ID);
        }
        drop(auth);
        if let Some(api_key) = snapshot.local_runtime_api_key.clone() {
            state
                .config
                .patch_runtime(json!({
                    "providers": {
                        OPENAI_CODEX_PROVIDER_ID: { "api_key": api_key }
                    }
                }))
                .await
                .map(|_| ())
        } else {
            state
                .config
                .delete_runtime_provider_key(OPENAI_CODEX_PROVIDER_ID)
                .await
                .map(|_| ())
        }
    } else {
        Ok(())
    };

    apply_openai_codex_runtime_credential(
        state,
        tenant_context,
        snapshot.persisted.as_ref(),
        snapshot.runtime_loaded,
    )
    .await;
    config_result?;
    Ok(())
}

async fn reconcile_openai_codex_oauth_runtime_from_disk(
    state: &AppState,
    tenant_context: &TenantContext,
) {
    let current = openai_codex_oauth_credential(state, tenant_context);
    if tenant_context.is_local_implicit() {
        let mut auth = state.auth.write().await;
        if let Some(api_key) = current
            .as_ref()
            .and_then(|credential| credential.api_key.clone())
        {
            auth.insert(OPENAI_CODEX_PROVIDER_ID.to_string(), api_key);
        } else {
            auth.remove(OPENAI_CODEX_PROVIDER_ID);
        }
    }
    apply_openai_codex_runtime_credential(
        state,
        tenant_context,
        current.as_ref(),
        current.is_some(),
    )
    .await;
}

pub(crate) fn openai_codex_oauth_refresh_failure_code(error: &anyhow::Error) -> &'static str {
    let message = error.to_string();
    if ["HTTP 400", "HTTP 401", "HTTP 403"]
        .iter()
        .any(|status| message.contains(status))
    {
        "credential_rejected"
    } else {
        "refresh_failed"
    }
}

fn refresh_mode(force: bool) -> &'static str {
    if force {
        "dispatch_retry"
    } else {
        "proactive"
    }
}

fn publish_openai_codex_refresh_event(
    state: &AppState,
    tenant_context: &TenantContext,
    event_type: &str,
    mode: &str,
    failure_code: Option<&str>,
) {
    state.event_bus.publish(crate::EngineEvent::new(
        event_type,
        json!({
            "providerID": OPENAI_CODEX_PROVIDER_ID,
            "tenantContext": tenant_context,
            "refreshMode": mode,
            "failureCode": failure_code,
            "occurredAtMs": crate::now_ms(),
        }),
    ));
}

pub(crate) async fn refresh_openai_codex_oauth_if_needed(
    state: &AppState,
    tenant_context: &TenantContext,
) -> anyhow::Result<()> {
    refresh_openai_codex_oauth(state, tenant_context, false).await
}

pub(crate) async fn refresh_openai_codex_oauth_now(
    state: &AppState,
    tenant_context: &TenantContext,
) -> anyhow::Result<()> {
    refresh_openai_codex_oauth(state, tenant_context, true).await
}

async fn refresh_openai_codex_oauth(
    state: &AppState,
    tenant_context: &TenantContext,
    force: bool,
) -> anyhow::Result<()> {
    refresh_openai_codex_oauth_with(
        state,
        tenant_context,
        force,
        refresh_openai_codex_oauth_credential,
    )
    .await
}

async fn refresh_openai_codex_oauth_with<Refresh, RefreshFuture>(
    state: &AppState,
    tenant_context: &TenantContext,
    force: bool,
    refresh: Refresh,
) -> anyhow::Result<()>
where
    Refresh: FnOnce(tandem_core::OAuthProviderCredential) -> RefreshFuture,
    RefreshFuture:
        std::future::Future<Output = anyhow::Result<tandem_core::OAuthProviderCredential>>,
{
    let initial_credential = openai_codex_oauth_credential(state, tenant_context);
    let mut credential_guard = state
        .oauth
        .provider_credential_guard(tenant_context, OPENAI_CODEX_PROVIDER_ID)
        .await;
    if initial_credential
        .as_ref()
        .is_some_and(|credential| credential.managed_by == "codex-cli")
        && tenant_context.is_local_implicit()
        && refresh_openai_codex_cli_oauth_from_disk_if_newer(state).await?
    {
        credential_guard.advance_generation();
    }
    let expected_generation = credential_guard.generation();
    let Some(credential) = openai_codex_oauth_credential(state, tenant_context) else {
        state
            .providers
            .clear_tenant_provider_bearer_token(tenant_context, OPENAI_CODEX_PROVIDER_ID)
            .await;
        return Ok(());
    };
    if initial_credential.as_ref() != Some(&credential) {
        load_openai_codex_oauth_into_runtime(state, tenant_context).await?;
        publish_openai_codex_refresh_event(
            state,
            tenant_context,
            "provider.oauth.refresh.coalesced",
            refresh_mode(force),
            None,
        );
        return Ok(());
    }
    load_openai_codex_oauth_into_runtime(state, tenant_context).await?;
    let now = crate::now_ms();
    if !openai_codex_oauth_refreshable_in_process(&credential)
        || (!force
            && credential.expires_at_ms > now.saturating_add(OPENAI_CODEX_OAUTH_REFRESH_SKEW_MS))
    {
        return Ok(());
    }

    let managed_by = credential.managed_by.clone();
    let refreshed = match refresh(credential.clone()).await {
        Ok(refreshed) => refreshed,
        Err(error) => {
            publish_openai_codex_reauth_required(
                state,
                tenant_context,
                &managed_by,
                refresh_mode(force),
                &error,
            )
            .await;
            return Err(error);
        }
    };
    debug_assert_eq!(credential_guard.generation(), expected_generation);
    credential_guard.advance_generation();
    if let Err(error) = persist_refreshed_openai_codex_oauth(
        state,
        tenant_context,
        &credential,
        refreshed,
        refresh_mode(force),
    )
    .await
    {
        publish_openai_codex_refresh_event(
            state,
            tenant_context,
            "provider.oauth.refresh.failed",
            refresh_mode(force),
            Some("persistence_failed"),
        );
        return Err(error);
    }
    publish_openai_codex_refresh_event(
        state,
        tenant_context,
        "provider.oauth.refresh.succeeded",
        refresh_mode(force),
        None,
    );
    Ok(())
}

pub(crate) fn openai_codex_oauth_refreshable_in_process(
    credential: &tandem_core::OAuthProviderCredential,
) -> bool {
    matches!(credential.managed_by.as_str(), "tandem" | "codex-cli")
        && !credential.refresh_token.trim().is_empty()
}

async fn refresh_openai_codex_oauth_credential(
    mut credential: tandem_core::OAuthProviderCredential,
) -> anyhow::Result<tandem_core::OAuthProviderCredential> {
    let response = reqwest::Client::new()
        .post(format!("{OPENAI_CODEX_OAUTH_ISSUER}/oauth/token"))
        .header("content-type", "application/json")
        .json(&json!({
            "client_id": OPENAI_CODEX_OAUTH_CLIENT_ID,
            "grant_type": "refresh_token",
            "refresh_token": credential.refresh_token,
        }))
        .send()
        .await?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("Codex OAuth refresh failed with HTTP {}", status.as_u16());
    }
    let refresh = response
        .json::<OpenAiCodexTokenExchangeResponse>()
        .await
        .map_err(|_| anyhow::anyhow!("Codex OAuth refresh returned an invalid response"))?;
    if let Some(access_token) = refresh.access_token.as_deref() {
        credential.access_token = access_token.to_string();
    }
    if let Some(refresh_token) = refresh.refresh_token.as_deref() {
        credential.refresh_token = refresh_token.to_string();
    }
    let id_token = refresh.id_token.as_deref();
    let (account_id, email, display_name, expires_at_ms) =
        resolve_openai_codex_identity(&credential.access_token, id_token);
    credential.account_id = account_id.or(credential.account_id);
    credential.email = email.or(credential.email);
    credential.display_name = display_name.or(credential.display_name);
    credential.expires_at_ms = expires_at_ms;
    if let Some(id_token) = id_token {
        if let Ok(api_key) = exchange_openai_codex_api_key(id_token).await {
            credential.api_key = Some(api_key);
        }
    }
    Ok(credential)
}

async fn persist_refreshed_openai_codex_oauth(
    state: &AppState,
    tenant_context: &TenantContext,
    previous: &tandem_core::OAuthProviderCredential,
    credential: tandem_core::OAuthProviderCredential,
    mode: &str,
) -> anyhow::Result<()> {
    let mut snapshot = snapshot_openai_codex_oauth_mutation(state, tenant_context).await;
    snapshot.persisted = Some(previous.clone());
    let runtime_token = tandem_core::ProviderCredential::OAuth(credential.clone())
        .runtime_bearer_token()
        .filter(|token| !token.trim().is_empty())
        .map(str::to_string);
    let api_key = credential.api_key.clone();
    let managed_by = credential.managed_by.clone();
    if !compare_and_set_openai_codex_oauth_credential(
        state,
        tenant_context,
        Some(previous),
        Some(credential.clone()),
    )
    .await?
    {
        reconcile_openai_codex_oauth_runtime_from_disk(state, tenant_context).await;
        anyhow::bail!("OAuth credential changed while refresh was in flight");
    }
    ensure_openai_codex_runtime_provider_loaded(state).await;
    if let Some(runtime_token) = runtime_token {
        state
            .providers
            .set_tenant_provider_bearer_token(
                tenant_context,
                OPENAI_CODEX_PROVIDER_ID,
                runtime_token,
            )
            .await;
    }
    if tenant_context.is_local_implicit() {
        if let Some(api_key) = api_key {
            state
                .auth
                .write()
                .await
                .insert(OPENAI_CODEX_PROVIDER_ID.to_string(), api_key);
        }
        state
            .providers
            .reload(state.config.get().await.into())
            .await;
    }

    if let Err(audit_error) = crate::audit::append_protected_audit_event(
        state,
        "provider.oauth.refresh.succeeded",
        tenant_context,
        tenant_context.actor_id.clone(),
        json!({
            "providerID": OPENAI_CODEX_PROVIDER_ID,
            "managedBy": managed_by,
            "refreshMode": mode,
            "occurredAtMs": crate::now_ms(),
        }),
    )
    .await
    {
        let rolled_back = compare_and_set_openai_codex_oauth_credential(
            state,
            tenant_context,
            Some(&credential),
            Some(previous.clone()),
        )
        .await;
        match rolled_back {
            Ok(true) => {
                restore_openai_codex_oauth_runtime(state, tenant_context, &snapshot).await?;
            }
            Ok(false) => {
                reconcile_openai_codex_oauth_runtime_from_disk(state, tenant_context).await;
            }
            Err(rollback_error) => {
                reconcile_openai_codex_oauth_runtime_from_disk(state, tenant_context).await;
                return Err(rollback_error).context(format!(
                    "protected audit persistence failed ({audit_error}); OAuth rollback also failed"
                ));
            }
        }
        return Err(audit_error).context(
            "protected audit persistence failed; refreshed OAuth credential was rolled back",
        );
    }
    Ok(())
}

async fn publish_openai_codex_reauth_required(
    state: &AppState,
    tenant_context: &TenantContext,
    managed_by: &str,
    mode: &str,
    error: &anyhow::Error,
) {
    let now = crate::now_ms();
    let failure_code = openai_codex_oauth_refresh_failure_code(error);
    publish_openai_codex_refresh_event(
        state,
        tenant_context,
        "provider.oauth.refresh.failed",
        mode,
        Some(failure_code),
    );
    crate::audit::append_protected_audit_event_best_effort(
        state,
        "provider.oauth.refresh.failed",
        tenant_context,
        tenant_context.actor_id.clone(),
        json!({
            "providerID": OPENAI_CODEX_PROVIDER_ID,
            "managedBy": managed_by,
            "reason": failure_code,
            "refreshMode": mode,
            "occurredAtMs": now,
        }),
    )
    .await;
    if failure_code != "credential_rejected" {
        return;
    }
    state.event_bus.publish(crate::EngineEvent::new(
        "provider.oauth.reauth_required",
        json!({
            "providerID": OPENAI_CODEX_PROVIDER_ID,
            "managedBy": managed_by,
            "tenantContext": tenant_context.clone(),
            "status": "reauth_required",
            "reason": failure_code,
            "refreshMode": mode,
            "occurredAtMs": now,
        }),
    ));
    crate::audit::append_protected_audit_event_best_effort(
        state,
        "provider.oauth.reauth_required",
        tenant_context,
        tenant_context.actor_id.clone(),
        json!({
            "providerID": OPENAI_CODEX_PROVIDER_ID,
            "managedBy": managed_by,
            "reason": failure_code,
            "refreshMode": mode,
            "occurredAtMs": now,
        }),
    )
    .await;
}

async fn refresh_openai_codex_cli_oauth_from_disk_if_newer(
    state: &AppState,
) -> anyhow::Result<bool> {
    let tenant_context = TenantContext::local_implicit();
    let security_dir = provider_auth_security_dir_for_state(state);
    let Some(existing) = tandem_core::load_provider_oauth_credential_for_tenant_in_dir(
        &security_dir,
        &tenant_context,
        OPENAI_CODEX_PROVIDER_ID,
    ) else {
        return Ok(false);
    };
    if existing.managed_by != "codex-cli" {
        return Ok(false);
    }

    let Some(incoming) = tandem_core::load_openai_codex_cli_oauth_credential() else {
        return Ok(false);
    };
    if existing == incoming || incoming.expires_at_ms <= existing.expires_at_ms {
        return Ok(false);
    }

    let runtime_token = tandem_core::ProviderCredential::OAuth(incoming.clone())
        .runtime_bearer_token()
        .filter(|token| !token.trim().is_empty())
        .map(str::to_string);
    persist_openai_codex_oauth_credential(state, &tenant_context, incoming).await?;
    ensure_openai_codex_runtime_provider(state).await;
    state
        .providers
        .reload(state.config.get().await.into())
        .await;
    if let Some(runtime_token) = runtime_token {
        state
            .providers
            .set_tenant_provider_bearer_token(
                &tenant_context,
                OPENAI_CODEX_PROVIDER_ID,
                runtime_token,
            )
            .await;
    }
    Ok(true)
}

fn config_patch_updates_openai_codex_default(input: &Value) -> bool {
    input
        .get("providers")
        .and_then(Value::as_object)
        .and_then(|providers| providers.get(OPENAI_CODEX_PROVIDER_ID))
        .and_then(Value::as_object)
        .and_then(|provider| provider.get("default_model"))
        .and_then(Value::as_str)
        .is_some_and(|model| !model.trim().is_empty())
}

fn openai_codex_default_model_from_layer(layers: &Value, layer: &str) -> Option<String> {
    layers
        .get(layer)
        .and_then(|value| value.get("providers"))
        .and_then(Value::as_object)
        .and_then(|providers| providers.get(OPENAI_CODEX_PROVIDER_ID))
        .and_then(Value::as_object)
        .and_then(|provider| provider.get("default_model"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(ToString::to_string)
}

async fn openai_codex_runtime_default_model(state: &AppState) -> String {
    let layers = state.config.get_layers_value().await;
    for layer in ["cli", "env", "managed", "project", "global"] {
        if let Some(model) = openai_codex_default_model_from_layer(&layers, layer) {
            // A persisted default can reference a model that no longer exists in
            // the catalog (e.g. the retired `gpt-5.1-codex-max`). Honoring it makes
            // every run — including channels that fetch this default fresh — fail
            // with a provider 400, so skip stale values and fall back to the
            // compiled default rather than trusting the saved config blindly.
            if tandem_providers::openai_codex_model_is_supported(&model) {
                return model;
            }
        }
    }
    OPENAI_CODEX_DEFAULT_MODEL.to_string()
}

async fn ensure_openai_codex_runtime_provider(state: &AppState) {
    let default_model = openai_codex_runtime_default_model(state).await;
    let _ = state
        .config
        .patch_runtime(json!({
            "providers": {
                OPENAI_CODEX_PROVIDER_ID: {
                    "url": OPENAI_CODEX_API_BASE_URL,
                    "default_model": default_model,
                }
            }
        }))
        .await;
}

async fn ensure_openai_codex_runtime_provider_loaded(state: &AppState) {
    if state
        .providers
        .list()
        .await
        .iter()
        .any(|provider| provider.id == OPENAI_CODEX_PROVIDER_ID)
    {
        return;
    }
    ensure_openai_codex_runtime_provider(state).await;
    state
        .providers
        .reload(state.config.get().await.into())
        .await;
}

async fn finish_provider_oauth_callback(
    state: AppState,
    id: String,
    input: ProviderOAuthCallbackInput,
) -> Value {
    let provider_id = canonical_provider_id(&id);
    if provider_id != OPENAI_CODEX_PROVIDER_ID {
        return json!({
            "ok": false,
            "error": format!("oauth is not supported for provider `{provider_id}`"),
        });
    }

    let Some(state_token) = input
        .state
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return json!({"ok": false, "error": "missing oauth state"});
    };

    let session_id = {
        let sessions = state.oauth.provider_sessions().read().await;
        sessions.iter().find_map(|(session_id, session)| {
            (session.provider_id == provider_id && session.state == state_token)
                .then(|| session_id.clone())
        })
    };
    let Some(session_id) = session_id else {
        return json!({"ok": false, "error": "oauth session not found or expired"});
    };

    if let Some(error) = input
        .error
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let detail = input
            .error_description
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| error.to_string());
        if let Some(session) = state
            .oauth
            .provider_sessions()
            .write()
            .await
            .get_mut(&session_id)
        {
            session.status = "error".to_string();
            session.error = Some(detail.clone());
        }
        return json!({"ok": false, "error": detail});
    }

    let Some(code) = input
        .code
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return json!({"ok": false, "error": "missing authorization code"});
    };

    let session = {
        state
            .oauth
            .provider_sessions()
            .read()
            .await
            .get(&session_id)
            .cloned()
    };
    let Some(session) = session else {
        return json!({"ok": false, "error": "oauth session not found"});
    };
    if session.expires_at_ms <= crate::now_ms() {
        return json!({"ok": false, "error": "oauth session expired before callback completed"});
    }

    let tenant_context = session.tenant_context.clone();
    let mut credential_guard = state
        .oauth
        .provider_credential_guard(&tenant_context, OPENAI_CODEX_PROVIDER_ID)
        .await;

    let exchanged =
        match exchange_openai_codex_code(code, &session.redirect_uri, &session.code_verifier).await
        {
            Ok(value) => value,
            Err(error) => {
                if let Some(entry) = state
                    .oauth
                    .provider_sessions()
                    .write()
                    .await
                    .get_mut(&session_id)
                {
                    entry.status = "error".to_string();
                    entry.error = Some(error.to_string());
                }
                return json!({"ok": false, "error": error.to_string()});
            }
        };

    let access_token = exchanged
        .access_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let refresh_token = exchanged
        .refresh_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let id_token = exchanged
        .id_token
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    let (Some(access_token), Some(refresh_token)) = (access_token, refresh_token) else {
        return json!({"ok": false, "error": "oauth token exchange returned incomplete credentials"});
    };

    let api_key = match id_token.as_deref() {
        Some(token) => exchange_openai_codex_api_key(token).await.ok(),
        None => None,
    };
    let (account_id, email, display_name, expires_at_ms) =
        resolve_openai_codex_identity(&access_token, id_token.as_deref());
    let oauth_credential = tandem_core::OAuthProviderCredential {
        provider_id: OPENAI_CODEX_PROVIDER_ID.to_string(),
        access_token: access_token.clone(),
        refresh_token,
        expires_at_ms,
        account_id,
        email: email.clone(),
        display_name: display_name.clone(),
        managed_by: "tandem".to_string(),
        api_key: api_key.clone(),
    };

    let snapshot = snapshot_openai_codex_oauth_mutation(&state, &tenant_context).await;
    credential_guard.advance_generation();
    let backend = match persist_openai_codex_oauth_credential(
        &state,
        &tenant_context,
        oauth_credential.clone(),
    )
    .await
    {
        Ok(tandem_core::ProviderAuthBackend::Keychain) => "keychain",
        Ok(tandem_core::ProviderAuthBackend::File) => "file",
        Err(error) => {
            let _ = restore_openai_codex_oauth_mutation(&state, &tenant_context, &snapshot).await;
            return json!({"ok": false, "error": error.to_string()});
        }
    };
    apply_openai_codex_runtime_credential(&state, &tenant_context, Some(&oauth_credential), true)
        .await;

    if tenant_context.is_local_implicit() {
        if let Some(api_key) = api_key {
            state
                .auth
                .write()
                .await
                .insert(OPENAI_CODEX_PROVIDER_ID.to_string(), api_key);
        }
    }

    if let Err(error) = crate::audit::append_protected_audit_event(
        &state,
        "provider.oauth.updated",
        &tenant_context,
        tenant_context.actor_id.clone(),
        json!({
            "providerID": OPENAI_CODEX_PROVIDER_ID,
            "backend": backend,
            "managedBy": "tandem",
            "email": email,
        }),
    )
    .await
    {
        if let Err(rollback_error) =
            restore_openai_codex_oauth_mutation(&state, &tenant_context, &snapshot).await
        {
            tracing::error!(
                error = ?rollback_error,
                "failed to restore provider OAuth state after protected audit failure"
            );
        }
        tracing::error!(
            error = ?error,
            "OAuth credential update was rolled back after required protected audit persistence failed"
        );
        return json!({
            "ok": false,
            "error": "The credential update was rolled back because its required audit record could not be persisted",
            "code": "PROTECTED_AUDIT_PERSISTENCE_FAILED",
            "retryable": true,
        });
    }

    if let Some(entry) = state
        .oauth
        .provider_sessions()
        .write()
        .await
        .get_mut(&session_id)
    {
        entry.status = "connected".to_string();
        entry.error = None;
        entry.email = email.clone();
    }

    json!({
        "ok": true,
        "provider_id": OPENAI_CODEX_PROVIDER_ID,
        "session_id": session_id,
        "email": email,
        "display_name": display_name,
        "expires_at_ms": expires_at_ms,
    })
}

fn redact_secret_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, field) in map.iter_mut() {
                if key.eq_ignore_ascii_case("api_key")
                    || key.eq_ignore_ascii_case("apikey")
                    || key.eq_ignore_ascii_case("bot_token")
                    || key.eq_ignore_ascii_case("botToken")
                {
                    *field = Value::String("[REDACTED]".to_string());
                } else {
                    redact_secret_fields(field);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_secret_fields(item);
            }
        }
        _ => {}
    }
}

fn redacted(mut value: Value) -> Value {
    redact_secret_fields(&mut value);
    value
}

fn contains_secret_config_fields(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, field)| {
            key.eq_ignore_ascii_case("api_key")
                || key.eq_ignore_ascii_case("apikey")
                || key.eq_ignore_ascii_case("bot_token")
                || key.eq_ignore_ascii_case("botToken")
                || contains_secret_config_fields(field)
        }),
        Value::Array(items) => items.iter().any(contains_secret_config_fields),
        _ => false,
    }
}

const MAX_JSON_DEPTH: usize = 64; // Prevent stack overflow from deeply nested JSON

fn merge_json(base: &mut Value, overlay: &Value) {
    merge_json_with_depth(base, overlay, 0);
}

fn merge_json_with_depth(base: &mut Value, overlay: &Value, depth: usize) {
    // SECURITY: Prevent stack overflow from unbounded recursion
    if depth > MAX_JSON_DEPTH {
        tracing::warn!(
            target: "tandem_server::config",
            "rejecting JSON merge: exceeded maximum nesting depth ({})",
            MAX_JSON_DEPTH
        );
        return;
    }

    if overlay.is_null() {
        return;
    }
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            for (key, value) in overlay_map {
                if value.is_null() {
                    continue;
                }
                match base_map.get_mut(key) {
                    Some(existing) => merge_json_with_depth(existing, value, depth + 1),
                    None => {
                        base_map.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (base_value, overlay_value) => {
            *base_value = overlay_value.clone();
        }
    }
}

fn identity_default_value() -> Value {
    json!({
        "bot": {
            "canonical_name": "Tandem",
            "avatar_url": null,
            "aliases": {
                "desktop": "Tandem",
                "tui": "Tandem TUI",
                "portal": "Tandem Portal",
                "control_panel": "Tandem Control Panel",
                "channels": "Tandem",
                "protocol": "Tandem",
                "cli": "Tandem"
            }
        },
        "personality": {
            "default": {
                "preset": "balanced",
                "custom_instructions": null
            },
            "per_agent": {}
        }
    })
}

fn personality_presets_catalog() -> Value {
    json!([
        {
            "id": "balanced",
            "label": "Balanced",
            "description": "Pragmatic, direct, and neutral tone."
        },
        {
            "id": "concise",
            "label": "Concise",
            "description": "Short, high-signal responses focused on outcomes."
        },
        {
            "id": "friendly",
            "label": "Friendly",
            "description": "Warm, approachable style while staying practical."
        },
        {
            "id": "mentor",
            "label": "Mentor",
            "description": "Guides with context and explicit reasoning."
        },
        {
            "id": "critical",
            "label": "Critical",
            "description": "Skeptical, risk-first framing with clear tradeoffs."
        }
    ])
}

fn normalize_effective_config_with_identity(mut value: Value) -> Value {
    let legacy_bot_name = value
        .get("bot_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToString::to_string);
    let legacy_persona = value
        .get("persona")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToString::to_string);

    if !value.is_object() {
        return value;
    }

    let root = value.as_object_mut().expect("checked object");
    if !root.contains_key("identity") || !root.get("identity").is_some_and(Value::is_object) {
        root.insert("identity".to_string(), identity_default_value());
    }
    if let Some(identity) = root.get_mut("identity") {
        let mut normalized = identity_default_value();
        merge_json(&mut normalized, identity);
        *identity = normalized;
    }

    if let Some(legacy_bot_name) = legacy_bot_name {
        let canonical_name = root
            .get("identity")
            .and_then(Value::as_object)
            .and_then(|identity| identity.get("bot"))
            .and_then(Value::as_object)
            .and_then(|bot| bot.get("canonical_name"))
            .and_then(Value::as_str);
        let should_fill = canonical_name
            .map(|value| value.trim().is_empty())
            .unwrap_or(true);
        if should_fill {
            root["identity"]["bot"]["canonical_name"] = Value::String(legacy_bot_name);
        }
    }

    if let Some(legacy_persona) = legacy_persona {
        let has_custom = root
            .get("identity")
            .and_then(|identity| identity.get("personality"))
            .and_then(|personality| personality.get("default"))
            .and_then(|default| default.get("custom_instructions"))
            .and_then(Value::as_str)
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false);
        if !has_custom {
            root["identity"]["personality"]["default"]["custom_instructions"] =
                Value::String(legacy_persona);
        }
    }

    let canonical_name = root
        .get("identity")
        .and_then(|identity| identity.get("bot"))
        .and_then(|bot| bot.get("canonical_name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("Tandem")
        .to_string();
    if let Some(aliases) = root
        .get_mut("identity")
        .and_then(Value::as_object_mut)
        .and_then(|identity| identity.get_mut("bot"))
        .and_then(Value::as_object_mut)
        .and_then(|bot| bot.get_mut("aliases"))
        .and_then(Value::as_object_mut)
    {
        for alias in [
            "desktop",
            "portal",
            "channels",
            "protocol",
            "cli",
            "control_panel",
        ] {
            let needs_fill = aliases
                .get(alias)
                .and_then(Value::as_str)
                .map(|v| v.trim().is_empty())
                .unwrap_or(true);
            if needs_fill {
                aliases.insert(alias.to_string(), Value::String(canonical_name.clone()));
            }
        }
        let tui_needs_fill = aliases
            .get("tui")
            .and_then(Value::as_str)
            .map(|v| v.trim().is_empty())
            .unwrap_or(true);
        if tui_needs_fill {
            aliases.insert(
                "tui".to_string(),
                Value::String(format!("{canonical_name} TUI")),
            );
        }
    }

    let default_preset_empty = root
        .get("identity")
        .and_then(|identity| identity.get("personality"))
        .and_then(|personality| personality.get("default"))
        .and_then(|default| default.get("preset"))
        .and_then(Value::as_str)
        .map(|v| v.trim().is_empty())
        .unwrap_or(true);
    if default_preset_empty {
        root["identity"]["personality"]["default"]["preset"] =
            Value::String("balanced".to_string());
    }

    root.insert("bot_name".to_string(), Value::String(canonical_name));
    let compat_persona = root
        .get("identity")
        .and_then(|identity| identity.get("personality"))
        .and_then(|personality| personality.get("default"))
        .and_then(|default| default.get("custom_instructions"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string);
    root.insert(
        "persona".to_string(),
        compat_persona.map_or(Value::Null, Value::String),
    );

    value
}

fn normalize_layers_with_identity(mut value: Value) -> Value {
    let Some(root) = value.as_object_mut() else {
        return value;
    };
    for layer in ["global", "project", "managed", "env", "runtime", "cli"] {
        if let Some(entry) = root.get_mut(layer) {
            let normalized = normalize_effective_config_with_identity(entry.clone());
            *entry = normalized;
        }
    }
    value
}

fn normalize_config_patch_input(mut input: Value) -> Value {
    let Some(root) = input.as_object_mut() else {
        return input;
    };
    let legacy_bot_name = root
        .get("bot_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToString::to_string);
    let legacy_persona = root
        .get("persona")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToString::to_string);

    if let Some(legacy_bot_name) = legacy_bot_name {
        root.entry("identity".to_string())
            .or_insert_with(|| json!({}));
        root["identity"]["bot"]["canonical_name"] = Value::String(legacy_bot_name);
    }
    if let Some(legacy_persona) = legacy_persona {
        root.entry("identity".to_string())
            .or_insert_with(|| json!({}));
        root["identity"]["personality"]["default"]["custom_instructions"] =
            Value::String(legacy_persona);
        if root["identity"]["personality"]["default"]
            .get("preset")
            .and_then(Value::as_str)
            .map(|v| v.trim().is_empty())
            .unwrap_or(true)
        {
            root["identity"]["personality"]["default"]["preset"] =
                Value::String("balanced".to_string());
        }
    }

    root.remove("bot_name");
    root.remove("persona");
    normalize_identity_avatar_patch(root);
    input
}

fn normalize_identity_avatar_patch(root: &mut serde_json::Map<String, Value>) {
    let avatar_slot = root
        .get_mut("identity")
        .and_then(Value::as_object_mut)
        .and_then(|identity| identity.get_mut("bot"))
        .and_then(Value::as_object_mut)
        .and_then(|bot| bot.get_mut("avatar_url"));

    let Some(slot) = avatar_slot else {
        return;
    };
    let Some(raw) = slot.as_str() else {
        return;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        *slot = Value::Null;
        return;
    }
    if let Some(normalized) = normalize_avatar_data_url(trimmed) {
        *slot = Value::String(normalized);
    }
}

fn normalize_avatar_data_url(input: &str) -> Option<String> {
    if !input.starts_with("data:image/") {
        return Some(input.to_string());
    }

    let (meta, payload) = input.split_once(',')?;
    if !meta.contains(";base64") {
        return None;
    }
    // Safety guard against very large inline payloads.
    if payload.len() > 24 * 1024 * 1024 {
        return None;
    }

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.as_bytes())
        .ok()?;
    if bytes.len() > 16 * 1024 * 1024 {
        return None;
    }

    let mut image = image::load_from_memory(&bytes).ok()?;
    if image.width() > 512 || image.height() > 512 {
        image = image.thumbnail(512, 512);
    }

    // Re-encode to PNG for consistent, browser-safe storage.
    let mut out = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .ok()?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(out);
    Some(format!("data:image/png;base64,{encoded}"))
}
