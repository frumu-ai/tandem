// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

pub(super) async fn delete_auth(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    Extension(locality): Extension<crate::http::host_authority::RequestLocality>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
    Path(id): Path<String>,
) -> Response {
    let normalized_id = id.trim().to_ascii_lowercase();
    if normalized_id.is_empty() {
        return Json(json!({"ok": false, "error": "provider id cannot be empty"})).into_response();
    }
    let (grant, effect) = match crate::http::host_authority::authorize_administrative_effect(
        &state,
        &tenant_context,
        verified.as_deref(),
        locality,
        crate::action_authorization::HostAction::ProviderCredentialUpdate,
        "provider_credential",
        normalized_id.clone(),
        json!({"provider_id": normalized_id, "operation": "delete"}),
    )
    .await
    {
        Ok(authorized) => authorized,
        Err(status) => return status.into_response(),
    };

    let mut credential_guard = state
        .oauth
        .provider_credential_guard(&tenant_context, &normalized_id)
        .await;
    let _persistence_guard = state.oauth.provider_credential_persistence_guard().await;
    let provider_auth_security_dir = provider_auth_security_dir_for_state(&state);
    let snapshot = snapshot_api_key_mutation(
        &state,
        &provider_auth_security_dir,
        &tenant_context,
        &normalized_id,
    )
    .await;
    if let Err(error) = grant.revalidate(&state, &effect) {
        return crate::http::host_authority::host_authorization_status(error).into_response();
    }
    credential_guard.advance_generation();
    let persisted_removed = match tandem_core::delete_provider_auth_for_tenant_in_dir(
        &provider_auth_security_dir,
        &tenant_context,
        &normalized_id,
    ) {
        Ok(removed) => removed,
        Err(error) => {
            rollback_api_key_mutation(
                &state,
                &provider_auth_security_dir,
                &tenant_context,
                &normalized_id,
                &snapshot,
                "provider API-key delete persistence failure",
            )
            .await;
            return provider_auth_mutation_error_response(
                &normalized_id,
                "PROVIDER_AUTH_PERSISTENCE_FAILED",
                format!("failed to delete persisted provider auth: {error}"),
            );
        }
    };

    let runtime_removed = tenant_context.is_local_implicit()
        && (snapshot.local_auth.is_some() || snapshot.runtime_api_key.is_some());
    if tenant_context.is_local_implicit() {
        if let Err(error) = state
            .config
            .delete_runtime_provider_key(&normalized_id)
            .await
        {
            rollback_api_key_mutation(
                &state,
                &provider_auth_security_dir,
                &tenant_context,
                &normalized_id,
                &snapshot,
                "provider API-key runtime delete failure",
            )
            .await;
            return provider_auth_mutation_error_response(
                &normalized_id,
                "PROVIDER_AUTH_RUNTIME_PATCH_FAILED",
                format!("failed to delete provider runtime auth: {error}"),
            );
        }
        state.auth.write().await.remove(&normalized_id);
    }
    if runtime_removed || persisted_removed {
        if let Err(error) = crate::audit::append_protected_audit_event(
            &state,
            "provider.secret.deleted",
            &tenant_context,
            request_actor(&request_principal, &tenant_context),
            json!({
                "providerID": normalized_id,
                "runtimeRemoved": runtime_removed,
                "persistedRemoved": persisted_removed,
            }),
        )
        .await
        {
            rollback_api_key_mutation(
                &state,
                &provider_auth_security_dir,
                &tenant_context,
                &normalized_id,
                &snapshot,
                "provider API-key delete audit failure",
            )
            .await;
            return super::protected_audit_error_response(error).into_response();
        }
    }
    if runtime_removed {
        state
            .providers
            .reload(state.config.get().await.into())
            .await;
    }
    Json(json!({"ok": runtime_removed || persisted_removed})).into_response()
}

