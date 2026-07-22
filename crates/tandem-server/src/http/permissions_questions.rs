// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use std::collections::HashSet;
use tandem_types::VerifiedTenantContext;

type QueueError = (StatusCode, Json<ErrorEnvelope>);

#[derive(Debug, Deserialize)]
pub(super) struct PermissionReplyInput {
    pub reply: String,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct QuestionReplyInput {
    #[serde(default)]
    pub _answers: Vec<Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct QuestionAnswerInput {
    pub answer: Option<String>,
}

fn queue_error(status: StatusCode, message: impl Into<String>, code: ErrorCode) -> QueueError {
    (status, Json(ErrorEnvelope::new(message, code)))
}

fn ensure_queue_reviewer(
    headers: &HeaderMap,
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
    verified: Option<&VerifiedTenantContext>,
) -> Result<String, QueueError> {
    let reviewer =
        super::governance::resolve_governance_actor(headers, tenant_context, request_principal);
    let standalone_local_owner = tenant_context.is_local_implicit()
        && reviewer.kind == crate::automation_v2::governance::GovernanceActorKind::System;
    if reviewer.kind != crate::automation_v2::governance::GovernanceActorKind::Human
        && !standalone_local_owner
    {
        return Err(queue_error(
            StatusCode::FORBIDDEN,
            "permission and question decisions require a human operator",
            ErrorCode::TenantContextDenied,
        ));
    }
    if !super::workflows::workflow_reviewer_is_eligible(tenant_context, verified) {
        return Err(queue_error(
            StatusCode::FORBIDDEN,
            "decision requires an eligible tenant reviewer",
            ErrorCode::TenantContextDenied,
        ));
    }
    reviewer
        .actor_id
        .or(reviewer.source)
        .filter(|actor| !actor.trim().is_empty())
        .or_else(|| standalone_local_owner.then(|| "standalone-local-owner".to_string()))
        .ok_or_else(|| {
            queue_error(
                StatusCode::FORBIDDEN,
                "reviewer identity is required",
                ErrorCode::TenantContextDenied,
            )
        })
}

fn ensure_independent_permission_reviewer(
    request: &tandem_core::PermissionRequest,
    reviewer: &str,
) -> Result<(), QueueError> {
    if tandem_core::permission_requires_independent_review(request)
        && request
            .requested_by
            .as_deref()
            .is_some_and(|requester| requester.eq_ignore_ascii_case(reviewer))
    {
        return Err(queue_error(
            StatusCode::FORBIDDEN,
            "high-impact permission decisions require an independent reviewer",
            ErrorCode::TenantContextDenied,
        ));
    }
    Ok(())
}

async fn queue_session_visibility(
    state: &AppState,
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
    verified: Option<&VerifiedTenantContext>,
) -> Option<HashSet<String>> {
    if super::workflows::workflow_reviewer_is_eligible(tenant_context, verified) {
        return None;
    }
    let mut caller_tenant = tenant_context.clone();
    caller_tenant.actor_id = request_principal
        .actor_id
        .clone()
        .or(caller_tenant.actor_id);
    Some(
        state
            .storage
            .list_session_summaries()
            .await
            .into_iter()
            .filter(|session| {
                super::sessions_actor_scope::session_visible_to_actor(
                    &caller_tenant,
                    &session.tenant_context,
                )
            })
            .map(|session| session.id)
            .collect(),
    )
}

fn queue_record_owned_by(
    requested_by: Option<&str>,
    session_id: Option<&str>,
    caller: Option<&str>,
    visible_sessions: &HashSet<String>,
) -> bool {
    caller.is_some_and(|actor| {
        requested_by.is_some_and(|requester| requester.eq_ignore_ascii_case(actor))
    }) || session_id.is_some_and(|id| visible_sessions.contains(id))
}

fn map_permission_reply_error(error: tandem_core::PermissionReplyError) -> QueueError {
    match error {
        tandem_core::PermissionReplyError::Expired => queue_error(
            StatusCode::CONFLICT,
            "Permission request has expired",
            ErrorCode::ApprovalReplyInvalid,
        ),
        tandem_core::PermissionReplyError::ActionMismatch => queue_error(
            StatusCode::CONFLICT,
            "Permission request action binding is invalid",
            ErrorCode::ApprovalReplyInvalid,
        ),
        tandem_core::PermissionReplyError::SessionMismatch => queue_error(
            StatusCode::NOT_FOUND,
            "Permission request not found",
            ErrorCode::ApprovalRequestNotFound,
        ),
        tandem_core::PermissionReplyError::PersistenceFailed => queue_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to persist permission decision",
            ErrorCode::ApprovalPersistenceFailed,
        ),
    }
}

async fn append_permission_decision_intent_audit(
    state: &AppState,
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
    reviewer: &str,
    request: &tandem_core::PermissionRequest,
    reply: &str,
    reason: &str,
) -> anyhow::Result<()> {
    crate::audit::append_protected_audit_event(
        state,
        "permission.decision",
        tenant_context,
        Some(reviewer.to_string()),
        json!({
            "requestID": &request.id,
            "sessionID": &request.session_id,
            "permission": &request.permission,
            "pattern": &request.pattern,
            "tool": &request.tool,
            "decision": reply,
            "requestedBy": &request.requested_by,
            "actionDigest": &request.action_digest,
            "expiresAtMs": request.expires_at_ms,
            "reason": reason,
            "principal": {
                "actorID": &request_principal.actor_id,
                "source": &request_principal.source,
            },
        }),
    )
    .await
}

async fn apply_permission_reply(
    state: &AppState,
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
    headers: &HeaderMap,
    verified: Option<&VerifiedTenantContext>,
    expected_session_id: Option<&str>,
    request_id: &str,
    reply: &str,
    reason: &str,
) -> Result<tandem_core::PermissionReplyOutcome, QueueError> {
    let request = state
        .permissions
        .get_for_tenant(request_id, tenant_context)
        .await
        .ok_or_else(|| {
            queue_error(
                StatusCode::NOT_FOUND,
                "Permission request not found",
                ErrorCode::ApprovalRequestNotFound,
            )
        })?;
    if expected_session_id.is_some() && request.session_id.as_deref() != expected_session_id {
        return Err(queue_error(
            StatusCode::NOT_FOUND,
            "Permission request not found",
            ErrorCode::ApprovalRequestNotFound,
        ));
    }
    let reviewer = ensure_queue_reviewer(headers, tenant_context, request_principal, verified)?;
    ensure_independent_permission_reviewer(&request, &reviewer)?;
    append_permission_decision_intent_audit(
        state,
        tenant_context,
        request_principal,
        &reviewer,
        &request,
        reply,
        reason,
    )
    .await
    .map_err(super::protected_audit_error_envelope)?;
    state
        .permissions
        .reply_with_provenance_for_tenant(
            tenant_context,
            expected_session_id,
            request_id,
            reply,
            Some(reviewer),
            Some(reason.to_string()),
        )
        .await
        .map_err(map_permission_reply_error)?
        .ok_or_else(|| {
            queue_error(
                StatusCode::CONFLICT,
                "Permission request is no longer pending",
                ErrorCode::ApprovalReplyInvalid,
            )
        })
}

pub(super) async fn list_permissions(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
) -> Json<Value> {
    let mut requests = state.permissions.list_for_tenant(&tenant_context).await;
    let mut rules = state
        .permissions
        .list_rules_for_tenant(&tenant_context)
        .await;
    let mut decisions = state
        .permissions
        .list_decisions_for_tenant(&tenant_context)
        .await;
    if let Some(visible_sessions) = queue_session_visibility(
        &state,
        &tenant_context,
        &request_principal,
        verified.as_deref(),
    )
    .await
    {
        let caller = request_principal
            .actor_id
            .as_deref()
            .or(tenant_context.actor_id.as_deref());
        requests.retain(|request| {
            queue_record_owned_by(
                request.requested_by.as_deref(),
                request.session_id.as_deref(),
                caller,
                &visible_sessions,
            )
        });
        rules.retain(|rule| {
            rule.session_id
                .as_deref()
                .is_some_and(|id| visible_sessions.contains(id))
        });
        decisions.retain(|decision| {
            decision
                .session_id
                .as_deref()
                .is_some_and(|id| visible_sessions.contains(id))
        });
    }
    Json(json!({
        "requests": requests,
        "rules": rules,
        "decisions": decisions
    }))
}

pub(super) async fn reply_permission(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<PermissionReplyInput>,
) -> Result<Json<Value>, QueueError> {
    if !matches!(
        input.reply.as_str(),
        "once" | "always" | "reject" | "allow" | "deny"
    ) {
        return Err(queue_error(
            StatusCode::BAD_REQUEST,
            "reply must be one of once|always|reject|allow|deny",
            ErrorCode::ApprovalReplyInvalid,
        ));
    }
    let outcome = apply_permission_reply(
        &state,
        &tenant_context,
        &request_principal,
        &headers,
        verified.as_deref(),
        None,
        &id,
        &input.reply,
        "http_permission_reply",
    )
    .await?;
    Ok(Json(json!({
        "ok": true,
        "requestID": id,
        "reply": input.reply,
        "status": "applied",
        "persistedRule": outcome.decision.standing_rule_persisted,
        "standingRuleID": outcome.decision.standing_rule_id
    })))
}

pub(super) async fn approve_tool_by_call(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path((session_id, tool_call_id)): Path<(String, String)>,
) -> Result<Json<Value>, QueueError> {
    apply_permission_reply(
        &state,
        &tenant_context,
        &request_principal,
        &headers,
        verified.as_deref(),
        Some(&session_id),
        &tool_call_id,
        "allow",
        "tool_call_approved",
    )
    .await?;
    Ok(Json(json!({"ok": true})))
}

pub(super) async fn deny_tool_by_call(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path((session_id, tool_call_id)): Path<(String, String)>,
) -> Result<Json<Value>, QueueError> {
    apply_permission_reply(
        &state,
        &tenant_context,
        &request_principal,
        &headers,
        verified.as_deref(),
        Some(&session_id),
        &tool_call_id,
        "deny",
        "tool_call_denied",
    )
    .await?;
    Ok(Json(json!({"ok": true})))
}

pub(super) async fn list_questions(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
) -> Json<Value> {
    let mut questions = state
        .storage
        .list_question_requests_for_tenant(&tenant_context)
        .await;
    if let Some(visible_sessions) = queue_session_visibility(
        &state,
        &tenant_context,
        &request_principal,
        verified.as_deref(),
    )
    .await
    {
        let caller = request_principal
            .actor_id
            .as_deref()
            .or(tenant_context.actor_id.as_deref());
        questions.retain(|question| {
            queue_record_owned_by(
                question.requested_by.as_deref(),
                Some(&question.session_id),
                caller,
                &visible_sessions,
            )
        });
    }
    Json(json!(questions))
}

fn map_question_lookup_error(error: anyhow::Error) -> QueueError {
    let message = error.to_string();
    if message.contains("EXPIRED") {
        queue_error(
            StatusCode::CONFLICT,
            "Question request has expired",
            ErrorCode::ApprovalReplyInvalid,
        )
    } else if message.contains("MISMATCH") || message.contains("UNBOUND") {
        queue_error(
            StatusCode::CONFLICT,
            "Question request action binding is invalid",
            ErrorCode::ApprovalReplyInvalid,
        )
    } else {
        queue_error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to load question request",
            ErrorCode::ApprovalPersistenceFailed,
        )
    }
}

