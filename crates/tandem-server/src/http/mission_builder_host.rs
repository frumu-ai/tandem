// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use futures::StreamExt;
use serde::Deserialize;
use serde_json::Value;
use tandem_observability::{emit_event, ObservabilityEvent, ProcessKind};
use tandem_plan_compiler::api as compiler_api;
use tandem_providers::{ChatMessage, StreamChunk};
use tandem_types::{ModelSpec, ToolMode};
use tandem_workflows::MissionBlueprint;
use tokio_util::sync::CancellationToken;
use tracing::Level;

use super::*;

#[derive(Debug, Deserialize)]
pub(crate) struct MissionDraftGenerationOutput {
    pub blueprint: MissionBlueprint,
    #[serde(default)]
    pub suggested_schedule: Option<Value>,
    #[serde(default)]
    pub generation_warnings: Vec<String>,
}

pub(crate) async fn generate_mission_draft(
    state: &AppState,
    intent: &str,
    workspace_root: &str,
    archetype_id: Option<&str>,
    tenant_context: &tandem_types::TenantContext,
    verified_tenant_context: Option<&tandem_types::VerifiedTenantContext>,
) -> Result<MissionDraftGenerationOutput, String> {
    if let Some(payload) = super::workflow_planner_policy::planner_test_override_payload(
        "TANDEM_MISSION_BUILDER_TEST_GENERATE_RESPONSE",
        false,
    ) {
        return decode_generation_output(payload, workspace_root);
    }

    let model = resolve_mission_builder_model(state).await.ok_or_else(|| {
        "No default provider model is configured for mission generation.".to_string()
    })?;
    let prompt = build_mission_generation_prompt(intent, workspace_root, archetype_id);
    let session_id = format!("mission-builder-{}", uuid::Uuid::new_v4());
    let run_id = format!("mission-builder-run-{}", uuid::Uuid::new_v4());
    let output = invoke_mission_builder_provider(
        state,
        &session_id,
        &run_id,
        &model,
        prompt.clone(),
        tenant_context,
        verified_tenant_context,
    )
    .await?;

    if let Some(value) = extract_generation_json_value(&output) {
        return decode_generation_output(value, workspace_root);
    }

    tracing::warn!(
        "mission builder returned non-JSON text; requesting a JSON-only repair response"
    );
    let repair_prompt = build_generation_json_repair_prompt(&prompt, &output);
    let repair_output = invoke_mission_builder_provider(
        state,
        &session_id,
        &run_id,
        &model,
        repair_prompt,
        tenant_context,
        verified_tenant_context,
    )
    .await?;
    let repaired = extract_generation_json_value(&repair_output).ok_or_else(|| {
        "Mission builder returned text without valid JSON, including after a repair retry."
            .to_string()
    })?;
    decode_generation_output(repaired, workspace_root)
}

async fn resolve_mission_builder_model(state: &AppState) -> Option<ModelSpec> {
    let effective = state.config.get_effective_value().await;
    crate::default_model_spec_from_effective_config(&effective)
}

fn decode_generation_output(
    value: Value,
    workspace_root: &str,
) -> Result<MissionDraftGenerationOutput, String> {
    let mut payload: MissionDraftGenerationOutput =
        serde_json::from_value(value).map_err(|error| truncate_text(&error.to_string(), 500))?;
    payload.blueprint.workspace_root = workspace_root.to_string();
    payload.blueprint.mission_id = if payload.blueprint.mission_id.trim().is_empty() {
        format!("mission_{}", uuid::Uuid::new_v4().simple())
    } else {
        payload.blueprint.mission_id.trim().to_string()
    };
    payload.blueprint.shared_context = payload
        .blueprint
        .shared_context
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    payload.generation_warnings = payload
        .generation_warnings
        .into_iter()
        .map(|row| row.trim().to_string())
        .filter(|row| !row.is_empty())
        .collect();
    Ok(payload)
}

