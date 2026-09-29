// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

struct PackBuilderPromptProvider {
    tool: String,
    args: Value,
    streams: std::sync::atomic::AtomicUsize,
}

#[async_trait::async_trait]
impl tandem_providers::Provider for PackBuilderPromptProvider {
    fn info(&self) -> tandem_types::ProviderInfo {
        tandem_types::ProviderInfo {
            id: "pack-builder-prompt-test".to_string(),
            name: "Pack Builder Prompt Test".to_string(),
            models: vec![tandem_types::ModelInfo {
                id: "pack-builder-prompt-test-1".to_string(),
                provider_id: "pack-builder-prompt-test".to_string(),
                display_name: "Pack Builder Prompt Test".to_string(),
                context_window: 8192,
            }],
        }
    }

    async fn complete(
        &self,
        _prompt: &str,
        _model_override: Option<&str>,
    ) -> anyhow::Result<String> {
        Ok("Done".to_string())
    }

    async fn stream(
        &self,
        _messages: Vec<tandem_providers::ChatMessage>,
        _model_override: Option<&str>,
        _tool_mode: tandem_types::ToolMode,
        _tools: Option<Vec<tandem_types::ToolSchema>>,
        _sampling: tandem_types::SamplingParams,
        _cancel: tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<
        std::pin::Pin<
            Box<dyn futures::Stream<Item = anyhow::Result<tandem_providers::StreamChunk>> + Send>,
        >,
    > {
        use std::sync::atomic::Ordering;
        let chunks = if self.streams.fetch_add(1, Ordering::SeqCst) == 0 {
            vec![
                Ok(tandem_providers::StreamChunk::ToolCallStart {
                    id: "pack-builder-call".to_string(),
                    name: self.tool.clone(),
                }),
                Ok(tandem_providers::StreamChunk::ToolCallDelta {
                    id: "pack-builder-call".to_string(),
                    args_delta: self.args.to_string(),
                }),
                Ok(tandem_providers::StreamChunk::ToolCallEnd {
                    id: "pack-builder-call".to_string(),
                }),
                Ok(tandem_providers::StreamChunk::Done {
                    finish_reason: "tool_calls".to_string(),
                    usage: None,
                }),
            ]
        } else {
            vec![
                Ok(tandem_providers::StreamChunk::TextDelta("Done".to_string())),
                Ok(tandem_providers::StreamChunk::Done {
                    finish_reason: "stop".to_string(),
                    usage: None,
                }),
            ]
        };
        Ok(Box::pin(futures::stream::iter(chunks)))
    }
}

async fn register_pack_builder_tool(state: &AppState) {
    state
        .runtime
        .get()
        .expect("runtime")
        .permissions
        .add_rule(
            "pack_builder",
            "pack_builder",
            tandem_core::PermissionAction::Allow,
        )
        .await;
    state
        .tools
        .register_tool(
            "pack_builder".to_string(),
            Arc::new(crate::pack_builder::PackBuilderTool::new(state.clone())),
        )
        .await;
}

fn direct_pack_builder_loopback() -> axum::extract::ConnectInfo<std::net::SocketAddr> {
    axum::extract::ConnectInfo("127.0.0.1:39731".parse().expect("loopback peer"))
}

#[tokio::test]
async fn pack_builder_direct_routes_reject_remote_peer() {
    let state = test_state().await;
    register_pack_builder_tool(&state).await;
    let app = app_router(state.clone());
    let remote_peer = axum::extract::ConnectInfo(
        "203.0.113.7:39731"
            .parse::<std::net::SocketAddr>()
            .expect("remote peer"),
    );

    for (method, path, body) in [
        (
            "POST",
            "/pack-builder/preview",
            json!({
                "goal": "Summarize local notes daily",
                "session_id": "remote-denied",
                "thread_key": "remote",
                "auto_apply": true
            })
            .to_string(),
        ),
        ("POST", "/pack-builder/apply", "{}".to_string()),
        ("POST", "/pack-builder/cancel", "{}".to_string()),
        ("GET", "/pack-builder/pending", String::new()),
    ] {
        let request = Request::builder()
            .method(method)
            .uri(path)
            .extension(remote_peer)
            .header("content-type", "application/json")
            .body(Body::from(body))
            .expect("pack builder request");
        let response = app.clone().oneshot(request).await.expect("response");
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "{method} {path}");
    }

