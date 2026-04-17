
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

async fn refresh_openai_codex_oauth_if_needed(state: &AppState) -> anyhow::Result<()> {
    let Some(mut credential) =
        tandem_core::load_provider_oauth_credential(OPENAI_CODEX_PROVIDER_ID)
    else {
        return Ok(());
    };
    if credential.managed_by != "tandem" {
        return refresh_openai_codex_cli_oauth_if_needed(state).await;
    }
    let now = crate::now_ms();
    if credential.expires_at_ms > now.saturating_add(OPENAI_CODEX_OAUTH_REFRESH_SKEW_MS) {
        return Ok(());
    }

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
    let text = response.text().await?;
    if !status.is_success() {
        anyhow::bail!("refresh failed with status {status}: {text}");
    }
    let refresh = serde_json::from_str::<OpenAiCodexTokenExchangeResponse>(&text)?;
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
            credential.api_key = Some(api_key.clone());
            state
                .auth
                .write()
                .await
                .insert(OPENAI_CODEX_PROVIDER_ID.to_string(), api_key);
        }
    }
    let _ = tandem_core::set_provider_oauth_credential(OPENAI_CODEX_PROVIDER_ID, credential)?;
    ensure_openai_codex_runtime_provider(state).await;
    state
        .providers
        .reload(state.config.get().await.into())
        .await;
    Ok(())
}

async fn refresh_openai_codex_cli_oauth_if_needed(state: &AppState) -> anyhow::Result<()> {
    let Some(existing) = tandem_core::load_provider_oauth_credential(OPENAI_CODEX_PROVIDER_ID)
    else {
        return Ok(());
    };
    if existing.managed_by != "codex-cli" {
        return Ok(());
    }

    let Some(incoming) = tandem_core::load_openai_codex_cli_oauth_credential() else {
        return Ok(());
    };
    if existing == incoming {
        return Ok(());
    }

    tandem_core::set_provider_oauth_credential(OPENAI_CODEX_PROVIDER_ID, incoming)?;
    ensure_openai_codex_runtime_provider(state).await;
    state
        .providers
        .reload(state.config.get().await.into())
        .await;
    Ok(())
}

async fn ensure_openai_codex_runtime_provider(state: &AppState) {
    let _ = state
        .config
        .patch_runtime(json!({
            "providers": {
                OPENAI_CODEX_PROVIDER_ID: {
                    "url": OPENAI_CODEX_API_BASE_URL,
                    "default_model": OPENAI_CODEX_DEFAULT_MODEL,
                }
            }
        }))
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
        let sessions = state.provider_oauth_sessions.read().await;
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
            .provider_oauth_sessions
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
            .provider_oauth_sessions
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

    let exchanged =
        match exchange_openai_codex_code(code, &session.redirect_uri, &session.code_verifier).await
        {
            Ok(value) => value,
            Err(error) => {
                if let Some(entry) = state
                    .provider_oauth_sessions
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

    let backend = match tandem_core::set_provider_oauth_credential(
        OPENAI_CODEX_PROVIDER_ID,
        oauth_credential,
    ) {
        Ok(tandem_core::ProviderAuthBackend::Keychain) => "keychain",
        Ok(tandem_core::ProviderAuthBackend::File) => "file",
        Err(error) => return json!({"ok": false, "error": error.to_string()}),
    };

    ensure_openai_codex_runtime_provider(&state).await;
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

    if let Some(entry) = state
        .provider_oauth_sessions
        .write()
        .await
        .get_mut(&session_id)
    {
        entry.status = "connected".to_string();
        entry.error = None;
        entry.email = email.clone();
    }

    let _ = crate::audit::append_protected_audit_event(
        &state,
        "provider.oauth.updated",
        &tandem_types::TenantContext::local_implicit(),
        None,
        json!({
            "providerID": OPENAI_CODEX_PROVIDER_ID,
            "backend": backend,
            "managedBy": "tandem",
            "email": email,
        }),
    )
    .await;

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

fn merge_json(base: &mut Value, overlay: &Value) {
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
                    Some(existing) => merge_json(existing, value),
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
