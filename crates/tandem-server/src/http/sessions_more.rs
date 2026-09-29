// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

// Continuation of session handlers split from sessions.rs for the file-size gate
// (same module via include!).

async fn wait_for_run_finished_event(
    state: &AppState,
    rx: &mut tokio::sync::broadcast::Receiver<EngineEvent>,
    session_id: &str,
    run_id: &str,
    max_wait: Duration,
) -> bool {
    let deadline = tokio::time::sleep(max_wait);
    tokio::pin!(deadline);

    loop {
        tokio::select! {
            _ = &mut deadline => {
                return state.run_registry.get(session_id).await.is_none();
            }
            event = rx.recv() => {
                match event {
                    Ok(event)
                        if event.event_type == "session.run.finished"
                            && event_matches_run(&event, session_id, run_id) =>
                    {
                        return true;
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if state.run_registry.get(session_id).await.is_none() {
                            return true;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        return state.run_registry.get(session_id).await.is_none();
                    }
                }
            }
        }
    }
}

pub(super) async fn fork_session(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    ensure_same_session_actor(&tenant_context, &session.tenant_context)?;
    let child = state
        .storage
        .fork_session(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(json!({"ok": true, "session": child})))
}

pub(super) async fn revert_session(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    ensure_same_session_actor(&tenant_context, &session.tenant_context)?;
    let ok = state
        .storage
        .revert_session(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok": ok})))
}

pub(super) async fn unrevert_session(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    ensure_same_session_actor(&tenant_context, &session.tenant_context)?;
    let ok = state
        .storage
        .unrevert_session(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok": ok})))
}

pub(super) async fn share_session(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    ensure_same_session_actor(&tenant_context, &session.tenant_context)?;
    let share_id = state
        .storage
        .set_shared(&id, true)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok": share_id.is_some(), "shareID": share_id})))
}

pub(super) async fn unshare_session(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    ensure_same_session_actor(&tenant_context, &session.tenant_context)?;
    let _ = state
        .storage
        .set_shared(&id, false)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok": true})))
}

pub(super) async fn summarize_session(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    ensure_same_session_actor(&tenant_context, &session.tenant_context)?;
    let total_messages = session.messages.len();
    let mut text_parts = Vec::new();
    for message in session.messages.iter().rev().take(4) {
        for part in &message.parts {
            if let MessagePart::Text { text } = part {
                text_parts.push(text.clone());
            }
        }
    }
    text_parts.reverse();
    let excerpt = text_parts.join(" ");
    let clipped = excerpt.chars().take(280).collect::<String>();
    let summary = if clipped.is_empty() {
        format!("Session with {total_messages} messages and no text parts.")
    } else {
        format!("Session with {total_messages} messages. Recent: {clipped}")
    };
    state
        .storage
        .set_summary(&id, summary.clone())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok": true, "summary": summary})))
}

pub(super) async fn session_diff(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    ensure_same_session_actor(&tenant_context, &session.tenant_context)?;
    let diff = state.storage.session_diff(&id).await;
    Ok(Json(json!(diff.unwrap_or_else(|| json!({})))))
}

pub(super) async fn session_children(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    ensure_same_session_actor(&tenant_context, &session.tenant_context)?;
    Ok(Json(json!(state.storage.children(&id).await)))
}

pub(super) async fn init_session() -> Json<Value> {
    Json(json!({"ok": true}))
}

#[derive(Debug)]
struct DirectKbTranscriptAuthorityDenied;

impl std::fmt::Display for DirectKbTranscriptAuthorityDenied {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("hosted_provider_authority_revoked")
    }
}

impl std::error::Error for DirectKbTranscriptAuthorityDenied {}

async fn finish_direct_kb_authority_denied_run(
    state: &AppState,
    session_id: &str,
    run_id: &str,
    tenant_context: &TenantContext,
) {
    const MESSAGE: &str = "Knowledgebase access was revoked.";
    let _ = state.run_registry.finish_if_match(session_id, run_id).await;
    publish_tenant_event(
        state,
        tenant_context,
        "session.error",
        json!({
            "sessionID": session_id,
            "error": {"code": "HOSTED_AUTHORITY_REVOKED", "message": MESSAGE},
        }),
    );
    publish_tenant_event(
        state,
        tenant_context,
        "session.status",
        json!({"sessionID": session_id, "status": "error"}),
    );
    publish_tenant_event(
        state,
        tenant_context,
        "session.updated",
        json!({"sessionID": session_id, "status": "error"}),
    );
    publish_tenant_event(
        state,
        tenant_context,
        "session.run.finished",
        json!({
            "sessionID": session_id,
            "runID": run_id,
            "finishedAtMs": crate::now_ms(),
            "status": "error",
            "error": MESSAGE,
            "failureCategory": "permission_denied",
        }),
    );
}

fn commit_direct_kb_message_write(
    state: &AppState,
    tenant_context: &TenantContext,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    commit: &mut dyn FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    state
        .enterprise
        .hosted_policy
        .with_current_policy(|policy| {
            super::require_hosted_permission_under_policy(
                tenant_context,
                verified_tenant_context,
                tandem_types::AccessPermission::HostedUse,
                policy,
            )
            .map_err(|_| anyhow::Error::new(DirectKbTranscriptAuthorityDenied))?;
            commit()
        })
        .map_err(|_| anyhow::Error::new(DirectKbTranscriptAuthorityDenied))?
}

#[allow(clippy::too_many_arguments)]
async fn persist_direct_kb_answer_messages(
    state: &AppState,
    session_id: &str,
    question: &str,
    tool_name: &str,
    tool_args: Value,
    tool_output: &str,
    answer: &str,
    outcome: &StrictKbGroundingOutcome,
    tenant_context: &TenantContext,
    verified_tenant_context: Option<&VerifiedTenantContext>,
) -> anyhow::Result<()> {
    if question.trim().is_empty() || answer.trim().is_empty() {
        return Ok(());
    }
    // Keep the published policy stable across both transcript writes. The
    // per-message callback checks it again after SQLite's writer wait and
    // holds the snapshot read lock through each durable commit.
    let _policy_publication = state.enterprise.hosted_policy.lock_publication().await;
    let user_message = Message::new(
        MessageRole::User,
        vec![
            MessagePart::Text {
                text: question.trim().to_string(),
            },
            MessagePart::ToolInvocation {
                tool: tool_name.to_string(),
                args: tool_args,
                result: (outcome.support != "blocked")
                    .then(|| Value::String(tool_output.to_string())),
                error: None,
            },
        ],
    );
    let commit_state = state.clone();
    let commit_tenant = tenant_context.clone();
    let commit_verified = verified_tenant_context.cloned();
    state
        .storage
        .append_message_with_commit_guard(session_id, user_message, move |commit| {
            commit_direct_kb_message_write(
                &commit_state,
                &commit_tenant,
                commit_verified.as_ref(),
                commit,
            )
        })
        .await?;
    let assistant_message = Message::new(
        MessageRole::Assistant,
        vec![MessagePart::Text {
            text: answer.trim().to_string(),
        }],
    );
    let commit_state = state.clone();
    let commit_tenant = tenant_context.clone();
    let commit_verified = verified_tenant_context.cloned();
    state
        .storage
        .append_message_with_commit_guard(session_id, assistant_message, move |commit| {
            commit_direct_kb_message_write(
                &commit_state,
                &commit_tenant,
                commit_verified.as_ref(),
                commit,
            )
        })
        .await
}

#[cfg(test)]
mod direct_kb_persistence_tests {
    use super::*;
    use tandem_types::{
        AuthorityChain, HumanActor, RequestPrincipal, TenantContextAssertionClaims,
    };

    fn outcome(support: &str) -> StrictKbGroundingOutcome {
        StrictKbGroundingOutcome {
            support: support.to_string(),
            sources: Vec::new(),
            evidence_count: 0,
        }
    }

    fn hosted_policy(
        version: u64,
        role: &str,
    ) -> tandem_enterprise_contract::hosted_policy::HostedPolicyBundle {
        serde_json::from_value(json!({
            "schema_version": 1,
            "policy_version": version,
            "organization_id": "direct-kb-org",
            "deployment_id": "direct-kb-deployment",
            "generated_at": chrono::DateTime::from_timestamp_millis(crate::now_ms() as i64).unwrap(),
            "users": [{
                "id": "admin", "email": null, "username": null, "role": role,
                "capabilities": tandem_enterprise_contract::hosted_policy::role_capabilities(role),
                "is_active": true, "email_verified": true
            }],
            "org_units": [], "org_unit_memberships": [], "deployment_grants": []
        }))
        .expect("hosted KB policy")
    }

    #[tokio::test]
    async fn direct_kb_transcript_omits_raw_result_when_blocked_but_keeps_local_success() {
        let state = crate::test_support::test_state().await;
        let session = Session::new(
            Some("direct KB persistence".to_string()),
            Some(".".to_string()),
        );
        let session_id = session.id.clone();
        let tenant = session.tenant_context.clone();
        state
            .storage
            .save_session(session)
            .await
            .expect("save session");

        persist_direct_kb_answer_messages(
            &state,
            &session_id,
            "question",
            "mcp.kb.answer_question",
            json!({"question": "question"}),
            "sensitive retrieved excerpt",
            "Knowledgebase access was revoked.",
            &outcome("blocked"),
            &tenant,
            None,
        )
        .await
        .expect("persist blocked local response without evidence");
        persist_direct_kb_answer_messages(
            &state,
            &session_id,
            "question",
            "mcp.kb.answer_question",
            json!({"question": "question"}),
            "ordinary retrieved excerpt",
            "ordinary answer",
            &outcome("supported"),
            &tenant,
            None,
        )
        .await
        .expect("persist ordinary local response");

        let persisted = state
            .storage
            .get_session(&session_id)
            .await
            .expect("session");
        assert_eq!(persisted.messages.len(), 4);
        assert!(matches!(
            &persisted.messages[0].parts[1],
            MessagePart::ToolInvocation { result: None, .. }
        ));
        assert!(matches!(
            &persisted.messages[2].parts[1],
            MessagePart::ToolInvocation { result: Some(value), .. }
                if value == &json!("ordinary retrieved excerpt")
        ));
    }

    #[tokio::test]
    async fn direct_kb_transcript_rejects_stale_hosted_authority_before_durable_write() {
        let state = crate::test_support::test_state().await;
        state
            .enterprise
            .hosted_policy
            .install_test_bundle(hosted_policy(1, "admin"))
            .expect("install hosted admin policy");
        let tenant = TenantContext::explicit_user_workspace(
            "direct-kb-org",
            "direct-kb-deployment",
            Some("direct-kb-deployment".to_string()),
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
            .expect("project hosted admin");
        let mut session = Session::new(Some("revoked KB".to_string()), Some(".".to_string()));
        session.tenant_context = tenant.clone();
        let session_id = session.id.clone();
        state
            .storage
            .save_session(session)
            .await
            .expect("save session");

        state
            .enterprise
            .hosted_policy
            .install_test_bundle(hosted_policy(2, "viewer"))
            .expect("revoke hosted use");
        let result = persist_direct_kb_answer_messages(
            &state,
            &session_id,
            "question",
            "mcp.kb.answer_question",
            json!({"question": "question"}),
            "sensitive retrieved excerpt",
            "ordinary answer",
            &outcome("supported"),
            &tenant,
            Some(&verified),
        )
        .await;
        assert!(result
            .expect_err("revoked hosted authority must deny")
            .is::<DirectKbTranscriptAuthorityDenied>());
        assert!(state
            .storage
            .get_session(&session_id)
            .await
            .expect("session")
            .messages
            .is_empty());
    }

    #[tokio::test]
    async fn direct_kb_revoked_transcript_finishes_run_without_evidence_in_events() {
        let state = crate::test_support::test_state().await;
        let session_id = "direct-kb-revoked-run";
        let run_id = "direct-kb-revoked-at-write";
        let tenant = TenantContext::default();
        state
            .run_registry
            .acquire(session_id, run_id.to_string(), None, None, None)
            .await
            .expect("acquire direct KB run");
        let mut events = state.event_bus.subscribe();

        finish_direct_kb_authority_denied_run(&state, session_id, run_id, &tenant).await;

        assert!(state.run_registry.get(session_id).await.is_none());
        let published = std::iter::from_fn(|| events.try_recv().ok()).collect::<Vec<_>>();
        assert_eq!(published.len(), 4);
        let finished = published
            .iter()
            .find(|event| event.event_type == "session.run.finished")
            .expect("run finish event");
        assert_eq!(finished.properties["status"], "error");
        assert_eq!(finished.properties["failureCategory"], "permission_denied");
        assert!(!published.iter().any(|event| {
            event
                .properties
                .to_string()
                .contains("sensitive retrieved excerpt")
        }));
    }
}