    let preview_body = json!({"goal": "Summarize local notes daily"}).to_string();
    let missing_peer = Request::builder()
        .method("POST")
        .uri("/pack-builder/preview")
        .header("content-type", "application/json")
        .body(Body::from(preview_body.clone()))
        .expect("missing-peer request");
    assert_eq!(
        app.clone()
            .oneshot(missing_peer)
            .await
            .expect("response")
            .status(),
        StatusCode::FORBIDDEN
    );
    let forwarded_loopback = Request::builder()
        .method("POST")
        .uri("/pack-builder/preview")
        .extension(direct_pack_builder_loopback())
        .header("x-forwarded-for", "203.0.113.7")
        .header("content-type", "application/json")
        .body(Body::from(preview_body.clone()))
        .expect("forwarded request");
    assert_eq!(
        app.clone()
            .oneshot(forwarded_loopback)
            .await
            .expect("response")
            .status(),
        StatusCode::FORBIDDEN
    );
    let pending_request = Request::builder()
        .method("GET")
        .uri("/pack-builder/pending?session_id=remote-denied&thread_key=remote")
        .extension(direct_pack_builder_loopback())
        .body(Body::empty())
        .expect("pending request");
    let pending_response = app
        .clone()
        .oneshot(pending_request)
        .await
        .expect("response");
    assert_eq!(pending_response.status(), StatusCode::OK);
    let pending_body = to_bytes(pending_response.into_body(), usize::MAX)
        .await
        .expect("pending body");
    let pending: Value = serde_json::from_slice(&pending_body).expect("pending json");
    assert!(pending.get("pending").is_none_or(Value::is_null));

