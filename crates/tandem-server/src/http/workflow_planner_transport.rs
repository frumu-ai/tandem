use futures::StreamExt;
use tandem_observability::{emit_event, ObservabilityEvent, ProcessKind};
use tandem_providers::{ChatMessage, StreamChunk, TokenUsage};
use tandem_types::ToolMode;
use tokio_util::sync::CancellationToken;
use tracing::Level;

use super::*;

pub(crate) async fn invoke_planner_provider(
    state: &AppState,
    session_id: &str,
    model: &tandem_types::ModelSpec,
    prompt: String,
    timeout_ms: u64,
) -> Result<String, tandem_plan_compiler::api::PlannerInvocationFailure> {
    let cancel = CancellationToken::new();
    emit_event(
        Level::INFO,
        ProcessKind::Engine,
        ObservabilityEvent {
            event: "provider.call.start",
            component: "workflow.planner",
            correlation_id: None,
            session_id: Some(session_id),
            run_id: None,
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
        let completion_fallback = || async {
            tracing::warn!(
                session_id = %session_id,
                provider_id = %model.provider_id,
                model_id = %model.model_id,
                "workflow planner stream decode failed; retrying with non-streamed completion"
            );
            state
                .providers
                .complete_for_provider(
                    Some(model.provider_id.as_str()),
                    &prompt,
                    Some(model.model_id.as_str()),
                )
                .await
                .map(|output| (output, None))
                .map_err(|error| planner_failure(&error.to_string()))
        };
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: prompt.clone(),
            attachments: Vec::new(),
        }];
        let stream = match state
            .providers
            .stream_for_provider(
                Some(model.provider_id.as_str()),
                Some(model.model_id.as_str()),
                messages,
                ToolMode::None,
                None,
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
        tokio::pin!(stream);
        let mut output = String::new();
        let mut saw_first_delta = false;
        let mut usage: Option<TokenUsage> = None;
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(StreamChunk::TextDelta(delta)) => {
                    if !saw_first_delta && !delta.trim().is_empty() {
                        saw_first_delta = true;
                        emit_event(
                            Level::INFO,
                            ProcessKind::Engine,
                            ObservabilityEvent {
                                event: "provider.call.first_byte",
                                component: "workflow.planner",
                                correlation_id: None,
                                session_id: Some(session_id),
                                run_id: None,
                                message_id: None,
                                provider_id: Some(model.provider_id.as_str()),
                                model_id: Some(model.model_id.as_str()),
                                status: Some("streaming"),
                                error_code: None,
                                detail: Some("first text delta"),
                            },
                        );
                    }
                    output.push_str(&delta);
                }
                Ok(StreamChunk::ReasoningDelta(delta)) => {
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
        Ok::<(String, Option<TokenUsage>), tandem_plan_compiler::api::PlannerInvocationFailure>((
            output, usage,
        ))
    };

    match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), planner_future).await {
        Ok(Ok((output, usage))) => {
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
                    correlation_id: None,
                    session_id: Some(session_id),
                    run_id: None,
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
            emit_event(
                Level::ERROR,
                ProcessKind::Engine,
                ObservabilityEvent {
                    event: "provider.call.error",
                    component: "workflow.planner",
                    correlation_id: None,
                    session_id: Some(session_id),
                    run_id: None,
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
            emit_event(
                Level::WARN,
                ProcessKind::Engine,
                ObservabilityEvent {
                    event: "provider.call.error",
                    component: "workflow.planner",
                    correlation_id: None,
                    session_id: Some(session_id),
                    run_id: None,
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
    use super::should_retry_planner_completion_fallback;

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
}