struct ApiKeyMutationSnapshot {
    persisted: Option<String>,
    local_auth: Option<String>,
    runtime_api_key: Option<String>,
}

async fn snapshot_api_key_mutation(
    state: &AppState,
    security_dir: &std::path::Path,
    tenant_context: &TenantContext,
    provider_id: &str,
) -> ApiKeyMutationSnapshot {
    let persisted = tandem_core::load_provider_auth_for_tenant_in_dir(security_dir, tenant_context)
        .remove(provider_id);
    if !tenant_context.is_local_implicit() {
        return ApiKeyMutationSnapshot {
            persisted,
            local_auth: None,
            runtime_api_key: None,
        };
    }

    let local_auth = state.auth.read().await.get(provider_id).cloned();
    let layers = state.config.get_layers_value().await;
    let runtime_api_key = layers
        .get("runtime")
        .and_then(|runtime| runtime.get("providers"))
        .and_then(Value::as_object)
        .and_then(|providers| {
            providers
                .get(provider_id)
                .or_else(|| {
                    providers
                        .iter()
                        .find(|(id, _)| id.eq_ignore_ascii_case(provider_id))
                        .map(|(_, value)| value)
                })
                .and_then(Value::as_object)
        })
        .and_then(|provider| provider.get("api_key").or_else(|| provider.get("apiKey")))
        .and_then(Value::as_str)
        .map(str::to_string);
    ApiKeyMutationSnapshot {
        persisted,
        local_auth,
        runtime_api_key,
    }
}

async fn restore_api_key_mutation(
    state: &AppState,
    security_dir: &std::path::Path,
    tenant_context: &TenantContext,
    provider_id: &str,
    snapshot: &ApiKeyMutationSnapshot,
) -> anyhow::Result<()> {
    let persistence_result = if let Some(token) = snapshot.persisted.as_deref() {
        tandem_core::set_provider_auth_for_tenant_in_dir(
            security_dir,
            tenant_context,
            provider_id,
            token,
        )
        .map(|_| ())
    } else {
        tandem_core::delete_provider_auth_for_tenant_in_dir(
            security_dir,
            tenant_context,
            provider_id,
        )
        .map(|_| ())
    };

    let runtime_result = if tenant_context.is_local_implicit() {
        let result = async {
            state
                .config
                .delete_runtime_provider_key(provider_id)
                .await?;
            if let Some(token) = snapshot.runtime_api_key.as_deref() {
                state
                    .config
                    .patch_runtime(json!({
                        "providers": {
                            provider_id.to_string(): { "api_key": token }
                        }
                    }))
                    .await?;
            }
            let mut auth = state.auth.write().await;
            match snapshot.local_auth.as_ref() {
                Some(token) => {
                    auth.insert(provider_id.to_string(), token.clone());
                }
                None => {
                    auth.remove(provider_id);
                }
            }
            drop(auth);
            state
                .providers
                .reload(state.config.get().await.into())
                .await;
            Ok(())
        }
        .await;
        result
    } else {
        Ok(())
    };

    persistence_result?;
    runtime_result
}

async fn rollback_api_key_mutation(
    state: &AppState,
    security_dir: &std::path::Path,
    tenant_context: &TenantContext,
    provider_id: &str,
    snapshot: &ApiKeyMutationSnapshot,
    context: &'static str,
) {
    if let Err(error) =
        restore_api_key_mutation(state, security_dir, tenant_context, provider_id, snapshot).await
    {
        tracing::error!(error = ?error, provider_id, context, "failed to restore provider API-key mutation");
    }
}

fn provider_auth_mutation_error_response(
    provider_id: &str,
    code: &'static str,
    error: String,
) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "ok": false,
            "id": provider_id,
            "code": code,
            "error": error,
        })),
    )
        .into_response()
}

