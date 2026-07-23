// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

pub(super) async fn auth_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
    axum::extract::Extension(tenant_context): axum::extract::Extension<TenantContext>,
    headers: HeaderMap,
) -> Json<Value> {
    let public_base_url = mcp_public_base_url_from_headers(&headers);
    if let Some(auth_challenge) =
        current_mcp_auth_challenge_for_tenant(&state, &name, &tenant_context).await
    {
        let existing_redirect =
            mcp_oauth_redirect_uri_from_authorization_url(&auth_challenge.authorization_url);
        let desired_redirect = public_base_url
            .as_deref()
            .map(|base_url| mcp_oauth_redirect_uri_for_base(base_url, &name));
        let redirect_matches = match desired_redirect.as_deref() {
            Some(redirect_uri) => existing_redirect.as_deref() == Some(redirect_uri),
            None => true,
        };
        if redirect_matches {
            return Json(json!({
                "ok": true,
                "pending": true,
                "lastAuthChallenge": auth_challenge,
                "authorizationUrl": auth_challenge.authorization_url,
            }));
        }
        let _ = state
            .mcp
            .clear_auth_challenge_for_tenant(&name, &tenant_context)
            .await;
        state
            .oauth
            .retain_mcp_sessions(|pending| {
                pending.server_name != name || pending.tenant_context != tenant_context
            })
            .await;
    }
    let server = state.mcp.list().await.get(&name).cloned();
    if server.as_ref().is_some_and(mcp_uses_oauth) {
        if let Ok(auth_challenge) =
            start_mcp_oauth_session(&state, &name, &tenant_context, public_base_url.as_deref())
                .await
        {
            return Json(json!({
                "ok": true,
                "pending": true,
                "lastAuthChallenge": auth_challenge,
                "authorizationUrl": auth_challenge.authorization_url,
            }));
        }
    }
    Json(json!({
        "ok": false,
        "pending": false,
        "name": name,
        "message": "No MCP auth challenge recorded yet.",
    }))
}

pub(super) async fn callback_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
    tenant_context: Option<axum::extract::Extension<TenantContext>>,
    headers: HeaderMap,
) -> Json<Value> {
    let tenant_context = tenant_context
        .map(|extension| extension.0)
        .unwrap_or_else(TenantContext::local_implicit);
    authenticate_mcp(
        State(state),
        Path(name),
        axum::extract::Extension(tenant_context),
        headers,
    )
    .await
}

pub(super) async fn callback_mcp_get(
    State(state): State<AppState>,
    Path(name): Path<String>,
    tenant_context: Option<axum::extract::Extension<TenantContext>>,
    Query(input): Query<McpOAuthCallbackInput>,
) -> impl IntoResponse {
    let request_tenant = tenant_context.map(|extension| extension.0);
    match finish_mcp_oauth_callback(state, name, request_tenant, input).await {
        Ok(()) => mcp_oauth_callback_html(
            true,
            "Tandem MCP Connected",
            "The MCP OAuth sign-in completed successfully. You can close this window.",
        )
        .into_response(),
        Err(error) => {
            mcp_oauth_callback_html(false, "Tandem MCP OAuth Failed", &error).into_response()
        }
    }
}

