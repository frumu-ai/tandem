// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

// This file is included only by the HTTP test tree. Keep its synthetic
// provider and direct test dispatches inside an explicit test module so the
// production provider-egress scanner can identify that boundary.
#[cfg(test)]
mod tests {
    use super::super::*;
    use axum::{extract::Extension, http::StatusCode, Router};
    use futures::{stream, Stream};
    use serde_json::{json, Value};
    use std::{
        path::{Path, PathBuf},
        pin::Pin,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };
    use tandem_plan_compiler::api::{PlannerLlmInvocation, PlannerLlmInvoker, PlannerSessionStore};
    use tandem_providers::{ChatMessage, Provider, ProviderAuthOverride, StreamChunk};
    use tandem_types::{
        AccessPermission, ModelInfo, ModelSpec, ProviderInfo, SamplingParams, TenantContext,
        ToolMode, ToolSchema,
    };
    use tokio_util::sync::CancellationToken;

    use super::super::legacy_routine_authority::{hosted_state, tenant, verified};

    fn hosted_planner_app(state: &AppState, actor: &str, policy_version: u64) -> Router {
        let mut identity = verified(actor, "member");
        identity.policy_version = Some(policy_version);
        state
            .enterprise
            .hosted_policy
            .project(&mut identity)
            .expect("project current hosted member");
        crate::http::routes_workflow_planner::apply(Router::new())
            .layer(Extension(tenant(actor)))
            .layer(Extension(identity))
            .with_state(state.clone())
    }

    async fn revoke_member_write_at_path(state: &AppState, path: &Path) {
        let mut policy: Value =
            serde_json::from_slice(&std::fs::read(path).expect("read hosted policy"))
                .expect("policy JSON");
        policy["policy_version"] = json!(2);
        policy["generated_at"] = json!(chrono::DateTime::from_timestamp_millis(
            crate::now_ms() as i64
        )
        .expect("current time"));
        policy["deployment_grants"] = json!([]);
        std::fs::write(path, serde_json::to_vec(&policy).expect("policy bytes"))
            .expect("write revoked policy");
        state
            .reload_hosted_policy()
            .await
            .expect("publish revoked policy");
    }

    async fn revoke_member_write(state: &AppState, policy_dir: &tempfile::TempDir) {
        revoke_member_write_at_path(state, &policy_dir.path().join("policy.json")).await;
    }

    struct RevokingPlannerProvider {
        state: AppState,
        policy_path: PathBuf,
        dispatches: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl Provider for RevokingPlannerProvider {
        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                id: "planner-revocation-test".to_string(),
                name: "planner revocation test".to_string(),
                models: vec![ModelInfo {
                    id: "planner-revocation-model".to_string(),
                    provider_id: "planner-revocation-test".to_string(),
                    display_name: "Planner Revocation Model".to_string(),
                    context_window: 8_192,
                }],
            }
        }

        async fn complete(
            &self,
            _prompt: &str,
            _model_override: Option<&str>,
        ) -> anyhow::Result<String> {
            anyhow::bail!("completion fallback is not expected")
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
            let dispatch = self.dispatches.fetch_add(1, Ordering::SeqCst);
            if dispatch == 0 {
                // The first provider response is non-JSON. Revoke write before the
                // planner's JSON-only repair dispatch reaches the registry.
                revoke_member_write_at_path(&self.state, &self.policy_path).await;
            }
            Ok(Box::pin(stream::iter([
                Ok(StreamChunk::TextDelta("not JSON".to_string())),
                Ok(StreamChunk::Done {
                    finish_reason: "stop".to_string(),
                    usage: None,
                }),
            ])))
        }

