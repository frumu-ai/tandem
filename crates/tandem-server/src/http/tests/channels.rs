// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

#[tokio::test]
async fn channels_config_returns_non_secret_shape() {
    let state = test_state().await;
    let _ = state
        .config
        .patch_project(json!({
            "channels": {
                "telegram": {
                    "bot_token": "tg-secret",
                    "allowed_users": ["@alice", "@bob"],
                    "mention_only": true,
                    "strict_kb_grounding": true
                },
                "discord": {
                    "bot_token": "dc-secret",
                    "allowed_users": ["*"],
                    "mention_only": false,
                    "guild_id": "1234",
                    "strict_kb_grounding": true,
                    "model_provider_id": "openai",
                    "model_id": "gpt-4.1-mini"
                },
                "slack": {
                    "bot_token": "sl-secret",
                    "channel_id": "C123",
                    "allowed_users": ["U1"],
                    "mention_only": true,
                    "strict_kb_grounding": false
                }
            }
        }))
        .await
        .expect("patch project");
    let app = app_router(state);

    let req = Request::builder()
        .method("GET")
        .uri("/channels/config")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("response body");
    let payload: Value = serde_json::from_slice(&body).expect("json body");
    assert_eq!(
        payload
            .get("telegram")
            .and_then(|v| v.get("has_token"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        payload
            .get("telegram")
            .and_then(|v| v.get("token_masked"))
            .and_then(Value::as_str),
        Some("****")
    );
    assert_eq!(
        payload
            .get("discord")
            .and_then(|v| v.get("token_masked"))
            .and_then(Value::as_str),
        Some("****")
    );
    assert_eq!(
        payload
            .get("discord")
            .and_then(|v| v.get("strict_kb_grounding"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        payload
            .get("telegram")
            .and_then(|v| v.get("strict_kb_grounding"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        payload
            .get("discord")
            .and_then(|v| v.get("model_provider_id"))
            .and_then(Value::as_str),
        Some("openai")
    );
    assert_eq!(
        payload
            .get("discord")
            .and_then(|v| v.get("model_id"))
            .and_then(Value::as_str),
        Some("gpt-4.1-mini")
    );
    assert_eq!(
        payload
            .get("slack")
            .and_then(|v| v.get("token_masked"))
            .and_then(Value::as_str),
        Some("****")
    );
    assert!(payload
        .get("telegram")
        .and_then(Value::as_object)
        .is_some_and(|obj| !obj.contains_key("bot_token")));
    assert!(payload
        .get("discord")
        .and_then(Value::as_object)
        .is_some_and(|obj| !obj.contains_key("bot_token")));
    assert!(payload
        .get("slack")
        .and_then(Value::as_object)
        .is_some_and(|obj| !obj.contains_key("bot_token")));
    assert_eq!(
        payload
            .get("slack")
            .and_then(|v| v.get("mention_only"))
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[tokio::test]
async fn channels_put_roundtrips_model_override() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let req = Request::builder()
        .method("PUT")
        .uri("/channels/discord")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "bot_token": "dc-secret",
                "allowed_users": ["*"],
                "mention_only": true,
                "guild_id": "1234",
                "strict_kb_grounding": true,
                "model_provider_id": "openai",
                "model_id": "gpt-4.1-mini"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let config_req = Request::builder()
        .method("GET")
        .uri("/channels/config")
        .body(Body::empty())
        .expect("request");
    let config_resp = app.clone().oneshot(config_req).await.expect("response");
    assert_eq!(config_resp.status(), StatusCode::OK);

    let body = to_bytes(config_resp.into_body(), usize::MAX)
        .await
        .expect("response body");
    let payload: Value = serde_json::from_slice(&body).expect("json body");
    assert_eq!(
        payload
            .get("discord")
            .and_then(|v| v.get("strict_kb_grounding"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        payload
            .get("discord")
            .and_then(|v| v.get("model_provider_id"))
            .and_then(Value::as_str),
        Some("openai")
    );
    assert_eq!(
        payload
            .get("discord")
            .and_then(|v| v.get("model_id"))
            .and_then(Value::as_str),
        Some("gpt-4.1-mini")
    );
}

#[tokio::test]
async fn channels_put_preserves_existing_token_when_only_model_changes() {
    let state = test_state().await;
    let _ = state
        .config
        .patch_project(json!({
            "channels": {
                "discord": {
                    "bot_token": "dc-secret",
                    "allowed_users": ["*"],
                    "mention_only": true,
                    "guild_id": "1234"
                }
            }
        }))
        .await
        .expect("patch project");
    let app = app_router(state.clone());

    let req = Request::builder()
        .method("PUT")
        .uri("/channels/discord")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "allowed_users": ["*"],
                "mention_only": true,
                "guild_id": "1234",
                "strict_kb_grounding": true,
                "model_provider_id": "openai",
                "model_id": "gpt-4.1-mini"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let config_req = Request::builder()
        .method("GET")
        .uri("/channels/config")
        .body(Body::empty())
        .expect("request");
    let config_resp = app.clone().oneshot(config_req).await.expect("response");
    assert_eq!(config_resp.status(), StatusCode::OK);

    let body = to_bytes(config_resp.into_body(), usize::MAX)
        .await
        .expect("response body");
    let payload: Value = serde_json::from_slice(&body).expect("json body");
    assert_eq!(
        payload
            .get("discord")
            .and_then(|v| v.get("strict_kb_grounding"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        payload
            .get("discord")
            .and_then(|v| v.get("has_token"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        payload
            .get("discord")
            .and_then(|v| v.get("model_provider_id"))
            .and_then(Value::as_str),
        Some("openai")
    );
    assert_eq!(
        payload
            .get("discord")
            .and_then(|v| v.get("model_id"))
            .and_then(Value::as_str),
        Some("gpt-4.1-mini")
    );
}

/// PR #1910 review: `GET /channels/config` exposes Slack connections only as
/// a `connections_summary` (never under the real `connections` config key),
/// and a PUT that echoes such a sanitized snapshot back must not wipe the
/// stored installation identity, secrets, or per-connection bindings.
#[tokio::test]
async fn channels_put_echoing_sanitized_config_preserves_slack_connections() {
    let state = test_state().await;
    let _ = state
        .config
        .patch_project(json!({
            "channels": {
                "slack": {
                    "bot_token": "xoxb-secret",
                    "channel_id": "C_MAIN",
                    "signing_secret": "shhh",
                    "events_enabled": true,
                    "team_id": "T1",
                    "app_id": "A1",
                    "tenant": { "org_id": "acme", "workspace_id": "hq" },
                    "connections": [
                        { "channel_id": "C_SALES", "org_units": ["sales"] }
                    ]
                }
            }
        }))
        .await
        .expect("patch project");
    let app = app_router(state.clone());

    // The config snapshot must carry the summary under a non-config key.
    let config_req = Request::builder()
        .method("GET")
        .uri("/channels/config")
        .body(Body::empty())
        .expect("request");
    let config_resp = app.clone().oneshot(config_req).await.expect("response");
    let body = to_bytes(config_resp.into_body(), usize::MAX)
        .await
        .expect("response body");
    let payload: Value = serde_json::from_slice(&body).expect("json body");
    let slack = payload.get("slack").expect("slack entry");
    assert!(
        slack.get("connections").is_none(),
        "the sanitized snapshot must not expose a `connections` config key"
    );
    assert_eq!(
        slack.get("allowed_users"),
        Some(&json!([])),
        "a missing Slack allowlist is deny-all on signed ingress and must \
         not be reported as a '*' wildcard"
    );
    let summary = slack
        .get("connections_summary")
        .and_then(Value::as_array)
        .expect("connections_summary rows");
    assert_eq!(summary.len(), 2, "top-level + per-connection entries");

    // Echo a sanitized snapshot back (the Channels page Reconnect shape):
    // no secrets, no connections, the snapshot's (empty) allowlist, plus
    // the summary key the server ignores.
    let put_req = Request::builder()
        .method("PUT")
        .uri("/channels/slack")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "channel_id": "C_MAIN",
                "allowed_users": slack.get("allowed_users"),
                "mention_only": false,
                "connections_summary": summary,
            })
            .to_string(),
        ))
        .expect("request");
    let put_resp = app.clone().oneshot(put_req).await.expect("response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let stored = state.config.get_project_value().await;
    let slack = stored
        .pointer("/channels/slack")
        .expect("stored slack config");
    assert_eq!(
        slack.pointer("/signing_secret").and_then(Value::as_str),
        Some("shhh"),
        "echoed sanitized config must not wipe the signing secret"
    );
    assert_eq!(
        slack.pointer("/team_id").and_then(Value::as_str),
        Some("T1")
    );
    assert_eq!(
        slack.pointer("/events_enabled").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        slack.pointer("/tenant/org_id").and_then(Value::as_str),
        Some("acme")
    );
    assert_eq!(
        slack
            .pointer("/connections/0/channel_id")
            .and_then(Value::as_str),
        Some("C_SALES"),
        "echoed sanitized config must not wipe per-connection entries"
    );
    assert!(
        slack
            .get("allowed_users")
            .and_then(Value::as_array)
            .is_none_or(Vec::is_empty),
        "echoing the snapshot must not widen a deny-all allowlist to '*'"
    );
}

#[tokio::test]
async fn channels_verify_discord_without_token_returns_setup_hint() {
    let state = test_state().await;
    let app = app_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/channels/discord/verify")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("response body");
    let payload: Value = serde_json::from_slice(&body).expect("json body");
    assert_eq!(
        payload.get("ok").and_then(Value::as_bool),
        Some(false),
        "verify should fail without token"
    );
    assert_eq!(
        payload.get("channel").and_then(Value::as_str),
        Some("discord")
    );
    assert!(
        payload
            .get("hints")
            .and_then(Value::as_array)
            .is_some_and(|arr| !arr.is_empty()),
        "verify should include setup hints"
    );
}

#[tokio::test]
async fn channels_put_normalizes_empty_allowed_users_to_wildcard() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let req = Request::builder()
        .method("PUT")
        .uri("/channels/telegram")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "bot_token": "tg-secret",
                "allowed_users": [],
                "mention_only": false
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let effective = state.config.get_effective_value().await;
    assert_eq!(
        effective
            .get("channels")
            .and_then(|v| v.get("telegram"))
            .and_then(|v| v.get("allowed_users"))
            .and_then(Value::as_array)
            .cloned(),
        Some(vec![Value::String("*".to_string())])
    );
}

/// PR #1910 review: a FRESH Slack save (nothing stored) that omits
/// `allowed_users` must persist an explicit empty allowlist — not
/// `SlackConfigFile`'s legacy `["*"]` serde default, which would open
/// signed Events ingress to every Slack user.
#[tokio::test]
async fn channels_put_fresh_slack_save_without_allowlist_stays_deny_all() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let req = Request::builder()
        .method("PUT")
        .uri("/channels/slack")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "bot_token": "xoxb-fresh",
                "channel_id": "C_FRESH"
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let stored = state.config.get_project_value().await;
    assert_eq!(
        stored.pointer("/channels/slack/allowed_users"),
        Some(&json!([])),
        "a fresh save without an allowlist must not persist the '*' wildcard"
    );
}

/// PR #1910 review (round 19): a save that CHANGES the Slack installation
/// identity must not carry the previous installation's secrets across — the
/// preservation block reads the effective config (keystore-injected
/// secrets), and re-hoisting them under the new identity would let app B
/// verify with app A's credentials instead of failing closed.
#[tokio::test]
async fn channels_put_migrating_installation_does_not_carry_secrets() {
    let state = test_state().await;
    let _ = state
        .config
        .patch_project(json!({
            "channels": {
                "slack": {
                    "bot_token": "xoxb-top",
                    "channel_id": "C_MAIN",
                    "team_id": "T1",
                    "app_id": "A_OLD",
                    "signing_secret": "shh-old",
                    "events_enabled": true,
                    "connections": [
                        { "channel_id": "C_SALES", "bot_token": "xoxb-sales" }
                    ]
                }
            }
        }))
        .await
        .expect("patch project");
    let app = app_router(state.clone());

    // Sanity: the effective config resolves the old installation's secrets.
    let effective = state.config.get_effective_value().await;
    let connections = crate::config::channels::slack_connections_from_effective_config(&effective);
    assert!(connections
        .iter()
        .any(|connection| connection.channel_id == "C_SALES"
            && connection.bot_token.as_deref() == Some("xoxb-sales")));

    // Sanitized echo that migrates the app identity WITHOUT credentials: the
    // old installation's bot token must not be carried under the new
    // identity, so the save has no usable connection left and fails closed
    // instead of appearing usable while delivery is broken.
    let req = Request::builder()
        .method("PUT")
        .uri("/channels/slack")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "channel_id": "C_MAIN",
                "team_id": "T1",
                "app_id": "A_NEW",
                "allowed_users": []
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "a migrating save without new credentials must be rejected"
    );

    // The same migration WITH the new installation's bot token succeeds —
    // and still must not carry the old app's secrets.
    let req = Request::builder()
        .method("PUT")
        .uri("/channels/slack")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "channel_id": "C_MAIN",
                "team_id": "T1",
                "app_id": "A_NEW",
                "bot_token": "xoxb-new",
                "allowed_users": []
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let effective = state.config.get_effective_value().await;
    assert_eq!(
        effective
            .pointer("/channels/slack/bot_token")
            .and_then(Value::as_str),
        Some("xoxb-new"),
        "the migrated installation must run on the newly supplied token"
    );
    assert!(
        effective
            .pointer("/channels/slack/signing_secret")
            .and_then(Value::as_str)
            .is_none_or(|secret| secret != "shh-old"),
        "the old app's signing secret must not follow the new identity"
    );
    let connections = crate::config::channels::slack_connections_from_effective_config(&effective);
    let sales = connections
        .iter()
        .find(|connection| connection.channel_id == "C_SALES")
        .expect("connection structure preserved");
    assert!(
        sales.bot_token.as_deref() != Some("xoxb-sales"),
        "an inheriting connection must not carry the old installation's token"
    );
    assert!(
        sales.signing_secret.is_none(),
        "the migrated installation must fail closed until its own secret is set"
    );
}

/// PR #1910 review: a migrating save that stays valid through a
/// self-declared connection must REVOKE the stored top-level bot token —
/// deleting it from the keystore — or injection would resurrect the old
/// app's token under the new installation identity on the next read.
#[tokio::test]
async fn channels_put_migrating_installation_revokes_stored_top_level_token() {
    let state = test_state().await;
    let _ = state
        .config
        .patch_project(json!({
            "channels": {
                "slack": {
                    "bot_token": "xoxb-top",
                    "channel_id": "C_MAIN",
                    "team_id": "T1",
                    "app_id": "A_OLD",
                    "connections": [
                        {
                            "channel_id": "C_SALES",
                            "team_id": "T2",
                            "app_id": "A2",
                            "bot_token": "xoxb-sales"
                        }
                    ]
                }
            }
        }))
        .await
        .expect("patch project");
    let app = app_router(state.clone());

    // Sanity: the stored top-level token is injected into the effective config.
    let effective = state.config.get_effective_value().await;
    assert_eq!(
        effective
            .pointer("/channels/slack/bot_token")
            .and_then(Value::as_str),
        Some("xoxb-top")
    );

    // Migrate the installation without a new token: the save stays valid
    // because C_SALES self-declares its installation and token.
    let req = Request::builder()
        .method("PUT")
        .uri("/channels/slack")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "channel_id": "C_MAIN",
                "team_id": "T1",
                "app_id": "A_NEW",
                "allowed_users": []
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);

    let effective = state.config.get_effective_value().await;
    assert!(
        effective
            .pointer("/channels/slack/bot_token")
            .and_then(Value::as_str)
            .is_none_or(|token| token.trim().is_empty()),
        "the old installation's stored token must not resurface under the new identity"
    );
    let connections = crate::config::channels::slack_connections_from_effective_config(&effective);
    assert!(
        connections
            .iter()
            .any(|connection| connection.channel_id == "C_SALES"
                && connection.bot_token.as_deref() == Some("xoxb-sales")),
        "the self-declared connection keeps its own credentials"
    );
}

// PR #1910 review: a fresh multi-connection config that carries channel_id
/// and bot_token only inside `connections[]` is a valid save — startability
/// is judged on the resolved connections, not the legacy top-level fields.
#[tokio::test]
async fn channels_put_accepts_connection_only_slack_configs() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let req = Request::builder()
        .method("PUT")
        .uri("/channels/slack")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "team_id": "T1",
                "app_id": "A1",
                "signing_secret": "shh",
                "events_enabled": true,
                "connections": [
                    { "channel_id": "C_SALES", "bot_token": "xoxb-sales" }
                ]
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "connection-only Slack configs must be accepted"
    );
    let effective = state.config.get_effective_value().await;
    let connections = crate::config::channels::slack_connections_from_effective_config(&effective);
    assert!(
        connections
            .iter()
            .any(|connection| connection.channel_id == "C_SALES"
                && connection.bot_token.as_deref() == Some("xoxb-sales")),
        "the resolved connection must be usable after the save"
    );

    // `GET /channels/config` reports `channel_id: null` for connection-only
    // configs; a client echoing that snapshot back (the Channels page
    // Reconnect) must not 400 on the explicit null before the preserved
    // `connections[]` can make the config startable.
    let req = Request::builder()
        .method("PUT")
        .uri("/channels/slack")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "channel_id": null,
                "events_enabled": true,
                "mention_only": false,
                "allowed_users": []
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "echoing the sanitized snapshot's channel_id: null must not 400"
    );
    let effective = state.config.get_effective_value().await;
    let connections = crate::config::channels::slack_connections_from_effective_config(&effective);
    assert!(
        connections
            .iter()
            .any(|connection| connection.channel_id == "C_SALES"
                && connection.bot_token.as_deref() == Some("xoxb-sales")),
        "the preserved connection must survive a null-channel_id echo"
    );

    // A config with no usable connection anywhere is still rejected (fresh
    // state, so nothing stored can be inherited to make it startable).
    let state = test_state().await;
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("PUT")
        .uri("/channels/slack")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "team_id": "T1",
                "connections": [ { "channel_id": "C_TOKENLESS" } ]
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "a config with no usable connection must still be rejected"
    );
}

#[tokio::test]
async fn channels_put_unknown_channel_returns_not_found() {
    let state = test_state().await;
    let app = app_router(state);

    let req = Request::builder()
        .method("PUT")
        .uri("/channels/unknown")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "bot_token": "x" }).to_string()))
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn channels_delete_unknown_channel_returns_not_found() {
    let state = test_state().await;
    let app = app_router(state);

    let req = Request::builder()
        .method("DELETE")
        .uri("/channels/unknown")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn channels_verify_unknown_channel_returns_not_found() {
    let state = test_state().await;
    let app = app_router(state);

    let req = Request::builder()
        .method("POST")
        .uri("/channels/unknown/verify")
        .body(Body::empty())
        .expect("request");
    let resp = app.clone().oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