async fn apply_question_reply(
    state: &AppState,
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
    headers: &HeaderMap,
    verified: Option<&VerifiedTenantContext>,
    expected_session_id: Option<&str>,
    question_id: &str,
    event_type: &str,
    decision: &str,
    answer_provided: bool,
) -> Result<bool, QueueError> {
    let request = state
        .storage
        .get_question_request_for_tenant(question_id, tenant_context, expected_session_id)
        .await
        .map_err(map_question_lookup_error)?
        .ok_or_else(|| {
            queue_error(
                StatusCode::NOT_FOUND,
                "Question request not found",
                ErrorCode::ApprovalRequestNotFound,
            )
        })?;
    let reviewer = ensure_queue_reviewer(headers, tenant_context, request_principal, verified)?;
    if request
        .requested_by
        .as_deref()
        .is_some_and(|requester| requester.eq_ignore_ascii_case(&reviewer))
    {
        return Err(queue_error(
            StatusCode::FORBIDDEN,
            "question responses require an independent reviewer",
            ErrorCode::TenantContextDenied,
        ));
    }
    crate::audit::append_protected_audit_event(
        state,
        event_type,
        tenant_context,
        Some(reviewer),
        json!({
            "questionID": &request.id,
            "sessionID": &request.session_id,
            "decision": decision,
            "requestedBy": &request.requested_by,
            "actionDigest": &request.action_digest,
            "expiresAtMs": request.expires_at_ms,
            "answerProvided": answer_provided,
        }),
    )
    .await
    .map_err(super::protected_audit_error_envelope)?;
    let removed = state
        .storage
        .decide_question_for_tenant(question_id, tenant_context, expected_session_id)
        .await
        .map_err(map_question_lookup_error)?;
    if removed.is_none() {
        return Err(queue_error(
            StatusCode::CONFLICT,
            "Question request is no longer pending",
            ErrorCode::ApprovalReplyInvalid,
        ));
    }
    Ok(true)
}

