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
        "rules": state.permissions.list_rules().await
    }))
}

pub(super) async fn reply_permission(
    State(state): State<AppState>,
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
    let ok = state.permissions.reply(&id, &input.reply).await;
    if !ok {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorEnvelope::new(
                "Permission request not found",
                ErrorCode::ApprovalRequestNotFound,
            )),
        ));
    }
    Ok(Json(json!({
        "ok": true,
        "requestID": id,
        "reply": input.reply,
        "status": "applied",
        "persistedRule": matches!(input.reply.as_str(), "always" | "allow")
    })))
}

pub(super) async fn approve_tool_by_call(
    State(state): State<AppState>,
    Path((session_id, tool_call_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let ok = state.permissions.reply(&tool_call_id, "allow").await;
    if !ok {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorEnvelope::new(
                "Permission request not found",
                ErrorCode::ApprovalRequestNotFound,
            )),
        ));
    }
    let _ = crate::audit::append_protected_audit_event(
        &state,
        "approval.granted",
        &tandem_types::TenantContext::local_implicit(),
        None,
        json!({
            "sessionID": session_id,
            "toolCallID": tool_call_id,
            "decision": "allow",
        }),
    )
    .await;
    Ok(Json(json!({"ok": true})))
}

pub(super) async fn deny_tool_by_call(
    State(state): State<AppState>,
    Path((session_id, tool_call_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let ok = state.permissions.reply(&tool_call_id, "deny").await;
    if !ok {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorEnvelope::new(
                "Permission request not found",
                ErrorCode::ApprovalRequestNotFound,
            )),
        ));
    }
    let _ = crate::audit::append_protected_audit_event(
        &state,
        "approval.denied",
        &tandem_types::TenantContext::local_implicit(),
        None,
        json!({
            "sessionID": session_id,
            "toolCallID": tool_call_id,
            "decision": "deny",
        }),
    )
    .await;
    Ok(Json(json!({"ok": true})))
}

pub(super) async fn list_questions(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.storage.list_question_requests().await))
}

pub(super) async fn reply_question(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(_input): Json<QuestionReplyInput>,
) -> Result<Json<Value>, StatusCode> {
    let ok = state
        .storage
        .reply_question(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if ok {
        state.event_bus.publish(EngineEvent::new(
            "question.replied",
            json!({"id": id, "ok": true}),
        ));
    }
    Ok(Json(json!({"ok": ok})))
}

pub(super) async fn reject_question(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let ok = state
        .storage
        .reject_question(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if ok {
        state.event_bus.publish(EngineEvent::new(
            "question.replied",
            json!({"id": id, "ok": false}),
        ));
    }
    Ok(Json(json!({"ok": ok})))
}

pub(super) async fn answer_question(
    State(state): State<AppState>,
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
    if ok {
        state.event_bus.publish(EngineEvent::new(
            "question.replied",
            json!({"id": question_id, "ok": true, "answer": input.answer}),
        ));
    }
    Ok(Json(json!({"ok": true})))
}