    let local_request = Request::builder()
        .method("POST")
        .uri("/pack-builder/preview")
        .extension(direct_pack_builder_loopback())
        .header("content-type", "application/json")
        .body(Body::from(preview_body.clone()))
        .expect("local request");
    assert_eq!(
        app.clone()
            .oneshot(local_request)
            .await
            .expect("response")
            .status(),
        StatusCode::OK
    );
    state.set_http_listener_bound_loopback_only(false);
    let wildcard_listener_request = Request::builder()
        .method("POST")
        .uri("/pack-builder/preview")
        .extension(direct_pack_builder_loopback())
        .header("content-type", "application/json")
        .body(Body::from(preview_body))
        .expect("wildcard-listener request");
    assert_eq!(
        app.oneshot(wildcard_listener_request)
            .await
            .expect("response")
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn pack_builder_generic_tool_route_rejects_remote_aliases_and_batch() {
    let state = test_state().await;
    register_pack_builder_tool(&state).await;
    state
        .runtime
        .get()
        .expect("runtime")
        .permissions
        .add_rule("batch", "batch", tandem_core::PermissionAction::Allow)
        .await;
    let app = app_router(state.clone());
    let remote_peer = axum::extract::ConnectInfo(
        "203.0.113.7:39731"
            .parse::<std::net::SocketAddr>()
            .expect("remote peer"),
    );
    for tool in [
        "pack_builder",
        "pack-builder",
        "functions.pack_builder",
        "default_api:pack_builder",
    ] {
        let request = Request::builder()
            .method("POST")
            .uri("/tool/execute")
            .extension(remote_peer)
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "tool": tool,
                    "args": {
                        "mode": "preview",
                        "goal": "Summarize local notes daily",
                        "auto_apply": true,
                        "__session_id": "remote-generic-denied",
                        "thread_key": "remote"
                    }
                })
                .to_string(),
            ))
            .expect("generic remote request");
        let response = app.clone().oneshot(request).await.expect("response");
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "tool alias {tool}"
        );
    }

    let preview = json!({
        "tool": "pack_builder",
        "args": {"mode": "preview", "goal": "Summarize local notes daily"}
    })
    .to_string();
    let missing_peer = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .header("content-type", "application/json")
        .body(Body::from(preview.clone()))
        .expect("missing-peer request");
    assert_eq!(
        app.clone()
            .oneshot(missing_peer)
            .await
            .expect("response")
            .status(),
        StatusCode::FORBIDDEN
    );
    let forwarded_loopback = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .extension(direct_pack_builder_loopback())
        .header("x-forwarded-for", "203.0.113.7")
        .header("content-type", "application/json")
        .body(Body::from(preview.clone()))
        .expect("forwarded request");
    assert_eq!(
        app.clone()
            .oneshot(forwarded_loopback)
            .await
            .expect("response")
            .status(),
        StatusCode::FORBIDDEN
    );

    let batch_request = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .extension(remote_peer)
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tool": "batch",
                "args": {"tool_calls": [{
                    "tool": "functions.pack_builder",
                    "args": {
                        "mode": "preview",
                        "goal": "Summarize local notes daily",
                        "auto_apply": true,
                        "__session_id": "remote-generic-denied",
                        "thread_key": "remote"
                    }
                }]}
            })
            .to_string(),
        ))
        .expect("batch request");
    let batch_response = app
        .clone()
        .oneshot(batch_request)
        .await
        .expect("batch response");
    assert_eq!(batch_response.status(), StatusCode::OK);
    let batch_body = to_bytes(batch_response.into_body(), usize::MAX)
        .await
        .expect("batch body");
    let batch_payload: Value = serde_json::from_slice(&batch_body).expect("batch json");
    let rows: Value = serde_json::from_str(batch_payload["output"].as_str().expect("batch output"))
        .expect("batch rows");
    assert_eq!(rows[0]["status"], "error");
    assert!(
        rows[0]["error"].as_str().is_some_and(
            |error| error.contains("pack_builder requires the local single-user runtime")
        )
    );

    let pending_request = Request::builder()
        .method("GET")
        .uri("/pack-builder/pending?session_id=remote-generic-denied&thread_key=remote")
        .extension(direct_pack_builder_loopback())
        .body(Body::empty())
        .expect("pending request");
    let pending_response = app
        .clone()
        .oneshot(pending_request)
        .await
        .expect("pending response");
    assert_eq!(pending_response.status(), StatusCode::OK);
    let pending_body = to_bytes(pending_response.into_body(), usize::MAX)
        .await
        .expect("pending body");
    let pending: Value = serde_json::from_slice(&pending_body).expect("pending json");
    assert!(pending.get("pending").is_none_or(Value::is_null));

    state.set_http_listener_bound_loopback_only(false);
    let wildcard_listener = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .extension(direct_pack_builder_loopback())
        .header("content-type", "application/json")
        .body(Body::from(preview))
        .expect("wildcard-listener request");
    assert_eq!(
        app.oneshot(wildcard_listener)
            .await
            .expect("response")
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn pack_builder_dispatch_rejects_automation_preflight_without_local_http_authority() {
    let state = test_state().await;
    register_pack_builder_tool(&state).await;

    // Automation specifications may contain required_tool_calls. A remote
    // caller must not gain host-local Pack Builder authority by scheduling one.
    for source in ["automation_preflight", "engine_loop"] {
        let context = state.tool_dispatch_context(
            tandem_tools::ToolDispatchSource::new(source),
            tandem_types::TenantContext::local_implicit(),
            vec!["pack_builder".to_string()],
        );
        let error = state
            .tool_dispatcher
            .dispatch("pack_builder", json!({"mode": "pending"}), context)
            .await
            .expect_err("autonomous dispatch must not grant Pack Builder authority");
        assert!(
            error
                .to_string()
                .contains("pack_builder requires the local single-user runtime"),
            "{source}: {error}"
        );
    }
}

#[tokio::test]
async fn pack_builder_prompt_run_authority_is_local_and_bound_to_exact_run() {
    let state = test_state().await;
    let tenant = tandem_types::TenantContext::local_implicit();
    let loopback_peer = "127.0.0.1:39731".parse().expect("loopback peer");
    let remote_peer = "203.0.113.7:39731".parse().expect("remote peer");
    let direct = crate::http::host_authority::RequestLocality::from_peer_and_headers(
        Some(loopback_peer),
        &HeaderMap::new(),
    );
    let remote = crate::http::host_authority::RequestLocality::from_peer_and_headers(
        Some(remote_peer),
        &HeaderMap::new(),
    );
    let mut forwarded_headers = HeaderMap::new();
    forwarded_headers.insert("x-forwarded-for", "203.0.113.7".parse().unwrap());
    let forwarded = crate::http::host_authority::RequestLocality::from_peer_and_headers(
        Some(loopback_peer),
        &forwarded_headers,
    );
    assert!(
        crate::http::host_authority::prompt_has_local_pack_builder_authority(
            &state,
            &tenant,
            None,
            Some(direct),
        )
    );
    for locality in [Some(remote), Some(forwarded), None] {
        assert!(
            !crate::http::host_authority::prompt_has_local_pack_builder_authority(
                &state, &tenant, None, locality,
            )
        );
    }

    let session_id = "pack-builder-prompt-session";
    let hook = crate::agent_teams::ServerToolPolicyHook::new(state.clone());
    let policy_context = |run_id: &str| tandem_tools::ToolDispatchPolicyContext {
        requested_tool: "functions.pack_builder".to_string(),
        canonical_tool: Some("pack_builder".to_string()),
        args: json!({"mode": "preview"}),
        tenant_context: tenant.clone(),
        verified_tenant_context: None,
        direct_loopback_http_request: false,
        source: tandem_tools::ToolDispatchSource::new("engine_loop")
            .session(session_id)
            .run(run_id),
        scope_allowlist: vec!["batch".to_string(), "pack_builder".to_string()],
        schema: None,
    };
    let denied = |decision: Option<tandem_tools::ToolDispatchDecision>| {
        assert_eq!(
            decision.expect("pack builder denial").outcome,
            tandem_tools::ToolDispatchPolicyOutcome::Denied
        );
    };
    denied(
        tandem_core::ToolPolicyHook::revalidate_dispatch(&hook, policy_context("remote-run"))
            .await
            .expect("policy"),
    );
    state
        .run_registry
        .acquire_http_prompt(
            session_id,
            "remote-run".to_string(),
            None,
            None,
            None,
            false,
        )
        .await
        .expect("remote run");
    denied(
        tandem_core::ToolPolicyHook::revalidate_dispatch(&hook, policy_context("remote-run"))
            .await
            .expect("policy"),
    );
    state
        .run_registry
        .finish_if_match(session_id, "remote-run")
        .await;
    state
        .run_registry
        .acquire_http_prompt(session_id, "local-run".to_string(), None, None, None, true)
        .await
        .expect("local run");
    denied(
        tandem_core::ToolPolicyHook::revalidate_dispatch(&hook, policy_context("remote-run"))
            .await
            .expect("policy"),
    );
    assert!(
        tandem_core::ToolPolicyHook::revalidate_dispatch(&hook, policy_context("local-run"))
            .await
            .expect("policy")
            .is_none(),
        "the exact direct-local prompt run may use Pack Builder"
    );
    state.set_http_listener_bound_loopback_only(false);
    denied(
        tandem_core::ToolPolicyHook::revalidate_dispatch(&hook, policy_context("local-run"))
            .await
            .expect("policy"),
    );
}

async fn run_pack_builder_prompt_for_peer(
    state: &AppState,
    tool: &str,
    args: Value,
    peer: Option<std::net::SocketAddr>,
    forwarded: bool,
) -> (String, String) {
    state
        .providers
        .replace_for_test(
            vec![Arc::new(PackBuilderPromptProvider {
                tool: tool.to_string(),
                args: args.clone(),
                streams: std::sync::atomic::AtomicUsize::new(0),
            })],
            Some("pack-builder-prompt-test".to_string()),
        )
        .await;
    let mut session = tandem_types::Session::new(
        Some("Pack Builder authority test".to_string()),
        Some(".".to_string()),
    );
    session.model = Some(tandem_types::ModelSpec {
        provider_id: "pack-builder-prompt-test".to_string(),
        model_id: "pack-builder-prompt-test-1".to_string(),
    });
    let session_id = session.id.clone();
    state
        .storage
        .save_session(session)
        .await
        .expect("save session");
    let mut request = Request::builder()
        .method("POST")
        .uri(format!("/session/{session_id}/prompt_sync"))
        .header("content-type", "application/json");
    if let Some(peer) = peer {
        request = request.extension(axum::extract::ConnectInfo(peer));
    }
    if forwarded {
        request = request.header("x-forwarded-for", "203.0.113.7");
    }
    let request = request
        .body(Body::from(
            json!({
                "parts": [{"type": "text", "text": format!("/tool {tool} {args}")}],
                "model": {
                    "provider_id": "pack-builder-prompt-test",
                    "model_id": "pack-builder-prompt-test-1"
                },
                "tool_mode": "auto",
                "tool_allowlist": ["batch", "pack_builder"]
            })
            .to_string(),
        ))
        .expect("prompt request");
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        app_router(state.clone()).oneshot(request),
    )
    .await
    .expect("prompt timeout")
    .expect("prompt response");
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("prompt body");
    (session_id, String::from_utf8(body.to_vec()).expect("UTF-8"))
}

