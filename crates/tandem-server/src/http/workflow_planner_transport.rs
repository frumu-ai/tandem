// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use futures::StreamExt;
use tandem_observability::{emit_event, ObservabilityEvent, ProcessKind};
use tandem_providers::{ChatMessage, StreamChunk, TokenUsage};
use tandem_types::ToolMode;
use tokio_util::sync::CancellationToken;
use tracing::Level;

use super::*;

fn workflow_planner_progress_event(
    tenant_context: &tandem_types::TenantContext,
    phase: &str,
    session_id: &str,
    run_id: &str,
    provider_id: &str,
    model_id: &str,
    response_chars: usize,
    elapsed_ms: u64,
) -> tandem_types::EngineEvent {
    crate::routines::types::tenant_scoped_engine_event(
        "workflow_planner.progress",
        tenant_context,
        json!({
            "phase": phase,
            "sessionID": session_id,
            "runID": run_id,
            "providerID": provider_id,
            "modelID": model_id,
            "responseChars": response_chars,
            "elapsedMs": elapsed_ms,
        }),
    )
}

#[allow(clippy::too_many_arguments)]
fn publish_workflow_planner_progress(
    state: &AppState,
    tenant_context: &tandem_types::TenantContext,
    phase: &str,
    session_id: &str,
    run_id: &str,
    provider_id: &str,
    model_id: &str,
    response_chars: usize,
    started_at: std::time::Instant,
) {
    let elapsed_ms = u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
    state.event_bus.publish(workflow_planner_progress_event(
        tenant_context,
        phase,
        session_id,
        run_id,
        provider_id,
        model_id,
        response_chars,
        elapsed_ms,
    ));
}

