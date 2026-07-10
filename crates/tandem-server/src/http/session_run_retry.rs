//! Tenant-scoped provider authentication and dispatch-boundary OAuth recovery.
//!
//! Recovery is installed as a task-local callback in `ProviderRegistry`. A
//! typed 401/403 from the Codex provider may refresh the tenant credential and
//! replay that one provider request. The surrounding engine prompt is never
//! replayed, so tool calls and other side effects completed earlier in a run
//! remain at-most-once.

use serde_json::json;
use tandem_data_boundary::SensitiveDataClass;
use tandem_providers::ProviderAuthRecovery;
use tandem_types::{SendMessageRequest, TenantContext};

use super::sessions::publish_tenant_event;
use crate::http::AppState;

const OPENAI_CODEX_PROVIDER_ID: &str = "openai-codex";
const DISPATCH_REFRESH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PromptExecutionSurface {
    Session,
    Channel,
    Workflow,
    Routine,
    Scheduled,
    Automation,
    KnowledgeBase,
    MissionBuilder,
    Planner,
}

impl PromptExecutionSurface {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Channel => "channel",
            Self::Workflow => "workflow",
            Self::Routine => "routine",
            Self::Scheduled => "scheduled",
            Self::Automation => "automation",
            Self::KnowledgeBase => "knowledge_base",
            Self::MissionBuilder => "mission_builder",
            Self::Planner => "planner",
        }
    }

    fn data_boundary_classes(self) -> Vec<SensitiveDataClass> {
        let mut classes = vec![
            SensitiveDataClass::CustomerData,
            SensitiveDataClass::SourceCode,
        ];
        match self {
            Self::Workflow
            | Self::Routine
            | Self::Scheduled
            | Self::Automation
            | Self::MissionBuilder
            | Self::Planner => {
                classes.push(SensitiveDataClass::ProprietaryBusinessData);
            }
            Self::KnowledgeBase => {
                classes.push(SensitiveDataClass::Legal);
                classes.push(SensitiveDataClass::ProprietaryBusinessData);
            }
            Self::Session | Self::Channel => {}
        }
        classes
    }
}

fn tenant_codex_oauth_credential(
    state: &AppState,
    tenant_context: &TenantContext,
) -> Option<tandem_core::OAuthProviderCredential> {
    let security_dir = crate::http::config_providers::provider_auth_security_dir_for_state(state);
    tandem_core::load_provider_oauth_credential_for_tenant_in_dir(
        &security_dir,
        tenant_context,
        OPENAI_CODEX_PROVIDER_ID,
    )
}

fn tenant_has_refreshable_codex_oauth(state: &AppState, tenant_context: &TenantContext) -> bool {
    tenant_codex_oauth_credential(state, tenant_context)
        .as_ref()
        .is_some_and(crate::http::config_providers::openai_codex_oauth_refreshable_in_process)
}