fn build_mission_generation_prompt(
    intent: &str,
    workspace_root: &str,
    archetype_id: Option<&str>,
) -> String {
    let archetype_line = archetype_id
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!("Archetype hint: {value}"))
        .unwrap_or_else(|| {
            "Archetype hint: choose the smallest suitable staged pattern.".to_string()
        });
    format!(
        concat!(
            "Design one Tandem mission blueprint from the human intent below.\n",
            "Return JSON only. Do not use markdown fences or commentary.\n\n",
            "Required response shape:\n",
            "{{\n",
            "  \"blueprint\": {{\n",
            "    \"mission_id\": \"string\",\n",
            "    \"title\": \"string\",\n",
            "    \"goal\": \"string\",\n",
            "    \"success_criteria\": [\"string\"],\n",
            "    \"shared_context\": \"string\",\n",
            "    \"workspace_root\": \"{workspace_root}\",\n",
            "    \"phases\": [],\n",
            "    \"milestones\": [],\n",
            "    \"team\": {{}},\n",
            "    \"workstreams\": [],\n",
            "    \"review_stages\": []\n",
            "  }},\n",
            "  \"suggested_schedule\": {{ \"type\": \"manual\" | \"interval\" | \"cron\", \"interval_seconds\"?: number, \"cron_expression\"?: string, \"timezone\"?: \"UTC\" }},\n",
            "  \"generation_warnings\": [\"string\"]\n",
            "}}\n\n",
            "Mission requirements:\n",
            "- Start from the user's intent and infer the mission title, goal, shared context, success criteria, workstreams, and reviews.\n",
            "- The human did not pre-fill mission goal, shared context, or success criteria; derive them.\n",
            "- Keep the mission simple for humans: one clear mission, then let the engine handle setup.\n",
            "- Use 3 to 7 scoped workstreams with one responsibility each.\n",
            "- Use explicit depends_on only for real handoffs.\n",
            "- Use input_refs when a stage needs named upstream outputs.\n",
            "- Every workstream must include a strong prompt and a concrete output_contract.\n",
            "- Add review, test, or approval stages only where they materially improve quality or promotion control.\n",
            "- Assume missions may run repeatedly over days, weeks, or months.\n",
            "- Infer a schedule when the intent clearly implies recurrence; otherwise use manual.\n",
            "- Prefer durable outputs and reusable validated artifacts over one-off chat responses.\n",
            "- Shared context should contain stable cross-cutting constraints, audience, deadlines, tone, approved sources, and things to avoid.\n",
            "- Success criteria must be measurable and concise.\n",
            "- Use realistic role names and output contract kinds.\n\n",
            "Scheduler guidance:\n",
            "- If the user asks for daily, weekly, monthly, every weekday, every morning, or another clear cadence, infer it.\n",
            "- If the cadence is ambiguous, choose manual and add a warning.\n",
            "- Timezone should be UTC.\n\n",
            "Human intent:\n",
            "Workspace root: {workspace_root}\n",
            "{archetype_line}\n",
            "Intent: {intent}\n"
        ),
        workspace_root = workspace_root,
        archetype_line = archetype_line,
        intent = intent.trim()
    )
}

fn build_generation_json_repair_prompt(original_prompt: &str, invalid_output: &str) -> String {
    format!(
        concat!(
            "You are revising a Tandem mission blueprint generation response.\n",
            "Return JSON only.\n",
            "The previous response was not valid JSON.\n",
            "Return one valid JSON object that matches the requested mission-builder shape exactly.\n",
            "Do not add markdown fences, prose, explanations, or commentary.\n\n",
            "Original prompt:\n{}\n\n",
            "Invalid response to repair:\n{}\n"
        ),
        original_prompt.trim(),
        invalid_output.trim()
    )
}