pub(super) async fn set_api_token(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(locality): Extension<crate::http::host_authority::RequestLocality>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
    headers: axum::http::HeaderMap,
    Json(input): Json<ApiTokenInput>,
) -> Response {
    if let Err(error) = authorize_api_token_management(&state, &headers).await {
        return Json(error).into_response();
    }
    let token = input.token.unwrap_or_default().trim().to_string();
    if token.is_empty() {
        return Json(json!({
            "ok": false,
            "error": "token cannot be empty"
        }))
        .into_response();
    }
    let scopes = match normalize_api_token_scopes(input.scopes) {
        Ok(scopes) => scopes,
        Err(error) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"ok": false, "code": "TOKEN_SCOPE_INVALID", "error": error})),
            )
                .into_response()
        }
    };
    let (grant, effect) = match crate::http::host_authority::authorize_administrative_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality,
        crate::action_authorization::HostAction::ApiTokenManage,
        "deployment_api_token",
        tenant
            .deployment_id
            .as_deref()
            .unwrap_or("local-deployment"),
        json!({
            "operation": "set",
            "token_digest": format!("{:x}", Sha256::digest(token.as_bytes())),
        }),
    )
    .await
    {
        Ok(authorized) => authorized,
        Err(status) => return status.into_response(),
    };
    if let Err(error) = grant.revalidate(&state, &effect) {
        return crate::http::host_authority::host_authorization_status(error).into_response();
    }
    let metadata = state.rotate_api_token(token, scopes).await;
    Json(json!({
        "ok": true,
        "token_id": metadata.token_id,
        "scopes": metadata.scopes,
        "created_at_ms": metadata.created_at_ms,
    }))
    .into_response()
}

pub(super) async fn clear_api_token(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(locality): Extension<crate::http::host_authority::RequestLocality>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
    headers: axum::http::HeaderMap,
) -> Response {
    if let Err(error) = authorize_api_token_management(&state, &headers).await {
        return Json(error).into_response();
    }
    if let Err(status) = crate::http::host_authority::authorize_administrative_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality,
        crate::action_authorization::HostAction::ApiTokenManage,
        "deployment_api_token",
        tenant
            .deployment_id
            .as_deref()
            .unwrap_or("local-deployment"),
        json!({"operation": "clear_denied"}),
    )
    .await
    {
        return status.into_response();
    }
    Json(json!({
        "ok": false,
        "error": "clearing the API token is disabled because it would reopen the HTTP API; use /auth/token/generate to rotate it"
    }))
    .into_response()
}

pub(super) async fn generate_api_token(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(locality): Extension<crate::http::host_authority::RequestLocality>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
    headers: axum::http::HeaderMap,
) -> Response {
    if let Err(error) = authorize_api_token_management(&state, &headers).await {
        return Json(error).into_response();
    }
    let token = format!("tk_{}", Uuid::new_v4().simple());
    let scopes = vec!["engine.api".to_string()];
    let (grant, effect) = match crate::http::host_authority::authorize_administrative_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality,
        crate::action_authorization::HostAction::ApiTokenManage,
        "deployment_api_token",
        tenant
            .deployment_id
            .as_deref()
            .unwrap_or("local-deployment"),
        json!({
            "operation": "generate",
            "token_digest": format!("{:x}", Sha256::digest(token.as_bytes())),
        }),
    )
    .await
    {
        Ok(authorized) => authorized,
        Err(status) => return status.into_response(),
    };
    if let Err(error) = grant.revalidate(&state, &effect) {
        return crate::http::host_authority::host_authorization_status(error).into_response();
    }
    let metadata = state.rotate_api_token(token.clone(), scopes).await;
    Json(json!({
        "ok": true,
        "token": token,
        "token_id": metadata.token_id,
        "scopes": metadata.scopes,
        "created_at_ms": metadata.created_at_ms,
    }))
    .into_response()
}

