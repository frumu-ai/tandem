use std::sync::Arc;

use serde_json::{json, Value};
use tandem_types::{EngineEvent, VerifiedTenantContext};
use tandem_wire::WireMessagePart;

use crate::{EventBus, Storage};

use super::{extract_todo_candidates_from_text, EngineLoop, ToolPolicyHook};

#[derive(Clone, Default)]
pub(super) struct PlanFallbackAuthority {
    hook: Option<Arc<dyn ToolPolicyHook>>,
    original_verified: Option<VerifiedTenantContext>,
}

impl PlanFallbackAuthority {
    fn commit_and_publish(
        &self,
        commit: &mut dyn FnMut() -> anyhow::Result<()>,
        bus: &EventBus,
        events: &[EngineEvent],
    ) -> anyhow::Result<()> {
        let mut continuation = || {
            commit()?;
            for event in events {
                bus.publish(event.clone());
            }
            Ok(())
        };
        if let Some(hook) = &self.hook {
            hook.with_session_commit_authority(self.original_verified.clone(), &mut continuation)
        } else {
            continuation()
        }
    }
}

impl EngineLoop {
    pub(super) async fn emit_plan_fallbacks(
        &self,
        session_id: &str,
        message_id: &str,
        run_id: Option<&str>,
        completion: &str,
        original_verified: Option<VerifiedTenantContext>,
        question_tool_used: bool,
    ) -> anyhow::Result<()> {
        // Retain the admitted identity and own the hook across the native writer wait.
        let authority = PlanFallbackAuthority {
            hook: self.tool_policy_hook.read().await.clone(),
            original_verified,
        };
        emit_plan_todo_fallback(
            self.storage.clone(),
            &self.event_bus,
            session_id,
            message_id,
            run_id,
            completion,
            authority.clone(),
        )
        .await?;
        let todos = self.storage.get_todos(session_id).await;
        if todos.is_empty() && !question_tool_used {
            emit_plan_question_fallback(
                self.storage.clone(),
                &self.event_bus,
                session_id,
                message_id,
                run_id,
                completion,
                authority,
            )
            .await?;
        }
        Ok(())
    }
}

fn event_properties(session_id: &str, run_id: Option<&str>, mut properties: Value) -> Value {
    let properties_object = properties
        .as_object_mut()
        .expect("plan fallback event properties must be an object");
    properties_object.insert("sessionID".to_string(), json!(session_id));
    if let Some(run_id) = run_id {
        properties_object.insert("runID".to_string(), json!(run_id));
    }
    properties
}

pub(super) async fn emit_plan_todo_fallback(
    storage: Arc<Storage>,
    bus: &EventBus,
    session_id: &str,
    message_id: &str,
    run_id: Option<&str>,
    completion: &str,
    authority: PlanFallbackAuthority,
) -> anyhow::Result<()> {
    let todos = extract_todo_candidates_from_text(completion);
    if todos.is_empty() {
        return Ok(());
    }
    let input = json!({"todos": todos});
    let invoke_part =
        WireMessagePart::tool_invocation(session_id, message_id, "todo_write", input.clone());
    let call_id = invoke_part.id.clone();
    let session = session_id.to_string();
    let message = message_id.to_string();
    let run = run_id.map(ToString::to_string);
    let bus = bus.clone();
    storage
        .set_todos_with_commit_guard(session_id, todos, move |canonical, commit| {
            let mut result_part = WireMessagePart::tool_result(
                &session,
                &message,
                "todo_write",
                Some(input),
                json!({"todos": canonical}),
            );
            result_part.id = call_id;
            let events = [
                EngineEvent::new(
                    "message.part.updated",
                    event_properties(&session, run.as_deref(), json!({"part": invoke_part})),
                ),
                EngineEvent::new(
                    "message.part.updated",
                    event_properties(&session, run.as_deref(), json!({"part": result_part})),
                ),
                EngineEvent::new(
                    "todo.updated",
                    event_properties(&session, run.as_deref(), json!({"todos": canonical})),
                ),
            ];
            authority.commit_and_publish(commit, &bus, &events)
        })
        .await
}

pub(super) async fn emit_plan_question_fallback(
    storage: Arc<Storage>,
    bus: &EventBus,
    session_id: &str,
    message_id: &str,
    run_id: Option<&str>,
    completion: &str,
    authority: PlanFallbackAuthority,
) -> anyhow::Result<()> {
    let trimmed = completion.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    let hints = extract_todo_candidates_from_text(trimmed)
        .into_iter()
        .take(6)
        .filter_map(|value| {
            value
                .get("content")
                .and_then(Value::as_str)
                .map(ToString::to_string)
        })
        .collect::<Vec<_>>();
    let mut options = hints
        .iter()
        .map(|label| json!({"label": label, "description": "Use this as a starting task"}))
        .collect::<Vec<_>>();
    if options.is_empty() {
        options = vec![
            json!({"label":"Define scope", "description":"Clarify the intended outcome"}),
            json!({"label":"Provide constraints", "description":"Budget, timeline, and constraints"}),
            json!({"label":"Draft a starter list", "description":"Generate a first-pass task list"}),
        ];
    }
    let questions = vec![json!({
        "header":"Planning Input",
        "question":"I couldn't produce a concrete task list yet. Which tasks should I include first?",
        "options": options,
        "multiple": true,
        "custom": true
    })];
    let session = session_id.to_string();
    let message = message_id.to_string();
    let run = run_id.map(ToString::to_string);
    let bus = bus.clone();
    storage
        .add_question_request_with_commit_guard(session_id, message_id, questions, move |request, commit| {
            // Only the canonical persisted request supplies the ID and tool binding.
            let event = EngineEvent::new("question.asked", event_properties(&session, run.as_deref(), json!({
                "id": request.id,
                "messageID": message,
                "questions": request.questions,
                "tool": request.tool.as_ref().map(|tool| json!({"callID": tool.call_id, "messageID": tool.message_id}))
            })));
            authority.commit_and_publish(commit, &bus, &[event])
        })
        .await?;
    Ok(())
}