pub(super) async fn authenticate_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
    axum::extract::Extension(tenant_context): axum::extract::Extension<TenantContext>,
    headers: HeaderMap,
) -> Json<Value> {
    let public_base_url = mcp_public_base_url_from_headers(&headers);
    if let Some(session) = find_pending_mcp_oauth_session(&state, &name, &tenant_context).await {
        let desired_redirect_uri = public_base_url
            .as_deref()
            .map(|base_url| mcp_oauth_redirect_uri_for_base(base_url, &name));
        if desired_redirect_uri
            .as_deref()
            .is_some_and(|redirect_uri| redirect_uri != session.redirect_uri)
        {
            let _ = state
                .mcp
                .clear_auth_challenge_for_tenant(&name, &tenant_context)
                .await;
            state
                .oauth
                .retain_mcp_sessions(|pending| {
                    pending.server_name != name || pending.tenant_context != tenant_context
                })
                .await;
        } else {
            let last_auth_challenge = mcp_auth_challenge_from_session(&session);
            let authorization_url = last_auth_challenge.authorization_url.clone();
            return Json(json!({
                "ok": true,
                "authenticated": false,
                "connected": false,
                "pendingAuth": true,
                "lastAuthChallenge": last_auth_challenge,
                "authorizationUrl": authorization_url,
            }));
        }
    }

    let refresh = state.mcp.refresh_for_tenant(&name, &tenant_context).await;
    let current = state.mcp.list().await.get(&name).cloned();
    let last_auth_challenge = state
        .mcp
        .auth_challenge_for_tenant(&name, &tenant_context)
        .await;
    match refresh {
        Ok(tools) => {
            let count = sync_mcp_tools_for_server_for_tenant(&state, &name, &tenant_context).await;
            let _ = state
                .mcp
                .clear_auth_challenge_for_tenant(&name, &tenant_context)
                .await;
            Json(json!({
                "ok": true,
                "authenticated": true,
                "connected": true,
                "pendingAuth": false,
                "lastAuthChallenge": Value::Null,
                "authorizationUrl": Value::Null,
                "count": count.max(tools.len()),
            }))
        }
        Err(error) => {
            let mut auth_challenge = last_auth_challenge;
            let connected = if let Some(server) = current.as_ref() {
                state
                    .mcp
                    .runtime_connected_for_tenant(&name, server, &tenant_context)
                    .await
            } else {
                false
            };
            if auth_challenge.is_none() {
                let server = state.mcp.list().await.get(&name).cloned();
                if server.as_ref().is_some_and(mcp_uses_oauth) {
                    auth_challenge = start_mcp_oauth_session(
                        &state,
                        &name,
                        &tenant_context,
                        public_base_url.as_deref(),
                    )
                    .await
                    .ok();
                }
            }
            Json(json!({
                "ok": false,
                "authenticated": false,
                "connected": connected,
                "pendingAuth": auth_challenge.is_some(),
                "lastAuthChallenge": auth_challenge,
                "authorizationUrl": auth_challenge.as_ref().map(|challenge| challenge.authorization_url.clone()),
                "error": error,
                "code": ErrorCode::McpOauthFailed,
                "retryable": false,
            }))
        }
    }
}

pub(super) async fn delete_auth_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
    axum::extract::Extension(tenant_context): axum::extract::Extension<TenantContext>,
    axum::extract::Extension(locality): axum::extract::Extension<
        crate::http::host_authority::RequestLocality,
    >,
    verified: Option<axum::extract::Extension<VerifiedTenantContext>>,
) -> Response {
    let (grant, effect) = match crate::http::host_authority::authorize_administrative_effect(
        &state,
        &tenant_context,
        verified.as_deref(),
        locality,
        crate::action_authorization::HostAction::McpServerManage,
        "mcp_server_auth",
        name.clone(),
        json!({"operation": "delete_auth", "name": name}),
    )
    .await
    {
        Ok(authorized) => authorized,
        Err(status) => return status.into_response(),
    };
    if let Err(error) = grant.revalidate(&state, &effect) {
        return crate::http::host_authority::host_authorization_status(error).into_response();
    }
    let removed_tool_count = unregister_mcp_bridge_tools_for_server(&state, &name).await;
    let removed_oauth_session_count = remove_mcp_oauth_sessions_for_server(&state, &name).await;
    let ok = state
        .mcp
        .clear_auth_material_for_tenant(&name, &tenant_context)
        .await;
    if ok {
        state.event_bus.publish(EngineEvent::new(
            "mcp.server.auth.deleted",
            json!({
                "name": name,
                "removedToolCount": removed_tool_count,
                "removedOauthSessionCount": removed_oauth_session_count,
            }),
        ));
    }
    Json(json!({
        "ok": ok,
        "removedToolCount": removed_tool_count,
        "removedOauthSessionCount": removed_oauth_session_count,
    }))
    .into_response()
}

pub(super) async fn mcp_tools(
    State(state): State<AppState>,
    tenant_context: Option<axum::extract::Extension<TenantContext>>,
) -> Json<Value> {
    let tenant_context = tenant_context
        .map(|extension| extension.0)
        .unwrap_or_else(TenantContext::local_implicit);
    Json(json!(
        state.mcp.list_tools_for_tenant(&tenant_context).await
    ))
}

pub(super) async fn mcp_resources(State(state): State<AppState>) -> Json<Value> {
    let resources = state
        .mcp
        .list()
        .await
        .into_values()
        .filter(|server| server.connected)
        .map(|server| {
            json!({
                "server": server.name,
                "resources": [
                    {"uri": format!("mcp://{}/tools", server.name), "name":"tools"},
                    {"uri": format!("mcp://{}/prompts", server.name), "name":"prompts"}
                ]
            })
        })
        .collect::<Vec<_>>();
    Json(json!(resources))
}