pub(super) async fn reply_question(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(_input): Json<QuestionReplyInput>,
) -> Result<Json<Value>, QueueError> {
    apply_question_reply(
        &state,
        &tenant_context,
        &request_principal,
        &headers,
        verified.as_deref(),
        None,
        &id,
        "question.replied",
        "answered",
        false,
    )
    .await?;
    state.event_bus.publish(EngineEvent::new(
        "question.replied",
        json!({"id": id, "ok": true}),
    ));
    Ok(Json(json!({"ok": true})))
}

pub(super) async fn reject_question(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, QueueError> {
    apply_question_reply(
        &state,
        &tenant_context,
        &request_principal,
        &headers,
        verified.as_deref(),
        None,
        &id,
        "question.rejected",
        "rejected",
        false,
    )
    .await?;
    state.event_bus.publish(EngineEvent::new(
        "question.replied",
        json!({"id": id, "ok": false}),
    ));
    Ok(Json(json!({"ok": true})))
}

pub(super) async fn answer_question(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified: Option<Extension<VerifiedTenantContext>>,
    headers: HeaderMap,
    Path((session_id, question_id)): Path<(String, String)>,
    Json(input): Json<QuestionAnswerInput>,
) -> Result<Json<Value>, QueueError> {
    apply_question_reply(
        &state,
        &tenant_context,
        &request_principal,
        &headers,
        verified.as_deref(),
        Some(&session_id),
        &question_id,
        "question.answered",
        "answered",
        input.answer.is_some(),
    )
    .await?;
    state.event_bus.publish(EngineEvent::new(
        "question.replied",
        json!({"id": question_id, "ok": true, "answer": input.answer}),
    ));
    Ok(Json(json!({"ok": true})))
}