pub(super) async fn list_api_tokens(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(locality): Extension<crate::http::host_authority::RequestLocality>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
) -> Response {
    let (grant, effect) = match crate::http::host_authority::authorize_administrative_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality,
        crate::action_authorization::HostAction::ApiTokenManage,
        "deployment_api_tokens",
        tenant
            .deployment_id
            .as_deref()
            .unwrap_or("local-deployment"),
        json!({"operation": "list"}),
    )
    .await
    {
        Ok(authorized) => authorized,
        Err(status) => return status.into_response(),
    };
    if let Err(error) = grant.revalidate(&state, &effect) {
        return crate::http::host_authority::host_authorization_status(error).into_response();
    }
    Json(json!({
        "tokens": state.transport_token_metadata().await,
    }))
    .into_response()
}

pub(super) async fn revoke_api_token(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(locality): Extension<crate::http::host_authority::RequestLocality>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
    Path(token_id): Path<String>,
) -> Response {
    let (grant, effect) = match crate::http::host_authority::authorize_administrative_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality,
        crate::action_authorization::HostAction::ApiTokenManage,
        "deployment_api_token",
        token_id.clone(),
        json!({"operation": "revoke", "token_id": token_id}),
    )
    .await
    {
        Ok(authorized) => authorized,
        Err(status) => return status.into_response(),
    };
    if let Err(error) = grant.revalidate(&state, &effect) {
        return crate::http::host_authority::host_authorization_status(error).into_response();
    }
    match state.revoke_api_token(&token_id).await {
        Ok(true) => Json(json!({"ok": true, "token_id": token_id})).into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(json!({"ok": false, "error": "transport token not found or already revoked"})),
        )
            .into_response(),
        Err(error) => (
            StatusCode::CONFLICT,
            Json(json!({"ok": false, "error": error})),
        )
            .into_response(),
    }
}

fn normalize_api_token_scopes(scopes: Option<Vec<String>>) -> Result<Vec<String>, String> {
    let mut scopes = scopes.unwrap_or_else(|| vec!["engine.api".to_string()]);
    scopes = scopes
        .into_iter()
        .map(|scope| scope.trim().to_ascii_lowercase())
        .filter(|scope| !scope.is_empty())
        .collect();
    scopes.sort();
    scopes.dedup();
    if scopes.is_empty() || scopes.iter().any(|scope| scope != "engine.api") {
        return Err("the supported transport-token scope is engine.api".to_string());
    }
    Ok(scopes)
}

async fn authorize_api_token_management(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<(), Value> {
    if state.api_token_required().await {
        return Ok(());
    }
    let expected = std::env::var("TANDEM_TOKEN_BOOTSTRAP_SECRET")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| json!({
            "ok": false,
            "error": "api token management requires TANDEM_TOKEN_BOOTSTRAP_SECRET before a token exists",
            "code": "TOKEN_BOOTSTRAP_REQUIRED"
        }))?;
    let provided = headers
        .get("x-tandem-bootstrap-token")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            json!({
                "ok": false,
                "error": "missing bootstrap token",
                "code": "TOKEN_BOOTSTRAP_REQUIRED"
            })
        })?;
    if constant_time_token_eq(provided, &expected) {
        Ok(())
    } else {
        Err(json!({
            "ok": false,
            "error": "invalid bootstrap token",
            "code": "TOKEN_BOOTSTRAP_DENIED"
        }))
    }
}

fn constant_time_token_eq(provided: &str, expected: &str) -> bool {
    let provided_hash = Sha256::digest(provided.as_bytes());
    let expected_hash = Sha256::digest(expected.as_bytes());
    let mut diff = 0u8;
    for (left, right) in provided_hash.iter().zip(expected_hash.iter()) {
        diff |= left ^ right;
    }
    diff == 0
}

#[derive(Debug)]
enum ProviderCatalogFetchResult {
    Remote {
        models: HashMap<String, WireProviderModel>,
    },
    Static {
        models: HashMap<String, WireProviderModel>,
    },
    Unavailable {
        message: String,
    },
    Error {
        message: String,
    },
}
