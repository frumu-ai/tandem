use super::*;

impl Storage {
    /// Normalize once, then expose those exact values after SQLite acquires
    /// its writer. The guard invokes its continuation exactly once and returns
    /// that result. Success-event publication belongs inside the guarded
    /// continuation after a successful commit, with no fallible work afterward.
    pub async fn set_todos_with_commit_guard<G>(
        &self,
        session_id: &str,
        todos: Vec<Value>,
        guard: G,
    ) -> anyhow::Result<()>
    where
        G: FnOnce(&[Value], &mut dyn FnMut() -> anyhow::Result<()>) -> anyhow::Result<()>
            + Send
            + 'static,
    {
        let session_id = session_id.to_string();
        let todos = normalize_todo_items(todos);
        self.run_blocking(move |repository| {
            repository.set_todos_with_commit_guard(&session_id, todos, guard)
        })
        .await
    }

    /// Prepare the tenant-bound request once. Retain the question writer in
    /// the native task, including when its awaiting caller is cancelled. The
    /// callback sees the same request the continuation writes to SQLite.
    pub async fn add_question_request_with_commit_guard<G>(
        &self,
        session_id: &str,
        message_id: &str,
        questions: Vec<Value>,
        guard: G,
    ) -> anyhow::Result<QuestionRequest>
    where
        G: FnOnce(&QuestionRequest, &mut dyn FnMut() -> anyhow::Result<()>) -> anyhow::Result<()>
            + Send
            + 'static,
    {
        if questions.is_empty() {
            anyhow::bail!(
                "cannot add empty question request for session {}",
                session_id
            );
        }
        let tenant_context = self
            .get_session(session_id)
            .await
            .map(|session| session.tenant_context)
            .unwrap_or_else(TenantContext::local_implicit);
        let requested_at_ms = now_ms_u64();
        let tool = QuestionToolRef {
            call_id: format!("call-{}", Uuid::new_v4()),
            message_id: message_id.to_string(),
        };
        let digest_payload = json!({
            "tenant": &tenant_context,
            "sessionID": session_id,
            "questions": &questions,
            "tool": &tool,
        });
        let request = QuestionRequest {
            id: format!("q-{}", Uuid::new_v4()),
            requested_by: tenant_context.actor_id.clone(),
            tenant_context,
            action_digest: format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&digest_payload).unwrap_or_default())
            ),
            expires_at_ms: requested_at_ms.saturating_add(QUESTION_REQUEST_TTL_MS),
            session_id: session_id.to_string(),
            questions,
            tool: Some(tool),
        };
        let request_for_store = request.clone();
        let question_writer = self.question_write_lock.clone().lock_owned().await;
        self.run_blocking(move |repository| {
            let _question_writer = question_writer;
            repository.add_question_with_commit_guard(&request_for_store, guard)
        })
        .await?;
        Ok(request)
    }
}
