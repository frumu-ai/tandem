// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

async fn retry_strict_kb_non_streaming_synthesis(
    state: &AppState,
    provider_id: &str,
    model_id: Option<&str>,
    messages: &[ChatMessage],
    stream_error: &str,
    session_id: &str,
    run_id: &str,
    tenant_context: &TenantContext,
    verified_tenant_context: Option<&VerifiedTenantContext>,
) -> Result<Option<StrictKbSynthesisResponse>, String> {
    tracing::warn!(
        error = %stream_error,
        "strict KB synthesis stream failed; retrying with non-streamed completion"
    );
    let prompt = messages
        .iter()
        .map(|message| format!("{}:\n{}", message.role, message.content))
        .collect::<Vec<_>>()
        .join("\n\n");
    let fallback_messages = [ChatMessage {
        role: String::new(),
        content: prompt,
        attachments: Vec::new(),
    }];
    let operation_id = format!("{session_id}:kb_synthesis:completion_fallback");
    let prepared = crate::provider_egress::prepare_chat_messages(
        state,
        Some(tenant_context),
        verified_tenant_context,
        Some(run_id),
        session_id,
        &operation_id,
        "server.session_kb_grounding.completion_fallback",
        crate::provider_egress::ServerProviderEgressKind::KnowledgeBase,
        provider_id,
        model_id,
        &fallback_messages,
    )
    .await?;
    let prompt = prepared
        .messages
        .first()
        .map(|message| message.content.as_str())
        .unwrap_or_default();
    let dispatch = state.providers.complete_with_egress_permit(
        &prepared.permit,
        Some(provider_id),
        prompt,
        model_id,
    );
    let authority = crate::http::session_run_retry::DirectProviderStreamAuthority::new(
        state,
        tenant_context,
        verified_tenant_context,
        crate::http::session_run_retry::PromptExecutionSurface::KnowledgeBase,
        CancellationToken::new(),
    );
    authority
        .guarded_future(
            crate::http::session_run_retry::scope_provider_auth_for_tenant(
                state,
                tenant_context,
                verified_tenant_context,
                crate::http::session_run_retry::PromptExecutionSurface::KnowledgeBase,
                Some(session_id),
                Some(run_id),
                Some(provider_id),
                dispatch,
            ),
        )
        .await
        .map_err(str::to_string)?
        .map_err(|error| error.to_string())
        .map(|completion| parse_strict_synthesis_response(&completion))
}