#[tokio::test]
async fn pack_builder_chat_prompt_denies_remote_direct_and_nested_batch_calls() {
    let state = test_state().await;
    register_pack_builder_tool(&state).await;
    state
        .runtime
        .get()
        .expect("runtime")
        .permissions
        .add_rule("batch", "batch", tandem_core::PermissionAction::Allow)
        .await;
    let remote_peer = Some("203.0.113.7:39731".parse().expect("remote peer"));
    let preview_args = json!({
        "mode": "preview",
        "goal": "Summarize local notes daily",
        "auto_apply": true
    });
    for (tool, args, peer, forwarded) in [
        ("pack_builder", preview_args.clone(), remote_peer, false),
        (
            "batch",
            json!({"tool_calls": [{
                "tool": "functions.pack_builder",
                "args": preview_args.clone()
            }]}),
            remote_peer,
            false,
        ),
        ("pack_builder", preview_args.clone(), None, false),
        (
            "pack_builder",
            preview_args.clone(),
            Some("127.0.0.1:39731".parse().expect("loopback peer")),
            true,
        ),
    ] {
        let (session_id, body) =
            run_pack_builder_prompt_for_peer(&state, tool, args, peer, forwarded).await;
        assert!(
            body.contains("pack_builder requires the local single-user runtime"),
            "{tool} should be blocked: {body}"
        );
        let pending = Request::builder()
            .method("GET")
            .uri(format!("/pack-builder/pending?session_id={session_id}"))
            .extension(direct_pack_builder_loopback())
            .body(Body::empty())
            .expect("pending request");
        let response = app_router(state.clone())
            .oneshot(pending)
            .await
            .expect("pending response");
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("pending body");
        let payload: Value = serde_json::from_slice(&bytes).expect("pending json");
        assert!(payload.get("pending").is_none_or(Value::is_null));
    }

    let (local_session, local_body) = run_pack_builder_prompt_for_peer(
        &state,
        "pack_builder",
        json!({
            "mode": "preview",
            "goal": "Summarize local notes daily",
            "auto_apply": false
        }),
        Some("127.0.0.1:39731".parse().expect("loopback peer")),
        false,
    )
    .await;
    assert!(
        !local_body.contains("pack_builder requires the local single-user runtime"),
        "local prompt should retain Pack Builder: {local_body}"
    );
    let pending = Request::builder()
        .method("GET")
        .uri(format!("/pack-builder/pending?session_id={local_session}"))
        .extension(direct_pack_builder_loopback())
        .body(Body::empty())
        .expect("local pending request");
    let response = app_router(state)
        .oneshot(pending)
        .await
        .expect("pending response");
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("pending body");
    let payload: Value = serde_json::from_slice(&bytes).expect("pending json");
    assert!(payload.get("pending").is_some_and(Value::is_object));
}