fn recovery_for_execution(
    state: &AppState,
    tenant_context: &TenantContext,
    surface: PromptExecutionSurface,
    session_id: Option<&str>,
    run_id: Option<&str>,
) -> ProviderAuthRecovery {
    let state = state.clone();
    let tenant_context = tenant_context.clone();
    let session_id = session_id.map(str::to_string);
    let run_id = run_id.map(str::to_string);
    ProviderAuthRecovery::new(move |provider_id| {
        let state = state.clone();
        let tenant_context = tenant_context.clone();
        let session_id = session_id.clone();
        let run_id = run_id.clone();
        async move {
            if !provider_id.eq_ignore_ascii_case(OPENAI_CODEX_PROVIDER_ID)
                || !tenant_has_refreshable_codex_oauth(&state, &tenant_context)
            {
                return Ok(false);
            }

            tracing::info!(
                provider_id = OPENAI_CODEX_PROVIDER_ID,
                session_id = session_id.as_deref().unwrap_or(""),
                run_id = run_id.as_deref().unwrap_or(""),
                surface = surface.as_str(),
                org_id = %tenant_context.org_id,
                workspace_id = %tenant_context.workspace_id,
                deployment_id = tenant_context.deployment_id.as_deref().unwrap_or(""),
                "provider dispatch rejected Codex OAuth; refreshing and retrying the dispatch once"
            );
            publish_tenant_event(
                &state,
                &tenant_context,
                "session.auth.refresh_retry",
                json!({
                    "sessionID": session_id,
                    "runID": run_id,
                    "surface": surface.as_str(),
                    "providerID": OPENAI_CODEX_PROVIDER_ID,
                    "retryBoundary": "provider_dispatch",
                }),
            );

            match tokio::time::timeout(
                DISPATCH_REFRESH_TIMEOUT,
                crate::http::config_providers::refresh_openai_codex_oauth_now(
                    &state,
                    &tenant_context,
                ),
            )
            .await
            {
                Ok(Ok(())) => Ok(true),
                Ok(Err(error)) => {
                    tracing::warn!(
                        provider_id = OPENAI_CODEX_PROVIDER_ID,
                        session_id = session_id.as_deref().unwrap_or(""),
                        run_id = run_id.as_deref().unwrap_or(""),
                        surface = surface.as_str(),
                        org_id = %tenant_context.org_id,
                        workspace_id = %tenant_context.workspace_id,
                        failure_code = crate::http::config_providers::openai_codex_oauth_refresh_failure_code(&error),
                        "Codex OAuth refresh before provider dispatch retry failed"
                    );
                    Ok(false)
                }
                Err(_) => {
                    publish_tenant_event(
                        &state,
                        &tenant_context,
                        "provider.oauth.refresh.failed",
                        json!({
                            "providerID": OPENAI_CODEX_PROVIDER_ID,
                            "refreshMode": "dispatch_retry",
                            "failureCode": "refresh_timeout",
                            "sessionID": session_id,
                            "runID": run_id,
                            "surface": surface.as_str(),
                            "occurredAtMs": crate::now_ms(),
                        }),
                    );
                    tracing::warn!(
                        provider_id = OPENAI_CODEX_PROVIDER_ID,
                        session_id = session_id.as_deref().unwrap_or(""),
                        run_id = run_id.as_deref().unwrap_or(""),
                        surface = surface.as_str(),
                        org_id = %tenant_context.org_id,
                        workspace_id = %tenant_context.workspace_id,
                        failure_code = "refresh_timeout",
                        "Codex OAuth refresh before provider dispatch retry timed out"
                    );
                    Ok(false)
                }
            }
        }
    })
}

/// Scope a direct or engine-owned provider execution to one tenant. Explicit
/// hosted tenants fail closed in the registry when no tenant bearer is loaded.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn scope_provider_auth_for_tenant<F>(
    state: &AppState,
    tenant_context: &TenantContext,
    surface: PromptExecutionSurface,
    session_id: Option<&str>,
    run_id: Option<&str>,
    provider_id_hint: Option<&str>,
    future: F,
) -> F::Output
where
    F: std::future::Future,
{
    let resolved_provider_is_codex = match provider_id_hint {
        Some(provider_id) => provider_id.eq_ignore_ascii_case(OPENAI_CODEX_PROVIDER_ID),
        None => state
            .providers
            .resolve_provider_route(None, None)
            .await
            .is_ok_and(|route| {
                route
                    .provider_id
                    .eq_ignore_ascii_case(OPENAI_CODEX_PROVIDER_ID)
            }),
    };
    if resolved_provider_is_codex || tenant_codex_oauth_credential(state, tenant_context).is_some()
    {
        if let Err(error) = crate::http::config_providers::load_openai_codex_oauth_into_runtime(
            state,
            tenant_context,
        )
        .await
        {
            tracing::warn!(
                provider_id = OPENAI_CODEX_PROVIDER_ID,
                session_id = session_id.unwrap_or(""),
                run_id = run_id.unwrap_or(""),
                surface = surface.as_str(),
                org_id = %tenant_context.org_id,
                workspace_id = %tenant_context.workspace_id,
                failure_code = crate::http::config_providers::openai_codex_oauth_refresh_failure_code(&error),
                "failed to load tenant Codex OAuth credential into provider runtime"
            );
        }
    }

    let recovery = recovery_for_execution(state, tenant_context, surface, session_id, run_id);
    state
        .providers
        .scope_tenant_provider_auth_with_recovery(tenant_context.clone(), recovery, future)
        .await
}