        async fn stream_with_auth_override(
            &self,
            messages: Vec<ChatMessage>,
            model_override: Option<&str>,
            tool_mode: ToolMode,
            tools: Option<Vec<ToolSchema>>,
            sampling: SamplingParams,
            cancel: CancellationToken,
            _auth_override: ProviderAuthOverride,
        ) -> anyhow::Result<Pin<Box<dyn Stream<Item = anyhow::Result<StreamChunk>> + Send>>>
        {
            self.stream(messages, model_override, tool_mode, tools, sampling, cancel)
                .await
        }
    }

    fn planner_request(path: &str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "prompt": "Plan a short research workflow",
                    "operator_preferences": {
                        "role_models": {
                            "planner": {
                                "provider_id": "openai-codex",
                                "model_id": "codex-test"
                            }
                        }
                    }
                })
                .to_string(),
            ))
            .expect("planner request")
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn hosted_use_only_member_cannot_preview_or_chat_start_before_side_effects() {
        use crate::http::session_run_retry::provider_auth_test_support::install_capturing_codex_provider;

        let (state, policy_dir) = hosted_state().await;
        revoke_member_write(&state, &policy_dir).await;
        let mut identity = verified("alice", "member");
        identity.policy_version = Some(2);
        state
            .enterprise
            .hosted_policy
            .project(&mut identity)
            .expect("project use-only identity");
        assert!(state
            .enterprise
            .hosted_policy
            .authorize_permission(Some(&identity), AccessPermission::HostedUse)
            .is_ok());
        assert!(state
            .enterprise
            .hosted_policy
            .authorize_permission(Some(&identity), AccessPermission::HostedAutomationWrite)
            .is_err());

        let scope = tenant("alice");
        let calls = install_capturing_codex_provider(
            &state,
            "provider must not run",
            &[(&scope, "planner-token")],
        )
        .await;
        let before_sessions = state.storage.list_sessions().await.len();
        for path in ["/workflow-plans/preview", "/workflow-plans/chat/start"] {
            let response = hosted_planner_app(&state, "alice", 2)
                .oneshot(planner_request(path))
                .await
                .expect("planner denial");
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{path}");
        }
        assert_eq!(state.storage.list_sessions().await.len(), before_sessions);
        assert!(calls.lock().expect("provider calls").is_empty());

        let host = crate::http::workflow_planner_host::WorkflowPlannerHost::new(
            &state,
            &scope,
            Some(&identity),
        );
        assert!(host
            .create_planner_session("blocked", "/tmp")
            .await
            .is_err());
        assert_eq!(state.storage.list_sessions().await.len(), before_sessions);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn planner_dispatch_rechecks_write_after_hosted_revocation() {
        use crate::http::session_run_retry::{
            provider_auth_test_support::install_capturing_codex_provider,
            scope_provider_auth_for_tenant, PromptExecutionSurface,
        };

        let (state, policy_dir) = hosted_state().await;
        let scope = tenant("alice");
        let mut identity = verified("alice", "member");
        state
            .enterprise
            .hosted_policy
            .project(&mut identity)
            .expect("project writer");
        let calls =
            install_capturing_codex_provider(&state, "not JSON", &[(&scope, "planner-token")])
                .await;
        let model = ModelSpec {
            provider_id: "openai-codex".to_string(),
            model_id: "codex-test".to_string(),
        };
        let first = scope_provider_auth_for_tenant(
            &state,
            &scope,
            Some(&identity),
            PromptExecutionSurface::Planner,
            None,
            Some("initial"),
            Some(&model.provider_id),
            state.providers.complete_for_provider(
                Some(&model.provider_id),
                "initial planner dispatch",
                Some(&model.model_id),
            ),
        )
        .await
        .expect("writer initial dispatch");
        assert_eq!(first, "not JSON");
        assert_eq!(calls.lock().expect("provider calls").len(), 1);

        revoke_member_write(&state, &policy_dir).await;
        let repair = scope_provider_auth_for_tenant(
            &state,
            &scope,
            Some(&identity),
            PromptExecutionSurface::Planner,
            None,
            Some("json-repair"),
            Some(&model.provider_id),
            state.providers.complete_for_provider(
                Some(&model.provider_id),
                "JSON-only repair dispatch",
                Some(&model.model_id),
            ),
        )
        .await;
        assert!(repair.is_err(), "revoked planner repair must not dispatch");
        assert_eq!(calls.lock().expect("provider calls").len(), 1);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn json_repair_never_dispatches_after_write_revocation() {
        let (state, policy_dir) = hosted_state().await;
        let scope = tenant("alice");
        let mut identity = verified("alice", "member");
        state
            .enterprise
            .hosted_policy
            .project(&mut identity)
            .expect("project planner writer");
        let dispatches = Arc::new(AtomicUsize::new(0));
        state
            .providers
            .replace_for_test(
                vec![Arc::new(RevokingPlannerProvider {
                    state: state.clone(),
                    policy_path: policy_dir.path().join("policy.json"),
                    dispatches: dispatches.clone(),
                })],
                Some("planner-revocation-test".to_string()),
            )
            .await;
        state
            .providers
            .set_tenant_provider_bearer_token(
                &scope,
                "planner-revocation-test",
                "planner-test-token".to_string(),
            )
            .await;

        let host = crate::http::workflow_planner_host::WorkflowPlannerHost::new(
            &state,
            &scope,
            Some(&identity),
        );
        let result = host
            .invoke_planner_llm(PlannerLlmInvocation {
                session_title: "Revoked during JSON repair".to_string(),
                workspace_root: "/tmp".to_string(),
                model: ModelSpec {
                    provider_id: "planner-revocation-test".to_string(),
                    model_id: "planner-revocation-model".to_string(),
                },
                prompt: "Produce a workflow plan as JSON".to_string(),
                run_key: "planner-repair-revocation".to_string(),
                timeout_ms: 5_000,
                override_env: "TANDEM_PLANNER_REPAIR_REVOCATION_TEST_UNUSED".to_string(),
            })
            .await;
        assert!(result.is_err(), "revoked repair must fail");
        assert_eq!(
            dispatches.load(Ordering::SeqCst),
            1,
            "only the pre-revocation invalid-JSON dispatch may reach the provider"
        );
    }

    #[tokio::test]
    async fn writer_and_standalone_local_preview_remain_available() {
        let (state, _policy_dir) = hosted_state().await;
        let response = hosted_planner_app(&state, "alice", 1)
            .oneshot(planner_request("/workflow-plans/preview"))
            .await
            .expect("hosted writer preview");
        assert_eq!(response.status(), StatusCode::OK);

        let local = test_state().await;
        let app = crate::http::routes_workflow_planner::apply(Router::new())
            .layer(Extension(TenantContext::local_implicit()))
            .with_state(local);
        let response = app
            .oneshot(planner_request("/workflow-plans/preview"))
            .await
            .expect("standalone preview");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn versioned_hosted_planner_identity_fails_if_policy_source_disappears() {
        let state = test_state().await;
        let asserted = verified("alice", "member");
        assert!(
            crate::http::workflow_planner_policy::require_live_planner_write(
                &state,
                &tenant("alice"),
                Some(&asserted),
            )
            .is_err()
        );
        assert!(
            crate::http::workflow_planner_policy::require_live_planner_write(
                &state,
                &TenantContext::local_implicit(),
                None,
            )
            .is_ok()
        );
    }
}