#[tokio::test]
async fn pack_builder_preview_external_goal_prefers_mcp_and_generates_mcp_actions() {
    let state = test_state().await;
    register_pack_builder_tool(&state).await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .extension(direct_pack_builder_loopback())
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tool": "pack_builder",
                "args": {
                    "mode": "preview",
                    "goal": "create a pack that checks latest headline news and posts to slack"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");

    let metadata = payload.get("metadata").cloned().unwrap_or(Value::Null);
    let mapped = metadata
        .get("mcp_mapping")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert!(
        mapped.iter().any(|row| {
            row.as_str()
                .is_some_and(|name| name.starts_with("mcp.") && !name.trim().is_empty())
        }),
        "expected at least one MCP tool mapping for external goal"
    );

    let zip_path = metadata
        .get("zip_path")
        .and_then(|v| v.as_str())
        .expect("zip path in preview");
    let file = std::fs::File::open(zip_path).expect("open zip");
    let mut archive = zip::ZipArchive::new(file).expect("zip archive");
    let mut mission = String::new();
    std::io::Read::read_to_string(
        &mut archive.by_name("missions/default.yaml").expect("mission"),
        &mut mission,
    )
    .expect("read mission");
    assert!(
        mission.lines().any(|line| line.contains("action: mcp.")),
        "mission should explicitly invoke discovered MCP tool IDs"
    );
}

#[tokio::test]
async fn pack_builder_preview_builtin_only_path_does_not_require_connector_selection() {
    let state = test_state().await;
    register_pack_builder_tool(&state).await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .extension(direct_pack_builder_loopback())
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tool": "pack_builder",
                "args": {
                    "mode": "preview",
                    "auto_apply": false,
                    "goal": "Create a pack that checks latest headline news every day at 8 AM"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let metadata = payload.get("metadata").cloned().unwrap_or(Value::Null);
    assert_eq!(
        metadata
            .get("connector_selection_required")
            .and_then(|v| v.as_bool()),
        Some(false)
    );
    assert_eq!(
        metadata
            .get("selected_connectors")
            .and_then(|v| v.as_array())
            .map(|v| v.len()),
        Some(0)
    );
}

#[tokio::test]
async fn pack_builder_preview_auto_applies_when_safe() {
    let state = test_state().await;
    register_pack_builder_tool(&state).await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .extension(direct_pack_builder_loopback())
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tool": "pack_builder",
                "args": {
                    "mode": "preview",
                    "goal": "Create a pack that checks latest headline news every day at 8 AM"
                }
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let metadata = payload.get("metadata").cloned().unwrap_or(Value::Null);
    assert_eq!(
        metadata
            .get("auto_applied_from_preview")
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(metadata.get("mode").and_then(|v| v.as_str()), Some("apply"));
    assert!(
        metadata
            .get("pack_installed")
            .and_then(|v| v.get("pack_id"))
            .and_then(|v| v.as_str())
            .is_some(),
        "expected installed pack in auto-apply response"
    );
}

#[tokio::test]
async fn pack_builder_confirmation_goal_applies_last_session_plan() {
    let state = test_state().await;
    register_pack_builder_tool(&state).await;
    let app = app_router(state);
    let session_id = "session-confirm-flow";

    let preview_req = Request::builder()
            .method("POST")
            .uri("/tool/execute")
            .extension(direct_pack_builder_loopback())
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "tool": "pack_builder",
                    "args": {
                        "__session_id": session_id,
                        "mode": "preview",
                        "auto_apply": false,
                        "goal": "Build a daily technology digest automation that sends a summary to info@frumu.ai at 8 AM"
                    }
                })
                .to_string(),
            ))
            .expect("preview request");
    let preview_resp = app
        .clone()
        .oneshot(preview_req)
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");
    let preview_meta = preview_payload
        .get("metadata")
        .cloned()
        .unwrap_or(Value::Null);
    let expected_plan_id = preview_meta
        .get("plan_id")
        .and_then(|v| v.as_str())
        .expect("preview plan id")
        .to_string();

    let confirm_req = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .extension(direct_pack_builder_loopback())
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tool": "pack_builder",
                "args": {
                    "__session_id": session_id,
                    "mode": "preview",
                    "goal": "ok"
                }
            })
            .to_string(),
        ))
        .expect("confirm request");
    let confirm_resp = app.oneshot(confirm_req).await.expect("confirm response");
    assert_eq!(confirm_resp.status(), StatusCode::OK);
    let confirm_body = to_bytes(confirm_resp.into_body(), usize::MAX)
        .await
        .expect("confirm body");
    let confirm_payload: Value = serde_json::from_slice(&confirm_body).expect("confirm json");
    let confirm_meta = confirm_payload
        .get("metadata")
        .cloned()
        .unwrap_or(Value::Null);

    assert_eq!(
        confirm_meta.get("mode").and_then(|v| v.as_str()),
        Some("apply")
    );
    assert_eq!(
        confirm_meta.get("plan_id").and_then(|v| v.as_str()),
        Some(expected_plan_id.as_str())
    );
    let installed_pack = confirm_meta
        .get("pack_installed")
        .and_then(|v| v.get("pack_id"))
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    assert!(
        !installed_pack.ends_with("_ok"),
        "confirmation should apply previous preview plan, not create *_ok pack IDs"
    );
}