/// Run one engine prompt under tenant provider authentication. Any eligible
/// OAuth replay occurs inside `ProviderRegistry` around the failed provider
/// request, never around this engine future.
pub(crate) async fn run_prompt_with_auth_recovery(
    state: &AppState,
    session_id: &str,
    run_id: &str,
    surface: PromptExecutionSurface,
    req: SendMessageRequest,
    correlation_id: Option<String>,
    tenant_context: &TenantContext,
) -> anyhow::Result<()> {
    let session_model = state
        .storage
        .get_session(session_id)
        .await
        .and_then(|session| session.model);
    let provider_id_hint = req
        .model
        .as_ref()
        .map(|model| model.provider_id.trim().to_string())
        .filter(|provider_id| !provider_id.is_empty())
        .or_else(|| {
            session_model
                .as_ref()
                .map(|model| model.provider_id.trim().to_string())
                .filter(|provider_id| !provider_id.is_empty())
        });
    let engine_run = state.engine_loop.run_prompt_async_with_execution_context(
        session_id.to_string(),
        req,
        correlation_id,
        Some(run_id.to_string()),
        surface.data_boundary_classes(),
    );
    scope_provider_auth_for_tenant(
        state,
        tenant_context,
        surface,
        Some(session_id),
        Some(run_id),
        provider_id_hint.as_deref(),
        engine_run,
    )
    .await
}

#[cfg(test)]
pub(crate) mod provider_auth_test_support {
    use super::*;
    use async_trait::async_trait;
    use futures::{stream, Stream};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use tandem_providers::{ChatMessage, Provider, ProviderAuthOverride, StreamChunk};
    use tandem_types::{ModelInfo, ProviderInfo, SamplingParams, ToolMode, ToolSchema};
    use tokio_util::sync::CancellationToken;

    #[derive(Clone)]
    struct CapturingCodexProvider {
        response: String,
        auth: Arc<Mutex<Vec<ProviderAuthOverride>>>,
    }

    impl CapturingCodexProvider {
        fn stream_response(
            &self,
            auth_override: ProviderAuthOverride,
        ) -> Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>> {
            self.auth
                .lock()
                .expect("provider auth capture")
                .push(auth_override);
            Box::pin(stream::iter([
                Ok(StreamChunk::TextDelta(self.response.clone())),
                Ok(StreamChunk::Done {
                    finish_reason: "stop".to_string(),
                    usage: None,
                }),
            ]))
        }
    }