pub(crate) async fn invoke_planner_provider(
    state: &AppState,
    session_id: &str,
    model: &tandem_types::ModelSpec,
    prompt: String,
    timeout_ms: u64,
    run_id: &str,
    tenant_context: &tandem_types::TenantContext,
) -> Result<String, tandem_plan_compiler::api::PlannerInvocationFailure> {
    let cancel = CancellationToken::new();
    let started_at = std::time::Instant::now();
    publish_workflow_planner_progress(
        state,
        tenant_context,
        "dispatch",
        session_id,
        run_id,
        model.provider_id.as_str(),
        model.model_id.as_str(),
        0,
        started_at,
    );
    emit_event(
        Level::INFO,
        ProcessKind::Engine,
        ObservabilityEvent {
            event: "provider.call.start",
            component: "workflow.planner",
            org_id: None,
            workspace_id: None,
            correlation_id: None,
            session_id: Some(session_id),
            run_id: Some(run_id),
            message_id: None,
            provider_id: Some(model.provider_id.as_str()),
            model_id: Some(model.model_id.as_str()),
            status: Some("dispatch"),
            error_code: None,
            detail: Some("planner provider dispatch"),
        },
    );

    let planner_future = async {
        let planner_failure = |error: &str| tandem_plan_compiler::api::PlannerInvocationFailure {
            reason: super::workflow_planner_policy::classify_planner_provider_failure_reason(error)
                .to_string(),
            detail: Some(truncate_text(error, 500)),
        };
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: prompt.clone(),
            attachments: Vec::new(),
        }];
        let session = state.storage.get_session(session_id).await;
        let operation_id = format!("{session_id}:workflow_planner");
        let prepared = crate::provider_egress::prepare_chat_messages(
            state,
            Some(tenant_context),
            session
                .as_ref()
                .and_then(|session| session.verified_tenant_context.as_ref()),
            Some(run_id),
            session_id,
            &operation_id,
            "server.workflow_planner",
            crate::provider_egress::ServerProviderEgressKind::WorkflowPlanner,
            model.provider_id.as_str(),
            Some(model.model_id.as_str()),
            &messages,
        )
        .await
        .map_err(|error| planner_failure(&error))?;
        let messages = prepared.messages;
        let prepared_prompt = messages
            .iter()
            .map(|message| format!("{}: {}", message.role, message.content))
            .collect::<Vec<_>>()
            .join("\n");
        let completion_fallback = || async {
            publish_workflow_planner_progress(
                state,
                tenant_context,
                "retrying",
                session_id,
                run_id,
                model.provider_id.as_str(),
                model.model_id.as_str(),
                0,
                started_at,
            );
            tracing::warn!(
                session_id = %session_id,
                provider_id = %model.provider_id,
                model_id = %model.model_id,
                "workflow planner stream decode failed; retrying with non-streamed completion"
            );
            let fallback_messages = [ChatMessage {
                role: String::new(),
                content: prepared_prompt.clone(),
                attachments: Vec::new(),
            }];
            let fallback_operation_id = format!("{operation_id}:completion_fallback");
            let fallback_prepared = crate::provider_egress::prepare_chat_messages(
                state,
                Some(tenant_context),
                session
                    .as_ref()
                    .and_then(|session| session.verified_tenant_context.as_ref()),
                Some(run_id),
                session_id,
                &fallback_operation_id,
                "server.workflow_planner.completion_fallback",
                crate::provider_egress::ServerProviderEgressKind::WorkflowPlanner,
                model.provider_id.as_str(),
                Some(model.model_id.as_str()),
                &fallback_messages,
            )
            .await
            .map_err(|error| planner_failure(&error))?;
            let fallback_prompt = fallback_prepared
                .messages
                .first()
                .map(|message| message.content.as_str())
                .unwrap_or_default();
            state
                .providers
                .complete_with_egress_permit(
                    &fallback_prepared.permit,
                    Some(model.provider_id.as_str()),
                    fallback_prompt,
                    Some(model.model_id.as_str()),
                )
                .await
                .map(|output| {
                    let response_chars = output.chars().count();
                    (output, None, response_chars)
                })
                .map_err(|error| planner_failure(&error.to_string()))
        };
        state.event_bus.publish(tandem_types::EngineEvent::new(
            "context.budget.bypassed",
            json!({
                "component": "workflow.planner",
                "reason": "direct provider send outside engine-loop context budget accounting",
                "sessionID": session_id,
                "promptMessageCount": messages.len(),
                "promptChars": prompt.len(),
                "providerID": model.provider_id,
                "modelID": model.model_id,
            }),
        ));
        let stream = match state
            .providers
            .stream_with_egress_permit(
                &prepared.permit,
                Some(model.provider_id.as_str()),
                Some(model.model_id.as_str()),
                messages,
                ToolMode::None,
                None,
                tandem_types::SamplingParams::default(),
                cancel.clone(),
            )
            .await
        {
            Ok(stream) => stream,
            Err(error) => {
                let error_text = error.to_string();
                if should_retry_planner_completion_fallback(&error_text) {
                    return completion_fallback().await;
                }
                return Err(planner_failure(&error_text));
            }
        };
        publish_workflow_planner_progress(
            state,
            tenant_context,
            "waiting",
            session_id,
            run_id,
            model.provider_id.as_str(),
            model.model_id.as_str(),
            0,
            started_at,
        );
        tokio::pin!(stream);
        let mut output = String::new();
        let mut saw_first_delta = false;
        let mut saw_reasoning_delta = false;
        let mut response_chars = 0usize;
        let mut last_progress_chars = 0usize;
        let mut last_progress_at = std::time::Instant::now();
        let mut usage: Option<TokenUsage> = None;
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(StreamChunk::TextDelta(delta)) => {
                    response_chars = response_chars.saturating_add(delta.chars().count());
                    if !saw_first_delta && !delta.trim().is_empty() {
                        saw_first_delta = true;
                        emit_event(
                            Level::INFO,
                            ProcessKind::Engine,
                            ObservabilityEvent {
                                event: "provider.call.first_byte",
                                component: "workflow.planner",
                                org_id: None,
                                workspace_id: None,
                                correlation_id: None,
                                session_id: Some(session_id),
                                run_id: Some(run_id),
                                message_id: None,
                                provider_id: Some(model.provider_id.as_str()),
                                model_id: Some(model.model_id.as_str()),
                                status: Some("streaming"),
                                error_code: None,
                                detail: Some("first text delta"),
                            },
                        );
                        publish_workflow_planner_progress(
                            state,
                            tenant_context,
                            "streaming",
                            session_id,
                            run_id,
                            model.provider_id.as_str(),
                            model.model_id.as_str(),
                            response_chars,
                            started_at,
                        );
                        last_progress_chars = response_chars;
                        last_progress_at = std::time::Instant::now();
                    } else if saw_first_delta
                        && response_chars > last_progress_chars
                        && (response_chars.saturating_sub(last_progress_chars) >= 512
                            || last_progress_at.elapsed() >= std::time::Duration::from_secs(1))
                    {
                        publish_workflow_planner_progress(
                            state,
                            tenant_context,
                            "streaming",
                            session_id,
                            run_id,
                            model.provider_id.as_str(),
                            model.model_id.as_str(),
                            response_chars,
                            started_at,
                        );
                        last_progress_chars = response_chars;
                        last_progress_at = std::time::Instant::now();
                    }
                    output.push_str(&delta);
                }
                Ok(StreamChunk::ReasoningDelta(delta)) => {
                    if !saw_first_delta && !saw_reasoning_delta && !delta.trim().is_empty() {
                        saw_reasoning_delta = true;
                        publish_workflow_planner_progress(
                            state,
                            tenant_context,
                            "thinking",
                            session_id,
                            run_id,
                            model.provider_id.as_str(),
                            model.model_id.as_str(),
                            response_chars,
                            started_at,
                        );
                    }
                    output.push_str(&delta);
                }
                Ok(StreamChunk::Done {
                    finish_reason: _,
                    usage: provider_usage,
                }) => {
                    usage = provider_usage;
                    break;
                }
                Ok(StreamChunk::ToolCallStart { .. })
                | Ok(StreamChunk::ToolCallDelta { .. })
                | Ok(StreamChunk::ToolCallEnd { .. }) => {}
                Err(error) => {
                    let error_text = error.to_string();
                    if should_retry_planner_completion_fallback(&error_text) {
                        return completion_fallback().await;
                    }
                    return Err(planner_failure(&error_text));
                }
            }
        }
        Ok::<(String, Option<TokenUsage>, usize), tandem_plan_compiler::api::PlannerInvocationFailure>(
            (output, usage, response_chars),
        )
    };

    let planner_future = crate::http::session_run_retry::scope_provider_auth_for_tenant(
        state,
        tenant_context,
        crate::http::session_run_retry::PromptExecutionSurface::Planner,
        Some(session_id),
        Some(run_id),
        Some(model.provider_id.as_str()),
        planner_future,
    );
    match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), planner_future).await {
        Ok(Ok((output, usage, response_chars))) => {
            publish_workflow_planner_progress(
                state,
                tenant_context,
                "validating",
                session_id,
                run_id,
                model.provider_id.as_str(),
                model.model_id.as_str(),
                response_chars,
                started_at,
            );
            let finish_detail = usage
                .as_ref()
                .map(|value| {
                    format!(
                        "planner stream complete (prompt={}, completion={})",
                        value.prompt_tokens, value.completion_tokens
                    )
                })
                .unwrap_or_else(|| "planner stream complete".to_string());
            emit_event(
                Level::INFO,
                ProcessKind::Engine,
                ObservabilityEvent {
                    event: "provider.call.finish",
                    component: "workflow.planner",
                    org_id: None,
                    workspace_id: None,
                    correlation_id: None,
                    session_id: Some(session_id),
                    run_id: Some(run_id),
                    message_id: None,
                    provider_id: Some(model.provider_id.as_str()),
                    model_id: Some(model.model_id.as_str()),
                    status: Some("completed"),
                    error_code: None,
                    detail: Some(&finish_detail),
                },
            );
            Ok(output)
        }
        Ok(Err(error)) => {
            publish_workflow_planner_progress(
                state,
                tenant_context,
                "failed",
                session_id,
                run_id,
                model.provider_id.as_str(),
                model.model_id.as_str(),
                0,
                started_at,
            );
            emit_event(
                Level::ERROR,
                ProcessKind::Engine,
                ObservabilityEvent {
                    event: "provider.call.error",
                    component: "workflow.planner",
                    org_id: None,
                    workspace_id: None,
                    correlation_id: None,
                    session_id: Some(session_id),
                    run_id: Some(run_id),
                    message_id: None,
                    provider_id: Some(model.provider_id.as_str()),
                    model_id: Some(model.model_id.as_str()),
                    status: Some("failed"),
                    error_code: Some(error.reason.as_str()),
                    detail: error.detail.as_deref(),
                },
            );
            Err(error)
        }
        Err(_) => {
            cancel.cancel();
            publish_workflow_planner_progress(
                state,
                tenant_context,
                "failed",
                session_id,
                run_id,
                model.provider_id.as_str(),
                model.model_id.as_str(),
                0,
                started_at,
            );
            emit_event(
                Level::WARN,
                ProcessKind::Engine,
                ObservabilityEvent {
                    event: "provider.call.error",
                    component: "workflow.planner",
                    org_id: None,
                    workspace_id: None,
                    correlation_id: None,
                    session_id: Some(session_id),
                    run_id: Some(run_id),
                    message_id: None,
                    provider_id: Some(model.provider_id.as_str()),
                    model_id: Some(model.model_id.as_str()),
                    status: Some("failed"),
                    error_code: Some("timeout"),
                    detail: Some("workflow planner llm call timed out before completion"),
                },
            );
            Err(tandem_plan_compiler::api::PlannerInvocationFailure {
                reason: "timeout".to_string(),
                detail: Some("Workflow planner timed out before completion.".to_string()),
            })
        }
    }
}