#[tokio::test]
async fn pack_builder_apply_requires_explicit_approvals() {
    let state = test_state().await;
    register_pack_builder_tool(&state).await;
    let app = app_router(state.clone());

    let preview_req = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .extension(direct_pack_builder_loopback())
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tool": "pack_builder",
                "args": {
                    "mode": "preview",
                    "goal": "create a pack for notion and slack sync"
                }
            })
            .to_string(),
        ))
        .expect("preview request");
    let preview_resp = app
        .clone()
        .oneshot(preview_req)
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");
    let plan_id = preview_payload
        .get("metadata")
        .and_then(|v| v.get("plan_id"))
        .and_then(|v| v.as_str())
        .expect("plan id");

    let apply_req = Request::builder()
        .method("POST")
        .uri("/tool/execute")
        .extension(direct_pack_builder_loopback())
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "tool": "pack_builder",
                "args": {
                    "mode": "apply",
                    "plan_id": plan_id
                }
            })
            .to_string(),
        ))
        .expect("apply request");
    let apply_resp = app.oneshot(apply_req).await.expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    let metadata = apply_payload
        .get("metadata")
        .cloned()
        .unwrap_or(Value::Null);
    assert_eq!(
        metadata.get("error").and_then(|v| v.as_str()),
        Some("approval_required")
    );
}

