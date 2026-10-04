// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Tenant-scoped provider authentication and dispatch-boundary OAuth recovery.
//!
//! Recovery is installed as a task-local callback in `ProviderRegistry`. A
//! typed 401/403 from the Codex provider may refresh the tenant credential and
//! replay that one provider request. The surrounding engine prompt is never
//! replayed, so tool calls and other side effects completed earlier in a run
//! remain at-most-once.

use futures::{Stream, StreamExt};
use serde_json::json;
use tandem_data_boundary::SensitiveDataClass;
use tandem_providers::{ProviderAuthRecovery, ProviderDispatchAuthority};
use tandem_types::{SendMessageRequest, TenantContext, VerifiedTenantContext};
use tokio_util::sync::CancellationToken;

use super::sessions::publish_tenant_event;
use crate::http::AppState;

const OPENAI_CODEX_PROVIDER_ID: &str = "openai-codex";
const DISPATCH_REFRESH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
pub(crate) const DIRECT_PROVIDER_AUTHORITY_REVOKED: &str = "hosted_provider_authority_revoked";

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

/// Direct provider streams outlive their dispatch-time authority check. Keep
/// their hosted grant live while a response is consumed, including idle waits.
/// This is intentionally separate from engine-owned streams, which have their
/// own execution lifecycle and cancellation boundary.
pub(crate) struct DirectProviderStreamAuthority<'a> {
    state: &'a AppState,
    tenant: &'a TenantContext,
    verified: Option<&'a VerifiedTenantContext>,
    surface: PromptExecutionSurface,
    cancel: CancellationToken,
}