fn truncate_text(input: &str, max_len: usize) -> String {
    let mut chars = input.chars();
    let truncated: String = chars.by_ref().take(max_len).collect();
    if chars.next().is_some() {
        format!("{}...", truncated.trim_end())
    } else {
        truncated
    }
}

fn should_retry_planner_completion_fallback(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("error decoding response body")
        || lower.contains("stream chunk error")
        || lower.contains("unexpected eof")
}

#[cfg(test)]
mod tests {
    use super::{
        invoke_planner_provider, should_retry_planner_completion_fallback,
        workflow_planner_progress_event,
    };
    use crate::http::session_run_retry::provider_auth_test_support::install_capturing_codex_provider;
    use futures::Stream;
    use std::{
        pin::Pin,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };
    use tandem_providers::{ChatMessage, Provider, ProviderAuthOverride, StreamChunk};
    use tandem_types::{
        ModelInfo, ModelSpec, ProviderInfo, SamplingParams, Session, TenantContext, ToolMode,
        ToolSchema,
    };
    use tokio_util::sync::CancellationToken;

    #[test]
    fn workflow_planner_progress_is_tenant_scoped_and_content_free() {
        let tenant = TenantContext::explicit("planner-org", "planner-workspace", None);
        let event = workflow_planner_progress_event(
            &tenant,
            "streaming",
            "planner-session",
            "workflow-plan-build:automations_page",
            "openai-codex",
            "gpt-test",
            1_024,
            2_500,
        );

        assert_eq!(event.event_type, "workflow_planner.progress");
        assert_eq!(event.properties["phase"], "streaming");
        assert_eq!(event.properties["responseChars"], 1_024);
        assert_eq!(event.properties["elapsedMs"], 2_500);
        assert!(event.properties.get("tenantContext").is_some());
        for forbidden in ["text", "delta", "reasoning", "prompt", "response"] {
            assert!(event.properties.get(forbidden).is_none());
        }
    }