#[tokio::test]
async fn pack_builder_preview_apply_cancel_pending_endpoints_roundtrip() {
    let state = test_state().await;
    register_pack_builder_tool(&state).await;
    let app = app_router(state);

    let preview_req = Request::builder()
        .method("POST")
        .uri("/pack-builder/preview")
        .extension(direct_pack_builder_loopback())
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "session_id": "pb-session-1",
                "thread_key": "web:thread-a",
                "auto_apply": false,
                "goal": "Create a pack to summarize public headline news daily."
            })
            .to_string(),
        ))
        .expect("preview request");
    let preview_resp = app
        .clone()
        .oneshot(preview_req)
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");
    let plan_id = preview_payload
        .get("plan_id")
        .and_then(|v| v.as_str())
        .expect("plan_id")
        .to_string();
    assert_eq!(
        preview_payload.get("status").and_then(|v| v.as_str()),
        Some("preview_pending")
    );

    let pending_req = Request::builder()
        .method("GET")
        .uri("/pack-builder/pending?session_id=pb-session-1&thread_key=web%3Athread-a")
        .extension(direct_pack_builder_loopback())
        .body(Body::empty())
        .expect("pending request");
    let pending_resp = app
        .clone()
        .oneshot(pending_req)
        .await
        .expect("pending response");
    assert_eq!(pending_resp.status(), StatusCode::OK);
    let pending_body = to_bytes(pending_resp.into_body(), usize::MAX)
        .await
        .expect("pending body");
    let pending_payload: Value = serde_json::from_slice(&pending_body).expect("pending json");
    assert_eq!(
        pending_payload
            .get("pending")
            .and_then(|v| v.get("plan_id"))
            .and_then(|v| v.as_str()),
        Some(plan_id.as_str())
    );

    let cancel_req = Request::builder()
        .method("POST")
        .uri("/pack-builder/cancel")
        .extension(direct_pack_builder_loopback())
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "session_id": "pb-session-1",
                "thread_key": "web:thread-a",
                "plan_id": plan_id
            })
            .to_string(),
        ))
        .expect("cancel request");
    let cancel_resp = app
        .clone()
        .oneshot(cancel_req)
        .await
        .expect("cancel response");
    assert_eq!(cancel_resp.status(), StatusCode::OK);
    let cancel_body = to_bytes(cancel_resp.into_body(), usize::MAX)
        .await
        .expect("cancel body");
    let cancel_payload: Value = serde_json::from_slice(&cancel_body).expect("cancel json");
    assert_eq!(
        cancel_payload.get("status").and_then(|v| v.as_str()),
        Some("cancelled")
    );
}