    #[async_trait]
    impl Provider for CapturingCodexProvider {
        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                id: OPENAI_CODEX_PROVIDER_ID.to_string(),
                name: "capturing Codex provider".to_string(),
                models: vec![ModelInfo {
                    id: "codex-test".to_string(),
                    provider_id: OPENAI_CODEX_PROVIDER_ID.to_string(),
                    display_name: "Codex Test".to_string(),
                    context_window: 8_192,
                }],
            }
        }

        async fn complete(
            &self,
            _prompt: &str,
            _model_override: Option<&str>,
        ) -> anyhow::Result<String> {
            self.auth
                .lock()
                .expect("provider auth capture")
                .push(ProviderAuthOverride::Inherit);
            Ok(self.response.clone())
        }

        async fn complete_with_auth_override(
            &self,
            _prompt: &str,
            _model_override: Option<&str>,
            auth_override: ProviderAuthOverride,
        ) -> anyhow::Result<String> {
            self.auth
                .lock()
                .expect("provider auth capture")
                .push(auth_override);
            Ok(self.response.clone())
        }

        async fn stream(
            &self,
            _messages: Vec<ChatMessage>,
            _model_override: Option<&str>,
            _tool_mode: ToolMode,
            _tools: Option<Vec<ToolSchema>>,
            _sampling: SamplingParams,
            _cancel: CancellationToken,
        ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>>
        {
            Ok(self.stream_response(ProviderAuthOverride::Inherit))
        }

        async fn stream_with_auth_override(
            &self,
            _messages: Vec<ChatMessage>,
            _model_override: Option<&str>,
            _tool_mode: ToolMode,
            _tools: Option<Vec<ToolSchema>>,
            _sampling: SamplingParams,
            _cancel: CancellationToken,
            auth_override: ProviderAuthOverride,
        ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>>
        {
            Ok(self.stream_response(auth_override))
        }
    }

    pub(crate) async fn install_capturing_codex_provider(
        state: &AppState,
        response: impl Into<String>,
        credentials: &[(&TenantContext, &str)],
    ) -> Arc<Mutex<Vec<ProviderAuthOverride>>> {
        let auth = Arc::new(Mutex::new(Vec::new()));
        state
            .providers
            .replace_for_test(
                vec![Arc::new(CapturingCodexProvider {
                    response: response.into(),
                    auth: auth.clone(),
                })],
                Some(OPENAI_CODEX_PROVIDER_ID.to_string()),
            )
            .await;
        state
            .providers
            .set_tenant_provider_bearer_token(
                &TenantContext::local_implicit(),
                OPENAI_CODEX_PROVIDER_ID,
                "local-token-that-hosted-must-not-inherit".to_string(),
            )
            .await;

        let security_dir =
            crate::http::config_providers::provider_auth_security_dir_for_state(state);
        for (tenant_context, token) in credentials {
            tandem_core::set_provider_oauth_credential_for_tenant_in_dir(
                &security_dir,
                tenant_context,
                OPENAI_CODEX_PROVIDER_ID,
                tandem_core::OAuthProviderCredential {
                    provider_id: OPENAI_CODEX_PROVIDER_ID.to_string(),
                    access_token: (*token).to_string(),
                    refresh_token: format!("refresh-{token}"),
                    expires_at_ms: crate::now_ms().saturating_add(60_000),
                    account_id: None,
                    email: None,
                    display_name: None,
                    managed_by: "tandem".to_string(),
                    api_key: None,
                },
            )
            .expect("persist hosted Codex test credential");
        }
        auth
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use futures::{stream, Stream};
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tandem_providers::{
        ChatMessage, Provider, ProviderAuthOverride, ProviderAuthenticationError, StreamChunk,
    };
    use tandem_types::{
        MessagePartInput, ModelInfo, ModelSpec, ProviderInfo, SamplingParams, Session, ToolMode,
        ToolSchema,
    };
    use tokio_util::sync::CancellationToken;

    #[test]
    fn execution_surfaces_have_stable_observability_labels() {
        let labels = [
            (PromptExecutionSurface::Session, "session"),
            (PromptExecutionSurface::Channel, "channel"),
            (PromptExecutionSurface::Workflow, "workflow"),
            (PromptExecutionSurface::Routine, "routine"),
            (PromptExecutionSurface::Scheduled, "scheduled"),
            (PromptExecutionSurface::Automation, "automation"),
            (PromptExecutionSurface::KnowledgeBase, "knowledge_base"),
            (PromptExecutionSurface::MissionBuilder, "mission_builder"),
            (PromptExecutionSurface::Planner, "planner"),
        ];
        for (surface, expected) in labels {
            assert_eq!(surface.as_str(), expected);
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn every_engine_execution_surface_dispatches_inside_hosted_tenant_auth_scope() {
        use super::provider_auth_test_support::install_capturing_codex_provider;

        let state = crate::test_support::test_state().await;
        let hosted = TenantContext::explicit("org-hosted", "workspace-hosted", None);
        let auth = install_capturing_codex_provider(
            &state,
            "surface completed",
            &[(&hosted, "hosted-token")],
        )
        .await;

        let surfaces = [
            PromptExecutionSurface::Session,
            PromptExecutionSurface::Channel,
            PromptExecutionSurface::Workflow,
            PromptExecutionSurface::Routine,
            PromptExecutionSurface::Scheduled,
            PromptExecutionSurface::Automation,
        ];
        for (index, surface) in surfaces.into_iter().enumerate() {
            let mut session = Session::new(
                Some(format!("{} auth scope", surface.as_str())),
                Some(".".to_string()),
            );
            session.tenant_context = hosted.clone();
            session.source_kind =
                (surface == PromptExecutionSurface::Channel).then(|| "channel".to_string());
            session.model = Some(ModelSpec {
                provider_id: OPENAI_CODEX_PROVIDER_ID.to_string(),
                model_id: "codex-test".to_string(),
            });
            let session_id = session.id.clone();
            state
                .storage
                .save_session(session)
                .await
                .expect("save surface session");
            run_prompt_with_auth_recovery(
                &state,
                &session_id,
                &format!("surface-run-{index}"),
                surface,
                SendMessageRequest {
                    parts: vec![MessagePartInput::Text {
                        text: format!("execute {} surface", surface.as_str()),
                    }],
                    model: None,
                    agent: None,
                    tool_mode: None,
                    tool_allowlist: None,
                    strict_kb_grounding: None,
                    context_mode: None,
                    write_required: None,
                    prewrite_requirements: None,
                    sampling: SamplingParams::default(),
                },
                Some(format!("surface:{}", surface.as_str())),
                &hosted,
            )
            .await
            .expect("surface engine dispatch");
        }

        let captured = auth.lock().expect("auth capture");
        assert_eq!(captured.len(), surfaces.len());
        assert!(captured.iter().all(
            |auth| matches!(auth, ProviderAuthOverride::Bearer(token) if token == "hosted-token")
        ));
    }

    #[derive(Clone)]
    struct ToolThenAuthProvider {
        dispatches: Arc<AtomicUsize>,
        seen_auth: Arc<Mutex<Vec<ProviderAuthOverride>>>,
    }

    impl ToolThenAuthProvider {
        fn dispatch(
            &self,
            auth_override: ProviderAuthOverride,
        ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>>
        {
            self.seen_auth
                .lock()
                .expect("auth capture")
                .push(auth_override);
            let dispatch = self.dispatches.fetch_add(1, Ordering::SeqCst);
            if dispatch == 1 {
                return Err(ProviderAuthenticationError::new(
                    401,
                    "provider request failed with status 401",
                )
                .into());
            }
            let chunks = if dispatch == 0 {
                vec![
                    Ok(StreamChunk::ToolCallStart {
                        id: "call_once".to_string(),
                        name: "todo_write".to_string(),
                    }),
                    Ok(StreamChunk::ToolCallDelta {
                        id: "call_once".to_string(),
                        args_delta: serde_json::json!({
                            "todos": [{"content": "execute exactly once"}]
                        })
                        .to_string(),
                    }),
                    Ok(StreamChunk::ToolCallEnd {
                        id: "call_once".to_string(),
                    }),
                    Ok(StreamChunk::Done {
                        finish_reason: "tool_calls".to_string(),
                        usage: None,
                    }),
                ]
            } else {
                vec![
                    Ok(StreamChunk::TextDelta("done".to_string())),
                    Ok(StreamChunk::Done {
                        finish_reason: "stop".to_string(),
                        usage: None,
                    }),
                ]
            };
            Ok(Box::pin(stream::iter(chunks)))
        }
    }

    #[async_trait]
    impl Provider for ToolThenAuthProvider {
        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                id: OPENAI_CODEX_PROVIDER_ID.to_string(),
                name: "tool then auth".to_string(),
                models: vec![ModelInfo {
                    id: "codex-test".to_string(),
                    provider_id: OPENAI_CODEX_PROVIDER_ID.to_string(),
                    display_name: "Codex Test".to_string(),
                    context_window: 8_192,
                }],
            }
        }

        async fn complete(
            &self,
            _prompt: &str,
            _model_override: Option<&str>,
        ) -> anyhow::Result<String> {
            anyhow::bail!("completion is not used")
        }

        async fn stream(
            &self,
            _messages: Vec<ChatMessage>,
            _model_override: Option<&str>,
            _tool_mode: ToolMode,
            _tools: Option<Vec<ToolSchema>>,
            _sampling: SamplingParams,
            _cancel: CancellationToken,
        ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>>
        {
            self.dispatch(ProviderAuthOverride::Inherit)
        }

        async fn stream_with_auth_override(
            &self,
            _messages: Vec<ChatMessage>,
            _model_override: Option<&str>,
            _tool_mode: ToolMode,
            _tools: Option<Vec<ToolSchema>>,
            _sampling: SamplingParams,
            _cancel: CancellationToken,
            auth_override: ProviderAuthOverride,
        ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>>
        {
            self.dispatch(auth_override)
        }
    }

    #[tokio::test]
    async fn auth_failure_after_tool_side_effect_retries_dispatch_without_replaying_tool() {
        let state = crate::test_support::test_state().await;
        let hosted = TenantContext::explicit("org-no-replay", "workspace-no-replay", None);
        let dispatches = Arc::new(AtomicUsize::new(0));
        let seen_auth = Arc::new(Mutex::new(Vec::new()));
        state
            .providers
            .replace_for_test(
                vec![Arc::new(ToolThenAuthProvider {
                    dispatches: dispatches.clone(),
                    seen_auth: seen_auth.clone(),
                })],
                Some(OPENAI_CODEX_PROVIDER_ID.to_string()),
            )
            .await;
        state
            .providers
            .set_tenant_provider_bearer_token(
                &hosted,
                OPENAI_CODEX_PROVIDER_ID,
                "expired-token".to_string(),
            )
            .await;

        let mut session = Session::new(Some("no replay".to_string()), Some(".".to_string()));
        session.tenant_context = hosted.clone();
        session.model = Some(ModelSpec {
            provider_id: OPENAI_CODEX_PROVIDER_ID.to_string(),
            model_id: "codex-test".to_string(),
        });
        let session_id = session.id.clone();
        state
            .storage
            .save_session(session)
            .await
            .expect("save session");
        state
            .engine_loop
            .set_session_allowed_tools(&session_id, vec!["todo_write".to_string()])
            .await;
        state
            .engine_loop
            .set_session_auto_approve_permissions(&session_id, true)
            .await;

        let refreshes = Arc::new(AtomicUsize::new(0));
        let recovery = ProviderAuthRecovery::new({
            let providers = state.providers.clone();
            let hosted = hosted.clone();
            let refreshes = refreshes.clone();
            move |_| {
                let providers = providers.clone();
                let hosted = hosted.clone();
                let refreshes = refreshes.clone();
                async move {
                    refreshes.fetch_add(1, Ordering::SeqCst);
                    providers
                        .set_tenant_provider_bearer_token(
                            &hosted,
                            OPENAI_CODEX_PROVIDER_ID,
                            "fresh-token".to_string(),
                        )
                        .await;
                    Ok(true)
                }
            }
        });
        let request = SendMessageRequest {
            parts: vec![MessagePartInput::Text {
                text: "update the todo list".to_string(),
            }],
            model: None,
            agent: None,
            tool_mode: Some(ToolMode::Auto),
            tool_allowlist: Some(vec!["todo_write".to_string()]),
            strict_kb_grounding: None,
            context_mode: None,
            write_required: None,
            prewrite_requirements: None,
            sampling: SamplingParams::default(),
        };
        let mut events = state.event_bus.subscribe();
        let engine_run = state.engine_loop.run_prompt_async_with_context(
            session_id.clone(),
            request,
            Some("no-replay".to_string()),
        );
        state
            .providers
            .scope_tenant_provider_auth_with_recovery(hosted.clone(), recovery, engine_run)
            .await
            .expect("engine run");

        let todos = state.storage.get_todos(&session_id).await;
        assert_eq!(todos.len(), 1);
        assert_eq!(
            todos[0].get("content").and_then(serde_json::Value::as_str),
            Some("execute exactly once")
        );
        let successful_tool_dispatches = std::iter::from_fn(|| events.try_recv().ok())
            .filter(|event| {
                event.event_type == "tool.dispatch.recorded"
                    && event
                        .properties
                        .get("tool")
                        .and_then(serde_json::Value::as_str)
                        == Some("todo_write")
                    && event
                        .properties
                        .get("status")
                        .and_then(serde_json::Value::as_str)
                        == Some("succeeded")
                    && event
                        .properties
                        .pointer("/source/session_id")
                        .and_then(serde_json::Value::as_str)
                        == Some(session_id.as_str())
            })
            .count();
        assert_eq!(successful_tool_dispatches, 1);

        let persisted = state
            .storage
            .get_session(&session_id)
            .await
            .expect("persisted session");
        assert_eq!(persisted.tenant_context, hosted);
        assert_eq!(dispatches.load(Ordering::SeqCst), 3);
        assert_eq!(refreshes.load(Ordering::SeqCst), 1);
        assert!(matches!(
            seen_auth.lock().expect("auth capture").last(),
            Some(ProviderAuthOverride::Bearer(token)) if token == "fresh-token"
        ));
    }
}