    struct StreamFailureProvider {
        complete_calls: Arc<AtomicUsize>,
    }

    #[async_trait::async_trait]
    impl Provider for StreamFailureProvider {
        fn info(&self) -> ProviderInfo {
            ProviderInfo {
                id: "planner-fallback-test".to_string(),
                name: "planner fallback test".to_string(),
                models: vec![ModelInfo {
                    id: "planner-fallback-model".to_string(),
                    provider_id: "planner-fallback-test".to_string(),
                    display_name: "Planner Fallback Model".to_string(),
                    context_window: 8_192,
                }],
            }
        }

        async fn complete(
            &self,
            _prompt: &str,
            _model_override: Option<&str>,
        ) -> anyhow::Result<String> {
            self.complete_calls.fetch_add(1, Ordering::SeqCst);
            Ok("completion must not run".to_string())
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
            anyhow::bail!("stream chunk error")
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

    #[test]
    fn planner_completion_fallback_retries_stream_decode_failures() {
        assert!(should_retry_planner_completion_fallback(
            "provider stream chunk error: error decoding response body"
        ));
        assert!(should_retry_planner_completion_fallback(
            "stream ended with unexpected eof"
        ));
    }

    #[test]
    fn planner_completion_fallback_ignores_auth_failures() {
        assert!(!should_retry_planner_completion_fallback(
            "provider authentication failed (401)"
        ));
    }

    #[tokio::test]
    #[serial_test::serial(data_boundary_env)]
    async fn planner_completion_fallback_evaluates_the_rebuilt_payload() {
        let previous = [
            "TANDEM_DATA_BOUNDARY_MODE",
            "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
            "TANDEM_DATA_BOUNDARY_MAX_PAYLOAD_BYTES",
        ]
        .map(|name| (name, std::env::var(name).ok()));
        std::env::set_var("TANDEM_DATA_BOUNDARY_MODE", "enforce");
        std::env::set_var(
            "TANDEM_DATA_BOUNDARY_PROVIDER_CLASSES",
            "planner-fallback-test=approved_external",
        );
        // `user\nx\n` fits, while the completion fallback's exact `user: x\n`
        // prompt payload does not.
        std::env::set_var("TANDEM_DATA_BOUNDARY_MAX_PAYLOAD_BYTES", "7");

        let state = crate::test_support::test_state().await;
        let complete_calls = Arc::new(AtomicUsize::new(0));
        state
            .providers
            .replace_for_test(
                vec![Arc::new(StreamFailureProvider {
                    complete_calls: complete_calls.clone(),
                })],
                Some("planner-fallback-test".to_string()),
            )
            .await;
        let tenant = TenantContext::explicit("planner-org", "planner-workspace", None);
        let mut session = Session::new(Some("planner fallback".to_string()), Some(".".to_string()));
        session.tenant_context = tenant.clone();
        let session_id = session.id.clone();
        state
            .storage
            .save_session(session)
            .await
            .expect("save planner fallback session");
        let model = ModelSpec {
            provider_id: "planner-fallback-test".to_string(),
            model_id: "planner-fallback-model".to_string(),
        };

        let error = invoke_planner_provider(
            &state,
            &session_id,
            &model,
            "x".to_string(),
            5_000,
            "planner-fallback-run",
            &tenant,
        )
        .await
        .expect_err("larger fallback payload must be evaluated and blocked");

        assert_eq!(complete_calls.load(Ordering::SeqCst), 0);
        assert!(
            error
                .detail
                .as_deref()
                .is_some_and(|detail| detail.contains("payload_too_large")),
            "unexpected planner failure: {error:?}"
        );

        for (name, value) in previous {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn planner_transport_isolates_hosted_codex_auth_from_local_and_other_tenants() {
        let state = crate::test_support::test_state().await;
        let tenant_a = TenantContext::explicit("planner-org-a", "planner-workspace-a", None);
        let tenant_b = TenantContext::explicit("planner-org-b", "planner-workspace-b", None);
        let tenant_missing =
            TenantContext::explicit("planner-org-missing", "planner-workspace-missing", None);
        let captured = install_capturing_codex_provider(
            &state,
            "planner-ok",
            &[
                (&tenant_a, "planner-token-a"),
                (&tenant_b, "planner-token-b"),
            ],
        )
        .await;
        let model = ModelSpec {
            provider_id: "openai-codex".to_string(),
            model_id: "codex-test".to_string(),
        };

        for (index, tenant_context) in [&tenant_a, &tenant_b, &tenant_missing]
            .into_iter()
            .enumerate()
        {
            let mut session = Session::new(
                Some(format!("planner tenant {index}")),
                Some(".".to_string()),
            );
            session.tenant_context = tenant_context.clone();
            let session_id = session.id.clone();
            state
                .storage
                .save_session(session)
                .await
                .expect("save planner session");
            let output = invoke_planner_provider(
                &state,
                &session_id,
                &model,
                "build a workflow".to_string(),
                5_000,
                &format!("planner-run-{index}"),
                tenant_context,
            )
            .await
            .expect("planner dispatch");
            assert_eq!(output, "planner-ok");
        }

        assert_eq!(
            captured.lock().expect("provider auth capture").as_slice(),
            [
                ProviderAuthOverride::Bearer("planner-token-a".to_string()),
                ProviderAuthOverride::Bearer("planner-token-b".to_string()),
                ProviderAuthOverride::Suppress,
            ]
        );
    }
}