#[tokio::test]
async fn pack_builder_preview_updates_context_blackboard_when_context_run_id_provided() {
    let state = test_state().await;
    register_pack_builder_tool(&state).await;
    let app = app_router(state);

    let preview_req = Request::builder()
        .method("POST")
        .uri("/pack-builder/preview")
        .extension(direct_pack_builder_loopback())
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "session_id": "pb-session-bb",
                "thread_key": "web:bb-thread",
                "context_run_id": "ctx-run-pack-builder-1",
                "auto_apply": false,
                "goal": "Create a pack to summarize public headline news daily."
            })
            .to_string(),
        ))
        .expect("preview request");
    let preview_resp = app
        .clone()
        .oneshot(preview_req)
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);

    let blackboard_req = Request::builder()
        .method("GET")
        .uri("/context/runs/ctx-run-pack-builder-1/blackboard")
        .body(Body::empty())
        .expect("blackboard request");
    let blackboard_resp = app
        .clone()
        .oneshot(blackboard_req)
        .await
        .expect("blackboard response");
    assert_eq!(blackboard_resp.status(), StatusCode::OK);
    let blackboard_body = to_bytes(blackboard_resp.into_body(), usize::MAX)
        .await
        .expect("blackboard body");
    let blackboard_payload: Value =
        serde_json::from_slice(&blackboard_body).expect("blackboard json");
    let tasks = blackboard_payload
        .get("blackboard")
        .and_then(|v| v.get("tasks"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(!tasks.is_empty());
    assert!(tasks.iter().any(|task| {
        task.get("task_type")
            .and_then(Value::as_str)
            .map(|row| row == "pack_builder.preview")
            .unwrap_or(false)
            && task
                .get("workflow_id")
                .and_then(Value::as_str)
                .map(|row| row == "pack_builder")
                .unwrap_or(false)
    }));
}

#[tokio::test]
async fn pack_builder_apply_endpoint_honors_thread_scoped_pending_plan() {
    let state = test_state().await;
    register_pack_builder_tool(&state).await;
    let app = app_router(state.clone());

    let preview_a = Request::builder()
        .method("POST")
        .uri("/pack-builder/preview")
        .extension(direct_pack_builder_loopback())
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "session_id": "pb-session-threads",
                "thread_key": "thread:a",
                "auto_apply": false,
                "goal": "Create a pack to summarize public headline news daily."
            })
            .to_string(),
        ))
        .expect("preview request");
    let preview_a_resp = app
        .clone()
        .oneshot(preview_a)
        .await
        .expect("preview response");
    assert_eq!(preview_a_resp.status(), StatusCode::OK);
    let preview_a_body = to_bytes(preview_a_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_a_payload: Value = serde_json::from_slice(&preview_a_body).expect("preview json");
    let plan_thread_a = preview_a_payload
        .get("plan_id")
        .and_then(|v| v.as_str())
        .expect("plan id")
        .to_string();

    let preview_b = Request::builder()
        .method("POST")
        .uri("/pack-builder/preview")
        .extension(direct_pack_builder_loopback())
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "session_id": "pb-session-threads",
                "thread_key": "thread:b",
                "auto_apply": false,
                "goal": "Create a pack to summarize public headline news daily."
            })
            .to_string(),
        ))
        .expect("preview request");
    let preview_b_resp = app
        .clone()
        .oneshot(preview_b)
        .await
        .expect("preview response");
    assert_eq!(preview_b_resp.status(), StatusCode::OK);

    let apply_req = Request::builder()
        .method("POST")
        .uri("/pack-builder/apply")
        .extension(direct_pack_builder_loopback())
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "session_id": "pb-session-threads",
                "thread_key": "thread:a",
                "approvals": {
                    "approve_pack_install": true,
                    "approve_connector_registration": true,
                    "approve_enable_routines": false
                },
                "secret_refs_confirmed": true
            })
            .to_string(),
        ))
        .expect("apply request");
    let apply_resp = app.oneshot(apply_req).await.expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    let automations_registered = apply_payload["automations_registered"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let routines_registered = apply_payload["routines_registered"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert_eq!(automations_registered.len(), routines_registered.len());
    if let Some(automation_id) = automations_registered.first().and_then(Value::as_str) {
        let automation = state
            .get_automation_v2(automation_id)
            .await
            .expect("stored pack builder automation");
        assert_eq!(
            automation
                .metadata
                .as_ref()
                .and_then(|v| v.get("origin"))
                .and_then(|v| v.as_str()),
            Some("pack_builder")
        );
        assert_eq!(automation.status, crate::AutomationV2Status::Paused);
        assert_eq!(
            automation
                .metadata
                .as_ref()
                .and_then(|v| v.get("activation_mode"))
                .and_then(|v| v.as_str()),
            Some("routine_wrapper_mirror")
        );
        assert_eq!(
            automation
                .metadata
                .as_ref()
                .and_then(|v| v.get("pack_builder_plan_id"))
                .and_then(|v| v.as_str()),
            Some(plan_thread_a.as_str())
        );
        assert_eq!(
            automation
                .metadata
                .as_ref()
                .and_then(|v| v.get("routine_id"))
                .and_then(|v| v.as_str()),
            routines_registered[0].as_str()
        );
    }
}

#[tokio::test]
async fn pack_builder_apply_endpoint_blocks_when_required_secrets_missing() {
    let state = test_state().await;
    register_pack_builder_tool(&state).await;
    let app = app_router(state);

    let preview_req = Request::builder()
        .method("POST")
        .uri("/pack-builder/preview")
        .extension(direct_pack_builder_loopback())
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "session_id": "pb-session-secrets",
                "thread_key": "thread:secrets",
                "auto_apply": false,
                "goal": "Create a pack that syncs Notion updates to Slack every day"
            })
            .to_string(),
        ))
        .expect("preview request");
    let preview_resp = app
        .clone()
        .oneshot(preview_req)
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");
    let required = preview_payload
        .get("required_secrets")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if required.is_empty() {
        return;
    }

    let apply_req = Request::builder()
            .method("POST")
            .uri("/pack-builder/apply")
            .extension(direct_pack_builder_loopback())
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "session_id": "pb-session-secrets",
                    "thread_key": "thread:secrets",
                    "plan_id": preview_payload.get("plan_id").and_then(|v| v.as_str()).unwrap_or_default(),
                    "approvals": {
                        "approve_pack_install": true,
                        "approve_connector_registration": true,
                        "approve_enable_routines": true
                    },
                    "secret_refs_confirmed": false
                })
                .to_string(),
            ))
            .expect("apply request");
    let apply_resp = app.oneshot(apply_req).await.expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    assert_eq!(
        apply_payload.get("status").and_then(|v| v.as_str()),
        Some("apply_blocked_missing_secrets")
    );
}