async fn invoke_mission_builder_provider(
    state: &AppState,
    session_id: &str,
    run_id: &str,
    model: &ModelSpec,
    prompt: String,
    tenant_context: &tandem_types::TenantContext,
    verified_tenant_context: Option<&tandem_types::VerifiedTenantContext>,
) -> Result<String, String> {
    let cancel = CancellationToken::new();
    emit_event(
        Level::INFO,
        ProcessKind::Engine,
        ObservabilityEvent {
            event: "provider.call.start",
            component: "mission.builder",
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
            detail: Some("mission builder provider dispatch"),
        },
    );

    let builder_future = async {
        let prompt_chars = prompt.len();
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: prompt,
            attachments: Vec::new(),
        }];
        let operation_id = format!("{session_id}:mission_builder");
        let prepared = crate::provider_egress::prepare_chat_messages(
            state,
            Some(tenant_context),
            verified_tenant_context,
            Some(run_id),
            session_id,
            &operation_id,
            "server.mission_builder",
            crate::provider_egress::ServerProviderEgressKind::MissionBuilder,
            model.provider_id.as_str(),
            Some(model.model_id.as_str()),
            &messages,
        )
        .await?;
        let messages = prepared.messages;
        state.event_bus.publish(tandem_types::EngineEvent::new(
            "context.budget.bypassed",
            json!({
                "component": "mission.builder",
                "reason": "direct provider send outside engine-loop context budget accounting",
                "sessionID": session_id,
                "promptMessageCount": messages.len(),
                "promptChars": prompt_chars,
                "providerID": model.provider_id,
                "modelID": model.model_id,
            }),
        ));
        let stream = state
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
            .map_err(|error| truncate_text(&error.to_string(), 500))?;
        tokio::pin!(stream);
        let mut output = String::new();
        let mut saw_first_delta = false;
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
                                component: "mission.builder",
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
                    }
                    output.push_str(&delta);
                }
                Ok(StreamChunk::ReasoningDelta(delta)) => output.push_str(&delta),
                Ok(StreamChunk::Done {
                    finish_reason: _,
                    usage: provider_usage,
                }) => {
                    let detail = provider_usage
                        .as_ref()
                        .map(|value| {
                            format!(
                                "mission builder stream complete (prompt={}, completion={})",
                                value.prompt_tokens, value.completion_tokens
                            )
                        })
                        .unwrap_or_else(|| "mission builder stream complete".to_string());
                    emit_event(
                        Level::INFO,
                        ProcessKind::Engine,
                        ObservabilityEvent {
                            event: "provider.call.finish",
                            component: "mission.builder",
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
                            detail: Some(&detail),
                        },
                    );
                    break;
                }
                Ok(StreamChunk::ToolCallStart { .. })
                | Ok(StreamChunk::ToolCallDelta { .. })
                | Ok(StreamChunk::ToolCallEnd { .. }) => {}
                Err(error) => return Err(truncate_text(&error.to_string(), 500)),
            }
        }
        Ok::<String, String>(output)
    };

    let builder_future = crate::http::session_run_retry::scope_provider_auth_for_tenant(
        state,
        tenant_context,
        crate::http::session_run_retry::PromptExecutionSurface::MissionBuilder,
        Some(session_id),
        Some(run_id),
        Some(model.provider_id.as_str()),
        builder_future,
    );
    match tokio::time::timeout(
        std::time::Duration::from_millis(
            super::workflow_planner_policy::planner_build_timeout_ms(),
        ),
        builder_future,
    )
    .await
    {
        Ok(Ok(output)) if !output.trim().is_empty() => Ok(output),
        Ok(Ok(_)) => Err("Mission builder completed without assistant text.".to_string()),
        Ok(Err(error)) => {
            emit_event(
                Level::ERROR,
                ProcessKind::Engine,
                ObservabilityEvent {
                    event: "provider.call.error",
                    component: "mission.builder",
                    org_id: None,
                    workspace_id: None,
                    correlation_id: None,
                    session_id: Some(session_id),
                    run_id: Some(run_id),
                    message_id: None,
                    provider_id: Some(model.provider_id.as_str()),
                    model_id: Some(model.model_id.as_str()),
                    status: Some("failed"),
                    error_code: Some("provider_request_failed"),
                    detail: Some(&error),
                },
            );
            Err(error)
        }
        Err(_) => {
            cancel.cancel();
            Err("Mission builder timed out before completion.".to_string())
        }
    }
}

fn extract_generation_json_value(text: &str) -> Option<Value> {
    compiler_api::extract_json_value_from_text(text)
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

#[cfg(test)]
mod provider_auth_scope_tests {
    use super::*;
    use crate::http::session_run_retry::provider_auth_test_support::install_capturing_codex_provider;
    use tandem_providers::ProviderAuthOverride;
    use tandem_types::TenantContext;

    #[tokio::test]
    #[serial_test::serial]
    async fn mission_builder_isolates_hosted_codex_auth_from_local_and_other_tenants() {
        let state = crate::test_support::test_state().await;
        let tenant_a = TenantContext::explicit("mission-org-a", "mission-workspace-a", None);
        let tenant_b = TenantContext::explicit("mission-org-b", "mission-workspace-b", None);
        let tenant_missing =
            TenantContext::explicit("mission-org-missing", "mission-workspace-missing", None);
        let captured = install_capturing_codex_provider(
            &state,
            "mission-ok",
            &[
                (&tenant_a, "mission-token-a"),
                (&tenant_b, "mission-token-b"),
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
            let output = invoke_mission_builder_provider(
                &state,
                &format!("mission-session-{index}"),
                &format!("mission-run-{index}"),
                &model,
                "build a mission".to_string(),
                tenant_context,
                None,
            )
            .await
            .expect("mission builder dispatch");
            assert_eq!(output, "mission-ok");
        }

        assert_eq!(
            captured.lock().expect("provider auth capture").as_slice(),
            [
                ProviderAuthOverride::Bearer("mission-token-a".to_string()),
                ProviderAuthOverride::Bearer("mission-token-b".to_string()),
                ProviderAuthOverride::Suppress,
            ]
        );
    }
}
