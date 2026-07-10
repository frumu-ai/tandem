use super::*;

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

pub(super) async fn list_permissions(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "requests": state.permissions.list().await,
        "rules": state.permissions.list_rules().await,
        "decisions": state.permissions.list_decisions().await
    }))
}

pub(super) async fn reply_permission(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    Path(id): Path<String>,
    Json(input): Json<PermissionReplyInput>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let accepted = matches!(
        input.reply.as_str(),
        "once" | "always" | "reject" | "allow" | "deny"
    );
    if !accepted {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorEnvelope::new(
                "reply must be one of once|always|reject|allow|deny",
                ErrorCode::ApprovalReplyInvalid,
            )),
        ));
    }
    let outcome = state
        .permissions
        .reply_with_provenance(
            &id,
            &input.reply,
            permission_actor(&request_principal),
            Some("http_permission_reply".to_string()),
        )
        .await;
    let Some(outcome) = outcome else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorEnvelope::new(
                "Permission request not found",
                ErrorCode::ApprovalRequestNotFound,
            )),
        ));
    };
    append_permission_decision_audit(&state, &tenant_context, &request_principal, &outcome)
        .await
        .map_err(super::protected_audit_error_envelope)?;
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
    Path((session_id, tool_call_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let outcome = state
        .permissions
        .reply_with_provenance(
            &tool_call_id,
            "allow",
            permission_actor(&request_principal),
            Some("tool_call_approved".to_string()),
        )
        .await;
    let Some(outcome) = outcome else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorEnvelope::new(
                "Permission request not found",
                ErrorCode::ApprovalRequestNotFound,
            )),
        ));
    };
    append_permission_decision_audit(&state, &tenant_context, &request_principal, &outcome)
        .await
        .map_err(super::protected_audit_error_envelope)?;
    crate::audit::append_protected_audit_event(
        &state,
        "approval.granted",
        &tandem_types::TenantContext::local_implicit(),
        permission_actor(&request_principal),
        json!({
            "sessionID": session_id,
            "toolCallID": tool_call_id,
            "decision": "allow",
        }),
    )
    .await
    .map_err(super::protected_audit_error_envelope)?;
    Ok(Json(json!({"ok": true})))
}

pub(super) async fn deny_tool_by_call(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    Path((session_id, tool_call_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let outcome = state
        .permissions
        .reply_with_provenance(
            &tool_call_id,
            "deny",
            permission_actor(&request_principal),
            Some("tool_call_denied".to_string()),
        )
        .await;
    let Some(outcome) = outcome else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorEnvelope::new(
                "Permission request not found",
                ErrorCode::ApprovalRequestNotFound,
            )),
        ));
    };
    append_permission_decision_audit(&state, &tenant_context, &request_principal, &outcome)
        .await
        .map_err(super::protected_audit_error_envelope)?;
    crate::audit::append_protected_audit_event(
        &state,
        "approval.denied",
        &tandem_types::TenantContext::local_implicit(),
        permission_actor(&request_principal),
        json!({
            "sessionID": session_id,
            "toolCallID": tool_call_id,
            "decision": "deny",
        }),
    )
    .await
    .map_err(super::protected_audit_error_envelope)?;
    Ok(Json(json!({"ok": true})))
}

fn permission_actor(request_principal: &RequestPrincipal) -> Option<String> {
    request_principal
        .actor_id
        .clone()
        .or_else(|| Some(request_principal.source.clone()))
}

async fn append_permission_decision_audit(
    state: &AppState,
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
    outcome: &tandem_core::PermissionReplyOutcome,
) -> anyhow::Result<()> {
    crate::audit::append_protected_audit_event(
        state,
        "permission.decision",
        tenant_context,
        permission_actor(request_principal),
        json!({
            "requestID": &outcome.request.id,
            "sessionID": &outcome.request.session_id,
            "permission": &outcome.request.permission,
            "pattern": &outcome.request.pattern,
            "tool": &outcome.request.tool,
            "decision": &outcome.decision.decision,
            "decidedAtMs": outcome.decision.decided_at_ms,
            "decidedBy": &outcome.decision.decided_by,
            "reason": &outcome.decision.reason,
            "standingRuleID": &outcome.decision.standing_rule_id,
            "standingRulePersisted": outcome.decision.standing_rule_persisted,
            "principal": {
                "actorID": &request_principal.actor_id,
                "source": &request_principal.source,
            },
            "rule": outcome.rule.as_ref().map(|rule| json!({
                "id": &rule.id,
                "permission": &rule.permission,
                "pattern": &rule.pattern,
                "action": &rule.action,
                "createdAtMs": &rule.created_at_ms,
                "createdBy": &rule.created_by,
                "sourceRequestID": &rule.source_request_id,
                "provenance": &rule.provenance,
            })),
        }),
    )
    .await
}

pub(super) async fn list_questions(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.storage.list_question_requests().await))
}

pub(super) async fn reply_question(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    Path(id): Path<String>,
    Json(_input): Json<QuestionReplyInput>,
) -> Result<Json<Value>, StatusCode> {
    let ok = state
        .storage
        .reply_question(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if ok {
        crate::audit::append_protected_audit_event(
            &state,
            "question.replied",
            &tenant_context,
            permission_actor(&request_principal),
            json!({"questionID": id, "decision": "answered"}),
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        state.event_bus.publish(EngineEvent::new(
            "question.replied",
            json!({"id": id, "ok": true}),
        ));
    }
    Ok(Json(json!({"ok": ok})))
}

pub(super) async fn reject_question(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let ok = state
        .storage
        .reject_question(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if ok {
        crate::audit::append_protected_audit_event(
            &state,
            "question.rejected",
            &tenant_context,
            permission_actor(&request_principal),
            json!({"questionID": id, "decision": "rejected"}),
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        state.event_bus.publish(EngineEvent::new(
            "question.replied",
            json!({"id": id, "ok": false}),
        ));
    }
    Ok(Json(json!({"ok": ok})))
}

pub(super) async fn answer_question(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    Path((_session_id, question_id)): Path<(String, String)>,
    Json(input): Json<QuestionAnswerInput>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let ok = state
        .storage
        .reply_question(&question_id)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorEnvelope::new(
                    "Failed to answer question",
                    ErrorCode::ApprovalPersistenceFailed,
                )),
            )
        })?;
    if !ok {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorEnvelope::new(
                "Question request not found",
                ErrorCode::ApprovalRequestNotFound,
            )),
        ));
    }
    crate::audit::append_protected_audit_event(
        &state,
        "question.answered",
        &tenant_context,
        permission_actor(&request_principal),
        json!({
            "questionID": question_id,
            "decision": "answered",
            "answerProvided": input.answer.is_some(),
        }),
    )
    .await
    .map_err(super::protected_audit_error_envelope)?;
    state.event_bus.publish(EngineEvent::new(
        "question.replied",
        json!({"id": question_id, "ok": true, "answer": input.answer}),
    ));
    Ok(Json(json!({"ok": true})))
}