impl<'a> DirectProviderStreamAuthority<'a> {
    pub(crate) fn new(
        state: &'a AppState,
        tenant: &'a TenantContext,
        verified: Option<&'a VerifiedTenantContext>,
        surface: PromptExecutionSurface,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            state,
            tenant,
            verified,
            surface,
            cancel,
        }
    }

    pub(crate) fn check(&self) -> Result<(), &'static str> {
        let result = if self.surface == PromptExecutionSurface::Planner {
            super::workflow_planner_policy::require_live_planner_write(
                self.state,
                self.tenant,
                self.verified,
            )
        } else {
            self.state
                .enterprise
                .hosted_policy
                .with_current_policy(|policy| {
                    super::require_hosted_permission_under_policy(
                        self.tenant,
                        self.verified,
                        tandem_types::AccessPermission::HostedUse,
                        policy,
                    )
                    .map_err(|_| "hosted_provider_authority_revoked")
                })
                .and_then(|decision| decision)
        };
        if result.is_err() {
            self.cancel.cancel();
            return Err(DIRECT_PROVIDER_AUTHORITY_REVOKED);
        }
        Ok(())
    }

    pub(crate) async fn next_chunk<S>(
        &self,
        stream: &mut S,
    ) -> Result<Option<S::Item>, &'static str>
    where
        S: Stream + Unpin,
    {
        self.check()?;
        let next = stream.next();
        tokio::pin!(next);
        let mut ticker = tokio::time::interval_at(
            tokio::time::Instant::now() + std::time::Duration::from_secs(1),
            std::time::Duration::from_secs(1),
        );
        loop {
            tokio::select! {
                biased;
                _ = ticker.tick() => self.check()?,
                chunk = &mut next => {
                    self.check()?;
                    return Ok(chunk);
                }
            }
        }
    }

    /// The direct completion fallbacks have no stream to poll. Dropping their
    /// future on denial stops the in-flight request before accepting output.
    pub(crate) async fn guarded_future<F>(&self, future: F) -> Result<F::Output, &'static str>
    where
        F: std::future::Future,
    {
        self.check()?;
        tokio::pin!(future);
        let mut ticker = tokio::time::interval_at(
            tokio::time::Instant::now() + std::time::Duration::from_secs(1),
            std::time::Duration::from_secs(1),
        );
        loop {
            tokio::select! {
                biased;
                _ = ticker.tick() => self.check()?,
                output = &mut future => {
                    self.check()?;
                    return Ok(output);
                }
            }
        }
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
    verified_tenant_context: Option<&VerifiedTenantContext>,
    surface: PromptExecutionSurface,
    session_id: Option<&str>,
    run_id: Option<&str>,
) -> ProviderAuthRecovery {
    let state = state.clone();
    let tenant_context = tenant_context.clone();
    let verified_tenant_context = verified_tenant_context.cloned();
    let session_id = session_id.map(str::to_string);
    let run_id = run_id.map(str::to_string);
    ProviderAuthRecovery::new(move |provider_id| {
        let state = state.clone();
        let tenant_context = tenant_context.clone();
        let verified_tenant_context = verified_tenant_context.clone();
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
                crate::http::config_providers::refresh_openai_codex_oauth_now_for_request(
                    &state,
                    &tenant_context,
                    verified_tenant_context.as_ref(),
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
    verified_tenant_context: Option<&VerifiedTenantContext>,
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
        if let Err(error) =
            crate::http::config_providers::load_openai_codex_oauth_into_runtime_for_request(
                state,
                tenant_context,
                verified_tenant_context,
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

    let recovery = recovery_for_execution(
        state,
        tenant_context,
        verified_tenant_context,
        surface,
        session_id,
        run_id,
    );
    let authority_state = state.clone();
    let authority_tenant = tenant_context.clone();
    let verified = verified_tenant_context.cloned();
    let authority = ProviderDispatchAuthority::new(move || {
        let state = authority_state.clone();
        let tenant = authority_tenant.clone();
        let verified = verified.clone();
        async move {
            if surface == PromptExecutionSurface::Planner {
                super::workflow_planner_policy::require_live_planner_write(
                    &state,
                    &tenant,
                    verified.as_ref(),
                )
                .map_err(anyhow::Error::msg)
            } else {
                state
                    .enterprise
                    .hosted_policy
                    .authorize_execution(verified.as_ref())
                    .map_err(anyhow::Error::msg)
            }
        }
    });
    let allow_private_provider_endpoints =
        crate::http::host_authority::standalone_local_runtime_posture(state, tenant_context);
    state
        .providers
        .scope_tenant_provider_auth_with_recovery(
            tenant_context.clone(),
            recovery,
            allow_private_provider_endpoints,
            authority.scope(future),
        )
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
    let session = state.storage.get_session(session_id).await;
    let session_model = session.as_ref().and_then(|session| session.model.as_ref());
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
        session
            .as_ref()
            .and_then(|session| session.verified_tenant_context.as_ref()),
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

    async fn hosted_direct_stream_fixture() -> (
        AppState,
        tempfile::TempDir,
        TenantContext,
        VerifiedTenantContext,
    ) {
        use tandem_types::{
            AuthorityChain, HumanActor, RequestPrincipal, TenantContextAssertionClaims,
        };

        let state = crate::test_support::test_state().await;
        let directory = tempfile::tempdir().expect("hosted policy directory");
        let path = directory.path().join("policy.json");
        write_direct_stream_policy(&path, 1, "admin");
        state.enterprise.hosted_policy.configure_test_source(
            "direct-stream-org",
            "direct-stream-deployment",
            path,
        );
        state
            .reload_hosted_policy()
            .await
            .expect("load hosted policy");

        let tenant = TenantContext::explicit_user_workspace(
            "direct-stream-org",
            "direct-stream-deployment",
            Some("direct-stream-deployment".to_string()),
            "admin",
        );
        let now = crate::now_ms();
        let mut claims = TenantContextAssertionClaims::new_v1(
            "tandem-web",
            "tandem-runtime",
            now,
            now + 60_000,
            uuid::Uuid::new_v4().to_string(),
            tenant.clone(),
            HumanActor::tandem_user("admin"),
            AuthorityChain::from_request(RequestPrincipal::authenticated_user(
                "admin",
                "tandem-web",
            )),
            vec!["hosted:role:admin".to_string()],
        );
        claims.policy_version = Some(1);
        claims.capabilities = tandem_enterprise_contract::hosted_policy::role_capabilities("admin")
            .into_iter()
            .map(str::to_string)
            .collect();
        let mut verified: VerifiedTenantContext = claims.into();
        state
            .enterprise
            .hosted_policy
            .project(&mut verified)
            .expect("project current admin");
        (state, directory, tenant, verified)
    }

    fn write_direct_stream_policy(path: &std::path::Path, version: u64, role: &str) {
        std::fs::write(
            path,
            serde_json::to_vec(&json!({
                "schema_version": 1,
                "policy_version": version,
                "organization_id": "direct-stream-org",
                "deployment_id": "direct-stream-deployment",
                "generated_at": chrono::DateTime::from_timestamp_millis(crate::now_ms() as i64).unwrap(),
                "users": [{
                    "id": "admin", "email": null, "username": null, "role": role,
                    "capabilities": tandem_enterprise_contract::hosted_policy::role_capabilities(role),
                    "is_active": true, "email_verified": true
                }],
                "org_units": [],
                "org_unit_memberships": [],
                "deployment_grants": []
            }))
            .expect("policy JSON"),
        )
        .expect("write hosted policy");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .expect("private hosted policy");
        }
    }

    #[tokio::test]
    async fn direct_provider_stream_rejects_chunk_after_hosted_revocation() {
        for surface in [
            PromptExecutionSurface::MissionBuilder,
            PromptExecutionSurface::Planner,
            PromptExecutionSurface::KnowledgeBase,
        ] {
            let (state, directory, tenant, verified) = hosted_direct_stream_fixture().await;
            let cancel = CancellationToken::new();
            let observed_cancel = cancel.clone();
            let reload_state = state.clone();
            let (polled_tx, polled_rx) = tokio::sync::oneshot::channel();
            let (release_tx, release_rx) = tokio::sync::oneshot::channel();
            let run = tokio::spawn(async move {
                let authority = DirectProviderStreamAuthority::new(
                    &state,
                    &tenant,
                    Some(&verified),
                    surface,
                    cancel,
                );
                let stream = stream::once(async move {
                    polled_tx.send(()).expect("stream polled");
                    release_rx.await.expect("release held stream");
                    Ok::<_, anyhow::Error>(StreamChunk::TextDelta("secret".to_string()))
                });
                tokio::pin!(stream);
                authority.next_chunk(&mut stream).await
            });
            polled_rx.await.expect("provider stream reached held chunk");
            write_direct_stream_policy(&directory.path().join("policy.json"), 2, "viewer");
            reload_state
                .reload_hosted_policy()
                .await
                .expect("publish revoked policy");
            release_tx.send(()).expect("release provider chunk");
            let result = run.await.expect("stream task");
            assert!(
                result.is_err(),
                "revoked {surface:?} chunk must be rejected"
            );
            assert!(
                observed_cancel.is_cancelled(),
                "revoked stream must be cancelled"
            );
        }
    }

    #[tokio::test]
    async fn idle_direct_provider_stream_cancels_within_one_tick_after_revocation() {
        let (state, directory, tenant, verified) = hosted_direct_stream_fixture().await;
        let cancel = CancellationToken::new();
        let observed_cancel = cancel.clone();
        let reload_state = state.clone();
        let (polled_tx, polled_rx) = tokio::sync::oneshot::channel();
        let run = tokio::spawn(async move {
            let authority = DirectProviderStreamAuthority::new(
                &state,
                &tenant,
                Some(&verified),
                PromptExecutionSurface::KnowledgeBase,
                cancel,
            );
            let mut polled_tx = Some(polled_tx);
            let stream = stream::poll_fn(move |_| {
                if let Some(tx) = polled_tx.take() {
                    tx.send(()).expect("idle stream polled");
                }
                std::task::Poll::<Option<anyhow::Result<StreamChunk>>>::Pending
            });
            tokio::pin!(stream);
            authority.next_chunk(&mut stream).await
        });
        polled_rx.await.expect("provider stream is idle");
        write_direct_stream_policy(&directory.path().join("policy.json"), 2, "viewer");
        reload_state
            .reload_hosted_policy()
            .await
            .expect("publish revoked policy");
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), run)
            .await
            .expect("idle stream revocation deadline")
            .expect("idle stream task");
        assert!(result.is_err());
        assert!(observed_cancel.is_cancelled());
    }

    #[tokio::test]
    async fn direct_provider_completion_fallback_stops_on_revocation() {
        let (state, directory, tenant, verified) = hosted_direct_stream_fixture().await;
        let cancel = CancellationToken::new();
        let observed_cancel = cancel.clone();
        let reload_state = state.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let run = tokio::spawn(async move {
            let authority = DirectProviderStreamAuthority::new(
                &state,
                &tenant,
                Some(&verified),
                PromptExecutionSurface::KnowledgeBase,
                cancel,
            );
            authority
                .guarded_future(async move {
                    started_tx.send(()).expect("completion started");
                    std::future::pending::<()>().await;
                })
                .await
        });
        started_rx.await.expect("completion fallback is waiting");
        write_direct_stream_policy(&directory.path().join("policy.json"), 2, "viewer");
        reload_state
            .reload_hosted_policy()
            .await
            .expect("publish revoked policy");
        let result = tokio::time::timeout(std::time::Duration::from_secs(2), run)
            .await
            .expect("completion revocation deadline")
            .expect("completion task");
        assert_eq!(result, Err(DIRECT_PROVIDER_AUTHORITY_REVOKED));
        assert!(observed_cancel.is_cancelled());
    }

    #[tokio::test]
    async fn direct_provider_stream_keeps_local_unversioned_execution() {
        let state = crate::test_support::test_state().await;
        let tenant = TenantContext::local_implicit();
        let cancel = CancellationToken::new();
        let authority = DirectProviderStreamAuthority::new(
            &state,
            &tenant,
            None,
            PromptExecutionSurface::KnowledgeBase,
            cancel.clone(),
        );
        let stream = stream::iter([Ok::<_, anyhow::Error>(StreamChunk::TextDelta(
            "local response".to_string(),
        ))]);
        tokio::pin!(stream);
        let chunk = authority
            .next_chunk(&mut stream)
            .await
            .expect("local stream allowed")
            .expect("local chunk");
        assert!(matches!(chunk, Ok(StreamChunk::TextDelta(text)) if text == "local response"));
        assert_eq!(authority.guarded_future(async { 42 }).await, Ok(42));
        assert!(!cancel.is_cancelled());
    }

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
            .scope_tenant_provider_auth_with_recovery(hosted.clone(), recovery, false, engine_run)
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
                        .get("receipt_phase")
                        .and_then(serde_json::Value::as_str)
                        == Some("execution_completed")
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
